//! Gesture dispatcher wiring for P1-606.
//!
//! The real capture/vision/spatial lanes plug in through `flick-core` traits. Until those lanes are
//! merged, this module keeps the dispatcher independent and testable with seeded mappings/anchors.

use std::{
    collections::HashMap,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use flick_api::{
    ActionResultEvent, ConfirmRequiredEvent, EventHub, GestureFiredEvent, LatencyBreakdown,
    WsServerMessage,
};
use flick_core::{
    Action, ActionOutcome, ActionStatus, ActionTarget, ActivityId, AnchorId, BuiltinGesture,
    GestureEvent, GestureId, GesturePhase, Handedness, MappingId, ResolvedAction,
    SuppressionReason, Verb,
};
use flick_ha::{
    EntityState, HaClient, RegistryEntity, RegistrySnapshot, SafetyClass, SafetyInput,
    SafetyValidator, VerbTarget,
    verb::{resolve_selected_dial, resolve_verb},
};
use flick_store::Store;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;

/// Dispatcher result for one gesture event.
#[derive(Debug, Clone, Default)]
pub struct DispatchReport {
    /// Concrete action outcomes.
    pub outcomes: Vec<ActionOutcome>,
    /// Suppression reasons emitted while resolving this gesture.
    pub suppressions: Vec<DispatchSuppression>,
}

/// A suppressed mapping or gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSuppression {
    /// Mapping id when suppression happened after lookup.
    pub mapping_id: Option<MappingId>,
    /// Stable reason code.
    pub reason: SuppressionReason,
}

/// User/runtime dispatcher settings.
#[derive(Debug, Clone)]
pub struct DispatcherSettings {
    /// Allows sensitive devices after a mapping also opts in and a confirmation gesture fires.
    pub allow_sensitive_actions: bool,
    /// Confirmation window for sensitive actions.
    pub confirmation_window: Duration,
    /// Default confirmation gesture.
    pub default_confirm_gesture: GestureId,
}

impl Default for DispatcherSettings {
    fn default() -> Self {
        Self {
            allow_sensitive_actions: false,
            confirmation_window: Duration::from_secs(3),
            default_confirm_gesture: GestureId::Builtin(BuiltinGesture::ThumbUp),
        }
    }
}

/// A dispatcher mapping loaded from SQLite or seeded by a fixture.
#[derive(Debug, Clone)]
pub struct DispatcherMapping {
    pub id: MappingId,
    pub name: String,
    pub enabled: bool,
    pub gesture_id: GestureId,
    pub hand: MappingHand,
    pub camera_ids: Vec<String>,
    pub target: MappingTarget,
    pub action: Action,
    pub cooldown_ms: u64,
    pub sensitive_ack: bool,
    pub confirm_gesture_id: Option<GestureId>,
    pub sort_order: i64,
}

impl DispatcherMapping {
    /// Creates a simple enabled global mapping.
    #[must_use]
    pub fn global(
        id: MappingId,
        name: impl Into<String>,
        gesture_id: GestureId,
        action: Action,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            enabled: true,
            gesture_id,
            hand: MappingHand::Any,
            camera_ids: Vec::new(),
            target: MappingTarget::Global,
            action,
            cooldown_ms: 1_000,
            sensitive_ack: false,
            confirm_gesture_id: None,
            sort_order: 0,
        }
    }

    /// Creates a targeted anchor mapping.
    #[must_use]
    pub fn anchor(
        id: MappingId,
        name: impl Into<String>,
        gesture_id: GestureId,
        anchor_id: AnchorId,
        action: Action,
    ) -> Self {
        let mut mapping = Self::global(id, name, gesture_id, action);
        mapping.target = MappingTarget::Anchor(anchor_id);
        mapping
    }
}

/// Mapping hand constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingHand {
    Any,
    Left,
    Right,
}

impl MappingHand {
    fn matches(self, hand: Handedness) -> bool {
        match self {
            Self::Any => true,
            Self::Left => matches!(hand, Handedness::Left),
            Self::Right => matches!(hand, Handedness::Right),
        }
    }
}

/// Dispatcher target mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingTarget {
    Global,
    Anchor(AnchorId),
    Domain(String),
}

/// Selected anchor data needed before flick-spatial repositories land.
#[derive(Debug, Clone)]
pub struct DispatcherAnchor {
    pub id: AnchorId,
    pub name: String,
    pub domain: String,
    pub entity: EntityState,
    pub levels: Vec<f64>,
}

impl DispatcherAnchor {
    /// Returns the verb resolution target.
    #[must_use]
    pub fn verb_target(&self) -> VerbTarget {
        VerbTarget::new(self.entity.clone(), self.levels.clone())
    }

    /// Builds dispatcher target metadata from a spatial anchor and HA entity state.
    #[must_use]
    pub fn from_spatial(anchor: &flick_spatial::Anchor, entity: EntityState) -> Self {
        let levels = anchor
            .verb_params
            .get("levels")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(serde_json::Value::as_f64).collect())
            .unwrap_or_default();
        Self {
            id: anchor.id,
            name: anchor.name.clone(),
            domain: anchor.domain.clone(),
            entity,
            levels,
        }
    }
}

