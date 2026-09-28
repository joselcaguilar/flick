#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command as OsCommand,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{
    ActivationPolicy, AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WindowEvent,
    image::Image,
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    tray::TrayIconBuilder,
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};
use tauri_plugin_updater::UpdaterExt;
use tokio::{sync::Mutex as AsyncMutex, time};
use url::Url;

const SIDECAR_NAME: &str = "binaries/flick-engine";
const UPDATE_STATE_FILE: &str = "update_state.json";

#[derive(Clone)]
struct AppState {
    supervisor: Arc<EngineSupervisor>,
    updates: Arc<UpdateManager>,
    hud: Arc<StdMutex<HudConfig>>,
    paused: Arc<StdMutex<bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineEndpoint {
    base_url: String,
    token: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EngineStatus {
    Starting,
    Ready,
    Restarting,
    Fatal,
    Stopped,
}

#[derive(Debug)]
struct EngineInner {
    endpoint: Option<EngineEndpoint>,
    version: Option<String>,
    status: EngineStatus,
    pid: Option<u32>,
    shutting_down: bool,
    crashes: VecDeque<Instant>,
    last_error: Option<String>,
}

struct EngineSupervisor {
    inner: AsyncMutex<EngineInner>,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ReadyLine {
    event: String,
    port: u16,
    version: String,
}

#[derive(Debug, Deserialize)]
struct EngineUpdatesResponse {
    busy_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HudConfig {
    position: HudPosition,
    enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum HudPosition {
    TopCenter,
    TopRight,
    BottomCenter,
    BottomRight,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self {
            position: HudPosition::TopCenter,
            enabled: false,
        }
    }
}

#[derive(Debug, Serialize)]
struct AppInfo {
    version: String,
    platform: &'static str,
    arch: &'static str,
    pro: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AppUpdateState {
    Idle,
    Checking,
    Available,
    Downloading,
    Installing,
    RollbackAvailable,
    RollingBack,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppUpdateStatus {
    state: AppUpdateState,
    available: Option<String>,
    downloaded: bool,
    rollback_to: Option<String>,
    error: Option<String>,
}

impl Default for AppUpdateStatus {
    fn default() -> Self {
        Self {
            state: AppUpdateState::Idle,
            available: None,
            downloaded: false,
            rollback_to: None,
            error: None,
        }
    }
}

struct UpdateManager {
    inner: AsyncMutex<AppUpdateStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskUpdateState {
    pending_app: Option<PendingAppUpdate>,
    rollback_to: Option<String>,
    health: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingAppUpdate {
    from: String,
    to: String,
    db_backup: Option<PathBuf>,
}

impl EngineSupervisor {
    fn new() -> Self {
        Self {
            inner: AsyncMutex::new(EngineInner {
                endpoint: None,
                version: None,
                status: EngineStatus::Stopped,
                pid: None,
                shutting_down: false,
                crashes: VecDeque::new(),
                last_error: None,
            }),
            client: reqwest::Client::new(),
        }
    }

    fn start(self: Arc<Self>, app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            self.run(app).await;
        });
    }

    async fn endpoint(&self) -> Option<EngineEndpoint> {
        self.inner.lock().await.endpoint.clone()
    }

    async fn run(&self, app: AppHandle) {
        let mut backoff = Duration::from_millis(500);
        loop {
            if self.is_shutting_down().await {
                self.set_status(&app, EngineStatus::Stopped, None).await;
                break;
            }

            self.set_status(&app, EngineStatus::Starting, None).await;
            match self.spawn_once(&app).await {
                Ok(()) => {
                    backoff = Duration::from_millis(500);
                }
                Err(error) => {
                    self.record_crash(&app, error.to_string()).await;
                }
            }

            if self.is_fatal().await || self.is_shutting_down().await {
                continue;
            }

            self.set_status(&app, EngineStatus::Restarting, None).await;
            time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(10));
        }
    }

    async fn spawn_once(&self, app: &AppHandle) -> anyhow::Result<()> {
        let token = generate_token()?;
        let data_dir = app.path().app_data_dir()?;
        fs::create_dir_all(&data_dir)?;
        let data_dir_arg = data_dir.to_string_lossy().to_string();
        let (mut rx, mut child) = app
            .shell()
            .sidecar(SIDECAR_NAME)?
            .args(["--sidecar", "--port", "0", "--data-dir", &data_dir_arg])
            .spawn()?;
        child.write(format!("{token}\n").as_bytes())?;
        let pid = child.pid();

        let ready = self.wait_ready(&mut rx).await?;
        if ready.event != "ready" {
            anyhow::bail!("unexpected sidecar event {}", ready.event);
        }
        let endpoint = EngineEndpoint {
            base_url: format!("http://127.0.0.1:{}", ready.port),
            token,
        };
        {
            let mut inner = self.inner.lock().await;
            inner.endpoint = Some(endpoint.clone());
            inner.version = Some(ready.version);
            inner.status = EngineStatus::Ready;
            inner.pid = Some(pid);
            inner.last_error = None;
        }
        let _ = app.emit("engine-status", EngineStatus::Ready);
        self.monitor(app, endpoint, child, rx).await;
        Ok(())
    }

    async fn wait_ready(
        &self,
        rx: &mut tokio::sync::mpsc::Receiver<CommandEvent>,
    ) -> anyhow::Result<ReadyLine> {
        let deadline = time::sleep(Duration::from_secs(10));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(CommandEvent::Stdout(line)) => {
                            let text = String::from_utf8_lossy(&line);
                            if let Ok(ready) = serde_json::from_str::<ReadyLine>(text.trim()) {
                                return Ok(ready);
                            }
                        }
                        Some(CommandEvent::Stderr(line)) => {
                            tracing::warn!(stderr = %String::from_utf8_lossy(&line), "engine sidecar stderr before ready");
                        }
                        Some(CommandEvent::Terminated(payload)) => {
                            anyhow::bail!("engine terminated before ready: code={:?}", payload.code);
                        }
                        Some(CommandEvent::Error(error)) => anyhow::bail!(error),
                        Some(_) => {}
                        None => anyhow::bail!("engine event stream closed before ready"),
                    }
                }
                () = &mut deadline => anyhow::bail!("engine ready handshake timed out"),
            }
        }
    }

    async fn monitor(
        &self,
        app: &AppHandle,
        endpoint: EngineEndpoint,
        child: CommandChild,
        mut rx: tokio::sync::mpsc::Receiver<CommandEvent>,
    ) {
        let mut child = Some(child);
        let mut interval = time::interval(Duration::from_secs(2));
        let mut failed_health_checks = 0u8;
        loop {
            if self.is_shutting_down().await {
                if let Some(child) = child.take() {
                    terminate_child(child).await;
                }
                self.clear_running(EngineStatus::Stopped).await;
                let _ = app.emit("engine-status", EngineStatus::Stopped);
                break;
            }

            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(CommandEvent::Terminated(payload)) => {
                            self.clear_running(EngineStatus::Restarting).await;
                            if !self.is_shutting_down().await {
                                self.record_crash(app, format!("engine exited: code={:?}", payload.code)).await;
                            }
                            break;
                        }
                        Some(CommandEvent::Stderr(line)) => tracing::warn!(stderr = %String::from_utf8_lossy(&line), "engine sidecar stderr"),
                        Some(CommandEvent::Stdout(line)) => tracing::debug!(stdout = %String::from_utf8_lossy(&line), "engine sidecar stdout"),
                        Some(CommandEvent::Error(error)) => {
                            self.record_crash(app, error).await;
                            break;
                        }
                        Some(_) => {}
                        None => {
                            self.record_crash(app, "engine event stream closed".to_owned()).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if self.health_ok(&endpoint).await {
                        failed_health_checks = 0;
                    } else {
                        failed_health_checks = failed_health_checks.saturating_add(1);
                        if failed_health_checks >= 3 {
                            if let Some(child) = child.take() {
                                terminate_child(child).await;
                            }
                            self.record_crash(app, "engine health check failed 3 times".to_owned()).await;
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn health_ok(&self, endpoint: &EngineEndpoint) -> bool {
        let url = format!("{}/health", endpoint.base_url);
        match self
            .client
            .get(url)
            .bearer_auth(&endpoint.token)
            .timeout(Duration::from_secs(1))
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(error) => {
                tracing::warn!(%error, "engine health check failed");
                false
            }
        }
    }

    async fn busy_reason(&self) -> Result<Option<String>, String> {
        let endpoint = self
            .endpoint()
            .await
            .ok_or_else(|| "engine is not ready".to_owned())?;
        let response = self
            .client
            .get(format!("{}/updates", endpoint.base_url))
            .bearer_auth(endpoint.token)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("engine /updates failed: {}", response.status()));
        }
        response
            .json::<EngineUpdatesResponse>()
            .await
            .map(|value| value.busy_reason)
            .map_err(|error| error.to_string())
    }

    async fn shutdown(&self) {
        let pid = {
            let mut inner = self.inner.lock().await;
            inner.shutting_down = true;
            inner.pid
        };
        if let Some(pid) = pid {
            terminate_pid(pid).await;
        }
    }

    async fn is_shutting_down(&self) -> bool {
        self.inner.lock().await.shutting_down
    }

    async fn is_fatal(&self) -> bool {
        self.inner.lock().await.status == EngineStatus::Fatal
    }

    async fn set_status(&self, app: &AppHandle, status: EngineStatus, error: Option<String>) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = status;
            inner.last_error = error;
        }
        let _ = app.emit("engine-status", status);
    }

    async fn clear_running(&self, status: EngineStatus) {
        let mut inner = self.inner.lock().await;
        inner.endpoint = None;
        inner.version = None;
        inner.pid = None;
        inner.status = status;
    }

    async fn record_crash(&self, app: &AppHandle, error: String) {
        let status = {
            let mut inner = self.inner.lock().await;
            inner.endpoint = None;
            inner.version = None;
            inner.pid = None;
            inner.last_error = Some(error.clone());
            let now = Instant::now();
            inner.crashes.push_back(now);
            while inner
                .crashes
                .front()
                .is_some_and(|crash| now.duration_since(*crash) > Duration::from_secs(120))
            {
                inner.crashes.pop_front();
            }
            if inner.crashes.len() >= 5 {
                inner.status = EngineStatus::Fatal;
                EngineStatus::Fatal
            } else {
                inner.status = EngineStatus::Restarting;
                EngineStatus::Restarting
            }
        };
        tracing::warn!(%error, ?status, "engine sidecar crashed");
        let _ = app.emit("engine-status", status);
        if status == EngineStatus::Fatal {
            let _ = app.emit("engine-fatal", error);
        }
    }
}

impl UpdateManager {
    fn new() -> Self {
        Self {
            inner: AsyncMutex::new(AppUpdateStatus::default()),
        }
    }

