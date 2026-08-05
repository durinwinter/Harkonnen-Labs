use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;

use crate::capacity::CapacityState;
use crate::setup::{ProviderConfig, SetupConfig};

// ── Public interface ──────────────────────────────────────────────────────────

/// A single turn in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// Parameters for an LLM call.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    /// 0.0–1.0
    pub temperature: f32,
}

impl LlmRequest {
    pub fn simple(system: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::system(system), Message::user(user)],
            max_tokens: 4096,
            temperature: 0.2,
        }
    }
}

/// Token and latency usage for a single LLM call.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LlmUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
}

impl LlmUsage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// Resolved response from any provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub usage: Option<LlmUsage>,
}

/// Typed provider backend for isolated sub-agents and provider-specific routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderBackend {
    Anthropic {
        model: String,
        api_key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    OpenAi {
        model: String,
        api_key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    Gemini {
        model: String,
        api_key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
}

impl ProviderBackend {
    pub fn from_config(config: &ProviderConfig) -> Option<Self> {
        match config.provider_type.as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic {
                model: config.model.clone(),
                api_key_env: config.api_key_env.clone(),
                base_url: config.base_url.clone(),
            }),
            "openai" | "codex" | "openai_compatible" | "openai-compatible" | "lmstudio"
            | "lm_studio" | "lm-studio" => Some(Self::OpenAi {
                model: config.model.clone(),
                api_key_env: config.api_key_env.clone(),
                base_url: config.base_url.clone(),
            }),
            "gemini" | "google" => Some(Self::Gemini {
                model: config.model.clone(),
                api_key_env: config.api_key_env.clone(),
                base_url: config.base_url.clone(),
            }),
            _ => None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        match &mut self {
            Self::Anthropic { model: current, .. }
            | Self::OpenAi { model: current, .. }
            | Self::Gemini { model: current, .. } => *current = model,
        }
        self
    }
}

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse>;
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Build the right `LlmProvider` for a given agent using the active setup.
/// Returns `None` if the provider is disabled or has no API key in the environment.
pub fn build_provider(
    agent_name: &str,
    agent_provider: &str,
    setup: &SetupConfig,
) -> Option<Box<dyn LlmProvider>> {
    build_provider_with_capacity(agent_name, agent_provider, setup, None)
}

pub fn build_provider_backend(
    agent_name: &str,
    agent_provider: &str,
    setup: &SetupConfig,
) -> Option<ProviderBackend> {
    let resolved_name = setup.resolve_agent_provider_name(agent_name, agent_provider);
    let provider_cfg = setup.resolve_provider(&resolved_name)?;
    if provider_cfg.enabled {
        ProviderBackend::from_config(provider_cfg)
    } else {
        None
    }
}

#[allow(dead_code)]
pub async fn complete(backend: &ProviderBackend, messages: &[Message]) -> Result<String> {
    complete_request(
        backend,
        LlmRequest {
            messages: messages.to_vec(),
            max_tokens: 4096,
            temperature: 0.2,
        },
    )
    .await
    .map(|response| response.content)
}

pub async fn complete_request(backend: &ProviderBackend, req: LlmRequest) -> Result<LlmResponse> {
    let provider = provider_from_backend(backend)?;
    provider.complete(req).await
}

fn provider_from_backend(backend: &ProviderBackend) -> Result<Box<dyn LlmProvider>> {
    match backend {
        ProviderBackend::Anthropic {
            model,
            api_key_env,
            base_url,
        } => {
            let api_key = env::var(api_key_env)
                .with_context(|| format!("missing Anthropic API key env {api_key_env}"))?;
            Ok(Box::new(AnthropicClient {
                api_key,
                model: model.clone(),
                base_url: anthropic_messages_url(base_url.as_deref()),
                http: build_http_client(),
            }))
        }
        ProviderBackend::OpenAi {
            model,
            api_key_env,
            base_url,
        } => {
            let api_key = optional_api_key(api_key_env)
                .with_context(|| format!("missing OpenAI API key env {api_key_env}"))?;
            Ok(Box::new(OpenAiClient {
                api_key,
                model: model.clone(),
                base_url: openai_chat_completions_url(base_url.as_deref()),
                http: build_http_client(),
            }))
        }
        ProviderBackend::Gemini {
            model,
            api_key_env,
            base_url,
        } => {
            let api_key = env::var(api_key_env)
                .with_context(|| format!("missing Gemini API key env {api_key_env}"))?;
            Ok(Box::new(GeminiClient {
                base_url: gemini_generate_content_url(base_url.as_deref(), model, &api_key),
                http: build_http_client(),
            }))
        }
    }
}

/// Capacity-aware provider builder. If the resolved provider is unavailable, falls back
/// to the next available provider in the capacity state. Returns `None` only if no
/// available provider with a valid API key can be found.
pub fn build_provider_with_capacity(
    agent_name: &str,
    agent_provider: &str,
    setup: &SetupConfig,
    capacity: Option<&CapacityState>,
) -> Option<Box<dyn LlmProvider>> {
    let resolved_name = setup.resolve_agent_provider_name(agent_name, agent_provider);

    // Build ordered list of providers to try: preferred first, then fallbacks.
    let candidates: Vec<String> = if let Some(cap) = capacity {
        match cap.fallback_for(&resolved_name) {
            None => vec![resolved_name.clone()], // preferred is available (or unknown) — use it
            Some(fb) => vec![fb],                // preferred is unavailable — use best alternative
        }
    } else {
        vec![resolved_name.clone()]
    };

    for candidate in &candidates {
        let provider_cfg = setup.resolve_provider(candidate)?;
        if !provider_cfg.enabled {
            continue;
        }
        let client: Box<dyn LlmProvider> = match provider_cfg.provider_type.as_str() {
            "anthropic" | "claude" => {
                let Ok(api_key) = std::env::var(&provider_cfg.api_key_env) else {
                    continue;
                };
                Box::new(AnthropicClient {
                    api_key,
                    model: provider_cfg.model.clone(),
                    base_url: anthropic_messages_url(provider_cfg.base_url.as_deref()),
                    http: build_http_client(),
                })
            }
            "gemini" | "google" => {
                let Ok(api_key) = std::env::var(&provider_cfg.api_key_env) else {
                    continue;
                };
                Box::new(GeminiClient {
                    base_url: gemini_generate_content_url(
                        provider_cfg.base_url.as_deref(),
                        &provider_cfg.model,
                        &api_key,
                    ),
                    http: build_http_client(),
                })
            }
            "openai" | "codex" | "openai_compatible" | "openai-compatible" | "lmstudio"
            | "lm_studio" | "lm-studio" => {
                let Ok(api_key) = optional_api_key(&provider_cfg.api_key_env) else {
                    continue;
                };
                Box::new(OpenAiClient {
                    api_key,
                    model: provider_cfg.model.clone(),
                    base_url: openai_chat_completions_url(provider_cfg.base_url.as_deref()),
                    http: build_http_client(),
                })
            }
            _ => continue,
        };
        return Some(client);
    }
    None
}

fn optional_api_key(api_key_env: &str) -> Result<Option<String>> {
    if api_key_env.trim().is_empty() {
        return Ok(None);
    }
    env::var(api_key_env)
        .map(Some)
        .with_context(|| format!("missing API key env {api_key_env}"))
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(http_timeout_secs()))
        .build()
        .expect("failed to build HTTP client")
}

