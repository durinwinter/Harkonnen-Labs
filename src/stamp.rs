use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

use crate::claude_pack::{copy_if_exists, write_text_file};

const STAMP_VERSION: u32 = 1;
const STAMP_MAJOR: u32 = 1;

const SKILLS: &[&str] = &["coobie", "scout", "keeper", "sable", "harkonnen"];

#[derive(Debug)]
struct RepoToml {
    stamp_version: u32,
    harkonnen_root: PathBuf,
    repo_name: String,
    managed_since: String,
}

fn read_repo_toml(repo_path: &Path) -> Result<Option<RepoToml>> {
    let toml_path = repo_path.join(".harkonnen").join("repo.toml");
    if !toml_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;

    let mut stamp_version: u32 = 0;
    let mut harkonnen_root = String::new();
    let mut repo_name = String::new();
    let mut managed_since = String::new();

    for line in raw.lines() {
        if let Some(v) = line.strip_prefix("stamp_version = ") {
            stamp_version = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("harkonnen_root = ") {
            harkonnen_root = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("repo_name = ") {
            repo_name = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("managed_since = ") {
            managed_since = v.trim().trim_matches('"').to_string();
        }
    }

    Ok(Some(RepoToml {
        stamp_version,
        harkonnen_root: PathBuf::from(harkonnen_root),
        repo_name,
        managed_since,
    }))
}

fn write_repo_toml(
    repo_path: &Path,
    harkonnen_root: &Path,
    repo_name: &str,
    managed_since: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let content = format!(
        "stamp_version = {STAMP_VERSION}\nharkonnen_root = \"{}\"\nrepo_name = \"{repo_name}\"\nmanaged_since = \"{managed_since}\"\nupdated_at = \"{now}\"\n",
        harkonnen_root.display()
    );
    let toml_path = repo_path.join(".harkonnen").join("repo.toml");
    write_text_file(&toml_path, &content)
}

fn copy_skills(repo_path: &Path, harkonnen_root: &Path) -> Result<()> {
    for skill in SKILLS {
        let from = harkonnen_root
            .join(".claude")
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        let to = repo_path
            .join(".claude")
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        copy_if_exists(&from, &to)?;
    }
    Ok(())
}

fn render_template(template: &str, repo_name: &str, harkonnen_root: &Path) -> String {
    template
        .replace("{{repo_name}}", repo_name)
        .replace("{{harkonnen_root}}", &harkonnen_root.display().to_string())
}

pub async fn stamp_init(
    repo_path: &Path,
    harkonnen_root: &Path,
    force: bool,
    overwrite_claude_md: bool,
) -> Result<()> {
    let existing = read_repo_toml(repo_path)?;

    if let Some(ref toml) = existing {
        if toml.stamp_version == STAMP_VERSION && !force {
            bail!(
                "already at latest stamp (v{STAMP_VERSION}) — use `--force` to reinitialize or `stamp update` to refresh skills"
            );
        }
        if toml.stamp_version < STAMP_VERSION {
            let old = toml.stamp_version;
            println!("upgrading stamp v{old} → v{STAMP_VERSION}");
            if STAMP_VERSION >= STAMP_MAJOR && old < STAMP_MAJOR {
                println!(
                    "warning: this is a major stamp version bump — CLAUDE.md may need manual \
                     review. Pass --overwrite-claude-md to replace it automatically."
                );
            }
        }
    }

    let repo_name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("managed-repo")
        .to_string();

    copy_skills(repo_path, harkonnen_root)?;

    let template_root = harkonnen_root
        .join("factory")
        .join("templates")
        .join("managed-repo")
        .join(".claude");

    let settings_src = template_root.join("settings.json");
    let settings_dst = repo_path.join(".claude").join("settings.json");
    if !settings_dst.exists() {
        copy_if_exists(&settings_src, &settings_dst)?;
    }

    let claude_md_src = template_root.join("CLAUDE.md");
    let claude_md_dst = repo_path.join(".claude").join("CLAUDE.md");
    if !claude_md_dst.exists() || overwrite_claude_md {
        if claude_md_src.exists() {
            let raw = fs::read_to_string(&claude_md_src)
                .with_context(|| format!("reading {}", claude_md_src.display()))?;
            let rendered = render_template(&raw, &repo_name, harkonnen_root);
            write_text_file(&claude_md_dst, &rendered)?;
        }
    }

    let managed_since = existing
        .as_ref()
        .map(|t| t.managed_since.clone())
        .unwrap_or_else(|| Utc::now().to_rfc3339());

    write_repo_toml(repo_path, harkonnen_root, &repo_name, &managed_since)?;

    println!("stamped: {}", repo_path.display());
    println!("  skills:       {}", SKILLS.join(", "));
    println!("  stamp_version: {STAMP_VERSION}");
    println!("  harkonnen_root: {}", harkonnen_root.display());

    Ok(())
}

pub async fn stamp_update(
    repo_path: &Path,
    harkonnen_root: &Path,
    overwrite_claude_md: bool,
) -> Result<()> {
    let existing = read_repo_toml(repo_path)?
        .ok_or_else(|| anyhow::anyhow!("not a stamped repo — run `stamp init` first"))?;

    let old_version = existing.stamp_version;
    if STAMP_VERSION > old_version && STAMP_VERSION >= STAMP_MAJOR && old_version < STAMP_MAJOR {
        println!(
            "warning: major stamp version bump (v{old_version} → v{STAMP_VERSION}) — \
             CLAUDE.md may need manual review. Pass --overwrite-claude-md to replace it."
        );
    }

    copy_skills(repo_path, harkonnen_root)?;

    if overwrite_claude_md {
        let claude_md_src = harkonnen_root
            .join("factory")
            .join("templates")
            .join("managed-repo")
            .join(".claude")
            .join("CLAUDE.md");
        let claude_md_dst = repo_path.join(".claude").join("CLAUDE.md");
        if claude_md_src.exists() {
            let raw = fs::read_to_string(&claude_md_src)
                .with_context(|| format!("reading {}", claude_md_src.display()))?;
            let rendered = render_template(&raw, &existing.repo_name, harkonnen_root);
            write_text_file(&claude_md_dst, &rendered)?;
        }
    }

    write_repo_toml(
        repo_path,
        harkonnen_root,
        &existing.repo_name,
        &existing.managed_since,
    )?;

    println!("updated: {}", repo_path.display());
    println!("  skills refreshed: {}", SKILLS.join(", "));
    if old_version < STAMP_VERSION {
        println!("  stamp_version: {old_version} → {STAMP_VERSION}");
    }

    Ok(())
}

pub async fn stamp_status(repo_path: &Path) -> Result<()> {
    let toml = read_repo_toml(repo_path)?
        .ok_or_else(|| anyhow::anyhow!("not a stamped repo — run `stamp init` first"))?;

    println!("repo:           {}", repo_path.display());
    println!("repo_name:      {}", toml.repo_name);
    println!("stamp_version:  {}", toml.stamp_version);
    println!("harkonnen_root: {}", toml.harkonnen_root.display());
    println!("managed_since:  {}", toml.managed_since);
    println!();
    println!("skills:");
    for skill in SKILLS {
        let skill_path = repo_path
            .join(".claude")
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        let status = if skill_path.exists() {
            "present"
        } else {
            "MISSING"
        };
        println!("  {skill:<12} {status}");
    }

    Ok(())
}
