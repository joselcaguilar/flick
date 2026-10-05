//! Axum REST, WebSocket events, MJPEG preview and OpenAPI for `06-data-model-and-api.md` §§5–6.
//!
//! # Engine boundary traits
//!
//! `flick-api` intentionally does not depend on the vision, gestures, spatial, Home Assistant or
//! update crates. The engine wires those lanes in by implementing these traits:
//!
//! - [`EngineControl`]: pause/resume/status/debug injection and camera runtime control.
//! - [`HaGateway`]: HA discovery, connect/status, picker caches, service calls and test calls.
//! - [`TeachGateway`]: teach and realign capture flows; persistence is supplied by the store.
//! - [`UpdatesGateway`]: update and model-pack status/check/install/rollback operations.
//! - [`PreviewSource`]: raw RGB preview frames; the API calls it only while an MJPEG viewer is
//!   connected, so there is no JPEG encoding cost when nobody is watching.
//!
//! The crate includes in-memory fakes for tests and `--dev` fake mode. SQLite persistence lives in
//! `flick-store`; async API handlers call it through a small `spawn_blocking` wrapper so rusqlite's
//! synchronous connection never blocks the Tokio reactor.

mod dto;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use bytes::Bytes;
use futures::{SinkExt, Stream, StreamExt, stream};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use ulid::Ulid;
use utoipa::OpenApi;

pub use dto::*;

const API_VERSION: &str = "v1";
const DEFAULT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// API server configuration.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Port used for Host allowlisting.
    pub port: u16,
    /// Per-launch bearer token.
    pub token: String,
    /// Allowed Host headers.
    pub allowed_hosts: HashSet<String>,
    /// Allowed Origin headers.
    pub allowed_origins: HashSet<String>,
    /// Whether debug/dev mode is enabled.
    pub dev: bool,
    /// Whether `/health` requires the launch bearer token.
    pub protect_health: bool,
}

impl ApiConfig {
    /// Creates production-like loopback config for the supplied port/token.
    #[must_use]
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        let mut allowed_hosts = HashSet::new();
        allowed_hosts.insert(format!("127.0.0.1:{port}"));
        allowed_hosts.insert(format!("localhost:{port}"));
        let mut allowed_origins = HashSet::new();
        allowed_origins.insert("tauri://localhost".to_owned());
        allowed_origins.insert("http://tauri.localhost".to_owned());
        allowed_origins.insert(format!("http://127.0.0.1:{port}"));
        allowed_origins.insert(format!("http://localhost:{port}"));
        Self {
            port,
            token: token.into(),
            allowed_hosts,
            allowed_origins,
            dev: false,
            protect_health: false,
        }
    }

    /// Creates `--dev` config: port 7871, token `dev-token`, Vite CORS origin and debug routes.
    #[must_use]
    pub fn dev() -> Self {
        let mut config = Self::new(7871, "dev-token");
        config.dev = true;
        config
            .allowed_origins
            .insert("http://localhost:5173".to_owned());
        config
    }

    /// Loopback socket address the engine should bind for this API config.
    #[must_use]
    pub fn bind_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

/// Runtime dependencies supplied by the engine integration layer.
#[derive(Clone)]
pub struct ApiGateways {
    /// Engine control boundary.
    pub engine: Arc<dyn EngineControl>,
    /// Home Assistant gateway boundary.
    pub ha: Arc<dyn HaGateway>,
    /// Teach/realign gateway boundary.
    pub teach: Arc<dyn TeachGateway>,
    /// Updates gateway boundary.
    pub updates: Arc<dyn UpdatesGateway>,
    /// Preview frame source.
    pub preview: Arc<dyn PreviewSource>,
    /// Mapping/settings persistence and dispatcher reload boundary.
    pub config: Arc<dyn ConfigGateway>,
}

impl ApiGateways {
    /// In-memory fake gateways used by tests and `--dev`.
    #[must_use]
    pub fn fake() -> Self {
        Self {
            engine: Arc::new(FakeEngine::default()),
            ha: Arc::new(FakeHa::default()),
            teach: Arc::new(FakeTeach),
            updates: Arc::new(FakeUpdates),
            preview: Arc::new(FakePreview::default()),
            config: Arc::new(FakeConfig),
        }
    }
}

/// API state shared by handlers.
#[derive(Clone)]
pub struct ApiState {
    config: ApiConfig,
    started_at: Instant,
    engine: Arc<dyn EngineControl>,
    ha: Arc<dyn HaGateway>,
    teach: Arc<dyn TeachGateway>,
    updates: Arc<dyn UpdatesGateway>,
    preview: Arc<dyn PreviewSource>,
    config_sync: Arc<dyn ConfigGateway>,
    tickets: Arc<PreviewTickets>,
    events: EventHub,
    mappings: Arc<Mutex<BTreeMap<String, Mapping>>>,
    places: Arc<Mutex<BTreeMap<String, Place>>>,
    anchors: Arc<Mutex<BTreeMap<String, Anchor>>>,
    settings: Arc<Mutex<SettingsMap>>,
}

impl ApiState {
    /// Builds API state from engine-supplied gateway implementations.
    #[must_use]
    pub fn new(config: ApiConfig, gateways: ApiGateways) -> Self {
        Self {
            config,
            started_at: Instant::now(),
            engine: gateways.engine,
            ha: gateways.ha,
            teach: gateways.teach,
            updates: gateways.updates,
            preview: gateways.preview,
            config_sync: gateways.config,
            tickets: Arc::new(PreviewTickets::default()),
            // Live hands and targeting events are bursty; keep room so slow sockets don't resync.
            events: EventHub::new(256),
            mappings: Arc::new(Mutex::new(BTreeMap::new())),
            places: Arc::new(Mutex::new(BTreeMap::new())),
            anchors: Arc::new(Mutex::new(BTreeMap::new())),
            settings: Arc::new(Mutex::new(default_settings())),
        }
    }

    /// Builds API state with in-memory fakes.
    #[must_use]
    pub fn fake(config: ApiConfig) -> Self {
        Self::new(config, ApiGateways::fake())
    }

    /// Returns the event hub used to publish server-side events.
    #[must_use]
    pub fn events(&self) -> EventHub {
        self.events.clone()
    }

    /// Seeds the in-memory mapping cache from engine persistence.
    pub fn replace_mappings(&self, mappings: Vec<Mapping>) {
        *self.mappings.lock().unwrap_or_else(|err| err.into_inner()) = mappings
            .into_iter()
            .map(|mapping| (mapping.id.clone(), mapping))
            .collect();
    }

    /// Seeds the in-memory anchor cache from engine persistence.
    pub fn replace_anchors(&self, anchors: Vec<Anchor>) {
        *self.anchors.lock().unwrap_or_else(|err| err.into_inner()) = anchors
            .into_iter()
            .map(|anchor| (anchor.id.clone(), anchor))
            .collect();
    }

    /// Seeds the in-memory settings cache from engine persistence.
    pub fn replace_settings(&self, settings: SettingsMap) {
        *self.settings.lock().unwrap_or_else(|err| err.into_inner()) = settings;
    }

    /// Returns a cloned settings snapshot.
    #[must_use]
    pub fn settings_snapshot(&self) -> SettingsMap {
        self.settings
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

/// Builds the axum router for the local API.
#[must_use = "the router must be served or exercised by tests"]
pub fn router(state: ApiState) -> Router {
    let state = Arc::new(state);
    let allowed_origins = state.config.allowed_origins.clone();
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_origin(AllowOrigin::predicate(move |origin, _request_head| {
            origin
                .to_str()
                .is_ok_and(|origin| allowed_origins.contains(origin))
        }));

    Router::new()
        .route("/health", get(health))
        .route("/updates", get(sidecar_updates))
        .route("/stream/{*camera_path}", get(stream_mjpeg))
        .route("/api/v1/openapi.json", get(openapi_json))
        .route("/api/v1/events", get(ws_events))
        .route("/api/v1/status", get(status))
        .route("/api/v1/engine/pause", post(pause_engine))
        .route("/api/v1/engine/resume", post(resume_engine))
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .route("/api/v1/ha/discover", get(ha_discover))
        .route("/api/v1/ha/connect", post(ha_connect))
        .route("/api/v1/ha/status", get(ha_status))
        .route("/api/v1/ha", delete(ha_delete).patch(ha_update))
        .route(
            "/api/v1/ha/client-certificate",
            get(ha_client_certificate)
                .put(put_ha_client_certificate)
                .delete(delete_ha_client_certificate),
        )
        .route("/api/v1/network", put(put_network))
        .route("/api/v1/ha/areas", get(ha_areas))
        .route("/api/v1/ha/entities", get(ha_entities))
        .route("/api/v1/ha/services", get(ha_services))
        .route("/api/v1/ha/call", post(ha_call))
        .route("/api/v1/cameras/available", get(cameras_available))
        .route("/api/v1/cameras", get(list_cameras).post(create_camera))
        .route(
            "/api/v1/cameras/{id}",
            patch(patch_camera).delete(delete_camera),
        )
        .route("/api/v1/cameras/{id}/start", post(start_camera))
        .route("/api/v1/cameras/{id}/stop", post(stop_camera))
        .route("/api/v1/cameras/{id}/preview-ticket", post(preview_ticket))
        .route("/api/v1/gestures", get(list_gestures).post(create_gesture))
        .route(
            "/api/v1/gestures/{id}",
            patch(patch_gesture).delete(delete_gesture),
        )
        .route("/api/v1/gestures/{id}/capture", post(capture_gesture))
        .route("/api/v1/gestures/{id}/motion-takes", get(motion_takes))
        .route("/api/v1/gestures/{id}/type", patch(patch_gesture_type))
        .route("/api/v1/capture/{session_id}/cancel", post(cancel_capture))
        .route("/api/v1/gestures/{id}/samples", get(gesture_samples))
        .route(
            "/api/v1/gestures/{id}/samples/{sample_id}",
            delete(delete_sample),
        )
        .route("/api/v1/classifier/train", post(train_classifier))
        .route("/api/v1/classifier", get(get_classifier))
        .route("/api/v1/mappings", get(list_mappings).post(create_mapping))
        .route(
            "/api/v1/mappings/{id}",
            patch(patch_mapping).delete(delete_mapping),
        )
        .route("/api/v1/mappings/{id}/test", post(test_mapping))
        .route("/api/v1/mappings/order", put(order_mappings))
        .route("/api/v1/activity", get(activity))
        .route("/api/v1/places", get(list_places))
        .route(
            "/api/v1/places/{id}",
            patch(patch_place).delete(delete_place),
        )
        .route("/api/v1/places/{id}/realign", post(start_realign))
        .route("/api/v1/realign/{session_id}/point", post(realign_point))
        .route("/api/v1/realign/{session_id}/commit", post(realign_commit))
        .route("/api/v1/anchors", get(list_anchors))
        .route(
            "/api/v1/anchors/{id}",
            patch(patch_anchor).delete(delete_anchor),
        )
        .route("/api/v1/anchors/{id}/test", post(test_anchor))
        .route("/api/v1/teach", post(start_teach))
        .route("/api/v1/teach/{session_id}/spot", post(teach_spot))
        .route(
            "/api/v1/teach/{session_id}/levels/use-current",
            post(teach_level_current),
        )
        .route(
            "/api/v1/teach/{session_id}/levels/test",
            post(teach_level_test),
        )
        .route("/api/v1/teach/{session_id}/commit", post(teach_commit))
        .route("/api/v1/teach/{session_id}/cancel", post(teach_cancel))
        .route("/api/v1/setup-assistant/suggest", post(setup_suggest))
        .route("/api/v1/updates", get(get_updates))
        .route("/api/v1/updates/check", post(check_updates))
        .route("/api/v1/updates/install", post(install_update))
        .route("/api/v1/updates/rollback", post(rollback_update))
        .route("/api/v1/models", get(get_models))
        .route("/api/v1/models/{pack_id}/install", post(install_model))
        .route("/api/v1/models/{pack_id}", delete(delete_model))
        .route("/api/v1/packs/export", post(export_pack))
        .route("/api/v1/diagnostics/bundle", get(diagnostics_bundle))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .layer(cors)
        .with_state(state)
}

async fn security_middleware(
    State(state): State<Arc<ApiState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiProblem> {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    check_host(&state, request.headers())?;
    check_origin(&state, request.headers())?;
    if method != Method::OPTIONS && requires_bearer(&state, &path) {
        check_bearer(&state, request.headers())?;
    }
    Ok(next.run(request).await)
}

fn requires_bearer(state: &ApiState, path: &str) -> bool {
    if path == "/health" {
        state.config.protect_health
    } else {
        !path.starts_with("/stream/") && path != "/api/v1/events"
    }
}

fn check_host(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiProblem> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiProblem::forbidden("bad_host", "Host header is required"))?;
    if state.config.allowed_hosts.contains(host) {
        Ok(())
    } else {
        Err(ApiProblem::forbidden("bad_host", "Host is not allowed"))
    }
}

fn check_origin(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiProblem> {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    if state.config.allowed_origins.contains(origin) {
        Ok(())
    } else {
        Err(ApiProblem::forbidden("bad_origin", "Origin is not allowed"))
    }
}

fn check_bearer(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiProblem> {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiProblem::unauthorized(
            "missing_token",
            "Authorization bearer token is required",
        ));
    };
    let expected = format!("Bearer {}", state.config.token);
    if constant_time_eq(value, &expected) {
        Ok(())
    } else {
        Err(ApiProblem::unauthorized(
            "bad_token",
            "Bearer token is invalid",
        ))
    }
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

/// Local API error rendered as RFC 9457 `application/problem+json`.
#[derive(Debug, Clone)]
pub struct ApiProblem {
    status: StatusCode,
    code: String,
    detail: String,
}

impl ApiProblem {
    /// Creates a new problem.
    #[must_use]
    pub fn new(status: StatusCode, code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            detail: detail.into(),
        }
    }

    /// 401 problem.
    #[must_use]
    pub fn unauthorized(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, detail)
    }

    /// 403 problem.
    #[must_use]
    pub fn forbidden(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, detail)
    }

