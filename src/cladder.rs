//! CLADDER benchmark adapter — Pearl's causal hierarchy evaluation.
//!
//! CLADDER (Causal Ladder Reasoning) tests three levels of Pearl's causal ladder:
//!   - **Associational** — "Does X correlate with Y?"  (P(Y | X))
//!   - **Interventional** — "What happens if we do X?"  (P(Y | do(X)))
//!   - **Counterfactual** — "What would have happened if X were different?"
//!
//! The adapter:
//! 1. Loads a CLADDER-format JSONL dataset (one question per line).
//! 2. Routes each question through the configured backend:
//!    - `harkonnen` — Coobie's `diagnose`-enriched PackChat dispatch which
//!      labels hypotheses by Pearl hierarchy level before answering.
//!    - `direct` — raw LLM baseline (no Coobie memory or causal graph).
//! 3. Scores answers as binary yes/no against ground truth.
//! 4. Reports accuracy broken down by hierarchy level so the three levels
//!    can be compared separately and against competitors.
//!
//! Dataset format (one JSON object per line):
//! ```json
//! {
//!   "question_id": "q001",
//!   "question": "If smoking causes cancer, does a smoker have a higher chance of cancer?",
//!   "answer": "yes",
//!   "rung": "associational",
//!   "given": { ... }
//! }
//! ```
//!
//! Env / manifest overrides:
//! - `CLADDER_DATASET`   — path to JSONL dataset file
//! - `CLADDER_OUTPUT`    — output directory (defaults to `factory/artifacts/benchmarks/cladder/`)
//! - `CLADDER_MODE`      — `harkonnen` (default) or `direct`
//! - `CLADDER_LIMIT`     — max questions to evaluate
//! - `CLADDER_PROVIDER`  — LLM provider for `direct` mode
//! - `CLADDER_MIN_ACC`   — minimum overall accuracy to pass (0.0 – 1.0)

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use crate::{
    benchmark::BenchmarkStatus,
    chat,
    config::Paths,
    llm::{self, LlmRequest, Message},
    models::PearlHierarchyLevel,
};

// ── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_AGENT: &str = "coobie";
const DEFAULT_ABSTENTION: &str = "unknown";

// ── Public enums and structs ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CladderMode {
    Harkonnen,
    Direct,
}

