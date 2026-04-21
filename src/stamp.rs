use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::claude_pack::{copy_if_exists, scan_target_project, write_text_file};
use crate::cli::{prompt_bool, prompt_choice, prompt_text};
use crate::skill_registry::{auto_select_skills, deploy_skill, load_registry, SkillEntry};

const STAMP_VERSION: u32 = 2;
const STAMP_MAJOR: u32 = 1;

const SKILLS: &[&str] = &["coobie", "scout", "keeper", "sable", "harkonnen"];

#[derive(Debug, Default, Deserialize)]
struct RepoTomlRaw {
    stamp_version: u32,
    harkonnen_root: String,
    repo_name: String,
    managed_since: String,
    updated_at: Option<String>,
    environment: Option<String>,
    domains: Option<Vec<String>>,
    features: Option<Vec<String>>,
    constraints: Option<Vec<String>>,
    interview_completed: Option<bool>,
}

#[derive(Debug)]
struct RepoToml {
    stamp_version: u32,
    harkonnen_root: PathBuf,
    repo_name: String,
    managed_since: String,
    environment: Option<String>,
    domains: Vec<String>,
    features: Vec<String>,
    constraints: Vec<String>,
    interview_completed: bool,
}

impl From<RepoTomlRaw> for RepoToml {
    fn from(r: RepoTomlRaw) -> Self {
        RepoToml {
            stamp_version: r.stamp_version,
            harkonnen_root: PathBuf::from(r.harkonnen_root),
            repo_name: r.repo_name,
            managed_since: r.managed_since,
            environment: r.environment,
            domains: r.domains.unwrap_or_default(),
            features: r.features.unwrap_or_default(),
            constraints: r.constraints.unwrap_or_default(),
            interview_completed: r.interview_completed.unwrap_or(false),
        }
    }
}

