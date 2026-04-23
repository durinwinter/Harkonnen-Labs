use anyhow::{bail, Context, Result};
use chrono::{Datelike, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::{
    benchmark::BenchmarkStatus,
    chat::{self, ChatMessage},
    config::Paths,
    llm::{self, LlmRequest, Message},
    memory::{MemoryProvenance, MemoryRetrievalHit, MemoryStore},
};

const DEFAULT_AGENT: &str = "coobie";
const DEFAULT_DIRECT_CONTEXT_CHAR_LIMIT: usize = 16_000;
const MIN_DIRECT_CONTEXT_CHAR_LIMIT: usize = 1_600;
const DEFAULT_RETRIEVAL_LIMIT: usize = 6;
const DEFAULT_CONTEXT_LIMIT: usize = 4;
const DEFAULT_ABSTENTION: &str = "I do not know.";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingQaMode {
    Harkonnen,
    Direct,
}

impl StreamingQaMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "packchat" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => bail!("unsupported STREAMINGQA_MODE: {}", other),
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
pub struct StreamingQaRunConfig {
    pub dataset_path: PathBuf,
    pub docs_path: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub mode: StreamingQaMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub min_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamingQaRunOutput {
    pub output_dir: PathBuf,
    pub predictions_path: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: StreamingQaMode,
    pub provider_label: String,
    pub metrics: StreamingQaMetrics,
    pub persistence: StreamingQaPersistenceSummary,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StreamingQaSuiteOutcome {
    Completed(StreamingQaRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamingQaMetrics {
    pub total_questions: usize,
    pub accuracy: f64,
    pub exact_match: f64,
    pub token_f1: f64,
    pub evidence_hit_rate: f64,
    pub answered_rate: f64,
    pub by_recency: BTreeMap<String, StreamingQaSliceMetrics>,
    pub by_origin: BTreeMap<String, StreamingQaSliceMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamingQaPersistenceSummary {
    pub memory_updates_db_path: String,
    pub persisted_supersession_events: usize,
    pub questions_requiring_supersession_history: usize,
    pub accuracy_on_updated_facts: f64,
    pub exact_match_on_updated_facts: f64,
    pub evidence_hit_rate_on_updated_facts: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamingQaSliceMetrics {
    pub total_questions: usize,
    pub accuracy: f64,
    pub exact_match: f64,
    pub token_f1: f64,
    pub evidence_hit_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
struct StreamingQaSummary {
    dataset_path: String,
    docs_path: Option<String>,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: StreamingQaMetrics,
    persistence: StreamingQaPersistenceSummary,
    predictions_path: String,
    questions: Vec<StreamingQaQuestionResult>,
}

#[derive(Debug, Clone, Serialize)]
struct StreamingQaQuestionResult {
    qa_id: String,
    question: String,
    expected_answers: Vec<String>,
    raw_hypothesis: String,
    hypothesis: String,
    accuracy: bool,
    exact_match: bool,
    token_f1: f64,
    evidence_hit: bool,
    requires_supersession_history: bool,
    recent_or_past: String,
    written_or_generated: String,
    question_ts: i64,
    evidence_ts: i64,
    evidence_id: String,
    contexts_used: Vec<String>,
    retrieval_trace: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StreamingQaPrediction<'a> {
    qa_id: &'a str,
    hypothesis: &'a str,
}

#[derive(Debug, Clone)]
struct StreamingQaQuestion {
    qa_id: String,
    question: String,
    answers: Vec<String>,
    question_ts: i64,
    evidence_ts: i64,
    evidence_id: String,
    recent_or_past: String,
    written_or_generated: String,
}

#[derive(Debug, Clone)]
struct StreamingQaDocument {
    id: String,
    title: String,
    text: String,
    publication_ts: i64,
    supersedes: Vec<String>,
}

#[derive(Debug, Default)]
struct StreamingQaAccumulator {
    total_questions: usize,
    accuracy_sum: f64,
    exact_match_sum: f64,
    token_f1_sum: f64,
    evidence_hit_sum: f64,
    answered_total: usize,
}

impl StreamingQaAccumulator {
    fn add(&mut self, result: &StreamingQaQuestionResult) {
        self.total_questions += 1;
        if result.accuracy {
            self.accuracy_sum += 1.0;
        }
        if result.exact_match {
            self.exact_match_sum += 1.0;
        }
        self.token_f1_sum += result.token_f1;
        if result.evidence_hit {
            self.evidence_hit_sum += 1.0;
        }
        if !result.hypothesis.trim().is_empty() {
            self.answered_total += 1;
        }
    }

    fn finalize(&self) -> StreamingQaSliceMetrics {
        StreamingQaSliceMetrics {
            total_questions: self.total_questions,
            accuracy: ratio(self.accuracy_sum, self.total_questions),
            exact_match: ratio(self.exact_match_sum, self.total_questions),
            token_f1: ratio(self.token_f1_sum, self.total_questions),
            evidence_hit_rate: ratio(self.evidence_hit_sum, self.total_questions),
        }
    }
}

#[derive(Debug, Clone)]
struct RetrievedStreamingContext {
    document: StreamingQaDocument,
    hit: MemoryRetrievalHit,
    query: String,
}

#[derive(Debug, Clone)]
struct StreamingQaAnswer {
    raw_hypothesis: String,
    contexts_used: Vec<String>,
    retrieval_trace: Vec<String>,
    retrieved_ids: Vec<String>,
    evidence_hit: bool,
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<StreamingQaSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(path) => path,
        None => {
            return Ok(StreamingQaSuiteOutcome::Skipped(
                "StreamingQA dataset not found. Set STREAMINGQA_DATASET or place a fixture under factory/benchmarks/fixtures/ or development_datasets/.".to_string(),
            ))
        }
    };

    let docs_path = resolve_docs_path(paths, overrides);
    let mode = StreamingQaMode::parse(read_env("STREAMINGQA_MODE", overrides))?;
    let output_dir = read_env("STREAMINGQA_OUTPUT_DIR", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            paths.artifacts.join("benchmarks").join(format!(
                "streamingqa-{}-{}",
                mode.as_str(),
                Utc::now().timestamp_millis()
            ))
        });

    let config = StreamingQaRunConfig {
        dataset_path,
        docs_path,
        output_dir,
        mode,
        agent: read_env("STREAMINGQA_AGENT", overrides)
            .unwrap_or_else(|| DEFAULT_AGENT.to_string()),
        direct_provider: read_env("STREAMINGQA_DIRECT_PROVIDER", overrides)
            .unwrap_or_else(|| "default".to_string()),
        limit: parse_env_usize_with_overrides("STREAMINGQA_LIMIT", overrides)?,
        min_accuracy: parse_env_f64_with_overrides("STREAMINGQA_MIN_ACCURACY", overrides)?,
    };

    Ok(StreamingQaSuiteOutcome::Completed(
        run(paths, &config).await?,
    ))
}

pub async fn run(paths: &Paths, config: &StreamingQaRunConfig) -> Result<StreamingQaRunOutput> {
    let (mut documents, mut questions) =
        load_dataset(&config.dataset_path, config.docs_path.as_deref()).await?;
    if let Some(limit) = config.limit {
        questions.truncate(limit);
    }
    if questions.is_empty() {
        bail!("StreamingQA dataset contains no questions")
    }
    if documents.is_empty() {
        bail!(
            "StreamingQA requires document context. Provide inline documents in the dataset fixture or set STREAMINGQA_DOCS to a JSON/JSONL export of WMT documents."
        )
    }

    documents.sort_by(|left, right| {
        left.publication_ts
            .cmp(&right.publication_ts)
            .then_with(|| left.id.cmp(&right.id))
    });
    questions.sort_by(|left, right| {
        left.question_ts
            .cmp(&right.question_ts)
            .then_with(|| left.qa_id.cmp(&right.qa_id))
    });

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| {
            format!(
                "creating StreamingQA output dir {}",
                config.output_dir.display()
            )
        })?;

    let predictions_path = config.output_dir.join("predictions.jsonl");
    let summary_path = config.output_dir.join("summary.json");
    let markdown_path = config.output_dir.join("summary.md");
    let memory_updates_db_path = config.output_dir.join("memory-updates.db");
    let memory_root = config.output_dir.join("temporal-memory");
    if memory_updates_db_path.exists() {
        let _ = tokio::fs::remove_file(&memory_updates_db_path).await;
    }
    if memory_root.exists() {
        let _ = tokio::fs::remove_dir_all(&memory_root).await;
    }
    tokio::fs::create_dir_all(memory_root.join("imports"))
        .await
        .with_context(|| format!("creating StreamingQA memory dir {}", memory_root.display()))?;
    let store = MemoryStore::new(memory_root.clone());
    let persistence_db = init_memory_updates_db(&memory_updates_db_path).await?;

    let mut document_by_id = HashMap::new();
    for document in &documents {
        document_by_id.insert(document.id.clone(), document.clone());
    }

    let mut predictions = Vec::with_capacity(questions.len());
    let mut results = Vec::with_capacity(questions.len());
    let mut accumulator = StreamingQaAccumulator::default();
    let mut by_recency: HashMap<String, StreamingQaAccumulator> = HashMap::new();
    let mut by_origin: HashMap<String, StreamingQaAccumulator> = HashMap::new();
    let mut update_questions = StreamingQaAccumulator::default();
    let verbose_progress = benchmark_verbose_progress();
    let show_raw_output = benchmark_show_raw_output();
    let mut doc_cursor = 0usize;
    let mut loaded_doc_memory_ids: HashMap<String, String> = HashMap::new();

    for (index, question) in questions.iter().enumerate() {
        ingest_available_documents(
            &store,
            &documents,
            &mut doc_cursor,
            question.question_ts,
            &mut loaded_doc_memory_ids,
            &persistence_db,
            &memory_root,
        )
        .await?;

        if verbose_progress {
            eprintln!(
                "[StreamingQA][{}/{}][{}] answering {} with {} docs available",
                index + 1,
                questions.len(),
                config.mode.as_str(),
                question.qa_id,
                loaded_doc_memory_ids.len()
            );
        }

        let answer = if loaded_doc_memory_ids.is_empty() {
            StreamingQaAnswer {
                raw_hypothesis: DEFAULT_ABSTENTION.to_string(),
                contexts_used: Vec::new(),
                retrieval_trace: vec!["no_documents_available_before_question_ts".to_string()],
                retrieved_ids: Vec::new(),
                evidence_hit: false,
            }
        } else {
            match config.mode {
                StreamingQaMode::Harkonnen => {
                    answer_with_harkonnen(paths, config, question, &store, &document_by_id).await?
                }
                StreamingQaMode::Direct => {
                    answer_with_direct(paths, config, question, &store, &document_by_id).await?
                }
            }
        };

        let requires_supersession_history =
            question_requires_supersession_history(question, &document_by_id);
        let result = evaluate_question(
            question,
            answer.raw_hypothesis,
            answer.contexts_used,
            answer.retrieval_trace,
            answer.evidence_hit,
            requires_supersession_history,
        );
        if !answer.retrieved_ids.is_empty() {
            store.mark_entries_loaded(&answer.retrieved_ids).await?;
            store
                .record_outcome(&answer.retrieved_ids, result.accuracy)
                .await?;
        }

        if verbose_progress {
            eprintln!(
                "[StreamingQA][{}/{}][{}] done {} accuracy={} exact_match={} evidence_hit={} token_f1={:.4}",
                index + 1,
                questions.len(),
                config.mode.as_str(),
                result.qa_id,
                result.accuracy,
                result.exact_match,
                result.evidence_hit,
                result.token_f1
            );
            eprintln!(
                "[StreamingQA][{}] answer: {}",
                result.qa_id, result.hypothesis
            );
            if show_raw_output {
                eprintln!(
                    "[StreamingQA][{}] raw output:\n{}\n---",
                    result.qa_id, result.raw_hypothesis
                );
            }
        }

        accumulator.add(&result);
        by_recency
            .entry(normalize_slice_key(&result.recent_or_past))
            .or_default()
            .add(&result);
        by_origin
            .entry(normalize_slice_key(&result.written_or_generated))
            .or_default()
            .add(&result);
        if result.requires_supersession_history {
            update_questions.add(&result);
        }
        predictions.push(serde_json::to_string(&StreamingQaPrediction {
            qa_id: &result.qa_id,
            hypothesis: &result.hypothesis,
        })?);
        results.push(result);
    }

    let metrics = StreamingQaMetrics {
        total_questions: accumulator.total_questions,
        accuracy: ratio(accumulator.accuracy_sum, accumulator.total_questions),
        exact_match: ratio(accumulator.exact_match_sum, accumulator.total_questions),
        token_f1: ratio(accumulator.token_f1_sum, accumulator.total_questions),
        evidence_hit_rate: ratio(accumulator.evidence_hit_sum, accumulator.total_questions),
        answered_rate: ratio(
            accumulator.answered_total as f64,
            accumulator.total_questions,
        ),
        by_recency: finalize_slices(by_recency),
        by_origin: finalize_slices(by_origin),
    };
    let persistence = StreamingQaPersistenceSummary {
        memory_updates_db_path: memory_updates_db_path.display().to_string(),
        persisted_supersession_events: count_memory_updates(&persistence_db).await?,
        questions_requiring_supersession_history: update_questions.total_questions,
        accuracy_on_updated_facts: ratio(
            update_questions.accuracy_sum,
            update_questions.total_questions,
        ),
        exact_match_on_updated_facts: ratio(
            update_questions.exact_match_sum,
            update_questions.total_questions,
        ),
        evidence_hit_rate_on_updated_facts: ratio(
            update_questions.evidence_hit_sum,
            update_questions.total_questions,
        ),
    };

    tokio::fs::write(&predictions_path, format!("{}\n", predictions.join("\n")))
        .await
        .with_context(|| {
            format!(
                "writing StreamingQA predictions {}",
                predictions_path.display()
            )
        })?;

    let summary = StreamingQaSummary {
        dataset_path: config.dataset_path.display().to_string(),
        docs_path: config
            .docs_path
            .as_ref()
            .map(|path| path.display().to_string()),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label(config),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        persistence: persistence.clone(),
        predictions_path: predictions_path.display().to_string(),
        questions: results,
    };

    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing StreamingQA summary {}", summary_path.display()))?;
    tokio::fs::write(&markdown_path, render_markdown_summary(&summary))
        .await
        .with_context(|| format!("writing StreamingQA markdown {}", markdown_path.display()))?;

    let threshold_failure = threshold_failure(&metrics, &persistence, config);

    Ok(StreamingQaRunOutput {
        output_dir: config.output_dir.clone(),
        predictions_path,
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label: provider_label(config),
        metrics,
        persistence,
        threshold_failure,
    })
}

pub fn render_step_stdout(output: &StreamingQaRunOutput) -> String {
    let mut lines = vec![
        format!("StreamingQA mode: {}", output.mode.as_str()),
        format!("Provider path: {}", output.provider_label),
        format!("StreamingQA output dir: {}", output.output_dir.display()),
        format!("Predictions: {}", output.predictions_path.display()),
        format!("Summary JSON: {}", output.summary_path.display()),
        format!("Summary Markdown: {}", output.markdown_path.display()),
        format!("Questions: {}", output.metrics.total_questions),
        format!("Accuracy: {:.4}", output.metrics.accuracy),
        format!("Exact match: {:.4}", output.metrics.exact_match),
        format!("Token F1: {:.4}", output.metrics.token_f1),
        format!("Evidence hit rate: {:.4}", output.metrics.evidence_hit_rate),
        format!(
            "Memory updates DB: {}",
            output.persistence.memory_updates_db_path
        ),
        format!(
            "Persisted supersession events: {}",
            output.persistence.persisted_supersession_events
        ),
        format!(
            "Updated-fact questions: {}",
            output.persistence.questions_requiring_supersession_history
        ),
        format!(
            "Updated-fact accuracy: {:.4}",
            output.persistence.accuracy_on_updated_facts
        ),
    ];
    if let Some(reason) = &output.threshold_failure {
        lines.push(format!("Threshold failure: {}", reason));
    }
    lines.join("\n")
}

pub fn status_for_output(output: &StreamingQaRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &StreamingQaRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

async fn answer_with_harkonnen(
    paths: &Paths,
    config: &StreamingQaRunConfig,
    question: &StreamingQaQuestion,
    store: &MemoryStore,
    document_by_id: &HashMap<String, StreamingQaDocument>,
) -> Result<StreamingQaAnswer> {
    let retrieved = retrieve_ranked_contexts(store, document_by_id, &question.question).await?;
    let selected_contexts = contexts_from_hits(&retrieved, DEFAULT_CONTEXT_LIMIT);
    if selected_contexts.is_empty() {
        return Ok(StreamingQaAnswer {
            raw_hypothesis: DEFAULT_ABSTENTION.to_string(),
            contexts_used: Vec::new(),
            retrieval_trace: vec!["no_retrieval_hits".to_string()],
            retrieved_ids: Vec::new(),
            evidence_hit: false,
        });
    }

    let retrieval_trace = retrieval_trace(&retrieved);
    let history = build_context_history(&question.qa_id, &config.agent, &selected_contexts);
    let prompt = build_harkonnen_prompt(question, &retrieval_trace);
    let history = history_with_prompt(&history, &question.qa_id, &config.agent, &prompt);
    let reply = chat::complete_agent_reply(&config.agent, &prompt, &history, None, paths)
        .await
        .with_context(|| {
            format!(
                "answering StreamingQA question {} via PackChat",
                question.qa_id
            )
        })?;

    Ok(StreamingQaAnswer {
        raw_hypothesis: reply,
        contexts_used: selected_contexts.iter().map(context_label).collect(),
        retrieval_trace,
        retrieved_ids: retrieved.iter().map(|hit| hit.hit.id.clone()).collect(),
        evidence_hit: retrieved
            .iter()
            .any(|hit| hit.document.id == question.evidence_id),
    })
}

async fn answer_with_direct(
    paths: &Paths,
    config: &StreamingQaRunConfig,
    question: &StreamingQaQuestion,
    store: &MemoryStore,
    document_by_id: &HashMap<String, StreamingQaDocument>,
) -> Result<StreamingQaAnswer> {
    let retrieved = retrieve_ranked_contexts(store, document_by_id, &question.question).await?;
    let selected_contexts = contexts_from_hits(&retrieved, DEFAULT_CONTEXT_LIMIT);
    if selected_contexts.is_empty() {
        return Ok(StreamingQaAnswer {
            raw_hypothesis: DEFAULT_ABSTENTION.to_string(),
            contexts_used: Vec::new(),
            retrieval_trace: vec!["no_retrieval_hits".to_string()],
            retrieved_ids: Vec::new(),
            evidence_hit: false,
        });
    }

    let retrieval_trace = retrieval_trace(&retrieved);
    let provider = llm::build_provider("benchmark", &config.direct_provider, &paths.setup)
        .with_context(|| {
            format!(
                "no configured provider available for StreamingQA direct mode via {}",
                config.direct_provider
            )
        })?;

    let mut context_budget = direct_context_char_limit();
    loop {
        let req = LlmRequest {
            messages: vec![Message::user(build_direct_prompt(
                question,
                &selected_contexts,
                context_budget,
            ))],
            max_tokens: 256,
            temperature: 0.0,
        };

        match provider.complete(req).await {
            Ok(resp) => {
                return Ok(StreamingQaAnswer {
                    raw_hypothesis: resp.content,
                    contexts_used: selected_contexts.iter().map(context_label).collect(),
                    retrieval_trace,
                    retrieved_ids: retrieved.iter().map(|hit| hit.hit.id.clone()).collect(),
                    evidence_hit: retrieved
                        .iter()
                        .any(|hit| hit.document.id == question.evidence_id),
                })
            }
            Err(err)
                if is_context_window_error(&err)
                    && context_budget > MIN_DIRECT_CONTEXT_CHAR_LIMIT =>
            {
                let next_budget = (context_budget / 2).max(MIN_DIRECT_CONTEXT_CHAR_LIMIT);
                if benchmark_verbose_progress() {
                    eprintln!(
                        "[StreamingQA][direct] context retry {} budget {} -> {} chars",
                        question.qa_id, context_budget, next_budget
                    );
                }
                if next_budget == context_budget {
                    return Err(err).with_context(|| {
                        format!(
                            "StreamingQA direct provider failed via {}",
                            config.direct_provider
                        )
                    });
                }
                context_budget = next_budget;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "StreamingQA direct provider failed via {}",
                        config.direct_provider
                    )
                });
            }
        }
    }
}

async fn retrieve_ranked_contexts(
    store: &MemoryStore,
    document_by_id: &HashMap<String, StreamingQaDocument>,
    query: &str,
) -> Result<Vec<RetrievedStreamingContext>> {
    let hits = store
        .retrieve_ranked_entries(query, None, DEFAULT_RETRIEVAL_LIMIT)
        .await?;
    let mut retrieved = Vec::new();
    for hit in hits {
        if let Some(document) = document_by_id.get(&hit.id).cloned() {
            retrieved.push(RetrievedStreamingContext {
                document,
                query: query.to_string(),
                hit,
            });
        }
    }
    Ok(retrieved)
}

async fn ingest_available_documents(
    store: &MemoryStore,
    documents: &[StreamingQaDocument],
    doc_cursor: &mut usize,
    question_ts: i64,
    loaded_doc_memory_ids: &mut HashMap<String, String>,
    persistence_db: &SqlitePool,
    memory_root: &Path,
) -> Result<()> {
    while *doc_cursor < documents.len() && documents[*doc_cursor].publication_ts <= question_ts {
        let document = &documents[*doc_cursor];
        let memory_id = document.id.clone();
        let mut tags = vec![
            "streamingqa".to_string(),
            "wmt".to_string(),
            format!("year:{}", timestamp_year(document.publication_ts)),
        ];
        for token in retrieval_tokens(&document.title, 4, 6) {
            if !tags.iter().any(|existing| existing == &token) {
                tags.push(token);
            }
        }
        let provenance = MemoryProvenance {
            source_label: Some(document.title.clone()),
            source_kind: Some("streamingqa_doc".to_string()),
            source_path: Some(document.id.clone()),
            ..MemoryProvenance::default()
        };
        store
            .store_with_metadata(
                &memory_id,
                tags,
                &document.title,
                &document.text,
                provenance,
            )
            .await?;
        loaded_doc_memory_ids.insert(document.id.clone(), memory_id.clone());

        for old_id in &document.supersedes {
            if let Some(old_memory_id) = loaded_doc_memory_ids.get(old_id) {
                store
                    .annotate_entry_status(old_memory_id, "superseded", Some(&memory_id))
                    .await?;
                persist_memory_update(
                    persistence_db,
                    old_memory_id,
                    &memory_id,
                    memory_root,
                    &format!("StreamingQA document {} supersedes {}", document.id, old_id),
                )
                .await?;
            }
        }
        *doc_cursor += 1;
    }
    Ok(())
}

async fn load_dataset(
    dataset_path: &Path,
    docs_path: Option<&Path>,
) -> Result<(Vec<StreamingQaDocument>, Vec<StreamingQaQuestion>)> {
    let raw = tokio::fs::read_to_string(dataset_path)
        .await
        .with_context(|| format!("reading StreamingQA dataset {}", dataset_path.display()))?;
    match dataset_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => load_dataset_from_json(&raw, dataset_path),
        "jsonl" | "ndjson" => {
            let questions = load_questions_from_jsonl(&raw, dataset_path)?;
            let docs_path = docs_path.context(
                "StreamingQA JSONL question files require STREAMINGQA_DOCS pointing to a JSON or JSONL WMT document export.",
            )?;
            let documents = load_documents(docs_path).await?;
            Ok((documents, questions))
        }
        other => bail!("unsupported StreamingQA dataset extension `{}`", other),
    }
}

fn load_dataset_from_json(
    raw: &str,
    dataset_path: &Path,
) -> Result<(Vec<StreamingQaDocument>, Vec<StreamingQaQuestion>)> {
    let value: Value = serde_json::from_str(raw)
        .with_context(|| format!("parsing StreamingQA dataset {}", dataset_path.display()))?;
    if let Some(obj) = value.as_object() {
        let documents = obj
            .get("documents")
            .and_then(Value::as_array)
            .map(|items| parse_documents(items))
            .transpose()?
            .unwrap_or_default();
        let questions = obj
            .get("questions")
            .and_then(Value::as_array)
            .map(|items| parse_questions(items))
            .transpose()?
            .unwrap_or_default();
        return Ok((documents, questions));
    }
    bail!("StreamingQA JSON fixtures must be objects with `documents` and `questions` arrays")
}

async fn load_documents(path: &Path) -> Result<Vec<StreamingQaDocument>> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading StreamingQA docs {}", path.display()))?;
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => {
            let value: Value = serde_json::from_str(&raw)
                .with_context(|| format!("parsing StreamingQA docs {}", path.display()))?;
            if let Some(array) = value.as_array() {
                return parse_documents(array);
            }
            if let Some(array) = value.get("documents").and_then(Value::as_array) {
                return parse_documents(array);
            }
            bail!("StreamingQA docs JSON must be an array or an object with `documents`")
        }
        "jsonl" | "ndjson" => {
            let mut documents = Vec::new();
            for (index, line) in raw.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let value: Value = serde_json::from_str(line).with_context(|| {
                    format!(
                        "parsing StreamingQA docs {} line {}",
                        path.display(),
                        index + 1
                    )
                })?;
                documents.push(parse_document_value(&value, index)?);
            }
            Ok(documents)
        }
        other => bail!("unsupported StreamingQA docs extension `{}`", other),
    }
}