impl CladderMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "packchat" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => bail!("unsupported CLADDER_MODE: {}", other),
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
pub struct CladderRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: CladderMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub min_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CladderRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: CladderMode,
    pub provider_label: String,
    pub metrics: CladderMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CladderSuiteOutcome {
    Completed(CladderRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CladderMetrics {
    pub total_questions: usize,
    pub overall_accuracy: f64,
    pub by_rung: BTreeMap<String, CladderRungMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CladderRungMetrics {
    pub total: usize,
    pub correct: usize,
    pub accuracy: f64,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct CladderQuestion {
    question_id: String,
    question: String,
    answer: String,
    rung: String,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CladderQuestionResult {
    question_id: String,
    question: String,
    expected: String,
    predicted: String,
    rung: String,
    hierarchy_level: String,
    correct: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CladderSummary {
    dataset_path: String,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: CladderMetrics,
    results: Vec<CladderQuestionResult>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<CladderSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(p) => p,
        None => {
            return Ok(CladderSuiteOutcome::Skipped(
                "CLADDER dataset not found. Set CLADDER_DATASET or place a fixture under \
                factory/benchmarks/fixtures/ or development_datasets/.".to_string(),
            ))
        }
    };

    let mode = CladderMode::parse(get_override(overrides, "CLADDER_MODE"))?;
    let agent = get_override(overrides, "CLADDER_AGENT")
        .unwrap_or_else(|| DEFAULT_AGENT.to_string());
    let direct_provider = get_override(overrides, "CLADDER_PROVIDER")
        .unwrap_or_else(|| "anthropic".to_string());
    let limit: Option<usize> = get_override(overrides, "CLADDER_LIMIT")
        .and_then(|v| v.parse().ok());
    let min_accuracy: Option<f64> = get_override(overrides, "CLADDER_MIN_ACC")
        .and_then(|v| v.parse().ok());
    let output_dir = get_override(overrides, "CLADDER_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.artifacts.join("benchmarks").join("cladder"));

    let config = CladderRunConfig {
        dataset_path,
        output_dir,
        mode,
        agent,
        direct_provider,
        limit,
        min_accuracy,
    };

    run(paths, &config).await.map(CladderSuiteOutcome::Completed)
}

pub async fn run(paths: &Paths, config: &CladderRunConfig) -> Result<CladderRunOutput> {
    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .context("creating CLADDER output dir")?;

    let questions = load_questions(&config.dataset_path).await?;
    let questions: Vec<_> = if let Some(limit) = config.limit {
        questions.into_iter().take(limit).collect()
    } else {
        questions
    };

    let provider_label = format!("{}-{}", config.mode.as_str(), config.agent);
    let mut results: Vec<CladderQuestionResult> = Vec::with_capacity(questions.len());
    let mut by_rung: BTreeMap<String, CladderRungMetrics> = BTreeMap::new();
    let mut total_correct = 0usize;

    for question in &questions {
        let predicted = answer_question(paths, config, question).await;
        let correct = answers_match(&question.answer, &predicted);
        let hierarchy_level = rung_to_hierarchy_label(&question.rung);

        if correct {
            total_correct += 1;
        }

        let rung_entry = by_rung.entry(question.rung.clone()).or_default();
        rung_entry.total += 1;
        if correct {
            rung_entry.correct += 1;
        }
        rung_entry.accuracy = if rung_entry.total > 0 {
            rung_entry.correct as f64 / rung_entry.total as f64
        } else {
            0.0
        };

        results.push(CladderQuestionResult {
            question_id: question.question_id.clone(),
            question: question.question.clone(),
            expected: question.answer.clone(),
            predicted,
            rung: question.rung.clone(),
            hierarchy_level,
            correct,
        });
    }

    let total = questions.len();
    let overall_accuracy = if total > 0 {
        total_correct as f64 / total as f64
    } else {
        0.0
    };

    let metrics = CladderMetrics {
        total_questions: total,
        overall_accuracy,
        by_rung,
    };

    let threshold_failure = config.min_accuracy.and_then(|min| {
        if overall_accuracy < min {
            Some(format!(
                "CLADDER accuracy {:.3} below threshold {:.3}",
                overall_accuracy, min
            ))
        } else {
            None
        }
    });

    let summary = CladderSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label.clone(),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        results,
    };

    let summary_path = config.output_dir.join("cladder_summary.json");
    tokio::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).context("serializing CLADDER summary")?,
    )
    .await
    .context("writing CLADDER summary")?;

    let markdown = render_markdown(&summary);
    let markdown_path = config.output_dir.join("cladder_report.md");
    tokio::fs::write(&markdown_path, markdown)
        .await
        .context("writing CLADDER markdown report")?;

    Ok(CladderRunOutput {
        output_dir: config.output_dir.clone(),
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label,
        metrics,
        threshold_failure,
    })
}

// ── Question answering ────────────────────────────────────────────────────────

async fn answer_question(
    paths: &Paths,
    config: &CladderRunConfig,
    question: &CladderQuestion,
) -> String {
    match config.mode {
        CladderMode::Harkonnen => {
            answer_via_packchat(paths, &config.agent, question).await
        }
        CladderMode::Direct => answer_direct(paths, &config.direct_provider, question).await,
    }
}

async fn answer_via_packchat(
    paths: &Paths,
    agent: &str,
    question: &CladderQuestion,
) -> String {
    let rung_label = rung_to_hierarchy_label(&question.rung);
    let prompt = if let Some(ctx) = &question.context {
        format!(
            "Causal question (hierarchy level: {rung_label}).\n\nContext:\n{ctx}\n\nQuestion: {}\n\nAnswer with a single word: yes or no.",
            question.question
        )
    } else {
        format!(
            "Causal question (hierarchy level: {rung_label}).\n\nQuestion: {}\n\nAnswer with a single word: yes or no.",
            question.question
        )
    };

    match chat::complete_agent_reply(agent, &prompt, &[], None, paths).await {
        Ok(reply) => extract_yes_no(&reply),
        Err(_) => DEFAULT_ABSTENTION.to_string(),
    }
}

async fn answer_direct(paths: &Paths, provider: &str, question: &CladderQuestion) -> String {
    let rung_label = rung_to_hierarchy_label(&question.rung);
    let system = format!(
        "You are a causal reasoning expert. Answer questions about the {} level of Pearl's causal ladder with a single word: yes or no.",
        rung_label
    );
    let user_text = if let Some(ctx) = &question.context {
        format!("Context:\n{}\n\nQuestion: {}", ctx, question.question)
    } else {
        question.question.clone()
    };

    let Some(llm) = llm::build_provider("benchmark", provider, &paths.setup) else {
        return DEFAULT_ABSTENTION.to_string();
    };

    let request = LlmRequest {
        messages: vec![Message::system(&system), Message::user(&user_text)],
        max_tokens: 16,
        temperature: 0.0,
    };

    match llm.complete(request).await {
        Ok(response) => extract_yes_no(&response.content),
        Err(_) => DEFAULT_ABSTENTION.to_string(),
    }
}

// ── Scoring ───────────────────────────────────────────────────────────────────

/// Normalise and compare answers; treats any non-yes/no as "unknown" mismatch.
fn answers_match(expected: &str, predicted: &str) -> bool {
    normalise_answer(expected) == normalise_answer(predicted)
}

fn normalise_answer(s: &str) -> &str {
    let s = s.trim();
    if s.eq_ignore_ascii_case("yes") {
        "yes"
    } else if s.eq_ignore_ascii_case("no") {
        "no"
    } else {
        "unknown"
    }
}

fn extract_yes_no(text: &str) -> String {
    let lower = text.to_lowercase();
    // Scan words for "yes" or "no" (exact word boundary).
    for word in lower.split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_alphabetic());
        if word == "yes" {
            return "yes".to_string();
        }
        if word == "no" {
            return "no".to_string();
        }
    }
    DEFAULT_ABSTENTION.to_string()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn rung_to_hierarchy_label(rung: &str) -> String {
    let rung = rung.trim().to_ascii_lowercase();
    let level = match rung.as_str() {
        "associational" | "rung1" | "1" | "correlation" => PearlHierarchyLevel::Associational,
        "interventional" | "rung2" | "2" | "intervention" | "do" => PearlHierarchyLevel::Interventional,
        "counterfactual" | "rung3" | "3" | "cf" => PearlHierarchyLevel::Counterfactual,
        _ => PearlHierarchyLevel::Associational,
    };
    // Serialise to match the model's serde snake_case tag.
    match level {
        PearlHierarchyLevel::Associational => "associational".to_string(),
        PearlHierarchyLevel::Interventional => "interventional".to_string(),
        PearlHierarchyLevel::Counterfactual => "counterfactual".to_string(),
    }
}

async fn load_questions(path: &Path) -> Result<Vec<CladderQuestion>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading CLADDER dataset from {}", path.display()))?;

    let mut questions = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let q: CladderQuestion = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing CLADDER question at line {} in {}",
                line_no + 1,
                path.display()
            )
        })?;
        questions.push(q);
    }
    Ok(questions)
}

