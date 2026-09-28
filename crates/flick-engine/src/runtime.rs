//! Runtime wiring for the engine binary and local API.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use flick_api::{
    ActionDto, ActionOutcomeDto, ActionTargetDto, Anchor as ApiAnchor, ApiConfig, ApiGateways,
    ApiProblem, ApiState, AvailableCamera, CameraFormat, CameraStatus, EngineControl, EngineStatus,
    FakeUpdates, HaArea, HaConnectRequest, HaDiscovery, HaEntity, HaGateway, HaInstance,
    HaServiceSchema, HaStatus as ApiHaStatus, LatencyBreakdown, PauseRequest, PreviewFrame,
    PreviewSource, RealignCommitResponse, RealignPointRequest, RealignPointResponse,
    RealignSession, SetupSuggestRequest, SetupSuggestion, StageLatency, TeachCommitRequest,
    TeachCommitResponse, TeachGateway, TeachLevelRequest, TeachLevelResponse, TeachRequest,
    TeachSession as ApiTeachSession, TeachSpotResponse, VerbBinding, router,
};
use flick_capture::{
    CameraPermissionStatus, CaptureHandle, CaptureStatus, FileSource, FileSourceOptions,
    LatestFrameSlot, LocalCameraOptions, LocalCameraSource, camera_permission_status,
    spawn_capture,
};
use flick_core::{
    Action, ActionOutcome, ActionStatus, ActionTarget, AnchorId, CameraId, DialProperty, Frame,
    FrameSource, HandFrame, HandPipeline, PlaceId, SourceInfo, SourceKind, Verb,
};
use flick_gestures::{GestureEngine, GestureEngineConfig};
use flick_ha::{
    EntityState, HaClient, HaConnectionConfig, HaStatus, RegistrySnapshot, SafetyInput,
    SafetyValidator, ServiceCallRecord,
};
use flick_spatial::{
    AnchorGeometry, CameraIntrinsics, DEFAULT_ESTIMATOR_VERSION, PlaceRecord, PlaceStatus,
    PointingRay, RaySource, RealignPair, StoredIntrinsics, TargetSelectorImpl,
    TargetSelectorSettings, TeachObservation, TeachSession as SpatialTeachSession, TeachTarget,
    realign,
};
use flick_store::Store;
use flick_vision::{EpChoice, HandPipelineImpl, ModelSet};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{net::TcpListener, sync::Mutex};

use crate::{
    config::RuntimeConfig,
    dispatcher::{
        Dispatcher, HaActionSink, NoopActionSink, owner_fan_anchor, owner_scenario_mappings,
    },
    fake_landmarks::{ReplayCatalog, ReplayFixture, replay_once},
    targeting_store::{SqliteTargetingStore, anchor_record_to_anchor, anchor_to_record},
};

/// Runs the API server until SIGTERM/Ctrl-C.
pub async fn serve(runtime: RuntimeConfig, token: String) -> anyhow::Result<()> {
    let requested_port = runtime.bootstrap.engine.port;
    let bind_addr = SocketAddr::from(([127, 0, 0, 1], requested_port));
    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;
    let actual_port = listener.local_addr()?.port();
    let mut api_config = api_config(&runtime, actual_port, token);
    api_config.protect_health = runtime.sidecar;

    let store = Arc::new(Store::open(&runtime.data_dir)?);
    let app = Arc::new(EngineApp::new(runtime.clone(), Arc::clone(&store)));
    let sink = if runtime.mock_ha {
        let client = start_mock_ha(&app).await?;
        Arc::new(HaActionSink::new(client)) as Arc<dyn flick_core::ActionSink>
    } else {
        Arc::new(NoopActionSink) as Arc<dyn flick_core::ActionSink>
    };

    let gateways = ApiGateways {
        engine: app.clone(),
        ha: app.clone(),
        teach: app.clone(),
        updates: Arc::new(FakeUpdates),
        preview: app.clone(),
    };
    let state = ApiState::new(api_config.clone(), gateways);
    let events = state.events();
    let dispatcher = Dispatcher::builder(sink)
        .store(store)
        .events(events)
        .mappings(owner_scenario_mappings())
        .anchors(vec![owner_fan_anchor()])
        .build();
    app.set_dispatcher(dispatcher).await;
    app.configure_fake_landmarks().await;

    if runtime.sidecar {
        println!(
            "{}",
            json!({"event":"ready","port":actual_port,"version":env!("CARGO_PKG_VERSION")})
        );
    } else if runtime.dev {
        println!(
            "flick-engine dev ready on http://127.0.0.1:{actual_port} (token: dev-token, mock_ha: {})",
            runtime.mock_ha
        );
    }

    let mut app_router: Router = router(state);
    if runtime.dev && !runtime.sidecar {
        app_router = app_router.merge(dev_router(app.clone(), api_config));
    }
    if let Some(path) = runtime.fake_camera.as_deref() {
        app.start_file_camera(path).await?;
    }
    if let Some(fixture) = runtime.fake_landmarks_autoplay.as_deref() {
        app.start_replay(fixture).await?;
    } else if runtime
        .fake_landmarks
        .as_ref()
        .is_some_and(|path| path.is_file())
    {
        app.start_replay("").await?;
    }
    axum::serve(listener, app_router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("API server failed")
}

fn dev_router(app: Arc<EngineApp>, config: ApiConfig) -> Router {
    Router::new()
        .route("/api/v1/dev/fixtures", get(dev_fixtures))
        .route("/api/v1/dev/replay", post(dev_replay))
        .route("/api/v1/dev/mock-ha/calls", get(dev_mock_ha_calls))
        .route_layer(middleware::from_fn_with_state(config, require_dev_auth))
        .with_state(app)
}

#[derive(Debug, Deserialize)]
struct DevReplayRequest {
    fixture: String,
}

#[derive(Debug, Serialize)]
struct DevReplayAccepted {
    fixture: String,
}

async fn dev_fixtures(
    State(app): State<Arc<EngineApp>>,
) -> Result<Json<Vec<ReplayFixture>>, ApiProblem> {
    app.replay_fixtures().await.map(Json)
}

async fn dev_replay(
    State(app): State<Arc<EngineApp>>,
    Json(request): Json<DevReplayRequest>,
) -> Result<(StatusCode, Json<DevReplayAccepted>), ApiProblem> {
    app.start_replay(&request.fixture)
        .await
        .map_err(|err| ApiProblem::validation("replay_start_failed", err.to_string()))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(DevReplayAccepted {
            fixture: request.fixture,
        }),
    ))
}

