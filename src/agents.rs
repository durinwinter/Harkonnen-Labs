use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::{models::AgentExecution, pidgin, setup::SetupConfig};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentProfile {
    pub name: String,
    pub display_name: String,
    pub role: String,
    pub provider: String,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    pub personality_file: String,
    /// Per-task dispatch preferences from the profile's `dispatch:` block.
    /// Takes priority over `[sub_agents.*]` in harkonnen.toml.
    #[serde(default)]
    pub dispatch: HashMap<String, crate::setup::SubAgentTaskConfig>,
}

pub fn load_profiles(dir: &Path) -> Result<HashMap<String, AgentProfile>> {
    let mut profiles = HashMap::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading agent profile directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading agent profile {}", path.display()))?;
        let profile: AgentProfile = serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing agent profile {}", path.display()))?;
        profiles.insert(profile.name.clone(), profile);
    }
    Ok(profiles)
}

pub fn build_execution(
    profile: &AgentProfile,
    setup: &SetupConfig,
    prompt: &str,
    summary: &str,
    output: &str,
) -> AgentExecution {
    let provider_name = setup.resolve_agent_provider_name(&profile.name, &profile.provider);
    let resolved = setup.resolve_agent_provider(&profile.name, &profile.provider);
    let model = profile
        .model_override
        .clone()
        .or_else(|| resolved.map(|provider| provider.model.clone()))
        .unwrap_or_else(|| "unresolved".to_string());
    let usage_rights = resolved.and_then(|provider| provider.usage_rights.clone());
    let surface = resolved.and_then(|provider| provider.surface.clone());
    let mode = match resolved {
        Some(provider) if provider.enabled && std::env::var(&provider.api_key_env).is_ok() => {
            "configured".to_string()
        }
        Some(provider) if provider.enabled => "simulated".to_string(),
        Some(_) => "disabled".to_string(),
        None => "unresolved".to_string(),
    };

    let pidgin_summary = pidgin::pidgin_for_agent_result(&profile.name, summary, output);
    let summary = pidgin::prepend_pidgin(&pidgin_summary, summary);

    AgentExecution {
        agent_name: profile.name.clone(),
        display_name: profile.display_name.clone(),
        role: profile.role.clone(),
        provider: provider_name,
        model,
        surface,
        usage_rights,
        mode,
        prompt: prompt.to_string(),
        pidgin_summary,
        summary,
        output: output.to_string(),
        allowed_tools: profile.allowed_tools.clone(),
        phase: None,
        episode_id: None,
        prompt_bundle_fingerprint: None,
        prompt_bundle_artifact: None,
        prompt_bundle_provider: None,
        pinned_skill_ids: Vec::new(),
        created_at: Utc::now(),
    }
}
