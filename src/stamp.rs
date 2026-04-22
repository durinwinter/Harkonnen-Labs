use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::claude_pack::{copy_if_exists, scan_target_project, write_text_file};
use crate::cli::{prompt_bool, prompt_choice, prompt_text};
use crate::mcp_registry::{
    auto_select_mcps, load_mcp_registry, render_mcp_args, write_mcps_to_settings,
    ConfiguredMcpServer, McpEntry,
};
use crate::skill_fetcher::{fetch_skills_from_repo, list_skills_in_repo};
use crate::skill_registry::{auto_select_skills, deploy_skill, load_registry, SkillEntry};

const STAMP_VERSION: u32 = 5;
const STAMP_MAJOR: u32 = 1;
const PUBLIC_SKILLS_REPO: &str = "https://github.com/anthropics/skills";
const PDF_STANDARDS_SKILL: &str = "pdf-standards";

const SKILLS: &[&str] = &["coobie", "scout", "keeper", "sable", "harkonnen"];

#[derive(Debug, Default, Deserialize, Serialize)]
struct RepoTomlRaw {
    stamp_version: u32,
    harkonnen_root: String,
    repo_name: String,
    managed_since: String,
    updated_at: Option<String>,
    repo_purpose: Option<String>,
    operator_intent: Option<String>,
    environment: Option<String>,
    domains: Option<Vec<String>>,
    vertical: Option<String>,
    attitudes: Option<Vec<String>>,
    features: Option<Vec<String>>,
    constraints: Option<Vec<String>>,
    skill_sources: Option<Vec<String>>,
    mcp_servers: Option<Vec<String>>,
    interview_completed: Option<bool>,
}

#[derive(Debug, Clone)]
struct RepoToml {
    stamp_version: u32,
    harkonnen_root: PathBuf,
    repo_name: String,
    managed_since: String,
    repo_purpose: Option<String>,
    operator_intent: Option<String>,
    environment: Option<String>,
    domains: Vec<String>,
    vertical: Option<String>,
    attitudes: Vec<String>,
    features: Vec<String>,
    constraints: Vec<String>,
    skill_sources: Vec<String>,
    mcp_servers: Vec<String>,
    interview_completed: bool,
}

impl From<RepoTomlRaw> for RepoToml {
    fn from(raw: RepoTomlRaw) -> Self {
        RepoToml {
            stamp_version: raw.stamp_version,
            harkonnen_root: PathBuf::from(raw.harkonnen_root),
            repo_name: raw.repo_name,
            managed_since: raw.managed_since,
            repo_purpose: raw.repo_purpose,
            operator_intent: raw.operator_intent,
            environment: raw.environment,
            domains: raw.domains.unwrap_or_default(),
            vertical: raw.vertical,
            attitudes: raw.attitudes.unwrap_or_default(),
            features: raw.features.unwrap_or_default(),
            constraints: raw.constraints.unwrap_or_default(),
            skill_sources: raw.skill_sources.unwrap_or_default(),
            mcp_servers: raw.mcp_servers.unwrap_or_default(),
            interview_completed: raw.interview_completed.unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone)]
struct ExternalSkillSelection {
    repo_url: String,
    selected_skill_names: Vec<String>,
}

struct InterviewAnswers {
    repo_purpose: String,
    operator_intent: Option<String>,
    stakeholder_attitudes: Vec<String>,
    prohibitions: Vec<String>,
    environment: Option<String>,
    vertical: Option<String>,
    confirmed_domains: Vec<String>,
    confirmed_projects: Vec<String>,
    external_skill_sources: Vec<String>,
    confirmed_mcps: Vec<ConfiguredMcpServer>,
}

fn read_repo_toml(repo_path: &Path) -> Result<Option<RepoToml>> {
    let toml_path = repo_path.join(".harkonnen").join("repo.toml");
    if !toml_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;
    let parsed: RepoTomlRaw =
        toml::from_str(&raw).with_context(|| format!("parsing {}", toml_path.display()))?;
    Ok(Some(parsed.into()))
}

fn write_repo_toml(repo_path: &Path, toml: &RepoToml) -> Result<()> {
    let persisted = RepoTomlRaw {
        stamp_version: STAMP_VERSION,
        harkonnen_root: toml.harkonnen_root.display().to_string(),
        repo_name: toml.repo_name.clone(),
        managed_since: toml.managed_since.clone(),
        updated_at: Some(Utc::now().to_rfc3339()),
        repo_purpose: toml.repo_purpose.clone(),
        operator_intent: toml.operator_intent.clone(),
        environment: toml.environment.clone(),
        domains: non_empty_vec(&toml.domains),
        vertical: toml.vertical.clone(),
        attitudes: non_empty_vec(&toml.attitudes),
        features: non_empty_vec(&toml.features),
        constraints: non_empty_vec(&toml.constraints),
        skill_sources: non_empty_vec(&toml.skill_sources),
        mcp_servers: non_empty_vec(&toml.mcp_servers),
        interview_completed: if toml.interview_completed {
            Some(true)
        } else {
            None
        },
    };
    let content = toml::to_string_pretty(&persisted)?;
    let toml_path = repo_path.join(".harkonnen").join("repo.toml");
    write_text_file(&toml_path, &content)
}

fn non_empty_vec(values: &[String]) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values.to_vec())
    }
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

