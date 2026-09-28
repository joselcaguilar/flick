use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use flick_core::{GestureEvent, SelectionState, SuppressionReason};
use flick_engine::dispatcher::{AnchorSeed, Dispatcher, HaActionSink, owner_scenario_mappings};
use flick_gestures::{GestureEngine, GestureEngineConfig, read_jsonl_str};
use flick_ha::{
    HaClient, HaConnectionConfig,
    mock::{MockHa, MockRegistries, MockScenario},
};
use flick_store::Store;
use serde_json::json;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/fixtures");

#[tokio::test]
async fn replay_owner_scenario_dispatches_mock_ha() -> anyhow::Result<()> {
    let anchors = load_anchors("targeting/bedroom_fan.anchors.json")?;
    let anchor_id = anchors[0].id;
    let (client, mock) = mock_client(&anchors[0].entity).await?;
    client.refresh_registry().await?;

    let store = Arc::new(Store::open_memory()?);
    let dispatcher = Dispatcher::builder(Arc::new(HaActionSink::new(client)))
        .store(Arc::clone(&store))
        .mappings(owner_scenario_mappings())
        .anchors(anchors)
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
    assert!(
        no_target
            .iter()
            .any(|reason| *reason == SuppressionReason::NoTarget)
    );
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
    assert!(
        target_selected
            .iter()
            .any(|reason| *reason == SuppressionReason::TargetSelected)
    );
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
    let latency_json: serde_json::Value = serde_json::from_str(&latency)?;
    assert!(latency_json.get("detect_ms").is_some());
    assert!(latency_json.get("dispatch_ms").is_some());
    assert!(latency_json.get("ha_ms").is_some());

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

fn load_anchors(fixture: &str) -> anyhow::Result<Vec<flick_engine::dispatcher::DispatcherAnchor>> {
    let path = Path::new(FIXTURES).join(fixture);
    let content = std::fs::read_to_string(path)?;
    let seeds: Vec<AnchorSeed> = serde_json::from_str(&content)?;
    seeds.into_iter().map(TryInto::try_into).collect()
}

async fn mock_client(
    entity: &flick_ha::EntityState,
) -> anyhow::Result<(HaClient, flick_ha::mock::MockHaHandle)> {
    let scenario = MockScenario {
        token: "mock-token".to_owned(),
        ha_version: "2026.9.0".to_owned(),
        config: json!({"location_name":"Mock Home","version":"2026.9.0","uuid":"mock-ha"}),
        entities: vec![
            flick_ha::EntityState {
                entity_id: "light.bed_light".to_owned(),
                state: "off".to_owned(),
                attributes: serde_json::Map::from_iter([
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
    service_data: serde_json::Value,
) {
    let calls = mock.calls().await;
    assert_eq!(calls.len(), len);
    let call = &calls[len - 1];
    assert_eq!(call.domain, domain);
    assert_eq!(call.service, service);
    assert_eq!(call.target, json!({"entity_id": [entity_id]}));
    assert_eq!(call.service_data, service_data);
}