    async fn status(&self) -> AppUpdateStatus {
        self.inner.lock().await.clone()
    }

    async fn set_status(&self, status: AppUpdateStatus) {
        *self.inner.lock().await = status;
    }

    async fn check(&self, app: &AppHandle) -> Result<AppUpdateStatus, String> {
        self.set_status(AppUpdateStatus {
            state: AppUpdateState::Checking,
            ..AppUpdateStatus::default()
        })
        .await;
        let updater = update_builder(app, None)?
            .build()
            .map_err(|error| error.to_string())?;
        let update = updater.check().await.map_err(|error| error.to_string())?;
        let status = if let Some(update) = update {
            AppUpdateStatus {
                state: AppUpdateState::Available,
                available: Some(update.version),
                downloaded: false,
                rollback_to: None,
                error: None,
            }
        } else {
            AppUpdateStatus::default()
        };
        self.set_status(status.clone()).await;
        Ok(status)
    }

    async fn mark_error(&self, error: String) {
        let mut current = self.inner.lock().await;
        current.state = AppUpdateState::Error;
        current.error = Some(error);
    }
}

#[tauri::command]
async fn engine_endpoint(state: State<'_, AppState>) -> Result<EngineEndpoint, String> {
    for _ in 0..100u8 {
        if let Some(endpoint) = state.supervisor.endpoint().await {
            return Ok(endpoint);
        }
        time::sleep(Duration::from_millis(100)).await;
    }
    Err("engine is not ready".to_owned())
}

#[tauri::command]
fn show_main_window(app: AppHandle, route: Option<String>) -> Result<(), String> {
    show_main_window_impl(&app, route)
}

#[tauri::command]
fn set_hud_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: HudConfig,
) -> Result<(), String> {
    {
        let mut stored = state
            .hud
            .lock()
            .map_err(|_| "HUD config lock poisoned".to_owned())?;
        *stored = config.clone();
    }
    apply_hud_config(&app, &config)
}

