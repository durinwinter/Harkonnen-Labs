use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    agents::{self, AgentProfile},
    config::Paths,
    setup::{slugify_machine_name, McpServerConfig, SystemDiscovery},
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
    machine_scope: String,
    project: PackProject,
    project_scan: ProjectScan,
    engine_routing: EngineRoutingSnapshot,
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

#[derive(Debug, Clone, Serialize, Default)]
struct ProjectScan {
    detected_roots: Vec<String>,
    read_first_files: Vec<String>,
    launch_commands: Vec<String>,
    validation_commands: Vec<String>,
    stack_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct EngineRoutingSnapshot {
    machine_scope: String,
    setup_name: String,
    platform: String,
    default_provider: String,
    coordinator: EngineCoordinator,
    openclaw_enabled: bool,
    openclaw_available: bool,
    providers: Vec<EngineProviderSnapshot>,
    assignments: Vec<AgentEngineAssignment>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct EngineCoordinator {
    provider: String,
    surface: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Default)]
struct EngineProviderSnapshot {
    name: String,
    provider_type: String,
    model: String,
    surface: String,
    enabled: bool,
    usage_rights: Option<String>,
    api_key_env: String,
    credential_state: String,
    operational_state: String,
}

#[derive(Debug, Clone, Serialize, Default)]
struct AgentEngineAssignment {
    agent: String,
    role: String,
    provider: String,
    model: String,
    surface: String,
    source: String,
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
    let scan = scan_target_project(&target_root)?;
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
    let engine_routing = build_engine_routing_snapshot(paths, &profiles, &agent_names);
    let machine_scope = engine_routing.machine_scope.clone();
    let machine_dir = pack_root.join("machines").join(&machine_scope);