/// Sink adapter around the real HA client.
#[derive(Clone)]
pub struct HaActionSink {
    client: HaClient,
}

impl HaActionSink {
    /// Creates a sink from a connected HA client.
    #[must_use]
    pub const fn new(client: HaClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl flick_core::ActionSink for HaActionSink {
    async fn execute(&self, action: &ResolvedAction) -> ActionOutcome {
        match &action.action {
            Action::Dial {
                entity_id,
                property,
                gain,
                min,
                max,
            } => {
                let concrete = flick_ha::dial::dial_action(
                    &flick_ha::DialTarget {
                        entity_id: entity_id.clone(),
                        property: *property,
                        base_value: None,
                        gain: *gain,
                        min: min.as_ref().and_then(serde_json::Value::as_f64),
                        max: max.as_ref().and_then(serde_json::Value::as_f64),
                    },
                    1.0,
                );
                self.client.call(concrete).await
            }
            _ => self.client.call(action.action.clone()).await,
        }
    }
}

/// Local fake action sink used before HA is configured.
#[derive(Default)]
pub struct NoopActionSink;

#[async_trait]
impl flick_core::ActionSink for NoopActionSink {
    async fn execute(&self, _action: &ResolvedAction) -> ActionOutcome {
        ActionOutcome {
            activity_id: None,
            status: ActionStatus::Suppressed,
            error_code: Some("ha.not_configured".to_owned()),
            message: Some("Home Assistant is not configured".to_owned()),
            ha_context_id: None,
            latency_ms: None,
        }
    }
}

/// Dispatcher wired to mapping/anchor stores, safety validation, HA and WS events.
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<DispatcherInner>,
}

struct DispatcherInner {
    store: Option<Arc<Store>>,
    sink: Mutex<Arc<dyn flick_core::ActionSink>>,
    safety: SafetyValidator,
    events: Option<EventHub>,
    settings: Mutex<DispatcherSettings>,
    registry: Mutex<RegistrySnapshot>,
    paused: AtomicBool,
    state: Mutex<DispatcherState>,
}

#[derive(Debug, Default)]
struct DispatcherState {
    mappings: Vec<DispatcherMapping>,
    anchors: HashMap<AnchorId, DispatcherAnchor>,
    cooldowns: HashMap<MappingId, Instant>,
    pending_confirmations: Vec<PendingConfirmation>,
}

#[derive(Debug, Clone)]
struct PendingConfirmation {
    confirm_gesture_id: GestureId,
    expires_at: Instant,
    resolved: ResolvedAction,
}

impl Dispatcher {
    /// Creates a dispatcher over a supplied action sink.
    #[must_use]
    pub fn new(sink: Arc<dyn flick_core::ActionSink>) -> Self {
        Self::builder(sink).build()
    }

    /// Starts a builder.
    #[must_use]
    pub fn builder(sink: Arc<dyn flick_core::ActionSink>) -> DispatcherBuilder {
        DispatcherBuilder {
            sink,
            store: None,
            safety: SafetyValidator::new(),
            events: None,
            settings: DispatcherSettings::default(),
            registry: RegistrySnapshot::default(),
            mappings: Vec::new(),
            anchors: Vec::new(),
        }
    }

    /// Replaces the in-memory mapping seed. Runtime DB reload will be added with the store lane.
    pub async fn set_mappings(&self, mappings: Vec<DispatcherMapping>) {
        self.inner.state.lock().await.mappings = mappings;
    }

    /// Replaces the in-memory anchor seed. This is the flick-spatial adapter seam.
    pub async fn set_anchors(&self, anchors: Vec<DispatcherAnchor>) {
        self.inner.state.lock().await.anchors = anchors
            .into_iter()
            .map(|anchor| (anchor.id, anchor))
            .collect();
    }

    /// Replaces the action sink without rebuilding mappings or cooldown state.
    pub async fn set_sink(&self, sink: Arc<dyn flick_core::ActionSink>) {
        *self.inner.sink.lock().await = sink;
    }

    /// Replaces dispatcher runtime settings without clearing mapping state.
    pub async fn set_settings(&self, settings: DispatcherSettings) {
        *self.inner.settings.lock().await = settings;
    }

    /// Replaces the HA registry snapshot used for target expansion and safety checks.
    pub async fn set_registry(&self, registry: RegistrySnapshot) {
        *self.inner.registry.lock().await = registry;
    }

    /// Pauses or resumes dispatch and clears pending confirmations when pausing.
    pub async fn set_paused(&self, paused: bool) {
        self.inner.paused.store(paused, Ordering::Relaxed);
        if paused {
            self.inner.state.lock().await.pending_confirmations.clear();
        }
    }