fn http_timeout_secs() -> u64 {
    env::var("HARKONNEN_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(120)
}

fn openai_chat_completions_url(base_url: Option<&str>) -> String {
    let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return "https://api.openai.com/v1/chat/completions".to_string();
    };

    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn anthropic_messages_url(base_url: Option<&str>) -> String {
    let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return "https://api.anthropic.com/v1/messages".to_string();
    };

    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

fn gemini_generate_content_url(base_url: Option<&str>, model: &str, api_key: &str) -> String {
    let model = model.trim_start_matches("models/");
    let base = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let trimmed = value.trim_end_matches('/');
            if trimmed.contains("{model}") {
                trimmed.replace("{model}", model)
            } else if trimmed.ends_with(":generateContent") {
                trimmed.to_string()
            } else if trimmed.ends_with("/v1beta") {
                format!("{trimmed}/models/{model}:generateContent")
            } else {
                format!("{trimmed}/v1beta/models/{model}:generateContent")
            }
        })
        .unwrap_or_else(|| {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                model
            )
        });

    if base.contains("?") {
        format!("{base}&key={api_key}")
    } else {
        format!("{base}?key={api_key}")
    }
}

// ── Anthropic ─────────────────────────────────────────────────────────────────

struct AnthropicClient {
    api_key: String,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Deserialize, Default)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let system = req
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let messages: Vec<AnthropicMessage> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| AnthropicMessage {
                role: &m.role,
                content: &m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            system,
            messages,
        };

        // Retry up to 3 times on 429 rate-limit responses, honouring Retry-After.
        let max_attempts = 3u32;
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let t0 = std::time::Instant::now();
            let resp = self
                .http
                .post(&self.base_url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .context("Anthropic API request failed")?;

            let status = resp.status();
            if status.as_u16() == 429 && attempt < max_attempts {
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(65);
                tracing::warn!(
                    attempt,
                    retry_after,
                    "Anthropic rate limit hit — waiting {retry_after}s before retry"
                );
                tokio::time::sleep(Duration::from_secs(retry_after)).await;
                continue;
            }

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                bail!("Anthropic API error {}: {}", status, body);
            }

            let latency_ms = t0.elapsed().as_millis() as u64;
            let parsed: AnthropicResponse =
                resp.json().await.context("parsing Anthropic response")?;

            let usage = Some(LlmUsage {
                input_tokens: parsed.usage.input_tokens,
                output_tokens: parsed.usage.output_tokens,
                latency_ms,
            });
            let content = parsed
                .content
                .into_iter()
                .map(|c| c.text)
                .collect::<Vec<_>>()
                .join("");

            return Ok(LlmResponse { content, usage });
        }
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────────

