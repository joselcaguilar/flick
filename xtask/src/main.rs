//! Flick developer task runner.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod bench;
mod build_pack;
mod bundle;
mod fetch_models;
mod gen_api;
mod record_landmarks;
mod serve_updates;
mod sign_index;

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "Flick developer automation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fetch and verify model files.
    FetchModels,
    /// Generate OpenAPI and TypeScript API bindings.
    GenApi,
    /// Run the benchmark harness.
    Bench,
    /// Record landmark fixtures.
    RecordLandmarks,
    /// Bundle application artifacts.
    Bundle,
    /// Build a signed model or catalog pack.
    BuildPack,
    /// Sign an OTA channel index.
    SignIndex,
    /// Serve a local update tree.
    ServeUpdates,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::FetchModels => fetch_models::run(),
        Command::GenApi => gen_api::run(),
        Command::Bench => bench::run(),
        Command::RecordLandmarks => record_landmarks::run(),
        Command::Bundle => bundle::run(),
        Command::BuildPack => build_pack::run(),
        Command::SignIndex => sign_index::run(),
        Command::ServeUpdates => serve_updates::run(),
    }
}
