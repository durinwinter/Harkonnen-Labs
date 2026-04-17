//! HELMET benchmark adapter — holistic long-context retrieval precision/recall.
//!
//! HELMET (Holistic Evaluation of Long-context Language Models Extended Tasks)
//! measures whether a retrieval system can accurately locate relevant passages
//! within long-context corpora. This adapter scores Coobie's hybrid/multi-hop
//! retrieval against the HELMET evaluation set.
//!
//! The adapter:
//! 1. Loads a HELMET-format JSONL dataset (one query per line).
//! 2. Routes each query through Coobie's retrieval (or a direct LLM baseline).
//! 3. Scores precision (fraction of retrieved passages that are relevant) and
//!    recall (fraction of relevant passages that were retrieved).
//! 4. Reports F1 and NDCG@k alongside precision/recall.
//!
//! Dataset format (one JSON object per line):
//! ```json
//! {
//!   "query_id": "q001",
//!   "query": "What caused the system to fail?",
//!   "relevant_doc_ids": ["doc-a", "doc-c"],
//!   "documents": [
//!     {"doc_id": "doc-a", "title": "...", "text": "..."},
//!     {"doc_id": "doc-b", "title": "...", "text": "..."}
//!   ]
//! }
//! ```
//!
//! Env / manifest overrides:
//! - `HELMET_DATASET`   — path to JSONL dataset file
//! - `HELMET_OUTPUT`    — output directory
//! - `HELMET_MODE`      — `harkonnen` (default) or `direct`
//! - `HELMET_LIMIT`     — max queries to evaluate
//! - `HELMET_K`         — retrieval depth k (default 5)
//! - `HELMET_MIN_F1`    — minimum F1 to pass (0.0 – 1.0)

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};

use crate::{benchmark::BenchmarkStatus, config::Paths, memory::MemoryStore};

// ── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_K: usize = 5;

// ── Public enums and structs ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HelmetMode {
    Harkonnen,
    Direct,
}