    /// 422 problem.
    #[must_use]
    pub fn validation(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, detail)
    }

    fn body(&self) -> ProblemJson {
        ProblemJson {
            r#type: "about:blank".to_owned(),
            title: self.status.canonical_reason().unwrap_or("error").to_owned(),
            status: self.status.as_u16(),
            detail: self.detail.clone(),
            code: self.code.clone(),
        }
    }
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body())).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

/// Controls engine runtime state. Implemented by `flick-engine`.
#[async_trait]
pub trait EngineControl: Send + Sync + 'static {
    /// Returns current status.
    async fn status(&self) -> EngineStatus;
    /// Pauses recognition.
    async fn pause(&self, request: PauseRequest) -> EngineStatus;
    /// Resumes recognition.
    async fn resume(&self) -> EngineStatus;
    /// Lists available cameras.
    async fn available_cameras(&self) -> Vec<AvailableCamera>;
    /// Lists configured cameras.
    async fn cameras(&self) -> Vec<Camera>;
    /// Creates a configured camera.
    async fn create_camera(&self, request: CameraCreate) -> Result<Camera, ApiProblem>;
    /// Updates a configured camera.
    async fn patch_camera(&self, id: &str, request: CameraPatch) -> Result<Camera, ApiProblem>;
    /// Deletes a configured camera.
    async fn delete_camera(&self, id: &str) -> Result<(), ApiProblem>;
    /// Starts camera capture.
    async fn start_camera(&self, camera_id: &str) -> CameraStatus;
    /// Stops camera capture.
    async fn stop_camera(&self, camera_id: &str) -> CameraStatus;
}

/// Home Assistant gateway used by API handlers. Implemented by `flick-engine`/`flick-ha`.
#[async_trait]
pub trait HaGateway: Send + Sync + 'static {
    /// Discovers HA instances.
    async fn discover(&self) -> Vec<HaDiscovery>;
    /// Connects and persists credentials through the engine.
    async fn connect(&self, request: HaConnectRequest) -> Result<HaInstance, ApiProblem>;
    /// Returns status.
    async fn status(&self) -> HaStatus;
    /// Deletes HA credentials.
    async fn delete(&self) -> Result<(), ApiProblem>;
    /// Changes the Remote/Home URLs and trusted networks, then reconnects.
    async fn update(&self, update: HaConnectionUpdate) -> Result<HaInstance, ApiProblem>;
    /// Records the current Wi-Fi network so the client can pick Home or Remote.
    async fn set_network(&self, report: NetworkReport);
    /// Installed mTLS client certificate, if any.
    async fn client_certificate(&self) -> HaClientCertificate;
    /// Imports a `.p12`/`.pfx` bundle or PEM certificate plus key, then reconnects.
    async fn set_client_certificate(
        &self,
        data: Vec<u8>,
        password: Option<String>,
    ) -> Result<HaClientCertificate, ApiProblem>;
    /// Removes the client certificate, then reconnects.
    async fn delete_client_certificate(&self) -> Result<(), ApiProblem>;
    /// Area picker.
    async fn areas(&self) -> Vec<HaArea>;
    /// Entity picker.
    async fn entities(&self, query: BTreeMap<String, String>) -> Vec<HaEntity>;
    /// Service schema picker.
    async fn services(&self, query: BTreeMap<String, String>) -> Vec<HaServiceSchema>;
    /// Test/call action.
    async fn call(&self, action: ActionDto) -> Result<ActionOutcomeDto, ApiProblem>;
}

/// Activity list query forwarded to the engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityQuery {
    /// Maximum rows to return.
    pub limit: usize,
    /// Cursor: return rows older than this activity id.
    pub before: Option<String>,
    /// Exact status filter.
    pub status: Option<String>,
    /// Include `suppressed` rows when no status filter is set.
    pub include_suppressed: bool,
}

/// Mapping/settings/targeting persistence hook supplied by the engine.
#[async_trait]
pub trait ConfigGateway: Send + Sync + 'static {
    /// Called after settings change through the API.
    async fn settings_changed(&self, settings: SettingsMap) -> Result<(), ApiProblem>;
    /// Called after mappings change through the API.
    async fn mappings_changed(&self, mappings: Vec<Mapping>) -> Result<(), ApiProblem>;
    /// Called after anchors change through the API.
    async fn anchors_changed(&self, anchors: Vec<Anchor>) -> Result<(), ApiProblem>;
    /// Lists persisted activity, newest first.
    async fn activity(&self, _query: ActivityQuery) -> Result<ActivityPage, ApiProblem> {
        Ok(ActivityPage {
            items: vec![],
            next_before: None,
        })
    }
}

/// Teach and realign flows supplied by the spatial lane.
#[async_trait]
pub trait TeachGateway: Send + Sync + 'static {
    /// Starts teaching.
    async fn start(&self, request: TeachRequest) -> Result<TeachSession, ApiProblem>;
    /// Captures a teaching spot.
    async fn spot(&self, session_id: &str) -> Result<TeachSpotResponse, ApiProblem>;
    /// Uses current state as a level.
    async fn use_current_level(
        &self,
        session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<TeachLevelResponse, ApiProblem>;
    /// Tests a taught level.
    async fn test_level(
        &self,
        session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<ActionOutcomeDto, ApiProblem>;
    /// Commits a teach session.
    async fn commit(
        &self,
        session_id: &str,
        request: TeachCommitRequest,
    ) -> Result<TeachCommitResponse, ApiProblem>;
    /// Cancels a teach session.
    async fn cancel(&self, session_id: &str) -> Result<(), ApiProblem>;
    /// Starts realign.
    async fn start_realign(&self, place_id: &str) -> Result<RealignSession, ApiProblem>;
    /// Captures realign point.
    async fn realign_point(
        &self,
        session_id: &str,
        request: RealignPointRequest,
    ) -> Result<RealignPointResponse, ApiProblem>;
    /// Commits realign.
    async fn realign_commit(&self, session_id: &str) -> Result<RealignCommitResponse, ApiProblem>;
    /// Suggests setup targets.
    async fn suggest(
        &self,
        request: SetupSuggestRequest,
    ) -> Result<Vec<SetupSuggestion>, ApiProblem>;
}

/// Update gateway supplied by the OTA lane.
#[async_trait]
pub trait UpdatesGateway: Send + Sync + 'static {
    /// Returns update state.
    async fn state(&self) -> UpdateStateDto;
    /// Checks for updates.
    async fn check(&self) -> Result<UpdateStateDto, ApiProblem>;
    /// Installs an update.
    async fn install(&self, request: UpdateActionRequest) -> Result<(), ApiProblem>;
    /// Rolls back an update.
    async fn rollback(&self, request: UpdateActionRequest) -> Result<(), ApiProblem>;
    /// Lists models/packs.
    async fn models(&self) -> Vec<ModelPack>;
    /// Installs an on-demand model pack.
    async fn install_model(&self, pack_id: &str) -> Result<(), ApiProblem>;
    /// Deletes an on-demand model pack.
    async fn delete_model(&self, pack_id: &str) -> Result<(), ApiProblem>;
}

/// Raw RGB frame for MJPEG preview.
pub struct PreviewFrame {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Capture sequence number; unchanged when no new frame arrived.
    pub seq: u64,
    /// RGB bytes, shared with the capture thread without copying.
    pub rgb: Arc<[u8]>,
}

/// Preview source supplied by the capture/engine lane.
#[async_trait]
pub trait PreviewSource: Send + Sync + 'static {
    /// Returns the newest raw preview frame. Called only while a viewer is connected.
    async fn next_frame(&self, camera_id: &str) -> Option<PreviewFrame>;
}

/// Broadcast event hub. Senders never wait for clients.
#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<WsServerMessage>,
}

impl EventHub {
    /// Creates a hub with bounded capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publishes an event without blocking on clients.
    pub fn publish(&self, event: WsServerMessage) {
        let _ = self.sender.send(event);
    }

    /// Subscribes to the event stream.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<WsServerMessage> {
        self.sender.subscribe()
    }
}

/// Default fake engine.
#[derive(Default)]
pub struct FakeEngine {
    paused: Mutex<bool>,
}

#[async_trait]
impl EngineControl for FakeEngine {
    async fn status(&self) -> EngineStatus {
        EngineStatus {
            paused: *self.paused.lock().unwrap_or_else(|err| err.into_inner()),
            paused_until: None,
            cameras: vec![],
            ha: HaStatus {
                state: "disconnected".to_owned(),
                ..HaStatus::default()
            },
            stages: vec![],
            camera_permission: Some("authorized".to_owned()),
        }
    }