#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|error| error.to_string())?;
    match parsed.scheme() {
        "https" | "x-apple.systempreferences" => app
            .opener()
            .open_url(url, None::<String>)
            .map_err(|error| error.to_string()),
        _ => Err("external URL scheme is not allowed".to_owned()),
    }
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        pro: false,
    }
}

#[tauri::command]
async fn app_update_status(state: State<'_, AppState>) -> Result<AppUpdateStatus, String> {
    Ok(state.updates.status().await)
}

#[tauri::command]
async fn app_update_check(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppUpdateStatus, String> {
    state.updates.check(&app).await
}

#[tauri::command]
async fn app_update_restart(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(reason) = state.supervisor.busy_reason().await? {
        return Err(format!("engine is busy: {reason}"));
    }

    state
        .updates
        .set_status(AppUpdateStatus {
            state: AppUpdateState::Installing,
            ..state.updates.status().await
        })
        .await;

    let updater = update_builder(&app, None)?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no app update is available".to_owned())?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let pending = snapshot_before_update(&data_dir, &update.current_version, &update.version)?;
    write_update_state(
        &data_dir,
        &DiskUpdateState {
            pending_app: Some(pending),
            rollback_to: None,
            health: "pending".to_owned(),
        },
    )?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    app.request_restart();
    Ok(())
}

#[tauri::command]
async fn app_update_rollback(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let disk_state = read_update_state(&data_dir)?.ok_or_else(|| "no rollback state".to_owned())?;
    let rollback_to = disk_state
        .rollback_to
        .clone()
        .or_else(|| {
            disk_state
                .pending_app
                .as_ref()
                .map(|pending| pending.from.clone())
        })
        .ok_or_else(|| "no rollback target".to_owned())?;

    state
        .updates
        .set_status(AppUpdateStatus {
            state: AppUpdateState::RollingBack,
            rollback_to: Some(rollback_to.clone()),
            ..state.updates.status().await
        })
        .await;

    if let Some(pending) = &disk_state.pending_app
        && let Some(backup) = &pending.db_backup
    {
        restore_db_backup(&data_dir, backup)?;
    }

    let updater = update_builder(&app, Some(rollback_to.clone()))?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("rollback artifact {rollback_to} is not available"))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    app.request_restart();
    Ok(())
}