async fn dev_mock_ha_calls(State(app): State<Arc<EngineApp>>) -> Json<Vec<ServiceCallRecord>> {
    Json(app.mock_ha_calls().await)
}

async fn require_dev_auth(
    State(config): State<ApiConfig>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !dev_host_allowed(&config, &headers)
        || !dev_origin_allowed(&config, &headers)
        || !dev_token_allowed(&config, &headers)
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

fn dev_host_allowed(config: &ApiConfig, headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| config.allowed_hosts.contains(host))
}

fn dev_origin_allowed(config: &ApiConfig, headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| config.allowed_origins.contains(origin))
}

fn dev_token_allowed(config: &ApiConfig, headers: &HeaderMap) -> bool {
    let expected = format!("Bearer {}", config.token);
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn api_config(runtime: &RuntimeConfig, port: u16, token: String) -> ApiConfig {
    let mut config = ApiConfig::new(port, token);
    if runtime.dev {
        config.dev = true;
        config.token = "dev-token".to_owned();
        config
            .allowed_origins
            .insert("http://localhost:5173".to_owned());
    }
    config
}

async fn start_mock_ha(app: &Arc<EngineApp>) -> anyhow::Result<HaClient> {
    let scenario = bedroom_fan_scenario();
    let (url, token, handle) = flick_ha::mock::MockHa::start(scenario).await?;
    let config = HaConnectionConfig::new(url.clone(), token.clone())?;
    let client = HaClient::connect(config).await?;
    let snapshot = client.refresh_registry().await.unwrap_or_default();
    app.set_ha_client(
        client.clone(),
        Some(HaInstance {
            id: "mock-ha".to_owned(),
            name: "Mock Home".to_owned(),
            base_url: url,
            ha_uuid: Some("mock-ha".to_owned()),
            auth_kind: "llat".to_owned(),
            ha_version: Some("2026.9.0".to_owned()),
            is_default: true,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        }),
        snapshot,
    )
    .await;
    app.set_mock_ha_handle(handle).await;
    Ok(client)
}

fn bedroom_fan_scenario() -> flick_ha::mock::MockScenario {
    flick_ha::mock::MockScenario {
        token: "mock-token".to_owned(),
        ha_version: "2026.9.0".to_owned(),
        config: json!({"location_name":"Mock Home","version":"2026.9.0","uuid":"mock-ha"}),
        entities: vec![
            EntityState {
                entity_id: "light.bed_light".to_owned(),
                state: "off".to_owned(),
                attributes: serde_json::Map::from_iter([
                    ("friendly_name".to_owned(), json!("Bed light")),
                    ("supported_features".to_owned(), json!(0)),
                ]),
                last_changed: None,
                last_updated: None,
            },
            owner_fan_anchor().entity,
        ],
        services: json!({
            "light": {"toggle": {}, "turn_on": {}, "turn_off": {}},
            "fan": {"turn_on": {}, "turn_off": {}, "toggle": {}, "increase_speed": {}, "decrease_speed": {}}
        }),
        registries: flick_ha::mock::MockRegistries::default(),
        call_delay_ms: 0,
        call_errors: Vec::new(),
    }
}

struct EngineApp {
    runtime: RuntimeConfig,
    store: Arc<Store>,
    started_at: Instant,
    paused: Mutex<bool>,
    dispatcher: Mutex<Option<Dispatcher>>,
    ha_client: Mutex<Option<HaClient>>,
    mock_ha_handle: Mutex<Option<flick_ha::mock::MockHaHandle>>,
    ha_instance: Mutex<Option<HaInstance>>,
    registry: Mutex<RegistrySnapshot>,
    targeting: SqliteTargetingStore,
    teach_sessions: Mutex<HashMap<String, EngineTeachSession>>,
    realign_sessions: Mutex<HashMap<String, EngineRealignSession>>,
    capture: Mutex<Option<EngineCapture>>,
    replay_catalog: Mutex<Option<ReplayCatalog>>,
    replay_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl EngineApp {
    fn new(runtime: RuntimeConfig, store: Arc<Store>) -> Self {
        Self {
            runtime,
            store: Arc::clone(&store),
            started_at: Instant::now(),
            paused: Mutex::new(false),
            dispatcher: Mutex::new(None),
            ha_client: Mutex::new(None),
            mock_ha_handle: Mutex::new(None),
            ha_instance: Mutex::new(None),
            registry: Mutex::new(RegistrySnapshot::default()),
            targeting: SqliteTargetingStore::new(Arc::clone(&store)),
            teach_sessions: Mutex::new(HashMap::new()),
            realign_sessions: Mutex::new(HashMap::new()),
            capture: Mutex::new(None),
            replay_catalog: Mutex::new(None),
            replay_task: Mutex::new(None),
        }
    }

    async fn set_dispatcher(&self, dispatcher: Dispatcher) {
        *self.dispatcher.lock().await = Some(dispatcher);
    }

    async fn set_ha_client(
        &self,
        client: HaClient,
        instance: Option<HaInstance>,
        snapshot: RegistrySnapshot,
    ) {
        *self.ha_client.lock().await = Some(client);
        *self.ha_instance.lock().await = instance;
        *self.registry.lock().await = snapshot;
    }

    async fn set_mock_ha_handle(&self, handle: flick_ha::mock::MockHaHandle) {
        *self.mock_ha_handle.lock().await = Some(handle);
    }

    async fn configure_fake_landmarks(&self) {
        let catalog = self.runtime.fake_landmarks.clone().map(ReplayCatalog::new);
        *self.replay_catalog.lock().await = catalog;
    }

    async fn replay_fixtures(&self) -> Result<Vec<ReplayFixture>, ApiProblem> {
        let catalog = self.replay_catalog.lock().await.clone().ok_or_else(|| {
            ApiProblem::validation("fake_landmarks_missing", "FLICK_FAKE_LANDMARKS is not set")
        })?;
        catalog
            .fixtures()
            .map_err(|err| ApiProblem::validation("fixture_list_failed", err.to_string()))
    }

    async fn start_replay(&self, fixture: &str) -> anyhow::Result<()> {
        let catalog = self
            .replay_catalog
            .lock()
            .await
            .clone()
            .context("FLICK_FAKE_LANDMARKS is not set")?;
        let (fixture_name, path) = catalog.resolve(fixture)?;
        let dispatcher = self
            .dispatcher
            .lock()
            .await
            .clone()
            .context("dispatcher is not ready")?;
        let handle = tokio::spawn(async move {
            match replay_once(fixture_name.clone(), path, dispatcher).await {
                Ok(stats) => tracing::info!(
                    fixture = %stats.fixture,
                    frames = stats.frames,
                    events = stats.events,
                    dispatched = stats.dispatched,
                    "fake landmark replay completed"
                ),
                Err(error) => {
                    tracing::warn!(%error, fixture = %fixture_name, "fake landmark replay failed")
                }
            }
        });
        let mut task = self.replay_task.lock().await;
        if let Some(previous) = task.take() {
            previous.abort();
        }
        *task = Some(handle);
        Ok(())
    }

    async fn mock_ha_calls(&self) -> Vec<ServiceCallRecord> {
        let handle = self.mock_ha_handle.lock().await.clone();
        if let Some(handle) = handle {
            handle.calls().await
        } else {
            Vec::new()
        }
    }

    async fn start_file_camera(&self, path: &Path) -> anyhow::Result<CameraStatus> {
        let source = FileSource::open(FileSourceOptions::fake_camera(path))?;
        self.start_source(source).await
    }

    async fn start_source<S>(&self, source: S) -> anyhow::Result<CameraStatus>
    where
        S: FrameSource,
    {
        let dispatcher = self.dispatcher.lock().await.clone();
        let model_root = model_manifest_path();
        let capture = EngineCapture::start(source, dispatcher, model_root.as_deref())?;
        let status = capture.status();
        let mut current = self.capture.lock().await;
        if let Some(old) = current.take() {
            let _ = old.stop();
        }
        *current = Some(capture);
        Ok(status)
    }

    fn ensure_place(&self, camera_id: CameraId) -> Result<PlaceRecord, ApiProblem> {
        let places = self
            .targeting
            .try_places_for_camera(camera_id)
            .map_err(store_problem)?;
        if let Some(place) = places.iter().find(|place| place.active).cloned() {
            return Ok(place);
        }
        if let Some(place) = places.first().cloned() {
            return Ok(place);
        }
        let intrinsics = flick_spatial::CameraIntrinsics::sane_default(1280, 720);
        let mut scene_signature = [0.0_f32; 384];
        scene_signature[0] = 1.0;
        let now = now_ms();
        let place = PlaceRecord {
            id: PlaceId::new(),
            camera_id,
            name: "Default place".to_owned(),
            scene_signature,
            embedder_version: "engine.stub.scene.v1".to_owned(),
            intrinsics: StoredIntrinsics::from(&intrinsics),
            status: PlaceStatus::Ok,
            active: true,
            created_at: now,
            updated_at: now,
        };
        self.targeting
            .try_upsert_place(&place)
            .map_err(store_problem)?;
        Ok(place)
    }

    async fn ha_status_dto(&self) -> ApiHaStatus {
        let client = self.ha_client.lock().await.clone();
        let instance = self.ha_instance.lock().await.clone();
        let Some(client) = client else {
            return ApiHaStatus {
                state: "disconnected".to_owned(),
                ha_version: None,
                instance,
            };
        };
        let status = client.status().borrow().clone();
        match status {
            HaStatus::Disconnected => ApiHaStatus {
                state: "disconnected".to_owned(),
                ha_version: None,
                instance,
            },
            HaStatus::Connecting | HaStatus::Reconnecting { .. } => ApiHaStatus {
                state: "connecting".to_owned(),
                ha_version: None,
                instance,
            },
            HaStatus::Ready { ha_version } => ApiHaStatus {
                state: "ready".to_owned(),
                ha_version,
                instance,
            },
            HaStatus::AuthFailed { .. } => ApiHaStatus {
                state: "auth_failed".to_owned(),
                ha_version: None,
                instance,
            },
        }
    }
}

struct EngineTeachSession {
    camera_id: String,
    target: serde_json::Value,
    levels: Vec<f64>,
    spatial: SpatialTeachSession,
}

struct EngineRealignSession {
    place_id: PlaceId,
    pairs: Vec<RealignPair>,
}

struct EngineCapture {
    source_info: SourceInfo,
    handle: Option<CaptureHandle>,
    worker_stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    latest_frame: Arc<std::sync::Mutex<Option<Frame>>>,
    latest_hands: Arc<std::sync::Mutex<Option<HandFrame>>>,
    last_error: Arc<std::sync::Mutex<Option<String>>>,
}

impl EngineCapture {
    fn start<S>(
        source: S,
        dispatcher: Option<Dispatcher>,
        model_root: Option<&Path>,
    ) -> anyhow::Result<Self>
    where
        S: FrameSource,
    {
        let source_info = source.info().clone();
        let slot = LatestFrameSlot::new();
        let handle = spawn_capture(source, slot.clone());
        let worker_stop = Arc::new(AtomicBool::new(false));
        let latest_frame = Arc::new(std::sync::Mutex::new(None));
        let latest_hands = Arc::new(std::sync::Mutex::new(None));
        let last_error = Arc::new(std::sync::Mutex::new(None));
        let worker = {
            let worker_stop = Arc::clone(&worker_stop);
            let latest_frame = Arc::clone(&latest_frame);
            let latest_hands = Arc::clone(&latest_hands);
            let last_error = Arc::clone(&last_error);
            let source_info = source_info.clone();
            let handle = tokio::runtime::Handle::current();
            let mut pipeline = build_hand_pipeline(model_root, source_info.mirror);
            let mut gestures = GestureEngine::new(GestureEngineConfig::default());
            let mut selector = TargetSelectorImpl::new(
                CameraIntrinsics::sane_default(source_info.width, source_info.height),
                Vec::new(),
                TargetSelectorSettings::default(),
            );
            thread::Builder::new()
                .name(format!("flick-vision-{}", source_info.id))
                .spawn(move || {
                    while !worker_stop.load(Ordering::Relaxed) {
                        let Some(frame) = slot.wait_latest(Duration::from_millis(100)) else {
                            continue;
                        };
                        if let Ok(mut latest) = latest_frame.lock() {
                            *latest = Some(frame.clone());
                        }
                        match pipeline.process(&frame) {
                            Ok(hands) => {
                                let selection = selector.update(&hands, None);
                                if let Some(dispatcher) = dispatcher.as_ref() {
                                    for event in gestures.update(&hands, &selection).events {
                                        let dispatcher = dispatcher.clone();
                                        handle.block_on(async move {
                                            let _ = dispatcher.dispatch(&event).await;
                                        });
                                    }
                                }
                                if let Ok(mut latest) = latest_hands.lock() {
                                    *latest = Some(hands);
                                }
                                if let Ok(mut error) = last_error.lock() {
                                    *error = None;
                                }
                            }
                            Err(err) => {
                                if let Ok(mut error) = last_error.lock() {
                                    *error = Some(err.to_string());
                                }
                            }
                        }
                    }
                })
                .context("failed to spawn vision worker")?
        };
        Ok(Self {
            source_info,
            handle: Some(handle),
            worker_stop,
            worker: Some(worker),
            latest_frame,
            latest_hands,
            last_error,
        })
    }

    fn status(&self) -> CameraStatus {
        let (state, error) = self
            .handle
            .as_ref()
            .map_or(("stopped".to_owned(), None), |handle| {
                capture_status_parts(handle.status())
            });
        let error = error.or_else(|| self.last_error.lock().ok().and_then(|value| value.clone()));
        CameraStatus {
            camera_id: self.source_info.id.to_string(),
            state,
            fps: Some(f64::from(self.source_info.fps)),
            error,
        }
    }

    fn latest_preview(&self) -> Option<PreviewFrame> {
        let frame = self.latest_frame.lock().ok()?.clone()?;
        if frame.format != flick_core::PixelFormat::Rgb8 {
            return None;
        }
        Some(PreviewFrame {
            width: frame.width,
            height: frame.height,
            rgb: frame.data.to_vec(),
        })
    }

    fn stop(mut self) -> CameraStatus {
        self.worker_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        CameraStatus {
            camera_id: self.source_info.id.to_string(),
            state: "stopped".to_owned(),
            fps: Some(0.0),
            error: None,
        }
    }
}

impl Drop for EngineCapture {
    fn drop(&mut self) {
        self.worker_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[async_trait]
impl EngineControl for EngineApp {
    async fn status(&self) -> EngineStatus {
        let paused = *self.paused.lock().await;
        let _ = self.started_at.elapsed();
        let _ = self.store.db_path();
        let _ = self.dispatcher.lock().await.is_some();
        let capture = self.capture.lock().await;
        let cameras = capture
            .as_ref()
            .map(|capture| vec![capture.status()])
            .unwrap_or_else(|| {
                vec![CameraStatus {
                    camera_id: "dev-camera".to_owned(),
                    state: "idle".to_owned(),
                    fps: Some(0.0),
                    error: None,
                }]
            });
        let stages = capture
            .as_ref()
            .and_then(|capture| {
                capture
                    .latest_hands
                    .lock()
                    .ok()
                    .and_then(|hands| hands.as_ref().map(|hands| hands.timings))
            })
            .map(|timings| {
                vec![
                    StageLatency {
                        stage: "capture".to_owned(),
                        p50_ms: f64::from(timings.capture_ms),
                        p95_ms: f64::from(timings.capture_ms),
                    },
                    StageLatency {
                        stage: "recognize".to_owned(),
                        p50_ms: f64::from(timings.total_ms),
                        p95_ms: f64::from(timings.total_ms),
                    },
                    StageLatency {
                        stage: "dispatch".to_owned(),
                        p50_ms: 0.0,
                        p95_ms: 0.0,
                    },
                ]
            })
            .unwrap_or_else(default_stages);
        EngineStatus {
            paused,
            paused_until: None,
            cameras,
            ha: self.ha_status_dto().await,
            stages,
            camera_permission: Some(permission_status_str(camera_permission_status()).to_owned()),
        }
    }

    async fn pause(&self, _request: PauseRequest) -> EngineStatus {
        *self.paused.lock().await = true;
        EngineControl::status(self).await
    }

    async fn resume(&self) -> EngineStatus {
        *self.paused.lock().await = false;
        EngineControl::status(self).await
    }

    async fn available_cameras(&self) -> Vec<AvailableCamera> {
        if let Some(path) = &self.runtime.fake_camera {
            return vec![available_camera(
                path.display().to_string(),
                "Fake camera".to_owned(),
                SourceKind::File,
            )];
        }
        match LocalCameraSource::enumerate() {
            Ok(devices) if !devices.is_empty() => devices
                .into_iter()
                .map(|device| available_camera(device.stable_id, device.name, SourceKind::Local))
                .collect(),
            _ => vec![available_camera(
                "0".to_owned(),
                "Local camera".to_owned(),
                SourceKind::Local,
            )],
        }
    }

    async fn start_camera(&self, camera_id: &str) -> CameraStatus {
        let result = if let Some(path) = self.runtime.fake_camera.as_deref() {
            self.start_file_camera(path).await
        } else {
            let source_id = CameraId::from_str(camera_id).unwrap_or_else(|_| CameraId::new());
            let index = camera_id.parse::<u32>().unwrap_or(0);
            match LocalCameraSource::open(LocalCameraOptions {
                camera_id: source_id,
                index,
                width: 1280,
                height: 720,
                fps: 30,
                mirror: true,
            }) {
                Ok(source) => self.start_source(source).await,
                Err(err) => Err(anyhow::Error::from(err)),
            }
        };
        match result {
            Ok(status) => status,
            Err(err) => CameraStatus {
                camera_id: camera_id.to_owned(),
                state: "error".to_owned(),
                fps: Some(0.0),
                error: Some(err.to_string()),
            },
        }
    }

    async fn stop_camera(&self, camera_id: &str) -> CameraStatus {
        let mut capture = self.capture.lock().await;
        if let Some(capture) = capture.take() {
            capture.stop()
        } else {
            CameraStatus {
                camera_id: camera_id.to_owned(),
                state: "stopped".to_owned(),
                fps: Some(0.0),
                error: None,
            }
        }
    }
}

#[async_trait]
impl TeachGateway for EngineApp {
    async fn start(&self, request: TeachRequest) -> Result<ApiTeachSession, ApiProblem> {
        let target = teach_target_from_value(&request.target)?;
        let anchor_id = request
            .anchor_id
            .as_deref()
            .map(AnchorId::from_str)
            .transpose()
            .map_err(|err| ApiProblem::validation("bad_anchor_id", err.to_string()))?
            .unwrap_or_else(AnchorId::new);
        let domain = teach_domain(&target);
        let name = teach_name(&target);
        let spatial =
            SpatialTeachSession::new(anchor_id, name, target, domain, DEFAULT_ESTIMATOR_VERSION);
        let session = ApiTeachSession {
            id: flick_core::TeachSessionId::new().to_string(),
            camera_id: request.camera_id,
            target: request.target,
            anchor_id: request.anchor_id,
            prompt: "Point at the device and hold still".to_owned(),
        };
        self.teach_sessions.lock().await.insert(
            session.id.clone(),
            EngineTeachSession {
                camera_id: session.camera_id.clone(),
                target: session.target.clone(),
                levels: Vec::new(),
                spatial,
            },
        );
        Ok(session)
    }

    async fn spot(&self, session_id: &str) -> Result<TeachSpotResponse, ApiProblem> {
        let mut sessions = self.teach_sessions.lock().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            ApiProblem::validation("teach_session_not_found", "teach session not found")
        })?;
        let spot_index = u32::try_from(session.spatial.observations().len() + 1)
            .map_err(|err| ApiProblem::validation("too_many_spots", err.to_string()))?;
        let observation = TeachObservation {
            spot_index,
            ray: PointingRay::new([0.0, 0.0, 0.0], [0.0, -0.1, 1.0], RaySource::FingerOnly),
            frames: 1,
            ray_jitter_deg: 0.0,
        };
        session.spatial.add_observation(observation);
        Ok(TeachSpotResponse {
            spot_index,
            ray_jitter_deg: 0.0,
            confidence: if spot_index >= 2 { 0.9 } else { 0.7 },
            kind: if spot_index >= 2 {
                "point3d"
            } else {
                "direction"
            }
            .to_owned(),
            residual_deg: None,
        })
    }

    async fn use_current_level(
        &self,
        session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<TeachLevelResponse, ApiProblem> {
        let mut sessions = self.teach_sessions.lock().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            ApiProblem::validation("teach_session_not_found", "teach session not found")
        })?;
        let level = f64::from(request.level);
        if !session.levels.contains(&level) {
            session.levels.push(level);
            session.levels.sort_by(f64::total_cmp);
        }
        Ok(TeachLevelResponse {
            levels: session.levels.clone(),
            current_percentage: level,
        })
    }

    async fn test_level(
        &self,
        session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<ActionOutcomeDto, ApiProblem> {
        let sessions = self.teach_sessions.lock().await;
        let session = sessions.get(session_id).ok_or_else(|| {
            ApiProblem::validation("teach_session_not_found", "teach session not found")
        })?;
        let Some(entity_id) = target_entity_id(&session.target) else {
            return Err(ApiProblem::validation(
                "unsupported_target",
                "level test requires an entity target",
            ));
        };
        let domain = entity_id
            .split_once('.')
            .map(|(domain, _)| domain)
            .unwrap_or("homeassistant");
        let action = Action::CallService {
            domain: domain.to_owned(),
            service: "turn_on".to_owned(),
            target: ActionTarget {
                entity_id: Some(vec![entity_id.to_owned()]),
                device_id: None,
                area_id: None,
            },
            data: json!({"percentage": request.level}),
            preset: Some("teach.level_test".to_owned()),
        };
        let client = self.ha_client.lock().await.clone();
        let outcome = if let Some(client) = client {
            client.call(action).await
        } else {
            ActionOutcome {
                activity_id: None,
                status: ActionStatus::Suppressed,
                error_code: Some("ha.not_configured".to_owned()),
                message: Some("Home Assistant is not configured".to_owned()),
                ha_context_id: None,
                latency_ms: None,
            }
        };
        Ok(outcome_to_dto(outcome))
    }

    async fn commit(
        &self,
        session_id: &str,
        request: TeachCommitRequest,
    ) -> Result<TeachCommitResponse, ApiProblem> {
        let session = self
            .teach_sessions
            .lock()
            .await
            .remove(session_id)
            .ok_or_else(|| {
                ApiProblem::validation("teach_session_not_found", "teach session not found")
            })?;
        let camera_id = CameraId::from_str(&session.camera_id)
            .map_err(|err| ApiProblem::validation("bad_camera_id", err.to_string()))?;
        let place = self.ensure_place(camera_id)?;
        let existing = self
            .targeting
            .try_anchors_for_place(place.id)
            .map_err(store_problem)?
            .iter()
            .filter_map(anchor_record_to_anchor)
            .collect::<Vec<_>>();
        let mut outcome = session
            .spatial
            .finish(&existing)
            .map_err(|err| ApiProblem::validation("teach_failed", err.to_string()))?;
        if let Some(name) = request.name {
            outcome.anchor.name = name;
        }
        if !session.levels.is_empty() {
            outcome.anchor.verb_params = json!({ "levels": session.levels });
        }
        let record = anchor_to_record(place.id, &outcome.anchor, false, false, now_ms());
        self.targeting
            .try_upsert_anchor(&record)
            .map_err(store_problem)?;
        Ok(TeachCommitResponse {
            anchor: api_anchor_from_record(&record),
            mapping_ids: request
                .verbs
                .into_iter()
                .map(|verb| verb.gesture_id)
                .collect(),
            distinctiveness_warnings: outcome
                .distinctiveness_warnings
                .into_iter()
                .map(|warning| warning.other_anchor_id.to_string())
                .collect(),
        })
    }

    async fn cancel(&self, session_id: &str) -> Result<(), ApiProblem> {
        self.teach_sessions.lock().await.remove(session_id);
        Ok(())
    }

    async fn start_realign(&self, place_id: &str) -> Result<RealignSession, ApiProblem> {
        let place_id = PlaceId::from_str(place_id)
            .map_err(|err| ApiProblem::validation("bad_place_id", err.to_string()))?;
        let anchors = self
            .targeting
            .try_anchors_for_place(place_id)
            .map_err(store_problem)?;
        let session = RealignSession {
            id: flick_core::TeachSessionId::new().to_string(),
            prompts: anchors
                .iter()
                .filter(|anchor| anchor.status == flick_core::AnchorStatus::Ok)
                .map(|anchor| format!("Point at {}", anchor.name))
                .collect(),
        };
        self.realign_sessions.lock().await.insert(
            session.id.clone(),
            EngineRealignSession {
                place_id,
                pairs: Vec::new(),
            },
        );
        Ok(session)
    }

    async fn realign_point(
        &self,
        session_id: &str,
        request: RealignPointRequest,
    ) -> Result<RealignPointResponse, ApiProblem> {
        let anchor_id = AnchorId::from_str(&request.anchor_id)
            .map_err(|err| ApiProblem::validation("bad_anchor_id", err.to_string()))?;
        let record = self
            .targeting
            .try_anchor(anchor_id)
            .map_err(store_problem)?
            .ok_or_else(|| ApiProblem::validation("anchor_not_found", "anchor not found"))?;
        let anchor = anchor_record_to_anchor(&record)
            .ok_or_else(|| ApiProblem::validation("anchor_geometry", "unsupported anchor"))?;
        let direction = anchor.direction_from([0.0, 0.0, 0.0]);
        let mut sessions = self.realign_sessions.lock().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            ApiProblem::validation("realign_session_not_found", "realign session not found")
        })?;
        session.pairs.push(RealignPair {
            anchor_id,
            old_direction: direction,
            new_direction: direction,
        });
        let residual_deg = if session.pairs.len() >= 2 {
            realign(&session.pairs)
                .ok()
                .map(|result| f64::from(result.residual_deg))
        } else {
            None
        };
        Ok(RealignPointResponse {
            captured: true,
            residual_deg,
        })
    }

    async fn realign_commit(&self, session_id: &str) -> Result<RealignCommitResponse, ApiProblem> {
        let session = self
            .realign_sessions
            .lock()
            .await
            .remove(session_id)
            .ok_or_else(|| {
                ApiProblem::validation("realign_session_not_found", "realign session not found")
            })?;
        let result = realign(&session.pairs)
            .map_err(|err| ApiProblem::validation("realign_failed", err.to_string()))?;
        let records = self
            .targeting
            .try_anchors_for_place(session.place_id)
            .map_err(store_problem)?;
        for record in records {
            let Some(mut anchor) = anchor_record_to_anchor(&record) else {
                continue;
            };
            anchor.geometry = match anchor.geometry {
                AnchorGeometry::Point3d {
                    position,
                    covariance,
                } => AnchorGeometry::Point3d {
                    position: result.transform.transform_point(position),
                    covariance,
                },
                AnchorGeometry::Direction {
                    direction,
                    teach_origin,
                    covariance,
                } => AnchorGeometry::Direction {
                    direction: result.transform.transform_direction(direction),
                    teach_origin,
                    covariance,
                },
            };
            let mut updated = anchor_to_record(
                record.place_id,
                &anchor,
                record.sensitive,
                record.sensitive_ack,
                now_ms(),
            );
            updated.created_at = record.created_at;
            updated.last_used_at = record.last_used_at;
            self.targeting
                .try_upsert_anchor(&updated)
                .map_err(store_problem)?;
        }
        Ok(RealignCommitResponse {
            applied: true,
            residual_deg: f64::from(result.residual_deg),
            needs_reteach: Vec::new(),
        })
    }

    async fn suggest(
        &self,
        _request: SetupSuggestRequest,
    ) -> Result<Vec<SetupSuggestion>, ApiProblem> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl HaGateway for EngineApp {
    async fn discover(&self) -> Vec<HaDiscovery> {
        if self.runtime.mock_ha {
            return vec![HaDiscovery {
                name: "Mock Home".to_owned(),
                base_url: "mock://home".to_owned(),
                uuid: Some("mock-ha".to_owned()),
                version: Some("2026.9.0".to_owned()),
            }];
        }
        flick_ha::discover_instances(std::time::Duration::from_secs(3))
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|item| HaDiscovery {
                name: item.name,
                base_url: item.base_url,
                uuid: item.uuid,
                version: item.version,
            })
            .collect()
    }

    async fn connect(&self, request: HaConnectRequest) -> Result<HaInstance, ApiProblem> {
        let config = HaConnectionConfig::new(request.base_url.clone(), request.token)
            .map_err(|err| ApiProblem::validation("ha_invalid_url", err.to_string()))?;
        let client = HaClient::connect(config)
            .await
            .map_err(|err| ApiProblem::validation("ha_connect_failed", err.to_string()))?;
        let snapshot = client.refresh_registry().await.unwrap_or_default();
        let instance = HaInstance {
            id: flick_core::HaInstanceId::new().to_string(),
            name: "Home Assistant".to_owned(),
            base_url: request.base_url,
            ha_uuid: None,
            auth_kind: "llat".to_owned(),
            ha_version: None,
            is_default: true,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        self.set_ha_client(client, Some(instance.clone()), snapshot)
            .await;
        Ok(instance)
    }

    async fn status(&self) -> ApiHaStatus {
        self.ha_status_dto().await
    }

    async fn delete(&self) -> Result<(), ApiProblem> {
        *self.ha_client.lock().await = None;
        *self.ha_instance.lock().await = None;
        Ok(())
    }

    async fn areas(&self) -> Vec<HaArea> {
        self.registry
            .lock()
            .await
            .areas
            .iter()
            .map(|area| HaArea {
                area_id: area.area_id.clone(),
                name: area.name.clone(),
                floor_id: area.floor_id.clone(),
            })
            .collect()
    }

    async fn entities(&self, query: BTreeMap<String, String>) -> Vec<HaEntity> {
        let domain = query.get("domain").map(String::as_str);
        let area_id = query.get("area_id").map(String::as_str);
        let q = query.get("q").map(String::as_str);
        self.registry
            .lock()
            .await
            .filter(domain, area_id, q)
            .into_iter()
            .flat_map(|group| group.entities)
            .map(|entity| HaEntity {
                entity_id: entity.entity_id,
                name: entity.name,
                domain: entity.domain,
                area_id: entity.area_id,
                device_id: entity.device_id,
                state: Some(entity.state),
                device_class: entity.device_class,
                supported_features: Some(entity.supported_features),
                attributes: json!({}),
            })
            .collect()
    }

    async fn services(&self, query: BTreeMap<String, String>) -> Vec<HaServiceSchema> {
        let snapshot = self.registry.lock().await.clone();
        if let Some(domain) = query.get("domain") {
            let services = snapshot
                .services
                .get(domain)
                .cloned()
                .unwrap_or_else(|| json!({}));
            return vec![HaServiceSchema {
                domain: domain.clone(),
                services,
            }];
        }
        snapshot
            .services
            .as_object()
            .into_iter()
            .flat_map(|object| object.iter())
            .map(|(domain, services)| HaServiceSchema {
                domain: domain.clone(),
                services: services.clone(),
            })
            .collect()
    }

    async fn call(&self, action: ActionDto) -> Result<ActionOutcomeDto, ApiProblem> {
        let action = action_from_dto(action)?;
        validate_safety(&action)?;
        let client = self.ha_client.lock().await.clone();
        let outcome = if let Some(client) = client {
            client.call(action).await
        } else {
            ActionOutcome {
                activity_id: None,
                status: ActionStatus::Suppressed,
                error_code: Some("ha.not_configured".to_owned()),
                message: Some("Home Assistant is not configured".to_owned()),
                ha_context_id: None,
                latency_ms: None,
            }
        };
        Ok(outcome_to_dto(outcome))
    }
}

#[async_trait]
impl PreviewSource for EngineApp {
    async fn next_frame(&self, _camera_id: &str) -> Option<PreviewFrame> {
        if let Some(frame) = self
            .capture
            .lock()
            .await
            .as_ref()
            .and_then(EngineCapture::latest_preview)
        {
            return Some(frame);
        }
        let width = 320;
        let height = 180;
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.push((x % 256) as u8);
                rgb.push((y % 256) as u8);
                rgb.push(64);
            }
        }
        Some(PreviewFrame {
            width: width as u32,
            height: height as u32,
            rgb,
        })
    }
}