    async fn pause(&self, _request: PauseRequest) -> EngineStatus {
        *self.paused.lock().unwrap_or_else(|err| err.into_inner()) = true;
        self.status().await
    }

    async fn resume(&self) -> EngineStatus {
        *self.paused.lock().unwrap_or_else(|err| err.into_inner()) = false;
        self.status().await
    }

    async fn available_cameras(&self) -> Vec<AvailableCamera> {
        vec![AvailableCamera {
            device_ref: "dev-camera".to_owned(),
            name: "Dev Camera".to_owned(),
            kind: "local".to_owned(),
            formats: vec![CameraFormat {
                width: 1280,
                height: 720,
                fps: 30,
                format: "rgb".to_owned(),
            }],
        }]
    }

    async fn cameras(&self) -> Vec<Camera> {
        Vec::new()
    }

    async fn create_camera(&self, request: CameraCreate) -> Result<Camera, ApiProblem> {
        let now = now_rfc3339();
        Ok(Camera {
            id: make_id(),
            name: request.name,
            kind: request.kind,
            device_ref: request.device_ref,
            url_redacted: request.url_redacted,
            enabled: true,
            mirror: true,
            rotation: 0,
            active_fps: 30,
            idle_fps: 5,
            max_hands: 2,
            roi: None,
            area_override: None,
            ha_area_id: None,
            ha_device_name: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    async fn patch_camera(&self, id: &str, request: CameraPatch) -> Result<Camera, ApiProblem> {
        let now = now_rfc3339();
        Ok(Camera {
            id: id.to_owned(),
            name: request.name.unwrap_or_else(|| "Dev Camera".to_owned()),
            kind: "local".to_owned(),
            device_ref: Some("dev-camera".to_owned()),
            url_redacted: None,
            enabled: request.enabled.unwrap_or(true),
            mirror: request.mirror.unwrap_or(true),
            rotation: request.rotation.unwrap_or(0),
            active_fps: request.active_fps.unwrap_or(30),
            idle_fps: request.idle_fps.unwrap_or(5),
            max_hands: request.max_hands.unwrap_or(2),
            roi: request.roi,
            area_override: request.area_override.flatten(),
            ha_area_id: None,
            ha_device_name: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    async fn delete_camera(&self, _id: &str) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn start_camera(&self, camera_id: &str) -> CameraStatus {
        CameraStatus {
            camera_id: camera_id.to_owned(),
            state: "running".to_owned(),
            fps: Some(30.0),
            error: None,
        }
    }

    async fn stop_camera(&self, camera_id: &str) -> CameraStatus {
        CameraStatus {
            camera_id: camera_id.to_owned(),
            state: "stopped".to_owned(),
            fps: Some(0.0),
            error: None,
        }
    }
}

/// Default fake HA gateway.
#[derive(Default)]
pub struct FakeHa {
    instance: Mutex<Option<HaInstance>>,
    client_certificate: Mutex<Option<HaClientCertificate>>,
}

#[async_trait]
impl HaGateway for FakeHa {
    async fn discover(&self) -> Vec<HaDiscovery> {
        vec![HaDiscovery {
            name: "Dev Home".to_owned(),
            base_url: "http://homeassistant.local:8123".to_owned(),
            uuid: Some("dev-ha".to_owned()),
            version: Some("2026.9".to_owned()),
        }]
    }

    async fn connect(&self, request: HaConnectRequest) -> Result<HaInstance, ApiProblem> {
        let instance = HaInstance {
            id: Ulid::generate().to_string(),
            name: "Dev Home".to_owned(),
            base_url: request.base_url,
            ha_uuid: Some("dev-ha".to_owned()),
            auth_kind: "llat".to_owned(),
            ha_version: Some("2026.9".to_owned()),
            is_default: true,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            ..HaInstance::default()
        };
        *self.instance.lock().unwrap_or_else(|err| err.into_inner()) = Some(instance.clone());
        Ok(instance)
    }

    async fn status(&self) -> HaStatus {
        let instance = self
            .instance
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        HaStatus {
            state: if instance.is_some() {
                "ready"
            } else {
                "disconnected"
            }
            .to_owned(),
            ha_version: instance.as_ref().and_then(|item| item.ha_version.clone()),
            connection: instance.as_ref().map(|_| "remote".to_owned()),
            active_url: instance.as_ref().map(|item| item.base_url.clone()),
            instance,
            ..HaStatus::default()
        }
    }

    async fn delete(&self) -> Result<(), ApiProblem> {
        *self.instance.lock().unwrap_or_else(|err| err.into_inner()) = None;
        Ok(())
    }

    async fn update(&self, update: HaConnectionUpdate) -> Result<HaInstance, ApiProblem> {
        let mut guard = self.instance.lock().unwrap_or_else(|err| err.into_inner());
        let instance = guard.as_mut().ok_or_else(|| {
            ApiProblem::validation("ha_not_configured", "Home Assistant is not configured")
        })?;
        if let Some(base_url) = update.base_url {
            instance.base_url = base_url;
        }
        if let Some(internal_url) = update.internal_url {
            instance.internal_url = Some(internal_url).filter(|url| !url.trim().is_empty());
        }
        if let Some(trusted_ssids) = update.trusted_ssids {
            instance.trusted_ssids = trusted_ssids;
        }
        Ok(instance.clone())
    }

    async fn set_network(&self, _report: NetworkReport) {}

    async fn client_certificate(&self) -> HaClientCertificate {
        self.client_certificate
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .unwrap_or_default()
    }

    async fn set_client_certificate(
        &self,
        data: Vec<u8>,
        _password: Option<String>,
    ) -> Result<HaClientCertificate, ApiProblem> {
        if data.is_empty() {
            return Err(ApiProblem::validation(
                "ha_cert_invalid",
                "Choose a .p12, .pfx or PEM certificate file.",
            ));
        }
        let certificate = HaClientCertificate {
            installed: true,
            subject: Some("Flick Dev Client".to_owned()),
            issuer: Some("Dev CA".to_owned()),
            not_after: Some("2030-01-01T00:00:00Z".to_owned()),
            sha256: Some("AB:CD".to_owned()),
            expired: false,
        };
        *self
            .client_certificate
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(certificate.clone());
        Ok(certificate)
    }

    async fn delete_client_certificate(&self) -> Result<(), ApiProblem> {
        *self
            .client_certificate
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
        Ok(())
    }

    async fn areas(&self) -> Vec<HaArea> {
        vec![HaArea {
            area_id: "living_room".to_owned(),
            name: "Living room".to_owned(),
            floor_id: None,
        }]
    }

    async fn entities(&self, query: BTreeMap<String, String>) -> Vec<HaEntity> {
        let domain_filter = query.get("domain").cloned();
        let items = vec![
            HaEntity {
                entity_id: "light.bed_light".to_owned(),
                name: "Bed light".to_owned(),
                domain: "light".to_owned(),
                area_id: Some("bedroom".to_owned()),
                device_id: None,
                state: Some("off".to_owned()),
                device_class: None,
                supported_features: Some(0),
                attributes: json!({}),
            },
            HaEntity {
                entity_id: "fan.ventilador_dormitorio".to_owned(),
                name: "Ventilador Dormitorio".to_owned(),
                domain: "fan".to_owned(),
                area_id: None,
                device_id: None,
                state: Some("on".to_owned()),
                device_class: None,
                supported_features: Some(53),
                attributes: json!({"percentage": 1, "percentage_step": 1.0}),
            },
        ];
        items
            .into_iter()
            .filter(|item| {
                domain_filter
                    .as_ref()
                    .is_none_or(|domain| domain == &item.domain)
            })
            .collect()
    }

    async fn services(&self, _query: BTreeMap<String, String>) -> Vec<HaServiceSchema> {
        vec![HaServiceSchema {
            domain: "light".to_owned(),
            services: json!({"toggle": {}}),
        }]
    }

    async fn call(&self, _action: ActionDto) -> Result<ActionOutcomeDto, ApiProblem> {
        Ok(ActionOutcomeDto {
            status: "ok".to_owned(),
            error_code: None,
            message: None,
            ha_context_id: Some(Ulid::generate().to_string()),
            latency: Some(LatencyBreakdown {
                detect_ms: None,
                dispatch_ms: Some(1.0),
                ha_ms: Some(10.0),
            }),
        })
    }
}

/// No-op config gateway used by API tests.
pub struct FakeConfig;

#[async_trait]
impl ConfigGateway for FakeConfig {
    async fn settings_changed(&self, _settings: SettingsMap) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn mappings_changed(&self, _mappings: Vec<Mapping>) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn anchors_changed(&self, _anchors: Vec<Anchor>) -> Result<(), ApiProblem> {
        Ok(())
    }
}

/// Fake teach gateway.
#[derive(Default)]
pub struct FakeTeach;

#[async_trait]
impl TeachGateway for FakeTeach {
    async fn start(&self, request: TeachRequest) -> Result<TeachSession, ApiProblem> {
        Ok(TeachSession {
            id: Ulid::generate().to_string(),
            camera_id: request.camera_id,
            target: request.target,
            anchor_id: request.anchor_id,
            prompt: "Point at the device and hold still".to_owned(),
        })
    }

    async fn spot(&self, _session_id: &str) -> Result<TeachSpotResponse, ApiProblem> {
        Ok(TeachSpotResponse {
            spot_index: 1,
            ray_jitter_deg: 2.0,
            confidence: 0.9,
            kind: "direction".to_owned(),
            residual_deg: None,
        })
    }

    async fn use_current_level(
        &self,
        _session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<TeachLevelResponse, ApiProblem> {
        Ok(TeachLevelResponse {
            levels: vec![f64::from(request.level)],
            current_percentage: f64::from(request.level),
        })
    }

    async fn test_level(
        &self,
        _session_id: &str,
        _request: TeachLevelRequest,
    ) -> Result<ActionOutcomeDto, ApiProblem> {
        Ok(ActionOutcomeDto {
            status: "ok".to_owned(),
            error_code: None,
            message: None,
            ha_context_id: None,
            latency: None,
        })
    }

    async fn commit(
        &self,
        _session_id: &str,
        request: TeachCommitRequest,
    ) -> Result<TeachCommitResponse, ApiProblem> {
        let anchor = Anchor {
            id: Ulid::generate().to_string(),
            place_id: Ulid::generate().to_string(),
            name: request.name.unwrap_or_else(|| "Taught device".to_owned()),
            target: json!({"entity_id": "fan.ventilador_dormitorio"}),
            domain: "fan".to_owned(),
            kind: "direction".to_owned(),
            verb_params: json!({"levels": [1]}),
            sensitive: false,
            sensitive_ack: false,
            status: "ok".to_owned(),
            verbs: vec![],
            last_used_at: None,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            camera_id: None,
            area_override: None,
        };
        Ok(TeachCommitResponse {
            anchor,
            mapping_ids: vec![],
            distinctiveness_warnings: vec![],
        })
    }

    async fn cancel(&self, _session_id: &str) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn start_realign(&self, _place_id: &str) -> Result<RealignSession, ApiProblem> {
        Ok(RealignSession {
            id: Ulid::generate().to_string(),
            prompts: vec![],
        })
    }

    async fn realign_point(
        &self,
        _session_id: &str,
        _request: RealignPointRequest,
    ) -> Result<RealignPointResponse, ApiProblem> {
        Ok(RealignPointResponse {
            captured: true,
            residual_deg: Some(2.0),
        })
    }

    async fn realign_commit(&self, _session_id: &str) -> Result<RealignCommitResponse, ApiProblem> {
        Ok(RealignCommitResponse {
            applied: true,
            residual_deg: 2.0,
            needs_reteach: vec![],
        })
    }

    async fn suggest(
        &self,
        _request: SetupSuggestRequest,
    ) -> Result<Vec<SetupSuggestion>, ApiProblem> {
        Ok(vec![])
    }
}

/// Fake updates gateway.
#[derive(Default)]
pub struct FakeUpdates;

#[async_trait]
impl UpdatesGateway for FakeUpdates {
    async fn state(&self) -> UpdateStateDto {
        UpdateStateDto {
            channel: "stable".to_owned(),
            app: AppUpdateState {
                current: DEFAULT_VERSION.to_owned(),
                available: None,
                state: "idle".to_owned(),
                rollback_to: None,
            },
            packs: self.models().await,
            index_version: None,
            last_check_at: None,
            busy_reason: None,
        }
    }

    async fn check(&self) -> Result<UpdateStateDto, ApiProblem> {
        Ok(self.state().await)
    }

    async fn install(&self, _request: UpdateActionRequest) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn rollback(&self, _request: UpdateActionRequest) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn models(&self) -> Vec<ModelPack> {
        vec![ModelPack {
            id: "catalog".to_owned(),
            version: "bundled".to_owned(),
            kind: "catalog".to_owned(),
            state: "active".to_owned(),
            provides: vec!["catalog@1".to_owned()],
            sha256: "bundled".to_owned(),
            source: "bundled".to_owned(),
            on_demand: false,
            reject_reason: None,
            installed_at: now_rfc3339(),
            activated_at: Some(now_rfc3339()),
        }]
    }

    async fn install_model(&self, _pack_id: &str) -> Result<(), ApiProblem> {
        Ok(())
    }

    async fn delete_model(&self, _pack_id: &str) -> Result<(), ApiProblem> {
        Ok(())
    }
}

/// Fake preview source returning a generated RGB gradient at ~30 fps.
pub struct FakePreview {
    started: Instant,
}

impl Default for FakePreview {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

#[async_trait]
impl PreviewSource for FakePreview {
    async fn next_frame(&self, _camera_id: &str) -> Option<PreviewFrame> {
        let width = 640;
        let height = 360;
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                rgb.push((x % 256) as u8);
                rgb.push((y % 256) as u8);
                rgb.push(96);
            }
        }
        Some(PreviewFrame {
            width,
            height,
            seq: (self.started.elapsed().as_millis() / 33) as u64,
            rgb: rgb.into(),
        })
    }
}

#[derive(Default)]
struct PreviewTickets {
    tickets: Mutex<HashMap<String, Ticket>>,
}

struct Ticket {
    camera_id: String,
    expires_at: OffsetDateTime,
    used: bool,
}

fn default_settings() -> SettingsMap {
    BTreeMap::from([
        ("detection.sensitivity".to_owned(), json!("normal")),
        ("safety.allow_sensitive".to_owned(), json!(false)),
        ("targeting.enabled".to_owned(), json!(true)),
        ("updates.channel".to_owned(), json!("stable")),
    ])
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn json_response<T: serde::Serialize>(value: T) -> Json<T> {
    Json(value)
}

fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn accepted() -> StatusCode {
    StatusCode::ACCEPTED
}

fn not_implemented(code: &'static str) -> ApiProblem {
    ApiProblem::new(
        StatusCode::NOT_IMPLEMENTED,
        code,
        "route is registered but not implemented yet",
    )
}

fn make_id() -> String {
    Ulid::generate().to_string()
}

fn validate_mapping(
    mapping: &MappingCreate,
    anchors: &BTreeMap<String, Anchor>,
) -> Result<(), ApiProblem> {
    match (&mapping.target_mode, &mapping.action) {
        (TargetModeDto::Global, ActionDto::Verb { .. }) => Err(ApiProblem::validation(
            "verb_requires_target",
            "verb actions require an anchor or domain target",
        )),
        (TargetModeDto::Global, ActionDto::Dial { entity_id, .. }) if entity_id == "$selected" => {
            Err(ApiProblem::validation(
                "verb_requires_target",
                "$selected dial actions require an anchor or domain target",
            ))
        }
        (
            TargetModeDto::Anchor,
            ActionDto::Verb {
                verb,
                level: Some(level),
            },
        ) if verb == "level_set" => {
            let Some(anchor_id) = mapping.anchor_id.as_ref() else {
                return Err(ApiProblem::validation(
                    "anchor_required",
                    "anchor mappings need anchor_id",
                ));
            };
            let levels_len = anchors
                .get(anchor_id)
                .and_then(|anchor| anchor.verb_params.get("levels"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if (*level as usize) <= levels_len {
                Ok(())
            } else {
                Err(ApiProblem::validation(
                    "level_not_taught",
                    "level_set refers to a level that has not been taught",
                ))
            }
        }
        (TargetModeDto::Anchor, _) if mapping.anchor_id.is_none() => Err(ApiProblem::validation(
            "anchor_required",
            "anchor mappings need anchor_id",
        )),
        (TargetModeDto::Domain, _) if mapping.target_domain.is_none() => Err(
            ApiProblem::validation("domain_required", "domain mappings need target_domain"),
        ),
        _ => Ok(()),
    }
}

fn mapping_from_create(
    create: MappingCreate,
    anchors: &BTreeMap<String, Anchor>,
) -> Result<Mapping, ApiProblem> {
    validate_mapping(&create, anchors)?;
    let now = now_rfc3339();
    Ok(Mapping {
        id: make_id(),
        name: create.name,
        enabled: true,
        gesture_id: create.gesture_id,
        hand: create.hand,
        camera_ids: create.camera_ids,
        target_mode: create.target_mode,
        anchor_id: create.anchor_id,
        target_domain: create.target_domain,
        mode: create.mode,
        action: create.action,
        sensitive: false,
        sensitive_ack: create.sensitive_ack,
        confirm_gesture_id: create.confirm_gesture_id,
        feedback: create
            .feedback
            .unwrap_or_else(|| json!({"hud": true, "sound": true})),
        sort_order: create.sort_order.unwrap_or(0),
        created_at: now.clone(),
        updated_at: now,
    })
}

#[utoipa::path(get, path = "/health", responses((status = 200, body = HealthResponse)))]
async fn health(State(state): State<Arc<ApiState>>) -> Json<HealthResponse> {
    json_response(HealthResponse {
        status: "ok".to_owned(),
        version: DEFAULT_VERSION.to_owned(),
        uptime_s: state.started_at.elapsed().as_secs(),
    })
}

#[derive(Debug, Clone, Serialize)]
struct SidecarUpdatesResponse {
    busy_reason: Option<String>,
}

async fn sidecar_updates(State(state): State<Arc<ApiState>>) -> Json<SidecarUpdatesResponse> {
    Json(SidecarUpdatesResponse {
        busy_reason: state.updates.state().await.busy_reason,
    })
}

#[utoipa::path(get, path = "/api/v1/openapi.json", responses((status = 200, description = "OpenAPI 3.1 document")))]
async fn openapi_json() -> Result<Json<Value>, ApiProblem> {
    serde_json::to_value(ApiDoc::openapi())
        .map(Json)
        .map_err(|err| {
            ApiProblem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "openapi_generation_failed",
                err.to_string(),
            )
        })
}

#[utoipa::path(get, path = "/api/v1/status", responses((status = 200, body = EngineStatus), (status = 401, body = ProblemJson), (status = 403, body = ProblemJson)))]
async fn status(State(state): State<Arc<ApiState>>) -> Json<EngineStatus> {
    json_response(state.engine.status().await)
}

#[utoipa::path(post, path = "/api/v1/engine/pause", request_body = PauseRequest, responses((status = 200, body = EngineStatus)))]
async fn pause_engine(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<PauseRequest>,
) -> Json<EngineStatus> {
    json_response(state.engine.pause(request).await)
}

#[utoipa::path(post, path = "/api/v1/engine/resume", responses((status = 200, body = EngineStatus)))]
async fn resume_engine(State(state): State<Arc<ApiState>>) -> Json<EngineStatus> {
    json_response(state.engine.resume().await)
}

#[utoipa::path(get, path = "/api/v1/settings", responses((status = 200, body = SettingsDto)))]
async fn get_settings(State(state): State<Arc<ApiState>>) -> Json<SettingsMap> {
    json_response(
        state
            .settings
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone(),
    )
}

#[utoipa::path(patch, path = "/api/v1/settings", request_body = SettingsDto, responses((status = 200, body = SettingsDto)))]
async fn patch_settings(
    State(state): State<Arc<ApiState>>,
    Json(patch): Json<SettingsMap>,
) -> Result<Json<SettingsMap>, ApiProblem> {
    let settings = {
        let mut settings = state.settings.lock().unwrap_or_else(|err| err.into_inner());
        settings.extend(patch);
        settings.clone()
    };
    state.config_sync.settings_changed(settings.clone()).await?;
    Ok(json_response(settings))
}

#[utoipa::path(get, path = "/api/v1/ha/discover", responses((status = 200, body = [HaDiscovery])))]
async fn ha_discover(State(state): State<Arc<ApiState>>) -> Json<Vec<HaDiscovery>> {
    json_response(state.ha.discover().await)
}

#[utoipa::path(post, path = "/api/v1/ha/connect", request_body = HaConnectRequest, responses((status = 200, body = HaInstance)))]
async fn ha_connect(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<HaConnectRequest>,
) -> Result<Json<HaInstance>, ApiProblem> {
    state.ha.connect(request).await.map(json_response)
}

#[utoipa::path(get, path = "/api/v1/ha/status", responses((status = 200, body = HaStatus)))]
async fn ha_status(State(state): State<Arc<ApiState>>) -> Json<HaStatus> {
    json_response(state.ha.status().await)
}

#[utoipa::path(delete, path = "/api/v1/ha", responses((status = 204)))]
async fn ha_delete(State(state): State<Arc<ApiState>>) -> Result<StatusCode, ApiProblem> {
    state.ha.delete().await.map(|()| no_content())
}

#[utoipa::path(patch, path = "/api/v1/ha", request_body = HaConnectionUpdate, responses((status = 200, body = HaInstance)))]
async fn ha_update(
    State(state): State<Arc<ApiState>>,
    Json(update): Json<HaConnectionUpdate>,
) -> Result<Json<HaInstance>, ApiProblem> {
    state.ha.update(update).await.map(json_response)
}

#[utoipa::path(get, path = "/api/v1/ha/client-certificate", responses((status = 200, body = HaClientCertificate)))]
async fn ha_client_certificate(State(state): State<Arc<ApiState>>) -> Json<HaClientCertificate> {
    json_response(state.ha.client_certificate().await)
}

#[utoipa::path(put, path = "/api/v1/ha/client-certificate", request_body = HaClientCertificateUpload, responses((status = 200, body = HaClientCertificate), (status = 422, body = ProblemJson)))]
async fn put_ha_client_certificate(
    State(state): State<Arc<ApiState>>,
    Json(upload): Json<HaClientCertificateUpload>,
) -> Result<Json<HaClientCertificate>, ApiProblem> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD
        .decode(upload.data.trim())
        .map_err(|_| {
            ApiProblem::validation("ha_cert_invalid", "The certificate file couldn't be read.")
        })?;
    let password = upload.password.filter(|password| !password.is_empty());
    state
        .ha
        .set_client_certificate(data, password)
        .await
        .map(json_response)
}

#[utoipa::path(delete, path = "/api/v1/ha/client-certificate", responses((status = 204)))]
async fn delete_ha_client_certificate(
    State(state): State<Arc<ApiState>>,
) -> Result<StatusCode, ApiProblem> {
    state
        .ha
        .delete_client_certificate()
        .await
        .map(|()| no_content())
}

#[utoipa::path(put, path = "/api/v1/network", request_body = NetworkReport, responses((status = 204)))]
async fn put_network(
    State(state): State<Arc<ApiState>>,
    Json(report): Json<NetworkReport>,
) -> StatusCode {
    state.ha.set_network(report).await;
    no_content()
}

#[utoipa::path(get, path = "/api/v1/ha/areas", responses((status = 200, body = [HaArea])))]
async fn ha_areas(State(state): State<Arc<ApiState>>) -> Json<Vec<HaArea>> {
    json_response(state.ha.areas().await)
}

#[utoipa::path(get, path = "/api/v1/ha/entities", params(("domain" = Option<String>, Query), ("area_id" = Option<String>, Query), ("q" = Option<String>, Query)), responses((status = 200, body = [HaEntity])))]
async fn ha_entities(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Vec<HaEntity>> {
    json_response(state.ha.entities(query).await)
}

#[utoipa::path(get, path = "/api/v1/ha/services", params(("domain" = Option<String>, Query)), responses((status = 200, body = [HaServiceSchema])))]
async fn ha_services(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Vec<HaServiceSchema>> {
    json_response(state.ha.services(query).await)
}

#[utoipa::path(post, path = "/api/v1/ha/call", request_body = ActionDto, responses((status = 200, body = ActionOutcomeDto), (status = 422, body = ProblemJson)))]
async fn ha_call(
    State(state): State<Arc<ApiState>>,
    Json(action): Json<ActionDto>,
) -> Result<Json<ActionOutcomeDto>, ApiProblem> {
    state.ha.call(action).await.map(json_response)
}

#[utoipa::path(get, path = "/api/v1/cameras/available", responses((status = 200, body = [AvailableCamera])))]
async fn cameras_available(State(state): State<Arc<ApiState>>) -> Json<Vec<AvailableCamera>> {
    json_response(state.engine.available_cameras().await)
}

#[utoipa::path(get, path = "/api/v1/cameras", responses((status = 200, body = [Camera])))]
async fn list_cameras(State(state): State<Arc<ApiState>>) -> Json<Vec<Camera>> {
    json_response(state.engine.cameras().await)
}

#[utoipa::path(post, path = "/api/v1/cameras", request_body = CameraCreate, responses((status = 200, body = Camera)))]
async fn create_camera(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<CameraCreate>,
) -> Result<Json<Camera>, ApiProblem> {
    state.engine.create_camera(request).await.map(json_response)
}

#[utoipa::path(patch, path = "/api/v1/cameras/{id}", request_body = CameraPatch, responses((status = 200, body = Camera), (status = 404, body = ProblemJson)))]
async fn patch_camera(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
    Json(mut request): Json<CameraPatch>,
) -> Result<Json<Camera>, ApiProblem> {
    request.area_override = validated_area_override(request.area_override)?;
    state
        .engine
        .patch_camera(&id, request)
        .await
        .map(json_response)
}

/// Trims a Flick-only area id; an explicit null passes through to clear it.
fn validated_area_override(
    area_override: Option<Option<String>>,
) -> Result<Option<Option<String>>, ApiProblem> {
    match area_override {
        Some(Some(area_id)) => {
            let area_id = area_id.trim();
            if area_id.is_empty()
                || area_id.chars().count() > 128
                || area_id.chars().any(char::is_control)
            {
                return Err(ApiProblem::validation(
                    "bad_area_id",
                    "area id must be 1-128 printable characters",
                ));
            }
            Ok(Some(Some(area_id.to_owned())))
        }
        other => Ok(other),
    }
}

#[utoipa::path(delete, path = "/api/v1/cameras/{id}", responses((status = 204)))]
async fn delete_camera(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    state.engine.delete_camera(&id).await?;
    Ok(no_content())
}

#[utoipa::path(post, path = "/api/v1/cameras/{id}/start", responses((status = 200, body = CameraStatus)))]
async fn start_camera(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Json<CameraStatus> {
    json_response(state.engine.start_camera(&id).await)
}

#[utoipa::path(post, path = "/api/v1/cameras/{id}/stop", responses((status = 200, body = CameraStatus)))]
async fn stop_camera(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Json<CameraStatus> {
    json_response(state.engine.stop_camera(&id).await)
}

#[utoipa::path(post, path = "/api/v1/cameras/{id}/preview-ticket", responses((status = 200, body = PreviewTicket)))]
async fn preview_ticket(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Json<PreviewTicket> {
    let ticket = make_id();
    let expires_at = OffsetDateTime::now_utc() + Duration::from_secs(60);
    state
        .tickets
        .tickets
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(
            ticket.clone(),
            Ticket {
                camera_id: id.clone(),
                expires_at,
                used: false,
            },
        );
    json_response(PreviewTicket {
        url: format!("/stream/{id}.mjpg?ticket={ticket}"),
        expires_at: expires_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| now_rfc3339()),
    })
}

#[utoipa::path(get, path = "/api/v1/gestures", responses((status = 200, body = [Gesture])))]
async fn list_gestures() -> Json<Vec<Gesture>> {
    json_response(builtin_gestures())
}

#[utoipa::path(post, path = "/api/v1/gestures", request_body = GestureCreate, responses((status = 200, body = Gesture)))]
async fn create_gesture(Json(request): Json<GestureCreate>) -> Json<Gesture> {
    json_response(Gesture {
        id: format!("custom.{}", make_id()),
        source: "custom".to_owned(),
        kind: request.kind.unwrap_or_else(|| "static".to_owned()),
        hands_required: request.hands_required.unwrap_or(1),
        name: request.name,
        icon: request.icon,
        hand_constraint: request.hand_constraint,
        threshold: None,
        enabled: true,
        sample_count: 0,
        accuracy: None,
        distinctiveness: None,
    })
}

#[utoipa::path(patch, path = "/api/v1/gestures/{id}", request_body = GesturePatch, responses((status = 501, body = ProblemJson)))]
async fn patch_gesture() -> Result<Json<Gesture>, ApiProblem> {
    Err(not_implemented("gesture_persistence_not_ready"))
}

#[utoipa::path(delete, path = "/api/v1/gestures/{id}", responses((status = 204)))]
async fn delete_gesture() -> StatusCode {
    no_content()
}

#[utoipa::path(post, path = "/api/v1/gestures/{id}/capture", request_body = CaptureRequest, responses((status = 200, body = CaptureSession)))]
async fn capture_gesture(
    Path(id): Path<String>,
    Json(request): Json<CaptureRequest>,
) -> Json<CaptureSession> {
    json_response(CaptureSession {
        id: make_id(),
        gesture_id: id,
        camera_id: request.camera_id,
        kind: request.kind,
        target_takes: request.takes,
        status: "running".to_owned(),
        created_at: now_rfc3339(),
    })
}

#[utoipa::path(get, path = "/api/v1/gestures/{id}/motion-takes", responses((status = 200, body = [MotionTake])))]
async fn motion_takes() -> Json<Vec<MotionTake>> {
    json_response(vec![])
}

#[utoipa::path(patch, path = "/api/v1/gestures/{id}/type", request_body = GestureTypePatch, responses((status = 501, body = ProblemJson)))]
async fn patch_gesture_type() -> Result<Json<Gesture>, ApiProblem> {
    Err(not_implemented("gesture_type_not_ready"))
}

#[utoipa::path(post, path = "/api/v1/capture/{session_id}/cancel", responses((status = 204)))]
async fn cancel_capture() -> StatusCode {
    no_content()
}

#[utoipa::path(get, path = "/api/v1/gestures/{id}/samples", responses((status = 200, body = [Sample])))]
async fn gesture_samples() -> Json<Vec<Sample>> {
    json_response(vec![])
}

#[utoipa::path(delete, path = "/api/v1/gestures/{id}/samples/{sample_id}", responses((status = 204)))]
async fn delete_sample() -> StatusCode {
    no_content()
}

#[utoipa::path(post, path = "/api/v1/classifier/train", request_body = TrainRequest, responses((status = 200, body = ClassifierReport)))]
async fn train_classifier() -> Json<ClassifierReport> {
    json_response(classifier_report())
}

#[utoipa::path(get, path = "/api/v1/classifier", responses((status = 200, body = ClassifierReport)))]
async fn get_classifier() -> Json<ClassifierReport> {
    json_response(classifier_report())
}

#[utoipa::path(get, path = "/api/v1/mappings", responses((status = 200, body = [Mapping])))]
async fn list_mappings(State(state): State<Arc<ApiState>>) -> Json<Vec<Mapping>> {
    json_response(
        state
            .mappings
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .values()
            .cloned()
            .collect(),
    )
}

#[utoipa::path(post, path = "/api/v1/mappings", request_body = MappingCreate, responses((status = 200, body = Mapping), (status = 422, body = ProblemJson)))]
async fn create_mapping(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<MappingCreate>,
) -> Result<Json<Mapping>, ApiProblem> {
    let anchors = state
        .anchors
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let mapping = mapping_from_create(request, &anchors)?;
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        mappings.insert(mapping.id.clone(), mapping.clone());
        mappings.values().cloned().collect::<Vec<_>>()
    };
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(json_response(mapping))
}

#[utoipa::path(patch, path = "/api/v1/mappings/{id}", request_body = MappingPatch, responses((status = 200, body = Mapping), (status = 422, body = ProblemJson)))]
async fn patch_mapping(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
    Json(request): Json<MappingPatch>,
) -> Result<Json<Mapping>, ApiProblem> {
    let gesture_id = request.gesture_id.as_deref().map(str::trim);
    if gesture_id.is_some_and(str::is_empty) {
        return Err(ApiProblem::validation(
            "bad_gesture_id",
            "Choose a gesture for this action.",
        ));
    }
    let gesture_id = gesture_id.map(str::to_owned);
    let anchors = state
        .anchors
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let (mapping, all_mappings) = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        let Some(current) = mappings.get(&id) else {
            return Err(ApiProblem::new(
                StatusCode::NOT_FOUND,
                "mapping_not_found",
                "mapping not found",
            ));
        };
        let target_mode = request.target_mode.unwrap_or(current.target_mode);
        let anchor_id = request
            .anchor_id
            .clone()
            .or_else(|| current.anchor_id.clone());
        let next_gesture = gesture_id
            .clone()
            .unwrap_or_else(|| current.gesture_id.clone());
        if target_mode == TargetModeDto::Anchor
            && anchor_id.is_some()
            && mappings.values().any(|other| {
                other.id != id
                    && other.target_mode == TargetModeDto::Anchor
                    && other.anchor_id == anchor_id
                    && other.gesture_id == next_gesture
            })
        {
            return Err(ApiProblem::validation(
                "gesture_conflict",
                "Another action on this device already uses that gesture. Pick a different gesture.",
            ));
        }
        let anchor_name = anchor_id
            .as_deref()
            .and_then(|anchor_id| anchors.get(anchor_id))
            .map(|anchor| anchor.name.clone());
        let auto_named = anchor_name.as_deref().is_some_and(|anchor_name| {
            current.name == mapping_auto_name(anchor_name, &current.gesture_id, &current.action)
        });
        let Some(mapping) = mappings.get_mut(&id) else {
            return Err(ApiProblem::new(
                StatusCode::NOT_FOUND,
                "mapping_not_found",
                "mapping not found",
            ));
        };
        let renamed = request.name.is_some();
        if let Some(name) = request.name {
            mapping.name = name;
        }
        if let Some(gesture_id) = gesture_id {
            mapping.gesture_id = gesture_id;
        }
        if let Some(enabled) = request.enabled {
            mapping.enabled = enabled;
        }
        if let Some(action) = request.action {
            mapping.action = action;
        }
        if let Some(target_mode) = request.target_mode {
            mapping.target_mode = target_mode;
        }
        if let Some(anchor_id) = request.anchor_id {
            mapping.anchor_id = Some(anchor_id);
        }
        if let Some(target_domain) = request.target_domain {
            mapping.target_domain = Some(target_domain);
        }
        if let Some(sensitive_ack) = request.sensitive_ack {
            mapping.sensitive_ack = sensitive_ack;
        }
        if !renamed
            && auto_named
            && let Some(anchor_name) = anchor_name.as_deref()
        {
            mapping.name = mapping_auto_name(anchor_name, &mapping.gesture_id, &mapping.action);
        }
        mapping.updated_at = now_rfc3339();
        (
            mapping.clone(),
            mappings.values().cloned().collect::<Vec<_>>(),
        )
    };
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(json_response(mapping))
}

#[utoipa::path(delete, path = "/api/v1/mappings/{id}", responses((status = 204)))]
async fn delete_mapping(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        mappings.remove(&id);
        mappings.values().cloned().collect::<Vec<_>>()
    };
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(no_content())
}

#[utoipa::path(post, path = "/api/v1/mappings/{id}/test", responses((status = 200, body = ActionOutcomeDto)))]
async fn test_mapping() -> Json<ActionOutcomeDto> {
    json_response(ActionOutcomeDto {
        status: "ok".to_owned(),
        error_code: None,
        message: None,
        ha_context_id: None,
        latency: None,
    })
}

#[utoipa::path(put, path = "/api/v1/mappings/order", request_body = MappingOrder, responses((status = 204)))]
async fn order_mappings(
    State(state): State<Arc<ApiState>>,
    Json(order): Json<MappingOrder>,
) -> Result<StatusCode, ApiProblem> {
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        for (index, id) in order.0.iter().enumerate() {
            if let Some(mapping) = mappings.get_mut(id) {
                mapping.sort_order = i64::try_from(index).unwrap_or(i64::MAX);
                mapping.updated_at = now_rfc3339();
            }
        }
        mappings.values().cloned().collect::<Vec<_>>()
    };
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(no_content())
}

const ACTIVITY_STATUSES: [&str; 7] = [
    "fired",
    "sent",
    "ok",
    "error",
    "timeout",
    "stale",
    "suppressed",
];

#[utoipa::path(get, path = "/api/v1/activity", params(("limit" = Option<u32>, Query), ("before" = Option<String>, Query), ("status" = Option<String>, Query), ("include_suppressed" = Option<bool>, Query)), responses((status = 200, body = ActivityPage), (status = 422, body = ProblemJson)))]
async fn activity(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<Json<ActivityPage>, ApiProblem> {
    let limit = match query.get("limit") {
        None => 50,
        Some(raw) => raw
            .parse::<usize>()
            .ok()
            .filter(|limit| (1..=200).contains(limit))
            .ok_or_else(|| {
                ApiProblem::validation("invalid_limit", "limit must be between 1 and 200")
            })?,
    };
    let status = query
        .get("status")
        .filter(|status| !status.is_empty())
        .cloned();
    if let Some(status) = &status
        && !ACTIVITY_STATUSES.contains(&status.as_str())
    {
        return Err(ApiProblem::validation(
            "invalid_status",
            "unknown activity status",
        ));
    }
    let include_suppressed = query
        .get("include_suppressed")
        .is_some_and(|value| value == "true" || value == "1");
    let page = state
        .config_sync
        .activity(ActivityQuery {
            limit,
            before: query
                .get("before")
                .filter(|before| !before.is_empty())
                .cloned(),
            status,
            include_suppressed,
        })
        .await?;
    Ok(json_response(page))
}

#[utoipa::path(get, path = "/api/v1/places", params(("camera_id" = Option<String>, Query)), responses((status = 200, body = [Place])))]
async fn list_places(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Vec<Place>> {
    let camera_id = query.get("camera_id").cloned();
    let places = state
        .places
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .values()
        .filter(|place| camera_id.as_ref().is_none_or(|id| id == &place.camera_id))
        .cloned()
        .collect();
    json_response(places)
}

#[utoipa::path(patch, path = "/api/v1/places/{id}", request_body = PlacePatch, responses((status = 200, body = Place)))]
async fn patch_place(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
    Json(request): Json<PlacePatch>,
) -> Result<Json<Place>, ApiProblem> {
    let mut places = state.places.lock().unwrap_or_else(|err| err.into_inner());
    let Some(place) = places.get_mut(&id) else {
        return Err(ApiProblem::new(
            StatusCode::NOT_FOUND,
            "place_not_found",
            "place not found",
        ));
    };
    place.name = request.name;
    place.updated_at = now_rfc3339();
    Ok(json_response(place.clone()))
}

#[utoipa::path(delete, path = "/api/v1/places/{id}", responses((status = 204)))]
async fn delete_place(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    state
        .places
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(&id);
    let removed_anchor_ids: HashSet<String> = {
        let mut anchors = state.anchors.lock().unwrap_or_else(|err| err.into_inner());
        let ids: HashSet<String> = anchors
            .values()
            .filter(|anchor| anchor.place_id == id)
            .map(|anchor| anchor.id.clone())
            .collect();
        anchors.retain(|_, anchor| anchor.place_id != id);
        ids
    };
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        mappings.retain(|_, mapping| {
            mapping
                .anchor_id
                .as_ref()
                .is_none_or(|anchor_id| !removed_anchor_ids.contains(anchor_id))
        });
        mappings.values().cloned().collect::<Vec<_>>()
    };
    let all_anchors = state
        .anchors
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .values()
        .cloned()
        .collect::<Vec<_>>();
    state.config_sync.anchors_changed(all_anchors).await?;
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(no_content())
}

#[utoipa::path(post, path = "/api/v1/places/{id}/realign", responses((status = 200, body = RealignSession)))]
async fn start_realign(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<Json<RealignSession>, ApiProblem> {
    state.teach.start_realign(&id).await.map(json_response)
}

#[utoipa::path(post, path = "/api/v1/realign/{session_id}/point", request_body = RealignPointRequest, responses((status = 200, body = RealignPointResponse)))]
async fn realign_point(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
    Json(request): Json<RealignPointRequest>,
) -> Result<Json<RealignPointResponse>, ApiProblem> {
    state
        .teach
        .realign_point(&session_id, request)
        .await
        .map(json_response)
}

#[utoipa::path(post, path = "/api/v1/realign/{session_id}/commit", responses((status = 200, body = RealignCommitResponse)))]
async fn realign_commit(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
) -> Result<Json<RealignCommitResponse>, ApiProblem> {
    state
        .teach
        .realign_commit(&session_id)
        .await
        .map(json_response)
}

#[utoipa::path(get, path = "/api/v1/anchors", params(("place_id" = Option<String>, Query)), responses((status = 200, body = [Anchor])))]
async fn list_anchors(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Json<Vec<Anchor>> {
    let place_id = query.get("place_id").cloned();
    let anchors = state
        .anchors
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .values()
        .filter(|anchor| place_id.as_ref().is_none_or(|id| id == &anchor.place_id))
        .cloned()
        .collect();
    json_response(anchors)
}

#[utoipa::path(patch, path = "/api/v1/anchors/{id}", request_body = AnchorPatch, responses((status = 200, body = Anchor)))]
async fn patch_anchor(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
    Json(request): Json<AnchorPatch>,
) -> Result<Json<Anchor>, ApiProblem> {
    let area_override = validated_area_override(request.area_override)?;
    let (anchor, all_anchors) = {
        let mut anchors = state.anchors.lock().unwrap_or_else(|err| err.into_inner());
        let Some(anchor) = anchors.get_mut(&id) else {
            return Err(ApiProblem::new(
                StatusCode::NOT_FOUND,
                "anchor_not_found",
                "anchor not found",
            ));
        };
        if let Some(name) = request.name {
            anchor.name = name;
        }
        if let Some(verb_params) = request.verb_params {
            anchor.verb_params = verb_params;
        }
        if let Some(area_override) = area_override {
            anchor.area_override = area_override;
        }
        anchor.updated_at = now_rfc3339();
        (
            anchor.clone(),
            anchors.values().cloned().collect::<Vec<_>>(),
        )
    };
    state.config_sync.anchors_changed(all_anchors).await?;
    Ok(json_response(anchor))
}

#[utoipa::path(delete, path = "/api/v1/anchors/{id}", responses((status = 204)))]
async fn delete_anchor(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    let all_anchors = {
        let mut anchors = state.anchors.lock().unwrap_or_else(|err| err.into_inner());
        anchors.remove(&id);
        anchors.values().cloned().collect::<Vec<_>>()
    };
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        mappings.retain(|_, mapping| mapping.anchor_id.as_ref() != Some(&id));
        mappings.values().cloned().collect::<Vec<_>>()
    };
    state.config_sync.anchors_changed(all_anchors).await?;
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(no_content())
}

#[utoipa::path(post, path = "/api/v1/anchors/{id}/test", responses((status = 200, body = AnchorTestResponse)))]
async fn test_anchor() -> Json<AnchorTestResponse> {
    json_response(AnchorTestResponse {
        selected: true,
        angular_error_deg: 2.0,
        runner_up: None,
    })
}

#[utoipa::path(post, path = "/api/v1/teach", request_body = TeachRequest, responses((status = 200, body = TeachSession)))]
async fn start_teach(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<TeachRequest>,
) -> Result<Json<TeachSession>, ApiProblem> {
    state.teach.start(request).await.map(json_response)
}

#[utoipa::path(post, path = "/api/v1/teach/{session_id}/spot", responses((status = 200, body = TeachSpotResponse)))]
async fn teach_spot(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
) -> Result<Json<TeachSpotResponse>, ApiProblem> {
    state.teach.spot(&session_id).await.map(json_response)
}

#[utoipa::path(post, path = "/api/v1/teach/{session_id}/levels/use-current", request_body = TeachLevelRequest, responses((status = 200, body = TeachLevelResponse)))]
async fn teach_level_current(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
    Json(request): Json<TeachLevelRequest>,
) -> Result<Json<TeachLevelResponse>, ApiProblem> {
    state
        .teach
        .use_current_level(&session_id, request)
        .await
        .map(json_response)
}

#[utoipa::path(post, path = "/api/v1/teach/{session_id}/levels/test", request_body = TeachLevelRequest, responses((status = 200, body = ActionOutcomeDto)))]
async fn teach_level_test(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
    Json(request): Json<TeachLevelRequest>,
) -> Result<Json<ActionOutcomeDto>, ApiProblem> {
    state
        .teach
        .test_level(&session_id, request)
        .await
        .map(json_response)
}

#[utoipa::path(post, path = "/api/v1/teach/{session_id}/commit", request_body = TeachCommitRequest, responses((status = 200, body = TeachCommitResponse)))]
async fn teach_commit(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
    Json(request): Json<TeachCommitRequest>,
) -> Result<Json<TeachCommitResponse>, ApiProblem> {
    let requested_verbs = request.verbs.clone();
    let mut response = state.teach.commit(&session_id, request).await?;
    let anchors = {
        let mut anchors = state.anchors.lock().unwrap_or_else(|err| err.into_inner());
        // Re-teach and append reuse the anchor id; keep the Flick-only area the user chose.
        if response.anchor.area_override.is_none()
            && let Some(previous) = anchors.get(&response.anchor.id)
        {
            response.anchor.area_override = previous.area_override.clone();
        }
        anchors.insert(response.anchor.id.clone(), response.anchor.clone());
        anchors.clone()
    };
    let created_mappings = requested_verbs
        .into_iter()
        .enumerate()
        .map(|(index, verb)| {
            mapping_from_create(
                MappingCreate {
                    name: teach_mapping_name(&response.anchor.name, &verb),
                    gesture_id: verb.gesture_id,
                    hand: "any".to_owned(),
                    allow_two_hands: false,
                    camera_ids: Vec::new(),
                    target_mode: TargetModeDto::Anchor,
                    anchor_id: Some(response.anchor.id.clone()),
                    target_domain: None,
                    mode: "tap".to_owned(),
                    hold_ms: None,
                    repeat_ms: None,
                    cooldown_ms: Some(600),
                    require_armed: false,
                    active_hours: None,
                    action: verb.action,
                    sensitive_ack: response.anchor.sensitive_ack,
                    confirm_gesture_id: None,
                    feedback: None,
                    sort_order: Some(index as i64),
                },
                &anchors,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let all_mappings = {
        let mut mappings = state.mappings.lock().unwrap_or_else(|err| err.into_inner());
        response.mapping_ids = created_mappings
            .into_iter()
            .map(|created| {
                // Re-teaching keeps one mapping per anchor gesture instead of stacking duplicates.
                let existing = mappings.values_mut().find(|mapping| {
                    mapping.target_mode == TargetModeDto::Anchor
                        && mapping.anchor_id == created.anchor_id
                        && mapping.gesture_id == created.gesture_id
                });
                if let Some(existing) = existing {
                    existing.name = created.name;
                    existing.action = created.action;
                    existing.enabled = true;
                    existing.sensitive_ack = created.sensitive_ack;
                    existing.updated_at = created.updated_at;
                    return existing.id.clone();
                }
                let id = created.id.clone();
                mappings.insert(id.clone(), created);
                id
            })
            .collect();
        mappings.values().cloned().collect::<Vec<_>>()
    };
    state.config_sync.mappings_changed(all_mappings).await?;
    Ok(json_response(response))
}

fn teach_mapping_name(anchor_name: &str, verb: &TeachVerb) -> String {
    mapping_auto_name(anchor_name, &verb.gesture_id, &verb.action)
}

fn mapping_auto_name(anchor_name: &str, gesture_id: &str, action: &ActionDto) -> String {
    let action = match action {
        ActionDto::Verb { verb, level } if verb == "level_set" => {
            format!("speed {}", level.unwrap_or(1))
        }
        ActionDto::Verb { verb, .. } => verb.replace('_', " "),
        ActionDto::CallService {
            domain, service, ..
        } => format!("{domain}.{service}"),
        ActionDto::Dial { property, .. } => format!("dial {property}"),
    };
    format!(
        "{anchor_name} + {} → {action}",
        gesture_id.replace("builtin.", "")
    )
}

#[utoipa::path(post, path = "/api/v1/teach/{session_id}/cancel", responses((status = 204)))]
async fn teach_cancel(
    State(state): State<Arc<ApiState>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    state.teach.cancel(&session_id).await.map(|()| no_content())
}

#[utoipa::path(post, path = "/api/v1/setup-assistant/suggest", request_body = SetupSuggestRequest, responses((status = 200, body = [SetupSuggestion])))]
async fn setup_suggest(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<SetupSuggestRequest>,
) -> Result<Json<Vec<SetupSuggestion>>, ApiProblem> {
    state.teach.suggest(request).await.map(json_response)
}

#[utoipa::path(get, path = "/api/v1/updates", responses((status = 200, body = UpdateStateDto)))]
async fn get_updates(State(state): State<Arc<ApiState>>) -> Json<UpdateStateDto> {
    json_response(state.updates.state().await)
}

#[utoipa::path(post, path = "/api/v1/updates/check", responses((status = 200, body = UpdateStateDto)))]
async fn check_updates(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<UpdateStateDto>, ApiProblem> {
    state.updates.check().await.map(json_response)
}

#[utoipa::path(post, path = "/api/v1/updates/install", request_body = UpdateActionRequest, responses((status = 202)))]
async fn install_update(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<UpdateActionRequest>,
) -> Result<StatusCode, ApiProblem> {
    state.updates.install(request).await.map(|()| accepted())
}

#[utoipa::path(post, path = "/api/v1/updates/rollback", request_body = UpdateActionRequest, responses((status = 202)))]
async fn rollback_update(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<UpdateActionRequest>,
) -> Result<StatusCode, ApiProblem> {
    state.updates.rollback(request).await.map(|()| accepted())
}

#[utoipa::path(get, path = "/api/v1/models", responses((status = 200, body = [ModelPack])))]
async fn get_models(State(state): State<Arc<ApiState>>) -> Json<Vec<ModelPack>> {
    json_response(state.updates.models().await)
}

#[utoipa::path(post, path = "/api/v1/models/{pack_id}/install", responses((status = 202)))]
async fn install_model(
    State(state): State<Arc<ApiState>>,
    Path(pack_id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    state
        .updates
        .install_model(&pack_id)
        .await
        .map(|()| accepted())
}

#[utoipa::path(delete, path = "/api/v1/models/{pack_id}", responses((status = 204)))]
async fn delete_model(
    State(state): State<Arc<ApiState>>,
    Path(pack_id): Path<String>,
) -> Result<StatusCode, ApiProblem> {
    state
        .updates
        .delete_model(&pack_id)
        .await
        .map(|()| no_content())
}

#[utoipa::path(post, path = "/api/v1/packs/export", request_body = PackExportRequest, responses((status = 200, body = GesturePackDto)))]
async fn export_pack() -> Json<GesturePackDto> {
    json_response(GesturePackDto {
        format: "flick.gesture-pack".to_owned(),
        version: 1,
        name: "Export".to_owned(),
        payload: json!({}),
    })
}

#[utoipa::path(get, path = "/api/v1/diagnostics/bundle", responses((status = 501, body = ProblemJson)))]
async fn diagnostics_bundle() -> Result<Response, ApiProblem> {
    Err(not_implemented("diagnostics_not_ready"))
}

#[utoipa::path(get, path = "/api/v1/events", responses((status = 101, description = "WebSocket event stream"), (status = 401, body = ProblemJson)))]
async fn ws_events(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiProblem> {
    let protocols = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let has_v1 = protocols.split(',').any(|item| item.trim() == "flick.v1");
    let bearer = format!("bearer.{}", state.config.token);
    let has_token = protocols
        .split(',')
        .any(|item| constant_time_eq(item.trim(), &bearer));
    if !has_v1 || !has_token {
        return Err(ApiProblem::unauthorized(
            "bad_ws_token",
            "WebSocket requires flick.v1 and bearer token subprotocols",
        ));
    }
    Ok(ws
        .protocols(["flick.v1"])
        .on_upgrade(move |socket| websocket_loop(socket, state))
        .into_response())
}

async fn websocket_loop(socket: axum::extract::ws::WebSocket, state: Arc<ApiState>) {
    let (mut sender, mut receiver) = socket.split();
    let hello = WsServerMessage::Hello {
        ts: now_rfc3339(),
        payload: HelloEvent {
            engine_version: DEFAULT_VERSION.to_owned(),
            api: API_VERSION.to_owned(),
            pro: false,
        },
    };
    if sender
        .send(Message::Text(
            serde_json::to_string(&hello)
                .unwrap_or_else(|_| "{}".to_owned())
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let mut rx = state.events.subscribe();
    let mut topics: HashSet<String> = HashSet::from([
        "status".to_owned(),
        "gestures".to_owned(),
        "actions".to_owned(),
        "ha".to_owned(),
        "capture".to_owned(),
        "targeting".to_owned(),
        "teach".to_owned(),
        "updates".to_owned(),
    ]);
    let mut last_hands: HashMap<String, Instant> = HashMap::new();
    loop {
        tokio::select! {
            maybe_msg = receiver.next() => {
                let Some(Ok(Message::Text(text))) = maybe_msg else { break; };
                if let Ok(message) = serde_json::from_str::<WsClientMessage>(&text) {
                    match message {
                        WsClientMessage::Subscribe { topics: incoming } => topics.extend(incoming),
                        WsClientMessage::Unsubscribe { topics: incoming } => {
                            for topic in incoming { topics.remove(&topic); }
                        }
                    }
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        if !event_allowed(&event, &topics, &mut last_hands) { continue; }
                        let Ok(text) = serde_json::to_string(&event) else { continue; };
                        if sender.send(Message::Text(text.into())).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let resync = WsServerMessage::Resync { ts: now_rfc3339(), payload: ResyncEvent { reason: "lagged".to_owned(), topics: topics.iter().cloned().collect() } };
                        let _ = sender.send(Message::Text(serde_json::to_string(&resync).unwrap_or_else(|_| "{}".to_owned()).into())).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn event_allowed(
    event: &WsServerMessage,
    topics: &HashSet<String>,
    last_hands: &mut HashMap<String, Instant>,
) -> bool {
    let topic = event.topic();
    if topic == "hello" || topic == "resync" {
        return true;
    }
    if let WsServerMessage::Hands { payload, .. } = event {
        if !topics.contains(&topic) {
            return false;
        }
        let now = Instant::now();
        let entry = last_hands
            .entry(payload.camera_id.clone())
            .or_insert(now - Duration::from_secs(1));
        // An empty frame clears the overlay, so it is never dropped.
        if !payload.hands.is_empty() && now.duration_since(*entry) < Duration::from_millis(66) {
            return false;
        }
        *entry = now;
        return true;
    }
    topics.contains(&topic)
}

#[utoipa::path(get, path = "/stream/{camera_id}.mjpg", params(("ticket" = String, Query), ("framing" = Option<String>, Query, description = "`raw` serves the same multipart bytes as application/octet-stream")), responses((status = 200, description = "multipart/x-mixed-replace MJPEG stream"), (status = 401, body = ProblemJson)))]
async fn stream_mjpeg(
    State(state): State<Arc<ApiState>>,
    Path(camera_path): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<Response, ApiProblem> {
    let camera_id = camera_path
        .strip_suffix(".mjpg")
        .ok_or_else(|| {
            ApiProblem::new(
                StatusCode::NOT_FOUND,
                "stream_not_found",
                "stream not found",
            )
        })?
        .to_owned();
    let ticket_value = query
        .get("ticket")
        .ok_or_else(|| ApiProblem::unauthorized("missing_ticket", "preview ticket is required"))?;
    {
        let mut tickets = state
            .tickets
            .tickets
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(ticket) = tickets.get_mut(ticket_value) else {
            return Err(ApiProblem::unauthorized(
                "bad_ticket",
                "preview ticket is invalid",
            ));
        };
        if ticket.used
            || ticket.camera_id != camera_id
            || ticket.expires_at < OffsetDateTime::now_utc()
        {
            return Err(ApiProblem::unauthorized(
                "bad_ticket",
                "preview ticket is invalid or already used",
            ));
        }
        ticket.used = true;
    }
    // WKWebView's fetch() rejects multipart/x-mixed-replace bodies, so the desktop UI asks
    // for the identical byte stream under a neutral content type and parses parts itself.
    let content_type = if query.get("framing").is_some_and(|framing| framing == "raw") {
        "application/octet-stream"
    } else {
        "multipart/x-mixed-replace; boundary=flick"
    };
    let stream = mjpeg_stream(camera_id, state.preview.clone());
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

fn mjpeg_stream(
    camera_id: String,
    preview: Arc<dyn PreviewSource>,
) -> impl Stream<Item = Result<Bytes, Infallible>> {
    stream::unfold(
        (camera_id, preview, None::<u64>),
        |(camera_id, preview, mut last_seq)| async move {
            loop {
                tokio::time::sleep(PREVIEW_POLL).await;
                let Some(frame) = preview.next_frame(&camera_id).await else {
                    continue;
                };
                if last_seq == Some(frame.seq) {
                    continue;
                }
                last_seq = Some(frame.seq);
                let chunk = tokio::task::spawn_blocking(move || encode_mjpeg_chunk(frame))
                    .await
                    .ok()
                    .flatten();
                if let Some(chunk) = chunk {
                    return Some((Ok(chunk), (camera_id, preview, last_seq)));
                }
            }
        },
    )
}

/// Preview poll interval. Short relative to a 30 fps capture period so a new frame is picked
/// up within a few ms instead of beating against the camera cadence; the seq check keeps
/// each capture frame encoded once.
const PREVIEW_POLL: Duration = Duration::from_millis(4);
const PREVIEW_MAX_WIDTH: u32 = 640;

fn encode_mjpeg_chunk(frame: PreviewFrame) -> Option<Bytes> {
    let expected = (frame.width as usize) * (frame.height as usize) * 3;
    if frame.width == 0 || frame.height == 0 || frame.rgb.len() < expected {
        return None;
    }
    let (width, height, rgb) = if frame.width > PREVIEW_MAX_WIDTH {
        let (w, h, rgb) = downscale_rgb(&frame);
        (w, h, std::borrow::Cow::Owned(rgb))
    } else {
        (
            frame.width,
            frame.height,
            std::borrow::Cow::Borrowed(&frame.rgb[..expected]),
        )
    };
    let mut jpeg = Vec::with_capacity(64 * 1024);
    jpeg_encoder::Encoder::new(&mut jpeg, 75)
        .encode(
            &rgb,
            u16::try_from(width).ok()?,
            u16::try_from(height).ok()?,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    let header = format!(
        "--flick\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        jpeg.len()
    );
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(&jpeg);
    bytes.extend_from_slice(b"\r\n");
    Some(Bytes::from(bytes))
}

/// Integer-stride nearest downscale to at most [`PREVIEW_MAX_WIDTH`], read straight from the
/// shared frame so the full-resolution buffer is never copied.
fn downscale_rgb(frame: &PreviewFrame) -> (u32, u32, Vec<u8>) {
    let step = frame.width.div_ceil(PREVIEW_MAX_WIDTH) as usize;
    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    let (dst_w, dst_h) = (src_w.div_ceil(step), src_h.div_ceil(step));
    let mut out = Vec::with_capacity(dst_w * dst_h * 3);
    for y in (0..src_h).step_by(step) {
        let row = &frame.rgb[y * src_w * 3..(y + 1) * src_w * 3];
        for px in row.as_chunks::<3>().0.iter().step_by(step) {
            out.extend_from_slice(px);
        }
    }
    (dst_w as u32, dst_h as u32, out)
}

fn builtin_gestures() -> Vec<Gesture> {
    [
        ("builtin.thumb_up", "Thumb up", "👍"),
        ("builtin.thumb_down", "Thumb down", "👎"),
        ("builtin.open_palm", "Open palm", "✋"),
        ("builtin.point", "Point", "☝"),
        ("builtin.circle_cw", "Circle clockwise", "↻"),
        ("builtin.circle_ccw", "Circle counter-clockwise", "↺"),
        ("builtin.two_hand_separate", "Two-hand separate", "↕"),
    ]
    .into_iter()
    .map(|(id, name, icon)| Gesture {
        id: id.to_owned(),
        source: "builtin".to_owned(),
        kind: "static".to_owned(),
        hands_required: if id == "builtin.two_hand_separate" {
            2
        } else {
            1
        },
        name: name.to_owned(),
        icon: Some(icon.to_owned()),
        hand_constraint: "any".to_owned(),
        threshold: None,
        enabled: true,
        sample_count: 0,
        accuracy: None,
        distinctiveness: None,
    })
    .collect()
}

fn classifier_report() -> ClassifierReport {
    ClassifierReport {
        id: None,
        algorithm: "proto_knn".to_owned(),
        embedder_version: "none".to_owned(),
        active: false,
        metrics: json!({}),
        trained_at: None,
    }
}

/// Serializes the OpenAPI document as pretty JSON.
pub fn openapi_json_pretty() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Flick Local API", version = "0.1.0", description = "Local-first HTTP and WebSocket API for Flick."),
    paths(
        health, openapi_json, status, pause_engine, resume_engine, get_settings, patch_settings,
        ha_discover, ha_connect, ha_status, ha_delete, ha_update, ha_client_certificate,
        put_ha_client_certificate, delete_ha_client_certificate, put_network, ha_areas, ha_entities, ha_services, ha_call,
        cameras_available, list_cameras, create_camera, patch_camera, delete_camera, start_camera,
        stop_camera, preview_ticket, list_gestures, create_gesture, patch_gesture, delete_gesture,
        capture_gesture, motion_takes, patch_gesture_type, cancel_capture, gesture_samples,
        delete_sample, train_classifier, get_classifier, list_mappings, create_mapping,
        patch_mapping, delete_mapping, test_mapping, order_mappings, activity, list_places,
        patch_place, delete_place, start_realign, realign_point, realign_commit, list_anchors,
        patch_anchor, delete_anchor, test_anchor, start_teach, teach_spot, teach_level_current,
        teach_level_test, teach_commit, teach_cancel, setup_suggest, get_updates, check_updates,
        install_update, rollback_update, get_models, install_model, delete_model, export_pack,
        diagnostics_bundle, ws_events, stream_mjpeg
    ),
    components(schemas(
        ProblemJson, HealthResponse, EngineStatus, PauseRequest, StageLatency, HaDiscovery,
        HaConnectRequest, HaInstance, HaStatus, HaConnectionUpdate, HaClientCertificate,
        HaClientCertificateUpload, NetworkReport, HaArea, HaEntity, HaServiceSchema, ActionDto,
        ActionTargetDto, ActionOutcomeDto, LatencyBreakdown, CameraFormat, AvailableCamera,
        CameraCreate, CameraPatch, Camera, CameraStatus, PreviewTicket, GestureCreate,
        GesturePatch, Gesture, CaptureRequest, CaptureSession, MotionTake, GestureTypePatch,
        Sample, TrainRequest, ClassifierReport, TargetModeDto, MappingCreate, MappingPatch,
        Mapping, MappingOrder, ActivityPage, ActivityItem, Place, PlacePatch, RealignSession,
        RealignPointRequest, RealignPointResponse, RealignCommitResponse, Anchor, VerbBinding,
        AnchorPatch, AnchorTestResponse, TeachRequest, TeachSession, TeachSpotResponse,
        TeachLevelRequest, TeachLevelResponse, TeachCommitRequest, TeachVerb, TeachCommitResponse,
        SetupSuggestRequest, SetupSuggestion, UpdateStateDto, AppUpdateState, UpdateActionRequest,
        ModelPack, PackExportRequest, GesturePackDto, WsClientMessage, WsServerMessage, EmptyEvent,
        HelloEvent, HandsEvent, HandEvent, RayEvent, GestureCandidateEvent, GestureSuppressedEvent,
        GestureFiredEvent, GestureValueEvent, ArmedEvent, ConfirmRequiredEvent, ActionResultEvent,
        HaStatusEvent, HaEntityEvent, CaptureProgressEvent, EnginePausedEvent, TargetHoverEvent,
        TargetSelectedEvent, TargetClearedEvent, TargetAmbiguousEvent, PlaceStatusEvent,
        TeachProgressEvent, UpdateAvailableEvent, UpdateProgressEvent, UpdateReadyEvent,
        ModelActivatedEvent, ModelRolledBackEvent, ResyncEvent
    )),
    tags((name = "flick", description = "Flick local API"))
)]
struct ApiDoc;

#[allow(dead_code)]
fn _system_time_to_rfc3339(time: SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_else(|_| now_rfc3339())
}

#[allow(dead_code)]
fn _parse_json<T: DeserializeOwned>(value: Value) -> Result<T, ApiProblem> {
    serde_json::from_value(value)
        .map_err(|err| ApiProblem::validation("invalid_json", err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_hub_lag_reports_resync_without_blocking() {
        let hub = EventHub::new(1);
        let mut rx = hub.subscribe();
        for seq in 0..10 {
            hub.publish(WsServerMessage::Hands {
                ts: now_rfc3339(),
                payload: HandsEvent {
                    camera_id: "cam".to_owned(),
                    seq,
                    hands: vec![],
                    ray: None,
                },
            });
        }
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            other => panic!("expected lagged receiver, got {other:?}"),
        }
    }

    #[test]
    fn mjpeg_chunk_downscales_720p_to_valid_640_jpeg() {
        let frame = PreviewFrame {
            width: 1280,
            height: 720,
            seq: 0,
            rgb: vec![128u8; 1280 * 720 * 3].into(),
        };
        let chunk = encode_mjpeg_chunk(frame).expect("chunk");
        let body_start = chunk
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("header")
            + 4;
        let jpeg = &chunk[body_start..chunk.len() - 2];
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let sof = jpeg
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("SOF0");
        let height = u16::from_be_bytes([jpeg[sof + 5], jpeg[sof + 6]]);
        let width = u16::from_be_bytes([jpeg[sof + 7], jpeg[sof + 8]]);
        assert_eq!((width, height), (640, 360));
    }
}
