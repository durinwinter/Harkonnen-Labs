//! LiveCodeBench adapter — competitive-programming pass@1 evaluation for Mason.
//!
//! LiveCodeBench collects problems from LeetCode, AtCoder, and Codeforces
//! released after a cutoff date, making it resistant to training-data contamination.
//! This adapter measures Mason's pass@1 rate on a local JSONL problem set.
//!
//! **How it works:**
//! 1. Load problems from a JSONL file (one object per line).
//! 2. For each problem, ask Mason (or the raw LLM in direct mode) to generate a Python solution.
//! 3. Execute the solution against the problem's `public_test_cases` using a subprocess.
//! 4. Record pass/fail per problem; report pass@1 overall and by difficulty.
//!
//! **Warning:** this adapter executes generated code in a subprocess. Use only with
//! a local benchmark dataset you trust, in a sandboxed environment if possible.
//!
//! **Dataset format** (one JSON object per line):
//! ```json
//! {
//!   "question_id":      "lc_1234",
//!   "question_title":   "Two Sum",
//!   "question_content": "Given an array of integers ...",
//!   "platform":         "leetcode",
//!   "difficulty":       "easy",
//!   "starter_code":     "",
//!   "public_test_cases": "[{\"input\":\"[2,7,11,15]\\n9\",\"output\":\"[0,1]\"}]"
//! }
//! ```
//! `public_test_cases` may be either a JSON string (the HuggingFace download format)
//! or an inline JSON array.
//!
//! **Env / manifest overrides:**
//! - `LCB_DATASET`       — path to JSONL dataset file (required, or a smoke fixture is used)
//! - `LCB_OUTPUT`        — output directory (defaults to `factory/artifacts/benchmarks/livecodebench/`)
//! - `LCB_MODE`          — `harkonnen` (default, Mason via agent dispatch) or `direct` (raw LLM)
//! - `LCB_LIMIT`         — max problems to evaluate
//! - `LCB_PROVIDER`      — LLM provider for `direct` mode
//! - `LCB_MIN_PASS_RATE` — minimum pass@1 to pass the suite (0.0 – 1.0)
//! - `LCB_TIMEOUT_SECS`  — per-test-case subprocess timeout in seconds (default 10)

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
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
const DEFAULT_TIMEOUT_SECS: u64 = 10;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LcbMode {
    Harkonnen,
    Direct,
}

