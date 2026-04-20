//! Aider Polyglot adapter — multi-language code-editing benchmark for Mason.
//!
//! The Aider Polyglot benchmark exercises code editing across multiple
//! programming languages using exercises sourced from exercism.io. Rather
//! than generating code from scratch, the model receives a stub implementation
//! and must fill it in to pass pre-written tests.
//!
//! **How it works:**
//! 1. Load exercises from a JSONL file (one object per line).
//! 2. For each exercise, write the stub and test files to a temp directory.
//! 3. Ask Mason (or the raw LLM in direct mode) to implement the stub.
//! 4. Write Mason's output back to the stub file.
//! 5. Run the exercise's test command in the temp directory.
//! 6. Record pass/fail; report pass rate overall and broken down by language.
//!
//! **Warning:** this adapter executes test commands as subprocesses. The test
//! command is taken from the dataset, so use only trusted local datasets.
//!
//! **Dataset format** (one JSON object per line):
//! ```json
//! {
//!   "exercise_id":    "two-fer",
//!   "language":       "python",
//!   "instructions":   "Create a phrase of the form 'One for X, one for me.'",
//!   "stub_filename":  "two_fer.py",
//!   "stub_content":   "def two_fer(name='you'):\n    pass\n",
//!   "test_filename":  "two_fer_test.py",
//!   "test_content":   "import unittest\n...",
//!   "test_command":   ["python3", "-m", "pytest", "two_fer_test.py", "-x", "-q"]
//! }
//! ```
//! Additional files (helpers, fixtures, configs) can be supplied in an optional
//! `extra_files` map from filename to content.
//!
//! **Env / manifest overrides:**
//! - `POLYGLOT_DATASET`       — path to JSONL dataset file (required, or smoke fixture is used)
//! - `POLYGLOT_OUTPUT`        — output directory (defaults to `factory/artifacts/benchmarks/aider-polyglot/`)
//! - `POLYGLOT_MODE`          — `harkonnen` (default, Mason via agent dispatch) or `direct` (raw LLM)
//! - `POLYGLOT_LIMIT`         — max exercises to evaluate
//! - `POLYGLOT_PROVIDER`      — LLM provider for `direct` mode
//! - `POLYGLOT_MIN_PASS_RATE` — minimum pass rate to not fail the suite (0.0 – 1.0)
//! - `POLYGLOT_TIMEOUT_SECS`  — per-exercise test timeout in seconds (default 30)

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::{
    benchmark::BenchmarkStatus,
    chat,
    config::Paths,
    llm::{self, LlmRequest, Message},
};

// ── Constants ─────────────────────────────────────────────────────────────────

const DEFAULT_AGENT: &str = "mason";
const DEFAULT_TIMEOUT_SECS: u64 = 30;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolyglotMode {
    Harkonnen,
    Direct,
}

