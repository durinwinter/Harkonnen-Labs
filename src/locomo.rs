use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::PathBuf;

use crate::{
    benchmark::BenchmarkStatus,
    chat::{self, ChatMessage},
    config::Paths,
    llm::{self, LlmRequest, Message},
};

const DEFAULT_AGENT: &str = "coobie";
const ABSTENTION_PHRASES: &[&str] = &[
    "no information available",
    "not mentioned",
    "i do not know",
    "i don't know",
    "unknown",
];

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoCoMoMode {
    Harkonnen,
    Direct,
}

impl LoCoMoMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "packchat" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => bail!("unsupported LOCOMO_MODE: {}", other),
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
pub struct LoCoMoRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: LoCoMoMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub min_proxy_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoCoMoRunOutput {
    pub output_dir: PathBuf,
    pub predictions_path: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: LoCoMoMode,
    pub provider_label: String,
    pub metrics: LoCoMoMetrics,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LoCoMoSuiteOutcome {
    Completed(LoCoMoRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LoCoMoMetrics {
    pub total_questions: usize,
    pub proxy_qa_score: f64,
    pub answered_rate: f64,
    pub by_category: BTreeMap<String, LoCoMoCategoryMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LoCoMoCategoryMetrics {
    pub total_questions: usize,
    pub proxy_qa_score: f64,
}

#[derive(Debug, Clone, Serialize)]
struct LoCoMoSummary {
    dataset_path: String,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: LoCoMoMetrics,
    predictions_path: String,
    questions: Vec<LoCoMoQuestionResult>,
}

#[derive(Debug, Clone, Serialize)]
struct LoCoMoQuestionResult {
    sample_id: String,
    qa_index: usize,
    category: i64,
    category_name: String,
    question: String,
    expected_answer: String,
    raw_hypothesis: String,
    hypothesis: String,
    score: f64,
}

#[derive(Debug, Serialize)]
struct LoCoMoPrediction<'a> {
    sample_id: &'a str,
    qa_index: usize,
    category: i64,
    question: &'a str,
    hypothesis: &'a str,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct LoCoMoSample {
    sample_id: String,
    conversation: BTreeMap<String, Value>,
    qa: Vec<LoCoMoQa>,
}

#[derive(Debug, Deserialize, Clone)]
struct LoCoMoQa {
    question: String,
    #[serde(default)]
    answer: Option<Value>,
    category: i64,
    #[serde(default)]
    adversarial_answer: Option<String>,
    #[serde(default, rename = "evidence")]
    _evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LoCoMoTurn {
    speaker: String,
    dia_id: String,
    text: String,
    #[serde(default)]
    blip_caption: Option<String>,
    #[serde(default)]
    img_url: Vec<String>,
}

#[derive(Debug, Default)]
struct LoCoMoAccumulator {
    total_questions: usize,
    score_sum: f64,
    answered_total: usize,
}

impl LoCoMoAccumulator {
    fn add(&mut self, result: &LoCoMoQuestionResult) {
        self.total_questions += 1;
        self.score_sum += result.score;
        if !result.hypothesis.trim().is_empty() {
            self.answered_total += 1;
        }
    }

    fn finalize(&self) -> LoCoMoCategoryMetrics {
        LoCoMoCategoryMetrics {
            total_questions: self.total_questions,
            proxy_qa_score: ratio(self.score_sum, self.total_questions),
        }
    }
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<LoCoMoSuiteOutcome> {
    let dataset_path = match resolve_dataset_path(paths, overrides) {
        Some(path) => path,
        None => {
            return Ok(LoCoMoSuiteOutcome::Skipped(
                "LoCoMo dataset not found. Set LOCOMO_DATASET or LOCOMO_ROOT, or place locomo10.json under development_datasets/.".to_string(),
            ))
        }
    };

    let mode = LoCoMoMode::parse(read_env("LOCOMO_MODE", overrides))?;
    let output_dir = read_env("LOCOMO_OUTPUT_DIR", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            paths.artifacts.join("benchmarks").join(format!(
                "locomo-{}-{}",
                mode.as_str(),
                Utc::now().timestamp_millis()
            ))
        });

    let config = LoCoMoRunConfig {
        dataset_path,
        output_dir,
        mode,
        agent: read_env("LOCOMO_AGENT", overrides).unwrap_or_else(|| DEFAULT_AGENT.to_string()),
        direct_provider: read_env("LOCOMO_DIRECT_PROVIDER", overrides)
            .unwrap_or_else(|| "default".to_string()),
        limit: parse_env_usize_with_overrides("LOCOMO_LIMIT", overrides)?,
        min_proxy_score: parse_env_f64_with_overrides("LOCOMO_MIN_PROXY_SCORE", overrides)?,
    };

    Ok(LoCoMoSuiteOutcome::Completed(run(paths, &config).await?))
}

pub async fn run(paths: &Paths, config: &LoCoMoRunConfig) -> Result<LoCoMoRunOutput> {
    let raw = tokio::fs::read_to_string(&config.dataset_path)
        .await
        .with_context(|| format!("reading LoCoMo dataset {}", config.dataset_path.display()))?;
    let samples: Vec<LoCoMoSample> = serde_json::from_str(&raw)
        .with_context(|| format!("parsing LoCoMo dataset {}", config.dataset_path.display()))?;
    if samples.is_empty() {
        bail!("LoCoMo dataset contains no samples")
    }

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| format!("creating LoCoMo output dir {}", config.output_dir.display()))?;

    let predictions_path = config.output_dir.join("predictions.jsonl");
    let summary_path = config.output_dir.join("summary.json");
    let markdown_path = config.output_dir.join("summary.md");

    let mut prediction_lines = Vec::new();
    let mut results = Vec::new();
    let mut accumulator = LoCoMoAccumulator::default();
    let mut by_category: HashMap<i64, LoCoMoAccumulator> = HashMap::new();
    let mut seen = 0usize;
    let total_questions = config
        .limit
        .unwrap_or_else(|| samples.iter().map(|sample| sample.qa.len()).sum());
    let verbose_progress = benchmark_verbose_progress();
    let show_raw_output = benchmark_show_raw_output();

    'outer: for sample in &samples {
        let history = build_history(sample, &config.agent)?;
        for (qa_index, qa) in sample.qa.iter().enumerate() {
            if let Some(limit) = config.limit {
                if seen >= limit {
                    break 'outer;
                }
            }
            if verbose_progress {
                eprintln!(
                    "[LoCoMo][{}/{}][{}] starting sample={} qa_index={} category={}",
                    seen + 1,
                    total_questions,
                    config.mode.as_str(),
                    sample.sample_id,
                    qa_index,
                    qa.category
                );
            }

            let raw_hypothesis = match config.mode {
                LoCoMoMode::Harkonnen => {
                    let prompt = build_question_prompt(qa);
                    let packchat_history = history_with_prompt(
                        &history,
                        &sample.sample_id,
                        qa_index,
                        &config.agent,
                        &prompt,
                    );
                    chat::complete_agent_reply(
                        &config.agent,
                        &prompt,
                        &packchat_history,
                        None,
                        paths,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "answering LoCoMo sample {} qa {} via PackChat",
                            sample.sample_id, qa_index
                        )
                    })?
                }
                LoCoMoMode::Direct => complete_direct_reply(paths, config, sample, qa, &history)
                    .await
                    .with_context(|| {
                        format!(
                            "answering LoCoMo sample {} qa {} via direct provider",
                            sample.sample_id, qa_index
                        )
                    })?,
            };

            let result = evaluate_question(sample, qa_index, qa, raw_hypothesis);
            if verbose_progress {
                eprintln!(
                    "[LoCoMo][{}/{}][{}] done sample={} qa_index={} score={:.4} answer={}",
                    seen + 1,
                    total_questions,
                    config.mode.as_str(),
                    result.sample_id,
                    result.qa_index,
                    result.score,
                    result.hypothesis
                );
                if show_raw_output {
                    eprintln!(
                        "[LoCoMo][{}:{}] raw output:
{}
---",
                        result.sample_id, result.qa_index, result.raw_hypothesis
                    );
                }
            }
            accumulator.add(&result);
            by_category.entry(qa.category).or_default().add(&result);
            prediction_lines.push(serde_json::to_string(&LoCoMoPrediction {
                sample_id: &result.sample_id,
                qa_index: result.qa_index,
                category: result.category,
                question: &result.question,
                hypothesis: &result.hypothesis,
                score: result.score,
            })?);
            results.push(result);
            seen += 1;
        }
    }

