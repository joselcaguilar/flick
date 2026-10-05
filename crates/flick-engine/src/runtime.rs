//! Runtime wiring for the engine binary and local API.

use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
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
    ActionDto, ActionOutcomeDto, ActionTargetDto, ActivityItem, ActivityPage, ActivityQuery,
    Anchor as ApiAnchor, ApiConfig, ApiGateways, ApiProblem, ApiState, AvailableCamera,
    Camera as ApiCamera, CameraCreate, CameraFormat, CameraPatch, CameraStatus, ConfigGateway,
    EmptyEvent, EngineControl, EnginePausedEvent, EngineStatus, EventHub, FakeUpdates, HaArea,
    HaClientCertificate, HaConnectRequest, HaConnectionUpdate, HaDiscovery, HaEntity, HaGateway,
    HaInstance, HaServiceSchema, HaStatus as ApiHaStatus, HaStatusEvent, HandEvent, HandsEvent,
    LatencyBreakdown, Mapping, NetworkReport, PauseRequest, PreviewFrame, PreviewSource, RayEvent,
    RealignCommitResponse, RealignPointRequest, RealignPointResponse, RealignSession, SettingsMap,
    SetupSuggestRequest, SetupSuggestion, StageLatency, TargetAmbiguousEvent, TargetClearedEvent,
    TargetHoverEvent, TargetModeDto, TargetSelectedEvent, TeachCommitRequest, TeachCommitResponse,
    TeachGateway, TeachLevelRequest, TeachLevelResponse, TeachProgressEvent, TeachRequest,
    TeachSession as ApiTeachSession, TeachSpotResponse, VerbBinding, WsServerMessage, router,
};
use flick_capture::{
    CameraPermissionStatus, CaptureHandle, CaptureStatus, FileSource, FileSourceOptions, FrameTap,
    LatestFrameSlot, LocalCameraOptions, LocalCameraSource, camera_permission_status,
    spawn_capture_with_tap,
};
use flick_core::{
    Action, ActionOutcome, ActionStatus, ActionTarget, AnchorId, CameraId, DialProperty,
    FaceKeypoints, Frame, FrameSource, GestureEvent, GestureId, GesturePhase, HandFrame,
    HandObservation, HandPipeline, Handedness, MappingId, PlaceId, SelectionState, SourceInfo,
    SourceKind, StoreError, Verb,
};
use flick_gestures::{GestureEngine, GestureEngineConfig};
use flick_ha::{
    ClientIdentity, EntityState, HaClient, HaConnectionConfig, HaStatus, KeyringSecretStore,
    RegistrySnapshot, SafetyClass, SafetyValidator, SecretStore, ServiceCallRecord,
    record_current_fan_level,
};
use flick_spatial::{
    Anchor as SpatialAnchor, AnchorGeometry, CameraIntrinsics, DEFAULT_ESTIMATOR_VERSION,
    FingerAim, PlaceRecord, PlaceStatus, PointingRay, RayEstimator, RayEstimatorSettings, RayModel,
    RaySource, RealignPair, StoredIntrinsics, TargetClearReason, TargetEvent, TargetSelectorImpl,
    TargetSelectorSettings, TeachObservation, TeachSession as SpatialTeachSession, TeachTarget,
    is_point_pose, owner_face, realign, seed_observations,
};
use flick_store::Store;
use flick_vision::{EpChoice, FaceKeypointRunner, HandPipelineImpl, ModelSet};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, watch},
};

use crate::{
    config::RuntimeConfig,
    dispatcher::{
        DispatchReport, Dispatcher, DispatcherAnchor, DispatcherMapping, DispatcherSettings,
        HaActionSink, MappingHand, MappingTarget, NoopActionSink, classify_action_safety,
        owner_fan_anchor, owner_scenario_mappings,
    },
    fake_landmarks::{ReplayCatalog, ReplayFixture, replay_once},
    host::HostIdentity,
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
    let app =
        Arc::new_cyclic(|weak| EngineApp::new(runtime.clone(), Arc::clone(&store), weak.clone()));
    if runtime.mock_ha {
        start_mock_ha(&app).await?;
    }

    let gateways = ApiGateways {
        engine: app.clone(),
        ha: app.clone(),
        teach: app.clone(),
        updates: Arc::new(FakeUpdates),
        preview: app.clone(),
        config: app.clone(),
    };
    let state = ApiState::new(api_config.clone(), gateways);
    let settings_map = load_settings(&state, &app.store)?;
    let dispatcher_settings = dispatcher_settings_from_map(&settings_map);
    let mut api_mappings = load_api_mappings(&app.store)?;
    let mut dispatcher_mappings = dispatcher_mappings_from_api(&api_mappings)?;
    let mut api_anchors = load_api_anchors(&app.targeting)?;
    apply_area_overrides(&app.store, &mut api_anchors)?;
    let mut dispatcher_anchors =
        dispatcher_anchors_from_api(&api_anchors, &app.registry.lock().await.clone());
    if runtime.mock_ha || runtime.fake_landmarks.is_some() {
        let owner_anchor = owner_fan_anchor();
        dispatcher_anchors.push(owner_anchor.clone());
        api_anchors.push(api_anchor_from_dispatcher(&owner_anchor));
        let owner_mappings = owner_scenario_mappings();
        api_mappings.extend(owner_mappings.iter().map(api_mapping_from_dispatcher));
        dispatcher_mappings.extend(owner_mappings);
    }
    let onboarding_completed = settings_bool(&settings_map, "onboarding.completed");
    state.replace_settings(settings_map);
    state.replace_mappings(api_mappings);
    state.replace_anchors(api_anchors);
    let events = state.events();
    app.set_events(events.clone()).await;
    let sink = if let Some(client) = app.ha_client.lock().await.clone() {
        Arc::new(HaActionSink::new(client)) as Arc<dyn flick_core::ActionSink>
    } else {
        Arc::new(NoopActionSink) as Arc<dyn flick_core::ActionSink>
    };
    let dispatcher = Dispatcher::builder(sink)
        .store(store)
        .events(events)
        .settings(dispatcher_settings)
        .registry(app.registry.lock().await.clone())
        .mappings(dispatcher_mappings)
        .anchors(dispatcher_anchors)
        .build();
    app.set_dispatcher(dispatcher).await;
    if !runtime.mock_ha {
        app.spawn_ha_restore();
    }
    app.configure_fake_landmarks().await;
    app.spawn_pause_timer();
    app.spawn_camera_autostart(onboarding_completed).await;

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
        let camera_id = load_cameras(&app.store)?
            .into_iter()
            .next()
            .map_or_else(|| CameraId::new().to_string(), |camera| camera.id);
        app.start_file_camera(&camera_id, path).await?;
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
        .with_graceful_shutdown(shutdown_signal(runtime.sidecar))
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
            ..HaInstance::default()
        }),
        snapshot,
        None,
    )
    .await;
    app.set_mock_ha_handle(handle).await;
    Ok(client)
}

fn load_settings(state: &ApiState, store: &Store) -> anyhow::Result<SettingsMap> {
    let mut settings = state.settings_snapshot();
    for (key, value) in store
        .settings()
        .all()
        .map_err(|err| anyhow::anyhow!("{err}"))?
    {
        settings.insert(key, value);
    }
    Ok(settings)
}

fn settings_bool(settings: &SettingsMap, key: &str) -> bool {
    settings
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn load_cameras(store: &Store) -> flick_store::Result<Vec<ApiCamera>> {
    let conn = store.connection();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, kind, device_ref, url_redacted, enabled, mirror, rotation, \
             active_fps, idle_fps, max_hands, roi, created_at, updated_at, area_override \
             FROM cameras ORDER BY created_at",
        )
        .map_err(store_database_error)?;
    let rows = stmt
        .query_map([], camera_from_row)
        .map_err(store_database_error)?;
    rows.map(|row| row.map_err(store_database_error)).collect()
}

fn load_camera(store: &Store, id: &str) -> flick_store::Result<Option<ApiCamera>> {
    let conn = store.connection();
    conn.query_row(
        "SELECT id, name, kind, device_ref, url_redacted, enabled, mirror, rotation, \
         active_fps, idle_fps, max_hands, roi, created_at, updated_at, area_override \
         FROM cameras WHERE id = ?1",
        params![id],
        camera_from_row,
    )
    .optional()
    .map_err(store_database_error)
}

fn upsert_camera(store: &Store, camera: &ApiCamera) -> flick_store::Result<()> {
    let conn = store.connection();
    let roi = camera
        .roi
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|err| StoreError::InvalidJson(err.to_string()))?;
    conn.execute(
        "INSERT INTO cameras \
         (id, name, kind, device_ref, url_redacted, enabled, mirror, rotation, active_fps, idle_fps, max_hands, roi, created_at, updated_at, area_override) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, kind = excluded.kind, device_ref = excluded.device_ref, \
         url_redacted = excluded.url_redacted, enabled = excluded.enabled, mirror = excluded.mirror, rotation = excluded.rotation, \
         active_fps = excluded.active_fps, idle_fps = excluded.idle_fps, max_hands = excluded.max_hands, roi = excluded.roi, \
         updated_at = excluded.updated_at, area_override = excluded.area_override",
        params![
            camera.id,
            camera.name,
            camera.kind,
            camera.device_ref,
            camera.url_redacted,
            if camera.enabled { 1 } else { 0 },
            if camera.mirror { 1 } else { 0 },
            camera.rotation,
            camera.active_fps,
            camera.idle_fps,
            camera.max_hands,
            roi,
            ms_from_rfc3339(&camera.created_at),
            ms_from_rfc3339(&camera.updated_at),
            camera.area_override,
        ],
    )
    .map_err(store_database_error)?;
    Ok(())
}

fn delete_camera_row(store: &Store, id: &str) -> flick_store::Result<()> {
    let conn = store.connection();
    conn.execute("DELETE FROM cameras WHERE id = ?1", params![id])
        .map_err(store_database_error)?;
    Ok(())
}

fn camera_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiCamera> {
    let roi_raw: Option<String> = row.get(11)?;
    let created_at: i64 = row.get(12)?;
    let updated_at: i64 = row.get(13)?;
    Ok(ApiCamera {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        device_ref: row.get(3)?,
        url_redacted: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        mirror: row.get::<_, i64>(6)? != 0,
        rotation: row.get(7)?,
        active_fps: row.get(8)?,
        idle_fps: row.get(9)?,
        max_hands: row.get(10)?,
        roi: roi_raw.and_then(|raw| serde_json::from_str(&raw).ok()),
        area_override: row.get(14)?,
        ha_area_id: None,
        ha_device_name: None,
        created_at: rfc3339_from_ms(created_at),
        updated_at: rfc3339_from_ms(updated_at),
    })
}

/// Local cameras sit with this computer, so they default to its Home Assistant device's area.
fn with_ha_area(mut camera: ApiCamera, registry: &RegistrySnapshot) -> ApiCamera {
    if camera.kind == "local"
        && let Some(device) = HostIdentity::current().ha_device(&registry.devices)
    {
        camera.ha_area_id = device.area_id.clone();
        camera.ha_device_name = device.display_name().map(ToOwned::to_owned);
    }
    camera
}

fn store_database_error(err: rusqlite::Error) -> StoreError {
    StoreError::Database(err.to_string())
}

fn ms_from_rfc3339(value: &str) -> i64 {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|time| i64::try_from(time.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX))
        .unwrap_or_else(|_| now_ms())
}

fn dispatcher_settings_from_map(settings: &SettingsMap) -> DispatcherSettings {
    let mut dispatcher = DispatcherSettings::default();
    if let Some(value) = settings
        .get("safety.allow_sensitive")
        .and_then(serde_json::Value::as_bool)
    {
        dispatcher.allow_sensitive_actions = value;
    }
    if let Some(value) = settings
        .get("debug.log_suppressed")
        .and_then(serde_json::Value::as_bool)
    {
        dispatcher.log_suppressed = value;
    }
    dispatcher
}