impl LcbMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "mason" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => anyhow::bail!("unsupported LCB_MODE: {}", other),
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
pub struct LcbRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: LcbMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub min_pass_rate: Option<f64>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LcbRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: LcbMode,
    pub provider_label: String,
    pub metrics: LcbMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LcbSuiteOutcome {
    Completed(LcbRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LcbMetrics {
    pub total_problems: usize,
    pub passed_problems: usize,
    pub pass_at_1: f64,
    pub by_difficulty: BTreeMap<String, LcbDifficultyMetrics>,
    pub by_platform: BTreeMap<String, LcbDifficultyMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LcbDifficultyMetrics {
    pub total: usize,
    pub passed: usize,
    pub pass_rate: f64,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct LcbProblem {
    #[serde(default)]
    question_id: String,
    #[serde(default)]
    question_title: String,
    #[serde(default)]
    question_content: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    difficulty: String,
    #[serde(default)]
    starter_code: String,
    // HuggingFace stores this as a JSON string; we also accept an inline array.
    #[serde(default)]
    public_test_cases: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct LcbTestCase {
    input: String,
    output: String,
}

#[derive(Debug, Clone, Serialize)]
struct LcbProblemResult {
    question_id: String,
    title: String,
    difficulty: String,
    platform: String,
    passed: bool,
    test_cases_total: usize,
    test_cases_passed: usize,
    generated_solution_excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
struct LcbSummary {
    dataset_path: String,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: LcbMetrics,
    results: Vec<LcbProblemResult>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<LcbSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(p) => p,
        None => {
            return Ok(LcbSuiteOutcome::Skipped(
                "LiveCodeBench dataset not found. Set LCB_DATASET to a local JSONL file, \
                or place a fixture at factory/benchmarks/fixtures/livecodebench.jsonl."
                    .to_string(),
            ));
        }
    };

    let mode = LcbMode::parse(get_override(overrides, "LCB_MODE"))?;
    let agent =
        get_override(overrides, "LCB_AGENT").unwrap_or_else(|| DEFAULT_AGENT.to_string());
    let direct_provider =
        get_override(overrides, "LCB_PROVIDER").unwrap_or_else(|| "anthropic".to_string());
    let limit: Option<usize> =
        get_override(overrides, "LCB_LIMIT").and_then(|v| v.parse().ok());
    let min_pass_rate: Option<f64> =
        get_override(overrides, "LCB_MIN_PASS_RATE").and_then(|v| v.parse().ok());
    let timeout_secs: u64 = get_override(overrides, "LCB_TIMEOUT_SECS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let output_dir = get_override(overrides, "LCB_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            paths
                .artifacts
                .join("benchmarks")
                .join("livecodebench")
        });

    let config = LcbRunConfig {
        dataset_path,
        output_dir,
        mode,
        agent,
        direct_provider,
        limit,
        min_pass_rate,
        timeout_secs,
    };

    run(paths, &config).await.map(LcbSuiteOutcome::Completed)
}

pub async fn run(paths: &Paths, config: &LcbRunConfig) -> Result<LcbRunOutput> {
    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .context("creating LiveCodeBench output dir")?;

    let problems = load_problems(&config.dataset_path).await?;
    let problems: Vec<_> = if let Some(limit) = config.limit {
        problems.into_iter().take(limit).collect()
    } else {
        problems
    };

    let provider_label = format!("{}-{}", config.mode.as_str(), &config.agent);
    let mut results: Vec<LcbProblemResult> = Vec::with_capacity(problems.len());
    let mut by_difficulty: BTreeMap<String, LcbDifficultyMetrics> = BTreeMap::new();
    let mut by_platform: BTreeMap<String, LcbDifficultyMetrics> = BTreeMap::new();
    let mut total_passed = 0usize;

    for problem in &problems {
        let test_cases = parse_test_cases(&problem.public_test_cases);
        let solution = generate_solution(paths, config, problem).await;
        let (tc_passed, tc_total) = run_test_cases(&solution, &test_cases, config.timeout_secs).await;
        let passed = tc_total > 0 && tc_passed == tc_total;

        if passed {
            total_passed += 1;
        }

        let diff_key = normalise_difficulty(&problem.difficulty);
        let plat_key = normalise_platform(&problem.platform);

        let diff_entry = by_difficulty.entry(diff_key).or_default();
        diff_entry.total += 1;
        if passed {
            diff_entry.passed += 1;
        }
        diff_entry.pass_rate = if diff_entry.total > 0 {
            diff_entry.passed as f64 / diff_entry.total as f64
        } else {
            0.0
        };

        let plat_entry = by_platform.entry(plat_key).or_default();
        plat_entry.total += 1;
        if passed {
            plat_entry.passed += 1;
        }
        plat_entry.pass_rate = if plat_entry.total > 0 {
            plat_entry.passed as f64 / plat_entry.total as f64
        } else {
            0.0
        };

        results.push(LcbProblemResult {
            question_id: problem.question_id.clone(),
            title: problem.question_title.clone(),
            difficulty: problem.difficulty.clone(),
            platform: problem.platform.clone(),
            passed,
            test_cases_total: tc_total,
            test_cases_passed: tc_passed,
            generated_solution_excerpt: excerpt(&solution, 200),
        });
    }

    let total = problems.len();
    let pass_at_1 = if total > 0 {
        total_passed as f64 / total as f64
    } else {
        0.0
    };

    let metrics = LcbMetrics {
        total_problems: total,
        passed_problems: total_passed,
        pass_at_1,
        by_difficulty,
        by_platform,
    };

    let threshold_failure = config.min_pass_rate.and_then(|min| {
        if pass_at_1 < min {
            Some(format!(
                "LiveCodeBench pass@1 {:.3} below threshold {:.3}",
                pass_at_1, min
            ))
        } else {
            None
        }
    });

    let summary = LcbSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label.clone(),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        results,
    };

    let summary_path = config.output_dir.join("lcb_summary.json");
    tokio::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).context("serializing LCB summary")?,
    )
    .await
    .context("writing LCB summary")?;

    let markdown = render_markdown(&summary);
    let markdown_path = config.output_dir.join("lcb_report.md");
    tokio::fs::write(&markdown_path, &markdown)
        .await
        .context("writing LCB markdown report")?;

    Ok(LcbRunOutput {
        output_dir: config.output_dir.clone(),
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label,
        metrics,
        threshold_failure,
    })
}

// ── Solution generation ───────────────────────────────────────────────────────

async fn generate_solution(paths: &Paths, config: &LcbRunConfig, problem: &LcbProblem) -> String {
    let prompt = build_generation_prompt(problem);
    let raw = match config.mode {
        LcbMode::Harkonnen => {
            chat::complete_agent_reply(&config.agent, &prompt, &[], None, paths)
                .await
                .unwrap_or_default()
        }
        LcbMode::Direct => generate_direct(paths, &config.direct_provider, problem).await,
    };
    extract_python_code(&raw)
}

async fn generate_direct(paths: &Paths, provider: &str, problem: &LcbProblem) -> String {
    let system = "You are a competitive programming expert. \
        Generate a complete, correct Python solution that reads from stdin and writes to stdout. \
        Return only the Python code with no explanation or markdown fences.";

    let Some(llm) = llm::build_provider("benchmark", provider, &paths.setup) else {
        return String::new();
    };

    let request = LlmRequest {
        messages: vec![
            Message::system(system),
            Message::user(&build_generation_prompt(problem)),
        ],
        max_tokens: 2048,
        temperature: 0.0,
    };

    llm.complete(request)
        .await
        .map(|r| r.content)
        .unwrap_or_default()
}

fn build_generation_prompt(problem: &LcbProblem) -> String {
    let mut prompt = format!(
        "Solve the following competitive programming problem.\n\n\
        Platform: {}\nDifficulty: {}\nTitle: {}\n\n\
        Problem:\n{}\n\n",
        problem.platform, problem.difficulty, problem.question_title, problem.question_content
    );
    if !problem.starter_code.is_empty() {
        prompt.push_str(&format!("Starter code:\n```python\n{}\n```\n\n", problem.starter_code));
    }
    prompt.push_str(
        "Write a complete Python solution that reads from stdin and writes to stdout. \
        Return only Python code with no markdown fences or explanation.",
    );
    prompt
}

// ── Test execution ────────────────────────────────────────────────────────────

async fn run_test_cases(
    code: &str,
    test_cases: &[LcbTestCase],
    timeout_secs: u64,
) -> (usize, usize) {
    if test_cases.is_empty() || code.trim().is_empty() {
        return (0, test_cases.len());
    }

    let tmp_dir = match tempdir().await {
        Some(d) => d,
        None => return (0, test_cases.len()),
    };
    let code_file = tmp_dir.join("solution.py");
    if tokio::fs::write(&code_file, code).await.is_err() {
        return (0, test_cases.len());
    }

    let mut passed = 0usize;
    for tc in test_cases {
        if run_single_test(&code_file, tc, timeout_secs).await {
            passed += 1;
        }
    }

    // Best-effort cleanup.
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;

    (passed, test_cases.len())
}

async fn run_single_test(code_file: &Path, tc: &LcbTestCase, timeout_secs: u64) -> bool {
    let run = async {
        let mut child = Command::new("python3")
            .arg(code_file)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(tc.input.as_bytes()).await;
        }

        let output = child.wait_with_output().await.ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Some(outputs_match(&stdout, &tc.output))
    };

    timeout(Duration::from_secs(timeout_secs), run)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

fn outputs_match(actual: &str, expected: &str) -> bool {
    normalise_output(actual) == normalise_output(expected)
}

fn normalise_output(s: &str) -> String {
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Code extraction ───────────────────────────────────────────────────────────

fn extract_python_code(raw: &str) -> String {
    // Strip ```python ... ``` or ``` ... ``` fences if present.
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix("```python")
        .or_else(|| trimmed.strip_prefix("```py"))
        .or_else(|| trimmed.strip_prefix("```"))
    {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

// ── Dataset loading ───────────────────────────────────────────────────────────

async fn load_problems(path: &Path) -> Result<Vec<LcbProblem>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading LCB dataset from {}", path.display()))?;

    let mut problems = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let p: LcbProblem = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing LCB problem at line {} in {}",
                line_no + 1,
                path.display()
            )
        })?;
        problems.push(p);
    }
    Ok(problems)
}

