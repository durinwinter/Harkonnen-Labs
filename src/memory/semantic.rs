use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticMemoryMetadata {
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub source_event_id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub memory_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sensitivity_label: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemoryWrite {
    pub content: String,
    #[serde(default)]
    pub metadata: SemanticMemoryMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemoryHit {
    pub content: String,
    pub source: String,
    #[serde(default)]
    pub score: Option<f32>,
    #[serde(default)]
    pub metadata: SemanticMemoryMetadata,
}

#[async_trait]
pub trait SemanticMemory: Debug + Send + Sync {
    async fn store(&self, write: SemanticMemoryWrite) -> Result<()>;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SemanticMemoryHit>>;
}

#[derive(Debug, Clone, Default)]
pub struct NoopSemanticMemory;

#[async_trait]
impl SemanticMemory for NoopSemanticMemory {
    async fn store(&self, _write: SemanticMemoryWrite) -> Result<()> {
        Ok(())
    }

    async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<SemanticMemoryHit>> {
        Ok(Vec::new())
    }
}
