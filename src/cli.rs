use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::benchmark;
use crate::capacity::CapacityState;
use crate::claude_pack::{export_claude_pack, ClaudePackRequest};
use crate::config::Paths;
use crate::models::EvidenceAnnotationBundle;
use crate::orchestrator::{AppContext, FailureHarness, RunRequest};
use crate::reporting;
use crate::setup::{
    available_template_names, command_available, compose_setup_id, default_provider_config,
    slugify_machine_name, MachineConfig, McpConfig, McpServerConfig, ProviderConfig, RoutingConfig,
    SetupConfig, SystemDiscovery,
};

#[derive(Parser, Debug)]
#[command(name = "harkonnen")]
#[command(about = "Harkonnen Labs AI Software Factory")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Spec {
        #[command(subcommand)]
        command: SpecCommands,
    },
    Run {
        #[command(subcommand)]
        command: RunCommands,
    },
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommands,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommands,
    },
    Setup {
        #[command(subcommand)]
        command: SetupCommands,
    },
    Soul {
        #[command(subcommand)]
        command: SoulCommands,
    },
    Serve(ServeArgs),
    Capacity {
        #[command(subcommand)]
        command: CapacityCommands,
    },
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum CapacityCommands {
    /// Show current provider capacity state.
    Show,
    /// Set a provider's capacity status.
    Set(CapacitySetArgs),
    /// Reassign all claims from unavailable providers to the best available alternative.
    Reassign,
}

#[derive(Subcommand, Debug)]
pub enum BenchmarkCommands {
    List(BenchmarkListArgs),
    Run(BenchmarkRunArgs),
    Report(BenchmarkReportArgs),
}

#[derive(Args, Debug)]
pub struct BenchmarkListArgs {
    #[arg(long)]
    pub manifest: Option<String>,
}

#[derive(Args, Debug)]
pub struct BenchmarkRunArgs {
    #[arg(long, value_delimiter = ',')]
    pub suite: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub all: bool,
    #[arg(long)]
    pub manifest: Option<String>,
    #[arg(long)]
    pub output: Option<String>,
    #[arg(long, default_value_t = false)]
    pub strict: bool,
    #[arg(long, default_value_t = false)]
    pub strict_availability: bool,
}

#[derive(Args, Debug)]
pub struct BenchmarkReportArgs {
    pub file: String,
}

#[derive(Subcommand, Debug)]
pub enum SpecCommands {
    Validate(SpecValidateArgs),
}

#[derive(Args, Debug)]
pub struct SpecValidateArgs {
    pub file: String,
}

#[derive(Subcommand, Debug)]
pub enum RunCommands {
    Start(RunStartArgs),
    Harness(RunHarnessArgs),
    Status(RunStatusArgs),
    Report(RunReportArgs),
}

#[derive(Args, Debug)]
pub struct RunStartArgs {
    pub spec: String,
    #[arg(long, conflicts_with = "product_path")]
    pub product: Option<String>,
    #[arg(long, conflicts_with = "product")]
    pub product_path: Option<String>,
}

#[derive(Args, Debug)]
pub struct RunHarnessArgs {
    pub spec: String,
    #[arg(long, conflicts_with = "product_path")]
    pub product: Option<String>,
    #[arg(long, conflicts_with = "product")]
    pub product_path: Option<String>,
    #[arg(long, value_parser = ["validation", "hidden_scenarios"])]
    pub phase: String,
    #[arg(long, default_value_t = 4)]
    pub times: usize,
    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Args, Debug)]
pub struct RunStatusArgs {
    pub run_id: String,
}

#[derive(Args, Debug)]
pub struct RunReportArgs {
    pub run_id: String,
}

#[derive(Subcommand, Debug)]
pub enum ArtifactCommands {
    Package(ArtifactPackageArgs),
}

#[derive(Args, Debug)]
pub struct ArtifactPackageArgs {
    pub run_id: String,
}

#[derive(Subcommand, Debug)]
pub enum MemoryCommands {
    Index,
    Init,
    Search(MemorySearchArgs),
    Import(MemoryImportArgs),
    Ingest(MemoryIngestArgs),
}

#[derive(Subcommand, Debug)]
pub enum EvidenceCommands {
    Init(EvidenceInitArgs),
    Validate(EvidenceValidateArgs),
    Promote(EvidencePromoteArgs),
}

#[derive(Args, Debug)]
pub struct MemorySearchArgs {
    pub query: String,
}

#[derive(Args, Debug)]
pub struct MemoryImportArgs {
    pub file: String,
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
}

#[derive(Args, Debug)]
pub struct MemoryIngestArgs {
    pub source: String,
    #[arg(long, default_value = "core", value_parser = ["core", "project"])]
    pub scope: String,
    #[arg(long)]
    pub project_root: Option<String>,
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub no_asset: bool,
}

#[derive(Args, Debug)]
pub struct EvidenceInitArgs {
    #[arg(long)]
    pub project_root: String,
}

#[derive(Args, Debug)]
pub struct EvidenceValidateArgs {
    pub file: String,
}