fn parse_test_cases(value: &serde_json::Value) -> Vec<LcbTestCase> {
    // The HuggingFace download encodes public_test_cases as a JSON string.
    // Handle both: a JSON string that deserialises to an array, and an inline array.
    match value {
        serde_json::Value::String(s) => {
            serde_json::from_str(s).unwrap_or_default()
        }
        serde_json::Value::Array(_) => {
            serde_json::from_value(value.clone()).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resolve_dataset_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(p) = get_override(overrides, "LCB_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(p) = env::var("LCB_DATASET") {
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
            .join("livecodebench.jsonl"),
        paths
            .root
            .join("factory")
            .join("benchmarks")
            .join("fixtures")
            .join("livecodebench-smoke.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("livecodebench.jsonl"),
        paths
            .artifacts
            .join("benchmarks")
            .join("fixtures")
            .join("livecodebench.jsonl"),
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

fn normalise_difficulty(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "easy" => "easy".to_string(),
        "medium" => "medium".to_string(),
        "hard" => "hard".to_string(),
        other if other.is_empty() => "unknown".to_string(),
        other => other.to_string(),
    }
}

fn normalise_platform(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "leetcode" => "leetcode".to_string(),
        "atcoder" => "atcoder".to_string(),
        "codeforces" => "codeforces".to_string(),
        other if other.is_empty() => "unknown".to_string(),
        other => other.to_string(),
    }
}

fn excerpt(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..max])
    }
}

