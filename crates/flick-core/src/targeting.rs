//! Device targeting and entitlement types.

use serde::{Deserialize, Serialize};

use crate::{AnchorId, CameraId, GestureEventId, HaInstanceId, MappingId};

/// Device targeting state for one camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SelectionState {
    /// No point pose or no anchors are active.
    Idle,
    /// Point pose is active but no anchor passes the hover test.
    Aiming,
    /// A candidate anchor is being dwelled on.
    Hover {
        /// Hovered anchor id.
        anchor_id: AnchorId,
        /// Selection score in `[0, 1]`.
        score: f32,
        /// Dwell progress in `[0, 1]`.
        dwell_progress: f32,
        /// Optional runner-up anchor id.
        runner_up: Option<AnchorId>,
    },
    /// A target is selected for verb gestures.
    Selected {
        /// Selected anchor id.
        anchor_id: AnchorId,
        /// Resolution domain such as `fan` or `light`.
        domain: String,
        /// Unix epoch milliseconds when the selection expires.
        expires_at_ms: i64,
    },
}

/// Core and Pro feature switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProFeature {
    /// More than the Core camera limits.
    Cameras,
    /// Multi-camera anchor fusion.
    Fusion,
    /// Far-field body gestures.
    Farfield,
    /// Smart Context action planning.
    Context,
    /// Headless LAN management.
    Headless,
    /// Included cloud setup assistant.
    Assist,
}

/// Camera limits enforced by entitlements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraLimits {
    /// Maximum local cameras.
    pub local: u32,
    /// Maximum RTSP cameras.
    pub rtsp: u32,
}

impl CameraLimits {
    /// Core entitlement: one local camera and one RTSP camera.
    pub const CORE: Self = Self { local: 1, rtsp: 1 };
}

/// Context supplied to an [`crate::ActionPlanner`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannerContext {
    /// Current camera id.
    pub camera_id: CameraId,
    /// Active Home Assistant instance.
    pub ha_id: Option<HaInstanceId>,
    /// Current selected anchor, if any.
    pub selected_anchor: Option<AnchorId>,
    /// Recent candidate event id.
    pub event_id: GestureEventId,
    /// Candidate mapping ids available to choose from.
    pub candidates: Vec<MappingId>,
}

/// Anchor persistence status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    /// Anchor is valid for the active place.
    Ok,
    /// Camera/place moved and the anchor needs realignment.
    NeedsRealign,
    /// Observations are not sufficient and the anchor needs re-teaching.
    NeedsReteach,
}

/// Anchor geometry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    /// A triangulated 3D point.
    Point3d,
    /// A direction valid near the teaching origin.
    Direction,
    /// A 2D region projected into 3D in a later phase.
    Region2d,
}
