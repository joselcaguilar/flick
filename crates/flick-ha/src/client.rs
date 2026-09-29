//! Reconnecting Home Assistant WebSocket client.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use flick_core::{Action, ActionOutcome, ActionStatus};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config, tungstenite::Message,
};

use crate::{
    HaError,
    error::{error_outcome, stale_outcome, timeout_outcome},
    protocol::{context_id, parse_compressed_entities, result_error},
    registry::{RegistryCache, RegistrySnapshot, build_snapshot},
    types::{EntityState, HaConnectionConfig, HaEvent, HaStatus},
    verb::{VerbResolution, VerbResolutionError, VerbTarget, resolve_verb},
};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Cheap cloneable handle to a reconnecting HA WebSocket task.
#[derive(Clone)]
pub struct HaClient {
    tx: mpsc::Sender<Command>,
    status_rx: watch::Receiver<HaStatus>,
    events_tx: broadcast::Sender<HaEvent>,
    registry: Arc<RwLock<RegistryCache>>,
}

impl HaClient {
    /// Connects to Home Assistant and starts the background reconnect loop.
    pub async fn connect(config: HaConnectionConfig) -> Result<Self, HaError> {
        let (tx, rx) = mpsc::channel(128);
        let (status_tx, status_rx) = watch::channel(HaStatus::Disconnected);
        let (events_tx, _) = broadcast::channel(256);
        let registry = Arc::new(RwLock::new(RegistryCache::default()));
        let actor_registry = Arc::clone(&registry);
        let actor_events = events_tx.clone();
        let actor = tokio::spawn(async move {
            run_actor(config, rx, status_tx, actor_events, actor_registry).await;
        });
        tokio::spawn(async move {
            if let Err(err) = actor.await
                && err.is_panic()
            {
                tracing::error!(error = %err, "home assistant client task panicked");
            }
        });
        Ok(Self {
            tx,
            status_rx,
            events_tx,
            registry,
        })
    }

    /// Returns a watch receiver for connection status.
    #[must_use]
    pub fn status(&self) -> watch::Receiver<HaStatus> {
        self.status_rx.clone()
    }

    /// Waits until the initial authentication attempt reaches `ready` or
    /// `auth_failed`.
    pub async fn wait_for_auth(&self) -> Result<HaStatus, HaError> {
        let mut rx = self.status();
        loop {
            let status = rx.borrow().clone();
            if matches!(status, HaStatus::Ready { .. } | HaStatus::AuthFailed { .. }) {
                return Ok(status);
            }
            rx.changed().await.map_err(|_| HaError::ClientStopped)?;
        }
    }

    /// Subscribes to HA client events.
    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<HaEvent> {
        self.events_tx.subscribe()
    }

    /// Sends a concrete `call_service` action and awaits the HA result.
    pub async fn call(&self, action: Action) -> ActionOutcome {
        let (respond, rx) = oneshot::channel();
        let command = Command::Call {
            action,
            created_at: Instant::now(),
            respond,
        };
        if self.tx.send(command).await.is_err() {
            return ActionOutcome {
                activity_id: None,
                status: ActionStatus::Error,
                error_code: Some("error.disconnected".to_owned()),
                message: Some("Home Assistant client stopped".to_owned()),
                ha_context_id: None,
                latency_ms: None,
            };
        }
        rx.await.unwrap_or_else(|_| ActionOutcome {
            activity_id: None,
            status: ActionStatus::Error,
            error_code: Some("error.disconnected".to_owned()),
            message: Some("Home Assistant client stopped".to_owned()),
            ha_context_id: None,
            latency_ms: None,
        })
    }

