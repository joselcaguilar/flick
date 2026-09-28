//! Dial absolute-value computation and 8 Hz coalescing.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use flick_core::{Action, ActionOutcome, ActionStatus, ActionTarget, DialProperty};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::HaClient;

const DIAL_PERIOD: Duration = Duration::from_millis(125);

/// A sink that can send dial-generated concrete actions.
#[async_trait]
pub trait DialCallSink: Send + Sync {
    /// Sends one concrete dial action.
    async fn call_action(&self, action: Action) -> ActionOutcome;
}

#[async_trait]
impl DialCallSink for HaClient {
    async fn call_action(&self, action: Action) -> ActionOutcome {
        self.call(action).await
    }
}

/// Dial target and scaling settings.
#[derive(Debug, Clone, PartialEq)]
pub struct DialTarget {
    /// HA entity id.
    pub entity_id: String,
    /// HA property being changed.
    pub property: DialProperty,
    /// Base value captured at dial start.
    pub base_value: Option<f64>,
    /// Scaling factor applied to deltas.
    pub gain: f64,
    /// Optional lower bound.
    pub min: Option<f64>,
    /// Optional upper bound.
    pub max: Option<f64>,
}

/// Dial event fed by the gesture FSM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DialEvent {
    /// Gesture updated with a relative delta.
    Update(f64),
    /// Gesture ended and the final delta must be sent.
    End(f64),
}

/// Background coalescer that sends at most 8 calls/s plus the final End value.
pub struct DialController {
    tx: mpsc::Sender<DialEvent>,
}

impl DialController {
    /// Starts a dial coalescer.
    #[must_use]
    pub fn start(target: DialTarget, sink: Arc<dyn DialCallSink>) -> Self {
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(run_dial(target, sink, rx));
        Self { tx }
    }

    /// Queues an update or end event.
    pub async fn send(&self, event: DialEvent) -> Result<(), mpsc::error::SendError<DialEvent>> {
        self.tx.send(event).await
    }
}

async fn run_dial(
    target: DialTarget,
    sink: Arc<dyn DialCallSink>,
    mut rx: mpsc::Receiver<DialEvent>,
) {
    let mut latest = None;
    let mut ticker = tokio::time::interval(DIAL_PERIOD);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event {
                    DialEvent::Update(delta) => latest = Some(delta),
                    DialEvent::End(delta) => {
                        send_delta(&target, sink.as_ref(), delta).await;
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                if let Some(delta) = latest.take() {
                    send_delta(&target, sink.as_ref(), delta).await;
                }
            }
            else => break,
        }
    }
}

async fn send_delta(target: &DialTarget, sink: &dyn DialCallSink, delta: f64) {
    let action = dial_action(target, delta);
    let _ = sink.call_action(action).await;
}

/// Builds the concrete action for a dial delta.
#[must_use]
pub fn dial_action(target: &DialTarget, delta: f64) -> Action {
    if let Some(base) = target.base_value {
        let value = clamp(base + target.gain * delta, target.min, target.max);
        return absolute_action(target, value);
    }
    relative_action(target, delta)
}

fn absolute_action(target: &DialTarget, value: f64) -> Action {
    let (domain, service, data) = match target.property {
        DialProperty::BrightnessPct => (
            "light",
            "turn_on",
            json!({ "brightness_pct": value.round() }),
        ),
        DialProperty::VolumeLevel => (
            "media_player",
            "volume_set",
            json!({ "volume_level": value }),
        ),
        DialProperty::Position => (
            "cover",
            "set_cover_position",
            json!({ "position": value.round() }),
        ),
        DialProperty::Percentage => (
            "fan",
            "set_percentage",
            json!({ "percentage": value.round() }),
        ),
        DialProperty::Temperature => (
            "climate",
            "set_temperature",
            json!({ "temperature": value }),
        ),
    };
    call(domain, service, target, data)
}

fn relative_action(target: &DialTarget, delta: f64) -> Action {
    match target.property {
        DialProperty::BrightnessPct => call(
            "light",
            "turn_on",
            target,
            json!({ "brightness_step_pct": if delta >= 0.0 { 10 } else { -10 } }),
        ),
        DialProperty::VolumeLevel => call(
            "media_player",
            if delta >= 0.0 {
                "volume_up"
            } else {
                "volume_down"
            },
            target,
            json!({}),
        ),
        DialProperty::Position => call(
            "cover",
            if delta >= 0.0 {
                "open_cover"
            } else {
                "close_cover"
            },
            target,
            json!({}),
        ),
        DialProperty::Percentage => call(
            "fan",
            if delta >= 0.0 {
                "increase_speed"
            } else {
                "decrease_speed"
            },
            target,
            json!({}),
        ),
        DialProperty::Temperature => call(
            "climate",
            "set_temperature",
            target,
            json!({ "temperature": delta }),
        ),
    }
}

fn call(domain: &str, service: &str, target: &DialTarget, data: Value) -> Action {
    Action::CallService {
        domain: domain.to_owned(),
        service: service.to_owned(),
        target: ActionTarget {
            entity_id: Some(vec![target.entity_id.clone()]),
            device_id: None,
            area_id: None,
        },
        data,
        preset: None,
    }
}

fn clamp(value: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let lower = min.unwrap_or(f64::NEG_INFINITY);
    let upper = max.unwrap_or(f64::INFINITY);
    value.clamp(lower, upper)
}

/// Success outcome used by tests for recording sinks.
#[must_use]
pub fn sent_outcome() -> ActionOutcome {
    ActionOutcome {
        activity_id: None,
        status: ActionStatus::Sent,
        error_code: None,
        message: None,
        ha_context_id: None,
        latency_ms: None,
    }
}