    write_text_file(
        &pack_root.join("README.md"),
        &build_pack_readme(&profile, paths),
    )?;
    write_text_file(
        &pack_root.join("project-context.md"),
        &build_project_context(&profile, paths, &scan),
    )?;
    write_text_file(
        &pack_root.join("project-scan.md"),
        &build_project_scan_doc(&profile, &scan),
    )?;
    write_text_file(
        &pack_root.join("launch-guide.md"),
        &build_launch_guide(&profile, paths, &scan),
    )?;
    write_text_file(
        &context_dir.join("engine-routing.md"),
        &build_engine_routing_doc(&engine_routing),
    )?;
    write_text_file(
        &machine_dir.join("engine-routing.yaml"),
        &serde_yaml::to_string(&engine_routing)?,
    )?;
    write_text_file_if_missing(
        &machine_dir.join("engine-state.yaml"),
        &build_engine_state_template(&engine_routing),
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
        machine_scope: machine_scope.clone(),
        project: PackProject {
            name: profile.name.clone(),
            slug: profile.slug.clone(),
            project_type: profile.project_type.clone(),
            domain: profile.domain.clone(),
            summary: profile.summary.clone(),
            target_root: target_root.display().to_string(),
        },
        project_scan: scan.clone(),
        engine_routing: engine_routing.clone(),
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
    merge_claude_md_block(&claude_md_path, &build_root_claude_block(&profile, paths))?;

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
            "OT / industrial automation".to_string()
        } else {
            "software product engineering".to_string()
        }
    });
    let summary = req.summary.unwrap_or_else(|| {
        if include_winccoa {
            format!(
                "{name} is an industrial-control product prepared for a machine-aware Harkonnen Labrador pack."
            )
        } else {
            format!("{name} is prepared for a machine-aware Harkonnen Labrador pack.")
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
    let machine_scope = current_machine_scope(paths);
    let engine_rel = machine_routing_rel(&machine_scope);
    let state_rel = machine_state_rel(&machine_scope);

    format!(
        r#"# Harkonnen Labrador Pack

This directory was generated by `harkonnen setup claude-pack` for `{}`.

It gives a separate project a coordinator-agnostic Labrador operating pack:

- project-level Claude subagents in `.claude/agents/`
- coordinator-agnostic routing and machine state in `.harkonnen/`
- Coobie seed memory copied from Harkonnen Labs
- MCP settings suitable for Claude Code when Claude is the active coordinator

Active setup snapshot: `{}`
Machine scope: `{}`
Project type: `{}`
Domain: `{}`

Start with `.harkonnen/context/engine-routing.md`, `{}`, and `{}`.
"#,
        profile.name,
        paths.setup.setup.name,
        machine_scope,
        profile.project_type,
        profile.domain,
        engine_rel,
        state_rel,
    )
}

fn build_project_context(profile: &ProjectProfile, paths: &Paths, scan: &ProjectScan) -> String {
    let machine_scope = current_machine_scope(paths);
    let engine_rel = machine_routing_rel(&machine_scope);
    let state_rel = machine_state_rel(&machine_scope);
    let discovery = SystemDiscovery::discover();
    let default_provider = paths.setup.providers.default.clone();
    let default_surface = paths
        .setup
        .resolve_provider(&default_provider)
        .and_then(|config| config.surface.clone())
        .unwrap_or_else(|| "unassigned".to_string());
    let mut out = format!(
        r#"# Project Context

Project: {}
Slug: {}
Type: {}
Domain: {}

Summary:
{}

Why this pack exists:
- This repo is being operated through a project-level Harkonnen Labrador pack.
- Role discipline matters even when the coordinator changes between Claude, Codex, Gemini, OpenClaw, or another runner.
- The goal is spec-first, observable, boundary-aware delivery rather than ad hoc prompting.

Operating model:
- Scout shapes requests into Harkonnen-style specs.
- Mason implements approved scope.
- Piper handles tools, docs, helper scripts, and MCP-assisted investigation.
- Bramble validates visible behavior.
- Sable performs acceptance review without writing implementation.
- Ash designs twins, simulations, and dependency stubs.
- Flint packages evidence and rollout notes.
- Coobie retrieves reusable patterns and stores lessons.
- Keeper enforces safety, scope, and boundary discipline.

Current Harkonnen setup snapshot:
- setup: {}
- platform: {}
- default provider: {} via {}
- openclaw enabled in setup: {}
- openclaw detected on this machine: {}
- machine scope: {}
"#,
        profile.name,
        profile.slug,
        profile.project_type,
        profile.domain,
        profile.summary,
        paths.setup.setup.name,
        paths.setup.setup.platform,
        default_provider,
        default_surface,
        paths.setup.setup.openclaw.unwrap_or(false),
        discovery.openclaw,
        machine_scope,
    );

    if !profile.constraints.is_empty() {
        out.push_str(
            r#"
Constraints:
"#,
        );
        for item in &profile.constraints {
            out.push_str(&format!(
                "- {item}
"
            ));
        }
    }

    if profile.include_winccoa {
        out.push_str(
            r#"
WinCC OA guidance:
 - Treat CTRL scripts, panels, datapoints, managers, alerting, and runtime actions as operationally sensitive.
 - Prefer read-first investigation, offline exports, simulators, and staged rollout instructions.
 - Any action that could affect a live plant, station, or operator workflow requires explicit human approval.
 - Be precise about panel paths, datapoint schemas, manager boundaries, and deployment assumptions.
"#,
        );
    }

    out.push_str(&format!(
        r#"
Coordinator-agnostic routing files:
 - `.harkonnen/context/engine-routing.md`
 - `{}`
 - `{}`

Files to read first:
 - `.harkonnen/project-context.md`
 - `.harkonnen/context/engine-routing.md`
 - `{}`
 - `{}`
 - `.harkonnen/project-scan.md`
 - `.harkonnen/spec-template.yaml`
 - `.harkonnen/context/system-manifest.yaml`
 - `.harkonnen/context/agent-roster.yaml`
 - `.harkonnen/memory/index.json`
"#,
        engine_rel, state_rel, engine_rel, state_rel,
    ));

    if !scan.stack_signals.is_empty() {
        out.push_str(
            r#"
Detected stack signals:
"#,
        );
        for item in &scan.stack_signals {
            out.push_str(&format!(
                "- {item}
"
            ));
        }
    }

    if !scan.detected_roots.is_empty() {
        out.push_str(
            r#"
Detected project roots:
"#,
        );
        for item in &scan.detected_roots {
            out.push_str(&format!(
                "- {item}
"
            ));
        }
    }

    if !scan.read_first_files.is_empty() {
        out.push_str(
            r#"
Local project files to read early:
"#,
        );
        for item in scan.read_first_files.iter().take(8) {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    out
}

fn build_project_scan_doc(profile: &ProjectProfile, scan: &ProjectScan) -> String {
    let mut out = format!(
        "# Project Scan

This file captures auto-detected local signals for `{}` so the Labrador pack can stay project-level and reusable across machines.
",
        profile.name,
    );

    if scan.detected_roots.is_empty() {
        out.push_str(
            "
No nested project roots were detected beyond the target root.
",
        );
    } else {
        out.push_str(
            "
Detected roots:
",
        );
        for item in &scan.detected_roots {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if !scan.stack_signals.is_empty() {
        out.push_str(
            "
Detected stack signals:
",
        );
        for item in &scan.stack_signals {
            out.push_str(&format!(
                "- {item}
"
            ));
        }
    }

    if !scan.read_first_files.is_empty() {
        out.push_str(
            "
Read-first local files:
",
        );
        for item in &scan.read_first_files {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if !scan.launch_commands.is_empty() {
        out.push_str(
            "
Launch commands:
",
        );
        for item in &scan.launch_commands {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if !scan.validation_commands.is_empty() {
        out.push_str(
            "
Validation commands:
",
        );
        for item in &scan.validation_commands {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    out
}

fn build_launch_guide(profile: &ProjectProfile, paths: &Paths, scan: &ProjectScan) -> String {
    let machine_scope = current_machine_scope(paths);
    let engine_rel = machine_routing_rel(&machine_scope);
    let state_rel = machine_state_rel(&machine_scope);
    let mut out = format!(
        r#"# Launch Guide

This project has a Harkonnen Labrador pack for `{}`.

1. Any coordinator should read `.harkonnen/context/engine-routing.md`, `{}`, and `{}` first.
2. If Claude Code is coordinating, restart it after the generated `.claude/settings.local.json` is written.
3. Route work according to the current machine-scoped engine plan before asking a Labrador to act.
4. Ask Scout to turn your requested work into a Harkonnen-style spec using `.harkonnen/spec-template.yaml`.
5. Ask Mason to implement only after the scope is clear.
6. Use Bramble for visible validation and Sable for acceptance review.
7. Use Keeper before any risky operational, deployment, or boundary-crossing step.

Suggested opener:
`Use Scout to draft a Harkonnen spec for the next {} change, then confirm the active coordinator and agent routes from .harkonnen/context/engine-routing.md before implementation starts.`
"#,
        profile.name, engine_rel, state_rel, profile.name,
    );

    if !scan.read_first_files.is_empty() {
        out.push_str(
            r#"
Project read-first files:
"#,
        );
        for item in scan.read_first_files.iter().take(6) {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if !scan.launch_commands.is_empty() {
        out.push_str(
            r#"
Local launch commands:
"#,
        );
        for item in &scan.launch_commands {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if !scan.validation_commands.is_empty() {
        out.push_str(
            r#"
Local validation commands:
"#,
        );
        for item in &scan.validation_commands {
            out.push_str(&format!(
                "- `{item}`
"
            ));
        }
    }

    if profile.include_winccoa {
        out.push_str(
            r#"
Suggested WinCC OA opener:
`Use Scout to draft a WinCC OA-safe Harkonnen spec for the target task, then have Ash outline the twin/simulation approach and Keeper identify any live-system risks.`
"#,
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
    let machine_scope = current_machine_scope(paths);
    let engine_rel = machine_routing_rel(&machine_scope);
    let state_rel = machine_state_rel(&machine_scope);
    let personality_addendum = load_agent_personality_addendum(profile, paths).unwrap_or_default();
    let winccoa_note = if project.include_winccoa {
        "
WinCC OA note:
- Treat runtime, managers, datapoints, panels, alarms, and live integrations as safety-sensitive.
- Prefer read-first investigation and simulation before proposing operational steps.
"
    } else {
        ""
    };

    format!(
        r#"---
name: {agent_name}
description: {description}
---

You are {display_name}, the Harkonnen Labrador `{agent_name}` for `{project_name}`.

You are part of a project-level Harkonnen Labrador pack. The coordinator may change between Claude Code, Codex, Gemini, OpenClaw, or another runner depending on this machine and this project's current state. Stay inside your specialty, return useful progress, and hand off cleanly when another Labrador should take over.

Shared Labrador personality:
- loyal to the mission
- persistent and calm under repetition
- honest when uncertain
- non-destructive and boundary-aware
- clear in summaries and next steps

{personality_addendum}Non-negotiable rules:
- return something useful every time
- do not fail silently
- do not bluff
- do not take destructive actions without approval
- protect the workspace, artifacts, and secrets
- respect the effective engine routing for this machine before assuming who should act

Read these first when they matter:
- `.harkonnen/project-context.md`
- `.harkonnen/context/engine-routing.md`
- `{engine_rel}`
- `{state_rel}`
- `.harkonnen/project-scan.md`
- `.harkonnen/context/system-manifest.yaml`
- `.harkonnen/context/agent-roster.yaml`
- `.harkonnen/spec-template.yaml`
- `.harkonnen/memory/index.json`
- `.harkonnen/launch-guide.md`

Role:
- display name: {display_name}
- factory role: {role}
- preferred provider in source factory: {provider}
- current project pack: machine-aware project pack with coordinator-agnostic `.harkonnen` routing

Responsibilities:
{responsibilities}Role-specific operating rules:
{role_rules}Handoff rules:
{handoff}Project constraints:
{constraints}Project summary:
{summary}
{winccoa_note}
When you finish, leave the main thread with:
- what you learned or changed
- any blockers or risks
- which Labrador should go next, if any
"#,
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
        engine_rel = engine_rel,
        state_rel = state_rel,
        personality_addendum = personality_addendum,
    )
}

fn load_agent_personality_addendum(profile: &AgentProfile, paths: &Paths) -> Result<String> {
    if profile.personality_file.ends_with("labrador.md") {
        return Ok(String::new());
    }

    let profile_dir = paths.factory.join("agents").join("profiles");
    let personality_path = profile_dir.join(&profile.personality_file);
    let raw = fs::read_to_string(&personality_path)
        .with_context(|| format!("reading agent personality {}", personality_path.display()))?;
    Ok(if raw.trim().is_empty() {
        String::new()
    } else {
        format!(
            "Agent-specific addendum:
{}

",
            raw.trim()
        )
    })
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
            "Harkonnen memory and causal reasoning specialist for {project_name}. Use proactively before planning, implementation, validation, and twin design so the pack can reuse prior lessons instead of rediscovering them."
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
            "consume Coobie's latest briefing before finalizing scope, ambiguity notes, or recommended steps".to_string(),
            "surface ambiguity, missing constraints, and acceptance gaps".to_string(),
            "do not implement code or operational changes".to_string(),
        ]),
        "mason" => bullet_lines(&[
            "implement the requested change with minimal, intentional edits".to_string(),
            "treat Coobie's guardrails and required checks as constraints, not optional commentary".to_string(),
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
            "prove or disprove Coobie's required checks with evidence instead of generic pass/fail language".to_string(),
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
                "use Coobie's environment risks to decide what the twin must simulate versus what can remain a stub".to_string(),
                "be explicit about what is simulated versus real".to_string(),
                "read product-runtime state from the active product API when twin facts are needed, while leaving pack coordination state on the Harkonnen API".to_string(),
                "treat product or project APIs as read-first until Keeper or the human approves writes".to_string(),
                "start by confirming runtime base URL, protocol, environment, and whether the endpoint is mock, simulator, lab, staging, or production".to_string(),
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
            "retrieve prior patterns from `.harkonnen/memory/index.json` and related notes before the pack plans or acts".to_string(),
            "emit a preflight briefing with domain signals, guardrails, required checks, and open questions".to_string(),
            "emit report-based responses after runs so the pack can react to Coobie's causal findings directly".to_string(),
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

fn build_root_claude_block(profile: &ProjectProfile, paths: &Paths) -> String {
    let machine_scope = current_machine_scope(paths);
    let engine_rel = machine_routing_rel(&machine_scope);
    let state_rel = machine_state_rel(&machine_scope);
    let winccoa_note = if profile.include_winccoa {
        "
- Treat WinCC OA runtime access, datapoints, panels, alarms, and manager operations as safety-sensitive."
    } else {
        ""
    };

    format!(
        r#"{LAB_PACK_START}
## Harkonnen Labrador Pack

This repo includes project-level Claude subagents under `.claude/agents/`.

Core coordination data lives under `.harkonnen` so the project can also be coordinated by Codex, Gemini, OpenClaw, or another runner on machines that support them.

Use the Labradors proactively:
- Coobie first to retrieve prior lessons, emit a preflight briefing, and surface required checks
- Scout after Coobie's briefing for spec shaping and ambiguity review
- Mason for implementation constrained by Coobie's guardrails
- Piper for tools, docs, and helpers
- Bramble for visible validation against Coobie's required checks
- Sable for acceptance review without writing code
- Ash for twin/simulation design driven by Coobie's environment risks
- Flint for evidence packaging
- Keeper for safety and boundary review

Primary context files:
- `.harkonnen/project-context.md`
- `.harkonnen/context/engine-routing.md`
- `{engine_rel}`
- `{state_rel}`
- `.harkonnen/project-scan.md`
- `.harkonnen/launch-guide.md`
- `.harkonnen/spec-template.yaml`
- `.harkonnen/context/system-manifest.yaml`
{winccoa_note}
{LAB_PACK_END}
"#,
        engine_rel = engine_rel,
        state_rel = state_rel,
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
        Value::Array(server.args.iter().cloned().map(Value::String).collect()),
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

fn current_machine_scope(paths: &Paths) -> String {
    machine_scope_id(paths, &SystemDiscovery::discover())
}

fn machine_scope_id(paths: &Paths, discovery: &SystemDiscovery) -> String {
    let raw = paths
        .setup
        .machine
        .as_ref()
        .map(|machine| machine.name.clone())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| discovery.hostname.clone())
        .or_else(|| discovery.username.clone())
        .unwrap_or_else(|| format!("{}-{}", paths.setup.setup.name, discovery.platform));
    let slug = slugify_machine_name(&raw);
    if slug.is_empty() {
        "current-machine".to_string()
    } else {
        slug
    }
}

fn machine_routing_rel(machine_scope: &str) -> String {
    format!(".harkonnen/machines/{machine_scope}/engine-routing.yaml")
}

fn machine_state_rel(machine_scope: &str) -> String {
    format!(".harkonnen/machines/{machine_scope}/engine-state.yaml")
}

fn build_engine_routing_snapshot(
    paths: &Paths,
    profiles: &std::collections::HashMap<String, AgentProfile>,
    agent_names: &[String],
) -> EngineRoutingSnapshot {
    let discovery = SystemDiscovery::discover();
    let machine_scope = machine_scope_id(paths, &discovery);
    let default_provider = paths.setup.providers.default.clone();
    let coordinator_surface = paths
        .setup
        .resolve_provider(&default_provider)
        .and_then(|config| config.surface.clone())
        .unwrap_or_else(|| "unassigned".to_string());

    let mut providers = Vec::new();
    for (name, config) in [
        ("claude", paths.setup.providers.claude.as_ref()),
        ("gemini", paths.setup.providers.gemini.as_ref()),
        ("codex", paths.setup.providers.codex.as_ref()),
    ] {
        if let Some(config) = config {
            providers.push(EngineProviderSnapshot {
                name: name.to_string(),
                provider_type: config.provider_type.clone(),
                model: config.model.clone(),
                surface: config
                    .surface
                    .clone()
                    .unwrap_or_else(|| "unassigned".to_string()),
                enabled: config.enabled,
                usage_rights: config.usage_rights.clone(),
                api_key_env: config.api_key_env.clone(),
                credential_state: if std::env::var(&config.api_key_env).is_ok() {
                    "env_present".to_string()
                } else {
                    "env_missing_or_external".to_string()
                },
                operational_state: if config.enabled {
                    "ready".to_string()
                } else {
                    "disabled".to_string()
                },
            });
        }
    }
    providers.sort_by(|left, right| left.name.cmp(&right.name));

    let mut assignments = Vec::new();
    for agent_name in agent_names {
        if let Some(profile) = profiles.get(agent_name) {
            let provider_name = paths
                .setup
                .resolve_agent_provider_name(&profile.name, &profile.provider);
            let resolved = paths
                .setup
                .resolve_agent_provider(&profile.name, &profile.provider);
            let source = if paths
                .setup
                .routing
                .as_ref()
                .and_then(|routing| routing.agents.get(&profile.name))
                .is_some()
            {
                "setup routing override".to_string()
            } else if profile.provider == "default" {
                "setup default provider".to_string()
            } else {
                "agent profile provider preference".to_string()
            };
            assignments.push(AgentEngineAssignment {
                agent: profile.name.clone(),
                role: profile.role.clone(),
                provider: provider_name,
                model: resolved
                    .map(|config| config.model.clone())
                    .unwrap_or_else(|| "unresolved".to_string()),
                surface: resolved
                    .and_then(|config| config.surface.clone())
                    .unwrap_or_else(|| "unassigned".to_string()),
                source,
            });
        }
    }

    let mut notes = vec![
        "Assignments are project-scoped and machine-scoped.".to_string(),
        "Treat the generated engine-routing.yaml as the base plan for this machine.".to_string(),
        "Treat engine-state.yaml as the live override layer when quotas, coordinator choice, or local tooling availability change.".to_string(),
    ];
    if paths.setup.setup.openclaw.unwrap_or(false) {
        notes.push(
            "OpenClaw is enabled in the setup and may orchestrate cross-engine handoffs when installed on this machine.".to_string(),
        );
    }

    EngineRoutingSnapshot {
        machine_scope,
        setup_name: paths.setup.setup.name.clone(),
        platform: paths.setup.setup.platform.clone(),
        default_provider: default_provider.clone(),
        coordinator: EngineCoordinator {
            provider: default_provider,
            surface: coordinator_surface,
            source: "setup providers.default".to_string(),
        },
        openclaw_enabled: paths.setup.setup.openclaw.unwrap_or(false),
        openclaw_available: discovery.openclaw,
        providers,
        assignments,
        notes,
    }
}

fn build_engine_routing_doc(routing: &EngineRoutingSnapshot) -> String {
    let engine_rel = machine_routing_rel(&routing.machine_scope);
    let state_rel = machine_state_rel(&routing.machine_scope);
    let mut out = format!(
        r#"# Engine Routing

This guidance is coordinator-agnostic. Any engine that coordinates the Labradors for this project should read the base routing snapshot at `{}` and then apply live overrides from `{}`.

Current machine scope:
- `{}`
- setup: `{}`
- platform: `{}`
- default coordinator: `{}` via `{}`
- openclaw enabled in setup: `{}`
- openclaw detected on this machine: `{}`

Routing rules:
- assignments are per project and per machine
- the coordinator can be Claude, Codex, Gemini, OpenClaw, or another local or remote runner
- when quotas or tooling change, update `{}` instead of rewriting agent prompts
- if `routing_overrides` exists in `{}`, treat it as higher priority than the base snapshot
"#,
        engine_rel,
        state_rel,
        routing.machine_scope,
        routing.setup_name,
        routing.platform,
        routing.coordinator.provider,
        routing.coordinator.surface,
        routing.openclaw_enabled,
        routing.openclaw_available,
        state_rel,
        state_rel,
    );

    if !routing.providers.is_empty() {
        out.push_str(
            r#"
Configured providers:
"#,
        );
        for provider in &routing.providers {
            out.push_str(&format!(
                "- `{}` -> model `{}`, surface `{}`, enabled={}, credential_state={}, operational_state={}
",
                provider.name,
                provider.model,
                provider.surface,
                provider.enabled,
                provider.credential_state,
                provider.operational_state,
            ));
        }
    }

    if !routing.assignments.is_empty() {
        out.push_str(
            r#"
Base agent assignments:
"#,
        );
        for assignment in &routing.assignments {
            out.push_str(&format!(
                "- `{}` -> `{}` via `{}` ({})
",
                assignment.agent, assignment.provider, assignment.surface, assignment.source,
            ));
        }
    }

    if !routing.notes.is_empty() {
        out.push_str(
            r#"
Notes:
"#,
        );
        for note in &routing.notes {
            out.push_str(&format!(
                "- {note}
"
            ));
        }
    }

    out
}

fn build_engine_state_template(routing: &EngineRoutingSnapshot) -> String {
    let mut out = format!(
        "# Live machine-scoped overrides for this project.
# Coordinators of any engine should merge this file over engine-routing.yaml.
# Edit this file when quotas, coordinator choice, or local tool availability change.

machine_scope: {}
updated_at: {}
coordinator_override: null
provider_state:
",
        routing.machine_scope,
        Utc::now().to_rfc3339(),
    );

    for provider in &routing.providers {
        out.push_str(&format!(
            "  {}:
    operational_state: {}
    notes: []
",
            provider.name, provider.operational_state,
        ));
    }

    out.push_str(
        "routing_overrides: {}
notes:
  - Update operational_state when quotas or availability change.
  - Valid operational_state values include ready, quota_exhausted, rate_limited, offline, disabled, and review.
  - Use routing_overrides to temporarily reassign agents on this machine for this project.
",
    );

    out
}

fn scan_target_project(target_root: &Path) -> Result<ProjectScan> {
    let mut roots = vec![String::from(".")];
    let mut stack_signals = Vec::new();
    let mut read_first_files = Vec::new();
    let mut launch_commands = Vec::new();
    let mut validation_commands = Vec::new();

    for rel in discover_paths(
        target_root,
        3,
        &[
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "go.mod",
            "docker-compose.yml",
            "compose.yml",
        ],
    )? {
        if let Some(parent) = Path::new(&rel).parent() {
            let parent_str = normalize_rel(parent);
            match Path::new(&rel).file_name().and_then(|value| value.to_str()) {
                Some("Cargo.toml") => {
                    if has_ancestor_manifest(target_root, &parent_str, "Cargo.toml") {
                        continue;
                    }
                    push_unique_string(&mut roots, parent_str.clone());
                    push_unique_string(
                        &mut stack_signals,
                        format!("Rust workspace or crate at `{parent_str}`"),
                    );
                    push_unique_string(
                        &mut validation_commands,
                        scoped_command(&parent_str, "cargo test -q"),
                    );
                }
                Some("package.json") => {
                    if has_ancestor_manifest(target_root, &parent_str, "package.json") {
                        continue;
                    }
                    push_unique_string(&mut roots, parent_str.clone());
                    push_unique_string(
                        &mut stack_signals,
                        format!("Node/Svelte/Vite package at `{parent_str}`"),
                    );
                    push_unique_string(
                        &mut validation_commands,
                        scoped_command(&parent_str, "npm run build"),
                    );
                }
                Some("pyproject.toml") => {
                    if has_ancestor_manifest(target_root, &parent_str, "pyproject.toml") {
                        continue;
                    }
                    push_unique_string(&mut roots, parent_str.clone());
                    push_unique_string(
                        &mut stack_signals,
                        format!("Python project at `{parent_str}`"),
                    );
                }
                Some("go.mod") => {
                    if has_ancestor_manifest(target_root, &parent_str, "go.mod") {
                        continue;
                    }
                    push_unique_string(&mut roots, parent_str.clone());
                    push_unique_string(&mut stack_signals, format!("Go module at `{parent_str}`"));
                }
                Some("docker-compose.yml") | Some("compose.yml") => {
                    if has_ancestor_manifest(target_root, &parent_str, "docker-compose.yml")
                        || has_ancestor_manifest(target_root, &parent_str, "compose.yml")
                    {
                        continue;
                    }
                    push_unique_string(&mut roots, parent_str.clone());
                    push_unique_string(
                        &mut stack_signals,
                        format!("Docker Compose runtime at `{parent_str}`"),
                    );
                    push_unique_string(
                        &mut launch_commands,
                        scoped_command(&parent_str, "docker compose up"),
                    );
                }
                _ => {}
            }
        }
    }
    let candidate_docs = [
        "README.md",
        "codex.md",
        "AGENTS.md",
        "CLAUDE.md",
        "docs/architecture.md",
        "docs/workflow-blueprint.md",
        "docs/requirements-alignment.md",
        "docs/E2E-Lamdet-Test.md",
        "functional-spec/README.md",
    ];

    let roots_snapshot = roots.clone();
    for root in &roots_snapshot {
        for candidate in candidate_docs {
            let rel = join_rel(root, candidate);
            if target_root.join(&rel).exists() {
                push_unique_string(&mut read_first_files, rel);
            }
        }

        let launch_script = join_rel(root, "scripts/launch.sh");
        if target_root.join(&launch_script).exists() {
            push_unique_string(
                &mut launch_commands,
                scoped_command(root, "./scripts/launch.sh up"),
            );
        }
    }

    roots.sort();
    stack_signals.sort();
    read_first_files.sort();
    launch_commands.sort();
    validation_commands.sort();

    Ok(ProjectScan {
        detected_roots: roots,
        read_first_files,
        launch_commands,
        validation_commands,
        stack_signals,
    })
}

fn discover_paths(
    target_root: &Path,
    max_depth: usize,
    file_names: &[&str],
) -> Result<Vec<String>> {
    let mut found = Vec::new();
    scan_dir(
        target_root,
        target_root,
        0,
        max_depth,
        file_names,
        &mut found,
    )?;
    found.sort();
    Ok(found)
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    file_names: &[&str],
    found: &mut Vec<String>,
) -> Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if matches!(
                name.as_ref(),
                ".git"
                    | "node_modules"
                    | "target"
                    | ".harkonnen"
                    | ".claude"
                    | ".svelte-kit"
                    | "dist"
            ) {
                continue;
            }
            scan_dir(root, &path, depth + 1, max_depth, file_names, found)?;
            continue;
        }

        if file_names
            .iter()
            .any(|candidate| *candidate == name.as_ref())
        {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            push_unique_string(found, rel);
        }
    }

    Ok(())
}

fn scoped_command(root: &str, command: &str) -> String {
    if root == "." {
        command.to_string()
    } else {
        format!("(cd {root} && {command})")
    }
}

fn join_rel(root: &str, child: &str) -> String {
    if root == "." {
        child.to_string()
    } else {
        format!("{root}/{child}")
    }
}

fn normalize_rel(path: &Path) -> String {
    let rel = path.to_string_lossy().replace('\\', "/");
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel
    }
}

fn has_ancestor_manifest(target_root: &Path, rel_dir: &str, manifest_name: &str) -> bool {
    if rel_dir == "." {
        return false;
    }

    let mut current = Path::new(rel_dir).parent();
    while let Some(parent) = current {
        let rel = normalize_rel(parent);
        if target_root.join(&rel).join(manifest_name).exists() {
            return true;
        }
        current = parent.parent();
    }

    false
}

fn push_unique_string(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
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
        let existing =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
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

pub(crate) fn write_text_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_text_file_if_missing(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_text_file(path, content)
}

pub(crate) fn copy_if_exists(from: &Path, to: &Path) -> Result<()> {
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
    fn engine_state_template_mentions_override_controls() {
        let snapshot = EngineRoutingSnapshot {
            machine_scope: "test-machine".to_string(),
            providers: vec![EngineProviderSnapshot {
                name: "claude".to_string(),
                operational_state: "ready".to_string(),
                ..EngineProviderSnapshot::default()
            }],
            ..EngineRoutingSnapshot::default()
        };

        let rendered = build_engine_state_template(&snapshot);
        assert!(rendered.contains("test-machine"));
        assert!(rendered.contains("routing_overrides"));
        assert!(rendered.contains("quota_exhausted"));
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