struct GeminiClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
    temperature: f32,
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    thinking_config: Option<GeminiThinkingConfig>,
}

/// Gemini 2.5+ and 3.x bill *reasoning* tokens against `maxOutputTokens`. A
/// thinking-heavy prompt can therefore spend the entire budget before emitting
/// a single token of answer: measured against `gemini-3.5-flash` with a 300
/// token budget, `thoughtsTokenCount` came back 284 and the reply was a 12
/// token fragment. Sending `thinkingBudget: 0` moved all 296 tokens to the
/// answer, so the budget is spent on output rather than on deliberation.
///
/// This matters here more than it would elsewhere: Mason's edit lane must
/// return whole files in one response, and a starved reply reaches the parser
/// looking like a malformed one, which sends the caller hunting a formatting
/// bug that does not exist.
#[derive(Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingBudget")]
    thinking_budget: i32,
}

/// Reasoning-token budget for Gemini calls.
///
/// The default is a small *positive* allowance rather than `0`. Zero is the
/// obvious choice — it gives the whole budget to the answer — but not every
/// model may disable thinking: `gemini-3.6-flash` rejects `thinkingBudget: 0`
/// with a bare `400 INVALID_ARGUMENT`, while accepting 128, 512 and 1024. A
/// zero default therefore breaks that model outright for anyone who never sets
/// the variable, trading a silent starvation bug for a hard failure. A small
/// allowance is accepted everywhere tested and still leaves the bulk of
/// `maxOutputTokens` for the answer.
///
/// Set `GEMINI_THINKING_BUDGET` to any positive token count to widen it, to `0`
/// to disable thinking on models that permit it (2.5 and 3.5 do), or to `-1` to
/// omit the field and let the model decide — the pre-fix behavior, which
/// starves long generations.
const GEMINI_DEFAULT_THINKING_BUDGET: i32 = 512;

fn gemini_thinking_budget() -> Option<i32> {
    let configured = std::env::var("GEMINI_THINKING_BUDGET")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .unwrap_or(GEMINI_DEFAULT_THINKING_BUDGET);
    if configured < 0 {
        None
    } else {
        Some(configured)
    }
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata", default)]
    usage_metadata: GeminiUsageMetadata,
}