#[derive(Args, Debug)]
pub struct EvidencePromoteArgs {
    pub file: String,
    #[arg(long, default_value = "follow-bundle", value_parser = ["follow-bundle", "project", "core"])]
    pub scope: String,
    #[arg(long)]
    pub project_root: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum SetupCommands {
    Check,
    Init(SetupInitArgs),
    ClaudePack(SetupClaudePackArgs),
}

#[derive(Subcommand, Debug)]
pub enum SoulCommands {
    Bootstrap(SoulBootstrapArgs),
    Show(SoulShowArgs),
}

#[derive(Args, Debug)]
pub struct SetupInitArgs {
    #[arg(long)]
    pub machine_name: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub organization: Option<String>,
    #[arg(long)]
    pub template: Option<String>,
    #[arg(long)]
    pub write: Option<String>,
    #[arg(long, default_value_t = false)]
    pub force: bool,
    #[arg(long, default_value_t = false)]
    pub non_interactive: bool,
}

#[derive(Args, Debug)]
pub struct SetupClaudePackArgs {
    #[arg(long)]
    pub target_path: String,
    #[arg(long)]
    pub project_name: Option<String>,
    #[arg(long)]
    pub project_slug: Option<String>,
    #[arg(long, default_value = "generic")]
    pub project_type: String,
    #[arg(long)]
    pub domain: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub constraints: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub winccoa: bool,
    #[arg(long, default_value_t = false)]
    pub no_settings: bool,
}

#[derive(Args, Debug)]
pub struct SoulBootstrapArgs {
    #[arg(long, default_value = "coobie")]
    pub self_name: String,
    #[arg(long)]
    pub output_root: Option<String>,
}

#[derive(Args, Debug)]
pub struct SoulShowArgs {
    #[arg(long, default_value = "coobie")]
    pub self_name: String,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
}

#[derive(Args, Debug)]
pub struct CapacitySetArgs {
    /// Provider name: claude, codex, or gemini.
    pub provider: String,
    /// Status: ok, near_limit, or at_limit.
    pub status: String,
    /// Optional human-readable note explaining the change.
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentProfileSummary {
    name: String,
    provider: String,
}

#[derive(Clone)]
struct McpInterviewTemplate {
    name: &'static str,
    prompt: &'static str,
    command: &'static str,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    aliases: &'static [&'static str],
    default_enabled: bool,
    customizable: bool,
}

impl McpInterviewTemplate {
    fn default_server(&self) -> McpServerConfig {
        let env = if self.env.is_empty() {
            None
        } else {
            Some(
                self.env
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            )
        };
        let aliases = if self.aliases.is_empty() {
            None
        } else {
            Some(self.aliases.iter().map(|value| value.to_string()).collect())
        };

        McpServerConfig {
            name: self.name.to_string(),
            command: self.command.to_string(),
            args: self.args.iter().map(|value| value.to_string()).collect(),
            env,
            tool_aliases: aliases,
        }
    }
}

pub async fn handle_benchmark(command: BenchmarkCommands, paths: &Paths) -> Result<()> {
    match command {
        BenchmarkCommands::List(args) => {
            let manifest_path = args
                .manifest
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| benchmark::default_manifest_path(paths));
            let manifest = benchmark::load_manifest(&manifest_path)?;
            println!("{}", benchmark::render_manifest_overview(&manifest));
        }
        BenchmarkCommands::Run(args) => {
            let manifest_path = args
                .manifest
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| benchmark::default_manifest_path(paths));
            let output_path = args.output.as_deref().map(PathBuf::from);
            let output = benchmark::run_benchmarks(
                paths,
                &manifest_path,
                &args.suite,
                args.all,
                output_path.as_deref(),
            )
            .await?;
            println!("Benchmark JSON: {}", output.json_path.display());
            println!("Benchmark Markdown: {}", output.markdown_path.display());
            println!(
                "Summary: total={} passed={} failed={} skipped={}",
                output.report.summary.total,
                output.report.summary.passed,
                output.report.summary.failed,
                output.report.summary.skipped,
            );
            if args.strict && output.report.summary.failed > 0 {
                bail!(
                    "benchmark run recorded {} failed suite(s)",
                    output.report.summary.failed
                );
            }
            if args.strict_availability && output.report.summary.skipped > 0 {
                bail!(
                    "benchmark run recorded {} skipped suite(s)",
                    output.report.summary.skipped
                );
            }
        }
        BenchmarkCommands::Report(args) => {
            let path = PathBuf::from(args.file);
            let report = benchmark::load_run_report(&path)?;
            println!("{}", benchmark::render_report_markdown(&report));
        }
    }
    Ok(())
}

pub async fn handle_spec(command: SpecCommands) -> Result<()> {
    match command {
        SpecCommands::Validate(args) => {
            let spec = crate::spec::load_spec(&args.file)?;
            println!("Spec valid: {} ({})", spec.title, spec.id);
        }
    }
    Ok(())
}

pub async fn handle_serve(args: ServeArgs, app: AppContext) -> Result<()> {
    crate::api::start_api_server(app, args.port).await?;
    Ok(())
}

pub async fn handle_run(command: RunCommands, app: AppContext) -> Result<()> {
    match command {
        RunCommands::Start(args) => {
            let req = build_run_request(args.spec, args.product, args.product_path, None)?;
            let run = app.start_run(req).await?;
            println!("Run {} finished with status: {}", run.run_id, run.status);
        }
        RunCommands::Harness(args) => {
            let message = args
                .message
                .unwrap_or_else(|| format!("Harness injected failure in {}", args.phase));
            let mut run_ids = Vec::new();
            for index in 0..args.times {
                let req = build_run_request(
                    args.spec.clone(),
                    args.product.clone(),
                    args.product_path.clone(),
                    Some(FailureHarness {
                        phase: args.phase.clone(),
                        message: message.clone(),
                    }),
                )?;
                let run = app.start_run(req).await?;
                println!(
                    "Harness run {}/{} -> {} ({})",
                    index + 1,
                    args.times,
                    run.run_id,
                    run.status
                );
                run_ids.push(run.run_id);
            }
            println!(
                "Harness complete for phase '{}'. Run IDs: {}",
                args.phase,
                run_ids.join(", ")
            );
        }
        RunCommands::Status(args) => {
            let run = app
                .get_run(&args.run_id)
                .await?
                .with_context(|| format!("run not found: {}", args.run_id))?;
            println!("Run {} status: {}", run.run_id, run.status);
        }
        RunCommands::Report(args) => {
            let report = reporting::build_report(&app, &args.run_id).await?;
            println!("{report}");
        }
    }
    Ok(())
}

fn build_run_request(
    spec_path: String,
    product: Option<String>,
    product_path: Option<String>,
    failure_harness: Option<FailureHarness>,
) -> Result<RunRequest> {
    if product.is_none() && product_path.is_none() {
        bail!("provide either --product <name> or --product-path <path>");
    }

    Ok(RunRequest {
        spec_path,
        product,
        product_path,
        run_hidden_scenarios: true,
        failure_harness,
    })
}

pub async fn handle_artifact(command: ArtifactCommands, app: AppContext) -> Result<()> {
    match command {
        ArtifactCommands::Package(args) => {
            let path = app.package_artifacts(&args.run_id).await?;
            println!("Artifact bundle: {}", path.display());
        }
    }
    Ok(())
}

pub async fn handle_memory(command: MemoryCommands, app: AppContext) -> Result<()> {
    match command {
        MemoryCommands::Index => {
            app.memory_store.reindex().await?;
            println!("Memory reindexed");
        }
        MemoryCommands::Init => {
            app.memory_store.init(&app.paths.setup).await?;

            // Pre-embed all memory entries so semantic search is ready immediately
            // on the next run. The model downloads once (~33MB) on first call.
            match &app.embedding_store {
                Some(es) => {
                    let entries = app.memory_store.list_entries().await?;
                    let root = app.memory_store.root.display().to_string();
                    let count = entries.len();
                    print!("Embedding {count} memory entries for semantic search...");
                    match es.ensure_embedded(&entries, &root).await {
                        Ok(()) => println!(" done."),
                        Err(e) => println!("\nEmbedding failed (will retry on next run): {e}"),
                    }
                }
                None => {
                    println!(
                        "Semantic embedding unavailable — memory init complete without embeddings."
                    );
                    println!("Embeddings will be generated automatically on the next `cargo run`.");
                }
            }
        }
        MemoryCommands::Search(args) => {
            let hits = app.memory_store.retrieve_context(&args.query).await?;
            for hit in hits {
                println!("{hit}\n");
            }
        }
        MemoryCommands::Import(args) => {
            let note_path = app
                .memory_store
                .import_asset(
                    Path::new(&args.file),
                    args.id.as_deref(),
                    args.tags,
                    args.summary.as_deref(),
                    args.notes.as_deref(),
                )
                .await?;
            println!("Imported memory note: {}", note_path.display());
        }
        MemoryCommands::Ingest(args) => {
            let result = app
                .ingest_memory_source(
                    &args.source,
                    &args.scope,
                    args.project_root.as_deref(),
                    args.id.as_deref(),
                    args.summary.as_deref(),
                    args.notes.as_deref(),
                    args.tags,
                    !args.no_asset,
                )
                .await?;
            println!("Ingested memory note: {}", result.note_path.display());
            if let Some(asset_path) = result.asset_path {
                println!("Stored source asset: {}", asset_path.display());
            }
            println!("Extracted title: {}", result.title);
            println!("Extracted chars: {}", result.extracted_chars);
            println!("Memory root: {}", result.memory_root.display());
        }
    }
    Ok(())
}

pub async fn handle_evidence(command: EvidenceCommands, app: AppContext) -> Result<()> {
    match command {
        EvidenceCommands::Init(args) => {
            let evidence_root = app
                .init_project_evidence(Path::new(&args.project_root))
                .await?;
            println!("Project evidence root: {}", evidence_root.display());
            println!(
                "Annotation bundles: {}",
                evidence_root.join("annotations").display()
            );
            println!("Causal records: {}", evidence_root.join("causal").display());
        }
        EvidenceCommands::Validate(args) => {
            let bundle = load_evidence_bundle(Path::new(&args.file))?;
            validate_evidence_bundle(&bundle)?;
            println!(
                "Evidence bundle valid: project='{}' scenario='{}' sources={} annotations={}",
                bundle.project,
                bundle.scenario,
                bundle.sources.len(),
                bundle.annotations.len()
            );
        }
        EvidenceCommands::Promote(args) => {
            let bundle = load_evidence_bundle(Path::new(&args.file))?;
            validate_evidence_bundle(&bundle)?;
            let result = app
                .promote_evidence_bundle(
                    Path::new(&args.file),
                    &bundle,
                    &args.scope,
                    args.project_root.as_deref(),
                )
                .await?;
            println!(
                "Promoted {} annotation(s); skipped {}.",
                result.promoted_ids.len(),
                result.skipped_annotations.len()
            );
            for id in result.promoted_ids {
                println!("Promoted memory: {}", id);
            }
            for skipped in result.skipped_annotations {
                println!("Skipped annotation: {}", skipped);
            }
        }
    }
    Ok(())
}

fn load_evidence_bundle(path: &Path) -> Result<EvidenceAnnotationBundle> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read evidence bundle: {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext == "json" {
        return serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse evidence json: {}", path.display()));
    }
    serde_yaml::from_str(&raw)
        .or_else(|_| serde_json::from_str(&raw))
        .with_context(|| format!("failed to parse evidence bundle: {}", path.display()))
}

fn validate_evidence_bundle(bundle: &EvidenceAnnotationBundle) -> Result<()> {
    if bundle.schema_version == 0 {
        bail!("schema_version must be >= 1");
    }
    let source_ids = bundle
        .sources
        .iter()
        .map(|source| source.source_id.trim())
        .collect::<HashSet<_>>();
    for annotation in &bundle.annotations {
        if annotation.annotation_id.trim().is_empty() {
            bail!("annotation_id cannot be empty");
        }
        let anchor_ids = annotation
            .anchors
            .iter()
            .map(|anchor| anchor.anchor_id.as_str())
            .collect::<HashSet<_>>();
        for source_id in &annotation.source_ids {
            if !source_ids.contains(source_id.trim()) {
                bail!(
                    "annotation '{}' references unknown source '{}'",
                    annotation.annotation_id,
                    source_id
                );
            }
        }
        for anchor in &annotation.anchors {
            if anchor.anchor_id.trim().is_empty() {
                bail!(
                    "annotation '{}' has anchor with empty anchor_id",
                    annotation.annotation_id
                );
            }
            if !source_ids.contains(anchor.source_id.trim()) {
                bail!(
                    "annotation '{}' anchor '{}' references unknown source '{}'",
                    annotation.annotation_id,
                    anchor.anchor_id,
                    anchor.source_id
                );
            }
        }
        for claim in &annotation.claims {
            if claim.claim_id.trim().is_empty() {
                bail!(
                    "annotation '{}' has claim with empty claim_id",
                    annotation.annotation_id
                );
            }
            for anchor_id in &claim.evidence_anchor_ids {
                if !anchor_ids.contains(anchor_id.as_str()) {
                    bail!(
                        "annotation '{}' claim '{}' references unknown anchor '{}'",
                        annotation.annotation_id,
                        claim.claim_id,
                        anchor_id
                    );
                }
            }
        }
    }
    Ok(())
}

pub async fn handle_setup(command: SetupCommands, paths: &Paths) -> Result<()> {
    match command {
        SetupCommands::Check => handle_setup_check(paths),
        SetupCommands::Init(args) => handle_setup_init(paths, args),
        SetupCommands::ClaudePack(args) => handle_setup_claude_pack(paths, args),
    }
}

pub async fn handle_soul(command: SoulCommands, paths: &Paths) -> Result<()> {
    match command {
        SoulCommands::Bootstrap(args) => {
            crate::calvin_archive::require_coobie(&args.self_name)?;
            let output = crate::calvin_archive::bootstrap_coobie(paths, args.output_root.as_deref())?;
            println!("Calvin Archive root: {}", output.root.display());
            println!("TypeDB schema: {}", output.schema_path.display());
            println!("TypeDB seed: {}", output.seed_path.display());
            println!(
                "Identity projection: {}",
                output.identity_json_path.display()
            );
            println!("Soul guide: {}", output.guide_path.display());
            println!("TypeDB scaffold script: scripts/bootstrap-calvin-archive-typedb.sh");
        }
        SoulCommands::Show(args) => {
            crate::calvin_archive::require_coobie(&args.self_name)?;
            let identity = crate::calvin_archive::coobie_identity();
            if args.json {
                println!("{}", serde_json::to_string_pretty(&identity)?);
            } else {
                println!("{}", crate::calvin_archive::render_identity_markdown(&identity));
            }
        }
    }
    Ok(())
}

fn handle_setup_check(paths: &Paths) -> Result<()> {
    let s = &paths.setup;
    println!("Setup:       {}", s.setup.name);
    if let Some(template) = &s.setup.template {
        println!("Template:    {}", template);
    }
    if let Some(role) = &s.setup.role {
        println!("Role:        {}", role);
    }
    if let Some(organization) = &s.setup.organization {
        println!("Org:         {}", organization);
    }
    println!("Platform:    {}", s.setup.platform);
    println!("AnythingLLM: {}", s.setup.anythingllm.unwrap_or(false));
    println!("OpenClaw:    {}", s.setup.openclaw.unwrap_or(false));
    if let Some(machine) = &s.machine {
        println!("Machine:     {}", machine.name);
        if let Some(hostname) = &machine.hostname {
            println!("Hostname:    {}", hostname);
        }
        if let Some(username) = &machine.username {
            println!("User:        {}", username);
        }
        if let Some(fingerprint) = &machine.fingerprint {
            println!("Arch:        {}", fingerprint.arch);
            println!(
                "Fingerprint: git={}, cargo={}, node={}, npm={}, docker={}, podman={}, openclaw={}",
                bool_tag(fingerprint.git),
                bool_tag(fingerprint.cargo),
                bool_tag(fingerprint.node),
                bool_tag(fingerprint.npm),
                bool_tag(fingerprint.docker),
                bool_tag(fingerprint.podman),
                bool_tag(fingerprint.openclaw),
            );
        }
    }
    println!();

    println!("Providers (default: {}):", s.providers.default);
    print_provider_status("claude", s.providers.claude.as_ref());
    print_provider_status("gemini", s.providers.gemini.as_ref());
    print_provider_status("codex", s.providers.codex.as_ref());
    let mut extra_names: Vec<&String> = s.providers.extras.keys().collect();
    extra_names.sort();
    for name in extra_names {
        print_provider_status(name, s.providers.extras.get(name));
    }

    print_agent_routing(paths, s)?;

    if let Some(mcp) = &s.mcp {
        println!();
        println!("MCP Servers:");
        for server in &mcp.servers {
            let found = command_available(&server.command);
            let tag = if found { "ok   " } else { "MISSING" };
            let aliases = server
                .tool_aliases
                .as_deref()
                .map(|a| a.join(", "))
                .unwrap_or_default();
            println!(
                "  [{}] {}  ({} {}...){}",
                tag,
                server.name,
                server.command,
                server.args.first().map(String::as_str).unwrap_or(""),
                if aliases.is_empty() {
                    String::new()
                } else {
                    format!("  aliases: {aliases}")
                }
            );
        }
    } else {
        println!();
        println!("MCP Servers: none configured");
    }
    Ok(())
}

fn handle_setup_init(paths: &Paths, args: SetupInitArgs) -> Result<()> {
    let interactive =
        !args.non_interactive && io::stdin().is_terminal() && io::stdout().is_terminal();
    let discovery = SystemDiscovery::discover();

    let detected_machine_name = discovery.default_machine_name();
    let machine_name = match args.machine_name {
        Some(name) => slugify_machine_name(&name),
        None if interactive => {
            slugify_machine_name(&prompt_text("Machine name", &detected_machine_name)?)
        }
        None => detected_machine_name,
    };

    let default_role = default_setup_role(args.template.as_deref(), &discovery);
    let setup_role = match args.role {
        Some(role) => slugify_machine_name(&role),
        None if interactive => slugify_machine_name(&prompt_text("Setup role", &default_role)?),
        None => default_role,
    };

    let organization = match args.organization {
        Some(organization) => normalize_optional_slug(&organization),
        None if interactive => prompt_optional_text("Organization or team (optional)", None)?,
        None => {
            normalize_optional_slug(&std::env::var("HARKONNEN_ORGANIZATION").unwrap_or_default())
        }
    };

    let template_names = available_template_names(&paths.root)?;
    let recommended_template = discovery
        .recommended_setup_name_for_role(&setup_role)
        .to_string();
    let template_name = select_template_name(
        &template_names,
        &recommended_template,
        args.template.as_deref(),
        interactive,
    )?;
    let template_path = discovery.recommended_template_path(&paths.root, &template_name);
    let mut config = SetupConfig::from_file(&template_path)?;

    if interactive {
        interview_runtime_features(&mut config, &discovery)?;
        interview_provider_access(&mut config)?;
    }
    normalize_provider_defaults(&mut config)?;
    if interactive {
        select_default_provider(&mut config)?;
    }
    rebalance_agent_routing(&mut config, interactive)?;
    if interactive {
        interview_mcp_servers(&mut config, &discovery)?;
    }
    normalize_mcp_servers(&mut config);

    let setup_id = compose_setup_id(&machine_name, &setup_role, organization.as_deref());
    config.setup.name = setup_id.clone();
    config.setup.template = Some(template_name.clone());
    config.setup.role = Some(setup_role.clone());
    config.setup.organization = organization.clone();
    config.setup.platform = discovery.platform.clone();
    config.machine = Some(MachineConfig {
        name: machine_name.clone(),
        hostname: discovery.hostname.clone(),
        username: discovery.username.clone(),
        generated_at: Some(Utc::now().to_rfc3339()),
        fingerprint: Some(discovery.to_machine_fingerprint()),
    });

    let recommended_write_path = discovery.default_write_path(&paths.root, &setup_id);
    let missing = discovery.missing_required_tools(&template_name);

    println!("Discovered System:");
    println!("  platform: {}", discovery.platform);
    println!("  arch:     {}", discovery.arch);
    println!("  git:      {}", bool_tag(discovery.git));
    println!("  cargo:    {}", bool_tag(discovery.cargo));
    println!("  node:     {}", bool_tag(discovery.node));
    println!("  npm:      {}", bool_tag(discovery.npm));
    println!("  docker:   {}", bool_tag(discovery.docker));
    println!("  podman:   {}", bool_tag(discovery.podman));
    println!("  openclaw: {}", bool_tag(discovery.openclaw));
    if let Some(hostname) = &discovery.hostname {
        println!("  hostname: {}", hostname);
    }
    println!();
    println!("Machine name:          {}", machine_name);
    println!("Setup role:            {}", setup_role);
    if let Some(organization) = &organization {
        println!("Organization:          {}", organization);
    }
    println!("Setup ID:              {}", setup_id);
    println!("Selected base template: {}", template_name);
    if missing.is_empty() {
        println!("Required tools:        all present for this template");
    } else {
        println!("Missing tools:         {}", missing.join(", "));
    }
    println!();
    println!("Effective providers:");
    print_provider_status("claude", config.providers.claude.as_ref());
    print_provider_status("gemini", config.providers.gemini.as_ref());
    print_provider_status("codex", config.providers.codex.as_ref());
    print_setup_mcp_preview(config.mcp.as_ref());

    let toml_text = render_generated_setup(&config)?;
    let target = match args.write {
        Some(path) => Some(resolve_write_target(&paths.root, &path)),
        None if interactive
            && prompt_bool(
                &format!(
                    "Write machine setup now to {}?",
                    relative_display(&paths.root, &recommended_write_path)
                ),
                true,
            )? =>
        {
            Some(recommended_write_path.clone())
        }
        _ => None,
    };

    match target {
        Some(target_path) => {
            if target_path.exists() && !args.force {
                bail!(
                    "target already exists: {} (use --force to overwrite)",
                    target_path.display()
                );
            }
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating parent directory for {}", target_path.display())
                })?;
            }
            std::fs::write(&target_path, toml_text)
                .with_context(|| format!("writing setup file: {}", target_path.display()))?;
            println!();
            println!("Wrote machine setup: {}", target_path.display());
            println!(
                "Activate with: {}",
                activation_hint(&config.setup.platform, &paths.root, &target_path)
            );
            println!("Then run: cargo run -- setup check");
        }
        None => {
            println!();
            println!("No file written.");
            println!(
                "Recommended write path: {}",
                relative_display(&paths.root, &recommended_write_path)
            );
            println!(
                "To instantiate this machine later, run: cargo run -- setup init --machine-name {} --role {}{} --template {} --write {}",
                machine_name,
                setup_role,
                organization
                    .as_ref()
                    .map(|value| format!(" --organization {}", value))
                    .unwrap_or_default(),
                template_name,
                relative_display(&paths.root, &recommended_write_path)
            );
            println!();
            println!("Generated config preview:");
            print_template_preview(&toml_text);
        }
    }

    Ok(())
}