    /// Sends a raw HA command and returns its `result` payload.
    pub async fn request(&self, payload: Value) -> Result<Value, HaError> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(Command::Request { payload, respond })
            .await
            .map_err(|_| HaError::ClientStopped)?;
        rx.await.map_err(|_| HaError::ChannelClosed)?
    }

    /// Reference-counts a `subscribe_entities` subscription for one entity.
    pub async fn subscribe_entity(
        &self,
        entity_id: impl Into<String>,
    ) -> Result<EntitySubscription, HaError> {
        let entity_id = entity_id.into();
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(Command::Subscribe {
                entity_id: entity_id.clone(),
                respond,
            })
            .await
            .map_err(|_| HaError::ClientStopped)?;
        rx.await.map_err(|_| HaError::ChannelClosed)??;
        Ok(EntitySubscription {
            entity_id,
            tx: self.tx.clone(),
        })
    }

    /// Refreshes states, services and registry metadata.
    pub async fn refresh_registry(&self) -> Result<RegistrySnapshot, HaError> {
        let states_value = self.request(json!({ "type": "get_states" })).await?;
        let states: Vec<EntityState> = serde_json::from_value(states_value)?;
        let services = self.request(json!({ "type": "get_services" })).await?;
        let areas = self
            .request(json!({ "type": "config/area_registry/list" }))
            .await
            .unwrap_or_else(|_| json!([]));
        let devices = self
            .request(json!({ "type": "config/device_registry/list" }))
            .await
            .unwrap_or_else(|_| json!([]));
        let entities = self
            .request(json!({ "type": "config/entity_registry/list_for_display" }))
            .await
            .unwrap_or_else(|_| json!([]));
        let snapshot = build_snapshot(states, services, areas, devices, entities);
        self.registry.write().await.replace(snapshot.clone());
        Ok(snapshot)
    }

    /// Returns the current cached registry snapshot.
    pub async fn registry(&self) -> RegistrySnapshot {
        self.registry.read().await.snapshot()
    }

    /// Resolves a targeted verb against a selected entity.
    pub fn resolve_verb(
        &self,
        target: &VerbTarget,
        verb: flick_core::Verb,
        level: Option<u32>,
    ) -> Result<VerbResolution, VerbResolutionError> {
        let _ = self;
        resolve_verb(target, verb, level)
    }
}

/// RAII guard for one entity subscription reference.
pub struct EntitySubscription {
    entity_id: String,
    tx: mpsc::Sender<Command>,
}

impl Drop for EntitySubscription {
    fn drop(&mut self) {
        let _ = self.tx.try_send(Command::Unsubscribe {
            entity_id: self.entity_id.clone(),
        });
    }
}

enum Command {
    Call {
        action: Action,
        created_at: Instant,
        respond: oneshot::Sender<ActionOutcome>,
    },
    Request {
        payload: Value,
        respond: oneshot::Sender<Result<Value, HaError>>,
    },
    Subscribe {
        entity_id: String,
        respond: oneshot::Sender<Result<(), HaError>>,
    },
    Unsubscribe {
        entity_id: String,
    },
}

struct QueuedCall {
    action: Action,
    created_at: Instant,
    respond: oneshot::Sender<ActionOutcome>,
}

struct SubscriptionEntry {
    refs: u32,
    subscription_id: Option<u64>,
}

enum PendingKind {
    Call {
        started: Instant,
        respond: oneshot::Sender<ActionOutcome>,
    },
    Request {
        respond: oneshot::Sender<Result<Value, HaError>>,
    },
    Subscribe {
        entity_id: String,
        respond: Option<oneshot::Sender<Result<(), HaError>>>,
    },
    Unsubscribe,
    Ping,
}

struct Pending {
    deadline: Instant,
    kind: PendingKind,
}

struct SessionState {
    next_id: u64,
    pending: HashMap<u64, Pending>,
    subscriptions: HashMap<String, SubscriptionEntry>,
    entity_states: HashMap<String, EntityState>,
}

