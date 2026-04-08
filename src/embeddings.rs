use anyhow::{bail, Context, Result};
use chrono::Utc;
#[cfg(feature = "local-embeddings")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
#[cfg(feature = "local-embeddings")]
use std::sync::Mutex;
use std::time::Duration;

use crate::{memory::MemoryEntry, setup::SetupConfig};

#[derive(Clone)]
enum EmbeddingBackend {
    #[cfg(feature = "local-embeddings")]
    Fastembed { model: Arc<Mutex<TextEmbedding>> },
    OpenAiCompatible {
        api_key: String,
        model: String,
        url: String,
        http: Client,
    },
}

struct RemoteEmbeddingBackend {
    backend: EmbeddingBackend,
    backend_id: String,
    model_id: String,
    description: String,
}

/// Semantic memory layer: stores embeddings in SQLite under the
/// `memory_embeddings` table and can use either a local fastembed model or an
/// OpenAI-compatible embeddings endpoint such as LM Studio.
pub struct EmbeddingStore {
    backend: Arc<EmbeddingBackend>,
    backend_id: String,
    model_id: String,
    description: String,
    pool: SqlitePool,
}

impl std::fmt::Debug for EmbeddingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingStore")
            .field("backend_id", &self.backend_id)
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

impl Clone for EmbeddingStore {
    fn clone(&self) -> Self {
        Self {
            backend: Arc::clone(&self.backend),
            backend_id: self.backend_id.clone(),
            model_id: self.model_id.clone(),
            description: self.description.clone(),
            pool: self.pool.clone(),
        }
    }
}

impl EmbeddingStore {
    pub async fn new(pool: SqlitePool, setup: &SetupConfig) -> Result<Self> {
        if let Some(remote) = build_remote_backend(setup)? {
            return Ok(Self {
                backend: Arc::new(remote.backend),
                backend_id: remote.backend_id,
                model_id: remote.model_id,
                description: remote.description,
                pool,
            });
        }

        #[cfg(feature = "local-embeddings")]
        {
            let model = tokio::task::spawn_blocking(|| {
                TextEmbedding::try_new(
                    TextInitOptions::new(EmbeddingModel::BGESmallENV15)
                        .with_show_download_progress(false),
                )
            })
            .await
            .context("embedding model init thread panicked")?
            .context("loading fastembed BGESmallENV15 model")?;

            return Ok(Self {
                backend: Arc::new(EmbeddingBackend::Fastembed {
                    model: Arc::new(Mutex::new(model)),
                }),
                backend_id: "fastembed".to_string(),
                model_id: "BGESmallENV15".to_string(),
                description: "fastembed BGESmallENV15".to_string(),
                pool,
            });
        }

        #[cfg(not(feature = "local-embeddings"))]
        {
            bail!(
                "local embeddings are disabled in this build; configure an OpenAI-compatible embedding provider or rebuild with feature `local-embeddings`"
            );
        }
    }

    pub fn backend_label(&self) -> &str {
        &self.description
    }