fn handle_setup_claude_pack(paths: &Paths, args: SetupClaudePackArgs) -> Result<()> {
    let summary = export_claude_pack(
        paths,
        ClaudePackRequest {
            target_path: args.target_path,
            project_name: args.project_name,
            project_slug: args.project_slug,
            project_type: args.project_type,
            domain: args.domain,
            summary: args.summary,
            constraints: args.constraints,
            include_winccoa: args.winccoa,
            write_settings: !args.no_settings,
        },
    )?;

    println!("Claude Labrador pack ready for {}", summary.project_name);
    println!("Target:      {}", summary.target_root.display());
    println!("Pack root:   {}", summary.pack_root.display());
    println!("CLAUDE.md:   {}", summary.claude_md_path.display());
    println!("Agents:      {}", summary.agents_written);
    if let Some(settings_path) = summary.settings_path {
        println!("Settings:    {}", settings_path.display());
    } else {
        println!("Settings:    skipped (--no-settings)");
    }
    println!("MCP servers: {}", summary.mcp_servers.join(", "));
    println!();
    println!("Next steps:");
    println!("  1. Open the target repo in Claude Code and restart it if settings changed.");
    println!("  2. Run /agents to confirm the Labradors are available.");
    println!("  3. Ask Scout to draft the first Harkonnen spec for the target project.");
    println!("  4. Use Keeper before any risky WinCC OA or environment-facing action.");
    Ok(())
}