async fn run_actor(
    config: HaConnectionConfig,
    mut rx: mpsc::Receiver<Command>,
    status_tx: watch::Sender<HaStatus>,
    events_tx: broadcast::Sender<HaEvent>,
    registry: Arc<RwLock<RegistryCache>>,
) {
    let mut attempt = 0_u32;
    let mut queued_calls = VecDeque::new();
    let mut subscriptions = HashMap::new();

    loop {
        set_status(&status_tx, &events_tx, HaStatus::Connecting);
        let last_error: Option<String>;
        match establish(&config).await {
            Ok((ws, ha_version, next_id)) => {
                attempt = 0;
                set_status(
                    &status_tx,
                    &events_tx,
                    HaStatus::Ready {
                        ha_version: Some(ha_version),
                    },
                );
                let mut state = SessionState {
                    next_id,
                    pending: HashMap::new(),
                    subscriptions,
                    entity_states: HashMap::new(),
                };
                let disconnected = run_connected(
                    ws,
                    &config,
                    &mut rx,
                    &events_tx,
                    &registry,
                    &mut queued_calls,
                    &mut state,
                )
                .await;
                subscriptions = state.subscriptions;
                last_error = Some("Connection to Home Assistant was lost".to_owned());
                fail_pending(&mut state.pending, HaError::Disconnected);
                set_status(&status_tx, &events_tx, HaStatus::Disconnected);
                if matches!(disconnected, DisconnectReason::Shutdown) {
                    break;
                }
            }
            Err(HaError::AuthInvalid(message)) => {
                set_status(&status_tx, &events_tx, HaStatus::AuthFailed { message });
                drain_auth_failed(&mut rx).await;
                break;
            }
            Err(err) => {
                tracing::warn!(url = %config.url, error = %err, "home assistant connect failed");
                last_error = Some(describe_connect_error(&err));
                set_status(&status_tx, &events_tx, HaStatus::Disconnected);
            }
        }

        attempt = attempt.saturating_add(1);
        drop_stale(&mut queued_calls, config.stale_action);
        let delay = backoff_delay(&config, attempt);
        set_status(
            &status_tx,
            &events_tx,
            HaStatus::Reconnecting {
                attempt,
                last_error: last_error.clone(),
            },
        );
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                () = &mut sleep => break,
                command = rx.recv() => {
                    let Some(command) = command else {
                        return;
                    };
                    handle_disconnected_command(command, &mut queued_calls, &mut subscriptions, config.stale_action);
                }
            }
            drop_stale(&mut queued_calls, config.stale_action);
        }
    }
}

/// Short, user-facing reason for a failed connection attempt.
fn describe_connect_error(err: &HaError) -> String {
    let HaError::WebSocket(detail) = err else {
        return err.to_string();
    };
    let lower = detail.to_ascii_lowercase();
    let reason = if lower.contains("timed out") {
        "the server didn't respond in time"
    } else if lower.contains("connection refused") {
        "the connection was refused (check the port)"
    } else if lower.contains("failed to lookup") || lower.contains("nodename nor servname") {
        "the host name couldn't be resolved"
    } else if lower.contains("certificate") || lower.contains("unknownissuer") {
        "its TLS certificate isn't trusted by this Mac"
    } else if lower.contains("tls") || lower.contains("handshake") {
        "the secure (TLS) connection failed"
    } else if lower.contains("404") || lower.contains("http error") {
        "that address isn't a Home Assistant server"
    } else if lower.contains("unreachable") || lower.contains("no route") {
        "the network is unreachable"
    } else {
        return detail.clone();
    };
    reason.to_owned()
}