fn main() {
    let update_policy = Arc::new(StdMutex::new(AppUpdatePolicy::default()));
    let plugin_policy = update_policy.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Err(error) = show_main_window_impl(app, None) {
                tracing::warn!(%error, "failed to focus existing instance");
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .default_version_comparator(move |current, update| {
                    plugin_policy
                        .lock()
                        .map(|policy| policy.accepts(&current, &update.version))
                        .unwrap_or(false)
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            engine_endpoint,
            show_main_window,
            set_hud_config,
            open_external,
            app_info,
            app_update_status,
            app_update_check,
            app_update_restart,
            app_update_rollback,
        ])
        .setup(move |app| {
            let supervisor = Arc::new(EngineSupervisor::new());
            let updates = Arc::new(UpdateManager::new());
            app.manage(AppState {
                supervisor: supervisor.clone(),
                updates: updates.clone(),
                hud: Arc::new(StdMutex::new(HudConfig::default())),
                paused: Arc::new(StdMutex::new(false)),
            });

            configure_windows(app.handle())?;
            install_tray(app.handle())?;
            install_shortcut(app.handle())?;
            install_deep_link_handler(app.handle());
            app.set_activation_policy(ActivationPolicy::Regular);

            let handle = app.handle().clone();
            supervisor.start(handle.clone());
            tauri::async_runtime::spawn(async move {
                run_post_update_health_check(handle, updates).await;
            });
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(error) => {
            eprintln!("failed to build Tauri app: {error}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event
            && let Some(state) = app.try_state::<AppState>()
        {
            tauri::async_runtime::block_on(state.supervisor.shutdown());
        }
    });
}

#[derive(Debug)]
struct AppUpdatePolicy {
    revoked: Vec<String>,
    rollout_pct: u8,
    rollback_to: Option<String>,
}

impl Default for AppUpdatePolicy {
    fn default() -> Self {
        Self {
            revoked: Vec::new(),
            rollout_pct: 100,
            rollback_to: None,
        }
    }
}

impl AppUpdatePolicy {
    fn accepts(&self, current: &Version, candidate: &Version) -> bool {
        let candidate_text = candidate.to_string();
        if self
            .revoked
            .iter()
            .any(|version| version == &candidate_text)
        {
            return false;
        }
        if let Some(rollback_to) = &self.rollback_to {
            return rollback_to == &candidate_text;
        }
        candidate > current && self.rollout_pct >= 100
    }
}

fn configure_windows(app: &AppHandle) -> tauri::Result<()> {
    if let Some(main) = app.get_webview_window("main") {
        let handle = app.clone();
        let main_for_event = main.clone();
        main.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = main_for_event.hide();
                let _ = handle.set_activation_policy(ActivationPolicy::Accessory);
            }
        });
    }

    if let Some(hud) = app.get_webview_window("hud") {
        hud.set_ignore_cursor_events(true)?;
        hud.set_always_on_top(true)?;
        hud.set_visible_on_all_workspaces(true)?;
        hud.set_skip_taskbar(true)?;
    }
    Ok(())
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(
        app,
        "status",
        "Watching — engine starting",
        false,
        None::<&str>,
    )?;
    let pause_15 = MenuItem::with_id(app, "pause_15", "15 min", true, None::<&str>)?;
    let pause_60 = MenuItem::with_id(app, "pause_60", "1 hour", true, None::<&str>)?;
    let pause_until = MenuItem::with_id(app, "pause_until", "Until I resume", true, None::<&str>)?;
    let pause = SubmenuBuilder::with_id(app, "pause", "Pause")
        .items(&[&pause_15, &pause_60, &pause_until])
        .build()?;
    let resume = MenuItem::with_id(app, "resume", "Resume", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Flick", true, None::<&str>)?;
    let studio = MenuItem::with_id(app, "studio", "Gesture Studio", true, None::<&str>)?;
    let activity = MenuItem::with_id(app, "activity", "Activity", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Flick", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&pause)
        .item(&resume)
        .separator()
        .item(&open)
        .item(&studio)
        .item(&activity)
        .item(&settings)
        .separator()
        .item(&update)
        .separator()
        .item(&quit)
        .build()?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .icon(template_tray_icon())
        .icon_as_template(true)
        .tooltip("Flick")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)?;
    let _tray = Box::leak(Box::new(tray));
    Ok(())
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open" => {
            let _ = show_main_window_impl(app, None);
        }
        "studio" => {
            let _ = show_main_window_impl(app, Some("/studio".to_owned()));
        }
        "activity" => {
            let _ = show_main_window_impl(app, Some("/activity".to_owned()));
        }
        "settings" => {
            let _ = show_main_window_impl(app, Some("/settings".to_owned()));
        }
        "pause_15" | "pause_60" | "pause_until" => set_pause(app, true),
        "resume" => set_pause(app, false),
        "update" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.updates.check(&app).await;
                }
            });
        }
        "quit" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app.try_state::<AppState>() {
                    state.supervisor.shutdown().await;
                }
                app.exit(0);
            });
        }
        _ => {}
    }
}