    pub async fn ensure_embedded(&self, entries: &[MemoryEntry], memory_root: &str) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT entry_id FROM memory_embeddings WHERE memory_root = ? AND backend_id = ? AND model_id = ?",
        )
        .bind(memory_root)
        .bind(&self.backend_id)
        .bind(&self.model_id)
        .fetch_all(&self.pool)
        .await?;

        let existing_set: std::collections::HashSet<String> = existing.into_iter().collect();
        let new_entries: Vec<&MemoryEntry> = entries
            .iter()
            .filter(|entry| !existing_set.contains(&entry.id))
            .collect();

        if new_entries.is_empty() {
            return Ok(());
        }

        let texts: Vec<String> = new_entries
            .iter()
            .map(|entry| {
                let tags = entry.tags.join(", ");
                let body: String = entry.content.chars().take(512).collect();
                format!("{}\n{}\n{}", entry.summary, tags, body)
            })
            .collect();

        let embeddings = self.embed_texts(texts).await?;
        let now = Utc::now().to_rfc3339();
        for (entry, embedding) in new_entries.iter().zip(embeddings.iter()) {
            let blob = vec_to_bytes(embedding);
            sqlx::query(
                "INSERT OR REPLACE INTO memory_embeddings \
                 (entry_id, memory_root, backend_id, model_id, embedding, embedded_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&entry.id)
            .bind(memory_root)
            .bind(&self.backend_id)
            .bind(&self.model_id)
            .bind(&blob)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn query_semantic(
        &self,
        query: &str,
        memory_root: &str,
        top_k: usize,
    ) -> Result<Vec<(f32, String)>> {
        let query_embeddings = self.embed_texts(vec![query.to_string()]).await?;
        let q_vec = &query_embeddings[0];

        let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT entry_id, embedding FROM memory_embeddings WHERE memory_root = ? AND backend_id = ? AND model_id = ?",
        )
        .bind(memory_root)
        .bind(&self.backend_id)
        .bind(&self.model_id)
        .fetch_all(&self.pool)
        .await?;

        let mut scored: Vec<(f32, String)> = rows
            .into_iter()
            .map(|(id, blob)| {
                let entry_vec = bytes_to_vec(&blob);
                let score = cosine_similarity(q_vec, &entry_vec);
                (score, id)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        match self.backend.as_ref() {
            #[cfg(feature = "local-embeddings")]
            EmbeddingBackend::Fastembed { model } => {
                let model = Arc::clone(model);
                tokio::task::spawn_blocking(move || model.lock().unwrap().embed(texts, None))
                    .await
                    .context("embedding thread panicked")?
                    .context("computing embeddings for memory entries")
            }
            EmbeddingBackend::OpenAiCompatible {
                api_key,
                model,
                url,
                http,
            } => request_remote_embeddings(http, api_key, model, url, texts).await,
        }
    }
}

fn build_remote_backend(setup: &SetupConfig) -> Result<Option<RemoteEmbeddingBackend>> {
    let Some(provider_cfg) = setup.resolve_provider("embedding") else {
        return Ok(None);
    };
    if !provider_cfg.enabled {
        return Ok(None);
    }

    let provider_type = provider_cfg.provider_type.trim().to_ascii_lowercase();
    if !matches!(provider_type.as_str(), "openai" | "codex") {
        bail!(
            "embedding provider type `{}` is not supported; use an OpenAI-compatible embeddings endpoint",
            provider_cfg.provider_type
        );
    }

    let api_key = std::env::var(&provider_cfg.api_key_env).with_context(|| {
        format!(
            "embedding provider is configured but {} is not set",
            provider_cfg.api_key_env
        )
    })?;

    let url = openai_embeddings_url(provider_cfg.base_url.as_deref());
    let description = format!(
        "OpenAI-compatible embeddings via {} ({})",
        url, provider_cfg.model
    );

    Ok(Some(RemoteEmbeddingBackend {
        backend: EmbeddingBackend::OpenAiCompatible {
            api_key,
            model: provider_cfg.model.clone(),
            url: url.clone(),
            http: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .context("building embedding HTTP client")?,
        },
        backend_id: format!("openai-compatible:{}", url),
        model_id: provider_cfg.model.clone(),
        description,
    }))
}

fn openai_embeddings_url(base_url: Option<&str>) -> String {
    let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return "https://api.openai.com/v1/embeddings".to_string();
    };

    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/embeddings") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/embeddings")
    } else {
        format!("{trimmed}/v1/embeddings")
    }
}

async fn request_remote_embeddings(
    http: &Client,
    api_key: &str,
    model: &str,
    url: &str,
    input: Vec<String>,
) -> Result<Vec<Vec<f32>>> {
    let expected = input.len();
    let response = http
        .post(url)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .json(&OpenAiEmbeddingRequest { model, input })
        .send()
        .await
        .with_context(|| format!("embedding request failed against {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("embedding API error {}: {}", status, body);
    }

    let mut parsed: OpenAiEmbeddingResponse = response
        .json()
        .await
        .context("parsing embedding response")?;
    parsed.data.sort_by_key(|item| item.index);
    let embeddings = parsed
        .data
        .into_iter()
        .map(|item| item.embedding)
        .collect::<Vec<_>>();
    if embeddings.len() != expected {
        bail!(
            "embedding API returned {} vectors for {} inputs",
            embeddings.len(),
            expected
        );
    }
    Ok(embeddings)
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingDatum>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingDatum {
    index: usize,
    embedding: Vec<f32>,
}

fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_embeddings_url_defaults_to_official_endpoint() {
        assert_eq!(
            openai_embeddings_url(None),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn openai_embeddings_url_appends_embeddings_path() {
        assert_eq!(
            openai_embeddings_url(Some("http://127.0.0.1:1234")),
            "http://127.0.0.1:1234/v1/embeddings"
        );
        assert_eq!(
            openai_embeddings_url(Some("http://127.0.0.1:1234/v1")),
            "http://127.0.0.1:1234/v1/embeddings"
        );
        assert_eq!(
            openai_embeddings_url(Some("http://127.0.0.1:1234/v1/embeddings")),
            "http://127.0.0.1:1234/v1/embeddings"
        );
    }
}
