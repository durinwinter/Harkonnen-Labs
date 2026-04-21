use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::process::Command;

use crate::{
    aider_polyglot, cladder, config::Paths, frames, helmet, livecodebench, locomo, longmemeval,
    scenario_delta, spec_adherence, streamingqa, twin_fidelity,
};

const SKIP_EXIT_CODE: i32 = 10;
const OUTPUT_LIMIT: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkManifest {
    pub version: u32,
    #[serde(default)]
    pub suites: Vec<BenchmarkSuite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub id: String,
    pub title: String,
    pub subsystem: String,
    pub category: String,
    pub tier: String,
    pub description: String,
    #[serde(default)]
    pub benchmark_url: Option<String>,
    #[serde(default)]
    pub leaderboard_url: Option<String>,
    #[serde(default)]
    pub baseline_reference: Option<String>,
    #[serde(default)]
    pub setup_notes: Vec<String>,
    #[serde(default)]
    pub required_env: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub working_dir_env: Option<String>,
    #[serde(default)]
    pub default_selected: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub steps: Vec<BenchmarkStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStep {
    pub id: String,
    #[serde(default)]
    pub builtin: Option<String>,
    #[serde(default)]
    pub program: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cwd_env: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkStatus {
    Passed,
    Failed,
    Skipped,
}

impl BenchmarkStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStepResult {
    pub id: String,
    pub label: String,
    pub status: BenchmarkStatus,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuiteResult {
    pub id: String,
    pub title: String,
    pub subsystem: String,
    pub category: String,
    pub tier: String,
    pub description: String,
    #[serde(default)]
    pub benchmark_url: Option<String>,
    #[serde(default)]
    pub leaderboard_url: Option<String>,
    #[serde(default)]
    pub baseline_reference: Option<String>,
    #[serde(default)]
    pub setup_notes: Vec<String>,
    #[serde(default)]
    pub required_env: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub status: BenchmarkStatus,
    pub duration_ms: u128,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub steps: Vec<BenchmarkStepResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunReport {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub manifest_path: String,
    pub repo_root: String,
    pub selected_suites: Vec<String>,
    pub summary: BenchmarkRunSummary,
    pub suites: Vec<BenchmarkSuiteResult>,
}

#[derive(Debug, Clone)]
pub struct BenchmarkRunOutput {
    pub json_path: PathBuf,
    pub markdown_path: PathBuf,
    pub report: BenchmarkRunReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunArtifactSummary {
    pub report_id: String,
    pub generated_at: DateTime<Utc>,
    pub manifest_path: String,
    pub json_path: String,
    pub markdown_path: String,
    pub selected_suites: Vec<String>,
    pub summary: BenchmarkRunSummary,
}

pub fn artifact_dir(paths: &Paths) -> PathBuf {
    paths.artifacts.join("benchmarks")
}

pub fn default_manifest_path(paths: &Paths) -> PathBuf {
    paths.factory.join("benchmarks").join("suites.yaml")
}

pub fn default_output_path(paths: &Paths) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    artifact_dir(paths).join(format!("benchmark-run-{}.json", stamp))
}

pub fn load_manifest(path: &Path) -> Result<BenchmarkManifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading benchmark manifest {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let manifest = if ext == "json" {
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing benchmark manifest json {}", path.display()))?
    } else {
        serde_yaml::from_str(&raw)
            .or_else(|_| serde_json::from_str(&raw))
            .with_context(|| format!("parsing benchmark manifest {}", path.display()))?
    };
    Ok(manifest)
}

pub fn render_manifest_overview(manifest: &BenchmarkManifest) -> String {
    if manifest.suites.is_empty() {
        return "No benchmark suites configured.".to_string();
    }

    let mut lines = vec![
        "Benchmark Suites".to_string(),
        "================".to_string(),
    ];
    for suite in &manifest.suites {
        lines.push(format!(
            "- {} [{} | {} | default={}]",
            suite.id,
            suite.subsystem,
            suite.tier,
            if suite.default_selected { "yes" } else { "no" }
        ));
        lines.push(format!("  {}", suite.title));
        lines.push(format!("  {}", suite.description));
        if !suite.required_env.is_empty() {
            lines.push(format!("  requires env: {}", suite.required_env.join(", ")));
        }
        if let Some(url) = &suite.benchmark_url {
            lines.push(format!("  benchmark: {}", url));
        }
        if let Some(reference) = &suite.baseline_reference {
            lines.push(format!("  baseline: {}", reference));
        }
    }
    lines.join("\n")
}

pub async fn run_benchmarks(
    paths: &Paths,
    manifest_path: &Path,
    selected_suite_ids: &[String],
    run_all: bool,
    output_path: Option<&Path>,
) -> Result<BenchmarkRunOutput> {
    let manifest = load_manifest(manifest_path)?;
    let suites = select_suites(&manifest, selected_suite_ids, run_all)?;

    let mut results = Vec::new();
    for suite in suites {
        results.push(run_suite(paths, &suite).await?);
    }

    let summary = summarize(&results);
    let selected_ids = results
        .iter()
        .map(|suite| suite.id.clone())
        .collect::<Vec<_>>();
    let report = BenchmarkRunReport {
        version: manifest.version,
        generated_at: Utc::now(),
        manifest_path: manifest_path.display().to_string(),
        repo_root: paths.root.display().to_string(),
        selected_suites: selected_ids,
        summary,
        suites: results,
    };

    let json_path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_path(paths));
    let markdown_path = json_path.with_extension("md");
    if let Some(parent) = json_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating benchmark output dir {}", parent.display()))?;
    }

    let raw_json = serde_json::to_string_pretty(&report)?;
    tokio::fs::write(&json_path, raw_json)
        .await
        .with_context(|| format!("writing benchmark report {}", json_path.display()))?;
    let markdown = render_report_markdown(&report);
    tokio::fs::write(&markdown_path, markdown)
        .await
        .with_context(|| format!("writing benchmark markdown {}", markdown_path.display()))?;

    Ok(BenchmarkRunOutput {
        json_path,
        markdown_path,
        report,
    })
}

pub fn load_run_report(path: &Path) -> Result<BenchmarkRunReport> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading benchmark run report {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing benchmark run report {}", path.display()))
}

pub fn list_recent_run_reports(
    paths: &Paths,
    limit: usize,
) -> Result<Vec<BenchmarkRunArtifactSummary>> {
    list_recent_run_reports_in_dir(&artifact_dir(paths), limit)
}

pub fn resolve_run_report_path(paths: &Paths, report_id: Option<&str>) -> Result<PathBuf> {
    resolve_run_report_path_in_dir(&artifact_dir(paths), report_id)
}

pub fn render_report_markdown(report: &BenchmarkRunReport) -> String {
    let mut lines = vec![
        "# Benchmark Report".to_string(),
        String::new(),
        format!("- Generated: {}", report.generated_at),
        format!("- Manifest: {}", report.manifest_path),
        format!("- Repo root: {}", report.repo_root),
        format!(
            "- Summary: total={} passed={} failed={} skipped={}",
            report.summary.total,
            report.summary.passed,
            report.summary.failed,
            report.summary.skipped
        ),
        String::new(),
        "## Suite Summary".to_string(),
        String::new(),
        "| Suite | Subsystem | Tier | Status | Duration ms |".to_string(),
        "| --- | --- | --- | --- | ---: |".to_string(),
    ];

    for suite in &report.suites {
        lines.push(format!(
            "| {} | {} | {} | {} | {} |",
            suite.id,
            suite.subsystem,
            suite.tier,
            suite.status.as_str(),
            suite.duration_ms
        ));
    }

    for suite in &report.suites {
        lines.push(String::new());
        lines.push(format!("## {} ({})", suite.title, suite.id));
        lines.push(String::new());
        lines.push(format!("- Status: {}", suite.status.as_str()));
        lines.push(format!("- Subsystem: {}", suite.subsystem));
        lines.push(format!("- Category: {}", suite.category));
        lines.push(format!("- Tier: {}", suite.tier));
        lines.push(format!("- Duration: {} ms", suite.duration_ms));
        if let Some(url) = &suite.benchmark_url {
            lines.push(format!("- Benchmark: {}", url));
        }
        if let Some(url) = &suite.leaderboard_url {
            lines.push(format!("- Leaderboard: {}", url));
        }
        if let Some(reference) = &suite.baseline_reference {
            lines.push(format!("- Baseline: {}", reference));
        }
        if let Some(reason) = &suite.reason {
            lines.push(format!("- Reason: {}", reason));
        }
        if !suite.required_env.is_empty() {
            lines.push(format!("- Required env: {}", suite.required_env.join(", ")));
        }
        if !suite.setup_notes.is_empty() {
            lines.push("- Setup notes:".to_string());
            for note in &suite.setup_notes {
                lines.push(format!("  - {}", note));
            }
        }
        if !suite.steps.is_empty() {
            lines.push(String::new());
            lines.push("### Steps".to_string());
            lines.push(String::new());
            lines.push("| Step | Status | Exit | Duration ms |".to_string());
            lines.push("| --- | --- | ---: | ---: |".to_string());
            for step in &suite.steps {
                lines.push(format!(
                    "| {} | {} | {} | {} |",
                    step.label,
                    step.status.as_str(),
                    step.exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    step.duration_ms
                ));
            }
        }
    }

    let correct_sections = render_correct_answer_sections(report);
    if !correct_sections.is_empty() {
        lines.push(String::new());
        lines.push("## Correct Answers".to_string());
        for section in correct_sections {
            lines.push(String::new());
            lines.extend(section);
        }
    }

    lines.push(String::new());
    lines.join("\n")
}

fn render_correct_answer_sections(report: &BenchmarkRunReport) -> Vec<Vec<String>> {
    report
        .suites
        .iter()
        .filter_map(render_correct_answer_section)
        .collect()
}

fn render_correct_answer_section(suite: &BenchmarkSuiteResult) -> Option<Vec<String>> {
    let summary_path = suite
        .steps
        .iter()
        .find_map(|step| extract_summary_json_path(&step.stdout))?;
    let raw = std::fs::read_to_string(&summary_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let questions = json.get("questions")?.as_array()?;

    let mut lines = vec![format!("### {} ({})", suite.title, suite.id)];
    let mut correct = Vec::new();

    for question in questions {
        if let Some(entry) = correct_answer_entry(question) {
            correct.push(entry);
        }
    }

    lines.push(format!(
        "- Correct answers: {}/{}",
        correct.len(),
        questions.len()
    ));
    if correct.is_empty() {
        lines.push("- None recorded in this run.".to_string());
    } else {
        for entry in correct {
            lines.push(format!("- {}", entry));
        }
    }

    Some(lines)
}

fn correct_answer_entry(question: &serde_json::Value) -> Option<String> {
    if question
        .get("exact_match")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        let id = question.get("question_id")?.as_str()?;
        let answer = question.get("hypothesis")?.as_str()?.trim();
        return Some(format!("`{}`: `{}`", id, answer));
    }

    let score = question.get("score").and_then(|value| value.as_f64())?;
    if score >= 1.0 {
        let sample_id = question.get("sample_id")?.as_str()?;
        let qa_index = question.get("qa_index")?.as_u64()?;
        let answer = question.get("hypothesis")?.as_str()?.trim();
        return Some(format!("`{}#{}`: `{}`", sample_id, qa_index, answer));
    }

    None
}

fn extract_summary_json_path(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        line.strip_prefix("Summary JSON: ")
            .map(|value| value.trim().to_string())
    })
}

