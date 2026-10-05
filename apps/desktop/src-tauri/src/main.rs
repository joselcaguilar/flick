#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command as OsCommand,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use semver::Version;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WindowEvent,
    image::Image,
    menu::{MenuBuilder, MenuItem, SubmenuBuilder},
    tray::TrayIconBuilder,
};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};
use tauri_plugin_updater::UpdaterExt;
use tauri_plugin_window_state::StateFlags;
use tokio::{
    sync::{Mutex as AsyncMutex, Notify},
    time,
};
use url::Url;

const SIDECAR_NAME: &str = "flick-engine";
const UPDATE_STATE_FILE: &str = "update_state.json";
const SHELL_PREFS_FILE: &str = "shell_prefs.json";
/// Passed by the Windows and Linux autostart entry so Flick starts without opening its window.
const HIDDEN_LAUNCH_ARG: &str = "--hidden";
const BACKOFF_RESET_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct AppState {
    supervisor: Arc<EngineSupervisor>,
    updates: Arc<UpdateManager>,
    hud: Arc<StdMutex<HudConfig>>,
    menu_bar: Arc<AtomicBool>,
}

/// Shell-only preferences, read before the webview loads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ShellPrefs {
    menu_bar: bool,
}

impl Default for ShellPrefs {
    fn default() -> Self {
        Self { menu_bar: true }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AppPreferences {
    menu_bar: bool,
    open_at_login: bool,
}

#[derive(Debug, Deserialize)]
struct AppPreferencesPatch {
    menu_bar: Option<bool>,
    open_at_login: Option<bool>,
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
    retry: Notify,
}

#[derive(Debug, Deserialize)]
struct ReadyLine {
    event: String,
    port: u16,
    version: String,
}

#[derive(Debug, Deserialize)]
struct EnginePauseState {
    paused: bool,
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
            retry: Notify::new(),
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
            if self.is_fatal().await {
                self.wait_for_retry_or_shutdown().await;
                backoff = Duration::from_millis(500);
                continue;
            }

            self.set_status(&app, EngineStatus::Starting, None).await;
            match self.spawn_once(&app).await {
                Ok(healthy_for) => {
                    if healthy_for >= BACKOFF_RESET_AFTER {
                        backoff = Duration::from_millis(500);
                    }
                }
                Err(error) => {
                    if self.is_shutting_down().await {
                        self.clear_running(EngineStatus::Stopped).await;
                    } else {
                        self.record_crash(&app, error.to_string()).await;
                    }
                }
            }

            if self.is_shutting_down().await {
                continue;
            }
            if self.is_fatal().await {
                continue;
            }

            self.set_status(&app, EngineStatus::Restarting, None).await;
            let sleeper = time::sleep(backoff);
            tokio::pin!(sleeper);
            tokio::select! {
                () = &mut sleeper => {
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
                () = self.retry.notified() => {}
            }
        }
    }

    async fn spawn_once(&self, app: &AppHandle) -> anyhow::Result<Duration> {
        let token = generate_token()?;
        let data_dir = app.path().app_data_dir()?;
        fs::create_dir_all(&data_dir)?;
        let data_dir_arg = data_dir.to_string_lossy().to_string();
        let models_dir = sidecar_models_dir(app)?;
        let (mut rx, child) = app
            .shell()
            .sidecar(SIDECAR_NAME)?
            .args(["--sidecar", "--port", "0", "--data-dir", &data_dir_arg])
            .env("FLICK_MODELS_DIR", models_dir)
            .spawn()?;
        let mut child = Some(child);
        let pid = child
            .as_ref()
            .map(CommandChild::pid)
            .ok_or_else(|| anyhow::anyhow!("engine child was not captured"))?;
        {
            let mut inner = self.inner.lock().await;
            inner.pid = Some(pid);
        }

        if let Err(error) = child
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("engine child was not captured"))?
            .write(format!("{token}\n").as_bytes())
        {
            terminate_child_slot(&mut child).await;
            return Err(error.into());
        }

        let ready = match self.wait_ready(&mut rx).await {
            Ok(ready) => ready,
            Err(error) => {
                terminate_child_slot(&mut child).await;
                return Err(error);
            }
        };
        if ready.event != "ready" {
            terminate_child_slot(&mut child).await;
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
        println!("engine sidecar ready: {}", endpoint.base_url);
        let _ = app.emit("engine-status", EngineStatus::Ready);
        let child = child
            .take()
            .ok_or_else(|| anyhow::anyhow!("engine child was not captured"))?;
        Ok(self.monitor(app, endpoint, child, rx).await)
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
    ) -> Duration {
        let ready_at = Instant::now();
        let mut healthy_until = ready_at;
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
                            terminate_child_slot(&mut child).await;
                            self.record_crash(app, error).await;
                            break;
                        }
                        Some(_) => {}
                        None => {
                            terminate_child_slot(&mut child).await;
                            self.record_crash(app, "engine event stream closed".to_owned()).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if self.health_ok(&endpoint).await {
                        failed_health_checks = 0;
                        healthy_until = Instant::now();
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
        healthy_until.duration_since(ready_at)
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

    async fn set_paused(&self, command: PauseCommand) -> Result<bool, String> {
        let endpoint = self
            .endpoint()
            .await
            .ok_or_else(|| "engine is not ready".to_owned())?;
        let command = match command {
            PauseCommand::Toggle => {
                let status = self
                    .client
                    .get(format!("{}/api/v1/status", endpoint.base_url))
                    .bearer_auth(&endpoint.token)
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|error| error.to_string())?
                    .json::<EnginePauseState>()
                    .await
                    .map_err(|error| error.to_string())?;
                if status.paused {
                    PauseCommand::Resume
                } else {
                    PauseCommand::Pause(None)
                }
            }
            command => command,
        };
        let request = match command {
            PauseCommand::Pause(duration_s) => self
                .client
                .post(format!("{}/api/v1/engine/pause", endpoint.base_url))
                .json(&serde_json::json!({ "duration_s": duration_s })),
            _ => self
                .client
                .post(format!("{}/api/v1/engine/resume", endpoint.base_url)),
        };
        let status = request
            .bearer_auth(&endpoint.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?
            .json::<EnginePauseState>()
            .await
            .map_err(|error| error.to_string())?;
        Ok(status.paused)
    }

    async fn report_network(&self, ssid: Option<&str>) -> Result<(), String> {
        let endpoint = self
            .endpoint()
            .await
            .ok_or_else(|| "engine is not ready".to_owned())?;
        self.client
            .put(format!("{}/api/v1/network", endpoint.base_url))
            .bearer_auth(&endpoint.token)
            .json(&serde_json::json!({ "ssid": ssid }))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?;
        Ok(())
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

    async fn retry(&self, app: &AppHandle) {
        let should_emit = {
            let mut inner = self.inner.lock().await;
            if inner.status == EngineStatus::Fatal {
                inner.crashes.clear();
                inner.last_error = None;
                inner.status = EngineStatus::Restarting;
                true
            } else {
                false
            }
        };
        if should_emit {
            let _ = app.emit("engine-status", EngineStatus::Restarting);
        }
        self.retry.notify_waiters();
    }

    async fn shutdown(&self) {
        let pid = {
            let mut inner = self.inner.lock().await;
            inner.shutting_down = true;
            inner.pid
        };
        self.retry.notify_waiters();
        if let Some(pid) = pid {
            terminate_pid(pid).await;
        }
    }

    async fn shutdown_and_wait(&self, timeout: Duration) -> Result<(), String> {
        self.shutdown().await;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.inner.lock().await.pid.is_none() {
                return Ok(());
            }
            time::sleep(Duration::from_millis(100)).await;
        }
        Err("engine did not exit before timeout".to_owned())
    }

    async fn wait_for_retry_or_shutdown(&self) {
        loop {
            let notified = self.retry.notified();
            if self.is_shutting_down().await || !self.is_fatal().await {
                return;
            }
            notified.await;
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
        eprintln!("engine sidecar crashed: {error}");
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
async fn engine_retry(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.retry(&app).await;
    Ok(())
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
fn camera_permission_status() -> String {
    current_camera_permission_status().to_owned()
}

#[tauri::command]
async fn camera_request_access(app: AppHandle) -> Result<String, String> {
    if current_camera_permission_status() == "not_determined" {
        show_main_window_impl(&app, None)?;
    }
    request_camera_access_impl().await
}

#[tauri::command]
fn open_camera_privacy_settings(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
            None::<String>,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn location_permission_status(app: AppHandle) -> Result<String, String> {
    location_permission_status_impl(&app).await
}

#[tauri::command]
async fn location_request_access(app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    if location_permission_status_impl(&app).await? == "not_determined" {
        show_main_window_impl(&app, None)?;
        on_main_thread(&app, wifi::request_access).await?;
    }
    location_permission_status_impl(&app).await
}

#[tauri::command]
fn open_location_privacy_settings(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices",
            None::<String>,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn current_wifi_ssid() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(read_wifi_ssid)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn app_preferences(app: AppHandle, state: State<'_, AppState>) -> Result<AppPreferences, String> {
    current_app_preferences(&app, &state)
}

#[tauri::command]
fn set_app_preferences(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: AppPreferencesPatch,
) -> Result<AppPreferences, String> {
    if let Some(menu_bar) = patch.menu_bar {
        write_shell_prefs(&app, ShellPrefs { menu_bar })?;
        state.menu_bar.store(menu_bar, Ordering::Relaxed);
        set_menu_bar_visible(&app, menu_bar).map_err(|error| error.to_string())?;
    }
    if let Some(open_at_login) = patch.open_at_login
        && open_at_login != open_at_login_enabled(&app)?
    {
        set_open_at_login(&app, open_at_login)?;
    }
    current_app_preferences(&app, &state)
}

fn current_app_preferences(app: &AppHandle, state: &AppState) -> Result<AppPreferences, String> {
    Ok(AppPreferences {
        menu_bar: state.menu_bar.load(Ordering::Relaxed),
        open_at_login: open_at_login_enabled(app)?,
    })
}

#[cfg(target_os = "macos")]
fn open_at_login_enabled(_app: &AppHandle) -> Result<bool, String> {
    Ok(login_item::is_enabled())
}

#[cfg(not(target_os = "macos"))]
fn open_at_login_enabled(app: &AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn set_open_at_login(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    login_item::set_enabled(enabled)
}

#[cfg(not(target_os = "macos"))]
fn set_open_at_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|error| error.to_string())
}

/// macOS registers Flick.app itself as a login item, so System Settings → General → Login Items
/// lists it under "Open at Login" with Flick's name and icon. Other platforms use the autostart plugin.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // SMAppService has no safe binding.
mod login_item {
    use objc2_foundation::NSAppleEventManager;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    const OPEN_APPLICATION: u32 = u32::from_be_bytes(*b"oapp");
    const PROP_DATA: u32 = u32::from_be_bytes(*b"prdt");
    const LAUNCHED_AS_LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");

    pub fn is_enabled() -> bool {
        // SAFETY: `mainAppService` and `status` have no preconditions.
        unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        // SAFETY: registering or unregistering the main app has no preconditions.
        let result = unsafe {
            let service = SMAppService::mainAppService();
            if enabled {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
        };
        result.map_err(|error| error.localizedDescription().to_string())?;
        if enabled && !is_enabled() {
            // SAFETY: opens System Settings; no preconditions.
            unsafe { SMAppService::openSystemSettingsLoginItems() };
            return Err(
                "macOS needs your OK. Turn on Flick in System Settings → General → Login Items."
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// True when macOS opened Flick as a login item. Only meaningful while the app is launching.
    pub fn launched_at_login() -> bool {
        let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
            return false;
        };
        event.eventID() == OPEN_APPLICATION
            && event
                .paramDescriptorForKeyword(PROP_DATA)
                .is_some_and(|value| value.enumCodeValue() == LAUNCHED_AS_LOGIN_ITEM)
    }

    /// Earlier builds wrote a LaunchAgent, which System Settings lists as "flick-desktop" under
    /// "Allow in the Background". Replace it with the login item, keeping the user's choice.
    pub fn migrate_launch_agent(app: &tauri::AppHandle) {
        use tauri_plugin_autostart::ManagerExt as _;
        let autolaunch = app.autolaunch();
        if autolaunch.is_enabled().unwrap_or(false) && autolaunch.disable().is_ok() {
            let _ = set_enabled(true);
        }
    }
}

fn read_shell_prefs(app: &AppHandle) -> ShellPrefs {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|dir| fs::read(dir.join(SHELL_PREFS_FILE)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_shell_prefs(app: &AppHandle, prefs: ShellPrefs) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&prefs).map_err(|error| error.to_string())?;
    fs::write(dir.join(SHELL_PREFS_FILE), bytes).map_err(|error| error.to_string())
}

/// Login launches start without the window. macOS login items can't pass arguments, so ask
/// AppKit how Flick was opened; the autostart plugin passes `--hidden` elsewhere.
fn launched_hidden() -> bool {
    #[cfg(target_os = "macos")]
    if login_item::launched_at_login() {
        return true;
    }
    std::env::args().any(|arg| arg == HIDDEN_LAUNCH_ARG)
}

fn set_menu_bar_visible(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_visible(visible)?;
    }
    Ok(())
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

    let updater = update_builder(&app, Some(rollback_to.clone()))?
        .build()
        .map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("rollback artifact {rollback_to} is not available"))?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    state
        .supervisor
        .shutdown_and_wait(Duration::from_secs(15))
        .await?;
    if let Some(pending) = &disk_state.pending_app
        && let Some(backup) = &pending.db_backup
    {
        restore_db_backup(&data_dir, backup)?;
    }
    update.install(bytes).map_err(|error| error.to_string())?;
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
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_LAUNCH_ARG]),
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
            engine_retry,
            show_main_window,
            set_hud_config,
            open_external,
            camera_permission_status,
            camera_request_access,
            open_camera_privacy_settings,
            location_permission_status,
            location_request_access,
            open_location_privacy_settings,
            current_wifi_ssid,
            app_preferences,
            set_app_preferences,
            app_info,
            app_update_status,
            app_update_check,
            app_update_restart,
            app_update_rollback,
        ])
        .setup(move |app| {
            let supervisor = Arc::new(EngineSupervisor::new());
            let updates = Arc::new(UpdateManager::new());
            let prefs = read_shell_prefs(app.handle());
            let menu_bar = Arc::new(AtomicBool::new(prefs.menu_bar));
            app.manage(AppState {
                supervisor: supervisor.clone(),
                updates: updates.clone(),
                hud: Arc::new(StdMutex::new(HudConfig::default())),
                menu_bar: menu_bar.clone(),
            });

            configure_windows(app.handle(), menu_bar)?;
            install_tray(app.handle())?;
            set_menu_bar_visible(app.handle(), prefs.menu_bar)?;
            install_shortcut(app.handle())?;
            install_deep_link_handler(app.handle());
            #[cfg(target_os = "macos")]
            login_item::migrate_launch_agent(app.handle());
            let hidden = launched_hidden();
            #[cfg(target_os = "macos")]
            app.set_activation_policy(if hidden && prefs.menu_bar {
                ActivationPolicy::Accessory
            } else {
                ActivationPolicy::Regular
            });
            if !hidden && let Some(main) = app.get_webview_window("main") {
                main.show()?;
                main.set_focus()?;
            }

            let handle = app.handle().clone();
            start_network_watch(supervisor.clone());
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

    app.run(|app, event| match event {
        RunEvent::ExitRequested { .. } => {
            if let Some(state) = app.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.supervisor.shutdown());
            }
        }
        // Clicking the Dock icon reopens the window when Flick lives in the Dock.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => {
            let _ = show_main_window_impl(app, None);
        }
        _ => {}
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

fn configure_windows(app: &AppHandle, menu_bar: Arc<AtomicBool>) -> tauri::Result<()> {
    if let Some(main) = app.get_webview_window("main") {
        let handle = app.clone();
        let main_for_event = main.clone();
        main.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = main_for_event.hide();
                if menu_bar.load(Ordering::Relaxed) {
                    leave_dock(&handle);
                }
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

/// Drops the Dock icon while Flick lives in the menu bar. Elsewhere, hiding the window already
/// removes its taskbar entry.
#[cfg(target_os = "macos")]
fn leave_dock(app: &AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
}

#[cfg(not(target_os = "macos"))]
fn leave_dock(_app: &AppHandle) {}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(
        app,
        "status",
        "Watching — engine starting",
        false,
        None::<&str>,
    )?;
    let retry_engine = MenuItem::with_id(app, "retry_engine", "Retry engine", true, None::<&str>)?;
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
        .item(&retry_engine)
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
        .icon(template_tray_icon()?)
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
        "pause_15" => request_pause(app, PauseCommand::Pause(Some(15 * 60))),
        "pause_60" => request_pause(app, PauseCommand::Pause(Some(60 * 60))),
        "pause_until" => request_pause(app, PauseCommand::Pause(None)),
        "resume" => request_pause(app, PauseCommand::Resume),
        "retry_engine" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app.try_state::<AppState>() {
                    state.supervisor.retry(&app).await;
                }
            });
        }
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
                request_pause(app, PauseCommand::Toggle);
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
    #[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn current_camera_permission_status() -> &'static str {
    macos_camera_status(nokhwa_bindings_macos::current_authorization_status())
}

#[cfg(not(target_os = "macos"))]
fn current_camera_permission_status() -> &'static str {
    "authorized"
}

#[cfg(target_os = "macos")]
async fn request_camera_access_impl() -> Result<String, String> {
    use nokhwa_bindings_macos::AVAuthorizationStatus;

    if nokhwa_bindings_macos::current_authorization_status() != AVAuthorizationStatus::NotDetermined
    {
        return Ok(current_camera_permission_status().to_owned());
    }

    let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
    let sender = Arc::new(StdMutex::new(Some(sender)));
    nokhwa_bindings_macos::request_permission_with_callback({
        let sender = sender.clone();
        move |_| {
            if let Ok(mut sender) = sender.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        }
    });

    receiver
        .await
        .map_err(|_| "camera permission request was canceled".to_owned())?;
    Ok(current_camera_permission_status().to_owned())
}

#[cfg(not(target_os = "macos"))]
async fn request_camera_access_impl() -> Result<String, String> {
    Ok(current_camera_permission_status().to_owned())
}

#[cfg(target_os = "macos")]
fn macos_camera_status(status: nokhwa_bindings_macos::AVAuthorizationStatus) -> &'static str {
    use nokhwa_bindings_macos::AVAuthorizationStatus;

    match status {
        AVAuthorizationStatus::NotDetermined => "not_determined",
        AVAuthorizationStatus::Restricted => "restricted",
        AVAuthorizationStatus::Denied => "denied",
        AVAuthorizationStatus::Authorized => "authorized",
    }
}

#[cfg(target_os = "macos")]
async fn location_permission_status_impl(app: &AppHandle) -> Result<String, String> {
    on_main_thread(app, || wifi::permission_status().to_owned()).await
}

#[cfg(not(target_os = "macos"))]
async fn location_permission_status_impl(_app: &AppHandle) -> Result<String, String> {
    Ok("unsupported".to_owned())
}

#[cfg(target_os = "macos")]
async fn on_main_thread<T: Send + 'static>(
    app: &AppHandle,
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = sender.send(task());
    })
    .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "main thread task was canceled".to_owned())
}