impl PolyglotMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "mason" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => anyhow::bail!("unsupported POLYGLOT_MODE: {}", other),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Harkonnen => "harkonnen",
            Self::Direct => "direct",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolyglotRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: PolyglotMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub min_pass_rate: Option<f64>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolyglotRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: PolyglotMode,
    pub provider_label: String,
    pub metrics: PolyglotMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum PolyglotSuiteOutcome {
    Completed(PolyglotRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PolyglotMetrics {
    pub total_exercises: usize,
    pub passed_exercises: usize,
    pub pass_rate: f64,
    pub by_language: BTreeMap<String, PolyglotLanguageMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PolyglotLanguageMetrics {
    pub total: usize,
    pub passed: usize,
    pub pass_rate: f64,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct PolyglotExercise {
    exercise_id: String,
    language: String,
    #[serde(default)]
    instructions: String,
    stub_filename: String,
    stub_content: String,
    test_filename: String,
    test_content: String,
    test_command: Vec<String>,
    #[serde(default)]
    extra_files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct PolyglotExerciseResult {
    exercise_id: String,
    language: String,
    passed: bool,
    exit_code: Option<i32>,
    test_output_excerpt: String,
    implementation_excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
struct PolyglotSummary {
    dataset_path: String,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: PolyglotMetrics,
    results: Vec<PolyglotExerciseResult>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<PolyglotSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(p) => p,
        None => {
            return Ok(PolyglotSuiteOutcome::Skipped(
                "Aider Polyglot dataset not found. Set POLYGLOT_DATASET to a local JSONL file, \
                or place a fixture at factory/benchmarks/fixtures/aider-polyglot-smoke.jsonl."
                    .to_string(),
            ));
        }
    };

    let mode = PolyglotMode::parse(get_override(overrides, "POLYGLOT_MODE"))?;
    let agent =
        get_override(overrides, "POLYGLOT_AGENT").unwrap_or_else(|| DEFAULT_AGENT.to_string());
    let direct_provider =
        get_override(overrides, "POLYGLOT_PROVIDER").unwrap_or_else(|| "anthropic".to_string());
    let limit: Option<usize> =
        get_override(overrides, "POLYGLOT_LIMIT").and_then(|v| v.parse().ok());
    let min_pass_rate: Option<f64> =
        get_override(overrides, "POLYGLOT_MIN_PASS_RATE").and_then(|v| v.parse().ok());
    let timeout_secs: u64 = get_override(overrides, "POLYGLOT_TIMEOUT_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let output_dir = get_override(overrides, "POLYGLOT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            paths.artifacts.join("benchmarks").join("aider-polyglot")
        });

    let config = PolyglotRunConfig {
        dataset_path,
        output_dir,
        mode,
        agent,
        direct_provider,
        limit,
        min_pass_rate,
        timeout_secs,
    };

    run(paths, &config).await.map(PolyglotSuiteOutcome::Completed)
}

pub async fn run(paths: &Paths, config: &PolyglotRunConfig) -> Result<PolyglotRunOutput> {
    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .context("creating Aider Polyglot output dir")?;

    let exercises = load_exercises(&config.dataset_path).await?;
    let exercises: Vec<_> = if let Some(limit) = config.limit {
        exercises.into_iter().take(limit).collect()
    } else {
        exercises
    };

    let provider_label = format!("{}-{}", config.mode.as_str(), &config.agent);
    let mut results: Vec<PolyglotExerciseResult> = Vec::with_capacity(exercises.len());
    let mut by_language: BTreeMap<String, PolyglotLanguageMetrics> = BTreeMap::new();
    let mut total_passed = 0usize;

    for exercise in &exercises {
        let (passed, exit_code, test_output) = run_exercise(paths, config, exercise).await;

        if passed {
            total_passed += 1;
        }

        let lang_entry = by_language
            .entry(exercise.language.clone())
            .or_default();
        lang_entry.total += 1;
        if passed {
            lang_entry.passed += 1;
        }
        lang_entry.pass_rate = if lang_entry.total > 0 {
            lang_entry.passed as f64 / lang_entry.total as f64
        } else {
            0.0
        };

        results.push(PolyglotExerciseResult {
            exercise_id: exercise.exercise_id.clone(),
            language: exercise.language.clone(),
            passed,
            exit_code,
            test_output_excerpt: excerpt(&test_output, 300),
            implementation_excerpt: excerpt(&exercise.stub_content, 150),
        });
    }

    let total = exercises.len();
    let pass_rate = if total > 0 {
        total_passed as f64 / total as f64
    } else {
        0.0
    };

    let metrics = PolyglotMetrics {
        total_exercises: total,
        passed_exercises: total_passed,
        pass_rate,
        by_language,
    };

    let threshold_failure = config.min_pass_rate.and_then(|min| {
        if pass_rate < min {
            Some(format!(
                "Aider Polyglot pass rate {:.3} below threshold {:.3}",
                pass_rate, min
            ))
        } else {
            None
        }
    });

    let summary = PolyglotSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label.clone(),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        results,
    };

    let summary_path = config.output_dir.join("polyglot_summary.json");
    tokio::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).context("serializing Polyglot summary")?,
    )
    .await
    .context("writing Polyglot summary")?;

    let markdown = render_markdown(&summary);
    let markdown_path = config.output_dir.join("polyglot_report.md");
    tokio::fs::write(&markdown_path, &markdown)
        .await
        .context("writing Polyglot markdown report")?;

    Ok(PolyglotRunOutput {
        output_dir: config.output_dir.clone(),
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label,
        metrics,
        threshold_failure,
    })
}

// ── Exercise execution ────────────────────────────────────────────────────────

async fn run_exercise(
    paths: &Paths,
    config: &PolyglotRunConfig,
    exercise: &PolyglotExercise,
) -> (bool, Option<i32>, String) {
    let work_dir = match make_work_dir(exercise).await {
        Some(d) => d,
        None => return (false, None, "failed to create work directory".to_string()),
    };

    // Write test file and any extras first.
    let test_path = work_dir.join(&exercise.test_filename);
    if tokio::fs::write(&test_path, &exercise.test_content).await.is_err() {
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        return (false, None, "failed to write test file".to_string());
    }
    for (filename, content) in &exercise.extra_files {
        let file_path = work_dir.join(filename);
        let _ = tokio::fs::write(&file_path, content).await;
    }

    // Ask Mason to implement the stub.
    let implementation = generate_implementation(paths, config, exercise).await;

    // Write the implemented stub.
    let stub_path = work_dir.join(&exercise.stub_filename);
    if tokio::fs::write(&stub_path, &implementation).await.is_err() {
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        return (false, None, "failed to write stub file".to_string());
    }

    // Run the test command.
    let (passed, exit_code, output) =
        run_test_command(&exercise.test_command, &work_dir, config.timeout_secs).await;

    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    (passed, exit_code, output)
}

async fn generate_implementation(
    paths: &Paths,
    config: &PolyglotRunConfig,
    exercise: &PolyglotExercise,
) -> String {
    let prompt = build_implementation_prompt(exercise);
    let raw = match config.mode {
        PolyglotMode::Harkonnen => {
            chat::complete_agent_reply(&config.agent, &prompt, &[], None, paths)
                .await
                .unwrap_or_default()
        }
        PolyglotMode::Direct => generate_direct(paths, &config.direct_provider, exercise).await,
    };
    extract_implementation(&raw, &exercise.language)
}

async fn generate_direct(
    paths: &Paths,
    provider: &str,
    exercise: &PolyglotExercise,
) -> String {
    let system = format!(
        "You are an expert {} programmer. Implement the given stub to pass the tests. \
        Return only the complete implementation file contents with no explanation or markdown fences.",
        exercise.language
    );

    let Some(llm) = llm::build_provider("benchmark", provider, &paths.setup) else {
        return exercise.stub_content.clone();
    };

    let request = LlmRequest {
        messages: vec![
            Message::system(&system),
            Message::user(&build_implementation_prompt(exercise)),
        ],
        max_tokens: 2048,
        temperature: 0.0,
    };

    llm.complete(request)
        .await
        .map(|r| r.content)
        .unwrap_or_else(|_| exercise.stub_content.clone())
}

fn build_implementation_prompt(exercise: &PolyglotExercise) -> String {
    let lang = &exercise.language;
    let mut prompt = format!(
        "Implement the following {} exercise to pass the tests.\n\n",
        lang
    );
    if !exercise.instructions.is_empty() {
        prompt.push_str(&format!("Instructions:\n{}\n\n", exercise.instructions));
    }
    prompt.push_str(&format!(
        "Stub file ({}):\n```{}\n{}\n```\n\n",
        exercise.stub_filename, lang, exercise.stub_content
    ));
    prompt.push_str(&format!(
        "Test file ({}):\n```{}\n{}\n```\n\n",
        exercise.test_filename, lang, exercise.test_content
    ));
    prompt.push_str(&format!(
        "Return only the complete updated {} file contents. \
        No explanation, no markdown fences, no extra commentary.",
        exercise.stub_filename
    ));
    prompt
}

// ── Test runner ───────────────────────────────────────────────────────────────

async fn run_test_command(
    command: &[String],
    cwd: &Path,
    timeout_secs: u64,
) -> (bool, Option<i32>, String) {
    let (program, args) = match command.split_first() {
        Some(pair) => pair,
        None => return (false, None, "empty test command".to_string()),
    };

    let run = async {
        Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
            .ok()
    };

    match timeout(Duration::from_secs(timeout_secs), run).await {
        Ok(Some(output)) => {
            let passed = output.status.success();
            let exit_code = output.status.code();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{}\n{}", stdout, stderr)
            };
            (passed, exit_code, combined)
        }
        Ok(None) => (false, None, "test command failed to start".to_string()),
        Err(_) => (false, None, format!("timed out after {}s", timeout_secs)),
    }
}