#[derive(Deserialize, Default)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
    #[serde(rename = "thoughtsTokenCount", default)]
    thoughts_token_count: u32,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    /// Absent when the candidate carries no content at all — which is exactly
    /// what a fully thought-starved response looks like.
    #[serde(default)]
    content: Option<GeminiCandidateContent>,
    #[serde(rename = "finishReason", default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiCandidateContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    /// A thought part carries no `text`, so this must tolerate its absence
    /// rather than failing the whole response.
    #[serde(default)]
    text: String,
}

/// Turn a parsed Gemini response into answer text, refusing anything the caller
/// would otherwise mistake for a complete reply.
///
/// Two failures are indistinguishable from success downstream unless caught
/// here: an empty answer, and one cut off at the token ceiling. Both previously
/// returned `Ok("")` or `Ok(<fragment>)`, so the caller reported "the model
/// returned nothing parseable" and the real cause — the budget — stayed hidden.
fn gemini_content_from_response(parsed: &GeminiResponse) -> Result<String> {
    let finish_reason = parsed
        .candidates
        .first()
        .and_then(|candidate| candidate.finish_reason.clone())
        .unwrap_or_default();

    let content = parsed
        .candidates
        .iter()
        .filter_map(|candidate| candidate.content.as_ref())
        .flat_map(|content| content.parts.iter())
        .map(|part| part.text.as_str())
        .collect::<Vec<_>>()
        .join("");

    let thoughts = parsed.usage_metadata.thoughts_token_count;
    let answered = parsed.usage_metadata.candidates_token_count;

    if finish_reason == "MAX_TOKENS" {
        bail!(
            "Gemini stopped at the output token ceiling (finishReason=MAX_TOKENS) after \
             emitting {answered} answer token(s) and spending {thoughts} on reasoning. The \
             reply is truncated, not malformed. Reasoning tokens are billed against \
             maxOutputTokens, so either raise the caller's token budget or lower \
             GEMINI_THINKING_BUDGET (currently {budget}).",
            budget = gemini_thinking_budget()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unset — model decides".to_string())
        );
    }

    if content.trim().is_empty() {
        bail!(
            "Gemini returned an empty response (finishReason={reason}, {thoughts} reasoning \
             token(s), {answered} answer token(s)). An empty body is a failed call, not an \
             empty answer.",
            reason = if finish_reason.is_empty() {
                "unreported"
            } else {
                &finish_reason
            }
        );
    }

    Ok(content)
}

#[async_trait::async_trait]
impl LlmProvider for GeminiClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let system_text = req
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let system_instruction = system_text.map(|text| GeminiContent {
            role: "user".into(), // Gemini system instruction uses "user" role in this field
            parts: vec![GeminiPart { text }],
        });

        let contents: Vec<GeminiContent> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                GeminiContent {
                    role: role.to_string(),
                    parts: vec![GeminiPart {
                        text: m.content.clone(),
                    }],
                }
            })
            .collect();

        let body = GeminiRequest {
            contents,
            system_instruction,
            generation_config: GeminiGenerationConfig {
                max_output_tokens: req.max_tokens,
                temperature: req.temperature,
                thinking_config: gemini_thinking_budget()
                    .map(|thinking_budget| GeminiThinkingConfig { thinking_budget }),
            },
        };

        let t0 = std::time::Instant::now();
        let resp = self
            .http
            .post(&self.base_url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Gemini API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Gemini API error {}: {}", status, body);
        }

        let latency_ms = t0.elapsed().as_millis() as u64;
        let parsed: GeminiResponse = resp.json().await.context("parsing Gemini response")?;

        let usage = Some(LlmUsage {
            input_tokens: parsed.usage_metadata.prompt_token_count,
            output_tokens: parsed.usage_metadata.candidates_token_count,
            latency_ms,
        });
        let content = gemini_content_from_response(&parsed)?;

        Ok(LlmResponse { content, usage })
    }
}

// ── OpenAI / Codex ────────────────────────────────────────────────────────────