fn default_setup_role(explicit_template: Option<&str>, discovery: &SystemDiscovery) -> String {
    if let Some(template) = explicit_template {
        return match template {
            "ci" => "ci".to_string(),
            "work-windows" => "work".to_string(),
            _ => discovery.default_role_name().to_string(),
        };
    }
    discovery.default_role_name().to_string()
}

fn interview_runtime_features(config: &mut SetupConfig, discovery: &SystemDiscovery) -> Result<()> {
    let openclaw_default = config.setup.openclaw.unwrap_or(false) || discovery.openclaw;
    config.setup.openclaw = Some(prompt_bool(
        "Include OpenClaw orchestration in this setup?",
        openclaw_default,
    )?);

    let anythingllm_default =
        config.setup.anythingllm.unwrap_or(false) && discovery.platform != "windows";
    config.setup.anythingllm = if discovery.platform == "windows" {
        Some(false)
    } else {
        Some(prompt_bool(
            "Include AnythingLLM in this setup?",
            anythingllm_default,
        )?)
    };

    Ok(())
}

fn normalize_optional_slug(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(slugify_machine_name(trimmed))
    }
}

fn prompt_optional_text(label: &str, default: Option<&str>) -> Result<Option<String>> {
    let default = default.unwrap_or("");
    let value = prompt_text(label, default)?;
    Ok(normalize_optional_slug(&value))
}

