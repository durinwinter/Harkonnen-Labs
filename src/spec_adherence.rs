//! Spec adherence benchmark — LLM-as-judge scoring against run outputs.
//!
//! This benchmark measures how well recent Harkonnen runs satisfy their declared
//! acceptance criteria. It can either:
//! - read a JSONL dataset of `{ run_id, spec_path, output_path }` entries, or
//! - query the local SQLite DB for the most recent completed runs and infer a
//!   primary output artifact from the implementation episode's changed files.
//!
//! Env / manifest overrides:
//! - `SPEC_ADHERENCE_DATASET`            — optional JSONL dataset path
//! - `SPEC_ADHERENCE_LIMIT`              — max entries to evaluate
//! - `SPEC_ADHERENCE_OUTPUT`             — output directory
//! - `SPEC_ADHERENCE_MIN_COMPLETENESS`   — minimum aggregate completeness to pass
//! - `SPEC_ADHERENCE_MODE`               — `standard` (default) or `without_scout`
//! - `SPEC_ADHERENCE_WITHOUT_SCOUT`      — boolean shorthand for `without_scout`

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use crate::{
    benchmark::BenchmarkStatus,
    config::Paths,
    llm::{self, LlmRequest},
    models::Spec,
    spec,
};

const DEFAULT_DB_LIMIT: usize = 10;
const MAX_ARTIFACT_CHARS: usize = 12_000;
const MAX_SUPPORTING_DOC_CHARS: usize = 3_000;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecAdherenceMode {
    Standard,
    WithoutScout,
}

