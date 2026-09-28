//! Reusable mock Home Assistant WebSocket server.

use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, broadcast},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::{EntityState, HaError, ServiceCallRecord};

/// Scenario file consumed by the mock server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockScenario {
    /// Accepted token.
    #[serde(default = "default_token")]
    pub token: String,
    /// HA version advertised during auth.
    #[serde(default = "default_version")]
    pub ha_version: String,
    /// `get_config` payload.
    #[serde(default)]
    pub config: Value,
    /// Initial entity states.
    #[serde(default)]
    pub entities: Vec<EntityState>,
    /// `get_services` payload.
    #[serde(default)]
    pub services: Value,
    /// Registry payloads.
    #[serde(default)]
    pub registries: MockRegistries,
    /// Artificial call delay.
    #[serde(default)]
    pub call_delay_ms: u64,
    /// Per-service errors.
    #[serde(default)]
    pub call_errors: Vec<MockCallError>,
}

impl Default for MockScenario {
    fn default() -> Self {
        Self {
            token: default_token(),
            ha_version: default_version(),
            config: json!({ "location_name": "Mock Home", "version": default_version(), "uuid": "mock-ha" }),
            entities: Vec::new(),
            services: json!({}),
            registries: MockRegistries::default(),
            call_delay_ms: 0,
            call_errors: Vec::new(),
        }
    }
}

impl MockScenario {
    /// Reads a scenario JSON file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, HaError> {
        let text =
            std::fs::read_to_string(path).map_err(|err| HaError::Protocol(err.to_string()))?;
        serde_json::from_str(&text).map_err(HaError::Json)
    }
}

/// Registry data returned by mock HA.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MockRegistries {
    /// Area registry list.
    #[serde(default)]
    pub areas: Value,
    /// Floor registry list.
    #[serde(default)]
    pub floors: Value,
    /// Label registry list.
    #[serde(default)]
    pub labels: Value,
    /// Device registry list.
    #[serde(default)]
    pub devices: Value,
    /// Compressed `entity_registry/list_for_display` payload.
    #[serde(default)]
    pub entities: Value,
}

/// Configured service error for a mock scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockCallError {
    /// Domain to match.
    pub domain: String,
    /// Service to match.
    pub service: String,
    /// HA error code.
    pub code: String,
    /// HA error message.
    pub message: String,
}

/// Mock server namespace.
pub struct MockHa;

impl MockHa {
    /// Starts a mock HA server on an ephemeral port.
    pub async fn start(scenario: MockScenario) -> Result<(String, String, MockHaHandle), HaError> {
        Self::start_on_port(scenario, 0).await
    }

    /// Starts a mock HA server on a requested port (`0` means ephemeral).
    pub async fn start_on_port(
        scenario: MockScenario,
        port: u16,
    ) -> Result<(String, String, MockHaHandle), HaError> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|err| HaError::WebSocket(err.to_string()))?;
        let addr = listener
            .local_addr()
            .map_err(|err| HaError::WebSocket(err.to_string()))?;
        let url = format!("ws://{addr}/api/websocket");
        let token = scenario.token.clone();
        let state = Arc::new(Mutex::new(MockState::new(scenario)));
        let (disconnect_tx, _) = broadcast::channel(16);
        let handle = MockHaHandle {
            state: Arc::clone(&state),
            disconnect_tx: disconnect_tx.clone(),
        };
        tokio::spawn(accept_loop(listener, state, disconnect_tx));
        Ok((url, token, handle))
    }
}

/// Handle used by tests and examples to inspect or manipulate the mock.
#[derive(Clone)]
pub struct MockHaHandle {
    state: Arc<Mutex<MockState>>,
    disconnect_tx: broadcast::Sender<()>,
}

impl MockHaHandle {
    /// Returns recorded service calls.
    pub async fn calls(&self) -> Vec<ServiceCallRecord> {
        self.state.lock().await.calls.clone()
    }

    /// Returns the current state for an entity.
    pub async fn entity(&self, entity_id: &str) -> Option<EntityState> {
        self.state.lock().await.entities.get(entity_id).cloned()
    }

    /// Counts how many subscribe requests have been created for an entity.
    pub async fn subscription_count(&self, entity_id: &str) -> usize {
        self.state
            .lock()
            .await
            .subscription_counts
            .get(entity_id)
            .copied()
            .unwrap_or(0)
    }

    /// Forces all connected clients to disconnect.
    pub fn disconnect_clients(&self) {
        let _ = self.disconnect_tx.send(());
    }
}

struct MockState {
    scenario: MockScenario,
    entities: HashMap<String, EntityState>,
    calls: Vec<ServiceCallRecord>,
    subscription_counts: HashMap<String, usize>,
}

impl MockState {
    fn new(scenario: MockScenario) -> Self {
        let entities = scenario
            .entities
            .iter()
            .map(|entity| (entity.entity_id.clone(), entity.clone()))
            .collect();
        Self {
            scenario,
            entities,
            calls: Vec::new(),
            subscription_counts: HashMap::new(),
        }
    }
}