fn select_template_name(
    template_names: &[String],
    recommended: &str,
    explicit: Option<&str>,
    interactive: bool,
) -> Result<String> {
    if let Some(explicit) = explicit {
        if template_names.iter().any(|name| name == explicit) {
            return Ok(explicit.to_string());
        }
        bail!("unknown template: {explicit}");
    }

    if interactive {
        let options = template_names.join(", ");
        let chosen = prompt_text(&format!("Base template ({options})"), recommended)?;
        if template_names.iter().any(|name| name == &chosen) {
            return Ok(chosen);
        }
        bail!("unknown template: {chosen}");
    }

    Ok(recommended.to_string())
}

fn interview_provider_access(config: &mut SetupConfig) -> Result<()> {
    configure_provider_prompt("claude", &mut config.providers.claude)?;
    configure_provider_prompt("gemini", &mut config.providers.gemini)?;
    configure_provider_prompt("codex", &mut config.providers.codex)?;
    Ok(())
}

fn configure_provider_prompt(name: &str, slot: &mut Option<ProviderConfig>) -> Result<()> {
    let had_provider = slot.is_some();
    let mut provider = slot.take().unwrap_or_else(|| {
        let mut provider = default_provider_config(name);
        provider.enabled = false;
        provider
    });
    let access_default =
        (had_provider && provider.enabled) || std::env::var(&provider.api_key_env).is_ok();
    let has_access = prompt_bool(
        &format!("Do you have access to {name} on this machine?"),
        access_default,
    )?;
    provider.enabled = has_access;
    if has_access {
        provider.model = prompt_text(&format!("Model for {name}"), &provider.model)?;
        let usage_default = provider
            .usage_rights
            .clone()
            .unwrap_or_else(|| "standard".to_string());
        provider.usage_rights = Some(prompt_text(
            &format!("Usage rights for {name}"),
            &usage_default,
        )?);
        let surface_default = provider.surface.clone().unwrap_or_else(|| {
            default_provider_config(name)
                .surface
                .unwrap_or_else(|| "unknown".to_string())
        });
        provider.surface = Some(prompt_text(
            &format!("Preferred surface for {name}"),
            &surface_default,
        )?);
        if matches!(provider.provider_type.as_str(), "openai" | "codex") {
            let base_url_default = provider.base_url.clone().unwrap_or_default();
            let base_url = prompt_text(
                &format!("OpenAI-compatible base URL for {name} (blank for default OpenAI API)"),
                &base_url_default,
            )?;
            provider.base_url = if base_url.trim().is_empty() {
                None
            } else {
                Some(base_url)
            };
        }
    }
    *slot = Some(provider);
    Ok(())
}

fn normalize_provider_defaults(config: &mut SetupConfig) -> Result<()> {
    let enabled = enabled_provider_names(config);
    if enabled.is_empty() {
        bail!("no providers are enabled; enable at least one provider during setup init");
    }
    if enabled.iter().all(|name| name != &config.providers.default) {
        config.providers.default = enabled[0].clone();
    }
    Ok(())
}