fn render_with_vars(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

fn build_domain_claude_sections(domains: &[String]) -> String {
    let mut out = String::new();
    for domain in domains {
        match domain.as_str() {
            "azure-databricks" => out.push_str(
                "\n## Azure Databricks\n\
                 - Treat production clusters as read-first.\n\
                 - Never write to production Delta tables without explicit approval.\n\
                 - Store secrets in Databricks secret scopes — not in notebook cells.\n",
            ),
            "sql" => out.push_str(
                "\n## SQL\n\
                 - Every schema change is a migration file — never mutate the schema directly.\n\
                 - Parameterize all user-supplied values. No string concatenation in queries.\n\
                 - Wrap large mutations in explicit transactions with rollback on error.\n",
            ),
            "docker" => out.push_str(
                "\n## Docker\n\
                 - Pin base image tags — never `latest` in production.\n\
                 - Containers are ephemeral — persist state via volumes or object storage.\n\
                 - Do not expose ports without a firewall rule in production.\n",
            ),
            "azure" => out.push_str(
                "\n## Azure\n\
                 - Confirm the active subscription before making changes: `az account show`.\n\
                 - Treat production resource groups as read-first.\n\
                 - Tag all new resources with env, project, and owner tags.\n",
            ),
            "winccoa" => out.push_str(
                "\n## WinCC OA\n\
                 - Treat CTRL scripts, panels, datapoints, and managers as operationally sensitive.\n\
                 - Any action affecting a live plant or operator workflow requires explicit human approval.\n\
                 - Prefer offline topology sketches, mocked datapoints, and staged exports over runtime mutation.\n\
                 - Escalate live SCADA / OT changes to Keeper before proceeding.\n",
            ),
            "pdf-standards" => out.push_str(
                "\n## Standards PDFs\n\
                 - Prefer exact section and page citations when summarizing standards or regulations.\n\
                 - Separate quoted requirements from your interpretation of those requirements.\n\
                 - Treat compliance guidance as traceable evidence, not loose paraphrase.\n",
            ),
            _ => {}
        }
    }
    out
}

fn merge_skill_permissions_into_settings(settings_path: &Path, perms: &[String]) -> Result<()> {
    if perms.is_empty() {
        return Ok(());
    }

    let existing: serde_json::Value = if settings_path.exists() {
        let raw = fs::read_to_string(settings_path)
            .with_context(|| format!("reading {}", settings_path.display()))?;
        serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let mut obj = match existing {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };

    let permissions = obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    let allow = permissions
        .get_mut("allow")
        .and_then(|value| value.as_array_mut());

    if let Some(allow_arr) = allow {
        for perm in perms {
            let value = serde_json::Value::String(perm.clone());
            if !allow_arr.contains(&value) {
                allow_arr.push(value);
            }
        }
    } else {
        let allow_arr: Vec<serde_json::Value> = perms
            .iter()
            .map(|permission| serde_json::Value::String(permission.clone()))
            .collect();
        if let Some(permissions_obj) = permissions.as_object_mut() {
            permissions_obj.insert("allow".to_string(), serde_json::Value::Array(allow_arr));
        }
    }

    let merged = serde_json::Value::Object(obj);
    let pretty = serde_json::to_string_pretty(&merged)?;
    write_text_file(settings_path, &pretty)
}

fn build_archive_seed(
    answers: &InterviewAnswers,
    scan: &crate::claude_pack::ProjectScan,
    repo_name: &str,
    deployed_skills: &[String],
) -> String {
    let generated_at = Utc::now().to_rfc3339();

    let prohibitions = if answers.prohibitions.is_empty() {
        "No explicit prohibitions recorded.".to_string()
    } else {
        answers
            .prohibitions
            .iter()
            .map(|prohibition| format!("- {prohibition}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let operator_intent = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.")
        .to_string();
    let attitudes = if answers.stakeholder_attitudes.is_empty() {
        "No stakeholder attitudes recorded.".to_string()
    } else {
        answers
            .stakeholder_attitudes
            .iter()
            .map(|attitude| format!("- {attitude}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let environment = answers
        .environment
        .as_deref()
        .unwrap_or("not specified")
        .to_string();

    let vertical = answers
        .vertical
        .as_deref()
        .unwrap_or("not specified")
        .to_string();

    let all_domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domains_str = list_or_default(&all_domains, "none");
    let stack_str = list_or_default(&scan.stack_signals, "none detected");
    let skill_sources = list_or_default(&answers.external_skill_sources, "none");
    let mcp_servers = if answers.confirmed_mcps.is_empty() {
        "none".to_string()
    } else {
        answers
            .confirmed_mcps
            .iter()
            .map(|server| server.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let logos = if scan.detected_roots.is_empty() {
        "No structural roots detected.".to_string()
    } else {
        scan.detected_roots
            .iter()
            .map(|root| format!("- `{root}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let validation = list_or_default(&scan.validation_commands, "none detected");
    let skills_str = list_or_default(deployed_skills, "none");

    render_with_vars(
        include_str!("../factory/templates/archive-seed.md"),
        &[
            ("repo_name", repo_name),
            ("generated_at", &generated_at),
            ("mythos", &answers.repo_purpose),
            ("environment", &environment),
            ("domains", &domains_str),
            ("vertical", &vertical),
            ("stack_signals", &stack_str),
            ("skill_sources", &skill_sources),
            ("episteme", ""),
            ("prohibitions", &prohibitions),
            ("operator_intent", &operator_intent),
            ("attitudes", &attitudes),
            ("logos_structure", &logos),
            ("deployed_skills", &skills_str),
            ("mcp_servers", &mcp_servers),
            ("validation_commands", &validation),
        ],
    )
}

fn build_intent_doc(
    answers: &InterviewAnswers,
    repo_name: &str,
    deployed_skills: &[String],
) -> String {
    let generated_at = Utc::now().to_rfc3339();

    let prohibitions_list = if answers.prohibitions.is_empty() {
        "None specified.".to_string()
    } else {
        answers
            .prohibitions
            .iter()
            .map(|prohibition| format!("- {prohibition}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let operator_intent = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.")
        .to_string();
    let attitudes_list = bullet_list_or_default(
        &answers.stakeholder_attitudes,
        "No stakeholder attitudes recorded.",
    );

    let environment = answers
        .environment
        .as_deref()
        .unwrap_or("not specified")
        .to_string();
    let vertical = answers
        .vertical
        .as_deref()
        .unwrap_or("not specified")
        .to_string();

    let all_domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domains_list = bullet_list_or_default(&all_domains, "none");
    let skill_sources_list = bullet_list_or_default(&answers.external_skill_sources, "none");
    let mcp_servers_list = if answers.confirmed_mcps.is_empty() {
        "none".to_string()
    } else {
        answers
            .confirmed_mcps
            .iter()
            .map(|server| server.name.clone())
            .collect::<Vec<_>>()
            .iter()
            .map(|server| format!("- {server}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let skills_list = deployed_skills
        .iter()
        .map(|skill| format!("- {skill}"))
        .collect::<Vec<_>>()
        .join("\n");

    render_with_vars(
        include_str!("../factory/templates/intent.md"),
        &[
            ("repo_name", repo_name),
            ("generated_at", &generated_at),
            ("repo_purpose", &answers.repo_purpose),
            ("operator_intent", &operator_intent),
            ("attitudes_list", &attitudes_list),
            ("prohibitions_list", &prohibitions_list),
            ("environment", &environment),
            ("vertical", &vertical),
            ("domains_list", &domains_list),
            ("skill_sources_list", &skill_sources_list),
            ("skills_list", &skills_list),
            ("mcp_servers_list", &mcp_servers_list),
        ],
    )
}

fn build_interview_context_doc(
    answers: &InterviewAnswers,
    repo_name: &str,
    deployed_skills: &[String],
) -> String {
    let generated_at = Utc::now().to_rfc3339();
    let operator_intent = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.")
        .to_string();
    let attitudes_list = bullet_list_or_default(
        &answers.stakeholder_attitudes,
        "No stakeholder attitudes recorded.",
    );
    let prohibitions_list =
        bullet_list_or_default(&answers.prohibitions, "No explicit prohibitions recorded.");
    let environment = answers
        .environment
        .as_deref()
        .unwrap_or("not specified")
        .to_string();
    let vertical = answers
        .vertical
        .as_deref()
        .unwrap_or("not specified")
        .to_string();
    let all_domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domains_list = bullet_list_or_default(&all_domains, "none");
    let skill_sources_list = bullet_list_or_default(&answers.external_skill_sources, "none");
    let skills_list = bullet_list_or_default(deployed_skills, "none");
    let mcp_names = answers
        .confirmed_mcps
        .iter()
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();
    let mcp_servers_list = bullet_list_or_default(&mcp_names, "none");

    render_with_vars(
        include_str!("../factory/templates/interview-context.md"),
        &[
            ("repo_name", repo_name),
            ("generated_at", &generated_at),
            ("repo_purpose", &answers.repo_purpose),
            ("operator_intent", &operator_intent),
            ("attitudes_list", &attitudes_list),
            ("prohibitions_list", &prohibitions_list),
            ("environment", &environment),
            ("vertical", &vertical),
            ("domains_list", &domains_list),
            ("skill_sources_list", &skill_sources_list),
            ("skills_list", &skills_list),
            ("mcp_servers_list", &mcp_servers_list),
        ],
    )
}

fn build_claude_interview_section(
    answers: &InterviewAnswers,
    interview_context_path: &Path,
) -> String {
    let purpose = answers.repo_purpose.trim();
    let stakes = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.");
    let environment = answers.environment.as_deref().unwrap_or("not specified");
    let vertical = answers.vertical.as_deref().unwrap_or("not specified");
    let domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domain_line = list_or_default(&domains, "none");
    let skill_sources = list_or_default(&answers.external_skill_sources, "none");
    let mcp_names = answers
        .confirmed_mcps
        .iter()
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();
    let mcp_line = list_or_default(&mcp_names, "none");

    let mut out = String::new();
    out.push_str("\n---\n\n## Project Interview Context\n\n");
    out.push_str(&format!(
        "Full interview context: `{}`\n\n",
        interview_context_path.display()
    ));
    out.push_str(&format!("- Purpose: {purpose}\n"));
    out.push_str(&format!("- Stakes: {stakes}\n"));
    out.push_str(&format!("- Environment: {environment}\n"));
    out.push_str(&format!("- Vertical: {vertical}\n"));
    out.push_str(&format!("- Domains: {domain_line}\n"));
    out.push_str(&format!("- External skill sources: {skill_sources}\n"));
    out.push_str(&format!("- MCP servers: {mcp_line}\n"));
    if answers.prohibitions.is_empty() {
        out.push_str("- Prohibitions: none recorded\n");
    } else {
        out.push_str("- Prohibitions:\n");
        for prohibition in &answers.prohibitions {
            out.push_str(&format!("  - {prohibition}\n"));
        }
    }
    if answers.stakeholder_attitudes.is_empty() {
        out.push_str("- Stakeholder attitudes: none recorded\n");
    } else {
        out.push_str("- Stakeholder attitudes:\n");
        for attitude in &answers.stakeholder_attitudes {
            out.push_str(&format!("  - {attitude}\n"));
        }
    }
    out
}

fn bullet_list_or_default(values: &[String], default: &str) -> String {
    if values.is_empty() {
        default.to_string()
    } else {
        values
            .iter()
            .map(|value| format!("- {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn list_or_default(values: &[String], default: &str) -> String {
    if values.is_empty() {
        default.to_string()
    } else {
        values.join(", ")
    }
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn find_skill_entry<'a>(registry: &'a [SkillEntry], name: &str) -> Option<&'a SkillEntry> {
    registry.iter().find(|entry| entry.toml.name == name)
}

fn extend_unique(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    let mut seen = target.iter().cloned().collect::<BTreeSet<_>>();
    for value in values {
        if seen.insert(value.clone()) {
            target.push(value);
        }
    }
}

fn detect_standards_keywords(vertical: &str) -> bool {
    let lowered = vertical.to_ascii_lowercase();
    [
        "pharma",
        "gmp",
        "fda",
        "cfr",
        "drug",
        "clinical",
        "iso",
        "iec",
        "standards",
        "regulatory",
        "compliance",
        "hipaa",
        "sox",
        "pci",
        "aml",
        "audit",
        "validated",
        "validation",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

fn default_mcp_config_value(key: &str) -> String {
    match key {
        "paths" => ".".to_string(),
        "db_path" => "factory/state.db".to_string(),
        _ if key.ends_with("_path") => ".".to_string(),
        _ => String::new(),
    }
}

fn collect_mcp_config(entry: &McpEntry) -> Result<HashMap<String, String>> {
    let mut config = HashMap::new();
    for key in &entry.toml.requires_config {
        let default = default_mcp_config_value(key);
        loop {
            let prompt = format!("Value for MCP config '{key}'");
            let value = prompt_text(&prompt, &default)?;
            if value.trim().is_empty() {
                println!("A value is required for '{key}'.");
                continue;
            }
            config.insert(key.clone(), value);
            break;
        }
    }
    Ok(config)
}

fn configured_mcp_from_entry(
    entry: &McpEntry,
    config: HashMap<String, String>,
) -> ConfiguredMcpServer {
    ConfiguredMcpServer {
        name: entry.toml.name.clone(),
        command: entry.toml.command.clone(),
        args: render_mcp_args(&entry.toml.args, &config),
        extra_permissions: entry.toml.extra_permissions.clone(),
        requires_env: entry.toml.requires_env.clone(),
        settings_target: entry.toml.settings_target.clone(),
    }
}

fn collect_custom_mcp_server() -> Result<ConfiguredMcpServer> {
    println!("  Custom MCP server:");
    let name = loop {
        let value = prompt_text("Name", "")?;
        if value.is_empty() {
            println!("A custom server name is required.");
        } else {
            break value;
        }
    };
    let command = loop {
        let value = prompt_text("Command", "")?;
        if value.is_empty() {
            println!("A command is required.");
        } else {
            break value;
        }
    };
    let args_raw = prompt_text("Args (comma-separated, blank for none)", "")?;
    let extra_permissions_raw =
        prompt_text("Extra permissions (comma-separated, blank for none)", "")?;
    let requires_env_raw = prompt_text("Required env vars (comma-separated, blank for none)", "")?;
    let requires_env = split_csv(&requires_env_raw);
    let default_target = if requires_env.is_empty() {
        "settings.json"
    } else {
        "settings.local.json"
    };
    let settings_target = prompt_choice(
        "Write server to",
        &["settings.json", "settings.local.json"],
        default_target,
    )?;

    Ok(ConfiguredMcpServer {
        name,
        command,
        args: split_csv(&args_raw),
        extra_permissions: split_csv(&extra_permissions_raw),
        requires_env,
        settings_target,
    })
}

async fn prompt_external_skill_selection(
    clone_url: &str,
    label: &str,
) -> Result<ExternalSkillSelection> {
    println!("  Inspecting {label}: {clone_url}");
    let mut selection = ExternalSkillSelection {
        repo_url: clone_url.to_string(),
        selected_skill_names: Vec::new(),
    };

    match list_skills_in_repo(clone_url).await {
        Ok(skill_names) => {
            if skill_names.is_empty() {
                println!("  Warning: no SKILL.md files were discovered in {clone_url}.");
                return Ok(selection);
            }

            println!("  Discovered skills:");
            for name in &skill_names {
                println!("    - {name}");
            }

            if prompt_bool("Copy all discovered skills from this source?", false)? {
                selection.selected_skill_names = skill_names;
            } else {
                for name in skill_names {
                    if prompt_bool(&format!("Copy '{name}'?"), false)? {
                        selection.selected_skill_names.push(name);
                    }
                }
            }
        }
        Err(error) => {
            println!("  Warning: could not inspect {clone_url}: {error}");
        }
    }

    selection.selected_skill_names.sort();
    selection.selected_skill_names.dedup();
    Ok(selection)
}

fn interview_skill_tier(
    registry: &[SkillEntry],
    auto_selected: &[&SkillEntry],
    existing_names: &BTreeSet<String>,
    tier: &str,
    title: &str,
    add_prompt: &str,
    excluded_names: &[&str],
) -> Result<Vec<String>> {
    println!("\n{title}:");
    let excluded = excluded_names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();

    let mut selected = Vec::new();
    let mut suggested_names = BTreeSet::new();

    for entry in auto_selected.iter().filter(|entry| entry.toml.tier == tier) {
        if !excluded.contains(&entry.toml.name) {
            suggested_names.insert(entry.toml.name.clone());
        }
    }

    for name in existing_names {
        if excluded.contains(name) {
            continue;
        }
        if registry
            .iter()
            .any(|entry| entry.toml.tier == tier && entry.toml.name == *name)
        {
            suggested_names.insert(name.clone());
        }
    }

    for name in suggested_names {
        if let Some(entry) = registry
            .iter()
            .find(|entry| entry.toml.tier == tier && entry.toml.name == name)
        {
            let default_yes = existing_names.contains(&name)
                || auto_selected
                    .iter()
                    .any(|candidate| candidate.toml.name == name);
            let source = if auto_selected
                .iter()
                .any(|candidate| candidate.toml.name == name)
            {
                "Suggested"
            } else {
                "Previously configured"
            };
            println!(
                "  {source}: {} — {}",
                entry.toml.name, entry.toml.description
            );
            if prompt_bool(
                &format!("Include '{}' skill?", entry.toml.name),
                default_yes,
            )? {
                selected.push(entry.toml.name.clone());
            }
        }
    }

    loop {
        if !prompt_bool(add_prompt, false)? {
            break;
        }
        let available_entries = registry
            .iter()
            .filter(|entry| {
                entry.toml.tier == tier
                    && !excluded.contains(&entry.toml.name)
                    && !selected.contains(&entry.toml.name)
            })
            .collect::<Vec<_>>();
        if available_entries.is_empty() {
            println!("No more {tier} skills available.");
            break;
        }

        println!("Available {tier} skills:");
        for entry in &available_entries {
            println!("  {} — {}", entry.toml.name, entry.toml.description);
        }

        let options = available_entries
            .iter()
            .map(|entry| entry.toml.name.as_str())
            .collect::<Vec<_>>();
        let chosen = prompt_choice("Skill", &options, options[0])?;
        if !selected.contains(&chosen) {
            selected.push(chosen);
        }
    }

    selected.sort();
    selected.dedup();
    Ok(selected)
}

fn collect_confirmed_skill_names(
    environment: &Option<String>,
    confirmed_domains: &[String],
    confirmed_projects: &[String],
    pdf_standards_skill: bool,
    external_sources: &[ExternalSkillSelection],
) -> Vec<String> {
    let mut names = SKILLS
        .iter()
        .map(|skill| (*skill).to_string())
        .collect::<Vec<_>>();
    if let Some(environment) = environment {
        extend_unique(&mut names, [environment.clone()]);
    }
    extend_unique(&mut names, confirmed_domains.iter().cloned());
    extend_unique(&mut names, confirmed_projects.iter().cloned());
    if pdf_standards_skill {
        extend_unique(&mut names, [PDF_STANDARDS_SKILL.to_string()]);
    }
    for source in external_sources {
        extend_unique(&mut names, source.selected_skill_names.iter().cloned());
    }
    names
}

fn print_mcp_env_notes(configured_mcps: &[ConfiguredMcpServer]) {
    for server in configured_mcps {
        if !server.requires_env.is_empty() {
            println!(
                "  Note: '{}' expects env vars: {}. Keep its config in {} and provide those values in your shell or local Claude config.",
                server.name,
                server.requires_env.join(", "),
                server.settings_target
            );
        }
    }
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
                    "warning: this is a major stamp version bump — CLAUDE.md may need manual review. Pass --overwrite-claude-md to replace it automatically."
                );
            }
        }
    }

    let repo_name = repo_path
        .file_name()
        .and_then(|name| name.to_str())
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
    if (!claude_md_dst.exists() || overwrite_claude_md) && claude_md_src.exists() {
        let raw = fs::read_to_string(&claude_md_src)
            .with_context(|| format!("reading {}", claude_md_src.display()))?;
        let rendered = render_template(&raw, &repo_name, harkonnen_root);
        write_text_file(&claude_md_dst, &rendered)?;
    }

    let managed_since = existing
        .as_ref()
        .map(|toml| toml.managed_since.clone())
        .unwrap_or_else(|| Utc::now().to_rfc3339());

    let new_toml = RepoToml {
        stamp_version: STAMP_VERSION,
        harkonnen_root: harkonnen_root.to_path_buf(),
        repo_name: repo_name.clone(),
        managed_since,
        repo_purpose: existing.as_ref().and_then(|toml| toml.repo_purpose.clone()),
        operator_intent: existing
            .as_ref()
            .and_then(|toml| toml.operator_intent.clone()),
        environment: existing.as_ref().and_then(|toml| toml.environment.clone()),
        domains: existing
            .as_ref()
            .map(|toml| toml.domains.clone())
            .unwrap_or_default(),
        vertical: existing.as_ref().and_then(|toml| toml.vertical.clone()),
        attitudes: existing
            .as_ref()
            .map(|toml| toml.attitudes.clone())
            .unwrap_or_default(),
        features: existing
            .as_ref()
            .map(|toml| toml.features.clone())
            .unwrap_or_default(),
        constraints: existing
            .as_ref()
            .map(|toml| toml.constraints.clone())
            .unwrap_or_default(),
        skill_sources: existing
            .as_ref()
            .map(|toml| toml.skill_sources.clone())
            .unwrap_or_default(),
        mcp_servers: existing
            .as_ref()
            .map(|toml| toml.mcp_servers.clone())
            .unwrap_or_default(),
        interview_completed: existing
            .as_ref()
            .map(|toml| toml.interview_completed)
            .unwrap_or(false),
    };

    write_repo_toml(repo_path, &new_toml)?;

    println!("stamped: {}", repo_path.display());
    println!("  skills:        {}", SKILLS.join(", "));
    println!("  stamp_version: {STAMP_VERSION}");
    println!("  harkonnen_root: {}", harkonnen_root.display());

    Ok(())
}

pub async fn stamp_update(
    repo_path: &Path,
    harkonnen_root: &Path,
    overwrite_claude_md: bool,
    re_interview: bool,
) -> Result<()> {
    let existing = read_repo_toml(repo_path)?
        .ok_or_else(|| anyhow::anyhow!("not a stamped repo — run `stamp init` first"))?;

    let old_version = existing.stamp_version;
    if STAMP_VERSION > old_version && STAMP_VERSION >= STAMP_MAJOR && old_version < STAMP_MAJOR {
        println!(
            "warning: major stamp version bump (v{old_version} → v{STAMP_VERSION}) — CLAUDE.md may need manual review. Pass --overwrite-claude-md to replace it."
        );
    }

    if re_interview {
        return stamp_interview(repo_path, harkonnen_root, true).await;
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
            let mut rendered = render_template(&raw, &existing.repo_name, harkonnen_root);
            let domain_sections = build_domain_claude_sections(&existing.domains);
            if !domain_sections.is_empty() {
                rendered.push_str("\n---\n");
                rendered.push_str(&domain_sections);
            }
            write_text_file(&claude_md_dst, &rendered)?;
        }
    }

    let updated_toml = RepoToml {
        stamp_version: STAMP_VERSION,
        harkonnen_root: harkonnen_root.to_path_buf(),
        repo_name: existing.repo_name.clone(),
        managed_since: existing.managed_since.clone(),
        repo_purpose: existing.repo_purpose.clone(),
        operator_intent: existing.operator_intent.clone(),
        environment: existing.environment.clone(),
        domains: existing.domains.clone(),
        vertical: existing.vertical.clone(),
        attitudes: existing.attitudes.clone(),
        features: existing.features.clone(),
        constraints: existing.constraints.clone(),
        skill_sources: existing.skill_sources.clone(),
        mcp_servers: existing.mcp_servers.clone(),
        interview_completed: existing.interview_completed,
    };

    write_repo_toml(repo_path, &updated_toml)?;

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

    println!("repo:                {}", repo_path.display());
    println!("repo_name:           {}", toml.repo_name);
    println!("stamp_version:       {}", toml.stamp_version);
    println!("harkonnen_root:      {}", toml.harkonnen_root.display());
    println!("managed_since:       {}", toml.managed_since);
    if let Some(ref repo_purpose) = toml.repo_purpose {
        println!("repo_purpose:        {repo_purpose}");
    }
    if let Some(ref operator_intent) = toml.operator_intent {
        println!("operator_intent:     {operator_intent}");
    }
    println!(
        "interview_completed: {}",
        if toml.interview_completed {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(ref environment) = toml.environment {
        println!("environment:         {environment}");
    }
    if let Some(ref vertical) = toml.vertical {
        println!("vertical:            {vertical}");
    }
    if !toml.attitudes.is_empty() {
        println!("attitudes:           {}", toml.attitudes.join(" | "));
    }
    if !toml.domains.is_empty() {
        println!("domains:             {}", toml.domains.join(", "));
    }
    if !toml.skill_sources.is_empty() {
        println!("skill_sources:       {}", toml.skill_sources.join(", "));
    }
    if !toml.mcp_servers.is_empty() {
        println!("mcp_servers:         {}", toml.mcp_servers.join(", "));
    }
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

pub async fn stamp_interview(repo_path: &Path, harkonnen_root: &Path, force: bool) -> Result<()> {
    let existing = read_repo_toml(repo_path)?;

    if let Some(ref toml) = existing {
        if toml.interview_completed && !force {
            bail!(
                "interview already completed — use `--force` to redo it, or `stamp update --re-interview`"
            );
        }
    } else {
        bail!("not a stamped repo — run `stamp init` first");
    }

    let existing = existing.expect("checked above");
    let scan = scan_target_project(repo_path)?;
    let platform = std::env::consts::OS;
    let registry_root = harkonnen_root.join("factory").join("skill-registry");
    let registry = load_registry(&registry_root).unwrap_or_default();
    let auto_selected = auto_select_skills(&registry, platform, &scan.stack_signals);
    let mcp_registry_root = harkonnen_root.join("factory").join("mcp-registry");
    let mcp_registry = load_mcp_registry(&mcp_registry_root).unwrap_or_default();
    let existing_names = existing.domains.iter().cloned().collect::<BTreeSet<_>>();

    println!("\n=== Stamp Interview: {} ===\n", existing.repo_name);
    println!("This interview seeds the Calvin Archive for Coobie.");
    println!("Detected platform: {platform}");
    if !scan.stack_signals.is_empty() {
        println!("Detected stack signals:");
        for signal in &scan.stack_signals {
            println!("  - {signal}");
        }
    }
    println!();

    println!("Q1 — Mythos");
    let repo_purpose = prompt_text(
        "What is this repo for? (one or two sentences)",
        existing.repo_purpose.as_deref().unwrap_or(""),
    )?;
    if repo_purpose.is_empty() {
        bail!("Repo purpose is required.");
    }

    println!("\nQ2 — Pathos");
    println!("Who uses it and what breaks if it fails? (blank to skip)");
    let operator_intent_raw =
        prompt_text("Stakes", existing.operator_intent.as_deref().unwrap_or(""))?;
    let operator_intent = if operator_intent_raw.is_empty() {
        None
    } else {
        Some(operator_intent_raw)
    };
    println!("\nStakeholder attitudes and organizational posture:");
    println!("Examples:");
    println!("  - Department X wants to replace software Y with Z");
    println!("  - Finance prefers AWS over Azure");
    println!("  - Operations dislikes cloud-hosted systems altogether");
    println!("Enter one attitude per line. Blank line to finish.");
    let mut stakeholder_attitudes = existing.attitudes.clone();
    if !stakeholder_attitudes.is_empty() {
        println!("Previously recorded attitudes:");
        for attitude in &stakeholder_attitudes {
            println!("  - {attitude}");
        }
        if !prompt_bool("Keep previously recorded attitudes?", true)? {
            stakeholder_attitudes.clear();
        }
    }
    loop {
        let attitude = prompt_text("Attitude (blank to finish)", "")?;
        if attitude.is_empty() {
            break;
        }
        stakeholder_attitudes.push(attitude);
    }
    stakeholder_attitudes.dedup();

    println!("\nQ3 — Ethos");
    println!("What must NEVER happen in this repo?");
    println!("Enter one prohibition per line. Blank line to finish.");
    let mut prohibitions = Vec::new();
    loop {
        let prohibition = prompt_text("Prohibition (blank to finish)", "")?;
        if prohibition.is_empty() {
            break;
        }
        prohibitions.push(prohibition);
    }

    println!("\nQ4 — Environment");
    let env_auto = auto_selected
        .iter()
        .filter(|entry| entry.toml.tier == "environment")
        .copied()
        .collect::<Vec<_>>();
    let detected_env = env_auto.first().map(|entry| entry.toml.name.clone());
    let suggested_env = detected_env.or_else(|| existing.environment.clone());

    let environment = if let Some(suggested_env) = suggested_env {
        println!("  Suggested environment: {suggested_env}");
        if prompt_bool(&format!("Use '{suggested_env}' as environment?"), true)? {
            Some(suggested_env)
        } else {
            let choice = prompt_choice(
                "Select environment",
                &["linux", "windows", "macos", "wsl", "none"],
                "none",
            )?;
            if choice == "none" {
                None
            } else {
                Some(choice)
            }
        }
    } else {
        println!("  No environment auto-detected for platform '{platform}'.");
        let choice = prompt_choice(
            "Select environment",
            &["linux", "windows", "macos", "wsl", "none"],
            "none",
        )?;
        if choice == "none" {
            None
        } else {
            Some(choice)
        }
    };

    let confirmed_domains = interview_skill_tier(
        &registry,
        &auto_selected,
        &existing_names,
        "domain",
        "Q5 — Domain skills",
        "Add another domain skill?",
        &[],
    )?;

    println!("\nQ5b — Vertical / Line of Business");
    println!("Examples: pharmaceutical, finance, manufacturing, healthcare, utilities");
    let vertical_raw = prompt_text(
        "Vertical (blank to skip)",
        existing.vertical.as_deref().unwrap_or(""),
    )?;
    let vertical = if vertical_raw.is_empty() {
        None
    } else {
        Some(vertical_raw)
    };

    let previous_pdf_skill = existing_names.contains(PDF_STANDARDS_SKILL);
    let pdf_standards_skill = if let Some(ref vertical) = vertical {
        if detect_standards_keywords(vertical) {
            println!(
                "  Standards-heavy vertical detected. A PDF/standards reading skill is available."
            );
            prompt_bool("Include 'pdf-standards' skill?", true)?
        } else if previous_pdf_skill {
            prompt_bool("Keep previously configured 'pdf-standards' skill?", true)?
        } else {
            false
        }
    } else if previous_pdf_skill {
        prompt_bool("Keep previously configured 'pdf-standards' skill?", true)?
    } else {
        false
    };

    println!("\nQ5c — External Skill Sources");
    let mut external_skill_sources = Vec::new();
    let existing_sources = existing
        .skill_sources
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if prompt_bool(
        &format!("Pull from public skills collection ({PUBLIC_SKILLS_REPO})?"),
        true,
    )? {
        external_skill_sources.push(
            prompt_external_skill_selection(PUBLIC_SKILLS_REPO, "public skills collection").await?,
        );
    }

    for source in existing
        .skill_sources
        .iter()
        .filter(|source| source.as_str() != PUBLIC_SKILLS_REPO)
    {
        if prompt_bool(&format!("Re-use saved skill source '{source}'?"), true)? {
            external_skill_sources
                .push(prompt_external_skill_selection(source, "saved external repo").await?);
        }
    }

    loop {
        if !prompt_bool("Add a corporate/private skill repo?", false)? {
            break;
        }
        let url = prompt_text("Git clone URL", "")?;
        if url.is_empty() {
            println!("No URL entered.");
            continue;
        }
        if existing_sources.contains(&url)
            || external_skill_sources
                .iter()
                .any(|selection| selection.repo_url == url)
        {
            println!("  Source already queued: {url}");
            continue;
        }
        external_skill_sources.push(prompt_external_skill_selection(&url, "external repo").await?);
    }

    let confirmed_projects = interview_skill_tier(
        &registry,
        &auto_selected,
        &existing_names,
        "project",
        "Q6 — Project skills",
        "Add another project skill?",
        &[PDF_STANDARDS_SKILL],
    )?;

    println!("\nQ6b — MCP Servers");
    let confirmed_skill_names = collect_confirmed_skill_names(
        &environment,
        &confirmed_domains,
        &confirmed_projects,
        pdf_standards_skill,
        &external_skill_sources,
    );
    let auto_mcp_names =
        auto_select_mcps(&mcp_registry, &confirmed_skill_names, &scan.stack_signals)
            .into_iter()
            .map(|entry| entry.toml.name.clone())
            .collect::<BTreeSet<_>>();
    let mut suggested_mcp_names = auto_mcp_names;
    for name in &existing.mcp_servers {
        if mcp_registry.iter().any(|entry| entry.toml.name == *name) {
            suggested_mcp_names.insert(name.clone());
        }
    }

    let mut confirmed_mcps = Vec::new();
    let mut confirmed_mcp_names = BTreeSet::new();

    for name in suggested_mcp_names {
        if let Some(entry) = mcp_registry.iter().find(|entry| entry.toml.name == name) {
            println!(
                "  Suggested: {} — {}",
                entry.toml.name, entry.toml.description
            );
            let default_yes = existing.mcp_servers.contains(&entry.toml.name)
                || auto_select_mcps(&mcp_registry, &confirmed_skill_names, &scan.stack_signals)
                    .iter()
                    .any(|candidate| candidate.toml.name == entry.toml.name);
            if prompt_bool(
                &format!("Include '{}' MCP server?", entry.toml.name),
                default_yes,
            )? {
                let config = collect_mcp_config(entry)?;
                confirmed_mcps.push(configured_mcp_from_entry(entry, config));
                confirmed_mcp_names.insert(entry.toml.name.clone());
            }
        }
    }

    loop {
        if !prompt_bool("Add another MCP server?", false)? {
            break;
        }

        let mut options = mcp_registry
            .iter()
            .filter(|entry| !confirmed_mcp_names.contains(&entry.toml.name))
            .map(|entry| entry.toml.name.clone())
            .collect::<Vec<_>>();
        options.sort();
        options.push("custom".to_string());
        let option_refs = options.iter().map(String::as_str).collect::<Vec<_>>();
        let chosen = prompt_choice("Server", &option_refs, option_refs[0])?;
        if chosen == "custom" {
            confirmed_mcps.push(collect_custom_mcp_server()?);
            continue;
        }

        if let Some(entry) = mcp_registry.iter().find(|entry| entry.toml.name == chosen) {
            let config = collect_mcp_config(entry)?;
            confirmed_mcps.push(configured_mcp_from_entry(entry, config));
            confirmed_mcp_names.insert(entry.toml.name.clone());
        }
    }

    println!("\nQ7 — Confirm");
    println!("  Global: {}", SKILLS.join(", "));
    if let Some(ref environment) = environment {
        println!("  Environment: {environment}");
    }
    if !confirmed_domains.is_empty() {
        println!("  Domain: {}", confirmed_domains.join(", "));
    }
    if pdf_standards_skill {
        println!("  Vertical skill: {PDF_STANDARDS_SKILL}");
    }
    if !stakeholder_attitudes.is_empty() {
        println!("  Attitudes:");
        for attitude in &stakeholder_attitudes {
            println!("    - {attitude}");
        }
    }
    if !confirmed_projects.is_empty() {
        println!("  Project: {}", confirmed_projects.join(", "));
    }
    let selected_external_skill_names = external_skill_sources
        .iter()
        .flat_map(|source| source.selected_skill_names.iter().cloned())
        .collect::<Vec<_>>();
    if !selected_external_skill_names.is_empty() {
        println!(
            "  External skills: {}",
            selected_external_skill_names.join(", ")
        );
    }
    let configured_mcp_names = confirmed_mcps
        .iter()
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();
    if !configured_mcp_names.is_empty() {
        println!("  MCP servers: {}", configured_mcp_names.join(", "));
    }
    println!();

    if !prompt_bool("Deploy these skills and write intent documents?", true)? {
        bail!("Interview cancelled.");
    }

    let answers = InterviewAnswers {
        repo_purpose,
        operator_intent,
        stakeholder_attitudes: stakeholder_attitudes.clone(),
        prohibitions,
        environment: environment.clone(),
        vertical: vertical.clone(),
        confirmed_domains: confirmed_domains.clone(),
        confirmed_projects: {
            let mut projects = confirmed_projects.clone();
            if pdf_standards_skill {
                extend_unique(&mut projects, [PDF_STANDARDS_SKILL.to_string()]);
            }
            projects
        },
        external_skill_sources: external_skill_sources
            .iter()
            .map(|selection| selection.repo_url.clone())
            .collect(),
        confirmed_mcps: confirmed_mcps.clone(),
    };

    copy_skills(repo_path, harkonnen_root)?;

    let mut deployed = SKILLS
        .iter()
        .map(|skill| (*skill).to_string())
        .collect::<Vec<_>>();
    let mut extra_permissions = Vec::new();

    if let Some(ref environment_name) = environment {
        if let Some(entry) = find_skill_entry(&registry, environment_name) {
            deploy_skill(entry, repo_path)?;
            extend_unique(&mut extra_permissions, entry.toml.extra_permissions.clone());
            extend_unique(&mut deployed, [environment_name.clone()]);
        }
    }

    for name in &confirmed_domains {
        if let Some(entry) = find_skill_entry(&registry, name) {
            deploy_skill(entry, repo_path)?;
            extend_unique(&mut extra_permissions, entry.toml.extra_permissions.clone());
            extend_unique(&mut deployed, [name.clone()]);
        }
    }

    for name in &confirmed_projects {
        if let Some(entry) = find_skill_entry(&registry, name) {
            deploy_skill(entry, repo_path)?;
            extend_unique(&mut extra_permissions, entry.toml.extra_permissions.clone());
            extend_unique(&mut deployed, [name.clone()]);
        }
    }

    if pdf_standards_skill {
        if let Some(entry) = find_skill_entry(&registry, PDF_STANDARDS_SKILL) {
            deploy_skill(entry, repo_path)?;
            extend_unique(&mut extra_permissions, entry.toml.extra_permissions.clone());
            extend_unique(&mut deployed, [PDF_STANDARDS_SKILL.to_string()]);
        }
    }

    for source in &external_skill_sources {
        if source.selected_skill_names.is_empty() {
            continue;
        }
        match fetch_skills_from_repo(&source.repo_url, repo_path, &source.selected_skill_names)
            .await
        {
            Ok(copied) => {
                if !copied.is_empty() {
                    println!(
                        "  Fetched external skills from {}: {}",
                        source.repo_url,
                        copied.join(", ")
                    );
                    extend_unique(&mut deployed, copied);
                }
            }
            Err(error) => {
                println!(
                    "  Warning: failed to fetch skills from {}: {}",
                    source.repo_url, error
                );
            }
        }
    }

    let settings_path = repo_path.join(".claude").join("settings.json");
    let local_settings_path = repo_path.join(".claude").join("settings.local.json");
    merge_skill_permissions_into_settings(&settings_path, &extra_permissions)?;
    write_mcps_to_settings(&settings_path, &local_settings_path, &confirmed_mcps)?;

    let archive_seed = build_archive_seed(&answers, &scan, &existing.repo_name, &deployed);
    let archive_path = repo_path.join(".harkonnen").join("archive-seed.md");
    write_text_file(&archive_path, &archive_seed)?;

    let intent_doc = build_intent_doc(&answers, &existing.repo_name, &deployed);
    let intent_path = repo_path.join(".harkonnen").join("intent.md");
    write_text_file(&intent_path, &intent_doc)?;

    let interview_context_doc =
        build_interview_context_doc(&answers, &existing.repo_name, &deployed);
    let interview_context_path = repo_path.join(".harkonnen").join("interview-context.md");
    write_text_file(&interview_context_path, &interview_context_doc)?;

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
        let mut rendered = render_template(&raw, &existing.repo_name, harkonnen_root);
        let domain_sections = build_domain_claude_sections(&answers.confirmed_projects);
        let mut all_sections = build_domain_claude_sections(&confirmed_domains);
        if !domain_sections.is_empty() {
            all_sections.push_str(&domain_sections);
        }
        if !all_sections.is_empty() {
            rendered.push_str("\n---\n");
            rendered.push_str(&all_sections);
        }
        rendered.push_str(&build_claude_interview_section(
            &answers,
            &interview_context_path,
        ));
        write_text_file(&claude_md_dst, &rendered)?;
    }

    let mut all_domains_for_toml = confirmed_domains.clone();
    extend_unique(&mut all_domains_for_toml, confirmed_projects.clone());
    if pdf_standards_skill {
        extend_unique(&mut all_domains_for_toml, [PDF_STANDARDS_SKILL.to_string()]);
    }
    let mut mcp_server_names = confirmed_mcps
        .iter()
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();
    mcp_server_names.sort();
    mcp_server_names.dedup();

    let updated_toml = RepoToml {
        stamp_version: STAMP_VERSION,
        harkonnen_root: harkonnen_root.to_path_buf(),
        repo_name: existing.repo_name.clone(),
        managed_since: existing.managed_since.clone(),
        repo_purpose: Some(answers.repo_purpose.clone()),
        operator_intent: answers.operator_intent.clone(),
        environment,
        domains: all_domains_for_toml,
        vertical,
        attitudes: stakeholder_attitudes,
        features: existing.features.clone(),
        constraints: answers.prohibitions.clone(),
        skill_sources: answers.external_skill_sources.clone(),
        mcp_servers: mcp_server_names,
        interview_completed: true,
    };
    write_repo_toml(repo_path, &updated_toml)?;

    println!("\n=== Interview complete ===");
    println!("  archive-seed:       {}", archive_path.display());
    println!("  intent:             {}", intent_path.display());
    println!("  interview-context:  {}", interview_context_path.display());
    println!("  CLAUDE.md:          {}", claude_md_dst.display());
    println!("  repo.toml:          interview_completed = true");
    println!("  skills deployed:    {}", deployed.join(", "));
    if !confirmed_mcps.is_empty() {
        println!("  MCP settings:       {}", settings_path.display());
        if confirmed_mcps
            .iter()
            .any(|server| server.settings_target == "settings.local.json")
        {
            println!("  MCP local settings: {}", local_settings_path.display());
        }
    }
    print_mcp_env_notes(&confirmed_mcps);

    Ok(())
}