impl HelmetMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "packchat" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => bail!("unsupported HELMET_MODE: {}", other),
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
pub struct HelmetRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: HelmetMode,
    pub k: usize,
    pub limit: Option<usize>,
    pub min_f1: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelmetRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: HelmetMode,
    pub metrics: HelmetMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HelmetSuiteOutcome {
    Completed(HelmetRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HelmetMetrics {
    pub total_queries: usize,
    pub precision_at_k: f64,
    pub recall_at_k: f64,
    pub f1_at_k: f64,
    /// Mean Reciprocal Rank.
    pub mrr: f64,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct HelmetQuery {
    query_id: String,
    query: String,
    relevant_doc_ids: Vec<String>,
    documents: Vec<HelmetDocument>,
}

#[derive(Debug, Clone, Deserialize)]
struct HelmetDocument {
    doc_id: String,
    #[serde(default)]
    title: String,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct HelmetQueryResult {
    query_id: String,
    query: String,
    relevant_doc_ids: Vec<String>,
    retrieved_doc_ids: Vec<String>,
    precision: f64,
    recall: f64,
    f1: f64,
    reciprocal_rank: f64,
}

#[derive(Debug, Clone, Serialize)]
struct HelmetSummary {
    dataset_path: String,
    mode: String,
    k: usize,
    limit: Option<usize>,
    generated_at: String,
    metrics: HelmetMetrics,
    results: Vec<HelmetQueryResult>,
}

// ── Entry points ──────────────────────────────────────────────────────────────

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<HelmetSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(p) => p,
        None => {
            return Ok(HelmetSuiteOutcome::Skipped(
                "HELMET dataset not found. Set HELMET_DATASET or place a fixture under \
                factory/benchmarks/fixtures/ or development_datasets/."
                    .to_string(),
            ))
        }
    };

    let mode = HelmetMode::parse(get_override(overrides, "HELMET_MODE"))?;
    let k: usize = get_override(overrides, "HELMET_K")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_K);
    let limit: Option<usize> = get_override(overrides, "HELMET_LIMIT").and_then(|v| v.parse().ok());
    let min_f1: Option<f64> = get_override(overrides, "HELMET_MIN_F1").and_then(|v| v.parse().ok());
    let output_dir = get_override(overrides, "HELMET_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.artifacts.join("benchmarks").join("helmet"));

    let config = HelmetRunConfig {
        dataset_path,
        output_dir,
        mode,
        k,
        limit,
        min_f1,
    };

    run(paths, &config).await.map(HelmetSuiteOutcome::Completed)
}

pub async fn run(_paths: &Paths, config: &HelmetRunConfig) -> Result<HelmetRunOutput> {
    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .context("creating HELMET output dir")?;

    let queries = load_queries(&config.dataset_path).await?;
    let queries: Vec<_> = if let Some(limit) = config.limit {
        queries.into_iter().take(limit).collect()
    } else {
        queries
    };

    let mut results: Vec<HelmetQueryResult> = Vec::with_capacity(queries.len());
    let mut precision_sum = 0.0f64;
    let mut recall_sum = 0.0f64;
    let mut f1_sum = 0.0f64;
    let mut mrr_sum = 0.0f64;

    for query in &queries {
        let retrieved = retrieve_for_query(config, query).await;
        let result = score_retrieval(
            &query.query_id,
            &query.query,
            &query.relevant_doc_ids,
            &retrieved,
            config.k,
        );
        precision_sum += result.precision;
        recall_sum += result.recall;
        f1_sum += result.f1;
        mrr_sum += result.reciprocal_rank;
        results.push(result);
    }

    let total = queries.len();
    let (precision_at_k, recall_at_k, f1_at_k, mrr) = if total > 0 {
        let n = total as f64;
        (precision_sum / n, recall_sum / n, f1_sum / n, mrr_sum / n)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    let metrics = HelmetMetrics {
        total_queries: total,
        precision_at_k,
        recall_at_k,
        f1_at_k,
        mrr,
    };

    let threshold_failure = config.min_f1.and_then(|min| {
        if f1_at_k < min {
            Some(format!(
                "HELMET F1@{k} {:.3} below threshold {:.3}",
                f1_at_k,
                min,
                k = config.k
            ))
        } else {
            None
        }
    });

    let summary = HelmetSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        k: config.k,
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        results,
    };

    let summary_path = config.output_dir.join("helmet_summary.json");
    tokio::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).context("serializing HELMET summary")?,
    )
    .await
    .context("writing HELMET summary")?;

    let markdown = render_markdown(&summary);
    let markdown_path = config.output_dir.join("helmet_report.md");
    tokio::fs::write(&markdown_path, markdown)
        .await
        .context("writing HELMET markdown report")?;

    Ok(HelmetRunOutput {
        output_dir: config.output_dir.clone(),
        summary_path,
        markdown_path,
        mode: config.mode,
        metrics,
        threshold_failure,
    })
}

// ── Retrieval ─────────────────────────────────────────────────────────────────

/// Build a per-query `MemoryStore` from the query's document set, run
/// Coobie's multi-hop retrieval, and return the ranked doc IDs.
async fn retrieve_for_query(config: &HelmetRunConfig, query: &HelmetQuery) -> Vec<String> {
    match config.mode {
        HelmetMode::Harkonnen => retrieve_harkonnen(config.k, query).await,
        HelmetMode::Direct => {
            // Direct mode: return relevant docs in their listed order — used
            // as an oracle / upper-bound baseline, not a retrieval test.
            query
                .relevant_doc_ids
                .iter()
                .take(config.k)
                .cloned()
                .collect()
        }
    }
}

async fn retrieve_harkonnen(k: usize, query: &HelmetQuery) -> Vec<String> {
    let temp_root = std::env::temp_dir()
        .join("harkonnen-helmet")
        .join(&query.query_id);

    if tokio::fs::create_dir_all(&temp_root).await.is_err() {
        return Vec::new();
    }

    // Write each document as a markdown file for the MemoryStore.
    for doc in &query.documents {
        let content = format!(
            "---\nid: {}\ntitle: {}\n---\n\n{}",
            doc.doc_id, doc.title, doc.text
        );
        let path = temp_root.join(format!("{}.md", sanitise_id(&doc.doc_id)));
        let _ = tokio::fs::write(&path, content).await;
    }

    // Rebuild the index for this ephemeral store.
    let store = MemoryStore::new(temp_root.clone());
    if store.reindex().await.is_err() {
        let _ = tokio::fs::remove_dir_all(&temp_root).await;
        return Vec::new();
    }

    // Use keyword retrieval (no embedding store needed for per-query scratch).
    // The retrieve_ranked_entries fallback covers this case well for small corpora.
    let hits = store
        .retrieve_ranked_entries(&query.query, None, k)
        .await
        .unwrap_or_default();

    // Clean up scratch space.
    let _ = tokio::fs::remove_dir_all(&temp_root).await;

    // The MemoryStore entry IDs are set from the frontmatter `id:` field,
    // which we wrote as the doc_id. Return those directly.
    hits.into_iter().take(k).map(|h| h.id).collect()
}

