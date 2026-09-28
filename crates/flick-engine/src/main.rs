use std::{
    io::{self, BufRead},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use clap::{Parser, Subcommand};
use flick_engine::{config, logging};
use serde::Serialize;
use tokio::net::TcpListener;

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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;
    runtime.block_on(async_main())
}

async fn async_main() -> anyhow::Result<()> {
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
            if runtime.sidecar {
                run_sidecar(runtime).await?;
                return Ok(());
            }
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

#[derive(Debug, Serialize)]
struct ReadyLine<'a> {
    event: &'a str,
    port: u16,
    version: &'a str,
}

#[derive(Debug, Serialize)]
struct HealthResponse<'a> {
    status: &'a str,
    version: &'a str,
}

#[derive(Debug, Serialize)]
struct UpdatesResponse {
    busy_reason: Option<String>,
}

#[derive(Clone)]
struct SidecarState {
    token: Arc<str>,
}

async fn run_sidecar(runtime: config::RuntimeConfig) -> anyhow::Result<()> {
    let token = read_sidecar_token()?;
    tracing::info!(
        data_dir = %runtime.data_dir.display(),
        "sidecar token received over stdin"
    );

    let bind_ip: IpAddr = runtime
        .bootstrap
        .engine
        .bind
        .parse()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let listener =
        TcpListener::bind(SocketAddr::new(bind_ip, runtime.bootstrap.engine.port)).await?;
    let port = listener.local_addr()?.port();

    let state = SidecarState {
        token: Arc::from(token),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/updates", get(updates))
        .route_layer(middleware::from_fn_with_state(state, require_sidecar_auth));

    println!(
        "{}",
        serde_json::to_string(&ReadyLine {
            event: "ready",
            port,
            version: env!("CARGO_PKG_VERSION"),
        })?
    );
    tracing::info!(port, "sidecar HTTP API ready");

    axum::serve(listener, app).await?;
    Ok(())
}

fn read_sidecar_token() -> anyhow::Result<String> {
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let token = line.trim().to_owned();
    anyhow::ensure!(!token.is_empty(), "missing sidecar token on stdin");
    Ok(token)
}

async fn health() -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn updates() -> Json<UpdatesResponse> {
    Json(UpdatesResponse { busy_reason: None })
}

async fn require_sidecar_auth(
    State(state): State<SidecarState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !is_loopback_host(&headers) || !is_allowed_origin(&headers) || !has_token(&headers, &state) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

fn is_loopback_host(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    host.starts_with("127.0.0.1:") || host.starts_with("localhost:") || host.starts_with("[::1]:")
}

fn is_allowed_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    origin == "http://localhost:5173"
        || origin == "http://127.0.0.1:5173"
        || origin == "tauri://localhost"
        || origin == "http://tauri.localhost"
}

fn has_token(headers: &HeaderMap, state: &SidecarState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}
