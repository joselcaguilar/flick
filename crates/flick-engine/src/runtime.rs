//! Runtime wiring for the engine binary and local API.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use async_trait::async_trait;
use axum::Router;
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
use flick_core::{
    Action, ActionOutcome, ActionStatus, ActionTarget, AnchorId, CameraId, DialProperty, PlaceId,
    Verb,
};
use flick_ha::{
    EntityState, HaClient, HaConnectionConfig, HaStatus, RegistrySnapshot, SafetyInput,
    SafetyValidator,
};
use flick_spatial::{
    AnchorGeometry, DEFAULT_ESTIMATOR_VERSION, PlaceRecord, PlaceStatus, PointingRay, RaySource,
    RealignPair, StoredIntrinsics, TeachObservation, TeachSession as SpatialTeachSession,
    TeachTarget, realign,
};
use flick_store::Store;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{net::TcpListener, sync::Mutex};

use crate::{
    config::RuntimeConfig,
    dispatcher::{
        Dispatcher, HaActionSink, NoopActionSink, owner_fan_anchor, owner_scenario_mappings,
    },
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
    let api_config = api_config(&runtime, actual_port, token);

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
    let state = ApiState::new(api_config, gateways);
    let events = state.events();
    let dispatcher = Dispatcher::builder(sink)
        .store(store)
        .events(events)
        .mappings(owner_scenario_mappings())
        .anchors(vec![owner_fan_anchor()])
        .build();
    app.set_dispatcher(dispatcher).await;

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

    let app_router: Router = router(state);
    axum::serve(listener, app_router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("API server failed")
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
    let (url, token, _handle) = flick_ha::mock::MockHa::start(scenario).await?;
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
    ha_instance: Mutex<Option<HaInstance>>,
    registry: Mutex<RegistrySnapshot>,
    targeting: SqliteTargetingStore,
    teach_sessions: Mutex<HashMap<String, EngineTeachSession>>,
    realign_sessions: Mutex<HashMap<String, EngineRealignSession>>,
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
            ha_instance: Mutex::new(None),
            registry: Mutex::new(RegistrySnapshot::default()),
            targeting: SqliteTargetingStore::new(Arc::clone(&store)),
            teach_sessions: Mutex::new(HashMap::new()),
            realign_sessions: Mutex::new(HashMap::new()),
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

#[async_trait]
impl EngineControl for EngineApp {
    async fn status(&self) -> EngineStatus {
        let paused = *self.paused.lock().await;
        let _ = self.started_at.elapsed();
        let _ = self.store.db_path();
        let _ = self.dispatcher.lock().await.is_some();
        EngineStatus {
            paused,
            paused_until: None,
            cameras: vec![CameraStatus {
                camera_id: "dev-camera".to_owned(),
                state: if self.runtime.fake_camera.is_some()
                    || self.runtime.fake_landmarks.is_some()
                {
                    "running"
                } else {
                    "idle"
                }
                .to_owned(),
                fps: Some(30.0),
                error: None,
            }],
            ha: self.ha_status_dto().await,
            stages: vec![
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
            ],
            camera_permission: Some("not_requested".to_owned()),
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
        vec![AvailableCamera {
            device_ref: self.runtime.fake_camera.as_ref().map_or_else(
                || "dev-camera".to_owned(),
                |path| path.display().to_string(),
            ),
            name: if self.runtime.fake_camera.is_some() {
                "Fake camera"
            } else {
                "Dev camera"
            }
            .to_owned(),
            kind: if self.runtime.fake_camera.is_some() {
                "file"
            } else {
                "local"
            }
            .to_owned(),
            formats: vec![CameraFormat {
                width: 1280,
                height: 720,
                fps: 30,
                format: "rgb".to_owned(),
            }],
        }]
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