#[cfg(target_os = "macos")]
fn read_wifi_ssid() -> Option<String> {
    wifi::current_ssid()
}

#[cfg(not(target_os = "macos"))]
fn read_wifi_ssid() -> Option<String> {
    None
}

/// Wi-Fi network name and the Location permission macOS requires before revealing it.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // CoreLocation and CoreWLAN have no safe bindings.
mod wifi {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};
    use objc2_core_wlan::CWWiFiClient;

    thread_local! {
        // CoreLocation wants one long-lived manager on the main thread.
        static LOCATION: RefCell<Option<Retained<CLLocationManager>>> = const { RefCell::new(None) };
    }

    fn with_manager<T>(task: impl FnOnce(&CLLocationManager) -> T) -> T {
        LOCATION.with(|cell| {
            let mut cell = cell.borrow_mut();
            // SAFETY: called on the main thread; the manager is retained for the app's lifetime.
            let manager = cell.get_or_insert_with(|| unsafe { CLLocationManager::new() });
            task(manager)
        })
    }

    /// Main thread only.
    pub fn permission_status() -> &'static str {
        // SAFETY: plain property read on a live manager.
        match with_manager(|manager| unsafe { manager.authorizationStatus() }) {
            CLAuthorizationStatus::NotDetermined => "not_determined",
            CLAuthorizationStatus::Restricted => "restricted",
            CLAuthorizationStatus::Denied => "denied",
            _ => "authorized",
        }
    }

    /// Main thread only. macOS answers asynchronously; callers poll `permission_status`.
    pub fn request_access() {
        // SAFETY: the manager stays alive while the system prompt is shown.
        with_manager(|manager| unsafe { manager.requestWhenInUseAuthorization() });
    }

    /// None without Location permission or when not on Wi-Fi.
    pub fn current_ssid() -> Option<String> {
        // SAFETY: CWWiFiClient is thread-safe and these are read-only queries.
        let ssid = unsafe {
            let interface = CWWiFiClient::sharedWiFiClient().interface()?;
            interface.ssid()?
        }
        .to_string();
        let ssid = ssid.trim();
        (!ssid.is_empty()).then(|| ssid.to_owned())
    }
}