    /// Dispatches one debounced gesture event.
    pub async fn dispatch(&self, event: &GestureEvent) -> DispatchReport {
        if event.phase != GesturePhase::Fired {
            return DispatchReport::default();
        }
        if self.inner.paused.load(Ordering::Relaxed) {
            self.insert_suppressed_activity(
                event,
                None,
                SuppressionReason::Paused,
                Some("Engine is paused"),
            )
            .await;
            return DispatchReport {
                outcomes: Vec::new(),
                suppressions: vec![DispatchSuppression {
                    mapping_id: None,
                    reason: SuppressionReason::Paused,
                }],
            };
        }

        if let Some(resolved) = self.take_pending_confirmation(event).await {
            let outcome = self.execute_resolved(event, resolved).await;
            return DispatchReport {
                outcomes: vec![outcome],
                suppressions: Vec::new(),
            };
        }

        let resolution = {
            let mut state = self.inner.state.lock().await;
            state
                .pending_confirmations
                .retain(|pending| Instant::now() <= pending.expires_at);
            resolve_mappings(&mut state, event)
        };

        let mut report = DispatchReport {
            suppressions: resolution.suppressions,
            outcomes: Vec::new(),
        };
        if resolution.mappings.is_empty() {
            if report.suppressions.is_empty() {
                report.suppressions.push(DispatchSuppression {
                    mapping_id: None,
                    reason: SuppressionReason::NoMapping,
                });
            }
            for suppression in &report.suppressions {
                self.insert_suppressed_activity(
                    event,
                    suppression.mapping_id,
                    suppression.reason,
                    None,
                )
                .await;
            }
            return report;
        }

        for mapping in resolution.mappings {
            match self.resolve_action(event, &mapping).await {
                Ok(resolved) => match self.apply_safety(event, &mapping, resolved).await {
                    SafetyDecision::Execute(resolved) => {
                        report
                            .outcomes
                            .push(self.execute_resolved(event, *resolved).await);
                    }
                    SafetyDecision::Suppressed(reason, message) => {
                        self.insert_suppressed_activity(
                            event,
                            Some(mapping.id),
                            reason,
                            message.as_deref(),
                        )
                        .await;
                        report.suppressions.push(DispatchSuppression {
                            mapping_id: Some(mapping.id),
                            reason,
                        });
                    }
                    SafetyDecision::ConfirmationQueued => {
                        self.insert_suppressed_activity(
                            event,
                            Some(mapping.id),
                            SuppressionReason::ConfirmationRequired,
                            Some("Sensitive action awaiting confirmation"),
                        )
                        .await;
                        report.suppressions.push(DispatchSuppression {
                            mapping_id: Some(mapping.id),
                            reason: SuppressionReason::ConfirmationRequired,
                        });
                    }
                },
                Err(err) => {
                    self.insert_suppressed_activity(
                        event,
                        Some(mapping.id),
                        err,
                        Some("Action could not be resolved"),
                    )
                    .await;
                    report.suppressions.push(DispatchSuppression {
                        mapping_id: Some(mapping.id),
                        reason: err,
                    });
                }
            }
        }
        report
    }

    /// Fires a mapping through the same safety and HA path used by API test buttons.
    pub async fn test_fire(&self, mapping_id: MappingId) -> ActionOutcome {
        let now = Instant::now();
        let mapping = {
            let state = self.inner.state.lock().await;
            state
                .mappings
                .iter()
                .find(|mapping| mapping.id == mapping_id)
                .cloned()
        };
        let Some(mapping) = mapping else {
            return suppressed_outcome("mapping_not_found", "mapping not found");
        };
        let event = GestureEvent {
            id: flick_core::GestureEventId::new(),
            camera_id: flick_core::CameraId::new(),
            gesture_id: mapping.gesture_id,
            hand: Handedness::Right,
            confidence: 1.0,
            phase: GesturePhase::Fired,
            value: None,
            target: match mapping.target {
                MappingTarget::Anchor(anchor_id) => Some(anchor_id),
                MappingTarget::Global | MappingTarget::Domain(_) => None,
            },
            onset_at: now,
            fired_at: now,
        };
        match self.resolve_action(&event, &mapping).await {
            Ok(resolved) => match self.apply_safety(&event, &mapping, resolved).await {
                SafetyDecision::Execute(resolved) => self.execute_resolved(&event, *resolved).await,
                SafetyDecision::Suppressed(reason, message) => suppressed_outcome(
                    &format!("{reason:?}"),
                    message.as_deref().unwrap_or("suppressed"),
                ),
                SafetyDecision::ConfirmationQueued => suppressed_outcome(
                    "confirmation_required",
                    "Sensitive action awaiting confirmation",
                ),
            },
            Err(reason) => {
                suppressed_outcome(&format!("{reason:?}"), "Action could not be resolved")
            }
        }
    }

    async fn take_pending_confirmation(&self, event: &GestureEvent) -> Option<ResolvedAction> {
        if self.inner.paused.load(Ordering::Relaxed) {
            self.inner.state.lock().await.pending_confirmations.clear();
            return None;
        }
        let mut state = self.inner.state.lock().await;
        let now = Instant::now();
        state
            .pending_confirmations
            .retain(|pending| now <= pending.expires_at);
        let index = state
            .pending_confirmations
            .iter()
            .position(|pending| pending.confirm_gesture_id == event.gesture_id)?;
        Some(state.pending_confirmations.remove(index).resolved)
    }

