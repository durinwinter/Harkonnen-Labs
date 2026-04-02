use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const CAPACITY_FILE: &str = "factory/state/capacity.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapacity {
    /// Whether this provider can currently accept work.
    pub available: bool,
    /// Human-readable status: "ok", "near_limit", or "at_limit".
    pub status: String,
    /// Routing preference — lower number is tried first.
    pub priority: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub updated_at: String,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityState {
    pub providers: HashMap<String, ProviderCapacity>,
    /// Ordered fallback chain used when a preferred provider is unavailable.
    #[serde(default)]
    pub fallback_chain: Vec<String>,
    pub updated_at: String,
}

impl CapacityState {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CAPACITY_FILE);
        if !path.exists() {
            return Ok(Self::default_state());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading capacity state: {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing capacity state: {}", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = root.join(CAPACITY_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
            .with_context(|| format!("writing capacity state: {}", path.display()))
    }

    /// Set a provider's status. Returns true if availability changed.
    pub fn set(&mut self, provider: &str, status: &str, note: Option<String>, by: &str) -> bool {
        let available = status != "at_limit";
        let now = Utc::now().to_rfc3339();
        let changed = self
            .providers
            .get(provider)
            .map(|p| p.available != available)
            .unwrap_or(true);

        let entry = self
            .providers
            .entry(provider.to_string())
            .or_insert_with(|| ProviderCapacity {
                available: true,
                status: "ok".to_string(),
                priority: 99,
                note: None,
                updated_at: now.clone(),
                updated_by: by.to_string(),
            });
        entry.available = available;
        entry.status = status.to_string();
        entry.note = note;
        entry.updated_at = now.clone();
        entry.updated_by = by.to_string();
        self.updated_at = now;
        changed
    }

    /// Returns the best available provider not in `exclude`, following the fallback chain.
    pub fn best_available(&self, exclude: &[&str]) -> Option<String> {
        // Collect available providers sorted by priority.
        let mut candidates: Vec<(&String, &ProviderCapacity)> = self
            .providers
            .iter()
            .filter(|(name, cap)| cap.available && !exclude.contains(&name.as_str()))
            .collect();
        candidates.sort_by_key(|(_, cap)| cap.priority);
        candidates.first().map(|(name, _)| (*name).clone())
    }

    /// If `preferred` is unavailable, return the best alternative. Otherwise None.
    pub fn fallback_for(&self, preferred: &str) -> Option<String> {
        let cap = self.providers.get(preferred)?;
        if cap.available {
            return None; // preferred is fine, no fallback needed
        }
        self.best_available(&[preferred])
    }

    pub fn is_available(&self, provider: &str) -> bool {
        self.providers
            .get(provider)
            .map(|cap| cap.available)
            .unwrap_or(true) // unknown providers are assumed available
    }

    fn default_state() -> Self {
        let now = Utc::now().to_rfc3339();
        let mut providers = HashMap::new();
        for (name, priority) in [("claude", 1u32), ("codex", 2), ("gemini", 3)] {
            providers.insert(
                name.to_string(),
                ProviderCapacity {
                    available: true,
                    status: "ok".to_string(),
                    priority,
                    note: None,
                    updated_at: now.clone(),
                    updated_by: "default".to_string(),
                },
            );
        }
        Self {
            providers,
            fallback_chain: vec![
                "claude".to_string(),
                "codex".to_string(),
                "gemini".to_string(),
            ],
            updated_at: now,
        }
    }
}