async fn accept_loop(
    listener: TcpListener,
    state: Arc<Mutex<MockState>>,
    disconnect_tx: broadcast::Sender<()>,
) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            break;
        };
        let state = Arc::clone(&state);
        let disconnect_rx = disconnect_tx.subscribe();
        tokio::spawn(async move {
            let _ = handle_client(stream, state, disconnect_rx).await;
        });
    }
}

async fn handle_client(
    stream: TcpStream,
    state: Arc<Mutex<MockState>>,
    mut disconnect_rx: broadcast::Receiver<()>,
) -> Result<(), HaError> {
    let mut ws = accept_async(stream)
        .await
        .map_err(|err| HaError::WebSocket(err.to_string()))?;
    let (ha_version, token) = {
        let state = state.lock().await;
        (
            state.scenario.ha_version.clone(),
            state.scenario.token.clone(),
        )
    };
    send_json(
        &mut ws,
        json!({ "type": "auth_required", "ha_version": ha_version }),
    )
    .await?;

    let Some(Ok(Message::Text(text))) = ws.next().await else {
        return Err(HaError::Disconnected);
    };
    let auth: Value = serde_json::from_str(&text)?;
    if auth.get("type").and_then(Value::as_str) != Some("auth")
        || auth.get("access_token").and_then(Value::as_str) != Some(token.as_str())
    {
        send_json(
            &mut ws,
            json!({ "type": "auth_invalid", "message": "Invalid password" }),
        )
        .await?;
        let _ = ws.close(None).await;
        return Ok(());
    }
    send_json(
        &mut ws,
        json!({ "type": "auth_ok", "ha_version": ha_version }),
    )
    .await?;

    let mut subscriptions: HashMap<u64, Vec<String>> = HashMap::new();
    loop {
        tokio::select! {
            _ = disconnect_rx.recv() => {
                let _ = ws.close(None).await;
                return Ok(());
            }
            message = ws.next() => {
                let Some(message) = message else { return Ok(()); };
                match message {
                    Ok(Message::Text(text)) => handle_command(&mut ws, &state, &mut subscriptions, &text).await?,
                    Ok(Message::Close(_)) => return Ok(()),
                    Ok(Message::Binary(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                    Err(err) => return Err(HaError::WebSocket(err.to_string())),
                }
            }
        }
    }
}

async fn handle_command(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    state: &Arc<Mutex<MockState>>,
    subscriptions: &mut HashMap<u64, Vec<String>>,
    text: &str,
) -> Result<(), HaError> {
    let value: Value = serde_json::from_str(text)?;
    let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "supported_features" | "ping" | "unsubscribe_events" => {
            send_result(ws, id, true, Value::Null).await
        }
        "get_config" => {
            let result = state.lock().await.scenario.config.clone();
            send_result(ws, id, true, result).await
        }
        "get_states" => {
            let result = Value::Array(
                state
                    .lock()
                    .await
                    .entities
                    .values()
                    .cloned()
                    .map(|entity| serde_json::to_value(entity).unwrap_or(Value::Null))
                    .collect(),
            );
            send_result(ws, id, true, result).await
        }
        "get_services" => {
            let result = state.lock().await.scenario.services.clone();
            send_result(ws, id, true, result).await
        }
        "config/area_registry/list" => {
            let result = state.lock().await.scenario.registries.areas.clone();
            send_result(ws, id, true, result).await
        }
        "config/floor_registry/list" => {
            let result = state.lock().await.scenario.registries.floors.clone();
            send_result(ws, id, true, result).await
        }
        "config/label_registry/list" => {
            let result = state.lock().await.scenario.registries.labels.clone();
            send_result(ws, id, true, result).await
        }
        "config/device_registry/list" => {
            let result = state.lock().await.scenario.registries.devices.clone();
            send_result(ws, id, true, result).await
        }
        "config/entity_registry/list_for_display" => {
            let result = state.lock().await.scenario.registries.entities.clone();
            send_result(ws, id, true, result).await
        }
        "subscribe_entities" => {
            let entity_ids = value
                .get("entity_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            {
                let mut locked = state.lock().await;
                for entity_id in &entity_ids {
                    *locked
                        .subscription_counts
                        .entry(entity_id.clone())
                        .or_insert(0) += 1;
                }
            }
            subscriptions.insert(id, entity_ids.clone());
            send_result(ws, id, true, Value::Null).await?;
            send_subscription_snapshot(ws, state, id, &entity_ids).await
        }
        "call_service" => handle_call_service(ws, state, subscriptions, id, &value).await,
        other => {
            send_error(
                ws,
                id,
                "unknown_command",
                &format!("unknown command {other}"),
            )
            .await
        }
    }
}

async fn handle_call_service(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    state: &Arc<Mutex<MockState>>,
    subscriptions: &HashMap<u64, Vec<String>>,
    id: u64,
    value: &Value,
) -> Result<(), HaError> {
    let domain = value
        .get("domain")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let service = value
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let target = value.get("target").cloned().unwrap_or_else(|| json!({}));
    let service_data = value
        .get("service_data")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let delay = state.lock().await.scenario.call_delay_ms;
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    if let Some(error) = matching_error(state, &domain, &service).await {
        return send_error(ws, id, &error.code, &error.message).await;
    }

    let changed = {
        let mut locked = state.lock().await;
        locked.calls.push(ServiceCallRecord {
            domain: domain.clone(),
            service: service.clone(),
            target: target.clone(),
            service_data: service_data.clone(),
        });
        update_state(
            &mut locked.entities,
            &domain,
            &service,
            &target,
            &service_data,
        )
    };

    send_result(ws, id, true, json!({ "context": { "id": format!("mock-{id}"), "parent_id": null, "user_id": "mock" }, "response": null })).await?;
    for entity in changed {
        send_entity_change(ws, subscriptions, &entity).await?;
    }
    Ok(())
}

async fn matching_error(
    state: &Arc<Mutex<MockState>>,
    domain: &str,
    service: &str,
) -> Option<MockCallError> {
    state
        .lock()
        .await
        .scenario
        .call_errors
        .iter()
        .find(|error| error.domain == domain && error.service == service)
        .cloned()
}

fn update_state(
    entities: &mut HashMap<String, EntityState>,
    domain: &str,
    service: &str,
    target: &Value,
    service_data: &Value,
) -> Vec<EntityState> {
    let ids = target_entity_ids(target);
    let mut changed = Vec::new();
    for entity_id in ids {
        if let Some(entity) = entities.get_mut(&entity_id) {
            match (domain, service) {
                ("fan", "turn_on") => {
                    entity.state = "on".to_owned();
                    if let Some(percentage) = service_data.get("percentage").cloned() {
                        entity
                            .attributes
                            .insert("percentage".to_owned(), percentage);
                    }
                    changed.push(entity.clone());
                }
                ("fan", "turn_off") => {
                    entity.state = "off".to_owned();
                    changed.push(entity.clone());
                }
                ("light", "toggle") => {
                    entity.state = if entity.state == "on" { "off" } else { "on" }.to_owned();
                    changed.push(entity.clone());
                }
                ("light", "turn_on") | ("switch", "turn_on") | ("input_boolean", "turn_on") => {
                    entity.state = "on".to_owned();
                    changed.push(entity.clone());
                }
                ("light", "turn_off") | ("switch", "turn_off") | ("input_boolean", "turn_off") => {
                    entity.state = "off".to_owned();
                    changed.push(entity.clone());
                }
                _ => {}
            }
        }
    }
    changed
}

fn target_entity_ids(target: &Value) -> Vec<String> {
    let Some(value) = target.get("entity_id") else {
        return Vec::new();
    };
    if let Some(id) = value.as_str() {
        return vec![id.to_owned()];
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

async fn send_subscription_snapshot(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    state: &Arc<Mutex<MockState>>,
    id: u64,
    entity_ids: &[String],
) -> Result<(), HaError> {
    let locked = state.lock().await;
    let mut adds = Map::new();
    for entity_id in entity_ids {
        if let Some(entity) = locked.entities.get(entity_id) {
            adds.insert(entity_id.clone(), compressed(entity));
        }
    }
    send_json(
        ws,
        json!({ "id": id, "type": "event", "event": { "a": adds } }),
    )
    .await
}

async fn send_entity_change(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    subscriptions: &HashMap<u64, Vec<String>>,
    entity: &EntityState,
) -> Result<(), HaError> {
    for (id, entities) in subscriptions {
        if entities
            .iter()
            .any(|entity_id| entity_id == &entity.entity_id)
        {
            let mut per_entity = Map::new();
            per_entity.insert(entity.entity_id.clone(), json!({ "+": compressed(entity) }));
            let mut changes = Map::new();
            changes.insert("c".to_owned(), Value::Object(per_entity));
            send_json(
                ws,
                json!({ "id": id, "type": "event", "event": Value::Object(changes) }),
            )
            .await?;
        }
    }
    Ok(())
}

fn compressed(entity: &EntityState) -> Value {
    json!({ "s": entity.state, "a": entity.attributes, "lc": entity.last_changed, "lu": entity.last_updated })
}

async fn send_result(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    id: u64,
    success: bool,
    result: Value,
) -> Result<(), HaError> {
    send_json(
        ws,
        json!({ "id": id, "type": "result", "success": success, "result": result }),
    )
    .await
}

async fn send_error(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    id: u64,
    code: &str,
    message: &str,
) -> Result<(), HaError> {
    send_json(ws, json!({ "id": id, "type": "result", "success": false, "error": { "code": code, "message": message } })).await
}

async fn send_json(
    ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    value: Value,
) -> Result<(), HaError> {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .map_err(|err| HaError::WebSocket(err.to_string()))
}

fn default_token() -> String {
    "mock-token".to_owned()
}

fn default_version() -> String {
    "2026.9.0".to_owned()
}