fn parse_documents(values: &[Value]) -> Result<Vec<StreamingQaDocument>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_document_value(value, index))
        .collect()
}

fn parse_questions(values: &[Value]) -> Result<Vec<StreamingQaQuestion>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_question_value(value, index))
        .collect()
}

fn load_questions_from_jsonl(raw: &str, path: &Path) -> Result<Vec<StreamingQaQuestion>> {
    let mut questions = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).with_context(|| {
            format!(
                "parsing StreamingQA jsonl {} line {}",
                path.display(),
                index + 1
            )
        })?;
        questions.push(parse_question_value(&value, index)?);
    }
    Ok(questions)
}

fn parse_question_value(value: &Value, index: usize) -> Result<StreamingQaQuestion> {
    let question = required_string_field(value, &["question"])
        .with_context(|| format!("StreamingQA question {} missing question text", index))?;
    let mut answers = string_list_field(value, &["answers"]);
    if answers.is_empty() {
        if let Some(answer) = optional_string_field(value, &["answer"]) {
            answers.push(answer);
        }
    }
    extend_unique_strings(
        &mut answers,
        string_list_field(value, &["answers_additional"]),
    );
    if answers.is_empty() {
        bail!("StreamingQA question {} has no reference answers", index)
    }

    Ok(StreamingQaQuestion {
        qa_id: optional_string_field(value, &["qa_id"])
            .unwrap_or_else(|| format!("streamingqa-{}", index)),
        question,
        answers,
        question_ts: optional_i64_field(value, &["question_ts"]).unwrap_or(0),
        evidence_ts: optional_i64_field(value, &["evidence_ts"]).unwrap_or(0),
        evidence_id: optional_string_field(value, &["evidence_id"]).unwrap_or_default(),
        recent_or_past: optional_string_field(value, &["recent_or_past"])
            .unwrap_or_else(|| "unknown".to_string()),
        written_or_generated: optional_string_field(value, &["written_or_generated"])
            .unwrap_or_else(|| "unknown".to_string()),
    })
}