fn build_hand_pipeline(model_root: Option<&Path>, mirrored: bool) -> HandPipelineImpl {
    let Some(model_root) = model_root else {
        return HandPipelineImpl::without_models().with_mirrored_input(mirrored);
    };
    match ModelSet::load(model_root)
        .and_then(|models| HandPipelineImpl::new(models, EpChoice::default()))
    {
        Ok(pipeline) => pipeline.with_mirrored_input(mirrored),
        Err(err) => {
            tracing::warn!(
                error = %err,
                model_root = %model_root.display(),
                "vision models unavailable; using model-free hand pipeline"
            );
            HandPipelineImpl::without_models().with_mirrored_input(mirrored)
        }
    }
}

fn model_manifest_path() -> Option<PathBuf> {
    let path = PathBuf::from("models/manifest.toml");
    path.exists().then_some(path)
}

fn capture_status_parts(status: CaptureStatus) -> (String, Option<String>) {
    match status {
        CaptureStatus::Running => ("running".to_owned(), None),
        CaptureStatus::Disconnected => ("reconnecting".to_owned(), None),
        CaptureStatus::Stopped => ("stopped".to_owned(), None),
        CaptureStatus::Failed(message) => ("error".to_owned(), Some(message)),
    }
}

fn permission_status_str(status: CameraPermissionStatus) -> &'static str {
    match status {
        CameraPermissionStatus::Authorized => "authorized",
        CameraPermissionStatus::NotAuthorized => "denied",
        CameraPermissionStatus::Unknown => "unknown",
    }
}