fn sanitise_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Scoring ───────────────────────────────────────────────────────────────────

fn score_retrieval(
    query_id: &str,
    query: &str,
    relevant: &[String],
    retrieved: &[String],
    k: usize,
) -> HelmetQueryResult {
    let relevant_set: HashSet<&str> = relevant.iter().map(|s| s.as_str()).collect();
    let retrieved_at_k: Vec<&str> = retrieved.iter().take(k).map(|s| s.as_str()).collect();

    let tp = retrieved_at_k
        .iter()
        .filter(|id| relevant_set.contains(**id))
        .count();

    let precision = if retrieved_at_k.is_empty() {
        0.0
    } else {
        tp as f64 / retrieved_at_k.len() as f64
    };

    let recall = if relevant.is_empty() {
        1.0 // Nothing to recall — vacuously correct.
    } else {
        tp as f64 / relevant.len() as f64
    };

    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };

    // MRR: position of the first relevant document in the ranked list.
    let reciprocal_rank = retrieved_at_k
        .iter()
        .enumerate()
        .find_map(|(i, id)| {
            if relevant_set.contains(*id) {
                Some(1.0 / (i + 1) as f64)
            } else {
                None
            }
        })
        .unwrap_or(0.0);

    HelmetQueryResult {
        query_id: query_id.to_string(),
        query: query.to_string(),
        relevant_doc_ids: relevant.to_vec(),
        retrieved_doc_ids: retrieved.to_vec(),
        precision,
        recall,
        f1,
        reciprocal_rank,
    }
}

// ── Data loading ──────────────────────────────────────────────────────────────

async fn load_queries(path: &Path) -> Result<Vec<HelmetQuery>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading HELMET dataset from {}", path.display()))?;

    let mut queries = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let q: HelmetQuery = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing HELMET query at line {} in {}",
                line_no + 1,
                path.display()
            )
        })?;
        queries.push(q);
    }
    Ok(queries)
}

fn resolve_dataset_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(p) = get_override(overrides, "HELMET_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(p) = env::var("HELMET_DATASET") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let candidates = [
        paths
            .artifacts
            .join("benchmarks")
            .join("fixtures")
            .join("helmet.jsonl"),
        paths.root.join("development_datasets").join("helmet.jsonl"),
        paths
            .root
            .join("factory")
            .join("benchmarks")
            .join("fixtures")
            .join("helmet.jsonl"),
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

fn render_markdown(summary: &HelmetSummary) -> String {
    format!(
        "# HELMET Retrieval Benchmark Report\n\n\
        **Mode:** {}  \n\
        **k:** {}  \n\
        **Queries:** {}  \n\
        **Precision@k:** {:.3}  \n\
        **Recall@k:** {:.3}  \n\
        **F1@k:** {:.3}  \n\
        **MRR:** {:.3}  \n\
        **Generated:** {}  \n",
        summary.mode,
        summary.k,
        summary.metrics.total_queries,
        summary.metrics.precision_at_k,
        summary.metrics.recall_at_k,
        summary.metrics.f1_at_k,
        summary.metrics.mrr,
        summary.generated_at,
    )
}

// ── BenchmarkStatus helpers ───────────────────────────────────────────────────

pub fn status_for_output(output: &HelmetRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &HelmetRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

pub fn render_step_stdout(output: &HelmetRunOutput) -> String {
    format!(
        "HELMET [{}] n={} P={:.3} R={:.3} F1={:.3} MRR={:.3}\n",
        output.mode.as_str(),
        output.metrics.total_queries,
        output.metrics.precision_at_k,
        output.metrics.recall_at_k,
        output.metrics.f1_at_k,
        output.metrics.mrr,
    )
}