fn parse_document_value(value: &Value, index: usize) -> Result<StreamingQaDocument> {
    let id = required_string_field(value, &["id", "sorting_key", "evidence_id"])
        .with_context(|| format!("StreamingQA document {} missing id", index))?;
    let title = optional_string_field(value, &["title", "headline", "summary"])
        .unwrap_or_else(|| format!("Document {}", id));
    let text = required_string_field(value, &["text", "content", "body"])
        .with_context(|| format!("StreamingQA document {} missing text", id))?;
    Ok(StreamingQaDocument {
        id,
        title,
        text,
        publication_ts: optional_i64_field(value, &["publication_ts", "evidence_ts"]).unwrap_or(0),
        supersedes: string_list_field(value, &["supersedes"]),
    })
}

fn optional_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        let Some(raw) = object.get(*key) else {
            continue;
        };
        let parsed = match raw {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(num) => Some(num.to_string()),
            _ => None,
        };
        if let Some(parsed) = parsed {
            if !parsed.is_empty() {
                return Some(parsed);
            }
        }
    }
    None
}

fn required_string_field(value: &Value, keys: &[&str]) -> Result<String> {
    optional_string_field(value, keys).context("missing required string field")
}

fn optional_i64_field(value: &Value, keys: &[&str]) -> Option<i64> {
    let object = value.as_object()?;
    for key in keys {
        let Some(raw) = object.get(*key) else {
            continue;
        };
        let parsed = match raw {
            Value::Number(num) => num.as_i64(),
            Value::String(text) => text.trim().parse::<i64>().ok(),
            _ => None,
        };
        if parsed.is_some() {
            return parsed;
        }
    }
    None
}