struct OpenAiClient {
    api_key: Option<String>,
    model: String,
    base_url: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<OpenAiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: OpenAiUsage,
}

#[derive(Deserialize, Default)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Deserialize)]
struct OpenAiChoiceMessage {
    content: String,
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {
        let messages: Vec<OpenAiMessage> = req
            .messages
            .iter()
            .map(|m| OpenAiMessage {
                role: &m.role,
                content: &m.content,
            })
            .collect();

        let body = OpenAiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            messages,
        };

        let t0 = std::time::Instant::now();
        let mut request = self
            .http
            .post(&self.base_url)
            .header("content-type", "application/json")
            .json(&body);
        if let Some(api_key) = self.api_key.as_deref().filter(|key| !key.is_empty()) {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let resp = request.send().await.context("OpenAI API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("OpenAI API error {}: {}", status, body);
        }

        let latency_ms = t0.elapsed().as_millis() as u64;
        let parsed: OpenAiResponse = resp.json().await.context("parsing OpenAI response")?;

        let usage = Some(LlmUsage {
            input_tokens: parsed.usage.prompt_tokens,
            output_tokens: parsed.usage.completion_tokens,
            latency_ms,
        });
        let content = parsed
            .choices
            .into_iter()
            .map(|c| c.message.content)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse { content, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        anthropic_messages_url, gemini_content_from_response, gemini_generate_content_url,
        openai_chat_completions_url, optional_api_key, GeminiResponse, ProviderBackend,
    };
    use crate::setup::ProviderConfig;

    fn parse_gemini(raw: &str) -> GeminiResponse {
        serde_json::from_str(raw).expect("fixture must deserialize")
    }

    /// Captured verbatim from `gemini-3.5-flash` with `maxOutputTokens: 300`:
    /// 284 of the 300 tokens went to reasoning and the answer was a fragment.
    /// The old parser returned that fragment as if it were the whole reply.
    #[test]
    fn gemini_truncated_by_thinking_is_an_error_not_a_fragment() {
        let response = parse_gemini(
            r#"{
              "candidates": [{
                "content": {"parts": [{"text": "To create a rich, interactive"}], "role": "model"},
                "finishReason": "MAX_TOKENS"
              }],
              "usageMetadata": {
                "promptTokenCount": 40,
                "candidatesTokenCount": 12,
                "thoughtsTokenCount": 284
              }
            }"#,
        );

        let err = gemini_content_from_response(&response)
            .expect_err("a truncated reply must not be reported as success");
        let message = err.to_string();
        assert!(message.contains("MAX_TOKENS"), "message was: {message}");
        assert!(message.contains("284"), "message was: {message}");
    }

    /// A fully thought-starved candidate carries no `content` at all. This used
    /// to fail deserialization or collapse to `Ok("")` depending on the shape.
    #[test]
    fn gemini_candidate_without_content_is_an_error() {
        let response = parse_gemini(
            r#"{
              "candidates": [{"finishReason": "STOP"}],
              "usageMetadata": {"thoughtsTokenCount": 512, "candidatesTokenCount": 0}
            }"#,
        );

