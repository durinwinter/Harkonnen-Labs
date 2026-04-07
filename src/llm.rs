use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::capacity::CapacityState;
use crate::setup::SetupConfig;

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

/// Resolved response from any provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
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
        let Ok(api_key) = std::env::var(&provider_cfg.api_key_env) else {
            continue;
        };
        let client: Box<dyn LlmProvider> = match provider_cfg.provider_type.as_str() {
            "anthropic" | "claude" => Box::new(AnthropicClient {
                api_key,
                model: provider_cfg.model.clone(),
                http: build_http_client(),
            }),
            "gemini" | "google" => Box::new(GeminiClient {
                api_key,
                model: provider_cfg.model.clone(),
                http: build_http_client(),
            }),
            "openai" | "codex" => Box::new(OpenAiClient {
                api_key,
                model: provider_cfg.model.clone(),
                http: build_http_client(),
            }),
            _ => continue,
        };
        return Some(client);
    }
    None
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build HTTP client")
}

// ── Anthropic ─────────────────────────────────────────────────────────────────

struct AnthropicClient {
    api_key: String,
    model: String,
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

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Anthropic API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Anthropic API error {}: {}", status, body);
        }

        let parsed: AnthropicResponse = resp.json().await.context("parsing Anthropic response")?;

        let content = parsed
            .content
            .into_iter()
            .map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse { content })
    }
}

// ── Gemini ────────────────────────────────────────────────────────────────────

struct GeminiClient {
    api_key: String,
    model: String,
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
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiCandidateContent,
}

#[derive(Deserialize)]
struct GeminiCandidateContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: String,
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
            },
        };

        // Gemini model IDs use "gemini-2.0-flash" style; strip any "models/" prefix if present
        let model = self.model.trim_start_matches("models/");
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, self.api_key
        );

        let resp = self
            .http
            .post(&url)
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

        let parsed: GeminiResponse = resp.json().await.context("parsing Gemini response")?;

        let content = parsed
            .candidates
            .into_iter()
            .flat_map(|c| c.content.parts)
            .map(|p| p.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse { content })
    }
}

// ── OpenAI / Codex ────────────────────────────────────────────────────────────

struct OpenAiClient {
    api_key: String,
    model: String,
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

        let resp = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("OpenAI API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("OpenAI API error {}: {}", status, body);
        }

        let parsed: OpenAiResponse = resp.json().await.context("parsing OpenAI response")?;

        let content = parsed
            .choices
            .into_iter()
            .map(|c| c.message.content)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse { content })
    }
}