fn string_list_field(value: &Value, keys: &[&str]) -> Vec<String> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    for key in keys {
        let Some(raw) = object.get(*key) else {
            continue;
        };
        let parsed = match raw {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(text.trim().to_string()),
                    Value::Number(num) => Some(num.to_string()),
                    _ => None,
                })
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>(),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    Vec::new()
                } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    trimmed
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .map(|item| item.trim().trim_matches('\'').trim_matches('"').to_string())
                        .filter(|item| !item.is_empty())
                        .collect::<Vec<_>>()
                } else {
                    vec![trimmed.to_string()]
                }
            }
            _ => Vec::new(),
        };
        if !parsed.is_empty() {
            return parsed;
        }
    }
    Vec::new()
}

fn contexts_from_hits(
    hits: &[RetrievedStreamingContext],
    limit: usize,
) -> Vec<StreamingQaDocument> {
    hits.iter()
        .take(limit)
        .map(|hit| hit.document.clone())
        .collect()
}

fn retrieval_trace(hits: &[RetrievedStreamingContext]) -> Vec<String> {
    if hits.is_empty() {
        return vec!["no_hits".to_string()];
    }
    hits.iter()
        .map(|hit| {
            let mut parts = vec![
                format!("query={}", hit.query),
                format!("doc_id={}", hit.document.id),
                format!("title={}", hit.document.title),
                format!("score={:.2}", hit.hit.score),
            ];
            if let Some(status) = hit.hit.status.as_deref() {
                parts.push(format!("status={}", status));
            }
            if let Some(superseded_by) = hit.hit.superseded_by.as_deref() {
                parts.push(format!("superseded_by={}", superseded_by));
            }
            if !hit.hit.surfaced_via.is_empty() {
                parts.push(format!("via={}", hit.hit.surfaced_via.join("; ")));
            }
            parts.join(" | ")
        })
        .collect()
}

