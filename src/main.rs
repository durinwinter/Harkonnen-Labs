mod agents;
mod api;
mod benchmark;
mod capacity;
mod chat;
mod cladder;
mod claude_pack;
mod cli;
mod config;
mod coobie;
mod coobie_palace;
mod db;
mod embeddings;
mod frames;
mod helmet;
mod llm;
mod locomo;
mod longmemeval;
mod memory;
mod models;
mod operator_model;
mod orchestrator;
mod pidgin;
mod policy;
mod reporting;
mod scenarios;
mod setup;
mod calvin_archive;
mod spec;
mod streamingqa;
mod tesseract;
mod workspace;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use orchestrator::AppContext;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present — silently skip if missing.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Spec { command } => cli::handle_spec(command).await?,
        Commands::Run { command } => {
            let app = AppContext::bootstrap().await?;
            cli::handle_run(command, app).await?
        }
        Commands::Artifact { command } => {
            let app = AppContext::bootstrap().await?;
            cli::handle_artifact(command, app).await?
        }
        Commands::Memory { command } => {
            let app = AppContext::bootstrap().await?;
            cli::handle_memory(command, app).await?
        }
        Commands::Evidence { command } => {
            let app = AppContext::bootstrap().await?;
            cli::handle_evidence(command, app).await?
        }
        Commands::Setup { command } => {
            // setup check doesn't need the DB — just path + config discovery
            let paths = config::Paths::discover()?;
            cli::handle_setup(command, &paths).await?
        }
        Commands::Soul { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_soul(command, &paths).await?
        }
        Commands::Serve(args) => {
            let app = AppContext::bootstrap().await?;
            cli::handle_serve(args, app).await?
        }
        Commands::Capacity { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_capacity(command, &paths).await?
        }
        Commands::Benchmark { command } => {
            let paths = config::Paths::discover()?;
            cli::handle_benchmark(command, &paths).await?
        }
    }

    Ok(())
}