/// Tells the engine which Wi-Fi network this computer is on, so it can choose between the Home
/// Assistant home and remote URLs. Sends on change, on engine restart, and every 30 seconds.
fn start_network_watch(supervisor: Arc<EngineSupervisor>) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<(String, Option<String>)> = None;
        let mut last_sent = Instant::now();
        let mut ticker = time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let Some(endpoint) = supervisor.endpoint().await else {
                last = None;
                continue;
            };
            let ssid = tauri::async_runtime::spawn_blocking(read_wifi_ssid)
                .await
                .unwrap_or(None);
            let current = (endpoint.base_url, ssid);
            if last.as_ref() == Some(&current) && last_sent.elapsed() < Duration::from_secs(30) {
                continue;
            }
            match supervisor.report_network(current.1.as_deref()).await {
                Ok(()) => {
                    last = Some(current);
                    last_sent = Instant::now();
                }
                Err(error) => tracing::debug!(%error, "network report failed"),
            }
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum PauseCommand {
    Pause(Option<u64>),
    Resume,
    Toggle,
}

/// The engine owns pause state; the tray and shortcut only ask it to change.
fn request_pause(app: &AppHandle, command: PauseCommand) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(error) = state.supervisor.set_paused(command).await {
            tracing::warn!(%error, ?command, "pause request failed");
        }
    });
}