fn build_context_history(
    thread_id: &str,
    agent: &str,
    contexts: &[StreamingQaDocument],
) -> Vec<ChatMessage> {
    contexts
        .iter()
        .enumerate()
        .map(|(index, context)| ChatMessage {
            message_id: format!("{}-ctx-{}", thread_id, index + 1),
            thread_id: thread_id.to_string(),
            role: "operator".to_string(),
            agent: Some(agent.to_string()),
            agent_runtime_id: None,
            content: format_context_note(context, 3_200),
            checkpoint_id: None,
            created_at: Utc::now(),
        })
        .collect()
}

fn history_with_prompt(
    history: &[ChatMessage],
    thread_id: &str,
    agent: &str,
    prompt: &str,
) -> Vec<ChatMessage> {
    let mut augmented = history.to_vec();
    augmented.push(ChatMessage {
        message_id: format!("{}-prompt", thread_id),
        thread_id: thread_id.to_string(),
        role: "operator".to_string(),
        agent: Some(agent.to_string()),
        agent_runtime_id: None,
        content: prompt.to_string(),
        checkpoint_id: None,
        created_at: Utc::now(),
    });
    augmented
}

fn format_context_note(context: &StreamingQaDocument, max_chars: usize) -> String {
    let excerpt = truncate_chars(context.text.trim(), max_chars);
    format!(
        "Context note [{}]\nDocument ID: {}\nPublication timestamp: {}\n{}",
        context.title, context.id, context.publication_ts, excerpt
    )
}