async fn tempdir() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("lcb_{}", uuid_fragment()));
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

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_markdown(summary: &LcbSummary) -> String {
    let mut out = format!(
        "# LiveCodeBench Report\n\n\
        **Mode:** {}  \n\
        **Provider:** {}  \n\
        **Problems:** {}  \n\
        **Pass@1:** {:.1}% ({}/{})  \n\
        **Generated:** {}  \n\n",
        summary.mode,
        summary.provider_label,
        summary.metrics.total_problems,
        summary.metrics.pass_at_1 * 100.0,
        summary.metrics.passed_problems,
        summary.metrics.total_problems,
        summary.generated_at,
    );

    out.push_str("## Results by Difficulty\n\n");
    out.push_str("| Difficulty | Total | Passed | Pass Rate |\n");
    out.push_str("|------------|-------|--------|-----------|\n");
    for (diff, m) in &summary.metrics.by_difficulty {
        out.push_str(&format!(
            "| {} | {} | {} | {:.1}% |\n",
            diff, m.total, m.passed, m.pass_rate * 100.0
        ));
    }

    out.push_str("\n## Results by Platform\n\n");
    out.push_str("| Platform | Total | Passed | Pass Rate |\n");
    out.push_str("|----------|-------|--------|-----------|\n");
    for (platform, m) in &summary.metrics.by_platform {
        out.push_str(&format!(
            "| {} | {} | {} | {:.1}% |\n",
            platform, m.total, m.passed, m.pass_rate * 100.0
        ));
    }

    out.push_str("\n## Problem Results\n\n");
    out.push_str("| ID | Title | Difficulty | Platform | Passed | Tests |\n");
    out.push_str("|----|-------|------------|----------|--------|-------|\n");
    for r in &summary.results {
        let mark = if r.passed { "✓" } else { "✗" };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {}/{} |\n",
            r.question_id,
            r.title,
            r.difficulty,
            r.platform,
            mark,
            r.test_cases_passed,
            r.test_cases_total,
        ));
    }

    out
}

// ── BenchmarkStatus helpers ───────────────────────────────────────────────────

pub fn status_for_output(output: &LcbRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &LcbRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

pub fn render_step_stdout(output: &LcbRunOutput) -> String {
    format!(
        "LiveCodeBench [{}] n={} pass@1={:.1}% ({}/{})\n",
        output.mode.as_str(),
        output.metrics.total_problems,
        output.metrics.pass_at_1 * 100.0,
        output.metrics.passed_problems,
        output.metrics.total_problems,
    )
}
