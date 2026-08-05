pub mod agents;
pub mod aider_polyglot;
pub mod api;
pub mod benchmark;
pub mod calvin_archive;
pub mod calvin_client;
pub mod capacity;
pub mod causal_graph;
pub mod chat;
pub mod cladder;
pub mod claude_pack;
pub mod cli;
pub mod config;
pub mod coobie;
pub mod coobie_palace;
pub mod db;
pub mod embeddings;
pub mod frames;
pub mod gaia;
pub mod helmet;
pub mod hook;
pub mod livecodebench;
pub mod llm;
pub mod locomo;
pub mod longmemeval;
pub mod mason_tools;
pub mod mason_transport;
pub mod mcp_registry;
pub mod mcp_server;
pub mod memory;
pub mod models;
pub mod openbrain;
pub mod operator_model;
pub mod orchestrator;
pub mod pidgin;
pub mod policy;
pub mod reporting;
pub mod scenario_delta;
pub mod scenarios;
pub mod setup;
pub mod skill_fetcher;
pub mod skill_registry;
pub mod spec;
pub mod spec_adherence;
pub mod stamp;
pub mod streamingqa;
pub mod subagent;
pub mod tesseract;
pub mod twin_fidelity;
pub mod workspace;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let parsed = cli::Cli::parse();

    match parsed.command {
        cli::Commands::Spec { command } => cli::handle_spec(command).await?,
        cli::Commands::Run { command } => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_run(command, app).await?
        }
        cli::Commands::Artifact { command } => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_artifact(command, app).await?
        }
        cli::Commands::Memory { command } => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_memory(command, app).await?
        }
        cli::Commands::Evidence { command } => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_evidence(command, app).await?
        }
        cli::Commands::Setup { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_setup(command, &paths).await?
        }
        cli::Commands::Mcp { command } => {
            let app = orchestrator::AppContext::bootstrap_for_mcp().await?;
            cli::handle_mcp(command, app).await?
        }
        cli::Commands::Soul { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_soul(command, &paths).await?
        }
        cli::Commands::Serve(args) => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_serve(args, app).await?
        }
        cli::Commands::Capacity { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_capacity(command, &paths).await?
        }
        cli::Commands::Benchmark { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_benchmark(command, &paths).await?
        }
        cli::Commands::Stamp { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_stamp(command, &paths).await?
        }
        cli::Commands::Hook { command } => cli::handle_hook(command)?,
        cli::Commands::Subagent { command } => cli::handle_subagent(command).await?,
        cli::Commands::Archive { command } => {
            let app = orchestrator::AppContext::bootstrap().await?;
            cli::handle_archive(command, app).await?
        }
    }

    Ok(())
}
