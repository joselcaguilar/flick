use std::path::PathBuf;

use clap::{Parser, Subcommand};
use flick_engine::{config, logging};

#[derive(Debug, Parser)]
#[command(name = "flick-engine", about = "Flick gesture engine")]
struct Cli {
    /// Run under the desktop sidecar supervisor.
    #[arg(long)]
    sidecar: bool,
    /// Enable development defaults.
    #[arg(long)]
    dev: bool,
    /// Replay a fake camera video or image directory.
    #[arg(long, env = "FLICK_FAKE_CAMERA")]
    fake_camera: Option<PathBuf>,
    /// Override the platform data directory.
    #[arg(long, env = "FLICK_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Override the local API port.
    #[arg(long, env = "FLICK_PORT")]
    port: Option<u16>,
    /// Subcommand to run.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the engine benchmark harness.
    Bench {
        /// Replay fixture to benchmark.
        #[arg(long)]
        fixture: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let overrides = config::CliOverrides {
        sidecar: cli.sidecar,
        dev: cli.dev,
        data_dir: cli.data_dir,
        port: cli.port,
        fake_camera: cli.fake_camera,
    };
    let runtime = config::load_with_overrides(&overrides)?;
    let _guard = logging::init(&runtime)?;

    match cli.command {
        Some(Commands::Bench { fixture }) => {
            tracing::info!(
                ?fixture,
                "bench command parsed; benchmark wiring is a later task"
            );
        }
        None => {
            tracing::info!(
                sidecar = runtime.sidecar,
                dev = runtime.dev,
                data_dir = %runtime.data_dir.display(),
                bind = %runtime.bootstrap.engine.bind,
                port = runtime.bootstrap.engine.port,
                fake_camera = ?runtime.fake_camera,
                fake_landmarks = ?runtime.fake_landmarks,
                update_url = ?runtime.update_url,
                "engine CLI parsed; runtime wiring is a later task"
            );
        }
    }

    Ok(())
}
