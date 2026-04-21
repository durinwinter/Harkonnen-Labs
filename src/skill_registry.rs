use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::claude_pack::copy_if_exists;

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct SkillTrigger {
    pub(crate) kind: String,
    pub(crate) value: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct SkillToml {
    pub(crate) name: String,
    pub(crate) tier: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) extra_permissions: Vec<String>,
    #[serde(default)]
    pub(crate) triggers: Vec<SkillTrigger>,
}

pub(crate) struct SkillEntry {
    pub(crate) toml: SkillToml,
    pub(crate) source_dir: PathBuf,
}

/// Walk `registry_root/{environment,domain,project}/*/skill.toml` and parse each entry.
pub(crate) fn load_registry(registry_root: &Path) -> Result<Vec<SkillEntry>> {
    let mut entries = Vec::new();
    for tier in &["environment", "domain", "project"] {
        let tier_dir = registry_root.join(tier);
        if !tier_dir.exists() {
            continue;
        }
        let read_dir = fs::read_dir(&tier_dir)
            .with_context(|| format!("reading registry tier dir {}", tier_dir.display()))?;
        for entry in read_dir {
            let entry = entry?;
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let toml_path = skill_dir.join("skill.toml");
            if !toml_path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&toml_path)
                .with_context(|| format!("reading {}", toml_path.display()))?;
            let toml: SkillToml = toml::from_str(&raw)
                .with_context(|| format!("parsing {}", toml_path.display()))?;
            entries.push(SkillEntry {
                toml,
                source_dir: skill_dir,
            });
        }
    }
    Ok(entries)
}

/// Return entries where at least one trigger fires for the given platform and stack signals.
pub(crate) fn auto_select_skills<'a>(
    entries: &'a [SkillEntry],
    platform: &str,
    stack_signals: &[String],
) -> Vec<&'a SkillEntry> {
    entries
        .iter()
        .filter(|e| any_trigger_fires(&e.toml.triggers, platform, stack_signals))
        .collect()
}

fn any_trigger_fires(triggers: &[SkillTrigger], platform: &str, stack_signals: &[String]) -> bool {
    for trigger in triggers {
        let fires = match trigger.kind.as_str() {
            "platform" => platform.eq_ignore_ascii_case(&trigger.value),
            "file" => stack_signals
                .iter()
                .any(|s| s.to_ascii_lowercase().contains(&trigger.value.to_ascii_lowercase())),
            "env_var" => std::env::var(&trigger.value).is_ok(),
            _ => false,
        };
        if fires {
            return true;
        }
    }
    false
}

/// Copy `SKILL.md` from source_dir → `repo/.claude/skills/<name>/SKILL.md`.
pub(crate) fn deploy_skill(entry: &SkillEntry, repo_path: &Path) -> Result<()> {
    let from = entry.source_dir.join("SKILL.md");
    let to = repo_path
        .join(".claude")
        .join("skills")
        .join(&entry.toml.name)
        .join("SKILL.md");
    copy_if_exists(&from, &to)
}