fn resolve_dataset_path(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    // 1. Explicit override.
    if let Some(p) = get_override(overrides, "CLADDER_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // 2. Env var.
    if let Ok(p) = env::var("CLADDER_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // 3. Well-known fixture locations.
    let candidates = [
        paths.artifacts.join("benchmarks").join("fixtures").join("cladder.jsonl"),
        paths.artifacts.join("benchmarks").join("fixtures").join("cladder_eval.jsonl"),
        paths.root.join("development_datasets").join("cladder.jsonl"),
        paths.root.join("development_datasets").join("cladder_eval.jsonl"),
        paths.root.join("factory").join("benchmarks").join("fixtures").join("cladder.jsonl"),
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

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_markdown(summary: &CladderSummary) -> String {
    let mut out = format!(
        "# CLADDER Benchmark Report\n\n\
        **Mode:** {}  \n\
        **Provider:** {}  \n\
        **Questions:** {}  \n\
        **Overall Accuracy:** {:.1}%  \n\
        **Generated:** {}  \n\n",
        summary.mode,
        summary.provider_label,
        summary.metrics.total_questions,
        summary.metrics.overall_accuracy * 100.0,
        summary.generated_at,
    );

    out.push_str("## Accuracy by Pearl Hierarchy Level\n\n");
    out.push_str("| Rung | Total | Correct | Accuracy |\n");
    out.push_str("|------|-------|---------|----------|\n");
    for (rung, m) in &summary.metrics.by_rung {
        out.push_str(&format!(
            "| {} | {} | {} | {:.1}% |\n",
            rung, m.total, m.correct, m.accuracy * 100.0
        ));
    }

    out.push_str("\n## Sample Results\n\n");
    for result in summary.results.iter().take(10) {
        let mark = if result.correct { "✓" } else { "✗" };
        out.push_str(&format!(
            "**{}** [{}] {} → expected `{}`, got `{}`  \n",
            result.question_id, result.rung, mark, result.expected, result.predicted
        ));
        out.push_str(&format!("_{}_\n\n", result.question));
    }

    out
}

// ── BenchmarkStatus helpers ───────────────────────────────────────────────────

pub fn status_for_output(output: &CladderRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &CladderRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

pub fn render_step_stdout(output: &CladderRunOutput) -> String {
    format!(
        "CLADDER [{}] n={} acc={:.1}%  rungs: {}\n",
        output.mode.as_str(),
        output.metrics.total_questions,
        output.metrics.overall_accuracy * 100.0,
        output
            .metrics
            .by_rung
            .iter()
            .map(|(r, m)| format!("{}={:.0}%", r, m.accuracy * 100.0))
            .collect::<Vec<_>>()
            .join(" "),
    )
}