impl SpecAdherenceMode {
    fn parse(overrides: &BTreeMap<String, String>) -> Result<Self> {
        if read_env("SPEC_ADHERENCE_WITHOUT_SCOUT", overrides)
            .as_deref()
            .map(parse_boolish)
            .transpose()?
            .unwrap_or(false)
        {
            return Ok(Self::WithoutScout);
        }

        match read_env("SPEC_ADHERENCE_MODE", overrides)
            .unwrap_or_else(|| "standard".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "standard" | "default" => Ok(Self::Standard),
            "without_scout" | "without-scout" | "no_scout" | "noscout" => {
                Ok(Self::WithoutScout)
            }
            other => bail!("unsupported SPEC_ADHERENCE_MODE: {}", other),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::WithoutScout => "without_scout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpecAdherenceRunConfig {
    pub dataset_path: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub limit: Option<usize>,
    pub min_completeness: Option<f64>,
    pub mode: SpecAdherenceMode,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecAdherenceRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: SpecAdherenceMode,
    pub provider_label: String,
    pub metrics: SpecAdherenceMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SpecAdherenceSuiteOutcome {
    Completed(SpecAdherenceRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SpecAdherenceMetrics {
    pub total_entries: usize,
    pub judged_entries: usize,
    pub skipped_entries: usize,
    pub total_criteria: usize,
    pub met: usize,
    pub partial: usize,
    pub unmet: usize,
    pub completeness: f64,
    pub precision: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct DatasetEntry {
    #[serde(default)]
    run_id: Option<String>,
    spec_path: String,
    output_path: String,
}

#[derive(Debug, Clone)]
struct EvaluationEntry {
    run_id: Option<String>,
    spec_path: PathBuf,
    output_path: PathBuf,
    output_origin: String,
}

#[derive(Debug, Clone, Serialize)]
struct EntryResult {
    run_id: Option<String>,
    spec_path: String,
    output_path: String,
    output_origin: String,
    criteria_count: usize,
    completeness: f64,
    precision: f64,
    summary: String,
    #[serde(default)]
    skipped_reason: Option<String>,
    #[serde(default)]
    criteria: Vec<CriterionResult>,
}

#[derive(Debug, Clone, Serialize)]
struct CriterionResult {
    criterion: String,
    verdict: String,
    evidence: String,
}

#[derive(Debug, Clone, Serialize)]
struct SummaryArtifact {
    dataset_path: Option<String>,
    mode: String,
    provider_label: String,
    generated_at: String,
    metrics: SpecAdherenceMetrics,
    entries: Vec<EntryResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct JudgeEnvelope {
    criteria: Vec<JudgeCriterion>,
    #[serde(default)]
    overall_summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JudgeCriterion {
    criterion: String,
    verdict: String,
    #[serde(default)]
    evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Met,
    Partial,
    Unmet,
}

impl Verdict {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "met" | "pass" | "passed" => Self::Met,
            "partial" | "partially_met" | "partially met" => Self::Partial,
            _ => Self::Unmet,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Met => "met",
            Self::Partial => "partial",
            Self::Unmet => "unmet",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceFileEntry {
    path: String,
    size: u64,
    hash: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceStateSnapshot {
    files: Vec<WorkspaceFileEntry>,
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<SpecAdherenceSuiteOutcome> {
    let dataset_path = read_env("SPEC_ADHERENCE_DATASET", overrides).map(PathBuf::from);
    let limit = parse_env_usize_with_overrides("SPEC_ADHERENCE_LIMIT", overrides)?;
    let min_completeness =
        parse_env_f64_with_overrides("SPEC_ADHERENCE_MIN_COMPLETENESS", overrides)?;
    let mode = SpecAdherenceMode::parse(overrides)?;
    let output_dir = read_env("SPEC_ADHERENCE_OUTPUT", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.artifacts.join("benchmarks").join("spec_adherence"));

    let config = SpecAdherenceRunConfig {
        dataset_path,
        output_dir,
        limit,
        min_completeness,
        mode,
    };

    match run(paths, &config).await {
        Ok(output) => Ok(SpecAdherenceSuiteOutcome::Completed(output)),
        Err(error) => {
            let message = error.to_string();
            if message.contains("No configured provider available")
                || message.contains("No benchmark entries available")
            {
                Ok(SpecAdherenceSuiteOutcome::Skipped(message))
            } else {
                Err(error)
            }
        }
    }
}

pub async fn run(paths: &Paths, config: &SpecAdherenceRunConfig) -> Result<SpecAdherenceRunOutput> {
    let provider_label = paths.setup.resolve_agent_provider_name("scout", "claude");
    let Some(provider) = llm::build_provider("scout", "claude", &paths.setup) else {
        return Ok(SpecAdherenceRunOutput {
            output_dir: config.output_dir.clone(),
            summary_path: config.output_dir.join("spec_adherence_summary.json"),
            markdown_path: config.output_dir.join("spec_adherence_report.md"),
            mode: config.mode,
            provider_label,
            metrics: SpecAdherenceMetrics::default(),
            threshold_failure: Some(
                "No configured provider available for spec-adherence judging via Scout route."
                    .to_string(),
            ),
        });
    };

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| format!("creating {}", config.output_dir.display()))?;

    let entries = if let Some(dataset_path) = &config.dataset_path {
        load_dataset_entries(paths, dataset_path, config.limit)?
    } else {
        let db_limit = config.limit.or(Some(DEFAULT_DB_LIMIT));
        let pool = open_pool(paths).await?;
        load_recent_run_entries(paths, &pool, db_limit).await?
    };

    if entries.is_empty() {
        return Ok(SpecAdherenceRunOutput {
            output_dir: config.output_dir.clone(),
            summary_path: config.output_dir.join("spec_adherence_summary.json"),
            markdown_path: config.output_dir.join("spec_adherence_report.md"),
            mode: config.mode,
            provider_label,
            metrics: SpecAdherenceMetrics::default(),
            threshold_failure: Some(
                "No benchmark entries available. Provide SPEC_ADHERENCE_DATASET or complete more runs."
                    .to_string(),
            ),
        });
    }

    let docs_root = paths.artifacts.join("docs");
    let mut results = Vec::with_capacity(entries.len());
    let mut metrics = SpecAdherenceMetrics {
        total_entries: entries.len(),
        ..Default::default()
    };

    for entry in entries {
        let outcome = evaluate_entry(
            &*provider,
            &docs_root,
            config.mode,
            &entry,
        )
        .await;

        match outcome {
            Ok(result) => {
                if result.skipped_reason.is_some() {
                    metrics.skipped_entries += 1;
                } else {
                    metrics.judged_entries += 1;
                    metrics.total_criteria += result.criteria.len();
                    for criterion in &result.criteria {
                        match Verdict::parse(&criterion.verdict) {
                            Verdict::Met => metrics.met += 1,
                            Verdict::Partial => metrics.partial += 1,
                            Verdict::Unmet => metrics.unmet += 1,
                        }
                    }
                }
                results.push(result);
            }
            Err(error) => {
                metrics.skipped_entries += 1;
                results.push(EntryResult {
                    run_id: entry.run_id.clone(),
                    spec_path: entry.spec_path.display().to_string(),
                    output_path: entry.output_path.display().to_string(),
                    output_origin: entry.output_origin.clone(),
                    criteria_count: 0,
                    completeness: 0.0,
                    precision: 0.0,
                    summary: "entry skipped".to_string(),
                    skipped_reason: Some(error.to_string()),
                    criteria: Vec::new(),
                });
            }
        }
    }

    if metrics.total_criteria > 0 {
        metrics.completeness =
            (metrics.met as f64 + (metrics.partial as f64 * 0.5)) / metrics.total_criteria as f64;
        metrics.precision = metrics.met as f64 / metrics.total_criteria as f64;
    }

    let threshold_failure = config.min_completeness.and_then(|threshold| {
        if metrics.judged_entries == 0 {
            Some("Spec adherence judged zero entries.".to_string())
        } else if metrics.completeness < threshold {
            Some(format!(
                "spec adherence completeness {:.3} below threshold {:.3}",
                metrics.completeness, threshold
            ))
        } else {
            None
        }
    });

    let summary = SummaryArtifact {
        dataset_path: config
            .dataset_path
            .as_ref()
            .map(|path| path.display().to_string()),
        mode: config.mode.as_str().to_string(),
        provider_label: provider_label.clone(),
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        entries: results,
    };

    let summary_path = config.output_dir.join("spec_adherence_summary.json");
    let markdown_path = config.output_dir.join("spec_adherence_report.md");
    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing {}", summary_path.display()))?;
    tokio::fs::write(&markdown_path, render_markdown_summary(&summary))
        .await
        .with_context(|| format!("writing {}", markdown_path.display()))?;

    Ok(SpecAdherenceRunOutput {
        output_dir: config.output_dir.clone(),
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label,
        metrics,
        threshold_failure,
    })
}

pub fn status_for_output(output: &SpecAdherenceRunOutput) -> BenchmarkStatus {
    if output.metrics.judged_entries == 0 {
        BenchmarkStatus::Skipped
    } else if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &SpecAdherenceRunOutput) -> Option<String> {
    if output.metrics.judged_entries == 0 {
        output
            .threshold_failure
            .clone()
            .or_else(|| Some("Spec adherence judged zero entries.".to_string()))
    } else {
        output.threshold_failure.clone()
    }
}

pub fn render_step_stdout(output: &SpecAdherenceRunOutput) -> String {
    format!(
        "SpecAdherence [{}] judged={}/{} criteria={} completeness={:.1}% precision={:.1}%\n",
        output.mode.as_str(),
        output.metrics.judged_entries,
        output.metrics.total_entries,
        output.metrics.total_criteria,
        output.metrics.completeness * 100.0,
        output.metrics.precision * 100.0,
    )
}

async fn evaluate_entry(
    provider: &dyn llm::LlmProvider,
    docs_root: &Path,
    mode: SpecAdherenceMode,
    entry: &EvaluationEntry,
) -> Result<EntryResult> {
    let spec_obj = spec::load_spec(
        entry
            .spec_path
            .to_str()
            .with_context(|| format!("non-utf8 spec path {}", entry.spec_path.display()))?,
    )?;
    let criteria = criteria_for_mode(&spec_obj, mode);
    if criteria.is_empty() {
        bail!("spec contains no usable criteria to judge");
    }

    let artifact_text = read_text_excerpt(&entry.output_path, MAX_ARTIFACT_CHARS)
        .with_context(|| format!("reading output artifact {}", entry.output_path.display()))?;
    let supporting_doc = entry
        .run_id
        .as_ref()
        .and_then(|run_id| {
            let path = docs_root.join(run_id).join("README.md");
            if path.exists() && path != entry.output_path {
                read_text_excerpt(&path, MAX_SUPPORTING_DOC_CHARS).ok()
            } else {
                None
            }
        })
        .unwrap_or_default();

    let request = LlmRequest::simple(
        "You are an exacting software-delivery benchmark judge. Score whether a run artifact satisfies each requested criterion using only the supplied spec excerpt and artifact text. Return valid JSON only. Do not use markdown fences.",
        build_judge_prompt(&spec_obj, mode, entry, &criteria, &artifact_text, &supporting_doc),
    );
    let raw = provider.complete(request).await?.content;
    let envelope = parse_judge_response(&raw)?;

    let mut criterion_results = Vec::with_capacity(criteria.len());
    let mut met = 0usize;
    let mut partial = 0usize;
    for (index, expected) in criteria.iter().enumerate() {
        let judged = envelope.criteria.get(index);
        let verdict = judged
            .map(|item| Verdict::parse(&item.verdict))
            .unwrap_or(Verdict::Unmet);
        let evidence = judged
            .map(|item| truncate_text(&item.evidence, 220))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "judge omitted explicit evidence".to_string());
        if verdict == Verdict::Met {
            met += 1;
        } else if verdict == Verdict::Partial {
            partial += 1;
        }
        criterion_results.push(CriterionResult {
            criterion: judged
                .map(|item| normalize_whitespace(&item.criterion))
                .filter(|criterion| !criterion.is_empty())
                .unwrap_or_else(|| expected.clone()),
            verdict: verdict.as_str().to_string(),
            evidence,
        });
    }

    let criteria_count = criterion_results.len();
    let completeness = if criteria_count == 0 {
        0.0
    } else {
        (met as f64 + partial as f64 * 0.5) / criteria_count as f64
    };
    let precision = if criteria_count == 0 {
        0.0
    } else {
        met as f64 / criteria_count as f64
    };

    Ok(EntryResult {
        run_id: entry.run_id.clone(),
        spec_path: entry.spec_path.display().to_string(),
        output_path: entry.output_path.display().to_string(),
        output_origin: entry.output_origin.clone(),
        criteria_count,
        completeness,
        precision,
        summary: envelope
            .overall_summary
            .map(|value| truncate_text(&value, 260))
            .unwrap_or_else(|| "judge summary unavailable".to_string()),
        skipped_reason: None,
        criteria: criterion_results,
    })
}

fn criteria_for_mode(spec_obj: &Spec, mode: SpecAdherenceMode) -> Vec<String> {
    match mode {
        SpecAdherenceMode::Standard => {
            if spec_obj.acceptance_criteria.is_empty() {
                derive_without_scout_criteria(spec_obj)
            } else {
                spec_obj.acceptance_criteria.clone()
            }
        }
        SpecAdherenceMode::WithoutScout => derive_without_scout_criteria(spec_obj),
    }
}

fn derive_without_scout_criteria(spec_obj: &Spec) -> Vec<String> {
    let mut criteria = Vec::new();
    if !spec_obj.purpose.trim().is_empty() {
        criteria.push(format!("Output advances the stated purpose: {}", spec_obj.purpose));
    }
    for scope in spec_obj.scope.iter().take(6) {
        criteria.push(format!("Addresses scope item: {}", scope));
    }
    for output in spec_obj.outputs.iter().take(4) {
        criteria.push(format!("Produces or updates expected output: {}", output));
    }
    if criteria.is_empty() {
        criteria.push(format!("Run meaningfully addresses spec {}", spec_obj.id));
    }
    criteria
}

fn build_judge_prompt(
    spec_obj: &Spec,
    mode: SpecAdherenceMode,
    entry: &EvaluationEntry,
    criteria: &[String],
    artifact_text: &str,
    supporting_doc: &str,
) -> String {
    let criteria_block = criteria
        .iter()
        .enumerate()
        .map(|(index, criterion)| format!("{}. {}", index + 1, criterion))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Benchmark: spec_adherence
Mode: {}
Run ID: {}
Spec ID: {}
Spec Title: {}
Purpose: {}
Scope:
{}

Declared Outputs:
{}

Criteria To Judge:
{}

Primary Output Artifact: {}
Primary Output Origin: {}

Primary Output Content:
{}

Supporting Run Doc Excerpt:
{}

Return this JSON schema exactly:
{{
  \"criteria\": [
    {{
      \"criterion\": \"repeat the criterion text\",
      \"verdict\": \"met|partial|unmet\",
      \"evidence\": \"brief reason grounded in the artifact text\"
    }}
  ],
  \"overall_summary\": \"one short sentence\"
}}

Rules:
- Judge only from the provided artifact text and supporting excerpt.
- A criterion is `met` only when the artifact gives strong evidence it was satisfied.
- Use `partial` for incomplete, ambiguous, or indirect evidence.
- Use `unmet` when the artifact does not support the criterion.
- Keep `criteria` in the same order and count as the provided list.",
        mode.as_str(),
        entry.run_id.as_deref().unwrap_or("none"),
        spec_obj.id,
        spec_obj.title,
        spec_obj.purpose,
        render_list(&spec_obj.scope, "No scope items declared."),
        render_list(&spec_obj.outputs, "No outputs declared."),
        criteria_block,
        entry.output_path.display(),
        entry.output_origin,
        artifact_text,
        if supporting_doc.trim().is_empty() {
            "(none)".to_string()
        } else {
            supporting_doc.to_string()
        },
    )
}

fn parse_judge_response(raw: &str) -> Result<JudgeEnvelope> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("judge returned empty response");
    }
    let candidate = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        extract_json_object(trimmed).unwrap_or_else(|| trimmed.to_string())
    };
    serde_json::from_str::<JudgeEnvelope>(&candidate)
        .with_context(|| format!("parsing judge JSON: {}", truncate_text(trimmed, 220)))
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| raw[start..=end].to_string())
}

async fn open_pool(paths: &Paths) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.db_file)
        .create_if_missing(false);
    SqlitePool::connect_with(options)
        .await
        .with_context(|| format!("opening sqlite db {}", paths.db_file.display()))
}

fn load_dataset_entries(
    paths: &Paths,
    dataset_path: &Path,
    limit: Option<usize>,
) -> Result<Vec<EvaluationEntry>> {
    if !dataset_path.exists() {
        bail!("dataset not found: {}", dataset_path.display());
    }
    let raw = std::fs::read_to_string(dataset_path)
        .with_context(|| format!("reading dataset {}", dataset_path.display()))?;
    let mut entries = Vec::new();
    for (line_number, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: DatasetEntry = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "parsing spec adherence dataset line {} from {}",
                line_number + 1,
                dataset_path.display()
            )
        })?;
        entries.push(EvaluationEntry {
            run_id: record.run_id,
            spec_path: resolve_repo_path(paths, &record.spec_path),
            output_path: resolve_repo_path(paths, &record.output_path),
            output_origin: "dataset".to_string(),
        });
        if limit.is_some_and(|value| entries.len() >= value) {
            break;
        }
    }
    Ok(entries)
}

