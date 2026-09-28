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
    ActionDto, ActionOutcomeDto, ActionTargetDto, Anchor as ApiAnchor, ApiConfig, ApiGateways,
    ApiProblem, ApiState, AvailableCamera, CameraFormat, CameraStatus, ConfigGateway,
    EngineControl, EngineStatus, FakeUpdates, HaArea, HaConnectRequest, HaDiscovery, HaEntity,
    HaGateway, HaInstance, HaServiceSchema, HaStatus as ApiHaStatus, LatencyBreakdown, Mapping,
    PauseRequest, PreviewFrame, PreviewSource, RealignCommitResponse, RealignPointRequest,
    RealignPointResponse, RealignSession, SettingsMap, SetupSuggestRequest, SetupSuggestion,
    StageLatency, TargetModeDto, TeachCommitRequest, TeachCommitResponse, TeachGateway,
    TeachLevelRequest, TeachLevelResponse, TeachRequest, TeachSession as ApiTeachSession,
    TeachSpotResponse, VerbBinding, router,
};
use flick_capture::{
    CameraPermissionStatus, CaptureHandle, CaptureStatus, FileSource, FileSourceOptions,
    LatestFrameSlot, LocalCameraOptions, LocalCameraSource, camera_permission_status,
    spawn_capture,
};
use flick_core::{
    Action, ActionOutcome, ActionStatus, ActionTarget, AnchorId, CameraId, DialProperty, Frame,
    FrameSource, GestureId, HandFrame, HandPipeline, MappingId, PlaceId, SourceInfo, SourceKind,
    Verb,
};
use flick_gestures::{GestureEngine, GestureEngineConfig};
use flick_ha::{
    EntityState, HaClient, HaConnectionConfig, HaStatus, KeyringSecretStore, RegistrySnapshot,
    SafetyClass, SafetyValidator, SecretStore, ServiceCallRecord, record_current_fan_level,
};
use flick_spatial::{
    AnchorGeometry, CameraIntrinsics, DEFAULT_ESTIMATOR_VERSION, PlaceRecord, PlaceStatus,
    PointingRay, RaySource, RealignPair, StoredIntrinsics, TargetSelectorImpl,
    TargetSelectorSettings, TeachObservation, TeachSession as SpatialTeachSession, TeachTarget,
    realign,
};
use flick_store::Store;
use flick_vision::{EpChoice, HandPipelineImpl, ModelSet};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{net::TcpListener, sync::Mutex};