// ── Code extraction ───────────────────────────────────────────────────────────

fn extract_implementation(raw: &str, language: &str) -> String {
    let trimmed = raw.trim();

    // Try to strip a fenced code block matching the language.
    let fence_variants = [
        format!("```{}", language),
        format!("```{}", language.to_ascii_lowercase()),
        "```".to_string(),
    ];
    for fence in &fence_variants {
        if let Some(inner) = trimmed.strip_prefix(fence.as_str()) {
            if let Some(end) = inner.rfind("```") {
                return inner[..end].trim().to_string();
            }
        }
    }

    trimmed.to_string()
}

// ── Dataset loading ───────────────────────────────────────────────────────────

async fn load_exercises(path: &Path) -> Result<Vec<PolyglotExercise>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading Polyglot dataset from {}", path.display()))?;

    let mut exercises = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let ex: PolyglotExercise = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing Polyglot exercise at line {} in {}",
                line_no + 1,
                path.display()
            )
        })?;
        exercises.push(ex);
    }
    Ok(exercises)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resolve_dataset_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(p) = get_override(overrides, "POLYGLOT_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(p) = env::var("POLYGLOT_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    let candidates = [
        paths
            .root
            .join("factory")
            .join("benchmarks")
            .join("fixtures")
            .join("aider-polyglot-smoke.jsonl"),
        paths
            .root
            .join("factory")
            .join("benchmarks")
            .join("fixtures")
            .join("aider-polyglot.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("aider-polyglot.jsonl"),
        paths
            .artifacts
            .join("benchmarks")
            .join("fixtures")
            .join("aider-polyglot.jsonl"),
    ];

    for p in &candidates {
        if p.exists() {
            return Some(p.clone());
        }
    }

    None
}

fn get_override(overrides: &BTreeMap<String, String>, key: &str) -> Option<String> {
    overrides.get(key).cloned().or_else(|| env::var(key).ok())
}

async fn make_work_dir(exercise: &PolyglotExercise) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "polyglot_{}_{}",
        exercise.language,
        uuid_fragment()
    ));
    tokio::fs::create_dir_all(&dir).await.ok()?;
    Some(dir)
}

