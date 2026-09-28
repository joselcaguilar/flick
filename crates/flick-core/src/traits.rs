//! Normative extension traits implemented by engine components and Pro crates.

use std::sync::Arc;

use async_trait::async_trait;
use smallvec::SmallVec;

use crate::{
    ActionOutcome, ActivePack, AnchorId, CameraLimits, CaptureError, FaceKeypoints, Frame,
    GestureCandidate, GestureEvent, HandFrame, MappingId, PackGeneration, PackKind, PlannerContext,
    ProFeature, ResolvedAction, SelectionState, SourceInfo, VisionError,
};

/// A blocking camera or replay source run on a dedicated capture thread.
pub trait FrameSource: Send + 'static {
    /// Returns stable metadata for this source.
    fn info(&self) -> &SourceInfo;

    /// Produces the next frame. This is blocking and never called on the async runtime.
    fn next_frame(&mut self) -> Result<Frame, CaptureError>;

    /// Sets the source's idle/active target FPS.
    fn set_target_fps(&mut self, _fps: u32) {}
}

/// A per-camera hand perception pipeline.
pub trait HandPipeline: Send + 'static {
    /// Processes one frame into tracked hand observations.
    fn process(&mut self, frame: &Frame) -> Result<HandFrame, VisionError>;
}

/// A per-frame recognizer that feeds the trigger FSM.
pub trait GestureRecognizer: Send + 'static {
    /// Stable recognizer id used in metrics and logs.
    fn id(&self) -> &'static str;

    /// Updates recognizer state with the latest hands and returns candidates.
    fn update(&mut self, hands: &HandFrame) -> SmallVec<[GestureCandidate; 4]>;
}

/// An action sink, normally the Home Assistant client.
#[async_trait]
pub trait ActionSink: Send + Sync + 'static {
    /// Executes a concrete resolved action and returns the outcome.
    async fn execute(&self, action: &ResolvedAction) -> ActionOutcome;
}

/// Device targeting selector for one camera.
pub trait TargetSelector: Send + 'static {
    /// Updates the selection state from hands and optional face keypoints.
    fn update(&mut self, hands: &HandFrame, face: Option<&FaceKeypoints>) -> SelectionState;

    /// Returns the currently selected anchor, if any.
    fn selected(&self) -> Option<AnchorId>;

    /// Refreshes the selection window after a targeted verb fires.
    fn refresh(&mut self);

    /// Clears the current selection.
    fn clear(&mut self);
}

/// Registry for active model and catalog packs.
pub trait PackRegistry: Send + Sync + 'static {
    /// Returns the active pack for a kind. The clone must be cheap.
    fn active(&self, kind: PackKind) -> Arc<ActivePack>;

    /// Subscribes to active-pack generation changes.
    fn subscribe(&self) -> tokio::sync::watch::Receiver<PackGeneration>;
}

/// Phase 4 Smart Context planner. The core default chooses the first candidate.
#[async_trait]
pub trait ActionPlanner: Send + Sync + 'static {
    /// Chooses one mapping from the dispatcher candidates.
    async fn choose(
        &self,
        _event: &GestureEvent,
        candidates: &[MappingId],
        _ctx: &PlannerContext,
    ) -> Option<MappingId> {
        candidates.first().copied()
    }
}

/// Core/Pro entitlement boundary.
pub trait Entitlements: Send + Sync + 'static {
    /// Maximum camera counts allowed.
    fn max_cameras(&self) -> CameraLimits;

    /// Whether a Pro feature is enabled.
    fn has(&self, feature: ProFeature) -> bool;
}

/// Core entitlement implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoreEntitlements;

impl Entitlements for CoreEntitlements {
    fn max_cameras(&self) -> CameraLimits {
        CameraLimits::CORE
    }

    fn has(&self, _feature: ProFeature) -> bool {
        false
    }
}
