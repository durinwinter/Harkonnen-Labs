use anyhow::Result;
use async_trait::async_trait;

use crate::openbrain::OpenBrainClient;

use super::semantic::{SemanticMemory, SemanticMemoryHit, SemanticMemoryWrite};

#[derive(Debug, Clone)]
pub struct OpenBrainSemanticMemory {
    client: OpenBrainClient,
}

impl OpenBrainSemanticMemory {
    pub fn new(client: OpenBrainClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SemanticMemory for OpenBrainSemanticMemory {
    async fn store(&self, write: SemanticMemoryWrite) -> Result<()> {
        self.client
            .capture_thought_with_metadata(&write.content, &write.metadata)
            .await
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SemanticMemoryHit>> {
        Ok(self
            .client
            .search_thoughts_limited(query, limit)
            .await?
            .into_iter()
            .map(|content| SemanticMemoryHit {
                content,
                source: "openbrain".to_string(),
                score: None,
                metadata: Default::default(),
            })
            .collect())
    }
}