    async fn resolve_action(
        &self,
        event: &GestureEvent,
        mapping: &DispatcherMapping,
    ) -> Result<ResolvedAction, SuppressionReason> {
        let action_resolution = match &mapping.action {
            Action::CallService {
                domain, service, ..
            } => ResolvedActionParts {
                action: mapping.action.clone(),
                summary: format!("{domain}.{service}"),
                anchor_id: event.target,
            },
            Action::Verb { verb, level } => {
                let anchor = self
                    .anchor_for_event(event)
                    .await
                    .ok_or(SuppressionReason::NoTarget)?;
                let resolution = resolve_verb(&anchor.verb_target(), *verb, *level)
                    .map_err(|_| SuppressionReason::NoMapping)?;
                ResolvedActionParts {
                    action: resolution.action,
                    summary: resolution.summary,
                    anchor_id: Some(anchor.id),
                }
            }
            Action::Dial {
                entity_id,
                property,
                gain,
                min,
                max,
            } if entity_id == "$selected" => {
                let anchor = self
                    .anchor_for_event(event)
                    .await
                    .ok_or(SuppressionReason::NoTarget)?;
                let action = resolve_selected_dial(
                    &anchor.verb_target(),
                    *property,
                    *gain,
                    min.clone(),
                    max.clone(),
                );
                ResolvedActionParts {
                    action,
                    summary: format!("{} dial", anchor.name),
                    anchor_id: Some(anchor.id),
                }
            }
            Action::Dial { .. } => return Err(SuppressionReason::NoMapping),
        };

        Ok(ResolvedAction {
            mapping_id: mapping.id,
            event_id: event.id,
            anchor_id: action_resolution.anchor_id,
            action: action_resolution.action,
            summary: action_resolution.summary,
            stale_after_ms: 2_000,
        })
    }

    async fn anchor_for_event(&self, event: &GestureEvent) -> Option<DispatcherAnchor> {
        let target = event.target?;
        self.inner.state.lock().await.anchors.get(&target).cloned()
    }

    async fn apply_safety(
        &self,
        event: &GestureEvent,
        mapping: &DispatcherMapping,
        resolved: ResolvedAction,
    ) -> SafetyDecision {
        if matches!(resolved.action, Action::Dial { .. }) {
            return SafetyDecision::Execute(Box::new(resolved));
        }
        if !matches!(resolved.action, Action::CallService { .. }) {
            return SafetyDecision::Suppressed(
                SuppressionReason::BlockedDomain,
                Some("Only concrete Home Assistant service calls can be dispatched".to_owned()),
            );
        }
        let registry = self.inner.registry.lock().await.clone();
        match classify_action_safety(&self.inner.safety, &resolved.action, &registry) {
            SafetyClass::Denied => SafetyDecision::Suppressed(
                SuppressionReason::BlockedDomain,
                Some("Action is denied by the safety policy".to_owned()),
            ),
            SafetyClass::Normal => SafetyDecision::Execute(Box::new(resolved)),
            SafetyClass::Sensitive => {
                let settings = self.inner.settings.lock().await.clone();
                if !settings.allow_sensitive_actions || !mapping.sensitive_ack {
                    return SafetyDecision::Suppressed(
                        SuppressionReason::BlockedDomain,
                        Some("Sensitive action is not enabled for this mapping".to_owned()),
                    );
                }
                let confirm_gesture_id = mapping
                    .confirm_gesture_id
                    .unwrap_or(settings.default_confirm_gesture);
                let expires_at = Instant::now() + settings.confirmation_window;
                {
                    let mut state = self.inner.state.lock().await;
                    state.pending_confirmations.push(PendingConfirmation {
                        confirm_gesture_id,
                        expires_at,
                        resolved: resolved.clone(),
                    });
                }
                self.publish_confirm_required(event, mapping, confirm_gesture_id, expires_at);
                SafetyDecision::ConfirmationQueued
            }
        }
    }

    async fn execute_resolved(
        &self,
        event: &GestureEvent,
        resolved: ResolvedAction,
    ) -> ActionOutcome {
        self.publish_gesture_fired(event, &resolved);
        let dispatch_started = Instant::now();
        let sink = self.inner.sink.lock().await.clone();
        let mut outcome = sink.execute(&resolved).await;
        let activity_id = outcome.activity_id.unwrap_or_else(ActivityId::new);
        outcome.activity_id = Some(activity_id);
        let ha_ms = outcome
            .latency_ms
            .map(|value| value as f64)
            .unwrap_or_else(|| dispatch_started.elapsed().as_secs_f64() * 1_000.0);
        let latency = latency_breakdown(
            event,
            Some(dispatch_started.elapsed().as_secs_f64() * 1_000.0),
            Some(ha_ms),
        );
        self.insert_activity(event, &resolved, &outcome, activity_id, &latency)
            .await;
        self.publish_action_result(event, &resolved, &outcome, activity_id, latency);
        outcome
    }

