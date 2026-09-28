//! Home Assistant action contracts and dispatcher results.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ActivityId, AnchorId, GestureEventId, MappingId};

/// Device-independent targeted verb names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// Increase, brighten, open or otherwise move upward.
    Up,
    /// Decrease, dim, close or otherwise move downward.
    Down,
    /// Turn on or start playback.
    On,
    /// Turn off.
    Off,
    /// Stop motion or pause playback.
    Stop,
    /// Toggle the device.
    Toggle,
    /// Set a taught one-based level.
    LevelSet,
}

/// Supported dial target properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialProperty {
    /// Home Assistant brightness percent.
    BrightnessPct,
    /// Media player volume level.
    VolumeLevel,
    /// Cover position.
    Position,
    /// Fan percentage.
    Percentage,
    /// Climate temperature.
    Temperature,
}

/// Home Assistant service target object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionTarget {
    /// Entity ids to target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<Vec<String>>,
    /// Device ids to target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Vec<String>>,
    /// Area ids to target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_id: Option<Vec<String>>,
}

/// JSON action stored in mappings and accepted by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// A direct Home Assistant `call_service` action.
    CallService {
        /// Home Assistant domain.
        domain: String,
        /// Service name within the domain.
        service: String,
        /// Home Assistant target object.
        target: ActionTarget,
        /// Service data object.
        #[serde(default)]
        data: Value,
        /// Optional UI preset metadata ignored by the dispatcher.
        #[serde(skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
    },
    /// A continuous dial action against an entity or `$selected`.
    Dial {
        /// Entity id, or `$selected` for targeted mappings.
        entity_id: String,
        /// Property to modify.
        property: DialProperty,
        /// Scaling factor for value deltas.
        gain: f64,
        /// Optional lower bound.
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<Value>,
        /// Optional upper bound.
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<Value>,
    },
    /// A targeted verb resolved against the selected anchor.
    Verb {
        /// Device-independent verb.
        verb: Verb,
        /// One-based level for [`Verb::LevelSet`].
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<u32>,
    },
}

/// A concrete action after mapping, targeting and safety resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedAction {
    /// Mapping that produced the action.
    pub mapping_id: MappingId,
    /// Gesture event that produced the action.
    pub event_id: GestureEventId,
    /// Selected anchor, if any.
    pub anchor_id: Option<AnchorId>,
    /// Concrete action to send.
    pub action: Action,
    /// Short human-readable summary for the activity log.
    pub summary: String,
    /// Monotonic deadline represented as milliseconds since event firing for stale guard.
    pub stale_after_ms: u64,
}

/// Action execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    /// The action was sent to Home Assistant.
    Sent,
    /// Home Assistant acknowledged success.
    Ok,
    /// Home Assistant returned an error.
    Error,
    /// The request timed out.
    Timeout,
    /// The action was stale and dropped.
    Stale,
    /// The action was suppressed before send.
    Suppressed,
}

/// Result of sending a resolved action to an [`crate::ActionSink`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionOutcome {
    /// Activity row id.
    pub activity_id: Option<ActivityId>,
    /// Execution status.
    pub status: ActionStatus,
    /// Stable error code, when known.
    pub error_code: Option<String>,
    /// Redacted human-facing message.
    pub message: Option<String>,
    /// Home Assistant context id from successful calls.
    pub ha_context_id: Option<String>,
    /// End-to-end latency in milliseconds.
    pub latency_ms: Option<u64>,
}