fn template_tray_icon() -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/tray/tray-template@2x.png"))
}

fn generate_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn sidecar_models_dir(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let resource_models = app.path().resource_dir()?.join("models");
    if resource_models.join("manifest.toml").exists() {
        return Ok(resource_models);
    }

    let repo_models = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("models");
    if repo_models.join("manifest.toml").exists() {
        return Ok(repo_models);
    }

    Ok(resource_models)
}

async fn terminate_child(child: CommandChild) {
    let pid = child.pid();
    terminate_pid(pid).await;
    time::sleep(Duration::from_secs(3)).await;
    if let Err(error) = child.kill() {
        tracing::debug!(%error, pid, "engine process already exited before hard kill");
    }
}

async fn terminate_child_slot(child: &mut Option<CommandChild>) {
    if let Some(child) = child.take() {
        terminate_child(child).await;
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
        Some(flick_store::snapshot_database(&db, &backups).map_err(|error| error.to_string())?)
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
        let db = data_dir.join("flick.db");
        remove_file_if_exists(sqlite_sidecar_path(&db, "-wal"))?;
        remove_file_if_exists(sqlite_sidecar_path(&db, "-shm"))?;
        fs::copy(backup, &db).map_err(|error| error.to_string())?;
        remove_file_if_exists(sqlite_sidecar_path(&db, "-wal"))?;
        remove_file_if_exists(sqlite_sidecar_path(&db, "-shm"))?;
    }
    Ok(())
}

fn sqlite_sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    let name = db
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    db.with_file_name(format!("{name}{suffix}"))
}

fn remove_file_if_exists(path: PathBuf) -> Result<(), String> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
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
