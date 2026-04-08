use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::{
    benchmark::BenchmarkStatus,
    chat::{self, ChatMessage},
    config::Paths,
    llm::{self, LlmRequest, Message},
};

const DEFAULT_AGENT: &str = "coobie";
const DEFAULT_ABSTENTION_PHRASES: &[&str] = &[
    "i don't know",
    "i do not know",
    "don't know",
    "do not know",
    "not enough information",
    "insufficient information",
    "cannot determine",
    "can't determine",
    "unable to determine",
    "unknown",
    "unsure",
    "not mentioned",
    "not provided",
    "no information",
    "not in the conversation",
];

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongMemEvalMode {
    Harkonnen,
    Direct,
}

impl LongMemEvalMode {
    fn parse(value: Option<String>) -> Result<Self> {
        match value
            .unwrap_or_else(|| "harkonnen".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "harkonnen" | "packchat" => Ok(Self::Harkonnen),
            "direct" | "raw" | "baseline" => Ok(Self::Direct),
            other => bail!("unsupported LONGMEMEVAL_MODE: {}", other),
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
pub struct LongMemEvalRunConfig {
    pub dataset_path: PathBuf,
    pub output_dir: PathBuf,
    pub mode: LongMemEvalMode,
    pub agent: String,
    pub direct_provider: String,
    pub limit: Option<usize>,
    pub official_eval_command: Option<String>,
    pub official_eval_root: Option<PathBuf>,
    pub min_proxy_exact: Option<f64>,
    pub min_proxy_f1: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LongMemEvalRunOutput {
    pub output_dir: PathBuf,
    pub predictions_path: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub mode: LongMemEvalMode,
    pub provider_label: String,
    pub metrics: LongMemEvalMetrics,
    pub official_eval: Option<LongMemEvalOfficialEvalResult>,
    pub threshold_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LongMemEvalSuiteOutcome {
    Completed(LongMemEvalRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct LongMemEvalOfficialEvalResult {
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LongMemEvalMetrics {
    pub total: usize,
    pub exact_match: f64,
    pub contains_answer: f64,
    pub token_f1: f64,
    pub abstention_question_accuracy: f64,
    pub abstention_false_positive_rate: f64,
    pub answered_rate: f64,
    pub by_question_type: BTreeMap<String, LongMemEvalQuestionTypeMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LongMemEvalQuestionTypeMetrics {
    pub total: usize,
    pub exact_match: f64,
    pub contains_answer: f64,
    pub token_f1: f64,
    pub abstention_question_accuracy: f64,
    pub abstention_false_positive_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
struct LongMemEvalSummary {
    dataset_path: String,
    mode: String,
    agent: String,
    provider_label: String,
    limit: Option<usize>,
    generated_at: String,
    metrics: LongMemEvalMetrics,
    predictions_path: String,
    official_eval: Option<LongMemEvalOfficialEvalResult>,
    questions: Vec<LongMemEvalQuestionResult>,
}

#[derive(Debug, Clone, Serialize)]
struct LongMemEvalQuestionResult {
    question_id: String,
    question_type: String,
    question_date: String,
    expected_answer: String,
    raw_hypothesis: String,
    hypothesis: String,
    exact_match: bool,
    contains_answer: bool,
    token_f1: f64,
    expected_abstention: bool,
    predicted_abstention: bool,
}

#[derive(Debug, Deserialize)]
struct LongMemEvalQuestion {
    question_id: String,
    #[serde(default)]
    question_type: String,
    question: String,
    answer: String,
    #[serde(default)]
    question_date: String,
    #[serde(default)]
    haystack_session_ids: Vec<String>,
    #[serde(default)]
    haystack_dates: Vec<String>,
    #[serde(default)]
    haystack_sessions: Vec<Vec<LongMemEvalTurn>>,
}

#[derive(Debug, Deserialize)]
struct LongMemEvalTurn {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct LongMemEvalPrediction<'a> {
    question_id: &'a str,
    hypothesis: &'a str,
}

#[derive(Debug, Default)]
struct LongMemEvalAccumulator {
    total: usize,
    exact_match: f64,
    contains_answer: f64,
    token_f1: f64,
    abstention_total: usize,
    abstention_correct: usize,
    non_abstention_total: usize,
    false_positive_abstentions: usize,
    answered_total: usize,
}

impl LongMemEvalAccumulator {
    fn add(&mut self, result: &LongMemEvalQuestionResult) {
        self.total += 1;
        if result.exact_match {
            self.exact_match += 1.0;
        }
        if result.contains_answer {
            self.contains_answer += 1.0;
        }
        self.token_f1 += result.token_f1;
        if !result.hypothesis.trim().is_empty() {
            self.answered_total += 1;
        }
        if result.expected_abstention {
            self.abstention_total += 1;
            if result.predicted_abstention {
                self.abstention_correct += 1;
            }
        } else {
            self.non_abstention_total += 1;
            if result.predicted_abstention {
                self.false_positive_abstentions += 1;
            }
        }
    }

    fn finalize(&self) -> LongMemEvalQuestionTypeMetrics {
        LongMemEvalQuestionTypeMetrics {
            total: self.total,
            exact_match: ratio(self.exact_match, self.total),
            contains_answer: ratio(self.contains_answer, self.total),
            token_f1: ratio(self.token_f1, self.total),
            abstention_question_accuracy: ratio(
                self.abstention_correct as f64,
                self.abstention_total,
            ),
            abstention_false_positive_rate: ratio(
                self.false_positive_abstentions as f64,
                self.non_abstention_total,
            ),
        }
    }
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<LongMemEvalSuiteOutcome> {
    let dataset_path = match read_env("LONGMEMEVAL_DATASET", overrides) {
        Some(value) => PathBuf::from(value),
        None => {
            return Ok(LongMemEvalSuiteOutcome::Skipped(
                "LONGMEMEVAL_DATASET is not set".to_string(),
            ))
        }
    };

    let mode = LongMemEvalMode::parse(read_env("LONGMEMEVAL_MODE", overrides))?;
    let output_dir = read_env("LONGMEMEVAL_OUTPUT_DIR", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            paths.artifacts.join("benchmarks").join(format!(
                "longmemeval-{}-{}",
                mode.as_str(),
                Utc::now().timestamp_millis()
            ))
        });

    let config = LongMemEvalRunConfig {
        dataset_path,
        output_dir,
        mode,
        agent: read_env("LONGMEMEVAL_AGENT", overrides)
            .unwrap_or_else(|| DEFAULT_AGENT.to_string()),
        direct_provider: read_env("LONGMEMEVAL_DIRECT_PROVIDER", overrides)
            .unwrap_or_else(|| "default".to_string()),
        limit: parse_env_usize_with_overrides("LONGMEMEVAL_LIMIT", overrides)?,
        official_eval_command: read_env("LONGMEMEVAL_OFFICIAL_EVAL_COMMAND", overrides),
        official_eval_root: read_env("LONGMEMEVAL_OFFICIAL_EVAL_ROOT", overrides)
            .map(PathBuf::from),
        min_proxy_exact: parse_env_f64_with_overrides("LONGMEMEVAL_MIN_PROXY_EXACT", overrides)?,
        min_proxy_f1: parse_env_f64_with_overrides("LONGMEMEVAL_MIN_PROXY_F1", overrides)?,
    };

    Ok(LongMemEvalSuiteOutcome::Completed(
        run(paths, &config).await?,
    ))
}

pub async fn run(paths: &Paths, config: &LongMemEvalRunConfig) -> Result<LongMemEvalRunOutput> {
    let raw = tokio::fs::read_to_string(&config.dataset_path)
        .await
        .with_context(|| {
            format!(
                "reading LongMemEval dataset {}",
                config.dataset_path.display()
            )
        })?;
    let mut questions: Vec<LongMemEvalQuestion> =
        serde_json::from_str(&raw).with_context(|| {
            format!(
                "parsing LongMemEval dataset {}",
                config.dataset_path.display()
            )
        })?;
    if let Some(limit) = config.limit {
        questions.truncate(limit);
    }
    if questions.is_empty() {
        bail!("LongMemEval dataset contains no questions")
    }

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| {
            format!(
                "creating LongMemEval output dir {}",
                config.output_dir.display()
            )
        })?;

    let predictions_path = config.output_dir.join("predictions.jsonl");
    let summary_path = config.output_dir.join("summary.json");
    let markdown_path = config.output_dir.join("summary.md");

    let mut prediction_lines = Vec::with_capacity(questions.len());
    let mut results = Vec::with_capacity(questions.len());
    let mut accumulator = LongMemEvalAccumulator::default();
    let mut by_type: HashMap<String, LongMemEvalAccumulator> = HashMap::new();

    for question in &questions {
        let history = build_history(question, &config.agent);
        let hypothesis = match config.mode {
            LongMemEvalMode::Harkonnen => {
                let prompt = build_question_prompt(question);
                chat::complete_agent_reply(&config.agent, &prompt, &history, None, paths)
                    .await
                    .with_context(|| {
                        format!(
                            "answering LongMemEval question {} via PackChat",
                            question.question_id
                        )
                    })?
            }
            LongMemEvalMode::Direct => complete_direct_reply(paths, config, question, &history)
                .await
                .with_context(|| {
                    format!(
                        "answering LongMemEval question {} via direct provider",
                        question.question_id
                    )
                })?,
        };

        let result = evaluate_question(question, hypothesis);
        accumulator.add(&result);
        by_type
            .entry(result.question_type.clone())
            .or_default()
            .add(&result);
        prediction_lines.push(serde_json::to_string(&LongMemEvalPrediction {
            question_id: &result.question_id,
            hypothesis: &result.hypothesis,
        })?);
        results.push(result);
    }

    let metrics = LongMemEvalMetrics {
        total: accumulator.total,
        exact_match: ratio(accumulator.exact_match, accumulator.total),
        contains_answer: ratio(accumulator.contains_answer, accumulator.total),
        token_f1: ratio(accumulator.token_f1, accumulator.total),
        abstention_question_accuracy: ratio(
            accumulator.abstention_correct as f64,
            accumulator.abstention_total,
        ),
        abstention_false_positive_rate: ratio(
            accumulator.false_positive_abstentions as f64,
            accumulator.non_abstention_total,
        ),
        answered_rate: ratio(accumulator.answered_total as f64, accumulator.total),
        by_question_type: finalize_by_type(by_type),
    };

    tokio::fs::write(
        &predictions_path,
        format!("{}\n", prediction_lines.join("\n")),
    )
    .await
    .with_context(|| {
        format!(
            "writing LongMemEval predictions {}",
            predictions_path.display()
        )
    })?;

    let official_eval = if let Some(command) = &config.official_eval_command {
        Some(run_official_eval(command, config, &predictions_path, paths).await?)
    } else {
        None
    };

    let summary = LongMemEvalSummary {
        dataset_path: config.dataset_path.display().to_string(),
        mode: config.mode.as_str().to_string(),
        agent: config.agent.clone(),
        provider_label: provider_label(config),
        limit: config.limit,
        generated_at: Utc::now().to_rfc3339(),
        metrics: metrics.clone(),
        predictions_path: predictions_path.display().to_string(),
        official_eval: official_eval.clone(),
        questions: results,
    };

    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing LongMemEval summary {}", summary_path.display()))?;

    let markdown = render_markdown_summary(&summary);
    tokio::fs::write(&markdown_path, markdown)
        .await
        .with_context(|| {
            format!(
                "writing LongMemEval markdown summary {}",
                markdown_path.display()
            )
        })?;

    let threshold_failure = threshold_failure(&metrics, config);

    Ok(LongMemEvalRunOutput {
        output_dir: config.output_dir.clone(),
        predictions_path,
        summary_path,
        markdown_path,
        mode: config.mode,
        provider_label: provider_label(config),
        metrics,
        official_eval,
        threshold_failure,
    })
}

pub fn render_step_stdout(output: &LongMemEvalRunOutput) -> String {
    let mut lines = vec![
        format!("LongMemEval mode: {}", output.mode.as_str()),
        format!("Provider path: {}", output.provider_label),
        format!("LongMemEval output dir: {}", output.output_dir.display()),
        format!("Predictions: {}", output.predictions_path.display()),
        format!("Summary JSON: {}", output.summary_path.display()),
        format!("Summary Markdown: {}", output.markdown_path.display()),
        format!("Questions: {}", output.metrics.total),
        format!("Proxy exact match: {:.4}", output.metrics.exact_match),
        format!(
            "Proxy contains-answer: {:.4}",
            output.metrics.contains_answer
        ),
        format!("Proxy token F1: {:.4}", output.metrics.token_f1),
        format!(
            "Abstention question accuracy: {:.4}",
            output.metrics.abstention_question_accuracy
        ),
        format!(
            "Abstention false positive rate: {:.4}",
            output.metrics.abstention_false_positive_rate
        ),
        format!("Answered rate: {:.4}", output.metrics.answered_rate),
    ];
    if let Some(eval) = &output.official_eval {
        lines.push(format!("Official eval command: {}", eval.command));
        lines.push(format!("Official eval exit code: {}", eval.exit_code));
        if !eval.stdout.trim().is_empty() {
            lines.push("Official eval stdout:".to_string());
            lines.push(eval.stdout.trim().to_string());
        }
        if !eval.stderr.trim().is_empty() {
            lines.push("Official eval stderr:".to_string());
            lines.push(eval.stderr.trim().to_string());
        }
    }
    if let Some(reason) = &output.threshold_failure {
        lines.push(format!("Threshold failure: {}", reason));
    }
    lines.join("\n")
}

pub fn status_for_output(output: &LongMemEvalRunOutput) -> BenchmarkStatus {
    if output.threshold_failure.is_some() {
        BenchmarkStatus::Failed
    } else {
        BenchmarkStatus::Passed
    }
}

pub fn reason_for_output(output: &LongMemEvalRunOutput) -> Option<String> {
    output.threshold_failure.clone()
}

fn build_history(question: &LongMemEvalQuestion, agent: &str) -> Vec<ChatMessage> {
    let mut sessions = question
        .haystack_sessions
        .iter()
        .enumerate()
        .map(|(idx, turns)| OrderedSession {
            idx,
            session_id: question
                .haystack_session_ids
                .get(idx)
                .cloned()
                .unwrap_or_default(),
            session_date: question
                .haystack_dates
                .get(idx)
                .cloned()
                .unwrap_or_default(),
            turns,
        })
        .collect::<Vec<_>>();

    sessions.sort_by(|left, right| {
        left.session_date
            .cmp(&right.session_date)
            .then(left.idx.cmp(&right.idx))
    });

    let mut history = Vec::new();
    for session in sessions {
        for (turn_idx, turn) in session.turns.iter().enumerate() {
            let role = if turn.role.eq_ignore_ascii_case("user") {
                "operator"
            } else {
                "agent"
            };
            let content = format_turn_content(
                &session.session_date,
                &session.session_id,
                turn_idx,
                &turn.content,
            );
            history.push(ChatMessage {
                message_id: format!("{}-{}-{}", question.question_id, session.idx, turn_idx),
                thread_id: question.question_id.clone(),
                role: role.to_string(),
                agent: Some(agent.to_string()),
                content,
                checkpoint_id: None,
                created_at: Utc::now(),
            });
        }
    }
    history
}

async fn complete_direct_reply(
    paths: &Paths,
    config: &LongMemEvalRunConfig,
    question: &LongMemEvalQuestion,
    history: &[ChatMessage],
) -> Result<String> {
    let provider = llm::build_provider("benchmark", &config.direct_provider, &paths.setup)
        .with_context(|| {
            format!(
                "no configured provider available for LongMemEval direct mode via {}",
                config.direct_provider
            )
        })?;

    let req = LlmRequest {
        messages: vec![Message::user(build_direct_prompt(question, history))],
        max_tokens: 512,
        temperature: 0.0,
    };

    provider
        .complete(req)
        .await
        .map(|resp| resp.content)
        .with_context(|| {
            format!(
                "LongMemEval direct provider failed via {}",
                config.direct_provider
            )
        })
}

fn build_direct_prompt(question: &LongMemEvalQuestion, history: &[ChatMessage]) -> String {
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
        "You are answering a memory question using only the transcript below.

Transcript:
{}

LongMemEval question type: {}
Question date: {}
Question: {}

Return only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the transcript does not contain enough information, reply exactly: I do not know.",
        transcript,
        question.question_type,
        question.question_date,
        question.question.trim()
    )
}

fn build_question_prompt(question: &LongMemEvalQuestion) -> String {
    format!(
        "LongMemEval question type: {}\nQuestion date: {}\nQuestion: {}\nAnswer using only the conversation history. Return only the bare answer text with no reasoning, no XML tags, no <think> blocks, and no preamble. If the history does not contain enough information, reply exactly: I do not know.",
        question.question_type,
        question.question_date,
        question.question.trim()
    )
}

fn evaluate_question(
    question: &LongMemEvalQuestion,
    raw_hypothesis: String,
) -> LongMemEvalQuestionResult {
    let hypothesis = extract_final_answer(&raw_hypothesis);
    let normalized_hypothesis = normalize_text(&hypothesis);
    let normalized_answer = normalize_text(&question.answer);
    let expected_abstention = is_abstention_question(question);
    let predicted_abstention = is_abstention_response(&normalized_hypothesis);

    LongMemEvalQuestionResult {
        question_id: question.question_id.clone(),
        question_type: if question.question_type.trim().is_empty() {
            "unknown".to_string()
        } else {
            question.question_type.clone()
        },
        question_date: question.question_date.clone(),
        expected_answer: question.answer.clone(),
        raw_hypothesis,
        hypothesis,
        exact_match: !normalized_hypothesis.is_empty()
            && normalized_hypothesis == normalized_answer,
        contains_answer: !normalized_hypothesis.is_empty()
            && !normalized_answer.is_empty()
            && (normalized_hypothesis.contains(&normalized_answer)
                || normalized_answer.contains(&normalized_hypothesis)),
        token_f1: token_f1(&normalized_hypothesis, &normalized_answer),
        expected_abstention,
        predicted_abstention,
    }
}

async fn run_official_eval(
    command: &str,
    config: &LongMemEvalRunConfig,
    predictions_path: &Path,
    paths: &Paths,
) -> Result<LongMemEvalOfficialEvalResult> {
    let cwd = config
        .official_eval_root
        .clone()
        .unwrap_or_else(|| paths.root.clone());
    let output = Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&cwd)
        .env("LONGMEMEVAL_PREDICTIONS_FILE", predictions_path)
        .env("LONGMEMEVAL_DATASET", &config.dataset_path)
        .output()
        .await
        .with_context(|| format!("running LongMemEval official eval command `{}`", command))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        bail!(
            "LongMemEval official eval command failed (exit {}): {}",
            exit_code,
            if stderr.is_empty() {
                stdout.clone()
            } else {
                stderr.clone()
            }
        );
    }

    Ok(LongMemEvalOfficialEvalResult {
        command: command.to_string(),
        cwd: cwd.display().to_string(),
        exit_code,
        stdout,
        stderr,
    })
}

fn render_markdown_summary(summary: &LongMemEvalSummary) -> String {
    let mut lines = vec![
        "# LongMemEval Summary".to_string(),
        String::new(),
        format!("- Dataset: {}", summary.dataset_path),
        format!("- Mode: {}", summary.mode),
        format!("- Agent: {}", summary.agent),
        format!("- Provider path: {}", summary.provider_label),
        format!("- Limit: {}", summary.limit.map(|v| v.to_string()).unwrap_or_else(|| "full".to_string())),
        format!("- Generated: {}", summary.generated_at),
        format!("- Predictions: {}", summary.predictions_path),
        String::new(),
        "## Aggregate Metrics".to_string(),
        String::new(),
        format!("- Questions: {}", summary.metrics.total),
        format!("- Proxy exact match: {:.4}", summary.metrics.exact_match),
        format!("- Proxy contains-answer: {:.4}", summary.metrics.contains_answer),
        format!("- Proxy token F1: {:.4}", summary.metrics.token_f1),
        format!(
            "- Abstention question accuracy: {:.4}",
            summary.metrics.abstention_question_accuracy
        ),
        format!(
            "- Abstention false positive rate: {:.4}",
            summary.metrics.abstention_false_positive_rate
        ),
        format!("- Answered rate: {:.4}", summary.metrics.answered_rate),
        String::new(),
        "## By Question Type".to_string(),
        String::new(),
        "| Type | Questions | Exact Match | Contains Answer | Token F1 | Abstention Accuracy | False Positive Abstention |".to_string(),
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |".to_string(),
    ];

    for (question_type, metrics) in &summary.metrics.by_question_type {
        lines.push(format!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
            question_type,
            metrics.total,
            metrics.exact_match,
            metrics.contains_answer,
            metrics.token_f1,
            metrics.abstention_question_accuracy,
            metrics.abstention_false_positive_rate,
        ));
    }

    if let Some(eval) = &summary.official_eval {
        lines.push(String::new());
        lines.push("## Official Eval".to_string());
        lines.push(String::new());
        lines.push(format!("- Command: {}", eval.command));
        lines.push(format!("- Working dir: {}", eval.cwd));
        lines.push(format!("- Exit code: {}", eval.exit_code));
        if !eval.stdout.is_empty() {
            lines.push(String::new());
            lines.push("```text".to_string());
            lines.push(eval.stdout.clone());
            lines.push("```".to_string());
        }
        if !eval.stderr.is_empty() {
            lines.push(String::new());
            lines.push("```text".to_string());
            lines.push(eval.stderr.clone());
            lines.push("```".to_string());
        }
    }

    lines.push(String::new());
    lines.join("\n")
}

fn threshold_failure(
    metrics: &LongMemEvalMetrics,
    config: &LongMemEvalRunConfig,
) -> Option<String> {
    if let Some(min_exact) = config.min_proxy_exact {
        if metrics.exact_match < min_exact {
            return Some(format!(
                "proxy exact match {:.4} below LONGMEMEVAL_MIN_PROXY_EXACT {:.4}",
                metrics.exact_match, min_exact
            ));
        }
    }
    if let Some(min_f1) = config.min_proxy_f1 {
        if metrics.token_f1 < min_f1 {
            return Some(format!(
                "proxy token F1 {:.4} below LONGMEMEVAL_MIN_PROXY_F1 {:.4}",
                metrics.token_f1, min_f1
            ));
        }
    }
    None
}

fn finalize_by_type(
    by_type: HashMap<String, LongMemEvalAccumulator>,
) -> BTreeMap<String, LongMemEvalQuestionTypeMetrics> {
    let mut ordered = BTreeMap::new();
    for (question_type, acc) in by_type {
        ordered.insert(question_type, acc.finalize());
    }
    ordered
}

fn format_turn_content(
    session_date: &str,
    session_id: &str,
    turn_idx: usize,
    content: &str,
) -> String {
    let mut parts = Vec::new();
    if !session_date.trim().is_empty() {
        parts.push(session_date.trim().to_string());
    }
    if !session_id.trim().is_empty() {
        parts.push(format!("session {}", session_id.trim()));
    }
    parts.push(format!("turn {}", turn_idx + 1));
    format!("[{}] {}", parts.join(" | "), content.trim())
}

fn is_abstention_question(question: &LongMemEvalQuestion) -> bool {
    question.question_id.ends_with("_abs")
        || question.question_type.eq_ignore_ascii_case("abstention")
}

fn is_abstention_response(normalized_hypothesis: &str) -> bool {
    DEFAULT_ABSTENTION_PHRASES
        .iter()
        .any(|phrase| normalized_hypothesis.contains(&normalize_text(phrase)))
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
    let recall = overlap as f64
        / answer_counts
            .values()
            .sum::<usize>()
            .saturating_add(overlap) as f64;
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

fn provider_label(config: &LongMemEvalRunConfig) -> String {
    match config.mode {
        LongMemEvalMode::Harkonnen => format!("PackChat agent {}", config.agent),
        LongMemEvalMode::Direct => format!("Direct provider {}", config.direct_provider),
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

struct OrderedSession<'a> {
    idx: usize,
    session_id: String,
    session_date: String,
    turns: &'a [LongMemEvalTurn],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_text_collapses_punctuation() {
        assert_eq!(normalize_text("Hello, World!"), "hello world");
    }

    #[test]
    fn token_f1_handles_exact_match() {
        assert!((token_f1("blue house", "blue house") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn abstention_detection_catches_common_phrases() {
        assert!(is_abstention_response(&normalize_text(
            "I do not know based on the conversation."
        )));
    }

    #[test]
    fn extract_final_answer_strips_think_blocks() {
        let raw = "<think>reasoning</think>

Grilled cheese.";
        assert_eq!(extract_final_answer(raw), "Grilled cheese.");
    }

    #[test]
    fn extract_final_answer_prefers_last_non_empty_line() {
        let raw = "Reasoning line
Answer: final output";
        assert_eq!(extract_final_answer(raw), "final output");
    }

    #[test]
    fn longmemeval_mode_parses_synonyms() {
        assert!(matches!(
            LongMemEvalMode::parse(Some("packchat".to_string())).unwrap(),
            LongMemEvalMode::Harkonnen
        ));
        assert!(matches!(
            LongMemEvalMode::parse(Some("raw".to_string())).unwrap(),
            LongMemEvalMode::Direct
        ));
    }
}