    if results.is_empty() {
        bail!("LoCoMo run selected zero QA items")
    }

    let metrics = LoCoMoMetrics {
        total_questions: accumulator.total_questions,
        proxy_qa_score: ratio(accumulator.score_sum, accumulator.total_questions),
        answered_rate: ratio(
            accumulator.answered_total as f64,
            accumulator.total_questions,
        ),
        by_category: finalize_by_category(by_category),
    };

    tokio::fs::write(
        &predictions_path,
        format!("{}\n", prediction_lines.join("\n")),
    )
    .await
    .with_context(|| format!("writing LoCoMo predictions {}", predictions_path.display()))?;

    let summary = LoCoMoSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label(config),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        predictions_path: predictions_path.display().to_string(),
        questions: results,
    };

    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing LoCoMo summary {}", summary_path.display()))?;

    tokio::fs::write(&markdown_path, render_markdown_summary(&summary))
        .await
        .with_context(|| {
            format!(
                "writing LoCoMo markdown summary {}",
                markdown_path.display()
            )
        })?;

    let threshold_failure = threshold_failure(&metrics, config);

    Ok(LoCoMoRunOutput {
        output_dir: config.output_dir.clone(),
        predictions_path,
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label: provider_label(config),
        metrics,
        threshold_failure,
    })
}