fn select_default_provider(config: &mut SetupConfig) -> Result<()> {
    let enabled = enabled_provider_names(config);
    if enabled.len() == 1 {
        config.providers.default = enabled[0].clone();
        return Ok(());
    }

    let current = if enabled.iter().any(|name| name == &config.providers.default) {
        config.providers.default.clone()
    } else {
        enabled[0].clone()
    };
    let chosen = prompt_text(
        &format!("Default provider ({})", enabled.join(", ")),
        &current,
    )?;
    if enabled.iter().any(|name| name == &chosen) {
        config.providers.default = chosen;
        Ok(())
    } else {
        bail!("unknown default provider: {chosen}");
    }
}

fn enabled_provider_names(config: &SetupConfig) -> Vec<String> {
    let mut names = Vec::new();
    if config
        .providers
        .claude
        .as_ref()
        .map(|p| p.enabled)
        .unwrap_or(false)
    {
        names.push("claude".to_string());
    }
    if config
        .providers
        .gemini
        .as_ref()
        .map(|p| p.enabled)
        .unwrap_or(false)
    {
        names.push("gemini".to_string());
    }
    if config
        .providers
        .codex
        .as_ref()
        .map(|p| p.enabled)
        .unwrap_or(false)
    {
        names.push("codex".to_string());
    }
    names
}

fn rebalance_agent_routing(config: &mut SetupConfig, interactive: bool) -> Result<()> {
    let enabled = enabled_provider_names(config);
    if enabled.is_empty() {
        bail!("no providers are enabled; cannot build routing plan");
    }

    let default_provider = config.providers.default.clone();
    let mut routing = config.routing.clone().unwrap_or_default();

    let judgment_default = preferred_provider(config, &["claude", &default_provider]);
    let implementation_default =
        preferred_provider(config, &["codex", "gemini", &default_provider]);
    let memory_default = preferred_provider(config, &["claude", &default_provider]);

    let judgment_provider = if interactive && enabled.len() > 1 {
        select_provider_for_role(
            "Provider for Scout, Sable, and Keeper",
            config,
            &judgment_default,
        )?
    } else {
        judgment_default
    };

    let implementation_provider = if interactive && enabled.len() > 1 {
        select_provider_for_role("Provider for Mason", config, &implementation_default)?
    } else {
        implementation_default
    };

    let memory_provider = if interactive && enabled.len() > 1 {
        select_provider_for_role("Provider for Coobie", config, &memory_default)?
    } else {
        memory_default
    };

    for agent in ["scout", "sable", "keeper"] {
        set_agent_route(&mut routing, agent, &judgment_provider, &default_provider);
    }
    set_agent_route(
        &mut routing,
        "mason",
        &implementation_provider,
        &default_provider,
    );
    set_agent_route(&mut routing, "coobie", &memory_provider, &default_provider);

    routing.agents.retain(|_, provider| {
        config
            .resolve_provider(provider)
            .map(|candidate| candidate.enabled)
            .unwrap_or(false)
    });

    config.routing = if routing.agents.is_empty() {
        None
    } else {
        Some(routing)
    };

    Ok(())
}

fn preferred_provider(config: &SetupConfig, preferred_order: &[&str]) -> String {
    for name in preferred_order {
        if provider_enabled(config, name) {
            return (*name).to_string();
        }
    }
    config.providers.default.clone()
}

fn provider_enabled(config: &SetupConfig, name: &str) -> bool {
    config
        .resolve_provider(name)
        .map(|provider| provider.enabled)
        .unwrap_or(false)
}

fn select_provider_for_role(label: &str, config: &SetupConfig, default: &str) -> Result<String> {
    let enabled = enabled_provider_names(config);
    let chosen = prompt_text(&format!("{label} ({})", enabled.join(", ")), default)?;
    if enabled.iter().any(|name| name == &chosen) {
        Ok(chosen)
    } else {
        bail!("unknown provider for {label}: {chosen}");
    }
}

fn set_agent_route(
    routing: &mut RoutingConfig,
    agent_name: &str,
    chosen_provider: &str,
    default_provider: &str,
) {
    let base_provider = match agent_name {
        "scout" | "sable" | "keeper" => "claude",
        _ => "default",
    };

    let should_clear = match base_provider {
        "default" => chosen_provider == default_provider,
        pinned => chosen_provider == pinned,
    };

    if should_clear {
        routing.agents.remove(agent_name);
    } else {
        routing
            .agents
            .insert(agent_name.to_string(), chosen_provider.to_string());
    }
}

fn interview_mcp_servers(config: &mut SetupConfig, discovery: &SystemDiscovery) -> Result<()> {
    let mut existing_by_name = HashMap::new();
    if let Some(mcp) = config.mcp.take() {
        for server in mcp.servers {
            existing_by_name.insert(server.name.clone(), server);
        }
    }

    let mut servers = Vec::new();
    for template in common_mcp_templates(discovery.platform.as_str()) {
        let existing = existing_by_name.remove(template.name);
        if let Some(server) = configure_mcp_server_prompt(&template, existing)? {
            servers.push(server);
        }
    }

    let mut remaining: Vec<_> = existing_by_name.into_iter().collect();
    remaining.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, server) in remaining {
        if prompt_bool(
            &format!("Keep existing custom MCP server {}?", server.name),
            true,
        )? {
            servers.push(server);
        }
    }

    while prompt_bool("Add another custom MCP server?", false)? {
        servers.push(prompt_custom_mcp_server()?);
    }

    config.mcp = if servers.is_empty() {
        None
    } else {
        Some(McpConfig { servers })
    };
    Ok(())
}

