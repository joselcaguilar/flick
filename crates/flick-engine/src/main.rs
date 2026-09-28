use std::{env, io, path::PathBuf};

use clap::{Parser, Subcommand};
use flick_engine::{bench, config, logging, runtime};

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
    /// Override the vision model directory.
    #[arg(long, env = "FLICK_MODELS_DIR")]
    models_dir: Option<PathBuf>,
    /// Override the local API port.
    #[arg(long, env = "FLICK_PORT")]
    port: Option<u16>,
    /// Use the built-in mock Home Assistant stack.
    #[arg(long)]
    mock_ha: bool,
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

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let overrides = config::CliOverrides {
        sidecar: cli.sidecar,
        dev: cli.dev,
        data_dir: cli.data_dir,
        port: cli.port,
        fake_camera: cli.fake_camera,
        models_dir: cli.models_dir,
        mock_ha: cli.mock_ha.then_some(true),
    };
    let runtime = config::load_with_overrides(&overrides)?;
    let _guard = logging::init(&runtime)?;

    match cli.command {
        Some(Commands::Bench { fixture }) => {
            bench::run(fixture)?;
        }
        None => {
            tracing::info!(
                sidecar = runtime.sidecar,
                dev = runtime.dev,
                data_dir = %runtime.data_dir.display(),
                bind = %runtime.bootstrap.engine.bind,
                port = runtime.bootstrap.engine.port,
                fake_camera = ?runtime.fake_camera,
                models_dir = ?runtime.models_dir,
                fake_landmarks = ?runtime.fake_landmarks,
                fake_landmarks_autoplay = ?runtime.fake_landmarks_autoplay,
                mock_ha = runtime.mock_ha,
                update_url = ?runtime.update_url,
                "starting engine runtime"
            );
            let token = api_token(runtime.sidecar, runtime.dev)?;
            runtime::serve(runtime, token).await?;
        }
    }

    Ok(())
}

fn api_token(sidecar: bool, dev: bool) -> anyhow::Result<String> {
    if sidecar {
        let mut token = String::new();
        io::stdin().read_line(&mut token)?;
        let token = token.trim().to_owned();
        anyhow::ensure!(!token.is_empty(), "sidecar token missing on stdin");
        return Ok(token);
    }
    if dev {
        return Ok("dev-token".to_owned());
    }
    env::var("FLICK_TOKEN")
        .or_else(|_| env::var("FLICK_API_TOKEN"))
        .map_err(|_| anyhow::anyhow!("headless mode requires FLICK_TOKEN"))
        .and_then(|token| {
            anyhow::ensure!(!token.is_empty(), "headless mode requires FLICK_TOKEN");
            Ok(token)
        })
}