fn load_api_mappings(store: &Store) -> anyhow::Result<Vec<Mapping>> {
    let conn = store.connection();
    let mut stmt = conn.prepare(
        "SELECT id, name, enabled, gesture_id, hand, camera_ids, target_mode, anchor_id, \
         target_domain, mode, action, sensitive, sensitive_ack, confirm_gesture_id, feedback, \
         sort_order, created_at, updated_at FROM mappings ORDER BY sort_order, created_at",
    )?;
    let rows = stmt.query_map([], |row| {
        let camera_ids_raw: String = row.get(5)?;
        let action_raw: String = row.get(10)?;
        let feedback_raw: String = row.get(14)?;
        let created_at: i64 = row.get(16)?;
        let updated_at: i64 = row.get(17)?;
        Ok(Mapping {
            id: row.get(0)?,
            name: row.get(1)?,
            enabled: row.get::<_, i64>(2)? != 0,
            gesture_id: row.get(3)?,
            hand: row.get(4)?,
            camera_ids: serde_json::from_str(&camera_ids_raw).unwrap_or_default(),
            target_mode: target_mode_from_str(&row.get::<_, String>(6)?),
            anchor_id: row.get(7)?,
            target_domain: row.get(8)?,
            mode: row.get(9)?,
            action: serde_json::from_str(&action_raw).unwrap_or_else(|_| ActionDto::CallService {
                domain: "homeassistant".to_owned(),
                service: "toggle".to_owned(),
                target: ActionTargetDto::default(),
                data: json!({}),
                preset: None,
            }),
            sensitive: row.get::<_, i64>(11)? != 0,
            sensitive_ack: row.get::<_, i64>(12)? != 0,
            confirm_gesture_id: row.get(13)?,
            feedback: serde_json::from_str(&feedback_raw).unwrap_or_else(|_| json!({})),
            sort_order: row.get(15)?,
            created_at: rfc3339_from_ms(created_at),
            updated_at: rfc3339_from_ms(updated_at),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn load_api_anchors(targeting: &SqliteTargetingStore) -> anyhow::Result<Vec<ApiAnchor>> {
    let mut cameras: HashMap<PlaceId, Option<String>> = HashMap::new();
    Ok(targeting
        .try_all_anchors()
        .map_err(|err| anyhow::anyhow!("{err}"))?
        .iter()
        .map(|record| {
            let camera_id = cameras
                .entry(record.place_id)
                .or_insert_with(|| {
                    targeting
                        .try_place(record.place_id)
                        .ok()
                        .flatten()
                        .map(|place| place.camera_id.to_string())
                })
                .clone();
            api_anchor_from_record(record, camera_id)
        })
        .collect())
}

/// Flick-only areas live beside the targeting columns; they never reach Home Assistant.
fn apply_area_overrides(store: &Store, anchors: &mut [ApiAnchor]) -> anyhow::Result<()> {
    let conn = store.connection();
    let mut stmt =
        conn.prepare("SELECT id, area_override FROM anchors WHERE area_override IS NOT NULL")?;
    let overrides = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    for anchor in anchors.iter_mut() {
        anchor.area_override = overrides.get(&anchor.id).cloned();
    }
    Ok(())
}

fn dispatcher_mappings_from_api(mappings: &[Mapping]) -> anyhow::Result<Vec<DispatcherMapping>> {
    mappings.iter().map(dispatcher_mapping_from_api).collect()
}

fn dispatcher_mapping_from_api(mapping: &Mapping) -> anyhow::Result<DispatcherMapping> {
    let action = action_from_dto(mapping.action.clone())
        .map_err(|_| anyhow::anyhow!("bad action for mapping {}", mapping.id))?;
    let target = match mapping.target_mode {
        TargetModeDto::Global => MappingTarget::Global,
        TargetModeDto::Anchor => MappingTarget::Anchor(
            mapping
                .anchor_id
                .as_deref()
                .context("anchor mapping missing anchor_id")?
                .parse()?,
        ),
        TargetModeDto::Domain => MappingTarget::Domain(
            mapping
                .target_domain
                .clone()
                .context("domain mapping missing target_domain")?,
        ),
    };
    Ok(DispatcherMapping {
        id: MappingId::from_str(&mapping.id)?,
        name: mapping.name.clone(),
        enabled: mapping.enabled,
        gesture_id: GestureId::from_str(&mapping.gesture_id)?,
        hand: mapping_hand_from_str(&mapping.hand),
        camera_ids: mapping.camera_ids.clone(),
        target,
        action,
        cooldown_ms: 1_000,
        sensitive_ack: mapping.sensitive_ack,
        confirm_gesture_id: mapping
            .confirm_gesture_id
            .as_deref()
            .map(GestureId::from_str)
            .transpose()?,
        sort_order: mapping.sort_order,
    })
}

fn dispatcher_anchors_from_api(
    anchors: &[ApiAnchor],
    registry: &RegistrySnapshot,
) -> Vec<DispatcherAnchor> {
    anchors
        .iter()
        .filter_map(|anchor| {
            let id = AnchorId::from_str(&anchor.id).ok()?;
            let entity_id = target_entity_id(&anchor.target)?;
            let entity = registry
                .entities
                .iter()
                .find(|entity| entity.entity_id == entity_id)
                .map(entity_state_from_registry)
                .unwrap_or_else(|| EntityState {
                    entity_id: entity_id.to_owned(),
                    state: "unknown".to_owned(),
                    attributes: serde_json::Map::new(),
                    last_changed: None,
                    last_updated: None,
                });
            let levels = anchor
                .verb_params
                .get("levels")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_f64)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(DispatcherAnchor {
                id,
                name: anchor.name.clone(),
                domain: anchor.domain.clone(),
                entity,
                levels,
            })
        })
        .collect()
}

fn entity_state_from_registry(entity: &flick_ha::RegistryEntity) -> EntityState {
    let mut attributes = serde_json::Map::from_iter([
        ("friendly_name".to_owned(), json!(entity.name)),
        (
            "supported_features".to_owned(),
            json!(entity.supported_features),
        ),
    ]);
    if let Some(device_class) = &entity.device_class {
        attributes.insert("device_class".to_owned(), json!(device_class));
    }
    EntityState {
        entity_id: entity.entity_id.clone(),
        state: entity.state.clone(),
        attributes,
        last_changed: None,
        last_updated: None,
    }
}

fn api_mapping_from_dispatcher(mapping: &DispatcherMapping) -> Mapping {
    let (target_mode, anchor_id, target_domain) = match &mapping.target {
        MappingTarget::Global => (TargetModeDto::Global, None, None),
        MappingTarget::Anchor(id) => (TargetModeDto::Anchor, Some(id.to_string()), None),
        MappingTarget::Domain(domain) => (TargetModeDto::Domain, None, Some(domain.clone())),
    };
    let now = now_rfc3339();
    Mapping {
        id: mapping.id.to_string(),
        name: mapping.name.clone(),
        enabled: mapping.enabled,
        gesture_id: mapping.gesture_id.to_string(),
        hand: mapping_hand_str(mapping.hand).to_owned(),
        camera_ids: mapping.camera_ids.clone(),
        target_mode,
        anchor_id,
        target_domain,
        mode: "tap".to_owned(),
        action: action_to_dto(&mapping.action),
        sensitive: false,
        sensitive_ack: mapping.sensitive_ack,
        confirm_gesture_id: mapping.confirm_gesture_id.map(|id| id.to_string()),
        feedback: json!({"hud": true, "sound": true}),
        sort_order: mapping.sort_order,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn api_anchor_from_dispatcher(anchor: &DispatcherAnchor) -> ApiAnchor {
    let now = now_rfc3339();
    ApiAnchor {
        id: anchor.id.to_string(),
        place_id: "dev-owner-fixture".to_owned(),
        name: anchor.name.clone(),
        target: json!({"entity_id": anchor.entity.entity_id}),
        domain: anchor.domain.clone(),
        kind: "direction".to_owned(),
        verb_params: json!({ "levels": anchor.levels }),
        sensitive: false,
        sensitive_ack: false,
        status: "ok".to_owned(),
        verbs: Vec::new(),
        last_used_at: None,
        created_at: now.clone(),
        updated_at: now,
        camera_id: None,
        area_override: None,
    }
}

fn target_mode_from_str(value: &str) -> TargetModeDto {
    match value {
        "anchor" => TargetModeDto::Anchor,
        "domain" => TargetModeDto::Domain,
        _ => TargetModeDto::Global,
    }
}

fn target_mode_str(value: TargetModeDto) -> &'static str {
    match value {
        TargetModeDto::Global => "global",
        TargetModeDto::Anchor => "anchor",
        TargetModeDto::Domain => "domain",
    }
}

fn mapping_hand_from_str(value: &str) -> MappingHand {
    match value {
        "left" => MappingHand::Left,
        "right" => MappingHand::Right,
        _ => MappingHand::Any,
    }
}

fn mapping_hand_str(value: MappingHand) -> &'static str {
    match value {
        MappingHand::Any => "any",
        MappingHand::Left => "left",
        MappingHand::Right => "right",
    }
}

fn action_to_dto(action: &Action) -> ActionDto {
    match action {
        Action::CallService {
            domain,
            service,
            target,
            data,
            preset,
        } => ActionDto::CallService {
            domain: domain.clone(),
            service: service.clone(),
            target: ActionTargetDto {
                entity_id: target.entity_id.clone(),
                device_id: target.device_id.clone(),
                area_id: target.area_id.clone(),
            },
            data: data.clone(),
            preset: preset.clone(),
        },
        Action::Dial {
            entity_id,
            property,
            gain,
            min,
            max,
        } => ActionDto::Dial {
            entity_id: entity_id.clone(),
            property: dial_property_str(*property).to_owned(),
            gain: *gain,
            min: min.clone(),
            max: max.clone(),
        },
        Action::Verb { verb, level } => ActionDto::Verb {
            verb: verb_str(*verb).to_owned(),
            level: *level,
        },
    }
}

fn dial_property_str(property: DialProperty) -> &'static str {
    match property {
        DialProperty::BrightnessPct => "brightness_pct",
        DialProperty::VolumeLevel => "volume_level",
        DialProperty::Position => "position",
        DialProperty::Percentage => "percentage",
        DialProperty::Temperature => "temperature",
    }
}

fn verb_str(verb: Verb) -> &'static str {
    match verb {
        Verb::Up => "up",
        Verb::Down => "down",
        Verb::On => "on",
        Verb::Off => "off",
        Verb::Stop => "stop",
        Verb::Toggle => "toggle",
        Verb::LevelSet => "level_set",
    }
}

fn rfc3339_from_ms(ms: i64) -> String {
    OffsetDateTime::from_unix_timestamp(ms.div_euclid(1_000))
        .and_then(|time| time.replace_nanosecond((ms.rem_euclid(1_000) as u32) * 1_000_000))
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
        .unwrap_or_else(now_rfc3339)
}

async fn wait_ha_ready(client: &HaClient) -> anyhow::Result<String> {
    let mut rx = client.status();
    let wait = async {
        loop {
            let status = rx.borrow_and_update().clone();
            match status {
                HaStatus::Ready { ha_version } => return Ok(ha_version.unwrap_or_default()),
                HaStatus::AuthFailed { message } => {
                    anyhow::bail!("Home Assistant rejected the access token: {message}");
                }
                HaStatus::Reconnecting {
                    last_error: Some(reason),
                    ..
                } => anyhow::bail!("Couldn't reach Home Assistant: {reason}"),
                HaStatus::Disconnected | HaStatus::Connecting | HaStatus::Reconnecting { .. } => {}
            }
            if rx.changed().await.is_err() {
                anyhow::bail!("Home Assistant connection closed unexpectedly");
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), wait)
        .await
        .context("Couldn't reach Home Assistant: the server didn't respond in time")?
}

struct SavedHaInstance {
    instance: HaInstance,
    keychain_ref: String,
    cert_sha256: Option<String>,
}

fn load_default_ha_instance(store: &Store) -> anyhow::Result<Option<SavedHaInstance>> {
    let conn = store.connection();
    conn.query_row(
        "SELECT id, name, base_url, ha_uuid, auth_kind, keychain_ref, ha_version, is_default, \
         created_at, updated_at, cert_sha256, internal_url, trusted_ssids FROM ha_instances \
         ORDER BY is_default DESC, updated_at DESC LIMIT 1",
        [],
        |row| {
            let created_at: i64 = row.get(8)?;
            let updated_at: i64 = row.get(9)?;
            let trusted_ssids: Option<String> = row.get(12)?;
            Ok(SavedHaInstance {
                instance: HaInstance {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    ha_uuid: row.get(3)?,
                    auth_kind: row.get(4)?,
                    ha_version: row.get(6)?,
                    is_default: row.get::<_, i64>(7)? != 0,
                    created_at: rfc3339_from_ms(created_at),
                    updated_at: rfc3339_from_ms(updated_at),
                    internal_url: row.get(11)?,
                    trusted_ssids: trusted_ssids
                        .and_then(|raw| serde_json::from_str(&raw).ok())
                        .unwrap_or_default(),
                },
                keychain_ref: row.get(5)?,
                cert_sha256: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn persist_ha_instance(
    store: &Store,
    instance: &HaInstance,
    keychain_ref: &str,
    cert_sha256: Option<&str>,
) -> anyhow::Result<()> {
    let now = now_ms();
    let trusted_ssids = serde_json::to_string(&instance.trusted_ssids)?;
    let conn = store.connection();
    conn.execute("UPDATE ha_instances SET is_default = 0", [])?;
    conn.execute(
        "INSERT INTO ha_instances \
         (id, name, base_url, ha_uuid, auth_kind, keychain_ref, cert_sha256, ha_version, is_default, \
         created_at, updated_at, internal_url, trusted_ssids) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11, ?12) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, base_url = excluded.base_url, \
         ha_uuid = excluded.ha_uuid, auth_kind = excluded.auth_kind, keychain_ref = excluded.keychain_ref, \
         cert_sha256 = excluded.cert_sha256, ha_version = excluded.ha_version, is_default = 1, \
         updated_at = excluded.updated_at, internal_url = excluded.internal_url, \
         trusted_ssids = excluded.trusted_ssids",
        params![
            instance.id,
            instance.name,
            instance.base_url,
            instance.ha_uuid,
            instance.auth_kind,
            keychain_ref,
            cert_sha256,
            instance.ha_version,
            now,
            now,
            instance.internal_url,
            trusted_ssids,
        ],
    )?;
    Ok(())
}

/// Applies a Connection settings change. A blank Home URL clears it.
fn apply_ha_update(
    instance: &mut HaInstance,
    update: HaConnectionUpdate,
) -> Result<(), ApiProblem> {
    if let Some(base_url) = update.base_url {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err(ApiProblem::validation(
                "ha_invalid_url",
                "The Remote URL can't be empty",
            ));
        }
        instance.base_url = base_url.trim_end_matches('/').to_owned();
    }
    if let Some(internal_url) = update.internal_url {
        let internal_url = internal_url.trim();
        instance.internal_url =
            (!internal_url.is_empty()).then(|| internal_url.trim_end_matches('/').to_owned());
    }
    if let Some(ssids) = update.trusted_ssids {
        instance.trusted_ssids = normalize_ssids(ssids);
    }
    Ok(())
}

/// Trims, drops blanks, and de-duplicates SSIDs while keeping their order.
fn normalize_ssids(ssids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ssid in ssids {
        let ssid = ssid.trim();
        if !ssid.is_empty() && !out.iter().any(|seen| seen == ssid) {
            out.push(ssid.to_owned());
        }
    }
    out
}

fn persist_settings(store: &Store, settings: &SettingsMap) -> anyhow::Result<()> {
    let now = now_ms();
    for (key, value) in settings {
        store.settings().set(key, value, now)?;
    }
    Ok(())
}

fn persist_mappings(store: &Store, mappings: &[Mapping]) -> anyhow::Result<()> {
    let mut conn = store.connection();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM mappings", [])?;
    let now = now_ms();
    for mapping in mappings {
        if let (TargetModeDto::Anchor, Some(anchor_id)) =
            (mapping.target_mode, mapping.anchor_id.as_ref())
        {
            let exists = tx
                .query_row(
                    "SELECT 1 FROM anchors WHERE id = ?1",
                    params![anchor_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                continue;
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO gestures \
             (id, source, kind, hands_required, name, hand_constraint, enabled, pro, created_at, updated_at) \
             VALUES (?1, 'builtin', 'static', 1, ?2, 'any', 1, 0, ?3, ?4)",
            params![mapping.gesture_id, mapping.gesture_id, now, now],
        )?;
        tx.execute(
            "INSERT INTO mappings \
             (id, name, enabled, gesture_id, hand, allow_two_hands, camera_ids, target_mode, anchor_id, target_domain, \
              mode, hold_ms, repeat_ms, cooldown_ms, require_armed, active_hours, action, sensitive, sensitive_ack, \
              confirm_gesture_id, feedback, sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, 800, 400, 1000, 0, NULL, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                mapping.id,
                mapping.name,
                if mapping.enabled { 1 } else { 0 },
                mapping.gesture_id,
                mapping.hand,
                serde_json::to_string(&mapping.camera_ids)?,
                target_mode_str(mapping.target_mode),
                mapping.anchor_id,
                mapping.target_domain,
                mapping.mode,
                serde_json::to_string(&mapping.action)?,
                if mapping.sensitive { 1 } else { 0 },
                if mapping.sensitive_ack { 1 } else { 0 },
                mapping.confirm_gesture_id,
                serde_json::to_string(&mapping.feedback)?,
                mapping.sort_order,
                now,
                now,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn persist_anchor_edits(store: &Store, anchors: &[ApiAnchor]) -> anyhow::Result<()> {
    let current_ids = anchors
        .iter()
        .map(|anchor| anchor.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let now = now_ms();
    let conn = store.connection();
    let mut stmt = conn.prepare("SELECT id FROM anchors")?;
    let existing = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in existing {
        if !current_ids.contains(&id) {
            conn.execute("DELETE FROM anchors WHERE id = ?1", params![id])?;
        }
    }
    for anchor in anchors {
        conn.execute(
            "UPDATE anchors SET name = ?1, verb_params = ?2, sensitive = ?3, sensitive_ack = ?4, status = ?5, updated_at = ?6, area_override = ?8 WHERE id = ?7",
            params![
                anchor.name,
                serde_json::to_string(&anchor.verb_params)?,
                if anchor.sensitive { 1 } else { 0 },
                if anchor.sensitive_ack { 1 } else { 0 },
                anchor.status,
                now,
                anchor.id,
                anchor.area_override,
            ],
        )?;
    }
    Ok(())
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
            EntityState {
                entity_id: "cover.garage".to_owned(),
                state: "closed".to_owned(),
                attributes: serde_json::Map::from_iter([
                    ("friendly_name".to_owned(), json!("Garage door")),
                    ("device_class".to_owned(), json!("garage")),
                    ("supported_features".to_owned(), json!(15)),
                ]),
                last_changed: None,
                last_updated: None,
            },
            owner_fan_anchor().entity,
        ],
        services: json!({
            "light": {"toggle": {}, "turn_on": {}, "turn_off": {}},
            "fan": {"turn_on": {}, "turn_off": {}, "toggle": {}, "increase_speed": {}, "decrease_speed": {}},
            "cover": {"open_cover": {}, "close_cover": {}, "stop_cover": {}}
        }),
        registries: flick_ha::mock::MockRegistries::default(),
        call_delay_ms: 0,
        call_errors: Vec::new(),
    }
}

fn append_owner_anchor_for_dev(runtime: &RuntimeConfig, anchors: &mut Vec<DispatcherAnchor>) {
    if !(runtime.mock_ha || runtime.fake_landmarks.is_some()) {
        return;
    }
    let owner_anchor = owner_fan_anchor();
    if !anchors.iter().any(|anchor| anchor.id == owner_anchor.id) {
        anchors.push(owner_anchor);
    }
}

/// Pausing releases the camera; resuming reopens the camera that was running.
#[derive(Debug, Default)]
struct PauseState {
    paused: bool,
    until: Option<(tokio::time::Instant, String)>,
    camera_id: Option<String>,
}

/// Keychain account holding the mTLS client certificate and key as PEM.
const HA_CLIENT_CERT_ACCOUNT: &str = "ha:client-certificate";

/// Reads the mTLS client identity from the keychain. An unreadable PEM counts
/// as no certificate; a keychain error is returned so a later attempt retries.
fn read_client_identity() -> Result<Option<Arc<ClientIdentity>>, flick_ha::HaError> {
    let Some(pem) = KeyringSecretStore::new().get(HA_CLIENT_CERT_ACCOUNT)? else {
        return Ok(None);
    };
    match ClientIdentity::from_pem(pem.as_bytes()) {
        Ok(identity) => Ok(Some(Arc::new(identity))),
        Err(err) => {
            tracing::warn!(error = %err, "stored client certificate is unreadable");
            Ok(None)
        }
    }
}

/// mTLS client identity, read from the keychain on first use.
enum ClientIdentitySlot {
    Unloaded,
    Loaded(Option<Arc<ClientIdentity>>),
}

struct EngineApp {
    runtime: RuntimeConfig,
    store: Arc<Store>,
    started_at: Instant,
    pause: Mutex<PauseState>,
    pause_changed: Notify,
    events: Mutex<Option<EventHub>>,
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
    /// Frames of the latest dev replay; teach spots sample them when no camera runs.
    replay_frames: Arc<std::sync::Mutex<Vec<HandFrame>>>,
    /// Wi-Fi SSID reported by the desktop shell; drives Home/Remote routing.
    network_tx: watch::Sender<Option<String>>,
    /// Bumped whenever the HA client is replaced, so stale sync tasks stop.
    ha_generation: std::sync::atomic::AtomicU64,
    /// Bumped when a restore starts, a connection is installed, or HA is
    /// disconnected. A background restore installs its client only while its
    /// epoch is the latest, so it can't undo what the user did while it
    /// waited on the keychain.
    ha_connect_epoch: std::sync::atomic::AtomicU64,
    /// Epoch of the startup restore while it runs (0 = none), so status reads
    /// "connecting" until it finishes or something newer supersedes it.
    ha_restore_epoch: std::sync::atomic::AtomicU64,
    /// mTLS client certificate presented when a server asks for one.
    client_identity: std::sync::Mutex<ClientIdentitySlot>,
    /// Bumped by every camera start or stop. A start still waiting on the
    /// macOS camera prompt runs only while its epoch is the latest, so it
    /// can't override a newer start or stop.
    camera_intent_epoch: std::sync::atomic::AtomicU64,
    weak_self: std::sync::Weak<EngineApp>,
}

impl EngineApp {
    fn new(runtime: RuntimeConfig, store: Arc<Store>, weak_self: std::sync::Weak<Self>) -> Self {
        Self {
            runtime,
            store: Arc::clone(&store),
            started_at: Instant::now(),
            pause: Mutex::new(PauseState::default()),
            pause_changed: Notify::new(),
            events: Mutex::new(None),
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
            replay_frames: Arc::default(),
            network_tx: watch::channel(None).0,
            ha_generation: std::sync::atomic::AtomicU64::new(0),
            ha_connect_epoch: std::sync::atomic::AtomicU64::new(0),
            ha_restore_epoch: std::sync::atomic::AtomicU64::new(0),
            client_identity: std::sync::Mutex::new(ClientIdentitySlot::Unloaded),
            camera_intent_epoch: std::sync::atomic::AtomicU64::new(0),
            weak_self,
        }
    }

    async fn set_dispatcher(&self, dispatcher: Dispatcher) {
        if let Some(client) = self.ha_client.lock().await.clone() {
            dispatcher
                .set_sink(Arc::new(HaActionSink::new(client)) as Arc<dyn flick_core::ActionSink>)
                .await;
        }
        dispatcher
            .set_registry(self.registry.lock().await.clone())
            .await;
        dispatcher.set_paused(self.pause.lock().await.paused).await;
        *self.dispatcher.lock().await = Some(dispatcher);
    }

    async fn set_events(&self, events: EventHub) {
        *self.events.lock().await = Some(events);
    }

    fn begin_ha_connect(&self) -> u64 {
        self.ha_connect_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    fn is_latest_ha_connect(&self, epoch: u64) -> bool {
        self.ha_connect_epoch
            .load(std::sync::atomic::Ordering::SeqCst)
            == epoch
    }

    /// Installs the client. With `restore_epoch` it is a background restore,
    /// dropped if a newer connect or a disconnect happened meanwhile; without
    /// one it is the newest connection and supersedes restores in flight.
    async fn set_ha_client(
        &self,
        client: HaClient,
        instance: Option<HaInstance>,
        snapshot: RegistrySnapshot,
        restore_epoch: Option<u64>,
    ) {
        // Held until installed, so `delete` can't interleave.
        let mut current = self.ha_client.lock().await;
        match restore_epoch {
            Some(epoch) if !self.is_latest_ha_connect(epoch) => {
                tracing::info!("dropping a superseded Home Assistant restore");
                return;
            }
            Some(_) => {}
            None => {
                self.begin_ha_connect();
            }
        }
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher
                .set_sink(
                    Arc::new(HaActionSink::new(client.clone())) as Arc<dyn flick_core::ActionSink>
                )
                .await;
            dispatcher.set_registry(snapshot.clone()).await;
        }
        let generation = self
            .ha_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let status = client.status();
        *current = Some(client);
        *self.ha_instance.lock().await = instance;
        *self.registry.lock().await = snapshot;
        self.spawn_ha_registry_sync(generation, status);
    }

    /// Refreshes the entity registry each time the client (re)connects after
    /// being down, e.g. after starting offline or switching Home/Remote URLs.
    /// Also publishes `ha.status` on every connection change.
    /// Holds only the status receiver so the client actor can still shut down.
    fn spawn_ha_registry_sync(&self, generation: u64, mut status: watch::Receiver<HaStatus>) {
        let weak = self.weak_self.clone();
        tokio::spawn(async move {
            let mut was_ready = matches!(*status.borrow_and_update(), HaStatus::Ready { .. });
            {
                let Some(app) = weak.upgrade() else { return };
                if app.ha_generation.load(std::sync::atomic::Ordering::SeqCst) != generation {
                    return;
                }
                app.publish_ha_status().await;
            }
            while status.changed().await.is_ok() {
                let ready = matches!(*status.borrow_and_update(), HaStatus::Ready { .. });
                let reconnected = ready && !was_ready;
                was_ready = ready;
                let Some(app) = weak.upgrade() else { return };
                if app.ha_generation.load(std::sync::atomic::Ordering::SeqCst) != generation {
                    return;
                }
                app.publish_ha_status().await;
                if !reconnected {
                    continue;
                }
                let Some(client) = app.ha_client.lock().await.clone() else {
                    return;
                };
                match client.refresh_registry().await {
                    Ok(snapshot) => {
                        if app.ha_generation.load(std::sync::atomic::Ordering::SeqCst) != generation
                        {
                            return;
                        }
                        if let Some(dispatcher) = app.dispatcher.lock().await.clone() {
                            dispatcher.set_registry(snapshot.clone()).await;
                        }
                        *app.registry.lock().await = snapshot;
                        if let Err(err) = app.reload_dispatcher_anchors_from_store().await {
                            tracing::warn!(error = ?err, "failed to reload anchors after HA reconnect");
                        }
                        // Lets views that read the registry, like camera areas, refetch it.
                        app.publish_ha_status().await;
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to refresh HA registry after reconnect")
                    }
                }
            }
        });
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
        let mut task = self.replay_task.lock().await;
        if let Some(previous) = task.take() {
            previous.abort();
        }
        if let Ok(mut frames) = self.replay_frames.lock() {
            frames.clear();
        }
        let recorded = Arc::clone(&self.replay_frames);
        *task = Some(tokio::spawn(async move {
            match replay_once(fixture_name.clone(), path, dispatcher, Some(recorded)).await {
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
        }));
        Ok(())
    }

    /// Dev stand-in for a live camera: rays from the latest replay once it has finished.
    async fn replayed_pointing_rays(&self) -> Vec<PointingRay> {
        let deadline = Instant::now() + REPLAY_SPOT_WAIT;
        while Instant::now() < deadline {
            let replaying = self
                .replay_task
                .lock()
                .await
                .as_ref()
                .is_some_and(|task| !task.is_finished());
            if !replaying {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let frames = self
            .replay_frames
            .lock()
            .map(|mut frames| std::mem::take(&mut *frames))
            .unwrap_or_default();
        // Fixtures are recorded at 720p.
        let mut estimator = RayEstimator::new(
            CameraIntrinsics::sane_default(1280, 720),
            TargetSelectorSettings::default().ray,
        );
        frames
            .iter()
            .filter_map(|frame| {
                let hand = pointing_hand(&frame.hands)?;
                estimator.estimate(hand, None, frame.captured_at).ok()
            })
            .collect()
    }

    async fn mock_ha_calls(&self) -> Vec<ServiceCallRecord> {
        let handle = self.mock_ha_handle.lock().await.clone();
        if let Some(handle) = handle {
            handle.calls().await
        } else {
            Vec::new()
        }
    }

    /// Restores the saved Home Assistant connection in the background, so a
    /// slow network or a keychain prompt can't hold up the desktop's ready
    /// handshake, which restarts the engine after 10 s.
    fn spawn_ha_restore(self: &Arc<Self>) {
        let epoch = self.begin_ha_connect();
        self.ha_restore_epoch
            .store(epoch, std::sync::atomic::Ordering::SeqCst);
        let app = Arc::clone(self);
        tokio::spawn(async move {
            app.publish_ha_status().await;
            app.restore_ha_connection(epoch).await;
            app.ha_restore_epoch
                .store(0, std::sync::atomic::Ordering::SeqCst);
            // A failed restore leaves no client, so this reports it as disconnected.
            app.publish_ha_status().await;
        });
    }

    /// Reconnects the saved instance; `epoch` is from `begin_ha_connect`.
    async fn restore_ha_connection(&self, epoch: u64) {
        let saved = match load_default_ha_instance(&self.store) {
            Ok(Some(saved)) => saved,
            Ok(None) => return,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load Home Assistant instance");
                return;
            }
        };
        {
            let mut instance = self.ha_instance.lock().await;
            if instance.is_none() && self.is_latest_ha_connect(epoch) {
                *instance = Some(saved.instance.clone());
            }
        }
        // Keychain reads block on an access prompt, e.g. after the app is re-signed.
        let keychain_ref = saved.keychain_ref.clone();
        let token =
            tokio::task::spawn_blocking(move || KeyringSecretStore::new().get(&keychain_ref)).await;
        let token = match token {
            Ok(Ok(Some(token))) => token,
            Ok(Ok(None)) => {
                tracing::warn!("stored Home Assistant token is missing from keychain");
                return;
            }
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "failed to read Home Assistant token");
                return;
            }
            Err(err) => {
                tracing::warn!(error = %err, "Home Assistant token read task failed");
                return;
            }
        };
        if !self.is_latest_ha_connect(epoch) {
            return;
        }
        self.preload_client_identity().await;
        if let Err(err) = self
            .connect_saved_ha(saved.instance, token, saved.cert_sha256, Some(epoch))
            .await
        {
            tracing::warn!(error = %err, "failed to restore Home Assistant connection");
            return;
        }
        // Anchors built before the registry arrived carry placeholder entity state.
        if let Err(err) = self.reload_dispatcher_anchors_from_store().await {
            tracing::warn!(error = ?err, "failed to reload anchors after restoring Home Assistant");
        }
    }

    /// The mTLS client identity, loading it from the keychain on first use.
    fn client_identity(&self) -> Option<Arc<ClientIdentity>> {
        let mut slot = self
            .client_identity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let ClientIdentitySlot::Loaded(identity) = &*slot {
            return identity.clone();
        }
        if self.runtime.mock_ha {
            *slot = ClientIdentitySlot::Loaded(None);
            return None;
        }
        match read_client_identity() {
            Ok(identity) => {
                *slot = ClientIdentitySlot::Loaded(identity.clone());
                identity
            }
            Err(err) => {
                // Leave it unloaded so a later attempt can read it.
                tracing::warn!(error = %err, "failed to read client certificate");
                None
            }
        }
    }

    /// Loads the client identity on a blocking thread, so a keychain prompt
    /// doesn't stall an async worker.
    async fn preload_client_identity(&self) {
        let unloaded = matches!(
            *self
                .client_identity
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ClientIdentitySlot::Unloaded
        );
        if self.runtime.mock_ha || !unloaded {
            return;
        }
        match tokio::task::spawn_blocking(read_client_identity).await {
            Ok(Ok(identity)) => {
                let mut slot = self
                    .client_identity
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // A certificate imported meanwhile wins.
                if matches!(*slot, ClientIdentitySlot::Unloaded) {
                    *slot = ClientIdentitySlot::Loaded(identity);
                }
            }
            Ok(Err(err)) => tracing::warn!(error = %err, "failed to read client certificate"),
            Err(err) => tracing::warn!(error = %err, "client certificate read task failed"),
        }
    }

    fn set_cached_client_identity(&self, identity: Option<Arc<ClientIdentity>>) {
        *self
            .client_identity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            ClientIdentitySlot::Loaded(identity);
    }

    /// Reconnects the saved instance so a certificate change takes effect,
    /// without making the caller wait for Home Assistant.
    async fn reconnect_saved_ha_in_background(&self) {
        let has_saved = self
            .ha_instance
            .lock()
            .await
            .as_ref()
            .is_some_and(|instance| instance.base_url != "mock://home");
        if self.runtime.mock_ha || !has_saved {
            return;
        }
        if let Some(app) = self.weak_self.upgrade() {
            let epoch = app.begin_ha_connect();
            tokio::spawn(async move { app.restore_ha_connection(epoch).await });
        }
    }

    /// Builds the client config for a saved instance, including Home/Remote
    /// routing inputs.
    fn ha_config(
        &self,
        instance: &HaInstance,
        token: String,
        cert_sha256: Option<String>,
    ) -> Result<HaConnectionConfig, flick_ha::HaError> {
        let mut config = HaConnectionConfig::new(instance.base_url.clone(), token)?
            .with_internal_url(instance.internal_url.as_deref())?;
        config.cert_sha256 = cert_sha256;
        config.client_identity = self.client_identity();
        config.trusted_ssids = instance.trusted_ssids.clone();
        config.ha_uuid = instance.ha_uuid.clone();
        config.network = Some(self.network_tx.subscribe());
        Ok(config)
    }

    /// Connects a saved instance. If Home Assistant is unreachable right now
    /// the client is still kept so it retries in the background and the UI
    /// can show why; the registry syncs once it connects.
    async fn connect_saved_ha(
        &self,
        instance: HaInstance,
        token: String,
        cert_sha256: Option<String>,
        restore_epoch: Option<u64>,
    ) -> anyhow::Result<()> {
        let config = self.ha_config(&instance, token, cert_sha256)?;
        let client = HaClient::connect(config).await?;
        let ready = wait_ha_ready(&client).await;
        if let Err(err) = &ready
            && matches!(*client.status().borrow(), HaStatus::AuthFailed { .. })
        {
            anyhow::bail!("{err:#}");
        }
        let snapshot = match ready {
            Ok(_) => client.refresh_registry().await.unwrap_or_default(),
            Err(err) => {
                tracing::warn!(error = %format!("{err:#}"), "Home Assistant not reachable yet; retrying in background");
                self.registry.lock().await.clone()
            }
        };
        self.set_ha_client(client, Some(instance), snapshot, restore_epoch)
            .await;
        Ok(())
    }

    async fn current_entity_state(&self, entity_id: &str) -> Result<EntityState, ApiProblem> {
        let client = self.ha_client.lock().await.clone().ok_or_else(|| {
            ApiProblem::validation("ha_not_configured", "Home Assistant is not configured")
        })?;
        let states = client
            .request(json!({"type":"get_states"}))
            .await
            .map_err(|err| ApiProblem::validation("ha_state_failed", err.to_string()))?;
        let states: Vec<EntityState> = serde_json::from_value(states)
            .map_err(|err| ApiProblem::validation("ha_state_failed", err.to_string()))?;
        states
            .into_iter()
            .find(|state| state.entity_id == entity_id)
            .ok_or_else(|| ApiProblem::validation("entity_not_found", "entity not found"))
    }

    async fn reload_dispatcher_anchors_from_store(&self) -> Result<(), ApiProblem> {
        let anchors = load_api_anchors(&self.targeting)
            .map_err(|err| ApiProblem::validation("anchor_reload_failed", err.to_string()))?;
        let mut dispatcher_anchors =
            dispatcher_anchors_from_api(&anchors, &self.registry.lock().await.clone());
        append_owner_anchor_for_dev(&self.runtime, &mut dispatcher_anchors);
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_anchors(dispatcher_anchors).await;
        }
        self.refresh_selector_anchors().await;
        Ok(())
    }

    /// Taught anchors for the camera's place, which the pointing selector aims at.
    fn selector_anchors_for(&self, camera_id: CameraId) -> Vec<SpatialAnchor> {
        let places = match self.targeting.try_places_for_camera(camera_id) {
            Ok(places) => places,
            Err(err) => {
                tracing::warn!(camera_id = %camera_id, error = %err, "failed to load places for targeting");
                return Vec::new();
            }
        };
        let Some(place) = places
            .iter()
            .find(|place| place.active)
            .or_else(|| places.first())
        else {
            return Vec::new();
        };
        match self.targeting.try_anchors_for_place(place.id) {
            Ok(records) => records.iter().filter_map(anchor_record_to_anchor).collect(),
            Err(err) => {
                tracing::warn!(place_id = %place.id, error = %err, "failed to load anchors for targeting");
                Vec::new()
            }
        }
    }

    async fn refresh_selector_anchors(&self) {
        let capture = self.capture.lock().await;
        if let Some(capture) = capture.as_ref() {
            capture.set_anchors(self.selector_anchors_for(capture.source_info.id));
        }
    }

    fn spawn_pause_timer(self: &Arc<Self>) {
        let app = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let deadline = app
                    .pause
                    .lock()
                    .await
                    .until
                    .as_ref()
                    .map(|(deadline, _)| *deadline);
                match deadline {
                    Some(deadline) => {
                        tokio::select! {
                            () = app.pause_changed.notified() => {}
                            () = tokio::time::sleep_until(deadline) => {
                                tracing::info!("pause expired, resuming");
                                EngineControl::resume(&*app).await;
                            }
                        }
                    }
                    None => app.pause_changed.notified().await,
                }
            }
        });
    }

    /// Clears the pause without reopening the camera. Returns the camera that was running.
    async fn clear_pause(&self) -> Option<String> {
        let camera_id = {
            let mut pause = self.pause.lock().await;
            if !pause.paused {
                return None;
            }
            pause.paused = false;
            pause.until = None;
            pause.camera_id.take()
        };
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_paused(false).await;
        }
        self.pause_changed.notify_one();
        if let Some(events) = self.events.lock().await.clone() {
            events.publish(WsServerMessage::EngineResumed {
                ts: now_rfc3339(),
                payload: EmptyEvent {},
            });
        }
        tracing::info!("engine resumed");
        camera_id
    }

    async fn spawn_camera_autostart(self: &Arc<Self>, onboarding_completed: bool) {
        if (self.runtime.dev && !self.runtime.sidecar)
            || self.runtime.fake_camera.is_some()
            || self.runtime.fake_landmarks.is_some()
        {
            return;
        }
        let app = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = app.autostart_cameras(onboarding_completed).await {
                tracing::warn!(error = %err, "camera autostart failed");
            }
        });
    }

    async fn autostart_cameras(&self, onboarding_completed: bool) -> anyhow::Result<()> {
        let permission = camera_permission_status();
        tracing::info!(
            permission = permission_status_str(permission),
            "camera autostart check"
        );
        let mut cameras = load_cameras(&self.store)?;
        if cameras.is_empty() {
            if !onboarding_completed && permission != CameraPermissionStatus::Authorized {
                tracing::info!(
                    permission = permission_status_str(permission),
                    "skipping default camera creation until onboarding completes or permission is authorized"
                );
                return Ok(());
            }
            if let Some(camera) = self.create_default_camera().await? {
                cameras.push(camera);
            }
        }
        if let Some(camera) = cameras
            .into_iter()
            .filter(|camera| camera.enabled && camera.kind == "local")
            .min_by(|left, right| left.created_at.cmp(&right.created_at))
        {
            tracing::info!(camera_id = %camera.id, camera_name = %camera.name, "autostarting camera");
            let _ = self.start_configured_camera(&camera).await;
        }
        Ok(())
    }

    async fn create_default_camera(&self) -> anyhow::Result<Option<ApiCamera>> {
        let device = LocalCameraSource::enumerate()
            .unwrap_or_default()
            .into_iter()
            .next();
        let device_ref = device
            .as_ref()
            .map(|device| device.stable_id.clone())
            .unwrap_or_else(|| "0".to_owned());
        let name = device
            .as_ref()
            .map(|device| device.name.clone())
            .unwrap_or_else(|| "Built-in camera".to_owned());
        let now = now_ms();
        let camera = ApiCamera {
            id: CameraId::new().to_string(),
            name,
            kind: "local".to_owned(),
            device_ref: Some(device_ref),
            url_redacted: None,
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
            created_at: rfc3339_from_ms(now),
            updated_at: rfc3339_from_ms(now),
        };
        upsert_camera(&self.store, &camera)?;
        tracing::info!(
            camera_id = %camera.id,
            device_ref = ?camera.device_ref,
            "created default camera row"
        );
        Ok(Some(camera))
    }

    fn begin_camera_intent(&self) -> u64 {
        self.camera_intent_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    /// Starts `camera` once the macOS camera prompt is answered, unless a
    /// newer camera start or stop happens first.
    fn start_camera_when_prompt_answered(&self, camera: ApiCamera, intent: u64) {
        let weak = self.weak_self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if app
                    .camera_intent_epoch
                    .load(std::sync::atomic::Ordering::SeqCst)
                    != intent
                {
                    return;
                }
                if camera_permission_status() != CameraPermissionStatus::NotDetermined {
                    tracing::info!(camera_id = %camera.id, "camera prompt answered, starting camera");
                    let _ = app.start_configured_camera(&camera).await;
                    return;
                }
            }
        });
    }

    async fn start_configured_camera(&self, camera: &ApiCamera) -> CameraStatus {
        let intent = self.begin_camera_intent();
        let paused = {
            let mut pause = self.pause.lock().await;
            if pause.paused {
                pause.camera_id = Some(camera.id.clone());
            }
            pause.paused
        };
        if paused {
            let status = paused_camera_status(&camera.id);
            self.publish_camera_status(status.clone()).await;
            return status;
        }
        if let Some(path) = self.runtime.fake_camera.as_deref() {
            return self
                .finish_camera_start(&camera.id, self.start_file_camera(&camera.id, path).await)
                .await;
        }
        if camera.kind != "local" {
            let status = CameraStatus {
                camera_id: camera.id.clone(),
                state: "error".to_owned(),
                fps: Some(0.0),
                error: Some(format!("unsupported camera kind {}", camera.kind)),
            };
            self.publish_camera_status(status.clone()).await;
            return status;
        }
        let source_id = CameraId::from_str(&camera.id).unwrap_or_else(|_| CameraId::new());
        let index = camera
            .device_ref
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok());
        let options = LocalCameraOptions {
            camera_id: source_id,
            index: index.unwrap_or(0),
            device_id: camera.device_ref.clone().filter(|_| index.is_none()),
            width: 1280,
            height: 720,
            fps: camera.active_fps,
            mirror: camera.mirror,
        };
        tracing::info!(
            camera_id = %camera.id,
            device_ref = ?camera.device_ref,
            permission = permission_status_str(camera_permission_status()),
            "starting local camera"
        );
        let opened = tokio::task::spawn_blocking(move || LocalCameraSource::open(options))
            .await
            .map_err(|err| anyhow::anyhow!("camera worker failed: {err}"))
            .and_then(|result| result.map_err(anyhow::Error::from));
        let status = match opened {
            Ok(source) => self.start_source(source).await,
            // The open stopped waiting on the macOS prompt; that isn't a denial.
            Err(err) if camera_permission_status() == CameraPermissionStatus::NotDetermined => {
                tracing::info!(
                    camera_id = %camera.id,
                    error = %err,
                    "camera prompt unanswered, starting once it is"
                );
                self.start_camera_when_prompt_answered(camera.clone(), intent);
                Ok(CameraStatus {
                    camera_id: camera.id.clone(),
                    state: "idle".to_owned(),
                    fps: Some(0.0),
                    error: None,
                })
            }
            Err(err) => Err(err),
        };
        self.finish_camera_start(&camera.id, status).await
    }

    async fn finish_camera_start(
        &self,
        camera_id: &str,
        result: anyhow::Result<CameraStatus>,
    ) -> CameraStatus {
        let status = match result {
            Ok(status) => status,
            Err(err) => camera_error_status(camera_id, &err),
        };
        self.publish_camera_status(status.clone()).await;
        status
    }

    async fn publish_teach_progress(
        &self,
        session_id: &str,
        phase: &str,
        ray_jitter_deg: Option<f64>,
        confidence: Option<f64>,
        hint: Option<&str>,
    ) {
        if let Some(events) = self.events.lock().await.clone() {
            events.publish(WsServerMessage::TeachProgress {
                ts: now_rfc3339(),
                payload: TeachProgressEvent {
                    session_id: session_id.to_owned(),
                    phase: phase.to_owned(),
                    ray_jitter_deg,
                    confidence,
                    hint: hint.map(ToOwned::to_owned),
                },
            });
        }
    }

    async fn publish_camera_status(&self, status: CameraStatus) {
        tracing::info!(
            camera_id = %status.camera_id,
            state = %status.state,
            error = ?status.error,
            "camera status changed"
        );
        if let Some(events) = self.events.lock().await.clone() {
            events.publish(WsServerMessage::CameraStatus {
                ts: now_rfc3339(),
                payload: status,
            });
        }
    }

    async fn publish_ha_status(&self) {
        let status = self.ha_status_dto().await;
        if let Some(events) = self.events.lock().await.clone() {
            events.publish(WsServerMessage::HaStatus {
                ts: now_rfc3339(),
                payload: HaStatusEvent {
                    state: status.state,
                    ha_version: status.ha_version,
                },
            });
        }
    }

    async fn start_file_camera(
        &self,
        camera_id: &str,
        path: &Path,
    ) -> anyhow::Result<CameraStatus> {
        let mut options = FileSourceOptions::fake_camera(path);
        if let Ok(camera_id) = CameraId::from_str(camera_id) {
            options.camera_id = camera_id;
        }
        let source = FileSource::open(options)?;
        self.start_source(source).await
    }

    async fn start_source<S>(&self, source: S) -> anyhow::Result<CameraStatus>
    where
        S: FrameSource,
    {
        let dispatcher = self.dispatcher.lock().await.clone();
        let events = self.events.lock().await.clone();
        let anchors = self.selector_anchors_for(source.info().id);
        let model_root = model_manifest_path(&self.runtime);
        let capture =
            EngineCapture::start(source, dispatcher, events, anchors, model_root.as_deref())?;
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

    /// Seeds an append teach session with the stored anchor so new spots refine it
    /// instead of replacing it. Only the camera/place that taught the anchor can append.
    fn append_seeds(
        &self,
        camera_id: &str,
        anchor_id: Option<AnchorId>,
        target: &TeachTarget,
    ) -> Result<Vec<TeachObservation>, ApiProblem> {
        let id = anchor_id.ok_or_else(|| {
            ApiProblem::validation(
                "append_requires_anchor",
                "Choose the device to add spots to",
            )
        })?;
        let record = self
            .targeting
            .try_anchor(id)
            .map_err(store_problem)?
            .ok_or_else(|| ApiProblem::validation("anchor_not_found", "anchor not found"))?;
        if &record.target != target {
            return Err(ApiProblem::validation(
                "target_mismatch",
                "The device no longer matches this taught spot; teach it again",
            ));
        }
        match CameraId::from_str(camera_id) {
            Ok(camera) => {
                if self.ensure_place(camera)?.id != record.place_id {
                    return Err(ApiProblem::validation(
                        "camera_mismatch",
                        "Add spots with the camera that taught this device",
                    ));
                }
            }
            Err(err) if !self.runtime.dev => {
                return Err(ApiProblem::validation("bad_camera_id", err.to_string()));
            }
            Err(_) => {}
        }
        let anchor = anchor_record_to_anchor(&record).ok_or_else(|| {
            ApiProblem::validation(
                "anchor_geometry",
                "This device's stored aim can't be extended; teach it again",
            )
        })?;
        Ok(seed_observations(&anchor))
    }

    async fn ha_status_dto(&self) -> ApiHaStatus {
        let client = self.ha_client.lock().await.clone();
        let instance = self.ha_instance.lock().await.clone();
        let network_ssid = self.network_tx.borrow().clone();
        let Some(client) = client else {
            let restore_epoch = self
                .ha_restore_epoch
                .load(std::sync::atomic::Ordering::SeqCst);
            let restoring = restore_epoch != 0 && self.is_latest_ha_connect(restore_epoch);
            return ApiHaStatus {
                state: if restoring {
                    "connecting"
                } else {
                    "disconnected"
                }
                .to_owned(),
                instance,
                network_ssid,
                ..ApiHaStatus::default()
            };
        };
        let status = client.status().borrow().clone();
        let route = client.route().borrow().clone();
        let (state, ha_version, last_error) = match status {
            HaStatus::Disconnected => ("disconnected", None, None),
            HaStatus::Connecting => ("connecting", None, None),
            HaStatus::Reconnecting { last_error, .. } => ("connecting", None, last_error),
            HaStatus::Ready { ha_version } => ("ready", ha_version, None),
            HaStatus::AuthFailed { message } => ("auth_failed", None, Some(message)),
        };
        let ready = state == "ready";
        ApiHaStatus {
            state: state.to_owned(),
            ha_version,
            instance,
            last_error,
            connection: route
                .as_ref()
                .filter(|_| ready)
                .map(|route| if route.internal { "home" } else { "remote" }.to_owned()),
            active_url: route.filter(|_| ready).map(|route| route.url),
            network_ssid,
        }
    }
}

struct EngineTeachSession {
    camera_id: String,
    target: serde_json::Value,
    /// Anchor explicitly being re-taught.
    replaces: Option<AnchorId>,
    levels: Vec<f64>,
    spatial: SpatialTeachSession,
}

struct EngineRealignSession {
    place_id: PlaceId,
    pairs: Vec<RealignPair>,
}

/// The WS server forwards at most one `hands` frame per 66 ms per socket.
const HANDS_EVENT_INTERVAL: Duration = Duration::from_millis(70);
/// Brief detector dropouts should not blink the overlay off.
const HANDS_CLEAR_AFTER: Duration = Duration::from_millis(150);
const TARGET_EVENT_INTERVAL: Duration = Duration::from_millis(100);
/// The selector reports hover every frame, so silence this long means the aim was abandoned.
const TARGET_AIMING_STALE: Duration = Duration::from_millis(600);
const VISION_SUMMARY_INTERVAL: Duration = Duration::from_secs(10);
const TEACH_SPOT_SAMPLE_WINDOW: Duration = Duration::from_millis(1_200);
const TEACH_SPOT_MIN_RAYS: usize = 5;
/// A spot this shaky adds noise, not information (its confidence jitter score is already 0).
const TEACH_SPOT_MAX_JITTER_DEG: f32 = 15.0;
/// Upper bound a dev teach spot waits for an in-flight fixture replay.
const REPLAY_SPOT_WAIT: Duration = Duration::from_secs(5);
const POINT_PRESENCE_MIN: f32 = 0.5;
/// Keeps eye-rooted aim through brief face misses, e.g. the pointing hand crossing the face.
const FACE_HOLD: Duration = Duration::from_secs(1);

type PendingAnchors = Arc<std::sync::Mutex<Option<Vec<SpatialAnchor>>>>;
type LatestVision = Arc<std::sync::Mutex<Option<VisionSnapshot>>>;

/// The vision worker's latest result, shared with teaching and status.
#[derive(Clone)]
struct VisionSnapshot {
    hands: HandFrame,
    /// Only tracked while a hand points.
    face: Option<FaceKeypoints>,
}

struct EngineCapture {
    source_info: SourceInfo,
    handle: Option<CaptureHandle>,
    worker_stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    dispatch_task: Option<tokio::task::JoinHandle<()>>,
    preview: FrameTap,
    latest_vision: LatestVision,
    last_error: Arc<std::sync::Mutex<Option<String>>>,
    pending_anchors: PendingAnchors,
}

impl EngineCapture {
    fn start<S>(
        source: S,
        dispatcher: Option<Dispatcher>,
        events: Option<EventHub>,
        anchors: Vec<SpatialAnchor>,
        model_root: Option<&Path>,
    ) -> anyhow::Result<Self>
    where
        S: FrameSource,
    {
        let source_info = source.info().clone();
        let slot = LatestFrameSlot::new();
        let preview = FrameTap::default();
        let handle = spawn_capture_with_tap(source, slot.clone(), Some(preview.clone()));
        let worker_stop = Arc::new(AtomicBool::new(false));
        let latest_vision: LatestVision = Arc::new(std::sync::Mutex::new(None));
        let last_error = Arc::new(std::sync::Mutex::new(None));
        let pending_anchors: PendingAnchors = Arc::new(std::sync::Mutex::new(None));
        let refresh_selection = Arc::new(AtomicBool::new(false));
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<GestureEvent>();
        let (target_tx, mut target_rx) = tokio::sync::mpsc::unbounded_channel::<TargetEvent>();
        // Ends on its own once the worker drops `target_tx`, so the final clear is still delivered.
        if let Some(events) = events.clone() {
            let dispatcher = dispatcher.clone();
            tokio::spawn(async move {
                while let Some(event) = target_rx.recv().await {
                    events.publish(target_message(event, dispatcher.as_ref()).await);
                }
            });
        }
        let dispatch_task = dispatcher.map(|dispatcher| {
            let refresh_selection = Arc::clone(&refresh_selection);
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    let report = dispatcher.dispatch(&event).await;
                    log_dispatch(&event, &report);
                    if event.target.is_some() && !report.outcomes.is_empty() {
                        refresh_selection.store(true, Ordering::Relaxed);
                    }
                }
            })
        });
        let worker = {
            let worker_stop = Arc::clone(&worker_stop);
            let latest_vision = Arc::clone(&latest_vision);
            let last_error = Arc::clone(&last_error);
            let pending_anchors = Arc::clone(&pending_anchors);
            let source_info = source_info.clone();
            let camera_id = source_info.id;
            let (mut pipeline, face_runner) = build_vision(model_root, source_info.mirror);
            let intrinsics = CameraIntrinsics::sane_default(source_info.width, source_info.height);
            let mut faces = FaceTracker::new(face_runner, intrinsics.clone());
            let mut gestures = GestureEngine::new(GestureEngineConfig::default());
            let mut anchor_count = anchors.len();
            tracing::info!(camera_id = %camera_id, anchors = anchor_count, "targeting anchors loaded");
            let mut selector =
                TargetSelectorImpl::new(intrinsics, anchors, TargetSelectorSettings::default());
            let mut hands_feed = events.map(|events| HandsFeed::new(events, camera_id.to_string()));
            let mut targets = TargetFeed::new(target_tx);
            let mut stats = VisionStats::new(Instant::now());
            thread::Builder::new()
                .name(format!("flick-vision-{}", source_info.id))
                .spawn(move || {
                    while !worker_stop.load(Ordering::Relaxed) {
                        let pending = pending_anchors
                            .lock()
                            .ok()
                            .and_then(|mut pending| pending.take());
                        if let Some(anchors) = pending {
                            anchor_count = anchors.len();
                            let selected = selector.selected();
                            selector.set_anchors(anchors);
                            // The selector drops a deleted anchor's selection without an event.
                            if let Some(anchor_id) =
                                selected.filter(|_| selector.selected().is_none())
                            {
                                targets.push(
                                    TargetEvent::Cleared {
                                        camera_id,
                                        anchor_id,
                                        reason: TargetClearReason::Paused,
                                    },
                                    Instant::now(),
                                );
                            }
                            tracing::info!(camera_id = %camera_id, anchors = anchor_count, "targeting anchors updated");
                        }
                        if refresh_selection.swap(false, Ordering::Relaxed) {
                            selector.refresh();
                        }
                        let Some(frame) = slot.wait_latest(Duration::from_millis(100)) else {
                            continue;
                        };
                        match pipeline.process(&frame) {
                            Ok(hands) => {
                                let now = Instant::now();
                                let face = faces.update(&frame, &hands, now);
                                let selection = selector.update(&hands, face.as_ref());
                                for event in selector.take_events() {
                                    targets.push(event, now);
                                }
                                targets.expire_aiming(camera_id, selector.selected(), now);
                                for event in gestures.update(&hands, &selection).events {
                                    let _ = event_tx.send(event);
                                }
                                if let Some(feed) = hands_feed.as_mut() {
                                    feed.publish(&hands, face.as_ref(), now);
                                }
                                let snapshot = VisionSnapshot { hands, face };
                                stats.record(
                                    &snapshot,
                                    &selection,
                                    selector.aim_error_deg(),
                                    camera_id,
                                    anchor_count,
                                    now,
                                );
                                if let Ok(mut latest) = latest_vision.lock() {
                                    *latest = Some(snapshot);
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
                    selector.clear();
                    for event in selector.take_events() {
                        targets.push(event, Instant::now());
                    }
                    targets.finish(camera_id);
                    if let Some(feed) = hands_feed.as_mut() {
                        feed.clear();
                    }
                })
                .context("failed to spawn vision worker")?
        };
        Ok(Self {
            source_info,
            handle: Some(handle),
            worker_stop,
            worker: Some(worker),
            dispatch_task,
            preview,
            latest_vision,
            last_error,
            pending_anchors,
        })
    }

    /// Hands the vision worker a fresh anchor set; it applies it before the next frame.
    fn set_anchors(&self, anchors: Vec<SpatialAnchor>) {
        if let Ok(mut pending) = self.pending_anchors.lock() {
            *pending = Some(anchors);
        }
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
        let frame = self.preview.latest()?;
        if frame.format != flick_core::PixelFormat::Rgb8 {
            return None;
        }
        Some(PreviewFrame {
            width: frame.width,
            height: frame.height,
            seq: frame.seq,
            rgb: frame.data,
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
        if let Some(dispatch_task) = self.dispatch_task.take() {
            dispatch_task.abort();
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
        if let Some(dispatch_task) = self.dispatch_task.take() {
            dispatch_task.abort();
        }
    }
}

/// Publishes throttled landmark frames for the live overlay.
struct HandsFeed {
    events: EventHub,
    camera_id: String,
    last_sent: Option<Instant>,
    last_seq: u64,
    showing: bool,
    empty_since: Option<Instant>,
}

impl HandsFeed {
    fn new(events: EventHub, camera_id: String) -> Self {
        Self {
            events,
            camera_id,
            last_sent: None,
            last_seq: 0,
            showing: false,
            empty_since: None,
        }
    }

    fn publish(&mut self, frame: &HandFrame, face: Option<&FaceKeypoints>, now: Instant) {
        self.last_seq = frame.seq;
        if frame.hands.is_empty() {
            let empty_since = *self.empty_since.get_or_insert(now);
            if now.saturating_duration_since(empty_since) >= HANDS_CLEAR_AFTER {
                self.clear();
            }
            return;
        }
        self.empty_since = None;
        if self
            .last_sent
            .is_some_and(|at| now.saturating_duration_since(at) < HANDS_EVENT_INTERVAL)
        {
            return;
        }
        self.last_sent = Some(now);
        self.showing = true;
        self.send(hands_event(&self.camera_id, frame, face));
    }

    fn clear(&mut self) {
        if !self.showing {
            return;
        }
        self.showing = false;
        self.empty_since = None;
        self.send(HandsEvent {
            camera_id: self.camera_id.clone(),
            seq: self.last_seq,
            hands: Vec::new(),
            ray: None,
        });
    }

    fn send(&self, payload: HandsEvent) {
        self.events.publish(WsServerMessage::Hands {
            ts: now_rfc3339(),
            payload,
        });
    }
}

/// Forwards selector events to the UI, throttling per-frame hover updates.
struct TargetFeed {
    tx: tokio::sync::mpsc::UnboundedSender<TargetEvent>,
    aiming: Option<(AnchorId, Instant)>,
    last_forwarded: Option<Instant>,
    selected: Option<TargetEvent>,
}

impl TargetFeed {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<TargetEvent>) -> Self {
        Self {
            tx,
            aiming: None,
            last_forwarded: None,
            selected: None,
        }
    }

    fn push(&mut self, event: TargetEvent, now: Instant) {
        let forward = match &event {
            TargetEvent::Hover {
                anchor_id,
                name,
                dwell_progress,
                ..
            } => {
                let changed = self.aim(*anchor_id, now);
                if changed {
                    tracing::info!(anchor_id = %anchor_id, name = %name, "pointing at device");
                }
                changed || *dwell_progress >= 1.0 || self.due(now)
            }
            TargetEvent::Ambiguous { anchor_ids, .. } => {
                let changed = self.aim(anchor_ids[0], now);
                if changed {
                    tracing::info!(first = %anchor_ids[0], second = %anchor_ids[1], "pointing between two devices");
                }
                changed || self.due(now)
            }
            TargetEvent::Selected {
                anchor_id, name, ..
            } => {
                tracing::info!(anchor_id = %anchor_id, name = %name, "device selected");
                self.aiming = None;
                self.selected = Some(event.clone());
                true
            }
            TargetEvent::Cleared {
                anchor_id, reason, ..
            } => {
                tracing::info!(anchor_id = %anchor_id, reason = clear_reason_str(*reason), "device target cleared");
                self.aiming = None;
                if self.selected_anchor() == Some(*anchor_id) {
                    self.selected = None;
                }
                true
            }
        };
        if forward {
            self.last_forwarded = Some(now);
            let _ = self.tx.send(event);
        }
    }

    /// Clears a hover or ambiguity the selector abandoned silently, so the HUD never sticks on it.
    fn expire_aiming(&mut self, camera_id: CameraId, selected: Option<AnchorId>, now: Instant) {
        let Some((anchor_id, seen_at)) = self.aiming else {
            return;
        };
        if now.saturating_duration_since(seen_at) < TARGET_AIMING_STALE {
            return;
        }
        self.aiming = None;
        self.last_forwarded = Some(now);
        let restore = self
            .selected
            .clone()
            .filter(|_| selected.is_some() && selected == self.selected_anchor());
        let _ = self.tx.send(restore.unwrap_or(TargetEvent::Cleared {
            camera_id,
            anchor_id,
            reason: TargetClearReason::HandLost,
        }));
    }

    fn finish(&mut self, camera_id: CameraId) {
        if let Some((anchor_id, _)) = self.aiming.take() {
            let _ = self.tx.send(TargetEvent::Cleared {
                camera_id,
                anchor_id,
                reason: TargetClearReason::Paused,
            });
        }
    }

    fn aim(&mut self, anchor_id: AnchorId, now: Instant) -> bool {
        let changed = self.aiming.map(|(current, _)| current) != Some(anchor_id);
        self.aiming = Some((anchor_id, now));
        changed
    }

    fn due(&self, now: Instant) -> bool {
        self.last_forwarded
            .is_none_or(|at| now.saturating_duration_since(at) >= TARGET_EVENT_INTERVAL)
    }

    fn selected_anchor(&self) -> Option<AnchorId> {
        match &self.selected {
            Some(TargetEvent::Selected { anchor_id, .. }) => Some(*anchor_id),
            _ => None,
        }
    }
}

/// Periodic log line that shows whether hands and pointing are being detected and aimed.
struct VisionStats {
    window_start: Instant,
    frames: u32,
    hand_frames: u32,
    point_frames: u32,
    face_frames: u32,
    aim_errors_deg: Vec<f32>,
    hover_frames: u32,
    selected_frames: u32,
}

impl VisionStats {
    const fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            frames: 0,
            hand_frames: 0,
            point_frames: 0,
            face_frames: 0,
            aim_errors_deg: Vec::new(),
            hover_frames: 0,
            selected_frames: 0,
        }
    }

    fn record(
        &mut self,
        snapshot: &VisionSnapshot,
        selection: &SelectionState,
        aim_error_deg: Option<f32>,
        camera_id: CameraId,
        anchors: usize,
        now: Instant,
    ) {
        let frame = &snapshot.hands;
        self.frames = self.frames.saturating_add(1);
        if !frame.hands.is_empty() {
            self.hand_frames = self.hand_frames.saturating_add(1);
        }
        if pointing_hand(&frame.hands).is_some() {
            self.point_frames = self.point_frames.saturating_add(1);
            // Pointing without a face means eye-taught anchors cannot be aimed at.
            if snapshot.face.is_some() {
                self.face_frames = self.face_frames.saturating_add(1);
            }
        }
        self.aim_errors_deg.extend(aim_error_deg);
        match selection {
            SelectionState::Hover { .. } => self.hover_frames = self.hover_frames.saturating_add(1),
            SelectionState::Selected { .. } => {
                self.selected_frames = self.selected_frames.saturating_add(1);
            }
            _ => {}
        }
        if now.saturating_duration_since(self.window_start) < VISION_SUMMARY_INTERVAL {
            return;
        }
        if self.hand_frames > 0 {
            // Median angle between the pointing ray and the closest anchor; high means it never hovers.
            self.aim_errors_deg.sort_by(f32::total_cmp);
            let aim_error_deg = self
                .aim_errors_deg
                .get(self.aim_errors_deg.len() / 2)
                .map(|deg| (deg * 10.0).round() / 10.0);
            tracing::info!(
                camera_id = %camera_id,
                frames = self.frames,
                hand_frames = self.hand_frames,
                point_frames = self.point_frames,
                face_frames = self.face_frames,
                aim_frames = self.aim_errors_deg.len(),
                aim_error_deg,
                hover_frames = self.hover_frames,
                selected_frames = self.selected_frames,
                anchors,
                "vision summary"
            );
        }
        *self = Self::new(now);
    }
}

fn pointing_hand(hands: &[HandObservation]) -> Option<&HandObservation> {
    hands
        .iter()
        .filter(|hand| hand.presence >= POINT_PRESENCE_MIN && is_point_pose(hand))
        .max_by(|a, b| a.presence.total_cmp(&b.presence))
}

/// Face keypoints for eye-rooted aim, detected only while a hand points.
struct FaceTracker {
    runner: Option<FaceKeypointRunner>,
    intrinsics: CameraIntrinsics,
    held: Option<HeldFace>,
    warned: bool,
}

/// The pointing hand's own face, kept briefly through detection misses.
struct HeldFace {
    face: FaceKeypoints,
    track_id: u32,
    at: Instant,
}

impl FaceTracker {
    const fn new(runner: Option<FaceKeypointRunner>, intrinsics: CameraIntrinsics) -> Self {
        Self {
            runner,
            intrinsics,
            held: None,
            warned: false,
        }
    }

    /// With several people in view, returns the face of the one pointing, never a bystander's.
    fn update(&mut self, frame: &Frame, hands: &HandFrame, now: Instant) -> Option<FaceKeypoints> {
        let hand = pointing_hand(&hands.hands)?;
        if let Some(runner) = self.runner.as_mut() {
            match runner.detect(frame) {
                Ok(faces) => {
                    let ipd = RayEstimatorSettings::default().interpupillary_distance_m;
                    if let Some(face) = owner_face(&faces, hand, &self.intrinsics, ipd) {
                        self.held = Some(HeldFace {
                            face: face.clone(),
                            track_id: hand.track_id,
                            at: now,
                        });
                    }
                }
                Err(err) => {
                    if !self.warned {
                        self.warned = true;
                        tracing::warn!(error = %err, "face keypoints failed; aiming falls back to finger-only rays");
                    }
                }
            }
        }
        self.held
            .as_ref()
            .filter(|held| {
                held.track_id == hand.track_id
                    && now.saturating_duration_since(held.at) <= FACE_HOLD
            })
            .map(|held| held.face.clone())
    }
}

fn hands_event(camera_id: &str, frame: &HandFrame, face: Option<&FaceKeypoints>) -> HandsEvent {
    let ray = pointing_hand(&frame.hands).map(|hand| {
        let [tip_x, tip_y, _] = hand.image[8];
        // The drawn line matches the aim: eyes → fingertip when a face is tracked, else along the finger.
        let (origin, extend, model) = match face {
            Some(face) => {
                let [[right_x, right_y], [left_x, left_y], ..] = face.points;
                (
                    [(right_x + left_x) * 0.5, (right_y + left_y) * 0.5],
                    0.5,
                    "eye",
                )
            }
            None => {
                let [mcp_x, mcp_y, _] = hand.image[5];
                ([mcp_x, mcp_y], 1.5, "finger")
            }
        };
        RayEvent {
            origin2d: origin,
            tip2d: [
                tip_x + (tip_x - origin[0]) * extend,
                tip_y + (tip_y - origin[1]) * extend,
            ],
            model: model.to_owned(),
        }
    });
    HandsEvent {
        camera_id: camera_id.to_owned(),
        seq: frame.seq,
        hands: frame
            .hands
            .iter()
            .map(|hand| HandEvent {
                track_id: hand.track_id,
                hand: match hand.hand {
                    Handedness::Left => "left",
                    Handedness::Right => "right",
                }
                .to_owned(),
                landmarks: hand.image.to_vec(),
                bbox: [hand.bbox.x, hand.bbox.y, hand.bbox.w, hand.bbox.h],
            })
            .collect(),
        ray,
    }
}

async fn target_message(event: TargetEvent, dispatcher: Option<&Dispatcher>) -> WsServerMessage {
    let ts = now_rfc3339();
    match event {
        TargetEvent::Hover {
            camera_id,
            anchor_id,
            name,
            score,
            dwell_progress,
            runner_up,
        } => WsServerMessage::TargetHover {
            ts,
            payload: TargetHoverEvent {
                camera_id: camera_id.to_string(),
                anchor_id: anchor_id.to_string(),
                name,
                score: f64::from(score),
                dwell_progress: f64::from(dwell_progress),
                runner_up: runner_up.map(|id| id.to_string()),
            },
        },
        TargetEvent::Selected {
            camera_id,
            anchor_id,
            name,
            domain,
            expires_at_ms,
        } => {
            let verbs = match dispatcher {
                Some(dispatcher) => dispatcher
                    .anchor_verbs(anchor_id)
                    .await
                    .iter()
                    .map(|(gesture_id, action)| VerbBinding {
                        gesture_id: gesture_id.to_string(),
                        label: format!("{} → {}", gesture_label(gesture_id), action_label(action)),
                    })
                    .collect(),
                None => Vec::new(),
            };
            WsServerMessage::TargetSelected {
                ts,
                payload: TargetSelectedEvent {
                    camera_id: camera_id.to_string(),
                    anchor_id: anchor_id.to_string(),
                    name,
                    domain,
                    expires_at: rfc3339_from_ms(expires_at_ms),
                    verbs,
                },
            }
        }
        TargetEvent::Cleared {
            camera_id,
            anchor_id,
            reason,
        } => WsServerMessage::TargetCleared {
            ts,
            payload: TargetClearedEvent {
                camera_id: camera_id.to_string(),
                anchor_id: anchor_id.to_string(),
                reason: clear_reason_str(reason).to_owned(),
            },
        },
        TargetEvent::Ambiguous {
            camera_id,
            anchor_ids,
        } => WsServerMessage::TargetAmbiguous {
            ts,
            payload: TargetAmbiguousEvent {
                camera_id: camera_id.to_string(),
                anchor_ids: anchor_ids.iter().map(ToString::to_string).collect(),
            },
        },
    }
}

const fn clear_reason_str(reason: TargetClearReason) -> &'static str {
    match reason {
        TargetClearReason::Timeout => "timeout",
        TargetClearReason::HandLost => "hand_lost",
        TargetClearReason::Reselected => "reselected",
        TargetClearReason::Paused => "paused",
    }
}

fn gesture_label(gesture_id: &GestureId) -> String {
    use flick_core::BuiltinGesture as Builtin;
    let GestureId::Builtin(builtin) = gesture_id else {
        return "Custom gesture".to_owned();
    };
    match builtin {
        Builtin::ClosedFist => "Fist",
        Builtin::OpenPalm => "Open palm",
        Builtin::PointingUp => "Point up",
        Builtin::ThumbUp => "Thumbs up",
        Builtin::ThumbDown => "Thumbs down",
        Builtin::Victory => "Victory",
        Builtin::ILoveYou => "I love you",
        Builtin::Point => "Point",
        Builtin::SwipeLeft => "Swipe left",
        Builtin::SwipeRight => "Swipe right",
        Builtin::SwipeUp => "Swipe up",
        Builtin::SwipeDown => "Swipe down",
        Builtin::PinchDial => "Pinch dial",
        Builtin::CircleCw => "Circle clockwise",
        Builtin::CircleCcw => "Circle counter-clockwise",
        Builtin::CircleAny => "Circle",
        Builtin::TwoHandSeparate => "Two hands apart",
    }
    .to_owned()
}

fn action_label(action: &Action) -> String {
    match action {
        Action::Verb { verb, level } => match verb {
            Verb::Up => "Up".to_owned(),
            Verb::Down => "Down".to_owned(),
            Verb::On => "On".to_owned(),
            Verb::Off => "Off".to_owned(),
            Verb::Stop => "Stop".to_owned(),
            Verb::Toggle => "Toggle".to_owned(),
            Verb::LevelSet => format!("Speed {}", level.unwrap_or(1)),
        },
        Action::CallService {
            domain, service, ..
        } => format!("{domain}.{service}"),
        Action::Dial { .. } => "Dial".to_owned(),
    }
}

fn log_dispatch(event: &GestureEvent, report: &DispatchReport) {
    if event.phase != GesturePhase::Fired {
        return;
    }
    let outcomes = report
        .outcomes
        .iter()
        .map(|outcome| format!("{:?}", outcome.status))
        .collect::<Vec<_>>();
    let suppressed = report
        .suppressions
        .iter()
        .map(|suppression| format!("{:?}", suppression.reason))
        .collect::<Vec<_>>();
    tracing::info!(
        gesture = %event.gesture_id,
        target = %event.target.map_or_else(|| "none".to_owned(), |id| id.to_string()),
        confidence = event.confidence,
        outcomes = ?outcomes,
        suppressed = ?suppressed,
        "gesture fired"
    );
}

/// Pointing rays sampled for one teach spot, per ray model.
struct SpotRays {
    eye: Vec<PointingRay>,
    finger: Vec<PointingRay>,
}

impl SpotRays {
    fn from_rays(rays: Vec<PointingRay>) -> Self {
        let (eye, finger) = rays
            .into_iter()
            .partition(|ray| ray.source == RaySource::EyeRooted);
        Self { eye, finger }
    }
}

/// Collects filtered eye-rooted and finger-only pointing rays from the running camera for one teach spot.
async fn sample_pointing_rays(
    latest_vision: &std::sync::Mutex<Option<VisionSnapshot>>,
    intrinsics: CameraIntrinsics,
) -> SpotRays {
    let settings = TargetSelectorSettings::default().ray;
    let mut eye_estimator = RayEstimator::new(
        intrinsics.clone(),
        RayEstimatorSettings {
            model: RayModel::Eye,
            ..settings.clone()
        },
    );
    let mut finger_estimator = RayEstimator::new(
        intrinsics,
        RayEstimatorSettings {
            model: RayModel::Finger,
            ..settings
        },
    );
    let mut rays = SpotRays {
        eye: Vec::new(),
        finger: Vec::new(),
    };
    let mut last_seq = None;
    let deadline = Instant::now() + TEACH_SPOT_SAMPLE_WINDOW;
    while Instant::now() < deadline {
        let snapshot = latest_vision.lock().ok().and_then(|latest| latest.clone());
        if let Some(VisionSnapshot { hands: frame, face }) = snapshot
            && last_seq != Some(frame.seq)
        {
            last_seq = Some(frame.seq);
            if let Some(hand) = pointing_hand(&frame.hands) {
                if let Some(face) = face.as_ref()
                    && let Ok(ray) = eye_estimator.estimate(hand, Some(face), frame.captured_at)
                {
                    rays.eye.push(ray);
                }
                if let Ok(ray) = finger_estimator.estimate(hand, None, frame.captured_at) {
                    rays.finger.push(ray);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(33)).await;
    }
    rays
}

/// Averages the settled half of the samples and reports the p90 angular spread in degrees.
fn settle_rays(rays: &[PointingRay]) -> Option<(PointingRay, f32)> {
    let settled = &rays[rays.len() / 2..];
    let last = settled.last()?;
    let count = settled.len() as f32;
    let mut origin = [0.0_f32; 3];
    let mut direction = [0.0_f32; 3];
    for ray in settled {
        for axis in 0..3 {
            origin[axis] += ray.origin[axis] / count;
            direction[axis] += ray.direction[axis];
        }
    }
    let norm = direction
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm <= f32::EPSILON {
        return None;
    }
    let direction = direction.map(|value| value / norm);
    let mut spread = settled
        .iter()
        .map(|ray| {
            let dot = (0..3)
                .map(|axis| ray.direction[axis] * direction[axis])
                .sum::<f32>();
            dot.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .collect::<Vec<_>>();
    spread.sort_by(f32::total_cmp);
    let p90 = spread[((spread.len() - 1) * 9) / 10];
    let mut ray = last.clone();
    ray.origin = origin;
    ray.direction = direction;
    Some((ray, p90))
}

/// One ray model's settled ray for a teach spot.
struct SettledRays {
    ray: PointingRay,
    jitter_deg: f32,
    frames: u32,
}

fn settle_model(rays: &[PointingRay]) -> Option<SettledRays> {
    if rays.len() < TEACH_SPOT_MIN_RAYS {
        return None;
    }
    let (ray, jitter_deg) = settle_rays(rays)?;
    Some(SettledRays {
        ray,
        jitter_deg,
        frames: u32::try_from(rays.len()).unwrap_or(u32::MAX),
    })
}

#[async_trait]
impl EngineControl for EngineApp {
    async fn status(&self) -> EngineStatus {
        let (paused, paused_until) = {
            let pause = self.pause.lock().await;
            (
                pause.paused,
                pause.until.as_ref().map(|(_, until)| until.clone()),
            )
        };
        let _ = self.started_at.elapsed();
        let _ = self.store.db_path();
        let _ = self.dispatcher.lock().await.is_some();
        let capture = self.capture.lock().await;
        let cameras = capture
            .as_ref()
            .map(|capture| vec![capture.status()])
            .unwrap_or_else(|| {
                let configured = load_cameras(&self.store).unwrap_or_default();
                if configured.is_empty() {
                    vec![CameraStatus {
                        camera_id: "dev-camera".to_owned(),
                        state: "idle".to_owned(),
                        fps: Some(0.0),
                        error: None,
                    }]
                } else {
                    configured
                        .into_iter()
                        .map(|camera| CameraStatus {
                            camera_id: camera.id,
                            state: match (camera.enabled, paused) {
                                (false, _) => "disabled",
                                (true, true) => "paused",
                                (true, false) => "idle",
                            }
                            .to_owned(),
                            fps: Some(0.0),
                            error: None,
                        })
                        .collect()
                }
            });
        let stages = capture
            .as_ref()
            .and_then(|capture| {
                capture
                    .latest_vision
                    .lock()
                    .ok()
                    .and_then(|latest| latest.as_ref().map(|snapshot| snapshot.hands.timings))
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
            paused_until,
            cameras,
            ha: self.ha_status_dto().await,
            stages,
            camera_permission: Some(permission_status_str(camera_permission_status()).to_owned()),
        }
    }

    async fn pause(&self, request: PauseRequest) -> EngineStatus {
        let until = request
            .duration_s
            .filter(|seconds| *seconds > 0)
            .map(|seconds| {
                let millis = i64::try_from(seconds.saturating_mul(1000)).unwrap_or(i64::MAX);
                (
                    tokio::time::Instant::now() + Duration::from_secs(seconds),
                    rfc3339_from_ms(now_ms().saturating_add(millis)),
                )
            });
        let until_label = until.as_ref().map(|(_, label)| label.clone());
        // Release the camera so the privacy indicator turns off while paused.
        let stopped = self.capture.lock().await.take().map(EngineCapture::stop);
        {
            let mut pause = self.pause.lock().await;
            pause.paused = true;
            pause.until = until;
            if let Some(stopped) = &stopped {
                pause.camera_id = Some(stopped.camera_id.clone());
            }
        }
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_paused(true).await;
        }
        self.pause_changed.notify_one();
        if let Some(stopped) = stopped {
            self.publish_camera_status(paused_camera_status(&stopped.camera_id))
                .await;
        }
        if let Some(events) = self.events.lock().await.clone() {
            events.publish(WsServerMessage::EnginePaused {
                ts: now_rfc3339(),
                payload: EnginePausedEvent {
                    until: until_label.clone(),
                },
            });
        }
        tracing::info!(until = ?until_label, "engine paused, camera released");
        EngineControl::status(self).await
    }

    async fn resume(&self) -> EngineStatus {
        if let Some(camera_id) = self.clear_pause().await
            && let Ok(Some(camera)) = load_camera(&self.store, &camera_id)
            && camera.enabled
        {
            let _ = self.start_configured_camera(&camera).await;
        }
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

    async fn cameras(&self) -> Vec<ApiCamera> {
        let registry = self.registry.lock().await.clone();
        load_cameras(&self.store)
            .unwrap_or_default()
            .into_iter()
            .map(|camera| with_ha_area(camera, &registry))
            .collect()
    }

    async fn create_camera(&self, request: CameraCreate) -> Result<ApiCamera, ApiProblem> {
        let now = now_ms();
        let camera = ApiCamera {
            id: CameraId::new().to_string(),
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
            created_at: rfc3339_from_ms(now),
            updated_at: rfc3339_from_ms(now),
        };
        upsert_camera(&self.store, &camera).map_err(store_problem)?;
        let _ = self.start_configured_camera(&camera).await;
        Ok(with_ha_area(camera, &self.registry.lock().await.clone()))
    }

    async fn patch_camera(&self, id: &str, request: CameraPatch) -> Result<ApiCamera, ApiProblem> {
        let mut camera = load_camera(&self.store, id)
            .map_err(store_problem)?
            .ok_or_else(|| {
                ApiProblem::new(
                    StatusCode::NOT_FOUND,
                    "camera_not_found",
                    "camera not found",
                )
            })?;
        let was_enabled = camera.enabled;
        if let Some(name) = request.name {
            camera.name = name;
        }
        if let Some(enabled) = request.enabled {
            camera.enabled = enabled;
        }
        if let Some(mirror) = request.mirror {
            camera.mirror = mirror;
        }
        if let Some(rotation) = request.rotation {
            camera.rotation = rotation;
        }
        if let Some(active_fps) = request.active_fps {
            camera.active_fps = active_fps;
        }
        if let Some(idle_fps) = request.idle_fps {
            camera.idle_fps = idle_fps;
        }
        if let Some(max_hands) = request.max_hands {
            camera.max_hands = max_hands;
        }
        if request.roi.is_some() {
            camera.roi = request.roi;
        }
        if let Some(area_override) = request.area_override {
            camera.area_override = area_override;
        }
        camera.updated_at = now_rfc3339();
        upsert_camera(&self.store, &camera).map_err(store_problem)?;
        match (was_enabled, camera.enabled) {
            (false, true) => {
                let _ = self.start_configured_camera(&camera).await;
            }
            (true, false) => {
                let _ = self.stop_camera(&camera.id).await;
            }
            _ => {}
        }
        Ok(with_ha_area(camera, &self.registry.lock().await.clone()))
    }

    async fn delete_camera(&self, id: &str) -> Result<(), ApiProblem> {
        let _ = self.stop_camera(id).await;
        delete_camera_row(&self.store, id).map_err(store_problem)?;
        Ok(())
    }

    async fn start_camera(&self, camera_id: &str) -> CameraStatus {
        // Starting a camera by hand ends a pause.
        let _ = self.clear_pause().await;
        match load_camera(&self.store, camera_id) {
            Ok(Some(camera)) => self.start_configured_camera(&camera).await,
            Ok(None) => {
                let camera = ApiCamera {
                    id: camera_id.to_owned(),
                    name: "Local camera".to_owned(),
                    kind: "local".to_owned(),
                    device_ref: Some(camera_id.to_owned()),
                    url_redacted: None,
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
                    created_at: now_rfc3339(),
                    updated_at: now_rfc3339(),
                };
                self.start_configured_camera(&camera).await
            }
            Err(err) => CameraStatus {
                camera_id: camera_id.to_owned(),
                state: "error".to_owned(),
                fps: Some(0.0),
                error: Some(err.to_string()),
            },
        }
    }

    async fn stop_camera(&self, camera_id: &str) -> CameraStatus {
        self.begin_camera_intent();
        {
            let mut pause = self.pause.lock().await;
            if pause.camera_id.as_deref() == Some(camera_id) {
                pause.camera_id = None;
            }
        }
        let mut capture = self.capture.lock().await;
        let status = if let Some(capture) = capture.take() {
            capture.stop()
        } else {
            CameraStatus {
                camera_id: camera_id.to_owned(),
                state: "stopped".to_owned(),
                fps: Some(0.0),
                error: None,
            }
        };
        tracing::info!(camera_id = %camera_id, "stopped camera");
        self.publish_camera_status(status.clone()).await;
        status
    }
}

#[async_trait]
impl TeachGateway for EngineApp {
    async fn start(&self, request: TeachRequest) -> Result<ApiTeachSession, ApiProblem> {
        let target = teach_target_from_value(&request.target)?;
        let replaces = request
            .anchor_id
            .as_deref()
            .map(AnchorId::from_str)
            .transpose()
            .map_err(|err| ApiProblem::validation("bad_anchor_id", err.to_string()))?;
        let seeds = if request.append {
            self.append_seeds(&request.camera_id, replaces, &target)?
        } else {
            Vec::new()
        };
        let domain = teach_domain(&target);
        let name = teach_name(&target);
        let mut spatial = SpatialTeachSession::new(
            replaces.unwrap_or_else(AnchorId::new),
            name,
            target,
            domain,
            DEFAULT_ESTIMATOR_VERSION,
        );
        for seed in seeds {
            spatial.add_observation(seed);
        }
        let prompt = if request.append {
            "Point at the device from a new spot"
        } else {
            "Point at the device and hold still"
        };
        let session = ApiTeachSession {
            id: flick_core::TeachSessionId::new().to_string(),
            camera_id: request.camera_id,
            target: request.target,
            anchor_id: request.anchor_id,
            prompt: prompt.to_owned(),
        };
        self.teach_sessions.lock().await.insert(
            session.id.clone(),
            EngineTeachSession {
                camera_id: session.camera_id.clone(),
                target: session.target.clone(),
                replaces,
                levels: Vec::new(),
                spatial,
            },
        );
        Ok(session)
    }

    async fn spot(&self, session_id: &str) -> Result<TeachSpotResponse, ApiProblem> {
        let camera_id = self
            .teach_sessions
            .lock()
            .await
            .get(session_id)
            .map(|session| session.camera_id.clone())
            .ok_or_else(|| {
                ApiProblem::validation("teach_session_not_found", "teach session not found")
            })?;
        let live = {
            let capture = self.capture.lock().await;
            capture
                .as_ref()
                .filter(|capture| capture.source_info.id.to_string() == camera_id)
                .map(|capture| {
                    (
                        Arc::clone(&capture.latest_vision),
                        CameraIntrinsics::sane_default(
                            capture.source_info.width,
                            capture.source_info.height,
                        ),
                    )
                })
        };
        // Fake-landmark dev engines have no live camera; replayed fixtures stand in for it.
        if live.is_none() && self.runtime.fake_landmarks.is_none() {
            return Err(ApiProblem::validation(
                "camera_not_running",
                "Start this camera before capturing a spot",
            ));
        }
        self.publish_teach_progress(session_id, "capturing", None, None, None)
            .await;
        let sampled = match live {
            Some((latest_vision, intrinsics)) => {
                sample_pointing_rays(&latest_vision, intrinsics).await
            }
            None => SpotRays::from_rays(self.replayed_pointing_rays().await),
        };
        // Eye-rooted rays aim best across a room, so they lead when the face is visible; the
        // finger-only ray is kept alongside as the fallback for when the face is not.
        let eye_rays = sampled.eye.len();
        let finger_rays = sampled.finger.len();
        let eye = settle_model(&sampled.eye);
        let finger = settle_model(&sampled.finger);
        let unsteady_jitter = eye
            .as_ref()
            .or(finger.as_ref())
            .map(|settled| settled.jitter_deg);
        let (primary, fallback) = match (eye, finger) {
            (Some(eye), finger) if eye.jitter_deg <= TEACH_SPOT_MAX_JITTER_DEG => (eye, finger),
            (_, Some(finger)) if finger.jitter_deg <= TEACH_SPOT_MAX_JITTER_DEG => (finger, None),
            _ => {
                if let Some(jitter_deg) = unsteady_jitter {
                    let hint = "Hold your pointing hand still until the capture finishes";
                    tracing::info!(
                        session_id,
                        eye_rays,
                        finger_rays,
                        jitter_deg,
                        "teach spot rejected an unsteady pointing hand"
                    );
                    self.publish_teach_progress(
                        session_id,
                        "error",
                        Some(f64::from(jitter_deg)),
                        None,
                        Some(hint),
                    )
                    .await;
                    return Err(ApiProblem::validation("unsteady_pointing_hand", hint));
                }
                let hint =
                    "Point at the device with your index finger so the camera can see your hand";
                tracing::info!(
                    session_id,
                    eye_rays,
                    finger_rays,
                    "teach spot found no steady pointing hand"
                );
                self.publish_teach_progress(session_id, "error", None, None, Some(hint))
                    .await;
                return Err(ApiProblem::validation("no_pointing_hand", hint));
            }
        };
        let jitter_deg = primary.jitter_deg;
        let ray_source = primary.ray.source;
        let finger_fallback = fallback.is_some();
        let (spot_index, preview) = {
            let mut sessions = self.teach_sessions.lock().await;
            let session = sessions.get_mut(session_id).ok_or_else(|| {
                ApiProblem::validation("teach_session_not_found", "teach session not found")
            })?;
            let spot_index = session.spatial.spots().saturating_add(1);
            for settled in std::iter::once(primary).chain(fallback) {
                session.spatial.add_observation(TeachObservation {
                    spot_index,
                    ray: settled.ray,
                    frames: settled.frames,
                    ray_jitter_deg: settled.jitter_deg,
                });
            }
            let preview = session
                .spatial
                .finish(&[])
                .map_err(|err| ApiProblem::validation("teach_failed", err.to_string()))?;
            (spot_index, preview)
        };
        let kind = match preview.anchor.geometry {
            AnchorGeometry::Point3d { .. } => "point3d",
            AnchorGeometry::Direction { .. } => "direction",
        };
        let confidence = f64::from(preview.quality.confidence);
        tracing::info!(
            session_id,
            spot_index,
            eye_rays,
            finger_rays,
            ray_source = ?ray_source,
            finger_fallback,
            jitter_deg,
            kind,
            confidence,
            "teach spot captured"
        );
        self.publish_teach_progress(
            session_id,
            "captured",
            Some(f64::from(jitter_deg)),
            Some(confidence),
            None,
        )
        .await;
        Ok(TeachSpotResponse {
            spot_index,
            ray_jitter_deg: f64::from(jitter_deg),
            confidence,
            kind: kind.to_owned(),
            residual_deg: (spot_index >= 2).then(|| f64::from(preview.quality.residual_deg)),
        })
    }

    async fn use_current_level(
        &self,
        session_id: &str,
        request: TeachLevelRequest,
    ) -> Result<TeachLevelResponse, ApiProblem> {
        let entity_id = {
            let sessions = self.teach_sessions.lock().await;
            let session = sessions.get(session_id).ok_or_else(|| {
                ApiProblem::validation("teach_session_not_found", "teach session not found")
            })?;
            target_entity_id(&session.target)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    ApiProblem::validation(
                        "unsupported_target",
                        "level capture requires an entity target",
                    )
                })?
        };
        let entity = self.current_entity_state(&entity_id).await?;
        let mut sessions = self.teach_sessions.lock().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            ApiProblem::validation("teach_session_not_found", "teach session not found")
        })?;
        session.levels =
            record_current_fan_level(session.levels.clone(), request.level, &entity)
                .map_err(|err| ApiProblem::validation("level_not_available", err.to_string()))?;
        let current_percentage = entity.attr_f64("percentage").unwrap_or(0.0);
        Ok(TeachLevelResponse {
            levels: session.levels.clone(),
            current_percentage,
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
        let index = usize::try_from(request.level.saturating_sub(1))
            .map_err(|err| ApiProblem::validation("bad_level", err.to_string()))?;
        let percentage = session.levels.get(index).copied().ok_or_else(|| {
            ApiProblem::validation("level_not_taught", "level has not been taught")
        })?;
        let action = Action::CallService {
            domain: domain.to_owned(),
            service: "turn_on".to_owned(),
            target: ActionTarget {
                entity_id: Some(vec![entity_id.to_owned()]),
                device_id: None,
                area_id: None,
            },
            data: json!({"percentage": percentage}),
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
        let camera_id = match CameraId::from_str(&session.camera_id) {
            Ok(id) => id,
            Err(err) if self.runtime.dev => {
                tracing::warn!(camera_id = %session.camera_id, error = %err, "using synthetic camera id for dev teach session");
                CameraId::new()
            }
            Err(err) => return Err(ApiProblem::validation("bad_camera_id", err.to_string())),
        };
        let place = self.ensure_place(camera_id)?;
        let records = self
            .targeting
            .try_anchors_for_place(place.id)
            .map_err(store_problem)?;
        // Re-teaching a device replaces its anchor in place, so its mappings keep working.
        let target = teach_target_from_value(&session.target)?;
        let replaced = session
            .replaces
            .and_then(|id| records.iter().find(|record| record.id == id))
            .or_else(|| records.iter().find(|record| record.target == target))
            .cloned();
        let existing = records
            .iter()
            .filter(|record| replaced.as_ref().is_none_or(|old| old.id != record.id))
            .filter_map(anchor_record_to_anchor)
            .collect::<Vec<_>>();
        let mut outcome = session
            .spatial
            .finish(&existing)
            .map_err(|err| ApiProblem::validation("teach_failed", err.to_string()))?;
        if let Some(old) = &replaced {
            outcome.anchor.id = old.id;
            outcome.anchor.name.clone_from(&old.name);
            outcome.anchor.verb_params = old.verb_params.clone();
        }
        if let Some(name) = request.name {
            outcome.anchor.name = name;
        }
        if !session.levels.is_empty() {
            outcome.anchor.verb_params = json!({ "levels": session.levels });
        }
        let mut record = anchor_to_record(
            place.id,
            &outcome.anchor,
            replaced.as_ref().is_some_and(|old| old.sensitive),
            replaced.as_ref().is_some_and(|old| old.sensitive_ack),
            now_ms(),
        );
        if let Some(old) = &replaced {
            record.created_at = old.created_at;
            record.last_used_at = old.last_used_at;
            tracing::info!(anchor_id = %old.id, name = %record.name, "re-taught device anchor");
        }
        self.targeting
            .try_upsert_anchor(&record)
            .map_err(store_problem)?;
        self.reload_dispatcher_anchors_from_store().await?;
        Ok(TeachCommitResponse {
            anchor: api_anchor_from_record(&record, Some(place.camera_id.to_string())),
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
        let realigned = |geometry: AnchorGeometry| match geometry {
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
        for record in records {
            let Some(mut anchor) = anchor_record_to_anchor(&record) else {
                continue;
            };
            anchor.geometry = realigned(anchor.geometry);
            anchor.finger_aim = anchor.finger_aim.take().map(|aim| FingerAim {
                geometry: realigned(aim.geometry),
                uncertainty_deg: aim.uncertainty_deg,
            });
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
        self.refresh_selector_anchors().await;
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
        if self.runtime.mock_ha && request.base_url == "mock://home" {
            let instance = self.ha_instance.lock().await.clone().unwrap_or(HaInstance {
                id: "mock-ha".to_owned(),
                name: "Mock Home".to_owned(),
                base_url: request.base_url,
                ha_uuid: Some("mock-ha".to_owned()),
                auth_kind: "llat".to_owned(),
                ha_version: Some("2026.9.0".to_owned()),
                is_default: true,
                created_at: now_rfc3339(),
                updated_at: now_rfc3339(),
                ..HaInstance::default()
            });
            return Ok(instance);
        }
        let token = request.token.clone();
        let cert_sha256 = request.trust_cert_sha256.clone();
        let mut config = HaConnectionConfig::new(request.base_url.clone(), token.clone())
            .map_err(|err| ApiProblem::validation("ha_invalid_url", err.to_string()))?;
        config.cert_sha256 = cert_sha256.clone();
        config.client_identity = self.client_identity();
        let client = HaClient::connect(config)
            .await
            .map_err(|err| ApiProblem::validation("ha_connect_failed", err.to_string()))?;
        let ha_version = wait_ha_ready(&client)
            .await
            .map_err(|err| ApiProblem::validation("ha_connect_failed", err.to_string()))?;
        let config = client
            .request(json!({"type":"get_config"}))
            .await
            .map_err(|err| ApiProblem::validation("ha_config_failed", err.to_string()))?;
        let snapshot = client
            .refresh_registry()
            .await
            .map_err(|err| ApiProblem::validation("ha_registry_failed", err.to_string()))?;
        let ha_uuid = config
            .get("uuid")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let instance_id = ha_uuid
            .clone()
            .unwrap_or_else(|| flick_core::HaInstanceId::new().to_string());
        let keychain_ref = format!("ha:{instance_id}");
        let previous = if self.runtime.mock_ha {
            None
        } else {
            load_default_ha_instance(&self.store)
                .ok()
                .flatten()
                .filter(|saved| saved.instance.id == instance_id)
                .map(|saved| saved.instance)
        };
        if !self.runtime.mock_ha {
            KeyringSecretStore::new()
                .set(&keychain_ref, &token)
                .map_err(|err| ApiProblem::validation("ha_keychain_failed", err.to_string()))?;
        }
        let instance = HaInstance {
            id: instance_id,
            name: config
                .get("location_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Home Assistant")
                .to_owned(),
            base_url: request.base_url,
            ha_uuid,
            auth_kind: "llat".to_owned(),
            ha_version: config
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| (!ha_version.is_empty()).then_some(ha_version)),
            is_default: true,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            internal_url: previous
                .as_ref()
                .and_then(|saved| saved.internal_url.clone()),
            trusted_ssids: previous
                .map(|saved| saved.trusted_ssids)
                .unwrap_or_default(),
        };
        if !self.runtime.mock_ha {
            persist_ha_instance(
                &self.store,
                &instance,
                &keychain_ref,
                cert_sha256.as_deref(),
            )
            .map_err(|err| ApiProblem::validation("ha_store_failed", err.to_string()))?;
        }
        self.set_ha_client(client, Some(instance.clone()), snapshot, None)
            .await;
        Ok(instance)
    }

    async fn status(&self) -> ApiHaStatus {
        self.ha_status_dto().await
    }

    async fn update(&self, update: HaConnectionUpdate) -> Result<HaInstance, ApiProblem> {
        let current = self.ha_instance.lock().await.clone();
        let is_mock = self.runtime.mock_ha
            || current
                .as_ref()
                .is_some_and(|instance| instance.base_url == "mock://home");
        if is_mock {
            let mut instance = current.ok_or_else(|| {
                ApiProblem::validation("ha_not_connected", "Connect Home Assistant first")
            })?;
            apply_ha_update(&mut instance, update)?;
            *self.ha_instance.lock().await = Some(instance.clone());
            return Ok(instance);
        }
        let saved = load_default_ha_instance(&self.store)
            .map_err(|err| ApiProblem::validation("ha_store_failed", err.to_string()))?
            .ok_or_else(|| {
                ApiProblem::validation("ha_not_connected", "Connect Home Assistant first")
            })?;
        let token = KeyringSecretStore::new()
            .get(&saved.keychain_ref)
            .map_err(|err| ApiProblem::validation("ha_keychain_failed", err.to_string()))?
            .ok_or_else(|| {
                ApiProblem::validation(
                    "ha_token_missing",
                    "The saved access token is missing. Reconnect Home Assistant.",
                )
            })?;
        let mut instance = saved.instance;
        apply_ha_update(&mut instance, update)?;
        self.ha_config(&instance, token.clone(), saved.cert_sha256.clone())
            .map_err(|err| ApiProblem::validation("ha_invalid_url", err.to_string()))?;
        instance.updated_at = now_rfc3339();
        persist_ha_instance(
            &self.store,
            &instance,
            &saved.keychain_ref,
            saved.cert_sha256.as_deref(),
        )
        .map_err(|err| ApiProblem::validation("ha_store_failed", err.to_string()))?;
        self.connect_saved_ha(instance.clone(), token, saved.cert_sha256, None)
            .await
            .map_err(|err| ApiProblem::validation("ha_connect_failed", format!("{err:#}")))?;
        Ok(instance)
    }

    async fn set_network(&self, report: NetworkReport) {
        let ssid = report
            .ssid
            .map(|ssid| ssid.trim().to_owned())
            .filter(|ssid| !ssid.is_empty());
        self.network_tx.send_if_modified(|current| {
            if *current == ssid {
                false
            } else {
                *current = ssid;
                true
            }
        });
    }

    async fn client_certificate(&self) -> HaClientCertificate {
        client_certificate_dto(self.client_identity().as_deref())
    }

    async fn set_client_certificate(
        &self,
        data: Vec<u8>,
        password: Option<String>,
    ) -> Result<HaClientCertificate, ApiProblem> {
        let identity = ClientIdentity::import(&data, password.as_deref())
            .map_err(|err| ApiProblem::validation(err.code(), err.to_string()))?;
        if !self.runtime.mock_ha {
            KeyringSecretStore::new()
                .set(HA_CLIENT_CERT_ACCOUNT, &identity.to_pem())
                .map_err(|err| ApiProblem::validation("ha_keychain_failed", err.to_string()))?;
        }
        let dto = client_certificate_dto(Some(&identity));
        self.set_cached_client_identity(Some(Arc::new(identity)));
        self.reconnect_saved_ha_in_background().await;
        Ok(dto)
    }

    async fn delete_client_certificate(&self) -> Result<(), ApiProblem> {
        if !self.runtime.mock_ha {
            KeyringSecretStore::new()
                .delete(HA_CLIENT_CERT_ACCOUNT)
                .map_err(|err| ApiProblem::validation("ha_keychain_failed", err.to_string()))?;
        }
        self.set_cached_client_identity(None);
        self.reconnect_saved_ha_in_background().await;
        Ok(())
    }

    async fn delete(&self) -> Result<(), ApiProblem> {
        // Held throughout, so a restore in flight can't reinstall a client.
        let mut client = self.ha_client.lock().await;
        self.begin_ha_connect();
        self.ha_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *client = None;
        *self.ha_instance.lock().await = None;
        *self.registry.lock().await = RegistrySnapshot::default();
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher
                .set_sink(Arc::new(NoopActionSink) as Arc<dyn flick_core::ActionSink>)
                .await;
            dispatcher.set_registry(RegistrySnapshot::default()).await;
        }
        drop(client);
        self.publish_ha_status().await;
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
        let registry = self.registry.lock().await.clone();
        validate_safety(&action, &registry)?;
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
impl ConfigGateway for EngineApp {
    async fn settings_changed(&self, settings: SettingsMap) -> Result<(), ApiProblem> {
        persist_settings(&self.store, &settings)
            .map_err(|err| ApiProblem::validation("settings_store_failed", err.to_string()))?;
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher
                .set_settings(dispatcher_settings_from_map(&settings))
                .await;
        }
        Ok(())
    }

    async fn mappings_changed(&self, mappings: Vec<Mapping>) -> Result<(), ApiProblem> {
        persist_mappings(&self.store, &mappings)
            .map_err(|err| ApiProblem::validation("mapping_store_failed", err.to_string()))?;
        let dispatcher_mappings = dispatcher_mappings_from_api(&mappings)
            .map_err(|err| ApiProblem::validation("mapping_reload_failed", err.to_string()))?;
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_mappings(dispatcher_mappings).await;
        }
        Ok(())
    }

    async fn anchors_changed(&self, anchors: Vec<ApiAnchor>) -> Result<(), ApiProblem> {
        persist_anchor_edits(&self.store, &anchors)
            .map_err(|err| ApiProblem::validation("anchor_store_failed", err.to_string()))?;
        let mut dispatcher_anchors =
            dispatcher_anchors_from_api(&anchors, &self.registry.lock().await.clone());
        append_owner_anchor_for_dev(&self.runtime, &mut dispatcher_anchors);
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_anchors(dispatcher_anchors).await;
        }
        self.refresh_selector_anchors().await;
        Ok(())
    }

    async fn activity(&self, query: ActivityQuery) -> Result<ActivityPage, ApiProblem> {
        let rows = self
            .store
            .activity()
            .list(
                query.limit,
                query.before.as_deref(),
                query.status.as_deref(),
                query.include_suppressed,
            )
            .map_err(|err| {
                ApiProblem::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "activity_store_failed",
                    err.to_string(),
                )
            })?;
        let next_before = if rows.len() == query.limit {
            rows.last().map(|row| row.id.clone())
        } else {
            None
        };
        let items = rows
            .into_iter()
            .map(|row| ActivityItem {
                id: row.id,
                ts: rfc3339_from_ms(row.ts_ms),
                camera_id: row.camera_id,
                gesture_id: row.gesture_id,
                confidence: row.confidence,
                mapping_id: row.mapping_id,
                anchor_id: row.anchor_id,
                action_summary: row.action_summary,
                status: row.status,
                reason: row.reason,
                message: row.message,
                latency: row
                    .latency
                    .and_then(|value| serde_json::from_value::<LatencyBreakdown>(value).ok()),
            })
            .collect();
        Ok(ActivityPage { items, next_before })
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
        None
    }
}

/// Builds the hand pipeline and, when its models load, the on-demand face keypoint runner.
fn build_vision(
    model_root: Option<&Path>,
    mirrored: bool,
) -> (HandPipelineImpl, Option<FaceKeypointRunner>) {
    let model_free = || HandPipelineImpl::without_models().with_mirrored_input(mirrored);
    let Some(model_root) = model_root else {
        return (model_free(), None);
    };
    let built = ModelSet::load(model_root).and_then(|models| {
        let face = FaceKeypointRunner::new(&models);
        HandPipelineImpl::new(models, EpChoice::default()).map(|pipeline| (pipeline, face))
    });
    match built {
        Ok((pipeline, face)) => {
            let ep_summary = pipeline
                .ep_summary()
                .into_iter()
                .map(|(model, ep)| format!("{model}={ep:?}"))
                .collect::<Vec<_>>()
                .join(",");
            tracing::info!(
                model_root = %model_root.display(),
                ep_summary,
                "vision models loaded"
            );
            (pipeline.with_mirrored_input(mirrored), Some(face))
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                model_root = %model_root.display(),
                "vision models unavailable; using model-free hand pipeline"
            );
            (model_free(), None)
        }
    }
}

fn model_manifest_path(runtime: &RuntimeConfig) -> Option<PathBuf> {
    let candidates = [
        // TODO: insert the active OTA model pack ahead of the bundled baseline once update
        // activation owns that state.
        runtime.models_dir.clone(),
        Some(PathBuf::from("models")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|path| path.join("manifest.toml").exists() || path.is_file())
}

fn capture_status_parts(status: CaptureStatus) -> (String, Option<String>) {
    match status {
        CaptureStatus::Running => ("running".to_owned(), None),
        CaptureStatus::Disconnected => ("reconnecting".to_owned(), None),
        CaptureStatus::Stopped => ("stopped".to_owned(), None),
        CaptureStatus::Failed(message) => ("error".to_owned(), Some(message)),
    }
}

fn paused_camera_status(camera_id: &str) -> CameraStatus {
    CameraStatus {
        camera_id: camera_id.to_owned(),
        state: "paused".to_owned(),
        fps: Some(0.0),
        error: None,
    }
}

fn camera_error_status(camera_id: &str, err: &anyhow::Error) -> CameraStatus {
    let permission_denied = err.chain().any(|cause| {
        cause
            .downcast_ref::<flick_core::CaptureError>()
            .is_some_and(|capture| matches!(capture, flick_core::CaptureError::PermissionDenied(_)))
    });
    CameraStatus {
        camera_id: camera_id.to_owned(),
        state: if permission_denied {
            "permission_denied"
        } else {
            "error"
        }
        .to_owned(),
        fps: Some(0.0),
        error: Some(err.to_string()),
    }
}

fn permission_status_str(status: CameraPermissionStatus) -> &'static str {
    match status {
        CameraPermissionStatus::NotDetermined => "not_determined",
        CameraPermissionStatus::Restricted => "restricted",
        CameraPermissionStatus::Denied => "denied",
        CameraPermissionStatus::Authorized => "authorized",
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

fn validate_safety(action: &Action, registry: &RegistrySnapshot) -> Result<(), ApiProblem> {
    if !matches!(action, Action::CallService { .. }) {
        return Err(ApiProblem::validation(
            "unsupported_action",
            "Only call_service can be sent directly",
        ));
    }
    let class = classify_action_safety(&SafetyValidator::new(), action, registry);
    if matches!(class, SafetyClass::Denied | SafetyClass::Sensitive) {
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

fn api_anchor_from_record(
    record: &flick_spatial::AnchorRecord,
    camera_id: Option<String>,
) -> ApiAnchor {
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
        camera_id,
        area_override: None,
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

fn client_certificate_dto(identity: Option<&ClientIdentity>) -> HaClientCertificate {
    let Some(identity) = identity else {
        return HaClientCertificate::default();
    };
    let info = identity.info();
    let not_after = OffsetDateTime::from_unix_timestamp(info.not_after).ok();
    HaClientCertificate {
        installed: true,
        subject: Some(info.subject.clone()),
        issuer: Some(info.issuer.clone()),
        not_after: not_after.and_then(|at| at.format(&Rfc3339).ok()),
        sha256: Some(info.sha256.clone()),
        expired: not_after.is_some_and(|at| at <= OffsetDateTime::now_utc()),
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

async fn shutdown_signal(watch_stdin: bool) {
    let stdin_eof = async move {
        if watch_stdin {
            let _ = tokio::task::spawn_blocking(wait_stdin_eof).await;
        } else {
            std::future::pending::<()>().await;
        }

        fn wait_stdin_eof() {
            let mut stdin = std::io::stdin();
            let mut buffer = [0_u8; 128];
            loop {
                match stdin.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        }
    };
    tokio::pin!(stdin_eof);
    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = &mut stdin_eof => {},
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
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = &mut stdin_eof => {},
        }
    }
}