fn common_mcp_templates(platform: &str) -> Vec<McpInterviewTemplate> {
    let mut templates = vec![
        McpInterviewTemplate {
            name: "filesystem",
            prompt: "filesystem MCP for product/workspace/artifact access",
            command: "npx",
            args: &[
                "-y",
                "@modelcontextprotocol/server-filesystem",
                "./products",
                "./factory/workspaces",
                "./factory/artifacts",
            ],
            env: &[],
            aliases: &["filesystem_read", "workspace_write", "artifact_writer"],
            default_enabled: true,
            customizable: false,
        },
        McpInterviewTemplate {
            name: "memory",
            prompt: "memory MCP for retrieval and scratch knowledge",
            command: "npx",
            args: &["-y", "@modelcontextprotocol/server-memory"],
            env: &[],
            aliases: &["memory_store", "metadata_query"],
            default_enabled: true,
            customizable: false,
        },
        McpInterviewTemplate {
            name: "sqlite",
            prompt: "sqlite MCP for run-state inspection",
            command: "npx",
            args: &[
                "-y",
                "@modelcontextprotocol/server-sqlite",
                "./factory/state.db",
            ],
            env: &[],
            aliases: &["db_read"],
            default_enabled: true,
            customizable: false,
        },
        McpInterviewTemplate {
            name: "github",
            prompt: "GitHub MCP for repository, issue, and doc access",
            command: "npx",
            args: &["-y", "@modelcontextprotocol/server-github"],
            env: &[("GITHUB_PERSONAL_ACCESS_TOKEN", "GITHUB_TOKEN")],
            aliases: &["fetch_docs", "github_read"],
            default_enabled: platform != "ci",
            customizable: false,
        },
        McpInterviewTemplate {
            name: "brave-search",
            prompt: "web-search MCP for external research",
            command: "npx",
            args: &["-y", "@modelcontextprotocol/server-brave-search"],
            env: &[("BRAVE_API_KEY", "BRAVE_API_KEY")],
            aliases: &["web_search"],
            default_enabled: platform != "ci",
            customizable: false,
        },
    ];

    templates.push(McpInterviewTemplate {
        name: "winccoa",
        prompt: "winccOA or OT/SCADA MCP integration",
        command: "winccoa-mcp",
        args: &[],
        env: &[
            ("WINCCOA_URL", "WINCCOA_URL"),
            ("WINCCOA_PROJECT", "WINCCOA_PROJECT"),
            ("WINCCOA_USERNAME", "WINCCOA_USERNAME"),
            ("WINCCOA_PASSWORD", "WINCCOA_PASSWORD"),
        ],
        aliases: &["ot_read", "telemetry_read", "winccoa_ops"],
        default_enabled: false,
        customizable: true,
    });

    templates
}

fn configure_mcp_server_prompt(
    template: &McpInterviewTemplate,
    existing: Option<McpServerConfig>,
) -> Result<Option<McpServerConfig>> {
    let enabled_default = existing.is_some() || template.default_enabled;
    let enabled = prompt_bool(&format!("Enable {}?", template.prompt), enabled_default)?;
    if !enabled {
        return Ok(None);
    }

    let mut server = existing.unwrap_or_else(|| template.default_server());
    if template.customizable {
        server.command = prompt_text(
            &format!("Command for {} MCP", template.name),
            &server.command,
        )?;
        server.args = prompt_csv(
            &format!("Args for {} MCP (comma separated)", template.name),
            &server.args,
        )?;
        let aliases_default = server.tool_aliases.clone().unwrap_or_default();
        let aliases = prompt_csv(
            &format!("Tool aliases for {} MCP (comma separated)", template.name),
            &aliases_default,
        )?;
        server.tool_aliases = if aliases.is_empty() {
            None
        } else {
            Some(aliases)
        };
    }

    let env_defaults = env_defaults(template, server.env.as_ref());
    if !env_defaults.is_empty() {
        server.env = Some(prompt_env_bindings(template.name, &env_defaults)?);
    }

    Ok(Some(server))
}

fn env_defaults(
    template: &McpInterviewTemplate,
    existing: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (key, value) in template.env {
        out.insert((*key).to_string(), (*value).to_string());
    }
    if let Some(existing) = existing {
        for (key, value) in existing {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn prompt_env_bindings(
    server_name: &str,
    defaults: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    println!("{} MCP environment bindings:", server_name);
    let mut keys: Vec<_> = defaults.keys().cloned().collect();
    keys.sort();
    let mut env = HashMap::new();
    for key in keys {
        let default = defaults.get(&key).cloned().unwrap_or_default();
        let value = prompt_text(&format!("Env var name for {key}"), &default)?;
        if !value.trim().is_empty() {
            env.insert(key, value);
        }
    }
    Ok(env)
}

fn prompt_custom_mcp_server() -> Result<McpServerConfig> {
    let name = prompt_text("Custom MCP name", "custom-server")?;
    let command = prompt_text("Command for custom MCP", "npx")?;
    let args = prompt_csv("Args for custom MCP (comma separated)", &[])?;
    let aliases = prompt_csv("Tool aliases for custom MCP (comma separated)", &[])?;
    let env_raw = prompt_text("Env bindings for custom MCP (KEY=VALUE,KEY2=VALUE2)", "")?;

    if name.trim().is_empty() {
        bail!("custom MCP name cannot be empty");
    }
    if command.trim().is_empty() {
        bail!("custom MCP command cannot be empty");
    }

    let env = parse_env_bindings(&env_raw)?;
    Ok(McpServerConfig {
        name: name.trim().to_string(),
        command,
        args,
        env: if env.is_empty() { None } else { Some(env) },
        tool_aliases: if aliases.is_empty() {
            None
        } else {
            Some(aliases)
        },
    })
}

fn parse_env_bindings(raw: &str) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for item in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let Some((key, value)) = item.split_once('=') else {
            bail!("invalid env binding: {item}; expected KEY=VALUE");
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            bail!("invalid env binding: {item}; expected KEY=VALUE");
        }
        env.insert(key.to_string(), value.to_string());
    }
    Ok(env)
}

fn prompt_csv(label: &str, default: &[String]) -> Result<Vec<String>> {
    let default_text = default.join(", ");
    let raw = prompt_text(label, &default_text)?;
    Ok(raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect())
}

fn normalize_mcp_servers(config: &mut SetupConfig) {
    let Some(mcp) = &mut config.mcp else {
        return;
    };

    let mut seen = HashSet::new();
    mcp.servers.retain(|server| {
        let name = server.name.trim();
        let command = server.command.trim();
        !name.is_empty() && !command.is_empty() && seen.insert(name.to_string())
    });

    if mcp.servers.is_empty() {
        config.mcp = None;
    }
}

fn print_setup_mcp_preview(mcp: Option<&McpConfig>) {
    println!();
    match mcp {
        Some(mcp) if !mcp.servers.is_empty() => {
            println!("Selected MCP servers:");
            for server in &mcp.servers {
                let aliases = server
                    .tool_aliases
                    .as_deref()
                    .map(|value| value.join(", "))
                    .unwrap_or_else(|| "no aliases".to_string());
                println!("  - {} via {} [{}]", server.name, server.command, aliases);
            }
        }
        _ => println!("Selected MCP servers: none"),
    }
}

fn render_generated_setup(config: &SetupConfig) -> Result<String> {
    let mut out = String::new();
    out.push_str("# Generated by `harkonnen setup init`\n");
    out.push_str(
        "# Review provider API key env vars, MCP settings, and routing before sharing broadly.\n\n",
    );
    out.push_str(&toml::to_string_pretty(config)?);
    Ok(out)
}

fn print_provider_status(name: &str, config: Option<&ProviderConfig>) {
    match config {
        None => println!("  [skip   ] {name}: not configured"),
        Some(c) if !c.enabled => println!("  [skip   ] {name}: disabled{}", provider_notes(c)),
        Some(c) => {
            let key_present = std::env::var(&c.api_key_env).is_ok();
            let tag = if key_present { "ok   " } else { "MISSING" };
            let key_note = if key_present {
                String::new()
            } else {
                format!("  (set {})", c.api_key_env)
            };
            println!(
                "  [{tag}] {name}: {}{}{}",
                c.model,
                provider_notes(c),
                key_note
            );
        }
    }
}

fn print_agent_routing(paths: &Paths, setup: &SetupConfig) -> Result<()> {
    let profiles_dir = paths.factory.join("agents").join("profiles");
    if !profiles_dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&profiles_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("yaml"))
        .collect();
    entries.sort();

    println!();
    println!("Agent Routing:");
    for path in entries {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading agent profile: {}", path.display()))?;
        let profile: AgentProfileSummary = serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing agent profile: {}", path.display()))?;

        let base_provider = setup.resolve_provider_name(&profile.provider);
        let effective_provider =
            setup.resolve_agent_provider_name(&profile.name, &profile.provider);
        let override_note = if effective_provider != base_provider {
            format!("  override from {base_provider}")
        } else {
            String::new()
        };
        let model_note = setup
            .resolve_agent_provider(&profile.name, &profile.provider)
            .map(|cfg| format!(" ({}){}", cfg.model, provider_notes(cfg)))
            .unwrap_or_else(|| " (unresolved)".to_string());

        println!(
            "  {} -> {}{}{}",
            profile.name, effective_provider, model_note, override_note
        );
    }

    Ok(())
}