fn install_shortcut(app: &AppHandle) -> Result<(), tauri_plugin_global_shortcut::Error> {
    let shortcut = if cfg!(target_os = "macos") {
        "Alt+Command+F"
    } else {
        "Control+Alt+F"
    };
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_pause(app);
            }
        })
}

fn install_deep_link_handler(app: &AppHandle) {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            handle_deep_link(&handle, &url);
        }
    });
}

fn handle_deep_link(app: &AppHandle, url: &Url) {
    if url.scheme() != "flick" {
        return;
    }
    let route = match url.host_str() {
        Some("open") => Some(format!("/{}", url.path().trim_start_matches('/'))),
        Some("ha") => Some("/settings/home-assistant".to_owned()),
        Some("pack") => Some("/settings/updates".to_owned()),
        _ => None,
    };
    let _ = show_main_window_impl(app, route);
}

fn show_main_window_impl(app: &AppHandle, route: Option<String>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_owned())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    app.set_activation_policy(ActivationPolicy::Regular)
        .map_err(|error| error.to_string())?;
    if let Some(route) = route {
        window
            .emit("flick://route", route)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn apply_hud_config(app: &AppHandle, config: &HudConfig) -> Result<(), String> {
    let window = app
        .get_webview_window("hud")
        .ok_or_else(|| "HUD window not found".to_owned())?;
    position_hud(app, &window, config.position)?;
    if config.enabled {
        window.show().map_err(|error| error.to_string())?;
    } else {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn position_hud(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    position: HudPosition,
) -> Result<(), String> {
    let monitor = app
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no primary monitor".to_owned())?;
    let origin = monitor.position();
    let size = monitor.size();
    let width = 360i32;
    let height = 96i32;
    let margin = 24i32;
    let screen_width = i32::try_from(size.width).map_err(|error| error.to_string())?;
    let screen_height = i32::try_from(size.height).map_err(|error| error.to_string())?;
    let (x, y) = match position {
        HudPosition::TopCenter => (origin.x + (screen_width - width) / 2, origin.y + margin),
        HudPosition::TopRight => (origin.x + screen_width - width - margin, origin.y + margin),
        HudPosition::BottomCenter => (
            origin.x + (screen_width - width) / 2,
            origin.y + screen_height - height - margin,
        ),
        HudPosition::BottomRight => (
            origin.x + screen_width - width - margin,
            origin.y + screen_height - height - margin,
        ),
    };
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

fn set_pause(app: &AppHandle, paused: bool) {
    if let Some(state) = app.try_state::<AppState>()
        && let Ok(mut value) = state.paused.lock()
    {
        *value = paused;
    }
    let _ = app.emit("pause-changed", paused);
}

fn toggle_pause(app: &AppHandle) {
    let paused = app
        .try_state::<AppState>()
        .and_then(|state| state.paused.lock().ok().map(|value| !*value))
        .unwrap_or(true);
    set_pause(app, paused);
}

fn template_tray_icon() -> Image<'static> {
    let mut rgba = vec![0u8; 18 * 18 * 4];
    for y in 2usize..16 {
        for x in 5usize..13 {
            if (x > 6 && x < 11) || y > 8 {
                let idx = (y * 18 + x) * 4;
                rgba[idx + 3] = 255;
            }
        }
    }
    for (x, y) in [(4usize, 6usize), (6, 3), (8, 2), (10, 3), (12, 5)] {
        for yy in y..(y + 7).min(18) {
            for xx in x..(x + 2).min(18) {
                let idx = (yy * 18 + xx) * 4;
                rgba[idx + 3] = 255;
            }
        }
    }
    Image::new_owned(rgba, 18, 18)
}

fn generate_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

async fn terminate_child(child: CommandChild) {
    let pid = child.pid();
    terminate_pid(pid).await;
    time::sleep(Duration::from_secs(3)).await;
    if let Err(error) = child.kill() {
        tracing::debug!(%error, pid, "engine process already exited before hard kill");
    }
}

async fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        match OsCommand::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
        {
            Ok(status) if status.success() => tracing::info!(pid, "sent SIGTERM to engine"),
            Ok(status) => tracing::warn!(pid, ?status, "SIGTERM command failed"),
            Err(error) => tracing::warn!(pid, %error, "failed to send SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

fn update_builder(
    app: &AppHandle,
    rollback_to: Option<String>,
) -> Result<tauri_plugin_updater::UpdaterBuilder, String> {
    let mut builder = app
        .updater_builder()
        .version_comparator(move |current, update| {
            if let Some(rollback_to) = &rollback_to {
                return update.version.to_string() == *rollback_to;
            }
            update.version > current
        });
    if cfg!(debug_assertions)
        && let Ok(endpoint) = std::env::var("FLICK_UPDATE_URL")
        && !endpoint.is_empty()
    {
        let endpoint = normalize_update_endpoint(&endpoint)?;
        builder = builder
            .endpoints(vec![endpoint])
            .map_err(|error| error.to_string())?;
    }
    Ok(builder)
}

fn normalize_update_endpoint(endpoint: &str) -> Result<Url, String> {
    let trimmed = endpoint.trim_end_matches('/');
    let url = if trimmed.contains("{{current_version}}") || trimmed.ends_with(".json") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/app/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}?channel=stable")
    };
    Url::parse(&url).map_err(|error| error.to_string())
}

fn snapshot_before_update(
    data_dir: &Path,
    from: &str,
    to: &str,
) -> Result<PendingAppUpdate, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let db = data_dir.join("flick.db");
    let backup = if db.exists() {
        let backups = data_dir.join("backups");
        fs::create_dir_all(&backups).map_err(|error| error.to_string())?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let path = backups.join(format!("flick-{from}-{ts}.db"));
        fs::copy(&db, &path).map_err(|error| error.to_string())?;
        Some(path)
    } else {
        None
    };
    Ok(PendingAppUpdate {
        from: from.to_owned(),
        to: to.to_owned(),
        db_backup: backup,
    })
}

fn restore_db_backup(data_dir: &Path, backup: &Path) -> Result<(), String> {
    if backup.exists() {
        fs::copy(backup, data_dir.join("flick.db")).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn update_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(UPDATE_STATE_FILE)
}

fn read_update_state(data_dir: &Path) -> Result<Option<DiskUpdateState>, String> {
    let path = update_state_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn write_update_state(data_dir: &Path, state: &DiskUpdateState) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let raw = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    fs::write(update_state_path(data_dir), raw).map_err(|error| error.to_string())
}

async fn run_post_update_health_check(app: AppHandle, updates: Arc<UpdateManager>) {
    let data_dir = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(error) => {
            updates.mark_error(error.to_string()).await;
            return;
        }
    };
    let Some(mut disk_state) = (match read_update_state(&data_dir) {
        Ok(state) => state,
        Err(error) => {
            updates.mark_error(error).await;
            return;
        }
    }) else {
        return;
    };
    if disk_state.pending_app.is_none() || disk_state.health != "pending" {
        return;
    }

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut healthy = false;
    while Instant::now() < deadline {
        if let Some(state) = app.try_state::<AppState>()
            && let Some(endpoint) = state.supervisor.endpoint().await
            && state.supervisor.health_ok(&endpoint).await
        {
            healthy = true;
            break;
        }
        time::sleep(Duration::from_secs(1)).await;
    }

    if healthy {
        disk_state.health = "ok".to_owned();
        disk_state.rollback_to = None;
        if let Err(error) = write_update_state(&data_dir, &disk_state) {
            updates.mark_error(error).await;
        }
    } else if let Some(pending) = disk_state.pending_app.clone() {
        disk_state.health = "failed".to_owned();
        disk_state.rollback_to = Some(pending.from.clone());
        if let Err(error) = write_update_state(&data_dir, &disk_state) {
            updates.mark_error(error).await;
            return;
        }
        updates
            .set_status(AppUpdateStatus {
                state: AppUpdateState::RollbackAvailable,
                available: None,
                downloaded: false,
                rollback_to: Some(pending.from),
                error: Some("post-update health check failed".to_owned()),
            })
            .await;
    }
}