async fn load_recent_run_entries(
    paths: &Paths,
    pool: &SqlitePool,
    limit: Option<usize>,
) -> Result<Vec<EvaluationEntry>> {
    let row_limit = limit.unwrap_or(DEFAULT_DB_LIMIT) as i64;
    let rows = sqlx::query(
        "SELECT run_id, status FROM runs WHERE status IN ('completed', 'completed_with_issues') ORDER BY created_at DESC LIMIT ?1",
    )
    .bind(row_limit)
    .fetch_all(pool)
    .await?;

    let mut entries = Vec::new();
    for row in rows {
        let run_id = row.get::<String, _>("run_id");
        let run_dir = paths.workspaces.join(&run_id).join("run");
        let spec_path = run_dir.join("spec.yaml");
        if !spec_path.exists() {
            continue;
        }
        if let Some((output_path, output_origin)) = choose_output_path(paths, pool, &run_id).await? {
            entries.push(EvaluationEntry {
                run_id: Some(run_id),
                spec_path,
                output_path,
                output_origin,
            });
        }
    }
    Ok(entries)
}

async fn choose_output_path(
    paths: &Paths,
    pool: &SqlitePool,
    run_id: &str,
) -> Result<Option<(PathBuf, String)>> {
    let staged_product = paths.workspaces.join(run_id).join("product");
    let docs_readme = paths.artifacts.join("docs").join(run_id).join("README.md");
    let run_dir = paths.workspaces.join(run_id).join("run");

    if staged_product.exists() {
        let changed_files = implementation_changed_files(pool, run_id).await?;
        let mut candidates = changed_files
            .into_iter()
            .map(|relative| {
                let full_path = staged_product.join(&relative);
                (artifact_rank(&relative), relative, full_path)
            })
            .filter(|(_, _, full_path)| full_path.exists())
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (_, relative, full_path) in candidates {
            if looks_like_text_artifact(&relative, &full_path) {
                return Ok(Some((full_path, format!("implementation_changed_file:{relative}"))));
            }
        }

        let staged_readme = staged_product.join("README.md");
        if staged_readme.exists() {
            return Ok(Some((staged_readme, "staged_product_readme".to_string())));
        }
    }

    if docs_readme.exists() {
        return Ok(Some((docs_readme, "flint_docs_readme".to_string())));
    }

    let validation_analysis = run_dir.join("validation_analysis.md");
    if validation_analysis.exists() {
        return Ok(Some((
            validation_analysis,
            "run_validation_analysis".to_string(),
        )));
    }

    let build_output = run_dir.join("build_output.txt");
    if build_output.exists() {
        return Ok(Some((build_output, "run_build_output".to_string())));
    }

    Ok(None)
}

