use std::{
    collections::HashMap,
    path::Path,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use flick_core::{
    AnchorId, CameraId, FaceKeypoints, GestureEvent, GestureEventId, GestureId, GesturePhase,
    HandFrame, HandObservation, Handedness, SelectionState, StageTimings, SuppressionReason,
};
use flick_engine::{
    dispatcher::{
        Dispatcher, DispatcherAnchor, HaActionSink, owner_fan_anchor, owner_scenario_mappings,
        owner_scenario_mappings_for,
    },
    fake_landmarks::replay_once,
    targeting_store::{SqliteTargetingStore, anchor_record_to_anchor, anchor_to_record},
};
use flick_gestures::{GestureEngine, GestureEngineConfig, read_jsonl_str};
use flick_ha::{
    EntityState, HaClient, HaConnectionConfig,
    mock::{MockHa, MockRegistries, MockScenario},
};
use flick_spatial::{
    Anchor, CameraIntrinsics, IntrinsicsSource, PlaceRecord, PlaceStatus, StoredIntrinsics,
    TargetSelectorImpl, TargetSelectorSettings, TargetingStore, TeachTarget,
};
use flick_store::Store;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use smallvec::SmallVec;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/fixtures");

#[tokio::test]
async fn replay_owner_scenario_dispatches_mock_ha() -> anyhow::Result<()> {
    let fixture = load_anchor_fixture("targeting/bedroom_fan.anchors.json")?;
    let anchor_id = fixture.fan_anchor_id()?;
    let dispatcher_anchors = fixture.dispatcher_anchors()?;
    let (client, mock) = mock_client(&fixture.entity_state("fan.ventilador_dormitorio")?).await?;
    client.refresh_registry().await?;

    let store = Arc::new(Store::open_memory()?);
    seed_targeting_store(Arc::clone(&store), &fixture)?;
    let dispatcher = Dispatcher::builder(Arc::new(HaActionSink::new(client)))
        .store(Arc::clone(&store))
        .mappings(owner_scenario_mappings_for(anchor_id))
        .anchors(dispatcher_anchors)
        .build();

    let idle = SelectionState::Idle;
    let selected = SelectionState::Selected {
        anchor_id,
        domain: "fan".to_owned(),
        expires_at_ms: 4_102_444_800_000,
    };

    dispatch_fixture(&dispatcher, "landmarks/thumb_up.jsonl", &idle).await?;
    assert_calls(&mock, 1, "light", "toggle", "light.bed_light", json!({})).await;

    let no_target =
        dispatch_fixture(&dispatcher, "landmarks/owner_fan_circle.jsonl", &idle).await?;
    assert!(no_target.contains(&SuppressionReason::NoTarget));
    assert_eq!(mock.calls().await.len(), 1);

    dispatch_fixture(&dispatcher, "landmarks/owner_fan_circle.jsonl", &selected).await?;
    assert_calls(
        &mock,
        2,
        "fan",
        "turn_on",
        "fan.ventilador_dormitorio",
        json!({"percentage": 1}),
    )
    .await;

    dispatch_fixture(&dispatcher, "landmarks/owner_fan_stop.jsonl", &selected).await?;
    assert_calls(
        &mock,
        3,
        "fan",
        "turn_off",
        "fan.ventilador_dormitorio",
        json!({}),
    )
    .await;

    let target_selected =
        dispatch_fixture(&dispatcher, "landmarks/thumb_up.jsonl", &selected).await?;
    assert!(target_selected.contains(&SuppressionReason::TargetSelected));
    assert_eq!(mock.calls().await.len(), 3);

    let conn = store.connection();
    let activity_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM activity_log", [], |row| row.get(0))?;
    assert!(activity_count >= 5);
    let latency: String = conn.query_row(
        "SELECT latency FROM activity_log WHERE status = 'ok' ORDER BY ts DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    let latency_json: Value = serde_json::from_str(&latency)?;
    assert!(latency_json.get("detect_ms").is_some());
    assert!(latency_json.get("dispatch_ms").is_some());
    assert!(latency_json.get("ha_ms").is_some());

    Ok(())
}

#[tokio::test]
async fn replay_targeting_fixtures_select_and_dispatch() -> anyhow::Result<()> {
    replay_targeting_fixture("point_fan_circle").await?;
    replay_targeting_fixture("point_fan_stop").await?;
    replay_targeting_fixture("two_anchors_25deg").await?;
    Ok(())
}

#[tokio::test]
async fn fake_landmark_replay_owner_fan_circle_dispatches_mock_ha() -> anyhow::Result<()> {
    let fan = owner_fan_anchor().entity;
    let (client, mock) = mock_client(&fan).await?;
    client.refresh_registry().await?;
    let dispatcher = Dispatcher::builder(Arc::new(HaActionSink::new(client)))
        .mappings(owner_scenario_mappings())
        .anchors(vec![owner_fan_anchor()])
        .build();
    let stats = replay_once(
        "landmarks/owner_fan_circle".to_owned(),
        Path::new(FIXTURES).join("landmarks/owner_fan_circle.jsonl"),
        dispatcher,
    )
    .await?;
    assert!(stats.frames > 0);
    assert_calls(
        &mock,
        1,
        "fan",
        "turn_on",
        "fan.ventilador_dormitorio",
        json!({"percentage": 1}),
    )
    .await;
    Ok(())
}

async fn replay_targeting_fixture(stem: &str) -> anyhow::Result<()> {
    let anchors = load_anchor_fixture("targeting/bedroom_fan.anchors.json")?;
    let fan = anchors.entity_state("fan.ventilador_dormitorio")?;
    let (client, mock) = mock_client(&fan).await?;
    client.refresh_registry().await?;

    let dispatcher = Dispatcher::builder(Arc::new(HaActionSink::new(client)))
        .mappings(owner_scenario_mappings_for(anchors.fan_anchor_id()?))
        .anchors(anchors.dispatcher_anchors()?)
        .build();

    let expected = load_targeting_expected(&format!("targeting/{stem}.expected.json"))?;
    let records = load_targeting_records(&format!("targeting/{stem}.jsonl"))?;
    let first = records
        .first()
        .ok_or_else(|| anyhow::anyhow!("empty targeting fixture {stem}"))?;
    let mut selector = TargetSelectorImpl::new(
        first.intrinsics.to_intrinsics(),
        anchors.anchors.clone(),
        TargetSelectorSettings::default(),
    );
    let base = Instant::now() + Duration::from_millis(10);
    let mut selected_after_initial = false;
    let mut selected_anchor = None;

    for record in records {
        let frame = record.to_frame(base)?;
        let selection = selector.update(&frame, record.face.as_ref());
        if let SelectionState::Selected { anchor_id, .. } = selection {
            selected_anchor = Some(anchor_id);
            if Some(anchor_id) == expected.expected_selected_anchor_id {
                selected_after_initial = true;
            }
            if selected_after_initial
                && expected
                    .forbidden_selected_anchor_ids_after_initial_selection
                    .contains(&anchor_id)
            {
                anyhow::bail!("selector locked onto forbidden anchor {anchor_id}");
            }
        }

        if let Some(gesture) = record.gesture {
            let target = match selection {
                SelectionState::Selected { anchor_id, .. } => Some(anchor_id),
                SelectionState::Hover { anchor_id, .. } => Some(anchor_id),
                SelectionState::Idle | SelectionState::Aiming => None,
            };
            let event = gesture.to_event(frame.camera_id, target, frame.captured_at)?;
            dispatcher.dispatch(&event).await;
        }
    }

    if let Some(expected_anchor) = expected.expected_selected_anchor_id {
        assert_eq!(selected_anchor, Some(expected_anchor));
    }
    let calls = mock.calls().await;
    assert_eq!(calls.len(), expected.expected_actions.len());
    for (call, expected) in calls.iter().zip(expected.expected_actions.iter()) {
        assert_eq!(call.domain, expected.domain);
        assert_eq!(call.service, expected.service);
        assert_eq!(call.target, expected.target);
        assert_eq!(call.service_data, expected.data);
    }
    Ok(())
}

async fn dispatch_fixture(
    dispatcher: &Dispatcher,
    fixture: &str,
    selection: &SelectionState,
) -> anyhow::Result<Vec<SuppressionReason>> {
    let mut reasons = Vec::new();
    for event in replay_events(fixture, selection)? {
        let report = dispatcher.dispatch(&event).await;
        reasons.extend(report.suppressions.into_iter().map(|item| item.reason));
    }
    Ok(reasons)
}

fn replay_events(fixture: &str, selection: &SelectionState) -> anyhow::Result<Vec<GestureEvent>> {
    let path = Path::new(FIXTURES).join(fixture);
    let content = std::fs::read_to_string(&path)?;
    let records = read_jsonl_str(&content)?;
    let base = Instant::now() + Duration::from_millis(10);
    let mut engine = GestureEngine::new(GestureEngineConfig::default());
    let mut events = Vec::new();
    for record in records {
        let frame = record.into_frame(base);
        let update = engine.update(&frame, selection);
        events.extend(update.events);
    }
    Ok(events)
}

fn load_anchor_fixture(fixture: &str) -> anyhow::Result<AnchorFixture> {
    let path = Path::new(FIXTURES).join(fixture);
    let content = std::fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(Into::into)
}

fn load_targeting_records(fixture: &str) -> anyhow::Result<Vec<TargetingFrameRecord>> {
    let path = Path::new(FIXTURES).join(fixture);
    let content = std::fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn load_targeting_expected(fixture: &str) -> anyhow::Result<TargetingExpected> {
    let path = Path::new(FIXTURES).join(fixture);
    let content = std::fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(Into::into)
}

fn seed_targeting_store(store: Arc<Store>, fixture: &AnchorFixture) -> anyhow::Result<()> {
    let mut targeting = SqliteTargetingStore::new(store);
    let place = fixture.place_record()?;
    targeting.try_upsert_place(&place)?;
    for anchor in &fixture.anchors {
        targeting.upsert_anchor(anchor_to_record(
            place.id,
            anchor,
            false,
            false,
            1_700_000_000_000,
        ))?;
    }
    let stored = targeting.anchors_for_place(place.id);
    assert_eq!(stored.len(), fixture.anchors.len());
    assert!(stored.iter().filter_map(anchor_record_to_anchor).count() >= 1);
    Ok(())
}

async fn mock_client(
    entity: &EntityState,
) -> anyhow::Result<(HaClient, flick_ha::mock::MockHaHandle)> {
    let scenario = MockScenario {
        token: "mock-token".to_owned(),
        ha_version: "2026.9.0".to_owned(),
        config: json!({"location_name":"Mock Home","version":"2026.9.0","uuid":"mock-ha"}),
        entities: vec![
            EntityState {
                entity_id: "light.bed_light".to_owned(),
                state: "off".to_owned(),
                attributes: Map::from_iter([
                    ("friendly_name".to_owned(), json!("Bed light")),
                    ("supported_features".to_owned(), json!(0)),
                ]),
                last_changed: None,
                last_updated: None,
            },
            entity.clone(),
        ],
        services: json!({
            "light": {"toggle": {}, "turn_on": {}, "turn_off": {}},
            "fan": {"turn_on": {}, "turn_off": {}, "toggle": {}}
        }),
        registries: MockRegistries::default(),
        call_delay_ms: 0,
        call_errors: Vec::new(),
    };
    let (url, token, handle) = MockHa::start(scenario).await?;
    let client = HaClient::connect(HaConnectionConfig::new(url, token)?).await?;
    Ok((client, handle))
}

async fn assert_calls(
    mock: &flick_ha::mock::MockHaHandle,
    len: usize,
    domain: &str,
    service: &str,
    entity_id: &str,
    service_data: Value,
) {
    let calls = mock.calls().await;
    assert_eq!(calls.len(), len);
    let call = &calls[len - 1];
    assert_eq!(call.domain, domain);
    assert_eq!(call.service, service);
    assert_eq!(call.target, json!({"entity_id": [entity_id]}));
    assert_eq!(call.service_data, service_data);
}

#[derive(Debug, Deserialize)]
struct AnchorFixture {
    camera_id: String,
    place: PlaceSeed,
    anchors: Vec<Anchor>,
    entity_states: Vec<EntityStateSeed>,
}

impl AnchorFixture {
    fn fan_anchor_id(&self) -> anyhow::Result<AnchorId> {
        self.anchors
            .iter()
            .find(|anchor| anchor.domain == "fan")
            .map(|anchor| anchor.id)
            .ok_or_else(|| anyhow::anyhow!("fan anchor missing"))
    }

    fn entity_state(&self, entity_id: &str) -> anyhow::Result<EntityState> {
        self.entity_states
            .iter()
            .find(|entity| entity.entity_id == entity_id)
            .map(EntityStateSeed::to_entity_state)
            .ok_or_else(|| anyhow::anyhow!("entity state missing for {entity_id}"))
    }

    fn dispatcher_anchors(&self) -> anyhow::Result<Vec<DispatcherAnchor>> {
        let states = self
            .entity_states
            .iter()
            .map(|state| (state.entity_id.clone(), state.to_entity_state()))
            .collect::<HashMap<_, _>>();
        self.anchors
            .iter()
            .filter_map(|anchor| match &anchor.target {
                TeachTarget::Entity(entity_id) => Some((anchor, entity_id)),
                TeachTarget::Device(_) | TeachTarget::Area(_) => None,
            })
            .map(|(anchor, entity_id)| {
                let entity = states
                    .get(entity_id)
                    .cloned()
                    .unwrap_or_else(|| default_entity_state(entity_id, &anchor.name));
                Ok(DispatcherAnchor::from_spatial(anchor, entity))
            })
            .collect()
    }

    fn place_record(&self) -> anyhow::Result<PlaceRecord> {
        let intrinsics = CameraIntrinsics::sane_default(1280, 720);
        Ok(PlaceRecord {
            id: self.place.id()?,
            camera_id: CameraId::from_str(&self.camera_id)?,
            name: self.place.name.clone(),
            scene_signature: self.place.scene_signature,
            embedder_version: self.place.embedder_version.clone(),
            intrinsics: StoredIntrinsics::from(&intrinsics),
            status: self.place.status,
            active: self.place.active,
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_000,
        })
    }
}

fn default_entity_state(entity_id: &str, name: &str) -> EntityState {
    EntityState {
        entity_id: entity_id.to_owned(),
        state: "off".to_owned(),
        attributes: Map::from_iter([
            ("friendly_name".to_owned(), json!(name)),
            ("supported_features".to_owned(), json!(0)),
        ]),
        last_changed: None,
        last_updated: None,
    }
}

#[derive(Debug, Deserialize)]
struct PlaceSeed {
    id: String,
    name: String,
    #[serde(with = "signature_serde")]
    scene_signature: [f32; 384],
    embedder_version: String,
    status: PlaceStatus,
    active: bool,
}

impl PlaceSeed {
    fn id(&self) -> anyhow::Result<flick_core::PlaceId> {
        flick_core::PlaceId::from_str(&self.id).map_err(Into::into)
    }
}

#[derive(Debug, Deserialize)]
struct EntityStateSeed {
    entity_id: String,
    state: String,
    #[serde(flatten)]
    attributes: Map<String, Value>,
}

impl EntityStateSeed {
    fn to_entity_state(&self) -> EntityState {
        let mut attributes = self.attributes.clone();
        if attributes.get("area_id").is_some_and(Value::is_null) {
            attributes.remove("area_id");
        }
        EntityState {
            entity_id: self.entity_id.clone(),
            state: self.state.clone(),
            attributes,
            last_changed: None,
            last_updated: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TargetingFrameRecord {
    camera_id: String,
    seq: u64,
    t_ms: u64,
    intrinsics: FixtureIntrinsics,
    face: Option<FaceKeypoints>,
    #[serde(default)]
    hands: Vec<HandObservation>,
    #[serde(default)]
    gesture: Option<FixtureGesture>,
    #[serde(default)]
    timings: StageTimings,
}

impl TargetingFrameRecord {
    fn to_frame(&self, base: Instant) -> anyhow::Result<HandFrame> {
        Ok(HandFrame {
            camera_id: CameraId::from_str(&self.camera_id)?,
            seq: self.seq,
            captured_at: base + Duration::from_millis(self.t_ms),
            hands: SmallVec::from_vec(self.hands.clone()),
            timings: self.timings,
        })
    }
}

#[derive(Debug, Deserialize)]
struct FixtureIntrinsics {
    width: u32,
    height: u32,
    hfov_deg: f32,
    source: IntrinsicsSource,
    version: String,
}

impl FixtureIntrinsics {
    fn to_intrinsics(&self) -> CameraIntrinsics {
        CameraIntrinsics::from_horizontal_fov(
            self.width,
            self.height,
            self.hfov_deg,
            self.source.clone(),
            self.version.clone(),
        )
    }
}

#[derive(Debug, Deserialize)]
struct FixtureGesture {
    id: String,
    phase: GesturePhase,
    target_anchor_id: Option<String>,
}

impl FixtureGesture {
    fn to_event(
        &self,
        camera_id: CameraId,
        selected_target: Option<AnchorId>,
        at: Instant,
    ) -> anyhow::Result<GestureEvent> {
        let target = selected_target.or_else(|| {
            self.target_anchor_id
                .as_deref()
                .and_then(|id| AnchorId::from_str(id).ok())
        });
        Ok(GestureEvent {
            id: GestureEventId::new(),
            camera_id,
            gesture_id: GestureId::from_str(&self.id)?,
            hand: Handedness::Right,
            confidence: 1.0,
            phase: self.phase,
            value: None,
            target,
            onset_at: at,
            fired_at: at,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct TargetingExpected {
    #[serde(default)]
    expected_selected_anchor_id: Option<AnchorId>,
    #[serde(default)]
    forbidden_selected_anchor_ids_after_initial_selection: Vec<AnchorId>,
    #[serde(default)]
    expected_actions: Vec<ExpectedAction>,
}

#[derive(Debug, Deserialize)]
struct ExpectedAction {
    domain: String,
    service: String,
    target: Value,
    data: Value,
}

mod signature_serde {
    use serde::{Deserialize, de};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[f32; 384], D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let values = Vec::<f32>::deserialize(deserializer)?;
        if values.len() != 384 {
            return Err(de::Error::invalid_length(
                values.len(),
                &"384 float scene-signature values",
            ));
        }
        let mut out = [0.0_f32; 384];
        out.copy_from_slice(&values);
        Ok(out)
    }
}