    fn publish_gesture_fired(&self, event: &GestureEvent, resolved: &ResolvedAction) {
        let Some(events) = &self.inner.events else {
            return;
        };
        events.publish(WsServerMessage::GestureFired {
            ts: now_rfc3339(),
            payload: GestureFiredEvent {
                event_id: event.id.to_string(),
                camera_id: event.camera_id.to_string(),
                gesture_id: event.gesture_id.to_string(),
                hand: hand_str(event.hand).to_owned(),
                confidence: f64::from(event.confidence),
                mapping_ids: vec![resolved.mapping_id.to_string()],
                action_summary: resolved.summary.clone(),
            },
        });
    }

    fn publish_confirm_required(
        &self,
        event: &GestureEvent,
        mapping: &DispatcherMapping,
        confirm_gesture_id: GestureId,
        expires_at: Instant,
    ) {
        let Some(events) = &self.inner.events else {
            return;
        };
        let expires_at = SystemTime::now()
            .checked_add(expires_at.saturating_duration_since(Instant::now()))
            .unwrap_or_else(SystemTime::now);
        events.publish(WsServerMessage::ConfirmRequired {
            ts: now_rfc3339(),
            payload: ConfirmRequiredEvent {
                event_id: event.id.to_string(),
                mapping_id: mapping.id.to_string(),
                confirm_gesture_id: confirm_gesture_id.to_string(),
                expires_at: system_time_rfc3339(expires_at),
            },
        });
    }

    fn publish_action_result(
        &self,
        event: &GestureEvent,
        resolved: &ResolvedAction,
        outcome: &ActionOutcome,
        activity_id: ActivityId,
        latency: LatencyBreakdown,
    ) {
        let Some(events) = &self.inner.events else {
            return;
        };
        events.publish(WsServerMessage::ActionResult {
            ts: now_rfc3339(),
            payload: ActionResultEvent {
                activity_id: activity_id.to_string(),
                event_id: event.id.to_string(),
                mapping_id: resolved.mapping_id.to_string(),
                status: action_status_str(outcome.status).to_owned(),
                error_code: outcome.error_code.clone(),
                message: outcome.message.clone(),
                latency,
            },
        });
    }

    async fn insert_suppressed_activity(
        &self,
        event: &GestureEvent,
        mapping_id: Option<MappingId>,
        reason: SuppressionReason,
        message: Option<&str>,
    ) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let id = ActivityId::new();
        let latency = latency_breakdown(event, None, None);
        let latency_json = serde_json::to_string(&latency).unwrap_or_else(|_| "{}".to_owned());
        let conn = store.connection();
        let _ = conn.execute(
            "INSERT INTO activity_log (id, ts, camera_id, gesture_id, hand, confidence, mapping_id, anchor_id, action_summary, status, reason, message, ha_context_id, latency) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 'suppressed', ?9, ?10, NULL, ?11)",
            rusqlite::params![
                id.to_string(),
                now_ms(),
                event.camera_id.to_string(),
                event.gesture_id.to_string(),
                hand_str(event.hand),
                f64::from(event.confidence),
                mapping_id.map(|id| id.to_string()),
                event.target.map(|id| id.to_string()),
                suppression_reason_str(reason),
                message,
                latency_json,
            ],
        );
    }

    async fn insert_activity(
        &self,
        event: &GestureEvent,
        resolved: &ResolvedAction,
        outcome: &ActionOutcome,
        activity_id: ActivityId,
        latency: &LatencyBreakdown,
    ) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let latency_json = serde_json::to_string(latency).unwrap_or_else(|_| "{}".to_owned());
        let conn = store.connection();
        let _ = conn.execute(
            "INSERT INTO activity_log (id, ts, camera_id, gesture_id, hand, confidence, mapping_id, anchor_id, action_summary, status, reason, message, ha_context_id, latency) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                activity_id.to_string(),
                now_ms(),
                event.camera_id.to_string(),
                event.gesture_id.to_string(),
                hand_str(event.hand),
                f64::from(event.confidence),
                resolved.mapping_id.to_string(),
                resolved.anchor_id.map(|id| id.to_string()),
                resolved.summary,
                action_status_str(outcome.status),
                outcome.error_code,
                outcome.message,
                outcome.ha_context_id,
                latency_json,
            ],
        );
    }
}

/// Builder for [`Dispatcher`].
pub struct DispatcherBuilder {
    sink: Arc<dyn flick_core::ActionSink>,
    store: Option<Arc<Store>>,
    safety: SafetyValidator,
    events: Option<EventHub>,
    settings: DispatcherSettings,
    registry: RegistrySnapshot,
    mappings: Vec<DispatcherMapping>,
    anchors: Vec<DispatcherAnchor>,
}

impl DispatcherBuilder {
    /// Adds activity-log persistence.
    #[must_use]
    pub fn store(mut self, store: Arc<Store>) -> Self {
        self.store = Some(store);
        self
    }

