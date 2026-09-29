#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use flick_core::{Action, ActionStatus, ActionTarget, DialProperty, Verb};
use flick_ha::{
    DialController, DialEvent, DialTarget, EntityState, FanLevelPlan, HaClient, HaConnectionConfig,
    HaStatus, SafetyInput, SafetyValidator, VerbTarget,
    dial::{DialCallSink, sent_outcome},
    map_ha_error,
    mock::{MockHa, MockScenario},
    next_fan_level, parse_compressed_entities, previous_fan_level,
};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

fn call(domain: &str, service: &str, entity_id: &str, data: Value) -> Action {
    Action::CallService {
        domain: domain.to_owned(),
        service: service.to_owned(),
        target: ActionTarget {
            entity_id: Some(vec![entity_id.to_owned()]),
            device_id: None,
            area_id: None,
        },
        data,
        preset: None,
    }
}

async fn wait_for_status(client: &HaClient, predicate: impl Fn(&HaStatus) -> bool) -> HaStatus {
    let mut rx = client.status();
    tokio::time::timeout(Duration::from_secs(2), async move {
        loop {
            let current = rx.borrow().clone();
            if predicate(&current) {
                return current;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}

fn entity(entity_id: &str, state: &str, attrs: Value) -> EntityState {
    EntityState {
        entity_id: entity_id.to_owned(),
        state: state.to_owned(),
        attributes: attrs.as_object().cloned().unwrap_or_else(Map::new),
        last_changed: None,
        last_updated: None,
    }
}

#[tokio::test]
async fn handshake_ok_and_invalid_token() {
    let (url, token, _handle) = MockHa::start(MockScenario::default()).await.unwrap();
    let client = HaClient::connect(HaConnectionConfig::new(&url, token).unwrap())
        .await
        .unwrap();
    let ready = client.wait_for_auth().await.unwrap();
    assert!(matches!(ready, HaStatus::Ready { .. }));

    let bad = HaClient::connect(HaConnectionConfig::new(&url, "bad-token").unwrap())
        .await
        .unwrap();
    let failed = bad.wait_for_auth().await.unwrap();
    assert!(matches!(failed, HaStatus::AuthFailed { .. }));
}

#[tokio::test(start_paused = true)]
async fn keepalive_pongs_prevent_timeout_and_drop_stops_client_task() {
    let (url, token, handle) = MockHa::start(MockScenario::default()).await.unwrap();
    let mut config = HaConnectionConfig::new(&url, token).unwrap();
    config.ping_interval = Duration::from_millis(100);
    config.ping_timeout = Duration::from_millis(250);
    config.reconnect_initial = Duration::from_secs(10);
    config.reconnect_max = Duration::from_secs(10);
    let client = HaClient::connect(config).await.unwrap();
    assert!(matches!(
        client.wait_for_auth().await.unwrap(),
        HaStatus::Ready { .. }
    ));

    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(matches!(*client.status().borrow(), HaStatus::Ready { .. }));

    drop(client);
    for _ in 0..20 {
        if handle.active_connections().await == 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(handle.active_connections().await, 0);
}

#[tokio::test]
async fn reconnect_resubscribes_and_drops_stale_action() {
    let scenario = MockScenario::from_path(format!(
        "{}/tests/support/scenarios/bedroom_fan.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (url, token, handle) = MockHa::start(scenario).await.unwrap();
    let mut config = HaConnectionConfig::new(&url, token).unwrap();
    config.stale_action = Duration::ZERO;
    config.reconnect_initial = Duration::from_millis(100);
    config.reconnect_max = Duration::from_millis(100);
    let client = HaClient::connect(config).await.unwrap();
    wait_for_status(&client, |status| matches!(status, HaStatus::Ready { .. })).await;
    let _sub = client
        .subscribe_entity("fan.ventilador_dormitorio")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    handle.disconnect_clients();
    wait_for_status(&client, |status| {
        matches!(
            status,
            HaStatus::Reconnecting { .. } | HaStatus::Disconnected
        )
    })
    .await;

    let outcome = client
        .call(call(
            "fan",
            "turn_on",
            "fan.ventilador_dormitorio",
            json!({ "percentage": 1 }),
        ))
        .await;
    assert_eq!(outcome.status, ActionStatus::Stale);

    wait_for_status(&client, |status| matches!(status, HaStatus::Ready { .. })).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(handle.subscription_count("fan.ventilador_dormitorio").await >= 2);
}

#[test]
fn ha_error_codes_map_to_action_outcomes() {
    let rows = [
        (
            Some("not_found"),
            "error.not_found",
            "Service or device not found — check the mapping",
        ),
        (Some("invalid_format"), "error.invalid", "bad json"),
        (
            Some("service_validation_error"),
            "error.invalid",
            "bad service",
        ),
        (
            Some("unauthorized"),
            "error.unauthorized",
            "Flick's HA user can't do this",
        ),
        (Some("home_assistant_error"), "error.ha", "boom"),
        (
            Some("other"),
            "error.other",
            "Home Assistant didn't respond",
        ),
        (None, "error.unknown", "Home Assistant didn't respond"),
    ];
    for (code, expected_code, expected_message) in rows {
        let message = match code {
            Some("invalid_format") => Some("bad json"),
            Some("service_validation_error") => Some("bad service"),
            Some("home_assistant_error") => Some("boom"),
            _ => None,
        };
        let (mapped_code, mapped_message) = map_ha_error(code, message);
        assert_eq!(mapped_code, expected_code);
        assert_eq!(mapped_message, expected_message);
    }
}

#[derive(Default)]
struct RecordingSink {
    calls: Mutex<Vec<Action>>,
}

#[async_trait]
impl DialCallSink for RecordingSink {
    async fn call_action(&self, action: Action) -> flick_core::ActionOutcome {
        self.calls.lock().await.push(action);
        sent_outcome()
    }
}

#[tokio::test(start_paused = true)]
async fn dial_coalesces_to_eight_hz_and_sends_final_value() {
    let sink = Arc::new(RecordingSink::default());
    let controller = DialController::start(
        DialTarget {
            entity_id: "fan.ventilador_dormitorio".to_owned(),
            property: DialProperty::Percentage,
            base_value: Some(1.0),
            gain: 1.0,
            min: Some(1.0),
            max: Some(100.0),
        },
        sink.clone(),
    );
    controller.send(DialEvent::Update(1.0)).await.unwrap();
    controller.send(DialEvent::Update(2.0)).await.unwrap();
    controller.send(DialEvent::Update(3.0)).await.unwrap();
    tokio::time::advance(Duration::from_millis(124)).await;
    tokio::task::yield_now().await;
    assert!(sink.calls.lock().await.len() <= 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    controller.send(DialEvent::Update(4.0)).await.unwrap();
    controller.send(DialEvent::End(5.0)).await.unwrap();
    tokio::task::yield_now().await;
    let calls = sink.calls.lock().await.clone();
    assert!(calls.len() <= 3);
    let Some(Action::CallService { data, .. }) = calls.last() else {
        panic!("missing call");
    };
    assert_eq!(data.get("percentage"), Some(&json!(6.0)));
}

#[test]
fn compressed_entity_parser_applies_add_change_and_remove() {
    let sample: Value = serde_json::from_str(include_str!(
        "support/fixtures/subscribe_entities_sample.json"
    ))
    .unwrap();
    let mut states = HashMap::from([(
        "sensor.removed".to_owned(),
        entity("sensor.removed", "on", json!({"friendly_name":"Removed"})),
    )]);
    let deltas = parse_compressed_entities(&sample, &mut states);
    assert_eq!(deltas.len(), 3);
    let fan = states.get("fan.ventilador_dormitorio").unwrap();
    assert_eq!(fan.state, "on");
    assert_eq!(fan.attributes.get("percentage"), Some(&json!(2)));
    assert!(!states.contains_key("sensor.removed"));
}

#[test]
fn safety_validator_classifies_denied_sensitive_and_normal() {
    let validator = SafetyValidator::new();
    let rows = [
        (
            SafetyInput {
                domain: "homeassistant",
                service: "restart",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Denied,
        ),
        (
            SafetyInput {
                domain: "hassio",
                service: "host_reboot",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Denied,
        ),
        (
            SafetyInput {
                domain: "recorder",
                service: "purge",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Denied,
        ),
        (
            SafetyInput {
                domain: "lock",
                service: "unlock",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Sensitive,
        ),
        (
            SafetyInput {
                domain: "alarm_control_panel",
                service: "alarm_arm_home",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Sensitive,
        ),
        (
            SafetyInput {
                domain: "cover",
                service: "open_cover",
                entity_domain: None,
                device_class: Some("garage"),
            },
            flick_ha::SafetyClass::Sensitive,
        ),
        (
            SafetyInput {
                domain: "cover",
                service: "open_cover",
                entity_domain: None,
                device_class: Some("shade"),
            },
            flick_ha::SafetyClass::Normal,
        ),
        (
            SafetyInput {
                domain: "valve",
                service: "open_valve",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Sensitive,
        ),
        (
            SafetyInput {
                domain: "script",
                service: "turn_on",
                entity_domain: None,
                device_class: None,
            },
            flick_ha::SafetyClass::Normal,
        ),
    ];
    for (input, expected) in rows {
        assert_eq!(validator.classify(&input), expected);
    }
}

#[test]
fn verb_resolution_and_fan_levels_match_the_spec_table() {
    let owner = VerbTarget::new(
        entity(
            "fan.ventilador_dormitorio",
            "off",
            json!({"friendly_name":"Ventilador dormitorio","supported_features":53,"percentage_step":1.0,"percentage":1,"direction":"reverse"}),
        ),
        vec![1.0],
    );
    let circle = flick_ha::verb::resolve_verb(&owner, Verb::LevelSet, Some(1)).unwrap();
    assert_eq!(
        circle.action,
        call(
            "fan",
            "turn_on",
            "fan.ventilador_dormitorio",
            json!({"percentage":1})
        )
    );
    let stop = flick_ha::verb::resolve_verb(&owner, Verb::Stop, None).unwrap();
    assert_eq!(
        stop.action,
        call("fan", "turn_off", "fan.ventilador_dormitorio", json!({}))
    );

    let three_speed_max = VerbTarget::new(
        entity(
            "fan.three",
            "on",
            json!({"supported_features":53,"percentage_step":33.33,"percentage":100}),
        ),
        vec![],
    );
    assert_eq!(
        next_fan_level(&three_speed_max),
        FanLevelPlan::AlreadyAtLimit
    );

    let three_speed_low = VerbTarget::new(
        entity(
            "fan.three",
            "on",
            json!({"supported_features":53,"percentage_step":33.33,"percentage":10}),
        ),
        vec![],
    );
    assert_eq!(previous_fan_level(&three_speed_low), FanLevelPlan::TurnOff);
    let down = flick_ha::verb::resolve_verb(&three_speed_low, Verb::Down, None).unwrap();
    assert_eq!(down.action, call("fan", "turn_off", "fan.three", json!({})));

    let unknown = VerbTarget::new(
        entity(
            "fan.unknown",
            "unknown",
            json!({"supported_features":53,"percentage_step":33.33}),
        ),
        vec![],
    );
    let up = flick_ha::verb::resolve_verb(&unknown, Verb::Up, None).unwrap();
    assert_eq!(
        up.action,
        call("fan", "increase_speed", "fan.unknown", json!({}))
    );

    let rows = [
        (
            VerbTarget::new(
                entity("light.kitchen", "on", json!({"brightness":128})),
                vec![50.0],
            ),
            Verb::Up,
            "light",
            "turn_on",
            json!({"brightness_step_pct":20}),
        ),
        (
            VerbTarget::new(
                entity("media_player.tv", "paused", json!({"volume_level":0.3})),
                vec![],
            ),
            Verb::Stop,
            "media_player",
            "media_pause",
            json!({}),
        ),
        (
            VerbTarget::new(
                entity("cover.shade", "open", json!({"device_class":"shade"})),
                vec![],
            ),
            Verb::Down,
            "cover",
            "close_cover",
            json!({}),
        ),
        (
            VerbTarget::new(
                entity(
                    "climate.room",
                    "heat",
                    json!({"temperature":20.0,"target_temp_step":0.5}),
                ),
                vec![],
            ),
            Verb::Up,
            "climate",
            "set_temperature",
            json!({"temperature":20.5}),
        ),
        (
            VerbTarget::new(entity("switch.plug", "off", json!({})), vec![]),
            Verb::Toggle,
            "switch",
            "toggle",
            json!({}),
        ),
    ];
    for (target, verb, domain, service, data) in rows {
        let resolved = flick_ha::verb::resolve_verb(&target, verb, None).unwrap();
        assert_eq!(
            resolved.action,
            call(domain, service, &target.entity.entity_id, data)
        );
    }
}

#[tokio::test]
async fn bedroom_fan_scenario_runs_end_to_end_through_client() {
    let scenario = MockScenario::from_path(format!(
        "{}/tests/support/scenarios/bedroom_fan.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (url, token, handle) = MockHa::start(scenario).await.unwrap();
    let client = HaClient::connect(HaConnectionConfig::new(&url, token).unwrap())
        .await
        .unwrap();
    wait_for_status(&client, |status| matches!(status, HaStatus::Ready { .. })).await;

    let fan = handle.entity("fan.ventilador_dormitorio").await.unwrap();
    let target = VerbTarget::new(fan, vec![1.0]);
    let circle = flick_ha::verb::resolve_verb(&target, Verb::LevelSet, Some(1)).unwrap();
    let outcome = client.call(circle.action).await;
    assert_eq!(outcome.status, ActionStatus::Ok);
    let fan = handle.entity("fan.ventilador_dormitorio").await.unwrap();
    assert_eq!(fan.state, "on");
    assert_eq!(fan.attributes.get("percentage"), Some(&json!(1)));

    let target = VerbTarget::new(fan, vec![1.0]);
    let stop = flick_ha::verb::resolve_verb(&target, Verb::Stop, None).unwrap();
    let outcome = client.call(stop.action).await;
    assert_eq!(outcome.status, ActionStatus::Ok);
    assert_eq!(
        handle
            .entity("fan.ventilador_dormitorio")
            .await
            .unwrap()
            .state,
        "off"
    );

    let calls = handle.calls().await;
    assert_eq!(calls[0].domain, "fan");
    assert_eq!(calls[0].service, "turn_on");
    assert_eq!(calls[0].service_data, json!({"percentage":1}));
    assert_eq!(calls[1].service, "turn_off");
}

#[tokio::test]
async fn wss_url_does_not_crash_client_task() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            drop(stream);
        }
    });
    let client = HaClient::connect(
        HaConnectionConfig::new(format!("https://127.0.0.1:{port}"), "token").unwrap(),
    )
    .await
    .unwrap();
    wait_for_status(&client, |status| {
        matches!(status, HaStatus::Reconnecting { .. })
    })
    .await;
    let err = client.request(json!({ "type": "ping" })).await.unwrap_err();
    assert!(
        !matches!(err, flick_ha::HaError::ClientStopped),
        "client task died: {err}"
    );
}

#[tokio::test]
async fn unreachable_home_url_falls_back_to_remote() {
    let (url, token, _handle) = MockHa::start(MockScenario::default()).await.unwrap();
    let (_network_tx, network_rx) = tokio::sync::watch::channel(Some("Home".to_owned()));
    let mut config = HaConnectionConfig::new(url, token)
        .unwrap()
        .with_internal_url(Some("http://127.0.0.1:1"))
        .unwrap();
    config.trusted_ssids = vec!["Home".to_owned()];
    config.network = Some(network_rx);
    let client = HaClient::connect(config).await.unwrap();
    wait_for_status(&client, |status| matches!(status, HaStatus::Ready { .. })).await;
    let route = client.route().borrow().clone().unwrap();
    assert!(!route.internal);
    assert!(route.url.starts_with("http://"));
}
