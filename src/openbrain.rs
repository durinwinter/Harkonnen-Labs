use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::memory::SemanticMemoryMetadata;
use crate::setup::OpenBrainConfig;

#[derive(Debug, Clone)]
pub struct OpenBrainClient {
    connection_url: String,
    http: Client,
    search_limit: usize,
    search_threshold: f64,
}

impl OpenBrainClient {
    pub fn new(config: &OpenBrainConfig) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }

        let connection_url = if let Some(url) = config.connection_url.as_ref() {
            Some(url.clone())
        } else {
            std::env::var(&config.connection_url_env).ok()
        };

        let Some(mut connection_url) = connection_url else {
            tracing::info!(
                env = %config.connection_url_env,
                "Open Brain enabled but no MCP connection URL is configured"
            );
            return Ok(None);
        };

        if !connection_url.contains("key=") {
            if let Ok(key) = std::env::var(&config.access_key_env) {
                let separator = if connection_url.contains('?') {
                    '&'
                } else {
                    '?'
                };
                connection_url = format!("{connection_url}{separator}key={key}");
            }
        }

        Ok(Some(Self {
            connection_url,
            http: Client::builder()
                .timeout(Duration::from_millis(config.timeout_ms.max(250)))
                .build()
                .context("building Open Brain HTTP client")?,
            search_limit: config.search_limit,
            search_threshold: config.search_threshold,
        }))
    }

    pub async fn search_thoughts(&self, query: &str) -> Result<Vec<String>> {
        self.search_thoughts_limited(query, self.search_limit).await
    }

    pub async fn search_thoughts_limited(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let response = self
            .call_tool(
                "search_thoughts",
                serde_json::json!({
                    "query": query,
                    "limit": limit.min(self.search_limit).max(1),
                    "threshold": self.search_threshold,
                }),
            )
            .await?;
        Ok(extract_text_content(&response)
            .into_iter()
            .map(|text| format!("[open-brain] {text}"))
            .collect())
    }

    pub async fn capture_thought(&self, content: &str, thought_type: Option<&str>) -> Result<()> {
        let mut args = serde_json::json!({ "content": content });
        if let Some(thought_type) = thought_type {
            args["type"] = Value::String(thought_type.to_string());
        }
        self.call_tool("capture_thought", args).await?;
        Ok(())
    }

    pub async fn capture_thought_with_metadata(
        &self,
        content: &str,
        metadata: &SemanticMemoryMetadata,
    ) -> Result<()> {
        let mut args = serde_json::json!({ "content": content });
        if let Some(memory_type) = metadata.memory_type.as_deref() {
            args["type"] = Value::String(memory_type.to_string());
        }
        let metadata_value = serde_json::to_value(metadata)?;
        if metadata_value
            .as_object()
            .map(|object| object.values().any(|value| !value.is_null()))
            .unwrap_or(false)
        {
            args["metadata"] = metadata_value;
        }
        self.call_tool("capture_thought", args).await?;
        Ok(())
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("harkonnen-{name}"),
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        });

        let response = self
            .http
            .post(&self.connection_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST Open Brain MCP tool {name}"))?;

        let status = response.status();
        let value: Value = response
            .json()
            .await
            .with_context(|| format!("decoding Open Brain MCP response for {name}"))?;
        if !status.is_success() {
            anyhow::bail!("Open Brain MCP returned HTTP {status}: {value}");
        }
        if let Some(error) = value.get("error") {
            anyhow::bail!("Open Brain MCP tool {name} failed: {error}");
        }
        Ok(value.get("result").cloned().unwrap_or(value))
    }
}

fn extract_text_content(value: &Value) -> Vec<String> {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