fn select_suites(
    manifest: &BenchmarkManifest,
    selected_suite_ids: &[String],
    run_all: bool,
) -> Result<Vec<BenchmarkSuite>> {
    if manifest.suites.is_empty() {
        bail!("benchmark manifest defines no suites");
    }

    let suites = if run_all {
        manifest.suites.clone()
    } else if !selected_suite_ids.is_empty() {
        let mut selected = Vec::new();
        for suite_id in selected_suite_ids {
            let suite = manifest
                .suites
                .iter()
                .find(|suite| suite.id == *suite_id)
                .with_context(|| format!("benchmark suite not found: {}", suite_id))?;
            selected.push(suite.clone());
        }
        selected
    } else {
        let defaults = manifest
            .suites
            .iter()
            .filter(|suite| suite.default_selected)
            .cloned()
            .collect::<Vec<_>>();
        if defaults.is_empty() {
            bail!("no default benchmark suites configured; use --all or --suite");
        }
        defaults
    };

    Ok(suites)
}

async fn run_suite(paths: &Paths, suite: &BenchmarkSuite) -> Result<BenchmarkSuiteResult> {
    let suite_started = Instant::now();
    let missing_env = suite
        .required_env
        .iter()
        .filter(|name| match env::var(name) {
            Ok(value) => value.trim().is_empty(),
            Err(_) => true,
        })
        .cloned()
        .collect::<Vec<_>>();

    if !missing_env.is_empty() {
        return Ok(BenchmarkSuiteResult {
            id: suite.id.clone(),
            title: suite.title.clone(),
            subsystem: suite.subsystem.clone(),
            category: suite.category.clone(),
            tier: suite.tier.clone(),
            description: suite.description.clone(),
            benchmark_url: suite.benchmark_url.clone(),
            leaderboard_url: suite.leaderboard_url.clone(),
            baseline_reference: suite.baseline_reference.clone(),
            setup_notes: suite.setup_notes.clone(),
            required_env: suite.required_env.clone(),
            tags: suite.tags.clone(),
            status: BenchmarkStatus::Skipped,
            duration_ms: suite_started.elapsed().as_millis(),
            reason: Some(format!("missing required env: {}", missing_env.join(", "))),
            steps: Vec::new(),
        });
    }

    if suite.steps.is_empty() {
        return Ok(BenchmarkSuiteResult {
            id: suite.id.clone(),
            title: suite.title.clone(),
            subsystem: suite.subsystem.clone(),
            category: suite.category.clone(),
            tier: suite.tier.clone(),
            description: suite.description.clone(),
            benchmark_url: suite.benchmark_url.clone(),
            leaderboard_url: suite.leaderboard_url.clone(),
            baseline_reference: suite.baseline_reference.clone(),
            setup_notes: suite.setup_notes.clone(),
            required_env: suite.required_env.clone(),
            tags: suite.tags.clone(),
            status: BenchmarkStatus::Skipped,
            duration_ms: suite_started.elapsed().as_millis(),
            reason: Some("suite defines no runnable steps".to_string()),
            steps: Vec::new(),
        });
    }

    let mut step_results = Vec::new();
    let mut suite_status = BenchmarkStatus::Passed;
    let mut suite_reason = None;

    for step in &suite.steps {
        let step_result = run_step(paths, suite, step).await?;
        match step_result.status {
            BenchmarkStatus::Passed => {}
            BenchmarkStatus::Skipped => {
                suite_status = BenchmarkStatus::Skipped;
                suite_reason = step_result.reason.clone();
                step_results.push(step_result);
                break;
            }
            BenchmarkStatus::Failed => {
                suite_status = BenchmarkStatus::Failed;
                suite_reason = step_result.reason.clone();
                step_results.push(step_result);
                break;
            }
        }
        step_results.push(step_result);
    }

    Ok(BenchmarkSuiteResult {
        id: suite.id.clone(),
        title: suite.title.clone(),
        subsystem: suite.subsystem.clone(),
        category: suite.category.clone(),
        tier: suite.tier.clone(),
        description: suite.description.clone(),
        benchmark_url: suite.benchmark_url.clone(),
        leaderboard_url: suite.leaderboard_url.clone(),
        baseline_reference: suite.baseline_reference.clone(),
        setup_notes: suite.setup_notes.clone(),
        required_env: suite.required_env.clone(),
        tags: suite.tags.clone(),
        status: suite_status,
        duration_ms: suite_started.elapsed().as_millis(),
        reason: suite_reason,
        steps: step_results,
    })
}