fn default_stages() -> Vec<StageLatency> {
    vec![
        StageLatency {
            stage: "capture".to_owned(),
            p50_ms: 0.0,
            p95_ms: 0.0,
        },
        StageLatency {
            stage: "recognize".to_owned(),
            p50_ms: 0.0,
            p95_ms: 0.0,
        },
        StageLatency {
            stage: "dispatch".to_owned(),
            p50_ms: 0.0,
            p95_ms: 0.0,
        },
    ]
}

fn available_camera(device_ref: String, name: String, kind: SourceKind) -> AvailableCamera {
    AvailableCamera {
        device_ref,
        name,
        kind: source_kind_str(kind).to_owned(),
        formats: vec![CameraFormat {
            width: 1280,
            height: 720,
            fps: 30,
            format: "rgb".to_owned(),
        }],
    }
}

fn source_kind_str(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Local => "local",
        SourceKind::Rtsp => "rtsp",
        SourceKind::File => "file",
    }
}

fn action_from_dto(action: ActionDto) -> Result<Action, ApiProblem> {
    match action {
        ActionDto::CallService {
            domain,
            service,
            target,
            data,
            preset,
        } => Ok(Action::CallService {
            domain,
            service,
            target: target_from_dto(target),
            data,
            preset,
        }),
        ActionDto::Dial {
            entity_id,
            property,
            gain,
            min,
            max,
        } => Ok(Action::Dial {
            entity_id,
            property: dial_property(&property)?,
            gain,
            min,
            max,
        }),
        ActionDto::Verb { verb, level } => Ok(Action::Verb {
            verb: verb_from_str(&verb)?,
            level,
        }),
    }
}