    /// Adds WS event publishing.
    #[must_use]
    pub fn events(mut self, events: EventHub) -> Self {
        self.events = Some(events);
        self
    }

    /// Overrides settings.
    #[must_use]
    pub fn settings(mut self, settings: DispatcherSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Seeds the HA registry snapshot used for safety expansion.
    #[must_use]
    pub fn registry(mut self, registry: RegistrySnapshot) -> Self {
        self.registry = registry;
        self
    }

    /// Seeds mappings.
    #[must_use]
    pub fn mappings(mut self, mappings: Vec<DispatcherMapping>) -> Self {
        self.mappings = mappings;
        self
    }

    /// Seeds anchors.
    #[must_use]
    pub fn anchors(mut self, anchors: Vec<DispatcherAnchor>) -> Self {
        self.anchors = anchors;
        self
    }

    /// Builds the dispatcher.
    #[must_use]
    pub fn build(self) -> Dispatcher {
        Dispatcher {
            inner: Arc::new(DispatcherInner {
                store: self.store,
                sink: Mutex::new(self.sink),
                safety: self.safety,
                events: self.events,
                settings: Mutex::new(self.settings),
                registry: Mutex::new(self.registry),
                paused: AtomicBool::new(false),
                state: Mutex::new(DispatcherState {
                    mappings: self.mappings,
                    anchors: self
                        .anchors
                        .into_iter()
                        .map(|anchor| (anchor.id, anchor))
                        .collect(),
                    cooldowns: HashMap::new(),
                    pending_confirmations: Vec::new(),
                }),
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct MappingResolution {
    mappings: Vec<DispatcherMapping>,
    suppressions: Vec<DispatchSuppression>,
}

fn resolve_mappings(state: &mut DispatcherState, event: &GestureEvent) -> MappingResolution {
    let now = Instant::now();
    let mut matching: Vec<DispatcherMapping> = state
        .mappings
        .iter()
        .filter(|mapping| mapping.enabled)
        .filter(|mapping| gesture_matches(mapping.gesture_id, event.gesture_id))
        .filter(|mapping| mapping.hand.matches(event.hand))
        .filter(|mapping| {
            mapping.camera_ids.is_empty()
                || mapping
                    .camera_ids
                    .iter()
                    .any(|id| id == &event.camera_id.to_string())
        })
        .cloned()
        .collect();

    if matching.is_empty() {
        return MappingResolution {
            mappings: Vec::new(),
            suppressions: vec![DispatchSuppression {
                mapping_id: None,
                reason: SuppressionReason::NoMapping,
            }],
        };
    }

    let mut suppressions = Vec::new();
    let selected_anchor = event.target.and_then(|id| state.anchors.get(&id));
    let selected_anchor_id = selected_anchor.map(|anchor| anchor.id);
    let selected_domain = selected_anchor.map(|anchor| anchor.domain.as_str());

    if let Some(anchor_id) = selected_anchor_id {
        let targeted: Vec<_> = matching
            .iter()
            .filter(|mapping| match &mapping.target {
                MappingTarget::Anchor(id) => *id == anchor_id,
                MappingTarget::Domain(domain) => {
                    selected_domain.is_some_and(|selected| selected == domain)
                }
                MappingTarget::Global => false,
            })
            .cloned()
            .collect();
        for mapping in matching
            .iter()
            .filter(|mapping| matches!(mapping.target, MappingTarget::Global))
        {
            suppressions.push(DispatchSuppression {
                mapping_id: Some(mapping.id),
                reason: SuppressionReason::TargetSelected,
            });
        }
        if targeted.is_empty() {
            return MappingResolution {
                mappings: Vec::new(),
                suppressions,
            };
        }
        matching = targeted;
    } else {
        for mapping in matching
            .iter()
            .filter(|mapping| !matches!(mapping.target, MappingTarget::Global))
        {
            suppressions.push(DispatchSuppression {
                mapping_id: Some(mapping.id),
                reason: SuppressionReason::NoTarget,
            });
        }
        matching.retain(|mapping| matches!(mapping.target, MappingTarget::Global));
    }

    matching.sort_by_key(|mapping| mapping.sort_order);
    matching.retain(|mapping| {
        let in_cooldown = state.cooldowns.get(&mapping.id).is_some_and(|last| {
            now.duration_since(*last) < Duration::from_millis(mapping.cooldown_ms)
        });
        if in_cooldown {
            suppressions.push(DispatchSuppression {
                mapping_id: Some(mapping.id),
                reason: SuppressionReason::Cooldown,
            });
            false
        } else {
            state.cooldowns.insert(mapping.id, now);
            true
        }
    });

    MappingResolution {
        mappings: matching,
        suppressions,
    }
}

fn gesture_matches(mapping_id: GestureId, event_id: GestureId) -> bool {
    mapping_id == event_id
        || matches!(
            (mapping_id, event_id),
            (
                GestureId::Builtin(BuiltinGesture::CircleAny),
                GestureId::Builtin(BuiltinGesture::CircleCw | BuiltinGesture::CircleCcw)
            )
        )
}

struct ResolvedActionParts {
    action: Action,
    summary: String,
    anchor_id: Option<AnchorId>,
}

enum SafetyDecision {
    Execute(Box<ResolvedAction>),
    Suppressed(SuppressionReason, Option<String>),
    ConfirmationQueued,
}

/// Classifies an action after expanding entity/device/area targets through the HA registry.
#[must_use]
pub fn classify_action_safety(
    validator: &SafetyValidator,
    action: &Action,
    registry: &RegistrySnapshot,
) -> SafetyClass {
    let Action::CallService {
        domain,
        service,
        target,
        ..
    } = action
    else {
        return SafetyClass::Denied;
    };

    let mut strictest = classify_concrete_target(validator, domain, service, None);
    let mut saw_explicit_target = false;
    let mut saw_resolved_entity = false;
    let mut unresolved = false;
    let mut resolved_ids = std::collections::HashSet::new();

    if let Some(entity_ids) = target.entity_id.as_ref() {
        saw_explicit_target = true;
        for entity_id in entity_ids {
            if let Some(entity) = registry
                .entities
                .iter()
                .find(|entity| entity.entity_id == *entity_id)
            {
                if resolved_ids.insert(entity.entity_id.clone()) {
                    saw_resolved_entity = true;
                    strictest = strictest_safety(
                        strictest,
                        classify_concrete_target(validator, domain, service, Some(entity)),
                    );
                }
            } else {
                unresolved = true;
            }
        }
    }

    if let Some(device_ids) = target.device_id.as_ref() {
        saw_explicit_target = true;
        for device_id in device_ids {
            let mut matched = false;
            for entity in registry
                .entities
                .iter()
                .filter(|entity| entity.device_id.as_deref() == Some(device_id.as_str()))
            {
                matched = true;
                if resolved_ids.insert(entity.entity_id.clone()) {
                    saw_resolved_entity = true;
                    strictest = strictest_safety(
                        strictest,
                        classify_concrete_target(validator, domain, service, Some(entity)),
                    );
                }
            }
            if !matched {
                unresolved = true;
            }
        }
    }

    if let Some(area_ids) = target.area_id.as_ref() {
        saw_explicit_target = true;
        for area_id in area_ids {
            let mut matched = false;
            for entity in registry
                .entities
                .iter()
                .filter(|entity| entity.area_id.as_deref() == Some(area_id.as_str()))
            {
                matched = true;
                if resolved_ids.insert(entity.entity_id.clone()) {
                    saw_resolved_entity = true;
                    strictest = strictest_safety(
                        strictest,
                        classify_concrete_target(validator, domain, service, Some(entity)),
                    );
                }
            }
            if !matched {
                unresolved = true;
            }
        }
    }

    if unresolved || (saw_explicit_target && !saw_resolved_entity) {
        strictest = strictest_safety(strictest, SafetyClass::Sensitive);
    }
    strictest
}

fn classify_concrete_target(
    validator: &SafetyValidator,
    domain: &str,
    service: &str,
    entity: Option<&RegistryEntity>,
) -> SafetyClass {
    let entity_domain = entity.map(|entity| entity.domain.as_str());
    let device_class = entity.and_then(|entity| entity.device_class.as_deref());
    let class = validator.classify(&SafetyInput {
        domain,
        service,
        entity_domain,
        device_class,
    });
    if matches!(class, SafetyClass::Normal)
        && (domain == "cover" || entity_domain == Some("cover"))
        && device_class.is_none()
    {
        SafetyClass::Sensitive
    } else {
        class
    }
}

fn strictest_safety(left: SafetyClass, right: SafetyClass) -> SafetyClass {
    match (left, right) {
        (SafetyClass::Denied, _) | (_, SafetyClass::Denied) => SafetyClass::Denied,
        (SafetyClass::Sensitive, _) | (_, SafetyClass::Sensitive) => SafetyClass::Sensitive,
        (SafetyClass::Normal, SafetyClass::Normal) => SafetyClass::Normal,
    }
}

fn suppressed_outcome(code: &str, message: &str) -> ActionOutcome {
    ActionOutcome {
        activity_id: None,
        status: ActionStatus::Suppressed,
        error_code: Some(code.to_owned()),
        message: Some(message.to_owned()),
        ha_context_id: None,
        latency_ms: None,
    }
}

fn latency_breakdown(
    event: &GestureEvent,
    dispatch_ms: Option<f64>,
    ha_ms: Option<f64>,
) -> LatencyBreakdown {
    LatencyBreakdown {
        detect_ms: Some(event.fired_at.duration_since(event.onset_at).as_secs_f64() * 1_000.0),
        dispatch_ms,
        ha_ms,
    }
}

fn action_status_str(status: ActionStatus) -> &'static str {
    match status {
        ActionStatus::Sent => "sent",
        ActionStatus::Ok => "ok",
        ActionStatus::Error => "error",
        ActionStatus::Timeout => "timeout",
        ActionStatus::Stale => "stale",
        ActionStatus::Suppressed => "suppressed",
    }
}

fn suppression_reason_str(reason: SuppressionReason) -> &'static str {
    match reason {
        SuppressionReason::BelowThreshold => "below_threshold",
        SuppressionReason::VoteFailed => "vote_failed",
        SuppressionReason::Cooldown => "cooldown",
        SuppressionReason::NotArmed => "not_armed",
        SuppressionReason::NoMapping => "no_mapping",
        SuppressionReason::BlockedDomain => "blocked_domain",
        SuppressionReason::Paused => "paused",
        SuppressionReason::TooSmall => "too_small",
        SuppressionReason::Ambiguous => "ambiguous",
        SuppressionReason::TargetSelected => "target_selected",
        SuppressionReason::NoTarget => "no_target",
        SuppressionReason::AmbiguousTarget => "ambiguous_target",
        SuppressionReason::NeedsRealign => "needs_realign",
        SuppressionReason::StaleAction => "stale_action",
        SuppressionReason::ConfirmationRequired => "confirmation_required",
        SuppressionReason::QuietHours => "quiet_hours",
    }
}

fn hand_str(hand: Handedness) -> &'static str {
    match hand {
        Handedness::Left => "left",
        Handedness::Right => "right",
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn system_time_rfc3339(value: SystemTime) -> String {
    let Ok(duration) = value.duration_since(UNIX_EPOCH) else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(duration.as_secs() as i64) else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    datetime
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

/// Fixture anchor seed format for `tools/fixtures/targeting/*.anchors.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorSeed {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub entity: EntityState,
    #[serde(default)]
    pub levels: Vec<f64>,
}

impl TryFrom<AnchorSeed> for DispatcherAnchor {
    type Error = anyhow::Error;

    fn try_from(value: AnchorSeed) -> Result<Self, Self::Error> {
        Ok(Self {
            id: AnchorId::from_str(&value.id)?,
            name: value.name,
            domain: value.domain,
            entity: value.entity,
            levels: value.levels,
        })
    }
}

/// Returns the seeded owner fan anchor used by P1-1009 replay fixtures.
pub fn owner_fan_anchor() -> DispatcherAnchor {
    DispatcherAnchor {
        id: parse_anchor_id("01J00000000000000000000002"),
        name: "Ventilador Dormitorio".to_owned(),
        domain: "fan".to_owned(),
        entity: EntityState {
            entity_id: "fan.ventilador_dormitorio".to_owned(),
            state: "on".to_owned(),
            attributes: serde_json::Map::from_iter([
                ("friendly_name".to_owned(), json!("Ventilador Dormitorio")),
                ("supported_features".to_owned(), json!(53)),
                ("percentage".to_owned(), json!(1)),
                ("percentage_step".to_owned(), json!(1.0)),
                ("direction".to_owned(), json!("reverse")),
            ]),
            last_changed: None,
            last_updated: None,
        },
        levels: vec![1.0],
    }
}

/// Owner scenario mappings used by replay, HA E2E and dev mode.
pub fn owner_scenario_mappings() -> Vec<DispatcherMapping> {
    owner_scenario_mappings_for(owner_fan_anchor().id)
}

/// Owner scenario mappings using the supplied targeting anchor id.
pub fn owner_scenario_mappings_for(anchor: AnchorId) -> Vec<DispatcherMapping> {
    let thumb_mapping = parse_mapping_id("01J00000000000000000000010");
    let circle_mapping = parse_mapping_id("01J00000000000000000000011");
    let stop_mapping = parse_mapping_id("01J00000000000000000000012");
    vec![
        DispatcherMapping::global(
            thumb_mapping,
            "Thumbs up toggles bed light",
            GestureId::Builtin(BuiltinGesture::ThumbUp),
            Action::CallService {
                domain: "light".to_owned(),
                service: "toggle".to_owned(),
                target: ActionTarget {
                    entity_id: Some(vec!["light.bed_light".to_owned()]),
                    device_id: None,
                    area_id: None,
                },
                data: json!({}),
                preset: Some("light.toggle".to_owned()),
            },
        ),
        DispatcherMapping::anchor(
            circle_mapping,
            "Fan circle sets speed 1",
            GestureId::Builtin(BuiltinGesture::CircleAny),
            anchor,
            Action::Verb {
                verb: Verb::LevelSet,
                level: Some(1),
            },
        ),
        DispatcherMapping::anchor(
            stop_mapping,
            "Fan separate stops",
            GestureId::Builtin(BuiltinGesture::TwoHandSeparate),
            anchor,
            Action::Verb {
                verb: Verb::Stop,
                level: None,
            },
        ),
    ]
}

fn parse_anchor_id(value: &str) -> AnchorId {
    match AnchorId::from_str(value) {
        Ok(id) => id,
        Err(err) => panic!("invalid static anchor id {value}: {err}"),
    }
}

fn parse_mapping_id(value: &str) -> MappingId {
    match MappingId::from_str(value) {
        Ok(id) => id,
        Err(err) => panic!("invalid static mapping id {value}: {err}"),
    }
}