fn build_harkonnen_prompt(question: &StreamingQaQuestion, retrieval_trace: &[String]) -> String {
    let trace = if retrieval_trace.is_empty() {
        "none".to_string()
    } else {
        retrieval_trace.join(" ; ")
    };
    format!(
        "StreamingQA temporal QA. Question timestamp: {}. Evidence timestamp: {}. Retrieval trace: {}.\nQuestion: {}\nAnswer using only the earlier context notes, preferring the newest non-superseded information available before the question timestamp. Return only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the notes do not contain enough information, reply exactly: I do not know.",
        question.question_ts,
        question.evidence_ts,
        trace,
        question.question.trim()
    )
}

fn build_direct_prompt(
    question: &StreamingQaQuestion,
    contexts: &[StreamingQaDocument],
    context_budget: usize,
) -> String {
    let blocks = contexts
        .iter()
        .map(|context| format_context_note(context, 6_000))
        .collect::<Vec<_>>();
    let (joined_context, truncated) = truncate_context_blocks(&blocks, context_budget);
    let label = if truncated {
        "Context notes (truncated to fit local model context):"
    } else {
        "Context notes:"
    };
    format!(
        "You are answering a StreamingQA question using only the temporal document notes below. Prefer the newest non-superseded information available before the question timestamp.\n\nQuestion timestamp: {}\nEvidence timestamp: {}\n{}\n{}\n\nQuestion: {}\n\nReturn only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the context notes do not contain enough information, reply exactly: I do not know.",
        question.question_ts,
        question.evidence_ts,
        label,
        joined_context,
        question.question.trim()
    )
}

fn evaluate_question(
    question: &StreamingQaQuestion,
    raw_hypothesis: String,
    contexts_used: Vec<String>,
    retrieval_trace: Vec<String>,
    evidence_hit: bool,
    requires_supersession_history: bool,
) -> StreamingQaQuestionResult {
    let hypothesis = extract_final_answer(&raw_hypothesis);
    let normalized_hypothesis = normalize_text(&hypothesis);
    let normalized_answers = question
        .answers
        .iter()
        .map(|answer| normalize_text(answer))
        .filter(|answer| !answer.is_empty())
        .collect::<Vec<_>>();

    let exact_match = normalized_answers
        .iter()
        .any(|answer| !normalized_hypothesis.is_empty() && &normalized_hypothesis == answer);
    let accuracy = normalized_answers.iter().any(|answer| {
        !normalized_hypothesis.is_empty()
            && !answer.is_empty()
            && (normalized_hypothesis.contains(answer) || answer.contains(&normalized_hypothesis))
    });
    let token_f1 = normalized_answers
        .iter()
        .map(|answer| token_f1(&normalized_hypothesis, answer))
        .fold(0.0f64, f64::max);

    StreamingQaQuestionResult {
        qa_id: question.qa_id.clone(),
        question: question.question.clone(),
        expected_answers: question.answers.clone(),
        raw_hypothesis,
        hypothesis,
        accuracy,
        exact_match,
        token_f1,
        evidence_hit,
        requires_supersession_history,
        recent_or_past: question.recent_or_past.clone(),
        written_or_generated: question.written_or_generated.clone(),
        question_ts: question.question_ts,
        evidence_ts: question.evidence_ts,
        evidence_id: question.evidence_id.clone(),
        contexts_used,
        retrieval_trace,
    }
}