async fn establish(config: &HaConnectionConfig) -> Result<(Ws, String, u64), HaError> {
    let connector = if config.url.starts_with("wss://") {
        Connector::Rustls(crate::tls::client_config(config.cert_sha256.as_deref())?)
    } else {
        Connector::Plain
    };
    let connect = connect_async_tls_with_config(&config.url, None, false, Some(connector));
    let (mut ws, _) = tokio::time::timeout(config.request_timeout, connect)
        .await
        .map_err(|_| HaError::WebSocket("connection timed out".to_owned()))?
        .map_err(|err| HaError::WebSocket(err.to_string()))?;

    let auth_required = read_json(&mut ws, config.request_timeout).await?;
    if auth_required.get("type").and_then(Value::as_str) != Some("auth_required") {
        return Err(HaError::Protocol("expected auth_required".to_owned()));
    }
    ws.send(Message::Text(
        json!({ "type": "auth", "access_token": config.token })
            .to_string()
            .into(),
    ))
    .await
    .map_err(|err| HaError::WebSocket(err.to_string()))?;
    let auth = read_json(&mut ws, config.request_timeout).await?;
    match auth.get("type").and_then(Value::as_str) {
        Some("auth_ok") => {
            let ha_version = auth
                .get("ha_version")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            ws.send(Message::Text(json!({ "id": 1_u64, "type": "supported_features", "features": { "coalesce_messages": 1 } }).to_string().into()))
                .await
                .map_err(|err| HaError::WebSocket(err.to_string()))?;
            let result = read_json(&mut ws, config.request_timeout).await?;
            if result.get("id").and_then(Value::as_u64) != Some(1)
                || result.get("success").and_then(Value::as_bool) != Some(true)
            {
                return Err(HaError::Protocol("supported_features failed".to_owned()));
            }
            Ok((ws, ha_version, 2))
        }
        Some("auth_invalid") => Err(HaError::AuthInvalid(
            auth.get("message")
                .and_then(Value::as_str)
                .unwrap_or("invalid token")
                .to_owned(),
        )),
        other => Err(HaError::Protocol(format!(
            "unexpected auth message {other:?}"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectReason {
    Socket,
    Shutdown,
}

async fn run_connected(
    mut ws: Ws,
    config: &HaConnectionConfig,
    rx: &mut mpsc::Receiver<Command>,
    events_tx: &broadcast::Sender<HaEvent>,
    _registry: &Arc<RwLock<RegistryCache>>,
    queued_calls: &mut VecDeque<QueuedCall>,
    state: &mut SessionState,
) -> DisconnectReason {
    resubscribe_all(&mut ws, config, state).await;
    subscribe_registry_invalidations(&mut ws, config, state).await;
    flush_queued_calls(&mut ws, config, queued_calls, state).await;

    let mut ping = tokio::time::interval(config.ping_interval);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut timeout_check = tokio::time::interval(Duration::from_millis(50));
    timeout_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            command = rx.recv() => {
                let Some(command) = command else {
                    return DisconnectReason::Shutdown;
                };
                if handle_connected_command(command, &mut ws, config, state, queued_calls).await.is_err() {
                    return DisconnectReason::Socket;
                }
            }
            message = ws.next() => {
                let Some(message) = message else {
                    return DisconnectReason::Socket;
                };
                match message {
                    Ok(Message::Text(text)) => {
                        if handle_text(&text, state, events_tx).is_err() {
                            return DisconnectReason::Socket;
                        }
                    }
                    Ok(Message::Binary(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                    Ok(Message::Close(_)) => return DisconnectReason::Socket,
                    Ok(Message::Frame(_)) => {}
                    Err(err) => {
                        tracing::debug!(error = %err, "home assistant socket closed");
                        return DisconnectReason::Socket;
                    }
                }
            }
            _ = ping.tick() => {
                if send_payload(&mut ws, config, state, json!({ "type": "ping" }), PendingKind::Ping, Some(config.ping_timeout)).await.is_err() {
                    return DisconnectReason::Socket;
                }
            }
            _ = timeout_check.tick() => {
                if expire_pending(&mut state.pending).contains(&ExpiredKind::Ping) {
                    return DisconnectReason::Socket;
                }
            }
            else => return DisconnectReason::Shutdown,
        }
    }
}

async fn handle_connected_command(
    command: Command,
    ws: &mut Ws,
    config: &HaConnectionConfig,
    state: &mut SessionState,
    queued_calls: &mut VecDeque<QueuedCall>,
) -> Result<(), HaError> {
    match command {
        Command::Call {
            action,
            created_at,
            respond,
        } => {
            if created_at.elapsed() > config.stale_action {
                let _ = respond.send(stale_outcome());
                return Ok(());
            }
            send_call(ws, config, state, action, created_at, respond).await
        }
        Command::Request { payload, respond } => send_payload(
            ws,
            config,
            state,
            payload,
            PendingKind::Request { respond },
            None,
        )
        .await
        .map(|_| ()),
        Command::Subscribe { entity_id, respond } => {
            let entry = state
                .subscriptions
                .entry(entity_id.clone())
                .or_insert(SubscriptionEntry {
                    refs: 0,
                    subscription_id: None,
                });
            entry.refs = entry.refs.saturating_add(1);
            if entry.subscription_id.is_some() {
                let _ = respond.send(Ok(()));
                return Ok(());
            }
            send_subscribe(ws, config, state, entity_id, Some(respond)).await
        }
        Command::Unsubscribe { entity_id } => {
            if let Some(entry) = state.subscriptions.get_mut(&entity_id) {
                entry.refs = entry.refs.saturating_sub(1);
                if entry.refs == 0 {
                    if let Some(subscription_id) = entry.subscription_id.take() {
                        let _ = send_payload(ws, config, state, json!({ "type": "unsubscribe_events", "subscription": subscription_id }), PendingKind::Unsubscribe, None).await;
                    }
                    state.subscriptions.remove(&entity_id);
                }
            }
            let _ = queued_calls;
            Ok(())
        }
    }
}

fn handle_disconnected_command(
    command: Command,
    queued_calls: &mut VecDeque<QueuedCall>,
    subscriptions: &mut HashMap<String, SubscriptionEntry>,
    stale_action: Duration,
) {
    match command {
        Command::Call {
            action,
            created_at,
            respond,
        } => {
            if created_at.elapsed() > stale_action {
                let _ = respond.send(stale_outcome());
            } else {
                queued_calls.push_back(QueuedCall {
                    action,
                    created_at,
                    respond,
                });
            }
        }
        Command::Request { respond, .. } => {
            let _ = respond.send(Err(HaError::Disconnected));
        }
        Command::Subscribe { entity_id, respond } => {
            let entry = subscriptions.entry(entity_id).or_insert(SubscriptionEntry {
                refs: 0,
                subscription_id: None,
            });
            entry.refs = entry.refs.saturating_add(1);
            let _ = respond.send(Ok(()));
        }
        Command::Unsubscribe { entity_id } => {
            if let Some(entry) = subscriptions.get_mut(&entity_id) {
                entry.refs = entry.refs.saturating_sub(1);
                if entry.refs == 0 {
                    subscriptions.remove(&entity_id);
                }
            }
        }
    }
}

async fn send_call(
    ws: &mut Ws,
    config: &HaConnectionConfig,
    state: &mut SessionState,
    action: Action,
    created_at: Instant,
    respond: oneshot::Sender<ActionOutcome>,
) -> Result<(), HaError> {
    let Action::CallService {
        domain,
        service,
        target,
        data,
        ..
    } = action
    else {
        let _ = respond.send(ActionOutcome {
            activity_id: None,
            status: ActionStatus::Suppressed,
            error_code: Some("unsupported_action".to_owned()),
            message: Some("Only call_service actions can be sent directly".to_owned()),
            ha_context_id: None,
            latency_ms: None,
        });
        return Ok(());
    };
    let mut payload = json!({
        "type": "call_service",
        "domain": domain,
        "service": service,
        "target": target,
        "service_data": data,
    });
    if service_returns_response(&payload) {
        payload["return_response"] = Value::Bool(true);
    }
    send_payload(
        ws,
        config,
        state,
        payload,
        PendingKind::Call {
            started: created_at,
            respond,
        },
        None,
    )
    .await
    .map(|_| ())
}

fn service_returns_response(_payload: &Value) -> bool {
    false
}

async fn send_subscribe(
    ws: &mut Ws,
    config: &HaConnectionConfig,
    state: &mut SessionState,
    entity_id: String,
    respond: Option<oneshot::Sender<Result<(), HaError>>>,
) -> Result<(), HaError> {
    send_payload(
        ws,
        config,
        state,
        json!({ "type": "subscribe_entities", "entity_ids": [entity_id.clone()] }),
        PendingKind::Subscribe { entity_id, respond },
        None,
    )
    .await
    .map(|_| ())
}

async fn send_payload(
    ws: &mut Ws,
    config: &HaConnectionConfig,
    state: &mut SessionState,
    mut payload: Value,
    kind: PendingKind,
    timeout: Option<Duration>,
) -> Result<u64, HaError> {
    let id = state.next_id;
    state.next_id = state.next_id.saturating_add(1);
    payload["id"] = Value::from(id);
    state.pending.insert(
        id,
        Pending {
            deadline: Instant::now() + timeout.unwrap_or(config.request_timeout),
            kind,
        },
    );
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|err| HaError::WebSocket(err.to_string()))?;
    Ok(id)
}

async fn resubscribe_all(ws: &mut Ws, config: &HaConnectionConfig, state: &mut SessionState) {
    let entities: Vec<String> = state
        .subscriptions
        .iter_mut()
        .filter_map(|(entity_id, entry)| {
            entry.subscription_id = None;
            (entry.refs > 0).then(|| entity_id.clone())
        })
        .collect();
    for entity_id in entities {
        let _ = send_subscribe(ws, config, state, entity_id, None).await;
    }
}

async fn subscribe_registry_invalidations(
    ws: &mut Ws,
    config: &HaConnectionConfig,
    state: &mut SessionState,
) {
    for event_type in [
        "entity_registry_updated",
        "device_registry_updated",
        "area_registry_updated",
    ] {
        let _ = send_payload(
            ws,
            config,
            state,
            json!({ "type": "subscribe_events", "event_type": event_type }),
            PendingKind::Unsubscribe,
            None,
        )
        .await;
    }
}

async fn flush_queued_calls(
    ws: &mut Ws,
    config: &HaConnectionConfig,
    queued_calls: &mut VecDeque<QueuedCall>,
    state: &mut SessionState,
) {
    while let Some(call) = queued_calls.pop_front() {
        if call.created_at.elapsed() > config.stale_action {
            let _ = call.respond.send(stale_outcome());
            continue;
        }
        if send_call(
            ws,
            config,
            state,
            call.action,
            call.created_at,
            call.respond,
        )
        .await
        .is_err()
        {
            break;
        }
    }
}

fn handle_text(
    text: &str,
    state: &mut SessionState,
    events_tx: &broadcast::Sender<HaEvent>,
) -> Result<(), HaError> {
    let value: Value = serde_json::from_str(text)?;
    if let Some(array) = value.as_array() {
        for item in array {
            handle_message(item, state, events_tx);
        }
    } else {
        handle_message(&value, state, events_tx);
    }
    Ok(())
}

fn handle_message(value: &Value, state: &mut SessionState, events_tx: &broadcast::Sender<HaEvent>) {
    match value.get("type").and_then(Value::as_str) {
        Some("result") => handle_result(value, state, events_tx),
        Some("pong") => handle_pong(value, state),
        Some("event") => handle_event(value, state, events_tx),
        _ => {}
    }
}

fn handle_pong(value: &Value, state: &mut SessionState) {
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return;
    };
    let is_ping = state
        .pending
        .get(&id)
        .is_some_and(|pending| matches!(&pending.kind, PendingKind::Ping));
    if is_ping {
        state.pending.remove(&id);
    }
}

fn handle_result(value: &Value, state: &mut SessionState, events_tx: &broadcast::Sender<HaEvent>) {
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return;
    };
    let Some(pending) = state.pending.remove(&id) else {
        return;
    };
    let success = value
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match pending.kind {
        PendingKind::Call { started, respond } => {
            let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let outcome = if success {
                ActionOutcome {
                    activity_id: None,
                    status: ActionStatus::Ok,
                    error_code: None,
                    message: None,
                    ha_context_id: value.get("result").and_then(context_id),
                    latency_ms: Some(latency_ms),
                }
            } else {
                let (code, message) = result_error(value);
                error_outcome(code, message, Some(latency_ms))
            };
            let _ = events_tx.send(HaEvent::ActionResult {
                outcome: outcome.clone(),
            });
            let _ = respond.send(outcome);
        }
        PendingKind::Request { respond } => {
            if success {
                let _ = respond.send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
            } else {
                let (code, message) = result_error(value);
                let detail = code.or(message).unwrap_or("request failed").to_owned();
                let _ = respond.send(Err(HaError::Protocol(detail)));
            }
        }
        PendingKind::Subscribe { entity_id, respond } => {
            if success {
                if let Some(entry) = state.subscriptions.get_mut(&entity_id) {
                    entry.subscription_id = Some(id);
                }
                if let Some(respond) = respond {
                    let _ = respond.send(Ok(()));
                }
            } else if let Some(respond) = respond {
                let (code, message) = result_error(value);
                let _ = respond.send(Err(HaError::Protocol(
                    code.or(message).unwrap_or("subscribe failed").to_owned(),
                )));
            }
        }
        PendingKind::Unsubscribe | PendingKind::Ping => {}
    }
}

fn handle_event(value: &Value, state: &mut SessionState, events_tx: &broadcast::Sender<HaEvent>) {
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return;
    };
    let is_entity_subscription = state
        .subscriptions
        .values()
        .any(|entry| entry.subscription_id == Some(id));
    if is_entity_subscription {
        for delta in parse_compressed_entities(value, &mut state.entity_states) {
            match delta {
                crate::EntityDelta::Added(entity) | crate::EntityDelta::Changed(entity) => {
                    let _ = events_tx.send(HaEvent::Entity { entity });
                }
                crate::EntityDelta::Removed(entity_id) => {
                    let _ = state.entity_states.remove(&entity_id);
                }
            }
        }
        return;
    }

    let event_type = value
        .get("event")
        .and_then(|event| event.get("event_type"))
        .and_then(Value::as_str);
    if matches!(
        event_type,
        Some("entity_registry_updated" | "device_registry_updated" | "area_registry_updated")
    ) {
        let kind = event_type
            .unwrap_or("registry_updated")
            .trim_end_matches("_updated")
            .to_owned();
        let _ = events_tx.send(HaEvent::RegistryInvalidated { kind });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpiredKind {
    Ping,
    Other,
}

fn expire_pending(pending: &mut HashMap<u64, Pending>) -> Vec<ExpiredKind> {
    let now = Instant::now();
    let expired_ids: Vec<u64> = pending
        .iter()
        .filter_map(|(id, pending)| (pending.deadline <= now).then_some(*id))
        .collect();
    let mut kinds = Vec::new();
    for id in expired_ids {
        if let Some(pending) = pending.remove(&id) {
            match pending.kind {
                PendingKind::Call { respond, .. } => {
                    let _ = respond.send(timeout_outcome());
                    kinds.push(ExpiredKind::Other);
                }
                PendingKind::Request { respond } => {
                    let _ = respond.send(Err(HaError::Timeout));
                    kinds.push(ExpiredKind::Other);
                }
                PendingKind::Subscribe { respond, .. } => {
                    if let Some(respond) = respond {
                        let _ = respond.send(Err(HaError::Timeout));
                    }
                    kinds.push(ExpiredKind::Other);
                }
                PendingKind::Unsubscribe => kinds.push(ExpiredKind::Other),
                PendingKind::Ping => kinds.push(ExpiredKind::Ping),
            }
        }
    }
    kinds
}

fn fail_pending(pending: &mut HashMap<u64, Pending>, err: HaError) {
    for (_, pending) in pending.drain() {
        match pending.kind {
            PendingKind::Call { respond, .. } => {
                let _ = respond.send(error_outcome(
                    Some("disconnected"),
                    Some("Home Assistant didn't respond"),
                    None,
                ));
            }
            PendingKind::Request { respond } => {
                let _ = respond.send(Err(match &err {
                    HaError::Disconnected => HaError::Disconnected,
                    _ => HaError::Disconnected,
                }));
            }
            PendingKind::Subscribe { respond, .. } => {
                if let Some(respond) = respond {
                    let _ = respond.send(Err(HaError::Disconnected));
                }
            }
            PendingKind::Unsubscribe | PendingKind::Ping => {}
        }
    }
}

fn drop_stale(queued_calls: &mut VecDeque<QueuedCall>, stale_action: Duration) {
    let mut kept = VecDeque::new();
    while let Some(call) = queued_calls.pop_front() {
        if call.created_at.elapsed() > stale_action {
            let _ = call.respond.send(stale_outcome());
        } else {
            kept.push_back(call);
        }
    }
    *queued_calls = kept;
}

async fn drain_auth_failed(rx: &mut mpsc::Receiver<Command>) {
    while let Some(command) = rx.recv().await {
        match command {
            Command::Call { respond, .. } => {
                let _ = respond.send(error_outcome(
                    Some("unauthorized"),
                    Some("Flick's HA user can't do this"),
                    None,
                ));
            }
            Command::Request { respond, .. } => {
                let _ = respond.send(Err(HaError::AuthInvalid("invalid token".to_owned())));
            }
            Command::Subscribe { respond, .. } => {
                let _ = respond.send(Err(HaError::AuthInvalid("invalid token".to_owned())));
            }
            Command::Unsubscribe { .. } => {}
        }
    }
}

async fn read_json(ws: &mut Ws, timeout: Duration) -> Result<Value, HaError> {
    let message = tokio::time::timeout(timeout, ws.next())
        .await
        .map_err(|_| HaError::Timeout)?
        .ok_or(HaError::Disconnected)?
        .map_err(|err| HaError::WebSocket(err.to_string()))?;
    match message {
        Message::Text(text) => serde_json::from_str(&text).map_err(HaError::Json),
        Message::Close(_) => Err(HaError::Disconnected),
        _ => Err(HaError::Protocol("expected text message".to_owned())),
    }
}

fn set_status(
    status_tx: &watch::Sender<HaStatus>,
    events_tx: &broadcast::Sender<HaEvent>,
    status: HaStatus,
) {
    let _ = status_tx.send(status.clone());
    let _ = events_tx.send(HaEvent::Status { status });
}

fn backoff_delay(config: &HaConnectionConfig, attempt: u32) -> Duration {
    let base_ms = config.reconnect_initial.as_millis();
    let cap_ms = config.reconnect_max.as_millis();
    let shift = attempt.saturating_sub(1).min(6);
    let exp = base_ms.saturating_mul(1_u128 << shift).min(cap_ms);
    let jitter = pseudo_jitter_ms(exp.max(1));
    Duration::from_millis(u64::try_from(jitter).unwrap_or(u64::MAX))
}

fn pseudo_jitter_ms(max_ms: u128) -> u128 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    1 + (nanos % max_ms)
}