fn target_from_dto(target: ActionTargetDto) -> ActionTarget {
    ActionTarget {
        entity_id: target.entity_id,
        device_id: target.device_id,
        area_id: target.area_id,
    }
}

fn dial_property(value: &str) -> Result<DialProperty, ApiProblem> {
    match value {
        "brightness_pct" => Ok(DialProperty::BrightnessPct),
        "volume_level" => Ok(DialProperty::VolumeLevel),
        "position" => Ok(DialProperty::Position),
        "percentage" => Ok(DialProperty::Percentage),
        "temperature" => Ok(DialProperty::Temperature),
        _ => Err(ApiProblem::validation(
            "bad_dial_property",
            "unknown dial property",
        )),
    }
}

fn verb_from_str(value: &str) -> Result<Verb, ApiProblem> {
    match value {
        "up" => Ok(Verb::Up),
        "down" => Ok(Verb::Down),
        "on" => Ok(Verb::On),
        "off" => Ok(Verb::Off),
        "stop" => Ok(Verb::Stop),
        "toggle" => Ok(Verb::Toggle),
        "level_set" => Ok(Verb::LevelSet),
        _ => Err(ApiProblem::validation("bad_verb", "unknown verb")),
    }
}

fn validate_safety(action: &Action) -> Result<(), ApiProblem> {
    let Action::CallService {
        domain,
        service,
        target,
        ..
    } = action
    else {
        return Err(ApiProblem::validation(
            "unsupported_action",
            "Only call_service can be sent directly",
        ));
    };
    let entity_domain = target
        .entity_id
        .as_ref()
        .and_then(|ids| ids.first())
        .and_then(|id| id.split_once('.').map(|(domain, _)| domain));
    let class = SafetyValidator::new().classify(&SafetyInput {
        domain,
        service,
        entity_domain,
        device_class: None,
    });
    if matches!(
        class,
        flick_ha::SafetyClass::Denied | flick_ha::SafetyClass::Sensitive
    ) {
        return Err(ApiProblem::validation(
            "safety_blocked",
            "action is blocked by the safety policy",
        ));
    }
    Ok(())
}