fn uuid_fragment() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:08x}", nanos)
}

fn excerpt(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..max])
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_markdown(summary: &PolyglotSummary) -> String {
    let mut out = format!(
        "# Aider Polyglot Benchmark Report\n\n\
        **Mode:** {}  \n\
        **Provider:** {}  \n\
        **Exercises:** {}  \n\
        **Pass Rate:** {:.1}% ({}/{})  \n\
        **Generated:** {}  \n\n",
        summary.mode,
        summary.provider_label,
        summary.metrics.total_exercises,
        summary.metrics.pass_rate * 100.0,
        summary.metrics.passed_exercises,
        summary.metrics.total_exercises,
        summary.generated_at,
    );

    out.push_str("## Results by Language\n\n");
    out.push_str("| Language | Total | Passed | Pass Rate |\n");
    out.push_str("|----------|-------|--------|-----------|\n");
    for (lang, m) in &summary.metrics.by_language {
        out.push_str(&format!(
            "| {} | {} | {} | {:.1}% |\n",
            lang, m.total, m.passed, m.pass_rate * 100.0
        ));
    }

    out.push_str("\n## Exercise Results\n\n");
    out.push_str("| Exercise | Language | Passed | Exit Code |\n");
    out.push_str("|----------|----------|--------|-----------|\n");
    for r in &summary.results {
        let mark = if r.passed { "✓" } else { "✗" };
        let code = r
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.exercise_id, r.language, mark, code
        ));
    }

    out.push_str("\n## Failed Exercise Details\n\n");
    for r in summary.results.iter().filter(|r| !r.passed) {
        out.push_str(&format!(
            "### {} ({})\n\nTest output:\n```\n{}\n```\n\n",
            r.exercise_id, r.language, r.test_output_excerpt
        ));
    }

    out
}

// ── BenchmarkStatus helpers ───────────────────────────────────────────────────

pub fn status_for_output(output: &PolyglotRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &PolyglotRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

pub fn render_step_stdout(output: &PolyglotRunOutput) -> String {
    let by_lang: Vec<String> = output
        .metrics
        .by_language
        .iter()
        .map(|(lang, m)| format!("{}={:.0}%", lang, m.pass_rate * 100.0))
        .collect();

    format!(
        "Aider Polyglot [{}] n={} pass={:.1}% ({}/{})  languages: {}\n",
        output.mode.as_str(),
        output.metrics.total_exercises,
        output.metrics.pass_rate * 100.0,
        output.metrics.passed_exercises,
        output.metrics.total_exercises,
        by_lang.join(" "),
    )
}