async fn run_step(
    paths: &Paths,
    suite: &BenchmarkSuite,
    step: &BenchmarkStep,
) -> Result<BenchmarkStepResult> {
    let cwd = resolve_cwd(paths, suite, step)?;
    let label = step.label.clone().unwrap_or_else(|| step.id.clone());
    let started = Instant::now();

    if let Some(builtin) = &step.builtin {
        return run_builtin_step(paths, step, builtin, &cwd, label, started).await;
    }

    if step.program.trim().is_empty() {
        return Ok(BenchmarkStepResult {
            id: step.id.clone(),
            label,
            status: BenchmarkStatus::Failed,
            program: step.program.clone(),
            args: step.args.clone(),
            cwd: cwd.display().to_string(),
            duration_ms: started.elapsed().as_millis(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            reason: Some(format!(
                "benchmark step {} has no program or builtin runner",
                step.id
            )),
        });
    }

    let mut command = Command::new(&step.program);
    command.args(&step.args).current_dir(&cwd);
    for (key, value) in &step.env {
        command.env(key, value);
    }

    let output = command
        .output()
        .await
        .with_context(|| format!("running benchmark step {}", step.id));

    let duration_ms = started.elapsed().as_millis();
    match output {
        Ok(output) => {
            let exit_code = output.status.code();
            let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
            let stderr = truncate(&String::from_utf8_lossy(&output.stderr));
            let (status, reason) = if output.status.success() {
                (BenchmarkStatus::Passed, None)
            } else if exit_code == Some(SKIP_EXIT_CODE) {
                (
                    BenchmarkStatus::Skipped,
                    Some(
                        first_non_empty(&stderr, &stdout)
                            .unwrap_or_else(|| format!("step {} requested skip", step.id)),
                    ),
                )
            } else {
                (
                    BenchmarkStatus::Failed,
                    Some(
                        first_non_empty(&stderr, &stdout)
                            .unwrap_or_else(|| format!("step {} failed", step.id)),
                    ),
                )
            };

            Ok(BenchmarkStepResult {
                id: step.id.clone(),
                label,
                status,
                program: step.program.clone(),
                args: step.args.clone(),
                cwd: cwd.display().to_string(),
                duration_ms,
                exit_code,
                stdout,
                stderr,
                reason,
            })
        }
        Err(error) => Ok(BenchmarkStepResult {
            id: step.id.clone(),
            label,
            status: BenchmarkStatus::Failed,
            program: step.program.clone(),
            args: step.args.clone(),
            cwd: cwd.display().to_string(),
            duration_ms,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            reason: Some(error.to_string()),
        }),
    }
}

async fn run_builtin_step(
    paths: &Paths,
    step: &BenchmarkStep,
    builtin: &str,
    cwd: &Path,
    label: String,
    started: Instant,
) -> Result<BenchmarkStepResult> {
    let duration_ms;
    let (status, stdout, stderr, reason) = match builtin {
        "longmemeval" => match longmemeval::run_with_overrides(paths, &step.env).await? {
            longmemeval::LongMemEvalSuiteOutcome::Completed(output) => {
                let status = longmemeval::status_for_output(&output);
                let reason = longmemeval::reason_for_output(&output);
                (
                    status,
                    longmemeval::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            longmemeval::LongMemEvalSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "locomo" => match locomo::run_with_overrides(paths, &step.env).await? {
            locomo::LoCoMoSuiteOutcome::Completed(output) => {
                let status = locomo::status_for_output(&output);
                let reason = locomo::reason_for_output(&output);
                (
                    status,
                    locomo::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            locomo::LoCoMoSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "frames" => match frames::run_with_overrides(paths, &step.env).await? {
            frames::FramesSuiteOutcome::Completed(output) => {
                let status = frames::status_for_output(&output);
                let reason = frames::reason_for_output(&output);
                (
                    status,
                    frames::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            frames::FramesSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "streamingqa" => match streamingqa::run_with_overrides(paths, &step.env).await? {
            streamingqa::StreamingQaSuiteOutcome::Completed(output) => {
                let status = streamingqa::status_for_output(&output);
                let reason = streamingqa::reason_for_output(&output);
                (
                    status,
                    streamingqa::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            streamingqa::StreamingQaSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "cladder" => match cladder::run_with_overrides(paths, &step.env).await? {
            cladder::CladderSuiteOutcome::Completed(output) => {
                let status = cladder::status_for_output(&output);
                let reason = cladder::reason_for_output(&output);
                (
                    status,
                    cladder::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            cladder::CladderSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "helmet" => match helmet::run_with_overrides(paths, &step.env).await? {
            helmet::HelmetSuiteOutcome::Completed(output) => {
                let status = helmet::status_for_output(&output);
                let reason = helmet::reason_for_output(&output);
                (
                    status,
                    helmet::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            helmet::HelmetSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "aider_polyglot" => match aider_polyglot::run_with_overrides(paths, &step.env).await? {
            aider_polyglot::PolyglotSuiteOutcome::Completed(output) => {
                let status = aider_polyglot::status_for_output(&output);
                let reason = aider_polyglot::reason_for_output(&output);
                (
                    status,
                    aider_polyglot::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            aider_polyglot::PolyglotSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "livecodebench" => match livecodebench::run_with_overrides(paths, &step.env).await? {
            livecodebench::LcbSuiteOutcome::Completed(output) => {
                let status = livecodebench::status_for_output(&output);
                let reason = livecodebench::reason_for_output(&output);
                (
                    status,
                    livecodebench::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            livecodebench::LcbSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "spec_adherence" => match spec_adherence::run_with_overrides(paths, &step.env).await? {
            spec_adherence::SpecAdherenceSuiteOutcome::Completed(output) => {
                let status = spec_adherence::status_for_output(&output);
                let reason = spec_adherence::reason_for_output(&output);
                (
                    status,
                    spec_adherence::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            spec_adherence::SpecAdherenceSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "scenario_delta" => match scenario_delta::run_with_overrides(paths, &step.env).await? {
            scenario_delta::ScenarioDeltaSuiteOutcome::Completed(output) => {
                let status = scenario_delta::status_for_output(&output);
                let reason = scenario_delta::reason_for_output(&output);
                (
                    status,
                    scenario_delta::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            scenario_delta::ScenarioDeltaSuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        "twin_fidelity" => match twin_fidelity::run_with_overrides(paths, &step.env).await? {
            twin_fidelity::TwinFidelitySuiteOutcome::Completed(output) => {
                let status = twin_fidelity::status_for_output(&output);
                let reason = twin_fidelity::reason_for_output(&output);
                (
                    status,
                    twin_fidelity::render_step_stdout(&output),
                    String::new(),
                    reason,
                )
            }
            twin_fidelity::TwinFidelitySuiteOutcome::Skipped(reason) => (
                BenchmarkStatus::Skipped,
                String::new(),
                String::new(),
                Some(reason),
            ),
        },
        _ => (
            BenchmarkStatus::Failed,
            String::new(),
            String::new(),
            Some(format!("unknown benchmark builtin runner: {}", builtin)),
        ),
    };
    duration_ms = started.elapsed().as_millis();

    Ok(BenchmarkStepResult {
        id: step.id.clone(),
        label,
        status,
        program: if step.program.trim().is_empty() {
            format!("builtin:{}", builtin)
        } else {
            step.program.clone()
        },
        args: step.args.clone(),
        cwd: cwd.display().to_string(),
        duration_ms,
        exit_code: None,
        stdout: truncate(&stdout),
        stderr: truncate(&stderr),
        reason,
    })
}

fn resolve_cwd(paths: &Paths, suite: &BenchmarkSuite, step: &BenchmarkStep) -> Result<PathBuf> {
    if let Some(name) = step.cwd_env.as_ref().or(suite.working_dir_env.as_ref()) {
        let value = env::var(name)
            .with_context(|| format!("benchmark working dir env not set: {}", name))?;
        if value.trim().is_empty() {
            bail!("benchmark working dir env is empty: {}", name);
        }
        return Ok(PathBuf::from(value));
    }

    if let Some(cwd) = step.cwd.as_ref().or(suite.working_dir.as_ref()) {
        let path = PathBuf::from(cwd);
        if path.is_absolute() {
            return Ok(path);
        }
        return Ok(paths.root.join(path));
    }

    Ok(paths.root.clone())
}

fn summarize(results: &[BenchmarkSuiteResult]) -> BenchmarkRunSummary {
    let mut summary = BenchmarkRunSummary {
        total: results.len(),
        passed: 0,
        failed: 0,
        skipped: 0,
    };
    for result in results {
        match result.status {
            BenchmarkStatus::Passed => summary.passed += 1,
            BenchmarkStatus::Failed => summary.failed += 1,
            BenchmarkStatus::Skipped => summary.skipped += 1,
        }
    }
    summary
}

fn truncate(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= OUTPUT_LIMIT {
        return trimmed.to_string();
    }
    let kept = trimmed.chars().take(OUTPUT_LIMIT).collect::<String>();
    format!("{}\n... [truncated]", kept)
}

fn first_non_empty(preferred: &str, fallback: &str) -> Option<String> {
    let preferred = preferred.trim();
    if !preferred.is_empty() {
        return Some(preferred.to_string());
    }
    let fallback = fallback.trim();
    if !fallback.is_empty() {
        return Some(fallback.to_string());
    }
    None
}

fn list_recent_run_reports_in_dir(
    dir: &Path,
    limit: usize,
) -> Result<Vec<BenchmarkRunArtifactSummary>> {
    if limit == 0 || !dir.exists() {
        return Ok(Vec::new());
    }

    let mut json_paths = std::fs::read_dir(dir)
        .with_context(|| format!("reading benchmark artifact dir {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .filter(|path| is_benchmark_report_json_path(path))
        .collect::<Vec<_>>();
    json_paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    json_paths
        .into_iter()
        .take(limit)
        .map(|json_path| {
            let report = load_run_report(&json_path)?;
            let report_id = report_id_from_json_path(&json_path).with_context(|| {
                format!("invalid benchmark report path {}", json_path.display())
            })?;
            Ok(BenchmarkRunArtifactSummary {
                report_id,
                generated_at: report.generated_at,
                manifest_path: report.manifest_path,
                json_path: json_path.display().to_string(),
                markdown_path: json_path.with_extension("md").display().to_string(),
                selected_suites: report.selected_suites,
                summary: report.summary,
            })
        })
        .collect()
}

fn resolve_run_report_path_in_dir(dir: &Path, report_id: Option<&str>) -> Result<PathBuf> {
    if !dir.exists() {
        bail!("benchmark artifact dir does not exist: {}", dir.display());
    }

    match report_id.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("latest") => latest_run_report_path_in_dir(dir),
        Some(value) => {
            if value.contains('/') || value.contains('\\') {
                bail!("benchmark report id must be a basename, not a path");
            }
            let stem = value.strip_suffix(".json").unwrap_or(value);
            let path = dir.join(format!("{stem}.json"));
            if !path.exists() {
                bail!("benchmark report not found: {}", path.display());
            }
            Ok(path)
        }
    }
}

fn latest_run_report_path_in_dir(dir: &Path) -> Result<PathBuf> {
    let mut json_paths = std::fs::read_dir(dir)
        .with_context(|| format!("reading benchmark artifact dir {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .filter(|path| is_benchmark_report_json_path(path))
        .collect::<Vec<_>>();
    json_paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    json_paths
        .into_iter()
        .next()
        .with_context(|| format!("no benchmark run reports found in {}", dir.display()))
}

fn is_benchmark_report_json_path(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.starts_with("benchmark-run-") && value.ends_with(".json"))
            .unwrap_or(false)
}

fn report_id_from_json_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_report(generated_at: DateTime<Utc>) -> BenchmarkRunReport {
        BenchmarkRunReport {
            version: 1,
            generated_at,
            manifest_path: "factory/benchmarks/suites.yaml".to_string(),
            repo_root: "/tmp/harkonnen".to_string(),
            selected_suites: vec!["local_regression".to_string()],
            summary: BenchmarkRunSummary {
                total: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
            },
            suites: vec![BenchmarkSuiteResult {
                id: "local_regression".to_string(),
                title: "Local Regression Gate".to_string(),
                subsystem: "factory".to_string(),
                category: "regression".to_string(),
                tier: "required".to_string(),
                description: "Fast local gate".to_string(),
                benchmark_url: None,
                leaderboard_url: None,
                baseline_reference: None,
                setup_notes: Vec::new(),
                required_env: Vec::new(),
                tags: Vec::new(),
                status: BenchmarkStatus::Passed,
                duration_ms: 12,
                reason: None,
                steps: Vec::new(),
            }],
        }
    }

    fn write_report(dir: &Path, stem: &str, report: &BenchmarkRunReport) {
        fs::create_dir_all(dir).unwrap();
        let json = serde_json::to_string_pretty(report).unwrap();
        fs::write(dir.join(format!("{stem}.json")), json).unwrap();
        fs::write(dir.join(format!("{stem}.md")), "# benchmark report").unwrap();
    }

    #[test]
    fn render_report_contains_suite_summary() {
        let report = sample_report(Utc::now());

        let markdown = render_report_markdown(&report);
        assert!(markdown.contains("# Benchmark Report"));
        assert!(markdown.contains("local_regression"));
        assert!(markdown.contains("Local Regression Gate"));
    }

    #[test]
    fn recent_run_reports_are_listed_newest_first() {
        let temp_dir = std::env::temp_dir().join(format!(
            "harkonnen-benchmark-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        write_report(
            &temp_dir,
            "benchmark-run-20260421T010000Z",
            &sample_report(Utc::now()),
        );
        write_report(
            &temp_dir,
            "benchmark-run-20260421T020000Z",
            &sample_report(Utc::now()),
        );

        let reports = list_recent_run_reports_in_dir(&temp_dir, 10).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].report_id, "benchmark-run-20260421T020000Z");
        assert_eq!(reports[1].report_id, "benchmark-run-20260421T010000Z");

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_run_report_path_supports_latest_and_named_ids() {
        let temp_dir = std::env::temp_dir().join(format!(
            "harkonnen-benchmark-resolve-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        write_report(
            &temp_dir,
            "benchmark-run-20260421T030000Z",
            &sample_report(Utc::now()),
        );
        write_report(
            &temp_dir,
            "benchmark-run-20260421T040000Z",
            &sample_report(Utc::now()),
        );

        let latest = resolve_run_report_path_in_dir(&temp_dir, None).unwrap();
        assert!(latest
            .display()
            .to_string()
            .ends_with("benchmark-run-20260421T040000Z.json"));

        let explicit =
            resolve_run_report_path_in_dir(&temp_dir, Some("benchmark-run-20260421T030000Z"))
                .unwrap();
        assert!(explicit
            .display()
            .to_string()
            .ends_with("benchmark-run-20260421T030000Z.json"));

        fs::remove_dir_all(temp_dir).unwrap();
    }
}
