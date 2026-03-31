use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    agents::{self, AgentProfile},
    config::Paths,
    setup::{slugify_machine_name, McpServerConfig},
};

const LAB_PACK_START: &str = "<!-- HARKONNEN LAB PACK START -->";
const LAB_PACK_END: &str = "<!-- HARKONNEN LAB PACK END -->";

#[derive(Debug, Clone)]
pub struct ClaudePackRequest {
    pub target_path: String,
    pub project_name: Option<String>,
    pub project_slug: Option<String>,
    pub project_type: String,
    pub domain: Option<String>,
    pub summary: Option<String>,
    pub constraints: Vec<String>,
    pub include_winccoa: bool,
    pub write_settings: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudePackSummary {
    pub project_name: String,
    pub project_slug: String,
    pub project_type: String,
    pub target_root: PathBuf,
    pub pack_root: PathBuf,
    pub settings_path: Option<PathBuf>,
    pub claude_md_path: PathBuf,
    pub agents_written: usize,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PackManifest {
    pack_version: String,
    generated_at: String,
    source_repo: String,
    active_setup: String,
    project: PackProject,
    agents: Vec<String>,
    mcp_servers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PackProject {
    name: String,
    slug: String,
    project_type: String,
    domain: String,
    summary: String,
    target_root: String,
}

#[derive(Debug, Clone)]
struct ProjectProfile {
    name: String,
    slug: String,
    project_type: String,
    domain: String,
    summary: String,
    constraints: Vec<String>,
    include_winccoa: bool,
}

pub fn export_claude_pack(paths: &Paths, req: ClaudePackRequest) -> Result<ClaudePackSummary> {
    let target_root = resolve_target_path(&paths.root, &req.target_path)?;
    if !target_root.exists() {
        bail!("target path does not exist: {}", target_root.display());
    }
    if !target_root.is_dir() {
        bail!("target path is not a directory: {}", target_root.display());
    }

    let write_settings = req.write_settings;
    let profile = build_project_profile(&target_root, req);
    let pack_root = target_root.join(".harkonnen");
    let claude_dir = target_root.join(".claude");
    let agents_dir = claude_dir.join("agents");
    let context_dir = pack_root.join("context");
    let memory_dir = pack_root.join("memory");
    let memory_notes_dir = memory_dir.join("notes");

    fs::create_dir_all(&agents_dir)?;
    fs::create_dir_all(&context_dir)?;
    fs::create_dir_all(&memory_notes_dir)?;

    let profiles = agents::load_profiles(&paths.factory.join("agents").join("profiles"))?;
    let mut agent_names: Vec<_> = profiles.keys().cloned().collect();
    agent_names.sort();

    write_text_file(&pack_root.join("README.md"), &build_pack_readme(&profile, paths))?;
    write_text_file(
        &pack_root.join("project-context.md"),
        &build_project_context(&profile, paths),
    )?;
    write_text_file(
        &pack_root.join("launch-guide.md"),
        &build_launch_guide(&profile),
    )?;
    write_text_file(
        &pack_root.join("spec-template.yaml"),
        &build_spec_template(&profile),
    )?;
    write_text_file(&memory_notes_dir.join("README.md"), MEMORY_NOTES_README)?;

    copy_if_exists(
        &paths.factory.join("context").join("agent-roster.yaml"),
        &context_dir.join("agent-roster.yaml"),
    )?;
    copy_if_exists(
        &paths.factory.join("context").join("mcp-tools.yaml"),
        &context_dir.join("mcp-tools.yaml"),
    )?;
    write_text_file(
        &context_dir.join("active-setup.toml"),
        &toml::to_string_pretty(&paths.setup)?,
    )?;

    let manifest = PackManifest {
        pack_version: "1".to_string(),
        generated_at: Utc::now().to_rfc3339(),
        source_repo: paths.root.display().to_string(),
        active_setup: paths.setup.setup.name.clone(),
        project: PackProject {
            name: profile.name.clone(),
            slug: profile.slug.clone(),
            project_type: profile.project_type.clone(),
            domain: profile.domain.clone(),
            summary: profile.summary.clone(),
            target_root: target_root.display().to_string(),
        },
        agents: agent_names.clone(),
        mcp_servers: build_mcp_server_names(paths, profile.include_winccoa),
    };
    write_text_file(
        &context_dir.join("system-manifest.yaml"),
        &serde_yaml::to_string(&manifest)?,
    )?;

    for file_name in [
        "00-system-context.md",
        "01-agent-roster.md",
        "02-setup-guide.md",
        "03-mcp-tools.md",
        "04-spec-format.md",
        "index.json",
    ] {
        copy_if_exists(&paths.memory.join(file_name), &memory_dir.join(file_name))?;
    }

    for agent_name in &agent_names {
        let profile_data = profiles
            .get(agent_name)
            .with_context(|| format!("missing agent profile: {agent_name}"))?;
        let markdown = build_agent_markdown(agent_name, profile_data, &profile, paths);
        write_text_file(&agents_dir.join(format!("{agent_name}.md")), &markdown)?;
    }
    write_text_file(
        &agents_dir.join("README.md"),
        &build_agents_readme(&agent_names, &profile),
    )?;

    let settings_path = if write_settings {
        let settings_path = claude_dir.join("settings.local.json");
        let mcp_servers = build_claude_settings_servers(paths, profile.include_winccoa);
        merge_json_object_at_path(&settings_path, "mcpServers", Value::Object(mcp_servers))?;
        Some(settings_path)
    } else {
        None
    };

    let claude_md_path = target_root.join("CLAUDE.md");
    merge_claude_md_block(&claude_md_path, &build_root_claude_block(&profile))?;

    Ok(ClaudePackSummary {
        project_name: profile.name,
        project_slug: profile.slug,
        project_type: profile.project_type,
        target_root,
        pack_root,
        settings_path,
        claude_md_path,
        agents_written: agent_names.len(),
        mcp_servers: manifest.mcp_servers,
    })
}

fn build_project_profile(target_root: &Path, req: ClaudePackRequest) -> ProjectProfile {
    let inferred_name = target_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "target-project".to_string());
    let name = req.project_name.unwrap_or(inferred_name);

    let mut project_type = req.project_type.trim().to_string();
    if req.include_winccoa && (project_type.is_empty() || project_type == "generic") {
        project_type = "winccoa".to_string();
    }
    if project_type.is_empty() {
        project_type = "generic".to_string();
    }

    let include_winccoa = req.include_winccoa || project_type.eq_ignore_ascii_case("winccoa");
    let domain = req.domain.unwrap_or_else(|| {
        if include_winccoa {
            "Siemens WinCC OA / industrial automation".to_string()
        } else {
            "software product engineering".to_string()
        }
    });
    let summary = req.summary.unwrap_or_else(|| {
        if include_winccoa {
            format!(
                "{name} is a Siemens WinCC OA product prepared for a Claude-only Harkonnen Labrador pack."
            )
        } else {
            format!("{name} is prepared for a Claude-only Harkonnen Labrador pack.")
        }
    });
    let slug = req
        .project_slug
        .map(|value| slugify_machine_name(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| slugify_machine_name(&name));
    let mut constraints = req.constraints;
    if include_winccoa {
        for rule in [
            "do not connect to or modify live production WinCC OA systems without explicit approval",
            "prefer simulation, exported configs, and local artifacts over runtime mutation",
            "treat SCADA, OT, alarms, and plant-control changes as safety-sensitive",
        ] {
            if !constraints.iter().any(|existing| existing == rule) {
                constraints.push(rule.to_string());
            }
        }
    }

    ProjectProfile {
        name,
        slug,
        project_type,
        domain,
        summary,
        constraints,
        include_winccoa,
    }
}

fn build_pack_readme(profile: &ProjectProfile, paths: &Paths) -> String {
    format!(
        "# Harkonnen Labrador Pack\n\nThis directory was generated by `harkonnen setup claude-pack` for `{}`.\n\nIt gives a separate project a Claude-first Labrador operating pack:\n\n- project-level Claude subagents in `.claude/agents/`\n- local project context in `.harkonnen/`\n- Coobie seed memory copied from Harkonnen Labs\n- MCP settings suitable for Claude Code on a work-laptop setup\n\nActive setup snapshot: `{}`\nProject type: `{}`\nDomain: `{}`\n\nStart with `.harkonnen/launch-guide.md` and `.harkonnen/project-context.md`.\n",
        profile.name, paths.setup.setup.name, profile.project_type, profile.domain
    )
}

fn build_project_context(profile: &ProjectProfile, paths: &Paths) -> String {
    let mut out = format!(
        "# Project Context\n\nProject: {}\nSlug: {}\nType: {}\nDomain: {}\n\nSummary:\n{}\n\nWhy this pack exists:\n- This repo is being operated through a Claude-only Labrador pack.\n- Role discipline matters even though every Labrador ultimately runs on Claude Code.\n- The goal is spec-first, observable, boundary-aware delivery rather than ad hoc prompting.\n\nOperating model:\n- Scout shapes requests into Harkonnen-style specs.\n- Mason implements approved scope.\n- Piper handles tools, docs, helper scripts, and MCP-assisted investigation.\n- Bramble validates visible behavior.\n- Sable performs acceptance review without writing implementation.\n- Ash designs twins, simulations, and dependency stubs.\n- Flint packages evidence and rollout notes.\n- Coobie retrieves reusable patterns and stores lessons.\n- Keeper enforces safety, scope, and boundary discipline.\n\nCurrent Harkonnen setup snapshot:\n- setup: {}\n- platform: {}\n- claude surface: {}\n",
        profile.name,
        profile.slug,
        profile.project_type,
        profile.domain,
        profile.summary,
        paths.setup.setup.name,
        paths.setup.setup.platform,
        paths
            .setup
            .providers
            .claude
            .as_ref()
            .and_then(|config| config.surface.clone())
            .unwrap_or_else(|| "claude-code".to_string()),
    );

    if !profile.constraints.is_empty() {
        out.push_str("\nConstraints:\n");
        for item in &profile.constraints {
            out.push_str(&format!("- {item}\n"));
        }
    }

    if profile.include_winccoa {
        out.push_str(
            "\nWinCC OA guidance:\n - Treat CTRL scripts, panels, datapoints, managers, alerting, and runtime actions as operationally sensitive.\n - Prefer read-first investigation, offline exports, simulators, and staged rollout instructions.\n - Any action that could affect a live plant, station, or operator workflow requires explicit human approval.\n - Be precise about panel paths, datapoint schemas, manager boundaries, and deployment assumptions.\n",
        );
    }

    out.push_str(
        "\nFiles to read first:\n - `.harkonnen/project-context.md`\n - `.harkonnen/spec-template.yaml`\n - `.harkonnen/context/system-manifest.yaml`\n - `.harkonnen/context/agent-roster.yaml`\n - `.harkonnen/memory/index.json`\n",
    );

    out
}

fn build_launch_guide(profile: &ProjectProfile) -> String {
    let mut out = format!(
        "# Launch Guide\n\nThis project has a Harkonnen Labrador pack for `{}`.\n\n1. Restart Claude Code after the generated `.claude/settings.local.json` is written.\n2. Run `/agents` and confirm the Labrador subagents are visible.\n3. Ask Scout to turn your requested work into a Harkonnen-style spec using `.harkonnen/spec-template.yaml`.\n4. Ask Mason to implement only after the scope is clear.\n5. Use Bramble for visible validation and Sable for acceptance review.\n6. Use Keeper before any risky operational, deployment, or boundary-crossing step.\n\nSuggested opener:\n`Use Scout to draft a Harkonnen spec for the next {} change, then use Coobie to retrieve similar patterns from .harkonnen/memory.`\n",
        profile.name, profile.name
    );

    if profile.include_winccoa {
        out.push_str(
            "\nSuggested WinCC OA opener:\n`Use Scout to draft a WinCC OA-safe Harkonnen spec for the SPO task, then have Ash outline the twin/simulation approach and Keeper identify any live-system risks.`\n",
        );
    }

    out
}

fn build_spec_template(profile: &ProjectProfile) -> String {
    let dependency_block = if profile.include_winccoa {
        "dependencies:\n  - winccoa\n  - ctrl\n  - panels\n  - datapoint model\n"
    } else {
        "dependencies:\n  - project runtime\n  - build tooling\n"
    };
    let security_block = if profile.include_winccoa {
        "security_expectations:\n  - no live system changes without explicit approval\n  - protect operator workflows, credentials, and plant safety boundaries\n  - prefer offline inspection and simulation first\n"
    } else {
        "security_expectations:\n  - protect credentials and production boundaries\n"
    };

    format!(
        "id: {slug}_feature\ntitle: Example {name} Change\npurpose: Describe the desired user-visible or operator-visible outcome.\nscope:\n  - add one bounded capability\n  - keep the change observable and reversible\nconstraints:\n{constraints}inputs:\n  - feature request or defect description\n  - relevant repo paths\noutputs:\n  - code/config changes\n  - validation evidence\nacceptance_criteria:\n  - visible behavior matches the requested outcome\n  - operator/developer workflow is documented\nforbidden_behaviors:\n  - silent behavior drift\n  - destructive changes outside approved scope\nrollback_requirements:\n  - changes can be reverted cleanly\n{dependency_block}performance_expectations:\n  - local validation remains practical for iterative work\n{security_block}",
        slug = profile.slug,
        name = profile.name,
        constraints = yaml_bullet_lines(&profile.constraints, "  - remain local-first\n"),
        dependency_block = dependency_block,
        security_block = security_block,
    )
}

fn yaml_bullet_lines(items: &[String], default_block: &str) -> String {
    if items.is_empty() {
        return default_block.to_string();
    }

    let mut out = String::new();
    for item in items {
        out.push_str("  - ");
        out.push_str(item);
        out.push('\n');
    }
    out
}

fn build_agents_readme(agent_names: &[String], profile: &ProjectProfile) -> String {
    let mut out = format!(
        "# Labrador Subagents\n\nThese project-level Claude subagents are installed for `{}`.\n\nUse `/agents` to inspect or refine them.\n\n",
        profile.name
    );
    for name in agent_names {
        out.push_str(&format!("- `{name}`\n"));
    }
    out
}

fn build_agent_markdown(
    agent_name: &str,
    profile: &AgentProfile,
    project: &ProjectProfile,
    paths: &Paths,
) -> String {
    let description = agent_description(agent_name, &project.name);
    let role_rules = agent_role_rules(agent_name, project.include_winccoa);
    let handoff = agent_handoff_rules(agent_name);
    let winccoa_note = if project.include_winccoa {
        "\nWinCC OA note:\n- Treat runtime, managers, datapoints, panels, alarms, and live integrations as safety-sensitive.\n- Prefer read-first investigation and simulation before proposing operational steps.\n"
    } else {
        ""
    };

    format!(
        "---\nname: {agent_name}\ndescription: {description}\n---\n\nYou are {display_name}, the Harkonnen Labrador `{agent_name}` for `{project_name}`.\n\nYou are part of a Claude Code subagent pack. Even though every Labrador runs on Claude on this machine, role discipline still matters. Stay inside your specialty, return useful progress, and hand off cleanly when another Labrador should take over.\n\nShared Labrador personality:\n- loyal to the mission\n- persistent and calm under repetition\n- honest when uncertain\n- non-destructive and boundary-aware\n- clear in summaries and next steps\n\nNon-negotiable rules:\n- return something useful every time\n- do not fail silently\n- do not bluff\n- do not take destructive actions without approval\n- protect the workspace, artifacts, and secrets\n\nRead these first when they matter:\n- `.harkonnen/project-context.md`\n- `.harkonnen/context/system-manifest.yaml`\n- `.harkonnen/context/agent-roster.yaml`\n- `.harkonnen/spec-template.yaml`\n- `.harkonnen/memory/index.json`\n- `.harkonnen/launch-guide.md`\n\nRole:\n- display name: {display_name}\n- factory role: {role}\n- preferred provider in source factory: {provider}\n- current project pack: Claude Code project subagent\n\nResponsibilities:\n{responsibilities}Role-specific operating rules:\n{role_rules}Handoff rules:\n{handoff}Project constraints:\n{constraints}Project summary:\n{summary}\n{winccoa_note}\nWhen you finish, leave the main thread with:\n- what you learned or changed\n- any blockers or risks\n- which Labrador should go next, if any\n",
        agent_name = agent_name,
        description = description,
        display_name = profile.display_name,
        project_name = project.name,
        role = profile.role,
        provider = paths
            .setup
            .resolve_agent_provider_name(&profile.name, &profile.provider),
        responsibilities = bullet_lines(&profile.responsibilities),
        role_rules = role_rules,
        handoff = handoff,
        constraints = bullet_lines(&project.constraints),
        summary = project.summary,
        winccoa_note = winccoa_note,
    )
}

fn agent_description(agent_name: &str, project_name: &str) -> String {
    match agent_name {
        "scout" => format!(
            "Harkonnen spec intake specialist for {project_name}. MUST BE USED first when requests are ambiguous, risky, or need to be turned into a scoped implementation spec."
        ),
        "mason" => format!(
            "Harkonnen implementation specialist for {project_name}. Use proactively after scope is clear and code or config changes are needed."
        ),
        "piper" => format!(
            "Harkonnen tools and automation specialist for {project_name}. Use for build helpers, docs lookup, scripts, and MCP-assisted investigation."
        ),
        "bramble" => format!(
            "Harkonnen validation specialist for {project_name}. Use proactively for visible tests, checks, and validation loops after changes."
        ),
        "sable" => format!(
            "Harkonnen acceptance reviewer for {project_name}. Use for independent acceptance review, hidden-risk thinking, and scenario evaluation. MUST NOT write implementation."
        ),
        "ash" => format!(
            "Harkonnen twin and simulation specialist for {project_name}. Use for dependency stubs, local twin plans, and safe test-environment design."
        ),
        "flint" => format!(
            "Harkonnen artifact and evidence specialist for {project_name}. Use for packaging outputs, change summaries, rollout evidence, and handoff bundles."
        ),
        "coobie" => format!(
            "Harkonnen memory specialist for {project_name}. Use proactively to retrieve prior patterns, summarize lessons, and keep reusable knowledge organized."
        ),
        "keeper" => format!(
            "Harkonnen safety and policy specialist for {project_name}. MUST BE USED for risky actions, boundary review, live-system risk review, and coordination conflicts."
        ),
        _ => format!("Harkonnen Labrador for {project_name}."),
    }
}

fn agent_role_rules(agent_name: &str, include_winccoa: bool) -> String {
    match agent_name {
        "scout" => bullet_lines(&[
            "shape requests into Harkonnen-style specs before implementation begins".to_string(),
            "surface ambiguity, missing constraints, and acceptance gaps".to_string(),
            "do not implement code or operational changes".to_string(),
        ]),
        "mason" => bullet_lines(&[
            "implement the requested change with minimal, intentional edits".to_string(),
            "preserve established project patterns unless the spec calls for change".to_string(),
            "stop and call Keeper if the task starts to cross safety or boundary lines".to_string(),
        ]),
        "piper" => {
            let mut rules = vec![
                "run or prepare tools, scripts, and documentation workflows that unblock the pack".to_string(),
                "prefer repeatable commands and clear operator notes".to_string(),
            ];
            if include_winccoa {
                rules.push(
                    "treat WinCC OA operational tooling as read-first unless explicitly told otherwise".to_string(),
                );
            }
            bullet_lines(&rules)
        }
        "bramble" => bullet_lines(&[
            "focus on visible validation, reproducible checks, and actionable failure analysis".to_string(),
            "do not silently waive failing checks".to_string(),
        ]),
        "sable" => bullet_lines(&[
            "review the work as an evaluator, not as an implementer".to_string(),
            "do not edit implementation files".to_string(),
            "look for hidden-risk scenarios, edge cases, and acceptance gaps".to_string(),
        ]),
        "ash" => {
            let mut rules = vec![
                "design safe local twins, stubs, and dependency simulations".to_string(),
                "be explicit about what is simulated versus real".to_string(),
            ];
            if include_winccoa {
                rules.push(
                    "prefer offline WinCC OA topology sketches, mocked datapoints, and manager simulations over runtime mutation".to_string(),
                );
            }
            bullet_lines(&rules)
        }
        "flint" => bullet_lines(&[
            "package evidence so a human can understand what changed and how to verify it".to_string(),
            "favor concise release notes, rollback notes, and artifact checklists".to_string(),
        ]),
        "coobie" => bullet_lines(&[
            "retrieve prior patterns from `.harkonnen/memory/index.json` and related notes before answering".to_string(),
            "store durable lessons under `.harkonnen/memory/notes/` when they are worth reusing".to_string(),
            "call out when the memory corpus is thin or missing domain examples".to_string(),
        ]),
        "keeper" => {
            let mut rules = vec![
                "review risky steps, secrets exposure, scope drift, and destructive actions before they happen".to_string(),
                "keep the main thread honest about what is safe, approved, and reversible".to_string(),
            ];
            if include_winccoa {
                rules.push(
                    "treat live SCADA, OT, operator workflow, and plant-facing changes as high risk by default".to_string(),
                );
            }
            bullet_lines(&rules)
        }
        _ => bullet_lines(&["stay within your assigned specialty".to_string()]),
    }
}

fn agent_handoff_rules(agent_name: &str) -> String {
    let targets = match agent_name {
        "scout" => vec!["mason", "coobie", "keeper"],
        "mason" => vec!["bramble", "piper", "keeper"],
        "piper" => vec!["mason", "bramble", "keeper"],
        "bramble" => vec!["sable", "mason", "keeper"],
        "sable" => vec!["keeper", "flint"],
        "ash" => vec!["mason", "bramble", "keeper"],
        "flint" => vec!["keeper"],
        "coobie" => vec!["scout", "mason", "bramble"],
        "keeper" => vec!["scout", "mason", "flint"],
        _ => vec!["keeper"],
    };
    let lines: Vec<String> = targets
        .into_iter()
        .map(|name| format!("hand off to `{name}` when that role is the better next step"))
        .collect();
    bullet_lines(&lines)
}

fn bullet_lines(items: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
    out
}

fn build_root_claude_block(profile: &ProjectProfile) -> String {
    let winccoa_note = if profile.include_winccoa {
        "\n- Treat WinCC OA runtime access, datapoints, panels, alarms, and manager operations as safety-sensitive."
    } else {
        ""
    };

    format!(
        "{LAB_PACK_START}\n## Harkonnen Labrador Pack\n\nThis repo includes project-level Claude subagents under `.claude/agents/`.\n\nUse the Labradors proactively:\n- Scout first for spec shaping and ambiguity review\n- Mason for implementation\n- Piper for tools, docs, and helpers\n- Bramble for visible validation\n- Sable for acceptance review without writing code\n- Ash for twin/simulation design\n- Flint for evidence packaging\n- Coobie for memory retrieval and lesson capture\n- Keeper for safety and boundary review\n\nPrimary context files:\n- `.harkonnen/project-context.md`\n- `.harkonnen/launch-guide.md`\n- `.harkonnen/spec-template.yaml`\n- `.harkonnen/context/system-manifest.yaml`\n{winccoa_note}\n{LAB_PACK_END}\n"
    )
}

fn build_claude_settings_servers(paths: &Paths, include_winccoa: bool) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert(
        "filesystem".to_string(),
        json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem", ".", "./.harkonnen"]
        }),
    );
    out.insert(
        "memory".to_string(),
        json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-memory"],
            "env": {
                "MEMORY_FILE_PATH": "./.harkonnen/memory/store.json"
            }
        }),
    );

    if let Some(mcp) = &paths.setup.mcp {
        for server in &mcp.servers {
            match server.name.as_str() {
                "filesystem" | "memory" | "sqlite" => continue,
                "winccoa" if !include_winccoa => continue,
                _ => {
                    out.insert(server.name.clone(), server_to_settings_json(server));
                }
            }
        }
    }

    if include_winccoa && !out.contains_key("winccoa") {
        out.insert(
            "winccoa".to_string(),
            json!({
                "command": "winccoa-mcp",
                "args": [],
                "env": {
                    "WINCCOA_URL": "WINCCOA_URL",
                    "WINCCOA_PROJECT": "WINCCOA_PROJECT",
                    "WINCCOA_USERNAME": "WINCCOA_USERNAME",
                    "WINCCOA_PASSWORD": "WINCCOA_PASSWORD"
                }
            }),
        );
    }

    out
}

