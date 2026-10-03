//! Spatial targeting for Flick.
//!
//! This crate implements the math-only pieces from `docs/spec/09-device-targeting.md`:
//! camera intrinsics, hand/eye pointing rays, anchor triangulation, the target-selection
//! FSM, place matching, and Kabsch/Wahba re-alignment. Runtime crates own persistence,
//! Home Assistant calls and WebSocket emission; they can consume [`TargetEvent`] values
//! from [`TargetSelectorImpl::take_events`].
//!
//! # Public API
//! - [`Spatial`] / [`TargetSelectorImpl::new`] construct one selector per camera from
//!   [`CameraIntrinsics`], taught [`Anchor`] values and [`TargetSelectorSettings`].
//! - [`TargetSelectorImpl::update`] accepts a `flick_core::HandFrame` and optional
//!   `flick_core::FaceKeypoints`, returning `flick_core::SelectionState`.
//! - [`TargetSelectorImpl::selected`] exposes the selected `AnchorId` so gesture code can
//!   set `GestureEvent.target`.
//! - [`TeachSession`] collects [`TeachObservation`]s, triangulates `point3d` anchors or a
//!   `direction` fallback, reports quality/distinctiveness, and computes live angular
//!   error for the “Point at it again” test.
//! - [`PlaceMatcher`] matches 384-dimensional scene signatures and reports when targeting
//!   should pause for re-alignment.
//! - [`realign`] solves a rotation from re-pointed anchors and returns a residual.

mod anchors;
mod intrinsics;
mod math;
mod persistence;
mod places;
mod ray;
mod selector;

pub use anchors::{
    Anchor, AnchorGeometry, AnchorQuality, AnchorScore, DistinctivenessWarning, FingerAim,
    RayObservation, TeachObservation, TeachSession, TeachTarget, TeachingError, TeachingOutcome,
    VerbParams, angular_error_deg, recompute_anchor, seed_observations,
};
pub use intrinsics::{
    CameraFov, CameraIntrinsics, DEFAULT_INTRINSICS_VERSION, FOV_TABLE, IntrinsicsSource,
};
pub use persistence::{
    AnchorObservationRecord, AnchorRecord, InMemoryTargetingStore, PlaceRecord, PlaceStatus,
    StoredIntrinsics, TargetingStore, TargetingStoreError,
};
pub use places::{
    PlaceDescriptor, PlaceMatch, PlaceMatcher, PlaceMatcherSettings, RealignError, RealignPair,
    RealignResult, Similarity, Transform3, needs_realign, realign,
};
pub use ray::{
    DEFAULT_ESTIMATOR_VERSION, DominantEye, HandPose, PointingRay, RayEstimateError, RayEstimator,
    RayEstimatorSettings, RayModel, RaySource,
};
pub use selector::{
    Spatial, TargetClearReason, TargetEvent, TargetSelectorImpl, TargetSelectorSettings,
    is_point_pose,
};