pub fn render_step_stdout(output: &LoCoMoRunOutput) -> String {
    let mut lines = vec![
        format!("LoCoMo mode: {}", output.mode.as_str()),
        format!("Provider path: {}", output.provider_label),
        format!("LoCoMo output dir: {}", output.output_dir.display()),
        format!("Predictions: {}", output.predictions_path.display()),
        format!("Summary JSON: {}", output.summary_path.display()),
        format!("Summary Markdown: {}", output.markdown_path.display()),
        format!("Questions: {}", output.metrics.total_questions),
        format!("Proxy QA score: {:.4}", output.metrics.proxy_qa_score),
        format!("Answered rate: {:.4}", output.metrics.answered_rate),
    ];
    if let Some(reason) = &output.threshold_failure {
        lines.push(format!("Threshold failure: {}", reason));
    }
    lines.join("\n")
}

pub fn status_for_output(output: &LoCoMoRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &LoCoMoRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

fn resolve_dataset_path(paths: &Paths, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(value) = read_env("LOCOMO_DATASET", overrides) {
        return Some(PathBuf::from(value));
    }
    if let Some(value) = read_env("LOCOMO_ROOT", overrides) {
        let root = PathBuf::from(value);
        let candidate = root.join("data").join("locomo10.json");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let candidates = [
        paths
            .root
            .join("development_datasets")
            .join("locomo10.json"),
        paths
            .root
            .join("development_datasets")
            .join("locomo")
            .join("data")
            .join("locomo10.json"),
        PathBuf::from("/tmp/locomo/data/locomo10.json"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

fn history_with_prompt(
    history: &[ChatMessage],
    sample_id: &str,
    qa_index: usize,
    agent: &str,
    prompt: &str,
) -> Vec<ChatMessage> {
    let mut augmented = history.to_vec();
    augmented.push(ChatMessage {
        message_id: format!("{}-qa-{}-prompt", sample_id, qa_index),
        thread_id: sample_id.to_string(),
        role: "operator".to_string(),
        agent: Some(agent.to_string()),
        content: prompt.to_string(),
        checkpoint_id: None,
        created_at: Utc::now(),
    });
    augmented
}

fn build_history(sample: &LoCoMoSample, agent: &str) -> Result<Vec<ChatMessage>> {
    let speaker_a = sample
        .conversation
        .get("speaker_a")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut sessions = Vec::new();
    for (key, value) in &sample.conversation {
        if !key.starts_with("session_") || key.ends_with("_date_time") {
            continue;
        }
        let Ok(session_num) = key.trim_start_matches("session_").parse::<usize>() else {
            continue;
        };
        let turns: Vec<LoCoMoTurn> = serde_json::from_value(value.clone())
            .with_context(|| format!("parsing LoCoMo conversation {}", key))?;
        if turns.is_empty() {
            continue;
        }
        let dt_key = format!("{}_date_time", key);
        let date_time = sample
            .conversation
            .get(&dt_key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        sessions.push((session_num, date_time, turns));
    }
    sessions.sort_by_key(|(num, _, _)| *num);

    let mut history = Vec::new();
    for (session_num, date_time, turns) in sessions {
        for (turn_idx, turn) in turns.into_iter().enumerate() {
            let role = if turn.speaker == speaker_a {
                "operator"
            } else {
                "agent"
            };
            history.push(ChatMessage {
                message_id: format!("{}-session-{}-{}", sample.sample_id, session_num, turn_idx),
                thread_id: sample.sample_id.clone(),
                role: role.to_string(),
                agent: Some(agent.to_string()),
                content: format_turn_content(session_num, &date_time, &turn),
                checkpoint_id: None,
                created_at: Utc::now(),
            });
        }
    }
    Ok(history)
}

fn format_turn_content(session_num: usize, date_time: &str, turn: &LoCoMoTurn) -> String {
    let mut content = format!(
        "[session {} | {} | {} | speaker: {}] {}",
        session_num,
        date_time.trim(),
        turn.dia_id.trim(),
        turn.speaker.trim(),
        turn.text.trim()
    );
    if !turn.img_url.is_empty() {
        if let Some(caption) = turn.blip_caption.as_deref() {
            content.push_str(&format!(" [shared image: {}]", caption.trim()));
        }
    }
    content
}

async fn complete_direct_reply(
    paths: &Paths,
    config: &LoCoMoRunConfig,
    sample: &LoCoMoSample,
    qa: &LoCoMoQa,
    history: &[ChatMessage],
) -> Result<String> {
    let provider = llm::build_provider("benchmark", &config.direct_provider, &paths.setup)
        .with_context(|| {
            format!(
                "no configured provider available for LoCoMo direct mode via {}",
                config.direct_provider
            )
        })?;

    let req = LlmRequest {
        messages: vec![Message::user(build_direct_prompt(sample, qa, history))],
        max_tokens: 512,
        temperature: 0.0,
    };

    provider
        .complete(req)
        .await
        .map(|resp| resp.content)
        .with_context(|| {
            format!(
                "LoCoMo direct provider failed via {}",
                config.direct_provider
            )
        })
}

fn build_direct_prompt(sample: &LoCoMoSample, qa: &LoCoMoQa, history: &[ChatMessage]) -> String {
    let transcript = history
        .iter()
        .map(|message| {
            let role = if message.role == "operator" {
                "User"
            } else {
                "Assistant"
            };
            format!("{}: {}", role, message.content.trim())
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are answering a LoCoMo memory question using only the transcript below.\n\nSample ID: {}\nQuestion category: {}\nTranscript:\n{}\n\nQuestion: {}\n\nReturn only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the transcript does not contain enough information, reply exactly: No information available.",
        sample.sample_id,
        qa.category,
        transcript,
        qa.question.trim(),
    )
}

fn build_question_prompt(qa: &LoCoMoQa) -> String {
    format!(
        "LoCoMo question category: {}\nQuestion: {}\nAnswer using only the conversation history. Return only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the history does not contain enough information, reply exactly: No information available.",
        qa.category,
        qa.question.trim(),
    )
}

fn evaluate_question(
    sample: &LoCoMoSample,
    qa_index: usize,
    qa: &LoCoMoQa,
    raw_hypothesis: String,
) -> LoCoMoQuestionResult {
    let hypothesis = extract_final_answer(&raw_hypothesis);
    let expected_answer = answer_text(qa);
    let score = category_score(&hypothesis, &expected_answer, qa.category);
    LoCoMoQuestionResult {
        sample_id: sample.sample_id.clone(),
        qa_index,
        category: qa.category,
        category_name: category_name(qa.category).to_string(),
        question: qa.question.clone(),
        expected_answer,
        raw_hypothesis,
        hypothesis,
        score,
    }
}

fn answer_text(qa: &LoCoMoQa) -> String {
    match qa.answer.as_ref() {
        Some(Value::String(text)) => {
            if qa.category == 3 {
                text.split(';').next().unwrap_or(text).trim().to_string()
            } else {
                text.trim().to_string()
            }
        }
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
        None => qa.adversarial_answer.clone().unwrap_or_default(),
    }
}

fn category_score(prediction: &str, answer: &str, category: i64) -> f64 {
    match category {
        1 => multi_answer_f1(prediction, answer),
        2 | 3 | 4 => f1_score(prediction, answer),
        5 => {
            let normalized = prediction.to_ascii_lowercase();
            if ABSTENTION_PHRASES
                .iter()
                .any(|phrase| normalized.contains(phrase))
            {
                1.0
            } else {
                0.0
            }
        }
        _ => f1_score(prediction, answer),
    }
}

fn multi_answer_f1(prediction: &str, answer: &str) -> f64 {
    let predictions = prediction
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let answers = answer
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if predictions.is_empty() || answers.is_empty() {
        return 0.0;
    }
    let sum = answers
        .iter()
        .map(|expected| {
            predictions
                .iter()
                .map(|pred| f1_score(pred, expected))
                .fold(0.0, f64::max)
        })
        .sum::<f64>();
    sum / answers.len() as f64
}

fn f1_score(prediction: &str, answer: &str) -> f64 {
    let pred_tokens = normalize_answer(prediction)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let answer_tokens = normalize_answer(answer)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if pred_tokens.is_empty() || answer_tokens.is_empty() {
        return 0.0;
    }

    let mut answer_counts = HashMap::new();
    for token in answer_tokens.iter() {
        *answer_counts.entry(token.clone()).or_insert(0usize) += 1;
    }

    let mut overlap = 0usize;
    for token in &pred_tokens {
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
    let recall = overlap as f64 / answer_tokens.len() as f64;
    if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    }
}

fn normalize_answer(value: &str) -> String {
    let cleaned = value.to_ascii_lowercase().replace(',', "");
    cleaned
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
        .filter(|token| !matches!(*token, "a" | "an" | "the" | "and"))
        .collect::<Vec<_>>()
        .join(" ")
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

fn category_name(category: i64) -> &'static str {
    match category {
        1 => "category_1",
        2 => "category_2",
        3 => "category_3",
        4 => "category_4",
        5 => "category_5",
        _ => "unknown",
    }
}

fn finalize_by_category(
    by_category: HashMap<i64, LoCoMoAccumulator>,
) -> BTreeMap<String, LoCoMoCategoryMetrics> {
    let mut ordered = BTreeMap::new();
    for (category, acc) in by_category {
        ordered.insert(category_name(category).to_string(), acc.finalize());
    }
    ordered
}

fn render_markdown_summary(summary: &LoCoMoSummary) -> String {
    let mut lines = vec![
        "# LoCoMo Summary".to_string(),
        String::new(),
        format!("- Dataset: {}", summary.dataset_path),
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
        format!("- Proxy QA score: {:.4}", summary.metrics.proxy_qa_score),
        format!("- Answered rate: {:.4}", summary.metrics.answered_rate),
        String::new(),
        "## By Category".to_string(),
        String::new(),
        "| Category | Questions | Proxy QA Score |".to_string(),
        "| --- | ---: | ---: |".to_string(),
    ];

    for (category, metrics) in &summary.metrics.by_category {
        lines.push(format!(
            "| {} | {} | {:.4} |",
            category, metrics.total_questions, metrics.proxy_qa_score
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn threshold_failure(metrics: &LoCoMoMetrics, config: &LoCoMoRunConfig) -> Option<String> {
    if let Some(min_score) = config.min_proxy_score {
        if metrics.proxy_qa_score < min_score {
            return Some(format!(
                "proxy QA score {:.4} below LOCOMO_MIN_PROXY_SCORE {:.4}",
                metrics.proxy_qa_score, min_score
            ));
        }
    }
    None
}

fn provider_label(config: &LoCoMoRunConfig) -> String {
    match config.mode {
        LoCoMoMode::Harkonnen => format!("PackChat agent {}", config.agent),
        LoCoMoMode::Direct => format!("Direct provider {}", config.direct_provider),
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

fn ratio(numerator: f64, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locomo_mode_parses_synonyms() {
        assert!(matches!(
            LoCoMoMode::parse(Some("packchat".to_string())).unwrap(),
            LoCoMoMode::Harkonnen
        ));
        assert!(matches!(
            LoCoMoMode::parse(Some("raw".to_string())).unwrap(),
            LoCoMoMode::Direct
        ));
    }

    #[test]
    fn f1_score_handles_overlap() {
        let score = f1_score("7 may 2023", "7 may 2023");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn multi_answer_f1_splits_answers() {
        let score = multi_answer_f1("red, blue", "blue, red");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn abstention_category_uses_expected_phrases() {
        assert_eq!(category_score("No information available.", "", 5), 1.0);
        assert_eq!(category_score("I guessed it", "", 5), 0.0);
    }
}