fn provider_notes(config: &ProviderConfig) -> String {
    let mut notes = Vec::new();
    if let Some(surface) = &config.surface {
        notes.push(format!("surface: {surface}"));
    }
    if let Some(base_url) = &config.base_url {
        notes.push(format!("base_url: {base_url}"));
    }
    if let Some(usage_rights) = &config.usage_rights {
        notes.push(format!("usage: {usage_rights}"));
    }
    if notes.is_empty() {
        String::new()
    } else {
        format!("  [{}]", notes.join(", "))
    }
}

fn prompt_text(label: &str, default: &str) -> Result<String> {
    print!("{} [{}]: ", label, default);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_bool(label: &str, default: bool) -> Result<bool> {
    let suffix = if default { "Y/n" } else { "y/N" };
    loop {
        print!("{} [{}]: ", label, suffix);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            return Ok(default);
        }
        match trimmed.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer yes or no."),
        }
    }
}

fn bool_tag(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn resolve_write_target(root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn activation_hint(platform: &str, root: &Path, path: &Path) -> String {
    let relative = relative_display(root, path);
    if platform == "windows" {
        format!("$env:HARKONNEN_SETUP = \"{relative}\"")
    } else {
        format!("export HARKONNEN_SETUP={relative}")
    }
}

pub async fn handle_capacity(command: CapacityCommands, paths: &Paths) -> Result<()> {
    match command {
        CapacityCommands::Show => {
            let state = CapacityState::load(&paths.root)?;
            println!("Provider Capacity (updated {})\n", state.updated_at);
            let mut providers: Vec<_> = state.providers.iter().collect();
            providers.sort_by_key(|(_, cap)| cap.priority);
            for (name, cap) in &providers {
                let avail = if cap.available { "✓" } else { "✗" };
                let note = cap.note.as_deref().unwrap_or("");
                println!(
                    "  [{avail}] {name:<8} {:<12} priority={} {}",
                    cap.status, cap.priority, note
                );
            }
            println!("\nFallback chain: {}", state.fallback_chain.join(" → "));
        }

        CapacityCommands::Set(args) => {
            let valid_statuses = ["ok", "near_limit", "at_limit"];
            if !valid_statuses.contains(&args.status.as_str()) {
                anyhow::bail!(
                    "invalid status '{}' — use: ok, near_limit, at_limit",
                    args.status
                );
            }
            let mut state = CapacityState::load(&paths.root)?;
            let availability_changed =
                state.set(&args.provider, &args.status, args.note.clone(), "human");
            state.save(&paths.root)?;
            println!("capacity: {} → {}", args.provider, args.status);
            if availability_changed {
                println!(
                    "  availability changed — run `harkonnen capacity reassign` to update assignments"
                );
            }
        }

        CapacityCommands::Reassign => {
            let state = CapacityState::load(&paths.root)?;
            let assignments_path = paths.factory.join("coordination").join("assignments.json");

            if !assignments_path.exists() {
                println!("No assignments.json found — nothing to reassign.");
                return Ok(());
            }

            let raw = std::fs::read_to_string(&assignments_path)?;
            let mut assignments: serde_json::Value = serde_json::from_str(&raw)?;
            let active = assignments
                .get_mut("active")
                .and_then(|a| a.as_object_mut());

            let Some(active) = active else {
                println!("No active claims in assignments.json.");
                return Ok(());
            };

            let mut reassigned: Vec<(String, String, String)> = Vec::new();

            for (agent, claim) in active.iter_mut() {
                // Extract owned strings before any mutable borrow.
                let provider = claim
                    .get("provider")
                    .and_then(|p| p.as_str())
                    .unwrap_or(agent.as_str())
                    .to_string();
                let task = claim
                    .get("task")
                    .and_then(|t| t.as_str())
                    .unwrap_or("(unknown task)")
                    .to_string();

                if !state.is_available(&provider) {
                    if let Some(fallback) = state.best_available(&[provider.as_str()]) {
                        let _ = task;
                        if let Some(obj) = claim.as_object_mut() {
                            obj.insert(
                                "provider".to_string(),
                                serde_json::Value::String(fallback.clone()),
                            );
                            obj.insert(
                                "reassigned_from".to_string(),
                                serde_json::Value::String(provider.to_string()),
                            );
                            obj.insert(
                                "reassigned_reason".to_string(),
                                serde_json::Value::String(format!(
                                    "{provider} is {}",
                                    state
                                        .providers
                                        .get(&provider)
                                        .map(|c| c.status.as_str())
                                        .unwrap_or("unavailable")
                                )),
                            );
                        }
                        reassigned.push((agent.clone(), provider.clone(), fallback));
                    }
                }
            }

            if reassigned.is_empty() {
                println!("All active claims are on available providers — nothing to reassign.");
            } else {
                assignments["updated_at"] =
                    serde_json::Value::String(chrono::Utc::now().to_rfc3339());
                std::fs::write(
                    &assignments_path,
                    serde_json::to_string_pretty(&assignments)?,
                )?;
                println!("Reassigned {} claim(s):", reassigned.len());
                for (agent, from, to) in &reassigned {
                    println!("  {agent}: {from} → {to}");
                }
                println!("\nUpdate assignments.md manually to reflect the handoff context.");
            }
        }
    }
    Ok(())
}

fn print_template_preview(template: &str) {
    for line in template.lines().take(60) {
        println!("  {}", line);
    }
    if template.lines().count() > 60 {
        println!("  ...");
    }
}