async fn implementation_changed_files(pool: &SqlitePool, run_id: &str) -> Result<Vec<String>> {
    let row = sqlx::query(
        "SELECT state_before, state_after FROM episodes WHERE run_id = ?1 AND phase = 'implementation' ORDER BY started_at DESC LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(Vec::new());
    };
    let before = row.get::<Option<String>, _>("state_before");
    let after = row.get::<Option<String>, _>("state_after");
    let Some(before) = parse_workspace_state_snapshot(before.as_deref()) else {
        return Ok(Vec::new());
    };
    let Some(after) = parse_workspace_state_snapshot(after.as_deref()) else {
        return Ok(Vec::new());
    };

    let before_map = before
        .files
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let after_map = after
        .files
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();

    let mut changed = after_map
        .iter()
        .filter_map(|(path, after_entry)| match before_map.get(path) {
            None => Some((*path).to_string()),
            Some(before_entry) => {
                if before_entry.hash != after_entry.hash || before_entry.size != after_entry.size {
                    Some((*path).to_string())
                } else {
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    changed.sort();
    Ok(changed)
}

fn parse_workspace_state_snapshot(raw: Option<&str>) -> Option<WorkspaceStateSnapshot> {
    raw.and_then(|value| serde_json::from_str::<WorkspaceStateSnapshot>(value).ok())
}

fn artifact_rank(relative: &str) -> i32 {
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with("cargo.lock")
        || lower.ends_with("package-lock.json")
        || lower.ends_with("pnpm-lock.yaml")
        || lower.ends_with(".snap")
    {
        return -1000;
    }
    if lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.contains("/__tests__/")
        || lower.ends_with(".min.js")
    {
        return -400;
    }
    let mut score = 0;
    if lower.ends_with("readme.md") {
        score += 1000;
    }
    if lower.starts_with("docs/") || lower.contains("/docs/") {
        score += 850;
    }
    if lower.contains("api") {
        score += 500;
    }
    if lower.ends_with(".md") {
        score += 700;
    }
    if lower.ends_with(".html") || lower.ends_with(".txt") {
        score += 650;
    }
    if lower.ends_with(".json") || lower.ends_with(".yaml") || lower.ends_with(".yml") {
        score += 500;
    }
    if lower.ends_with(".rs")
        || lower.ends_with(".py")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".css")
        || lower.ends_with(".scss")
        || lower.ends_with(".go")
        || lower.ends_with(".java")
    {
        score += 420;
    }
    score
}

fn looks_like_text_artifact(relative: &str, full_path: &Path) -> bool {
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".ico")
        || lower.ends_with(".pdf")
        || lower.ends_with(".zip")
        || lower.ends_with(".wasm")
        || lower.ends_with(".so")
        || lower.ends_with(".dll")
        || lower.ends_with(".exe")
    {
        return false;
    }
    if let Ok(bytes) = std::fs::read(full_path) {
        !bytes.iter().take(512).any(|byte| *byte == 0)
    } else {
        false
    }
}

fn resolve_repo_path(paths: &Paths, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        paths.root.join(path)
    }
}

fn read_text_excerpt(path: &Path, max_chars: usize) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.iter().take(512).any(|byte| *byte == 0) {
        bail!("artifact appears to be binary: {}", path.display());
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(truncate_text(&text, max_chars))
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let head = max_chars / 2;
    let tail = max_chars.saturating_sub(head + 32);
    let prefix = trimmed.chars().take(head).collect::<String>();
    let suffix = trimmed
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}\n\n...[truncated]...\n\n{suffix}")
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_markdown_summary(summary: &SummaryArtifact) -> String {
    let mut lines = vec![
        "# Spec Adherence Summary".to_string(),
        String::new(),
        format!("- Mode: {}", summary.mode),
        format!("- Judge provider: {}", summary.provider_label),
        format!("- Generated: {}", summary.generated_at),
        format!(
            "- Entries: {} total / {} judged / {} skipped",
            summary.metrics.total_entries,
            summary.metrics.judged_entries,
            summary.metrics.skipped_entries
        ),
        format!("- Criteria judged: {}", summary.metrics.total_criteria),
        format!(
            "- Completeness: {:.1}%",
            summary.metrics.completeness * 100.0
        ),
        format!("- Precision: {:.1}%", summary.metrics.precision * 100.0),
        String::new(),
        "## Entries".to_string(),
    ];
    for entry in &summary.entries {
        lines.push(String::new());
        lines.push(format!(
            "### {}",
            entry.run_id.as_deref().unwrap_or(&entry.output_path)
        ));
        lines.push(format!("- Spec: {}", entry.spec_path));
        lines.push(format!("- Output: {}", entry.output_path));
        lines.push(format!("- Origin: {}", entry.output_origin));
        if let Some(reason) = &entry.skipped_reason {
            lines.push(format!("- Skipped: {}", reason));
            continue;
        }
        lines.push(format!(
            "- Completeness: {:.1}% | Precision: {:.1}% | Criteria: {}",
            entry.completeness * 100.0,
            entry.precision * 100.0,
            entry.criteria_count
        ));
        lines.push(format!("- Summary: {}", entry.summary));
        for criterion in &entry.criteria {
            lines.push(format!(
                "- [{}] {} — {}",
                criterion.verdict, criterion.criterion, criterion.evidence
            ));
        }
    }
    lines.join("\n")
}

fn render_list(values: &[String], empty: &str) -> String {
    if values.is_empty() {
        empty.to_string()
    } else {
        values
            .iter()
            .map(|value| format!("- {}", value))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn parse_boolish(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => bail!("parsing boolean value `{}`", other),
    }
}

fn parse_env_usize_with_overrides(
    name: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<Option<usize>> {
    match read_env(name, overrides) {
        Some(value) => value
            .parse::<usize>()
            .map(Some)
            .with_context(|| format!("parsing {} as usize", name)),
        None => Ok(None),
    }
}

fn parse_env_f64_with_overrides(
    name: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<Option<f64>> {
    match read_env(name, overrides) {
        Some(value) => value
            .parse::<f64>()
            .map(Some)
            .with_context(|| format!("parsing {} as f64", name)),
        None => Ok(None),
    }
}

fn read_env(name: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    overrides
        .get(name)
        .cloned()
        .or_else(|| env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{artifact_rank, derive_without_scout_criteria, parse_boolish, SpecAdherenceMode};
    use crate::models::Spec;

    #[test]
    fn boolish_parser_accepts_common_values() {
        assert!(parse_boolish("true").unwrap());
        assert!(parse_boolish("1").unwrap());
        assert!(!parse_boolish("false").unwrap());
        assert!(!parse_boolish("0").unwrap());
    }

    #[test]
    fn without_scout_criteria_derive_from_spec_shape() {
        let spec = Spec {
            id: "demo".to_string(),
            title: "Demo".to_string(),
            purpose: "Ship a dashboard".to_string(),
            scope: vec!["Add dashboard route".to_string()],
            constraints: Vec::new(),
            inputs: Vec::new(),
            outputs: vec!["README".to_string()],
            acceptance_criteria: vec!["Dashboard works".to_string()],
            forbidden_behaviors: Vec::new(),
            rollback_requirements: Vec::new(),
            dependencies: Vec::new(),
            performance_expectations: Vec::new(),
            security_expectations: Vec::new(),
            twin_services: Vec::new(),
            project_components: Vec::new(),
            scenario_blueprint: None,
            worker_harness: None,
            test_commands: Vec::new(),
        };
        let derived = derive_without_scout_criteria(&spec);
        assert!(derived.iter().any(|line| line.contains("purpose")));
        assert!(derived.iter().any(|line| line.contains("scope item")));
        assert!(derived.iter().any(|line| line.contains("expected output")));
    }

    #[test]
    fn artifact_rank_prefers_readmes_over_locks() {
        assert!(artifact_rank("README.md") > artifact_rank("Cargo.lock"));
        assert!(artifact_rank("docs/API.md") > artifact_rank("tests/app_test.rs"));
        let _ = SpecAdherenceMode::Standard;
    }
}