use crate::{
    config::RuntimeConfig,
    dispatcher::{
        Dispatcher, DispatcherAnchor, DispatcherMapping, DispatcherSettings, HaActionSink,
        MappingHand, MappingTarget, NoopActionSink, classify_action_safety, owner_fan_anchor,
        owner_scenario_mappings,
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
    if runtime.mock_ha {
        start_mock_ha(&app).await?;
    } else {
        app.restore_ha_connection().await;
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
    state.replace_settings(settings_map);
    state.replace_mappings(api_mappings);
    state.replace_anchors(api_anchors);
    let events = state.events();
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
        }),
        snapshot,
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

fn dispatcher_settings_from_map(settings: &SettingsMap) -> DispatcherSettings {
    let mut dispatcher = DispatcherSettings::default();
    if let Some(value) = settings
        .get("safety.allow_sensitive")
        .and_then(serde_json::Value::as_bool)
    {
        dispatcher.allow_sensitive_actions = value;
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
    Ok(targeting
        .try_all_anchors()
        .map_err(|err| anyhow::anyhow!("{err}"))?
        .iter()
        .map(api_anchor_from_record)
        .collect())
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
    let status = tokio::time::timeout(Duration::from_secs(8), client.wait_for_auth())
        .await
        .context("timed out waiting for Home Assistant authentication")??;
    match status {
        HaStatus::Ready { ha_version } => Ok(ha_version.unwrap_or_default()),
        HaStatus::AuthFailed { message } => {
            anyhow::bail!("Home Assistant authentication failed: {message}");
        }
        HaStatus::Disconnected | HaStatus::Connecting | HaStatus::Reconnecting { .. } => {
            anyhow::bail!("Home Assistant authentication did not complete")
        }
    }
}

fn load_default_ha_instance(store: &Store) -> anyhow::Result<Option<(HaInstance, String)>> {
    let conn = store.connection();
    conn.query_row(
        "SELECT id, name, base_url, ha_uuid, auth_kind, keychain_ref, ha_version, is_default, \
         created_at, updated_at FROM ha_instances ORDER BY is_default DESC, updated_at DESC LIMIT 1",
        [],
        |row| {
            let created_at: i64 = row.get(8)?;
            let updated_at: i64 = row.get(9)?;
            Ok((
                HaInstance {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    base_url: row.get(2)?,
                    ha_uuid: row.get(3)?,
                    auth_kind: row.get(4)?,
                    ha_version: row.get(6)?,
                    is_default: row.get::<_, i64>(7)? != 0,
                    created_at: rfc3339_from_ms(created_at),
                    updated_at: rfc3339_from_ms(updated_at),
                },
                row.get(5)?,
            ))
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
    let conn = store.connection();
    conn.execute("UPDATE ha_instances SET is_default = 0", [])?;
    conn.execute(
        "INSERT INTO ha_instances \
         (id, name, base_url, ha_uuid, auth_kind, keychain_ref, cert_sha256, ha_version, is_default, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, base_url = excluded.base_url, \
         ha_uuid = excluded.ha_uuid, auth_kind = excluded.auth_kind, keychain_ref = excluded.keychain_ref, \
         cert_sha256 = excluded.cert_sha256, ha_version = excluded.ha_version, is_default = 1, updated_at = excluded.updated_at",
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
        ],
    )?;
    Ok(())
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
            "UPDATE anchors SET name = ?1, verb_params = ?2, sensitive = ?3, sensitive_ack = ?4, status = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                anchor.name,
                serde_json::to_string(&anchor.verb_params)?,
                if anchor.sensitive { 1 } else { 0 },
                if anchor.sensitive_ack { 1 } else { 0 },
                anchor.status,
                now,
                anchor.id,
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
        if let Some(client) = self.ha_client.lock().await.clone() {
            dispatcher
                .set_sink(Arc::new(HaActionSink::new(client)) as Arc<dyn flick_core::ActionSink>)
                .await;
        }
        dispatcher
            .set_registry(self.registry.lock().await.clone())
            .await;
        dispatcher.set_paused(*self.paused.lock().await).await;
        *self.dispatcher.lock().await = Some(dispatcher);
    }

    async fn set_ha_client(
        &self,
        client: HaClient,
        instance: Option<HaInstance>,
        snapshot: RegistrySnapshot,
    ) {
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher
                .set_sink(
                    Arc::new(HaActionSink::new(client.clone())) as Arc<dyn flick_core::ActionSink>
                )
                .await;
            dispatcher.set_registry(snapshot.clone()).await;
        }
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

    async fn restore_ha_connection(&self) {
        match load_default_ha_instance(&self.store) {
            Ok(Some((instance, keychain_ref))) => {
                let secrets = KeyringSecretStore::new();
                match secrets.get(&keychain_ref) {
                    Ok(Some(token)) => {
                        if let Err(err) = self.connect_saved_ha(instance, token).await {
                            tracing::warn!(error = %err, "failed to restore Home Assistant connection");
                        }
                    }
                    Ok(None) => {
                        tracing::warn!("stored Home Assistant token is missing from keychain")
                    }
                    Err(err) => tracing::warn!(error = %err, "failed to read Home Assistant token"),
                }
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(error = %err, "failed to load Home Assistant instance"),
        }
    }

    async fn connect_saved_ha(&self, instance: HaInstance, token: String) -> anyhow::Result<()> {
        let config = HaConnectionConfig::new(instance.base_url.clone(), token)?;
        let client = HaClient::connect(config).await?;
        wait_ha_ready(&client).await?;
        let snapshot = client.refresh_registry().await?;
        self.set_ha_client(client, Some(instance), snapshot).await;
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
        Ok(())
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
        let model_root = model_manifest_path(&self.runtime);
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
    dispatch_task: Option<tokio::task::JoinHandle<()>>,
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
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let dispatch_task = dispatcher.map(|dispatcher| {
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    let _ = dispatcher.dispatch(&event).await;
                }
            })
        });
        let worker = {
            let worker_stop = Arc::clone(&worker_stop);
            let latest_frame = Arc::clone(&latest_frame);
            let latest_hands = Arc::clone(&latest_hands);
            let last_error = Arc::clone(&last_error);
            let source_info = source_info.clone();
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
                                for event in gestures.update(&hands, &selection).events {
                                    let _ = event_tx.send(event);
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
            dispatch_task,
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
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_paused(true).await;
        }
        EngineControl::status(self).await
    }

    async fn resume(&self) -> EngineStatus {
        *self.paused.lock().await = false;
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher.set_paused(false).await;
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
        self.reload_dispatcher_anchors_from_store().await?;
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
            });
            return Ok(instance);
        }
        let token = request.token.clone();
        let cert_sha256 = request.trust_cert_sha256.clone();
        let config = HaConnectionConfig::new(request.base_url.clone(), token.clone())
            .map_err(|err| ApiProblem::validation("ha_invalid_url", err.to_string()))?;
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
        *self.registry.lock().await = RegistrySnapshot::default();
        if let Some(dispatcher) = self.dispatcher.lock().await.clone() {
            dispatcher
                .set_sink(Arc::new(NoopActionSink) as Arc<dyn flick_core::ActionSink>)
                .await;
            dispatcher.set_registry(RegistrySnapshot::default()).await;
        }
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
        Ok(())
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
        Ok(pipeline) => {
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
            pipeline.with_mirrored_input(mirrored)
        }
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