struct InterviewAnswers {
    repo_purpose: String,
    operator_intent: Option<String>,
    prohibitions: Vec<String>,
    environment: Option<String>,
    confirmed_domains: Vec<String>,
    confirmed_projects: Vec<String>,
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
    let now = Utc::now().to_rfc3339();
    let mut content = format!(
        "stamp_version = {}\nharkonnen_root = \"{}\"\nrepo_name = \"{}\"\nmanaged_since = \"{}\"\nupdated_at = \"{now}\"\n",
        STAMP_VERSION,
        toml.harkonnen_root.display(),
        toml.repo_name,
        toml.managed_since,
    );
    if let Some(ref env) = toml.environment {
        content.push_str(&format!("environment = \"{env}\"\n"));
    }
    if !toml.domains.is_empty() {
        let domains_toml = toml
            .domains
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("domains = [{domains_toml}]\n"));
    }
    if !toml.features.is_empty() {
        let feats = toml
            .features
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("features = [{feats}]\n"));
    }
    if !toml.constraints.is_empty() {
        let cons = toml
            .constraints
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("constraints = [{cons}]\n"));
    }
    if toml.interview_completed {
        content.push_str("interview_completed = true\n");
    }
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
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };

    let permissions = obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    let allow = permissions
        .get_mut("allow")
        .and_then(|v| v.as_array_mut());

    if let Some(allow_arr) = allow {
        for perm in perms {
            let val = serde_json::Value::String(perm.clone());
            if !allow_arr.contains(&val) {
                allow_arr.push(val);
            }
        }
    } else {
        let allow_arr: Vec<serde_json::Value> = perms
            .iter()
            .map(|p| serde_json::Value::String(p.clone()))
            .collect();
        if let Some(permissions_obj) = permissions.as_object_mut() {
            permissions_obj.insert(
                "allow".to_string(),
                serde_json::Value::Array(allow_arr),
            );
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
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let operator_intent = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.")
        .to_string();

    let environment = answers
        .environment
        .as_deref()
        .unwrap_or("not specified")
        .to_string();

    let all_domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domains_str = if all_domains.is_empty() {
        "none".to_string()
    } else {
        all_domains.join(", ")
    };

    let stack_str = if scan.stack_signals.is_empty() {
        "none detected".to_string()
    } else {
        scan.stack_signals.join(", ")
    };

    let logos = if scan.detected_roots.is_empty() {
        "No structural roots detected.".to_string()
    } else {
        scan.detected_roots
            .iter()
            .map(|r| format!("- `{r}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let validation = if scan.validation_commands.is_empty() {
        "none detected".to_string()
    } else {
        scan.validation_commands.join(", ")
    };

    let skills_str = deployed_skills.join(", ");

    render_with_vars(
        include_str!("../factory/templates/archive-seed.md"),
        &[
            ("repo_name", repo_name),
            ("generated_at", &generated_at),
            ("mythos", &answers.repo_purpose),
            ("environment", &environment),
            ("domains", &domains_str),
            ("stack_signals", &stack_str),
            ("episteme", ""),
            ("prohibitions", &prohibitions),
            ("operator_intent", &operator_intent),
            ("logos_structure", &logos),
            ("deployed_skills", &skills_str),
            ("validation_commands", &validation),
        ],
    )
}

fn build_intent_doc(answers: &InterviewAnswers, repo_name: &str, deployed_skills: &[String]) -> String {
    let generated_at = Utc::now().to_rfc3339();

    let prohibitions_list = if answers.prohibitions.is_empty() {
        "None specified.".to_string()
    } else {
        answers
            .prohibitions
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let operator_intent = answers
        .operator_intent
        .as_deref()
        .unwrap_or("Not specified.")
        .to_string();

    let environment = answers
        .environment
        .as_deref()
        .unwrap_or("not specified")
        .to_string();

    let all_domains: Vec<String> = answers
        .confirmed_domains
        .iter()
        .chain(answers.confirmed_projects.iter())
        .cloned()
        .collect();
    let domains_list = if all_domains.is_empty() {
        "none".to_string()
    } else {
        all_domains
            .iter()
            .map(|d| format!("- {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let skills_list = deployed_skills
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");

    render_with_vars(
        include_str!("../factory/templates/intent.md"),
        &[
            ("repo_name", repo_name),
            ("generated_at", &generated_at),
            ("repo_purpose", &answers.repo_purpose),
            ("operator_intent", &operator_intent),
            ("prohibitions_list", &prohibitions_list),
            ("environment", &environment),
            ("domains_list", &domains_list),
            ("skills_list", &skills_list),
        ],
    )
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

    let new_toml = RepoToml {
        stamp_version: STAMP_VERSION,
        harkonnen_root: harkonnen_root.to_path_buf(),
        repo_name: repo_name.clone(),
        managed_since,
        environment: existing.as_ref().and_then(|t| t.environment.clone()),
        domains: existing.as_ref().map(|t| t.domains.clone()).unwrap_or_default(),
        features: existing.as_ref().map(|t| t.features.clone()).unwrap_or_default(),
        constraints: existing.as_ref().map(|t| t.constraints.clone()).unwrap_or_default(),
        interview_completed: existing.as_ref().map(|t| t.interview_completed).unwrap_or(false),
    };

    write_repo_toml(repo_path, &new_toml)?;

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
    re_interview: bool,
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
        environment: existing.environment.clone(),
        domains: existing.domains.clone(),
        features: existing.features.clone(),
        constraints: existing.constraints.clone(),
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

    println!("repo:              {}", repo_path.display());
    println!("repo_name:         {}", toml.repo_name);
    println!("stamp_version:     {}", toml.stamp_version);
    println!("harkonnen_root:    {}", toml.harkonnen_root.display());
    println!("managed_since:     {}", toml.managed_since);
    println!(
        "interview_completed: {}",
        if toml.interview_completed { "yes" } else { "no" }
    );
    if let Some(ref env) = toml.environment {
        println!("environment:       {env}");
    }
    if !toml.domains.is_empty() {
        println!("domains:           {}", toml.domains.join(", "));
    }
    println!();
    println!("skills:");
    for skill in SKILLS {
        let skill_path = repo_path
            .join(".claude")
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        let status = if skill_path.exists() { "present" } else { "MISSING" };
        println!("  {skill:<12} {status}");
    }

    Ok(())
}

pub async fn stamp_interview(
    repo_path: &Path,
    harkonnen_root: &Path,
    force: bool,
) -> Result<()> {
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

    let existing = existing.unwrap();

    // Auto-scan
    let scan = scan_target_project(repo_path)?;
    let platform = std::env::consts::OS;
    let registry_root = harkonnen_root.join("factory").join("skill-registry");
    let registry = load_registry(&registry_root).unwrap_or_default();
    let auto_selected = auto_select_skills(&registry, platform, &scan.stack_signals);

    println!("\n=== Stamp Interview: {} ===\n", existing.repo_name);
    println!("This interview seeds the Calvin Archive for Coobie.");
    println!("Detected platform: {platform}");
    if !scan.stack_signals.is_empty() {
        println!("Detected stack signals:");
        for s in &scan.stack_signals {
            println!("  - {s}");
        }
    }
    println!();

    // Q1 — Mythos
    let repo_purpose = prompt_text("What is this repo for? (one or two sentences)", "")?;
    if repo_purpose.is_empty() {
        bail!("Repo purpose is required.");
    }

    // Q2 — Pathos
    println!("\nWho uses it and what breaks if it fails? (blank to skip)");
    let operator_intent_raw = prompt_text("Stakes", "")?;
    let operator_intent = if operator_intent_raw.is_empty() {
        None
    } else {
        Some(operator_intent_raw)
    };

    // Q3 — Ethos: prohibitions
    println!("\nWhat must NEVER happen in this repo?");
    println!("Enter one prohibition per line. Blank line to finish.");
    let mut prohibitions = Vec::new();
    loop {
        let p = prompt_text("Prohibition (blank to finish)", "")?;
        if p.is_empty() {
            break;
        }
        prohibitions.push(p);
    }

    // Q4 — Environment
    println!("\nEnvironment detection:");
    let env_auto: Vec<&SkillEntry> = auto_selected
        .iter()
        .filter(|e| e.toml.tier == "environment")
        .copied()
        .collect();

    let detected_env = if !env_auto.is_empty() {
        let name = &env_auto[0].toml.name;
        println!("  Detected: {name}");
        name.as_str()
    } else {
        println!("  No environment auto-detected for platform '{platform}'");
        ""
    };

    let environment = if !detected_env.is_empty()
        && prompt_bool(&format!("Use '{detected_env}' as environment?"), true)?
    {
        Some(detected_env.to_string())
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
    };

    // Q5 — Domain skills
    println!("\nDomain skill selection:");
    let domain_auto: Vec<&SkillEntry> = auto_selected
        .iter()
        .filter(|e| e.toml.tier == "domain")
        .copied()
        .collect();

    let mut confirmed_domains: Vec<String> = Vec::new();

    for entry in &domain_auto {
        println!("  Auto-detected: {} — {}", entry.toml.name, entry.toml.description);
        if prompt_bool(&format!("Include '{}' skill?", entry.toml.name), true)? {
            confirmed_domains.push(entry.toml.name.clone());
        }
    }

    loop {
        if !prompt_bool("Add another domain skill?", false)? {
            break;
        }
        let available: Vec<&str> = registry
            .iter()
            .filter(|e| e.toml.tier == "domain" && !confirmed_domains.contains(&e.toml.name))
            .map(|e| e.toml.name.as_str())
            .collect();
        if available.is_empty() {
            println!("No more domain skills available.");
            break;
        }
        println!("Available domain skills:");
        for name in &available {
            if let Some(entry) = registry.iter().find(|e| e.toml.name.as_str() == *name) {
                println!("  {} — {}", name, entry.toml.description);
            }
        }
        let chosen = prompt_text("Skill name", available[0])?;
        if available.contains(&chosen.as_str()) {
            confirmed_domains.push(chosen);
        } else {
            println!("Unknown skill '{chosen}' — skipped.");
        }
    }

    // Q6 — Project skills
    println!("\nProject skill selection:");
    let project_auto: Vec<&SkillEntry> = auto_selected
        .iter()
        .filter(|e| e.toml.tier == "project")
        .copied()
        .collect();

    let mut confirmed_projects: Vec<String> = Vec::new();
    for entry in &project_auto {
        println!("  Auto-detected: {} — {}", entry.toml.name, entry.toml.description);
        if prompt_bool(&format!("Include '{}' skill?", entry.toml.name), true)? {
            confirmed_projects.push(entry.toml.name.clone());
        }
    }

    // Q7 — Confirmation
    println!("\n=== Skills to deploy ===");
    print!("  Global: ");
    println!("{}", SKILLS.join(", "));
    if let Some(ref env) = environment {
        println!("  Environment: {env}");
    }
    if !confirmed_domains.is_empty() {
        println!("  Domain: {}", confirmed_domains.join(", "));
    }
    if !confirmed_projects.is_empty() {
        println!("  Project: {}", confirmed_projects.join(", "));
    }
    println!();

    if !prompt_bool("Deploy these skills and write intent documents?", true)? {
        bail!("Interview cancelled.");
    }

    // Execute
    let answers = InterviewAnswers {
        repo_purpose,
        operator_intent,
        prohibitions,
        environment: environment.clone(),
        confirmed_domains: confirmed_domains.clone(),
        confirmed_projects: confirmed_projects.clone(),
    };

    // 1. Global skills
    copy_skills(repo_path, harkonnen_root)?;

    // 2. Registry skills
    let mut deployed: Vec<String> = SKILLS.iter().map(|s| s.to_string()).collect();
    let mut extra_permissions: Vec<String> = Vec::new();

    let deploy_skill_by_name = |name: &str| -> Option<&SkillEntry> {
        registry.iter().find(|e| e.toml.name == name)
    };

    if let Some(ref env_name) = environment {
        if let Some(entry) = deploy_skill_by_name(env_name) {
            deploy_skill(entry, repo_path)?;
            extra_permissions.extend(entry.toml.extra_permissions.clone());
            deployed.push(env_name.clone());
        }
    }
    for name in &confirmed_domains {
        if let Some(entry) = deploy_skill_by_name(name) {
            deploy_skill(entry, repo_path)?;
            extra_permissions.extend(entry.toml.extra_permissions.clone());
            deployed.push(name.clone());
        }
    }
    for name in &confirmed_projects {
        if let Some(entry) = deploy_skill_by_name(name) {
            deploy_skill(entry, repo_path)?;
            extra_permissions.extend(entry.toml.extra_permissions.clone());
            deployed.push(name.clone());
        }
    }

    // 3. Merge permissions into settings.json
    let settings_path = repo_path.join(".claude").join("settings.json");
    merge_skill_permissions_into_settings(&settings_path, &extra_permissions)?;

    // 4. Write archive-seed.md
    let archive_seed = build_archive_seed(&answers, &scan, &existing.repo_name, &deployed);
    let archive_path = repo_path.join(".harkonnen").join("archive-seed.md");
    write_text_file(&archive_path, &archive_seed)?;

    // 5. Write intent.md
    let intent_doc = build_intent_doc(&answers, &existing.repo_name, &deployed);
    let intent_path = repo_path.join(".harkonnen").join("intent.md");
    write_text_file(&intent_path, &intent_doc)?;

    // 6. Re-render CLAUDE.md with domain sections
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
        let all_domains: Vec<String> = confirmed_domains
            .iter()
            .chain(confirmed_projects.iter())
            .cloned()
            .collect();
        let domain_sections = build_domain_claude_sections(&all_domains);
        if !domain_sections.is_empty() {
            rendered.push_str("\n---\n");
            rendered.push_str(&domain_sections);
        }
        write_text_file(&claude_md_dst, &rendered)?;
    }

    // 7. Write extended repo.toml
    let all_domains_for_toml: Vec<String> = confirmed_domains
        .iter()
        .chain(confirmed_projects.iter())
        .cloned()
        .collect();
    let updated_toml = RepoToml {
        stamp_version: STAMP_VERSION,
        harkonnen_root: harkonnen_root.to_path_buf(),
        repo_name: existing.repo_name.clone(),
        managed_since: existing.managed_since.clone(),
        environment: environment.clone(),
        domains: all_domains_for_toml,
        features: existing.features.clone(),
        constraints: answers.prohibitions.clone(),
        interview_completed: true,
    };
    write_repo_toml(repo_path, &updated_toml)?;

    // 8. Summary
    println!("\n=== Interview complete ===");
    println!("  archive-seed:  {}", archive_path.display());
    println!("  intent:        {}", intent_path.display());
    println!("  CLAUDE.md:     {}", claude_md_dst.display());
    println!("  repo.toml:     interview_completed = true");
    println!("  Skills:        {}", deployed.join(", "));

    Ok(())
}
