//! Gesture candidates, fired events and suppression reasons.

use serde::{Deserialize, Serialize};

use crate::{AnchorId, CameraId, GestureEventId, GestureId, Handedness};

/// Phase of a gesture event emitted by the trigger FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GesturePhase {
    /// A discrete gesture fired, or a continuous gesture started.
    Fired,
    /// A continuous gesture produced an updated value.
    Update,
    /// A continuous gesture ended.
    End,
}

/// A per-frame recognizer candidate before trigger-FSM voting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GestureCandidate {
    /// Candidate gesture id.
    pub gesture_id: GestureId,
    /// Track id that produced the candidate.
    pub track_id: u32,
    /// Physical hand that produced the candidate.
    pub hand: Handedness,
    /// Classifier or recognizer confidence in `[0, 1]`.
    pub confidence: f32,
    /// Optional progress value in `[0, 1]` for UI rings.
    pub progress: Option<f32>,
    /// Optional continuous value for dial-like recognizers.
    pub value: Option<f32>,
}

/// A debounced gesture event ready for dispatch.
#[derive(Debug, Clone)]
pub struct GestureEvent {
    /// Event id for correlation with action results.
    pub id: GestureEventId,
    /// Flick camera id.
    pub camera_id: CameraId,
    /// Fired gesture id.
    pub gesture_id: GestureId,
    /// Physical hand that fired.
    pub hand: Handedness,
    /// Final confidence in `[0, 1]`.
    pub confidence: f32,
    /// Event phase.
    pub phase: GesturePhase,
    /// Optional continuous value.
    pub value: Option<f32>,
    /// Selected anchor when a targeted device is active.
    pub target: Option<AnchorId>,
    /// Monotonic onset timestamp.
    pub onset_at: std::time::Instant,
    /// Monotonic fired timestamp.
    pub fired_at: std::time::Instant,
}

/// Stable reason codes for suppressed gesture or action attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReason {
    /// Candidate score was below the configured threshold.
    BelowThreshold,
    /// N-of-M voting failed.
    VoteFailed,
    /// The mapping is still in cooldown.
    Cooldown,
    /// Arm mode is enabled and no arm window is open.
    NotArmed,
    /// No enabled mapping matched the gesture.
    NoMapping,
    /// The resolved domain or service is denied by policy.
    BlockedDomain,
    /// The engine or camera is paused.
    Paused,
    /// The hand was too small for reliable recognition.
    TooSmall,
    /// Competing candidates were too close to choose safely.
    Ambiguous,
    /// A selected target took precedence over a global mapping.
    TargetSelected,
    /// A targeted mapping existed but no device was selected.
    NoTarget,
    /// Multiple anchors were within the targeting margin.
    AmbiguousTarget,
    /// The active place or anchor needs re-alignment.
    NeedsRealign,
    /// A pending action became stale before dispatch.
    StaleAction,
    /// A sensitive action is waiting for confirmation.
    ConfirmationRequired,
    /// The action was blocked by quiet hours.
    QuietHours,
}