fn build_mcp_server_names(paths: &Paths, include_winccoa: bool) -> Vec<String> {
    let mut names: Vec<String> = build_claude_settings_servers(paths, include_winccoa)
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

fn server_to_settings_json(server: &McpServerConfig) -> Value {
    let mut object = Map::new();
    object.insert("command".to_string(), Value::String(server.command.clone()));
    object.insert(
        "args".to_string(),
        Value::Array(
            server
                .args
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    if let Some(env) = &server.env {
        let env_object: Map<String, Value> = env
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();
        object.insert("env".to_string(), Value::Object(env_object));
    }
    Value::Object(object)
}

fn merge_json_object_at_path(path: &Path, key: &str, new_value: Value) -> Result<()> {
    let mut root = if path.exists() {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading JSON settings file {}", path.display()))?;
        serde_json::from_str::<Value>(&raw)
            .with_context(|| format!("parsing JSON settings file {}", path.display()))?
    } else {
        json!({})
    };

    if !root.is_object() {
        root = json!({});
    }

    let object = root
        .as_object_mut()
        .context("root JSON settings value was not an object")?;
    let target = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }

    let target_map = target
        .as_object_mut()
        .context("settings target was not an object")?;
    let Some(new_map) = new_value.as_object() else {
        bail!("new JSON settings content for {key} was not an object");
    };
    for (child_key, child_value) in new_map {
        target_map.insert(child_key.clone(), child_value.clone());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&root)?)
        .with_context(|| format!("writing JSON settings file {}", path.display()))?;
    Ok(())
}

fn merge_claude_md_block(path: &Path, block: &str) -> Result<()> {
    let merged = if path.exists() {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        upsert_marked_block(&existing, block)
    } else {
        block.to_string()
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, merged).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn upsert_marked_block(existing: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (existing.find(LAB_PACK_START), existing.find(LAB_PACK_END)) {
        let end = end + LAB_PACK_END.len();
        let mut merged = String::new();
        merged.push_str(&existing[..start]);
        if !merged.ends_with('\n') && !merged.is_empty() {
            merged.push('\n');
        }
        merged.push_str(block);
        if end < existing.len() {
            if !block.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(existing[end..].trim_start_matches('\n'));
        }
        return merged;
    }

    if existing.trim().is_empty() {
        block.to_string()
    } else {
        format!("{}\n\n{}", existing.trim_end(), block)
    }
}

fn resolve_target_path(root: &Path, raw: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        root.join(candidate)
    };
    Ok(resolved.canonicalize().unwrap_or(resolved))
}

fn write_text_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn copy_if_exists(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(from).with_context(|| format!("reading {}", from.display()))?;
    write_text_file(to, &raw)
}

const MEMORY_NOTES_README: &str = "# Coobie Project Notes\n\nAdd durable project-specific lessons here as Markdown notes. Rebuild or re-read the pack memory when this corpus grows.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_marked_block_replaces_existing_section() {
        let existing = format!("# Existing\n\n{LAB_PACK_START}\nold\n{LAB_PACK_END}\n\nTail\n");
        let next = upsert_marked_block(&existing, "NEW BLOCK");
        assert!(next.contains("# Existing"));
        assert!(next.contains("NEW BLOCK"));
        assert!(next.contains("Tail"));
        assert!(!next.contains("old"));
    }

    #[test]
    fn settings_merge_keeps_existing_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.local.json");
        fs::write(
            &path,
            r#"{"permissions":{"allow":["Read"]},"mcpServers":{"github":{"command":"npx","args":["-y","old"]}}}"#,
        )
        .expect("seed");

        merge_json_object_at_path(
            &path,
            "mcpServers",
            json!({
                "memory": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-memory"]
                }
            }),
        )
        .expect("merge");

        let merged: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
        assert!(merged.get("permissions").is_some());
        assert!(merged["mcpServers"].get("github").is_some());
        assert!(merged["mcpServers"].get("memory").is_some());
    }
}