fn render_markdown_summary(summary: &StreamingQaSummary) -> String {
    let mut lines = vec![
        "# StreamingQA Summary".to_string(),
        String::new(),
        format!("- Dataset: {}", summary.dataset_path),
        format!(
            "- Docs: {}",
            summary
                .docs_path
                .clone()
                .unwrap_or_else(|| "inline fixture documents".to_string())
        ),
        format!("- Mode: {}", summary.mode),
        format!("- Agent: {}", summary.agent),
        format!("- Provider path: {}", summary.provider_label),
        format!(
            "- Limit: {}",
            summary
                .limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "full".to_string())
        ),
        format!("- Generated: {}", summary.generated_at),
        format!("- Predictions: {}", summary.predictions_path),
        String::new(),
        "## Aggregate Metrics".to_string(),
        String::new(),
        format!("- Questions: {}", summary.metrics.total_questions),
        format!("- Accuracy: {:.4}", summary.metrics.accuracy),
        format!("- Exact match: {:.4}", summary.metrics.exact_match),
        format!("- Token F1: {:.4}", summary.metrics.token_f1),
        format!(
            "- Evidence hit rate: {:.4}",
            summary.metrics.evidence_hit_rate
        ),
        format!("- Answered rate: {:.4}", summary.metrics.answered_rate),
        String::new(),
        "## Persistence".to_string(),
        String::new(),
        format!(
            "- Memory updates DB: {}",
            summary.persistence.memory_updates_db_path
        ),
        format!(
            "- Persisted supersession events: {}",
            summary.persistence.persisted_supersession_events
        ),
        format!(
            "- Updated-fact questions: {}",
            summary.persistence.questions_requiring_supersession_history
        ),
        format!(
            "- Updated-fact accuracy: {:.4}",
            summary.persistence.accuracy_on_updated_facts
        ),
        format!(
            "- Updated-fact exact match: {:.4}",
            summary.persistence.exact_match_on_updated_facts
        ),
        format!(
            "- Updated-fact evidence hit rate: {:.4}",
            summary.persistence.evidence_hit_rate_on_updated_facts
        ),
        String::new(),
        "## By Recency".to_string(),
        String::new(),
        "| Slice | Questions | Accuracy | Exact Match | Token F1 | Evidence Hit |".to_string(),
        "| --- | ---: | ---: | ---: | ---: | ---: |".to_string(),
    ];

    for (slice, metrics) in &summary.metrics.by_recency {
        lines.push(format!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} |",
            slice,
            metrics.total_questions,
            metrics.accuracy,
            metrics.exact_match,
            metrics.token_f1,
            metrics.evidence_hit_rate,
        ));
    }

    lines.push(String::new());
    lines.push("## By Origin".to_string());
    lines.push(String::new());
    lines.push(
        "| Slice | Questions | Accuracy | Exact Match | Token F1 | Evidence Hit |".to_string(),
    );
    lines.push("| --- | ---: | ---: | ---: | ---: | ---: |".to_string());
    for (slice, metrics) in &summary.metrics.by_origin {
        lines.push(format!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} |",
            slice,
            metrics.total_questions,
            metrics.accuracy,
            metrics.exact_match,
            metrics.token_f1,
            metrics.evidence_hit_rate,
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn threshold_failure(
    metrics: &StreamingQaMetrics,
    persistence: &StreamingQaPersistenceSummary,
    config: &StreamingQaRunConfig,
) -> Option<String> {
    if persistence.questions_requiring_supersession_history > 0
        && persistence.persisted_supersession_events == 0
    {
        return Some(
            "fixture required supersession history, but no persisted memory_updates rows were recorded"
                .to_string(),
        );
    }
    if let Some(min_accuracy) = config.min_accuracy {
        if metrics.accuracy < min_accuracy {
            return Some(format!(
                "accuracy {:.4} below STREAMINGQA_MIN_ACCURACY {:.4}",
                metrics.accuracy, min_accuracy
            ));
        }
    }
    None
}

async fn init_memory_updates_db(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_updates (
            update_id       TEXT PRIMARY KEY,
            old_memory_id   TEXT NOT NULL,
            new_memory_id   TEXT NOT NULL,
            memory_root     TEXT NOT NULL DEFAULT '',
            reason          TEXT NOT NULL DEFAULT '',
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;
    Ok(pool)
}

async fn persist_memory_update(
    pool: &SqlitePool,
    old_memory_id: &str,
    new_memory_id: &str,
    memory_root: &Path,
    reason: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO memory_updates (update_id, old_memory_id, new_memory_id, memory_root, reason, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(old_memory_id)
    .bind(new_memory_id)
    .bind(memory_root.display().to_string())
    .bind(reason)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

async fn count_memory_updates(pool: &SqlitePool) -> Result<usize> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_updates")
        .fetch_one(pool)
        .await?;
    Ok(count.max(0) as usize)
}

fn question_requires_supersession_history(
    question: &StreamingQaQuestion,
    document_by_id: &HashMap<String, StreamingQaDocument>,
) -> bool {
    document_by_id
        .get(&question.evidence_id)
        .map(|document| !document.supersedes.is_empty())
        .unwrap_or(false)
}

fn finalize_slices(
    slices: HashMap<String, StreamingQaAccumulator>,
) -> BTreeMap<String, StreamingQaSliceMetrics> {
    let mut ordered = BTreeMap::new();
    for (key, acc) in slices {
        ordered.insert(key, acc.finalize());
    }
    ordered
}

fn normalize_slice_key(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_final_answer(raw_hypothesis: &str) -> String {
    let without_think = strip_think_blocks(raw_hypothesis);
    let trimmed = without_think.trim();
    if trimmed.is_empty() {
        return raw_hypothesis.trim().to_string();
    }

    let mut chosen = trimmed.to_string();
    let non_empty_lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if let Some(last_line) = non_empty_lines.last() {
        chosen = (*last_line).to_string();
    }

    for prefix in ["answer:", "final answer:", "response:"] {
        if chosen.to_lowercase().starts_with(prefix) {
            chosen = chosen[prefix.len()..].trim().to_string();
            break;
        }
    }

    chosen
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches(' ')
        .to_string()
}

fn strip_think_blocks(value: &str) -> String {
    let mut remaining = value;
    let mut cleaned = String::new();
    loop {
        if let Some(start) = remaining.find("<think>") {
            cleaned.push_str(&remaining[..start]);
            let after_start = &remaining[start + "<think>".len()..];
            if let Some(end) = after_start.find("</think>") {
                remaining = &after_start[end + "</think>".len()..];
                continue;
            }
            break;
        }
        cleaned.push_str(remaining);
        break;
    }
    cleaned
}

fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_f1(prediction: &str, answer: &str) -> f64 {
    if prediction.is_empty() || answer.is_empty() {
        return 0.0;
    }
    let pred_tokens = prediction.split_whitespace().collect::<Vec<_>>();
    let answer_tokens = answer.split_whitespace().collect::<Vec<_>>();
    if pred_tokens.is_empty() || answer_tokens.is_empty() {
        return 0.0;
    }

    let mut answer_counts = HashMap::new();
    for token in answer_tokens {
        *answer_counts.entry(token).or_insert(0usize) += 1;
    }

    let answer_token_total: usize = answer_counts.values().sum();
    let mut overlap = 0usize;
    for token in pred_tokens.iter().copied() {
        if let Some(count) = answer_counts.get_mut(token) {
            if *count > 0 {
                overlap += 1;
                *count -= 1;
            }
        }
    }
    if overlap == 0 {
        return 0.0;
    }

    let precision = overlap as f64 / pred_tokens.len() as f64;
    let recall = overlap as f64 / answer_token_total as f64;
    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

fn ratio(numerator: f64, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator / denominator as f64
    }
}

fn provider_label(config: &StreamingQaRunConfig) -> String {
    match config.mode {
        StreamingQaMode::Harkonnen => format!(
            "PackChat agent {} with temporal invalidation-aware retrieval",
            config.agent
        ),
        StreamingQaMode::Direct => format!(
            "Direct provider {} with flat temporal retrieved context",
            config.direct_provider
        ),
    }
}

fn context_label(context: &StreamingQaDocument) -> String {
    format!("{} ({})", context.title, context.id)
}

fn read_env(name: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    overrides
        .get(name)
        .cloned()
        .or_else(|| env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_dataset_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(value) = read_env("STREAMINGQA_DATASET", overrides) {
        return Some(PathBuf::from(value));
    }
    let candidates = [
        paths
            .root
            .join("factory")
            .join("benchmarks")
            .join("fixtures")
            .join("streamingqa-smoke.json"),
        paths
            .root
            .join("development_datasets")
            .join("streaminqa_eval.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("streamingqa_eval.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("streamingqa")
            .join("streaminqa_eval.jsonl"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

fn resolve_docs_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(value) = read_env("STREAMINGQA_DOCS", overrides) {
        return Some(PathBuf::from(value));
    }
    if let Some(value) = read_env("STREAMINGQA_ROOT", overrides) {
        let root = PathBuf::from(value);
        for candidate in [
            root.join("wmt_docs.jsonl"),
            root.join("wmt_docs.json"),
            root.join("streamingqa_docs.jsonl"),
            root.join("streamingqa_docs.json"),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    let candidates = [
        paths
            .root
            .join("development_datasets")
            .join("wmt_docs.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("wmt_docs.json"),
        paths
            .root
            .join("development_datasets")
            .join("streamingqa_docs.jsonl"),
        paths
            .root
            .join("development_datasets")
            .join("streamingqa_docs.json"),
    ];
    candidates.into_iter().find(|path| path.exists())
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

fn direct_context_char_limit() -> usize {
    env::var("STREAMINGQA_DIRECT_MAX_CHARS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= MIN_DIRECT_CONTEXT_CHAR_LIMIT)
        .unwrap_or(DEFAULT_DIRECT_CONTEXT_CHAR_LIMIT)
}

fn is_context_window_error(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("context length")
        || text.contains("context window")
        || text.contains("context size")
        || text.contains("has been exceeded")
        || text.contains("n_keep")
        || text.contains("n_ctx")
        || text.contains("too many tokens")
}

fn benchmark_verbose_progress() -> bool {
    benchmark_flag_enabled("HARKONNEN_BENCH_VERBOSE")
        || benchmark_flag_enabled("HARKONNEN_BENCH_PROGRESS")
}

fn benchmark_show_raw_output() -> bool {
    benchmark_flag_enabled("HARKONNEN_BENCH_SHOW_RAW")
        || benchmark_flag_enabled("HARKONNEN_BENCH_SHOW_THINK")
}

fn benchmark_flag_enabled(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn truncate_context_blocks(blocks: &[String], max_chars: usize) -> (String, bool) {
    let joined = blocks.join("\n\n");
    if joined.chars().count() <= max_chars {
        return (joined, false);
    }

    let mut kept = Vec::new();
    let mut used = 0usize;
    for block in blocks {
        let block_chars = block.chars().count();
        let separator = if kept.is_empty() { 0 } else { 2 };
        if used + block_chars + separator > max_chars {
            let remaining = max_chars.saturating_sub(used + separator);
            if remaining > 0 && kept.is_empty() {
                kept.push(truncate_chars(block, remaining));
            }
            break;
        }
        kept.push(block.clone());
        used += block_chars + separator;
    }
    (kept.join("\n\n"), true)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let total = value.chars().count();
    if total <= max_chars {
        return value.to_string();
    }
    let mut out = value.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn timestamp_year(timestamp: i64) -> i32 {
    chrono::DateTime::<Utc>::from_timestamp(timestamp, 0)
        .map(|dt| dt.year())
        .unwrap_or(1970)
}

fn retrieval_tokens(value: &str, min_len: usize, limit: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= min_len)
        .map(|token| token.to_ascii_lowercase())
    {
        if tokens.iter().any(|existing| existing == &token) {
            continue;
        }
        tokens.push(token);
        if tokens.len() >= limit {
            break;
        }
    }
    tokens
}

fn extend_unique_strings(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_question_answers_from_string_list_literal() {
        let value = serde_json::json!({
            "qa_id": "q1",
            "question": "Who is the mayor?",
            "answers": "['Alice Hart', 'Alice']",
            "question_ts": 10,
            "evidence_ts": 5,
            "evidence_id": "doc-1"
        });
        let question = parse_question_value(&value, 0).unwrap();
        assert_eq!(
            question.answers,
            vec!["Alice Hart".to_string(), "Alice".to_string()]
        );
    }

    #[test]
    fn parse_document_supports_sorting_key_and_supersedes() {
        let value = serde_json::json!({
            "sorting_key": "doc-2",
            "title": "Oakview update",
            "text": "Ben Moss became mayor of Oakview.",
            "publication_ts": 20,
            "supersedes": ["doc-1"]
        });
        let document = parse_document_value(&value, 0).unwrap();
        assert_eq!(document.id, "doc-2");
        assert_eq!(document.supersedes, vec!["doc-1".to_string()]);
    }

    #[test]
    fn evaluate_question_uses_best_reference_answer() {
        let question = StreamingQaQuestion {
            qa_id: "q".to_string(),
            question: "Who is the mayor?".to_string(),
            answers: vec!["Alice Hart".to_string(), "Alice".to_string()],
            question_ts: 0,
            evidence_ts: 0,
            evidence_id: "doc-1".to_string(),
            recent_or_past: "recent".to_string(),
            written_or_generated: "written".to_string(),
        };
        let result = evaluate_question(
            &question,
            "Answer: Alice".to_string(),
            Vec::new(),
            Vec::new(),
            true,
            false,
        );
        assert!(result.accuracy);
        assert!(result.exact_match);
    }

    #[test]
    fn question_requires_supersession_history_when_evidence_supersedes_prior_doc() {
        let question = StreamingQaQuestion {
            qa_id: "q".to_string(),
            question: "Who became mayor after the special election?".to_string(),
            answers: vec!["Ben Moss".to_string()],
            question_ts: 20,
            evidence_ts: 20,
            evidence_id: "doc-2".to_string(),
            recent_or_past: "recent".to_string(),
            written_or_generated: "written".to_string(),
        };
        let mut documents = HashMap::new();
        documents.insert(
            "doc-2".to_string(),
            StreamingQaDocument {
                id: "doc-2".to_string(),
                title: "Oakview update".to_string(),
                text: "Ben Moss became mayor.".to_string(),
                publication_ts: 20,
                supersedes: vec!["doc-1".to_string()],
            },
        );
        assert!(question_requires_supersession_history(
            &question, &documents
        ));
    }

    #[test]
    fn threshold_failure_requires_persisted_updates_for_updated_fact_questions() {
        let metrics = StreamingQaMetrics::default();
        let persistence = StreamingQaPersistenceSummary {
            questions_requiring_supersession_history: 1,
            ..StreamingQaPersistenceSummary::default()
        };
        let config = StreamingQaRunConfig {
            dataset_path: PathBuf::from("fixture.json"),
            docs_path: None,
            output_dir: PathBuf::from("/tmp/streamingqa"),
            mode: StreamingQaMode::Harkonnen,
            agent: "coobie".to_string(),
            direct_provider: "default".to_string(),
            limit: None,
            min_accuracy: None,
        };
        let reason = threshold_failure(&metrics, &persistence, &config).unwrap();
        assert!(reason.contains("memory_updates"));
    }
}