fn outcome_to_dto(outcome: ActionOutcome) -> ActionOutcomeDto {
    ActionOutcomeDto {
        status: match outcome.status {
            ActionStatus::Sent => "sent",
            ActionStatus::Ok => "ok",
            ActionStatus::Error => "error",
            ActionStatus::Timeout => "timeout",
            ActionStatus::Stale => "stale",
            ActionStatus::Suppressed => "suppressed",
        }
        .to_owned(),
        error_code: outcome.error_code,
        message: outcome.message,
        ha_context_id: outcome.ha_context_id,
        latency: Some(LatencyBreakdown {
            detect_ms: None,
            dispatch_ms: None,
            ha_ms: outcome.latency_ms.map(|value| value as f64),
        }),
    }
}

fn teach_target_from_value(value: &serde_json::Value) -> Result<TeachTarget, ApiProblem> {
    serde_json::from_value(value.clone())
        .map_err(|err| ApiProblem::validation("bad_teach_target", err.to_string()))
}

fn teach_domain(target: &TeachTarget) -> String {
    match target {
        TeachTarget::Entity(entity_id) => entity_id
            .split_once('.')
            .map(|(domain, _)| domain.to_owned())
            .unwrap_or_else(|| "homeassistant".to_owned()),
        TeachTarget::Device(_) | TeachTarget::Area(_) => "homeassistant".to_owned(),
    }
}