        let err = gemini_content_from_response(&response)
            .expect_err("an empty body is a failed call, not an empty answer");
        assert!(err.to_string().contains("empty"), "message was: {err}");
    }

    /// Thought parts arrive without a `text` field. They must not break parsing,
    /// but a response consisting only of them still has no answer in it.
    #[test]
    fn gemini_thought_only_parts_do_not_count_as_an_answer() {
        let response = parse_gemini(
            r#"{
              "candidates": [{
                "content": {"parts": [{"thoughtSignature": "EroJCrcJARFNMg"}], "role": "model"},
                "finishReason": "STOP"
              }],
              "usageMetadata": {"thoughtsTokenCount": 300, "candidatesTokenCount": 0}
            }"#,
        );

        let err = gemini_content_from_response(&response)
            .expect_err("thought-only parts carry no answer");
        assert!(err.to_string().contains("empty"), "message was: {err}");
    }

    /// `gemini-3.6-flash` answers `thinkingBudget: 0` with a bare 400, so the
    /// default must not be zero or that model fails for every caller who never
    /// sets the variable.
    #[test]
    fn gemini_default_thinking_budget_is_positive() {
        assert!(
            super::GEMINI_DEFAULT_THINKING_BUDGET > 0,
            "a zero default is rejected outright by gemini-3.6-flash"
        );
    }

    #[test]
    fn gemini_complete_response_returns_joined_text() {
        let response = parse_gemini(
            r#"{
              "candidates": [{
                "content": {"parts": [{"text": "var a = 1;"}, {"text": "\nvar b = 2;"}], "role": "model"},
                "finishReason": "STOP"
              }],
              "usageMetadata": {"candidatesTokenCount": 12}
            }"#,
        );

        assert_eq!(
            gemini_content_from_response(&response).expect("a complete reply must parse"),
            "var a = 1;\nvar b = 2;"
        );
    }

    #[test]
    fn openai_base_url_defaults_to_public_api() {
        assert_eq!(
            openai_chat_completions_url(None),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_base_url_appends_v1_chat_completions() {
        assert_eq!(
            openai_chat_completions_url(Some("http://localhost:11434")),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn openai_base_url_respects_existing_v1_suffix() {
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn openai_base_url_accepts_full_endpoint() {
        assert_eq!(
            openai_chat_completions_url(Some("http://localhost:1234/v1/chat/completions")),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn anthropic_base_url_defaults_to_public_api() {
        assert_eq!(
            anthropic_messages_url(None),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn anthropic_base_url_appends_messages_endpoint() {
        assert_eq!(
            anthropic_messages_url(Some("https://gateway.example.com/anthropic")),
            "https://gateway.example.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn gemini_base_url_defaults_to_public_api() {
        assert_eq!(
            gemini_generate_content_url(None, "gemini-2.0-flash", "test-key"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=test-key"
        );
    }

    #[test]
    fn gemini_base_url_accepts_template_path() {
        assert_eq!(
            gemini_generate_content_url(
                Some("https://gateway.example.com/google/v1beta/models/{model}:generateContent"),
                "models/gemini-2.0-flash",
                "test-key"
            ),
            "https://gateway.example.com/google/v1beta/models/gemini-2.0-flash:generateContent?key=test-key"
        );
    }

    #[test]
    fn provider_backend_maps_openai_config() {
        let cfg = ProviderConfig {
            provider_type: "openai".to_string(),
            model: "gpt-5.1".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            enabled: true,
            credential_kind: None,
            usage_rights: None,
            surface: None,
            base_url: Some("https://gateway.example.com/openai".to_string()),
        };

        assert_eq!(
            ProviderBackend::from_config(&cfg),
            Some(ProviderBackend::OpenAi {
                model: "gpt-5.1".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                base_url: Some("https://gateway.example.com/openai".to_string()),
            })
        );
    }

    #[test]
    fn provider_backend_model_override_preserves_credentials() {
        let backend = ProviderBackend::Anthropic {
            model: "claude-sonnet-4-6".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            base_url: None,
        }
        .with_model("claude-opus-4-6");

        assert_eq!(
            backend,
            ProviderBackend::Anthropic {
                model: "claude-opus-4-6".to_string(),
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
                base_url: None,
            }
        );
    }

    #[test]
    fn provider_backend_maps_lmstudio_as_openai_compatible() {
        let cfg = ProviderConfig {
            provider_type: "lmstudio".to_string(),
            model: "local/llama".to_string(),
            api_key_env: "".to_string(),
            enabled: true,
            credential_kind: None,
            usage_rights: None,
            surface: None,
            base_url: Some("http://localhost:1234".to_string()),
        };

        assert_eq!(
            ProviderBackend::from_config(&cfg),
            Some(ProviderBackend::OpenAi {
                model: "local/llama".to_string(),
                api_key_env: "".to_string(),
                base_url: Some("http://localhost:1234".to_string()),
            })
        );
    }

    #[test]
    fn optional_api_key_allows_local_openai_compatible_without_auth() {
        assert_eq!(optional_api_key("").expect("optional key"), None);
        assert_eq!(optional_api_key("   ").expect("optional key"), None);
    }
}