fn teach_name(target: &TeachTarget) -> String {
    match target {
        TeachTarget::Entity(entity_id) => entity_id.clone(),
        TeachTarget::Device(device_id) => device_id.clone(),
        TeachTarget::Area(area_id) => area_id.clone(),
    }
}

fn target_entity_id(target: &serde_json::Value) -> Option<&str> {
    target.get("entity_id").and_then(serde_json::Value::as_str)
}

fn api_anchor_from_record(record: &flick_spatial::AnchorRecord) -> ApiAnchor {
    ApiAnchor {
        id: record.id.to_string(),
        place_id: record.place_id.to_string(),
        name: record.name.clone(),
        target: serde_json::to_value(&record.target).unwrap_or_else(|_| json!({})),
        domain: record.domain.clone(),
        kind: anchor_kind_str(record.kind).to_owned(),
        verb_params: record.verb_params.clone(),
        sensitive: record.sensitive,
        sensitive_ack: record.sensitive_ack,
        status: anchor_status_str(record.status).to_owned(),
        verbs: Vec::<VerbBinding>::new(),
        last_used_at: record.last_used_at.map(ms_rfc3339),
        created_at: ms_rfc3339(record.created_at),
        updated_at: ms_rfc3339(record.updated_at),
    }
}

const fn anchor_kind_str(value: flick_core::AnchorKind) -> &'static str {
    match value {
        flick_core::AnchorKind::Point3d => "point3d",
        flick_core::AnchorKind::Direction => "direction",
        flick_core::AnchorKind::Region2d => "region2d",
    }
}

const fn anchor_status_str(value: flick_core::AnchorStatus) -> &'static str {
    match value {
        flick_core::AnchorStatus::Ok => "ok",
        flick_core::AnchorStatus::NeedsRealign => "needs_realign",
        flick_core::AnchorStatus::NeedsReteach => "needs_reteach",
    }
}

fn store_problem(err: flick_core::StoreError) -> ApiProblem {
    ApiProblem::validation("store_error", err.to_string())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn ms_rfc3339(value: i64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(value.div_euclid(1_000)) else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    datetime
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async {
                if let Some(signal) = &mut sigterm {
                    signal.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
