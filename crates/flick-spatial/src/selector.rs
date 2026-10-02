use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flick_core::{
    AnchorId, AnchorStatus, CameraId, FaceKeypoints, HandFrame, HandObservation, SelectionState,
    TargetSelector,
};
use serde::{Deserialize, Serialize};

use crate::{
    Anchor, AnchorScore, CameraIntrinsics, PointingRay, RayEstimator, RayEstimatorSettings,
    RayModel, RaySource,
    anchors::{score_anchor, score_geometry},
    math::{angle_deg, v3},
};

/// Default point-pose stable duration.
const POINT_STABLE: Duration = Duration::from_millis(200);
/// Point-pose loss duration that returns Aiming to Idle.
const POINT_LOST: Duration = Duration::from_millis(300);
/// Maximum angular speed allowed while reselecting from Selected.
const RESELECT_MAX_ANGULAR_SPEED_DEG_S: f32 = 60.0;
/// A noisy ray that briefly leaves the anchor keeps the dwell running instead of restarting it.
const HOVER_GRACE: Duration = Duration::from_millis(200);
/// Hover tolerance bounds when an eye-rooted anchor is aimed at with its finger-only fallback.
const FINGER_FALLBACK_MIN_TOLERANCE_DEG: f32 = 15.0;
const FINGER_FALLBACK_MAX_TOLERANCE_DEG: f32 = 20.0;

/// Settings for the target-selection FSM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetSelectorSettings {
    /// Whether targeting is enabled.
    pub enabled: bool,
    /// Base hover tolerance in degrees.
    pub tolerance_deg: f32,
    /// Required margin over runner-up in degrees.
    pub margin_deg: f32,
    /// Dwell duration to select.
    pub dwell: Duration,
    /// Selection window after dwell or verb refresh.
    pub window: Duration,
    /// Ray-estimator tuning. Each anchor is aimed at with the ray model it was taught with, or
    /// its finger-only fallback without a face, so `ray.model` is ignored here.
    pub ray: RayEstimatorSettings,
}

impl Default for TargetSelectorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            tolerance_deg: 10.0,
            margin_deg: 5.0,
            dwell: Duration::from_millis(500),
            window: Duration::from_millis(4000),
            ray: RayEstimatorSettings::default(),
        }
    }
}

/// Reason for a `target.cleared` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetClearReason {
    /// Selection window elapsed.
    Timeout,
    /// Hand/point pose was lost before selection.
    HandLost,
    /// Another anchor was selected after full dwell.
    Reselected,
    /// Engine or targeting was paused/cleared.
    Paused,
}

/// Data needed by runtime crates to emit `target.*` events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TargetEvent {
    /// `target.hover` payload.
    Hover {
        /// Camera id.
        camera_id: CameraId,
        /// Hovered anchor id.
        anchor_id: AnchorId,
        /// Anchor name.
        name: String,
        /// Selection score.
        score: f32,
        /// Dwell progress in `[0, 1]`.
        dwell_progress: f32,
        /// Optional runner-up.
        runner_up: Option<AnchorId>,
    },
    /// `target.selected` payload.
    Selected {
        /// Camera id.
        camera_id: CameraId,
        /// Selected anchor id.
        anchor_id: AnchorId,
        /// Anchor name.
        name: String,
        /// Resolution domain.
        domain: String,
        /// Unix epoch expiration time in milliseconds.
        expires_at_ms: i64,
    },
    /// `target.cleared` payload.
    Cleared {
        /// Camera id.
        camera_id: CameraId,
        /// Cleared anchor id.
        anchor_id: AnchorId,
        /// Clear reason.
        reason: TargetClearReason,
    },
    /// `target.ambiguous` payload.
    Ambiguous {
        /// Camera id.
        camera_id: CameraId,
        /// Best and runner-up anchor ids.
        anchor_ids: [AnchorId; 2],
    },
}

/// Per-camera spatial target selector.
#[derive(Debug, Clone)]
pub struct TargetSelectorImpl {
    anchors: Vec<Anchor>,
    settings: TargetSelectorSettings,
    eye_estimator: RayEstimator,
    finger_estimator: RayEstimator,
    selected: Option<Selected>,
    hover: Option<Hover>,
    point_started: Option<Instant>,
    point_track_id: Option<u32>,
    last_point_at: Option<Instant>,
    last_ray: Option<LastRay>,
    last_camera_id: Option<CameraId>,
    aim_error_deg: Option<f32>,
    events: Vec<TargetEvent>,
}

/// Compatibility alias for the crate-level spatial component.
pub type Spatial = TargetSelectorImpl;

impl TargetSelectorImpl {
    /// Creates a selector from camera intrinsics, anchors and settings.
    #[must_use]
    pub fn new(
        camera_intrinsics: CameraIntrinsics,
        anchors: Vec<Anchor>,
        settings: TargetSelectorSettings,
    ) -> Self {
        let eye_estimator = RayEstimator::new(
            camera_intrinsics.clone(),
            RayEstimatorSettings {
                model: RayModel::Eye,
                ..settings.ray.clone()
            },
        );
        let finger_estimator = RayEstimator::new(
            camera_intrinsics,
            RayEstimatorSettings {
                model: RayModel::Finger,
                ..settings.ray.clone()
            },
        );
        Self {
            anchors,
            settings,
            eye_estimator,
            finger_estimator,
            selected: None,
            hover: None,
            point_started: None,
            point_track_id: None,
            last_point_at: None,
            last_ray: None,
            last_camera_id: None,
            aim_error_deg: None,
            events: Vec::with_capacity(8),
        }
    }

    /// Replaces active anchors without resetting filters or the selected target.
    pub fn set_anchors(&mut self, anchors: Vec<Anchor>) {
        self.anchors = anchors;
        if let Some(selected) = self.selected.as_ref()
            && !self
                .anchors
                .iter()
                .any(|anchor| anchor.id == selected.anchor_id)
        {
            self.selected = None;
            self.hover = None;
        }
    }

    /// Returns the active selected anchor if the selection has not been cleared by an update.
    #[must_use]
    pub fn selected(&self) -> Option<AnchorId> {
        self.selected.as_ref().map(|selected| selected.anchor_id)
    }

    /// Drains pending `target.*` events for the engine event bus.
    #[must_use]
    pub fn take_events(&mut self) -> Vec<TargetEvent> {
        let mut events = Vec::with_capacity(self.events.capacity());
        std::mem::swap(&mut events, &mut self.events);
        events
    }

    /// Angular error to the closest anchor in the latest update, when it aimed a pointing ray.
    #[must_use]
    pub const fn aim_error_deg(&self) -> Option<f32> {
        self.aim_error_deg
    }

    /// Updates the selection state from one processed hand frame and optional face keypoints.
    pub fn update(&mut self, hands: &HandFrame, face: Option<&FaceKeypoints>) -> SelectionState {
        self.last_camera_id = Some(hands.camera_id);
        self.aim_error_deg = None;
        let now = hands.captured_at;
        self.expire_selection(hands.camera_id, now);

        if !self.settings.enabled || self.anchors.is_empty() {
            self.clear_internal(Some(hands.camera_id), TargetClearReason::Paused);
            return SelectionState::Idle;
        }

        let Some(hand) = self.pointing_hand(hands) else {
            return self.handle_no_point(hands.camera_id, now);
        };
        self.observe_point_pose(hand.track_id, now);
        if !self.point_pose_stable(now) {
            return self.selected_state_or(SelectionState::Idle);
        }

        let rays = LiveRays {
            eye: face.and_then(|face| self.eye_estimator.estimate(hand, Some(face), now).ok()),
            finger: self.finger_estimator.estimate(hand, None, now).ok(),
        };
        let Some(aim) = rays.eye.as_ref().or(rays.finger.as_ref()) else {
            return self.selected_state_or(SelectionState::Aiming);
        };
        let angular_speed = self.angular_speed_deg_s(aim, now);
        self.last_ray = Some(LastRay {
            direction: aim.direction,
            source: aim.source,
            at: now,
        });

        let Some(candidate) = self.best_candidate(&rays) else {
            self.miss_hover(now);
            return self.selected_state_or(SelectionState::Aiming);
        };

        if candidate.ambiguous {
            self.events.push(TargetEvent::Ambiguous {
                camera_id: hands.camera_id,
                anchor_ids: [
                    candidate.anchor_id,
                    candidate.runner_up_anchor_id.unwrap_or(candidate.anchor_id),
                ],
            });
            self.hover = None;
            return self.selected_state_or(SelectionState::Aiming);
        }

        if let Some(mut selected) = self.selected.clone() {
            if selected.anchor_id == candidate.anchor_id {
                // Still aiming at it, so the window runs from the last aim, not the first.
                selected.expires_at = now + self.settings.window;
                let state = self.selection_state(&selected);
                self.selected = Some(selected);
                return state;
            }
            if angular_speed > RESELECT_MAX_ANGULAR_SPEED_DEG_S {
                self.hover = None;
                return self.selection_state(&selected);
            }
            return self.update_hover(hands.camera_id, now, candidate, true);
        }

        self.update_hover(hands.camera_id, now, candidate, false)
    }

    /// Refreshes the 4 s selection window after a targeted verb fires.
    pub fn refresh(&mut self) {
        if let Some(mut selected) = self.selected.take() {
            selected.expires_at = Instant::now() + self.settings.window;
            self.selected = Some(selected);
        }
    }

    /// Clears the current selection and hover state.
    pub fn clear(&mut self) {
        self.clear_internal(self.last_camera_id, TargetClearReason::Paused);
    }

    fn pointing_hand<'a>(&self, hands: &'a HandFrame) -> Option<&'a HandObservation> {
        hands
            .hands
            .iter()
            .filter(|hand| hand.presence >= 0.5 && is_point_pose(hand))
            .max_by(|a, b| {
                a.presence
                    .partial_cmp(&b.presence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    fn observe_point_pose(&mut self, track_id: u32, now: Instant) {
        if self.point_track_id != Some(track_id) {
            self.point_track_id = Some(track_id);
            self.point_started = Some(now);
        } else if self.point_started.is_none() {
            self.point_started = Some(now);
        }
        self.last_point_at = Some(now);
    }

    fn point_pose_stable(&self, now: Instant) -> bool {
        self.point_started
            .and_then(|started| elapsed(now, started))
            .is_some_and(|duration| duration >= POINT_STABLE)
    }

    fn handle_no_point(&mut self, camera_id: CameraId, now: Instant) -> SelectionState {
        self.point_started = None;
        self.point_track_id = None;
        if self.selected.is_some() {
            return self.selected_state_or(SelectionState::Idle);
        }
        if self
            .last_point_at
            .and_then(|last| elapsed(now, last))
            .is_some_and(|duration| duration < POINT_LOST)
        {
            return SelectionState::Aiming;
        }
        if let Some(hover) = self.hover {
            self.events.push(TargetEvent::Cleared {
                camera_id,
                anchor_id: hover.anchor_id,
                reason: TargetClearReason::HandLost,
            });
        }
        self.hover = None;
        SelectionState::Idle
    }

    fn expire_selection(&mut self, camera_id: CameraId, now: Instant) {
        if let Some(selected) = self.selected.clone()
            && now >= selected.expires_at
        {
            self.events.push(TargetEvent::Cleared {
                camera_id,
                anchor_id: selected.anchor_id,
                reason: TargetClearReason::Timeout,
            });
            self.selected = None;
            self.hover = None;
        }
    }

    fn best_candidate(&mut self, rays: &LiveRays) -> Option<Candidate> {
        let mut best: Option<Candidate> = None;
        let mut runner: Option<Candidate> = None;
        for anchor in &self.anchors {
            if anchor.status != AnchorStatus::Ok {
                continue;
            }
            let Some((score, tolerance)) = live_score(anchor, rays, self.settings.tolerance_deg)
            else {
                continue;
            };
            let candidate = Candidate {
                anchor_id: anchor.id,
                score,
                tolerance_deg: tolerance,
                runner_up_anchor_id: None,
                ambiguous: false,
            };
            if best.is_none_or(|b| score.angular_error_deg < b.score.angular_error_deg) {
                runner = best;
                best = Some(candidate);
            } else if runner.is_none_or(|r| score.angular_error_deg < r.score.angular_error_deg) {
                runner = Some(candidate);
            }
        }
        let mut best = best?;
        self.aim_error_deg = Some(best.score.angular_error_deg);
        if best.score.angular_error_deg > best.tolerance_deg {
            return None;
        }
        if let Some(runner) = runner {
            best.runner_up_anchor_id = Some(runner.anchor_id);
            let margin = runner.score.angular_error_deg - best.score.angular_error_deg;
            best.ambiguous = margin < self.settings.margin_deg;
        }
        Some(best)
    }

    fn miss_hover(&mut self, now: Instant) {
        if self
            .hover
            .and_then(|hover| elapsed(now, hover.last_hit_at))
            .is_some_and(|gap| gap >= HOVER_GRACE)
        {
            self.hover = None;
        }
    }

    fn update_hover(
        &mut self,
        camera_id: CameraId,
        now: Instant,
        candidate: Candidate,
        _reselecting: bool,
    ) -> SelectionState {
        match self.hover.as_mut() {
            Some(hover) if hover.anchor_id == candidate.anchor_id => hover.last_hit_at = now,
            _ => {
                self.hover = Some(Hover {
                    anchor_id: candidate.anchor_id,
                    started_at: now,
                    last_hit_at: now,
                });
            }
        }
        let Some(hover) = self.hover else {
            return SelectionState::Aiming;
        };
        let progress = elapsed(now, hover.started_at)
            .map(|duration| duration.as_secs_f32() / self.settings.dwell.as_secs_f32())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        if let Some(anchor) = self.anchor(candidate.anchor_id) {
            self.events.push(TargetEvent::Hover {
                camera_id,
                anchor_id: candidate.anchor_id,
                name: anchor.name.clone(),
                score: candidate.score.score,
                dwell_progress: progress,
                runner_up: candidate.runner_up_anchor_id,
            });
        }
        if progress >= 1.0 {
            if let Some(previous) = self.selected.as_ref() {
                self.events.push(TargetEvent::Cleared {
                    camera_id,
                    anchor_id: previous.anchor_id,
                    reason: TargetClearReason::Reselected,
                });
            }
            let expires_at = now + self.settings.window;
            let domain = self
                .anchor(candidate.anchor_id)
                .map(|anchor| anchor.domain.clone())
                .unwrap_or_default();
            self.selected = Some(Selected {
                anchor_id: candidate.anchor_id,
                domain: domain.clone(),
                expires_at,
            });
            self.hover = None;
            if let Some(anchor) = self.anchor(candidate.anchor_id) {
                self.events.push(TargetEvent::Selected {
                    camera_id,
                    anchor_id: candidate.anchor_id,
                    name: anchor.name.clone(),
                    domain,
                    expires_at_ms: epoch_ms_for_instant(expires_at),
                });
            }
            return SelectionState::Selected {
                anchor_id: candidate.anchor_id,
                domain: self
                    .anchor(candidate.anchor_id)
                    .map(|anchor| anchor.domain.clone())
                    .unwrap_or_default(),
                expires_at_ms: epoch_ms_for_instant(expires_at),
            };
        }
        SelectionState::Hover {
            anchor_id: candidate.anchor_id,
            score: candidate.score.score,
            dwell_progress: progress,
            runner_up: candidate.runner_up_anchor_id,
        }
    }

    fn selected_state_or(&self, fallback: SelectionState) -> SelectionState {
        self.selected
            .as_ref()
            .map(|selected| self.selection_state(selected))
            .unwrap_or(fallback)
    }

    fn selection_state(&self, selected: &Selected) -> SelectionState {
        SelectionState::Selected {
            anchor_id: selected.anchor_id,
            domain: selected.domain.clone(),
            expires_at_ms: epoch_ms_for_instant(selected.expires_at),
        }
    }

    fn clear_internal(&mut self, camera_id: Option<CameraId>, reason: TargetClearReason) {
        if let Some(selected) = self.selected.as_ref()
            && let Some(camera_id) = camera_id
        {
            self.events.push(TargetEvent::Cleared {
                camera_id,
                anchor_id: selected.anchor_id,
                reason,
            });
        }
        self.selected = None;
        self.hover = None;
    }

    fn anchor(&self, id: AnchorId) -> Option<&Anchor> {
        self.anchors.iter().find(|anchor| anchor.id == id)
    }

    fn angular_speed_deg_s(&self, ray: &PointingRay, now: Instant) -> f32 {
        let Some(last) = self.last_ray.filter(|last| last.source == ray.source) else {
            return 0.0;
        };
        let Some(dt) = elapsed(now, last.at).map(|duration| duration.as_secs_f32()) else {
            return 0.0;
        };
        if dt <= 1.0e-4 {
            0.0
        } else {
            angle_deg(v3(last.direction), v3(ray.direction)) / dt
        }
    }
}

impl TargetSelector for TargetSelectorImpl {
    fn update(&mut self, hands: &HandFrame, face: Option<&FaceKeypoints>) -> SelectionState {
        TargetSelectorImpl::update(self, hands, face)
    }

    fn selected(&self) -> Option<AnchorId> {
        TargetSelectorImpl::selected(self)
    }

    fn refresh(&mut self) {
        TargetSelectorImpl::refresh(self);
    }

    fn clear(&mut self) {
        TargetSelectorImpl::clear(self);
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    anchor_id: AnchorId,
    score: AnchorScore,
    tolerance_deg: f32,
    runner_up_anchor_id: Option<AnchorId>,
    ambiguous: bool,
}

#[derive(Debug, Clone)]
struct Selected {
    anchor_id: AnchorId,
    domain: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct Hover {
    anchor_id: AnchorId,
    started_at: Instant,
    last_hit_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct LastRay {
    direction: [f32; 3],
    source: RaySource,
    at: Instant,
}

/// This frame's ray from each model; eye-rooted needs a visible face.
struct LiveRays {
    eye: Option<PointingRay>,
    finger: Option<PointingRay>,
}

impl LiveRays {
    fn for_source(&self, source: RaySource) -> Option<&PointingRay> {
        match source {
            RaySource::EyeRooted => self.eye.as_ref(),
            RaySource::FingerOnly => self.finger.as_ref(),
        }
    }
}

fn tolerance_for(uncertainty_deg: f32, base: f32) -> f32 {
    (base + uncertainty_deg.max(0.0)).clamp(base, 15.0)
}

/// Scores an anchor with the ray model it was taught with. A ray from the other model misses
/// by tens of degrees, so it is never substituted; without a face, an eye-rooted anchor is
/// aimed at with the finger-only model taught alongside it instead.
fn live_score(anchor: &Anchor, rays: &LiveRays, base_tolerance: f32) -> Option<(AnchorScore, f32)> {
    if let Some(ray) = rays.for_source(anchor.ray_source) {
        return Some((
            score_anchor(anchor, ray),
            tolerance_for(anchor.uncertainty_deg, base_tolerance),
        ));
    }
    let fallback = anchor.finger_aim.as_ref()?;
    let ray = rays.finger.as_ref()?;
    // Finger-only rays are noisier, so the fallback gets a wider tolerance.
    let tolerance = (base_tolerance + fallback.uncertainty_deg.max(0.0)).clamp(
        FINGER_FALLBACK_MIN_TOLERANCE_DEG,
        FINGER_FALLBACK_MAX_TOLERANCE_DEG,
    );
    Some((
        score_geometry(&fallback.geometry, fallback.uncertainty_deg, ray),
        tolerance,
    ))
}

/// Returns true when the hand shows the index-finger pointing pose used for targeting.
#[must_use]
pub fn is_point_pose(hand: &HandObservation) -> bool {
    finger_extended(hand, 5, 6, 7, 8)
        && finger_curled(hand, 9, 10, 12)
        && finger_curled(hand, 13, 14, 16)
        && finger_curled(hand, 17, 18, 20)
}

fn finger_extended(hand: &HandObservation, mcp: usize, pip: usize, dip: usize, tip: usize) -> bool {
    joint_angle(hand, mcp, pip, dip) >= 160.0 && joint_angle(hand, pip, dip, tip) >= 160.0
}

fn finger_curled(hand: &HandObservation, _mcp: usize, pip: usize, tip: usize) -> bool {
    let wrist = landmark(hand, 0);
    let pip = landmark(hand, pip);
    let tip = landmark(hand, tip);
    (tip - wrist).norm() < (pip - wrist).norm()
}

fn joint_angle(hand: &HandObservation, a: usize, b: usize, c: usize) -> f32 {
    let va = landmark(hand, a) - landmark(hand, b);
    let vc = landmark(hand, c) - landmark(hand, b);
    angle_deg(va, vc)
}

fn landmark(hand: &HandObservation, idx: usize) -> nalgebra::Vector3<f32> {
    nalgebra::Vector3::new(hand.image[idx][0], hand.image[idx][1], hand.image[idx][2])
}

fn elapsed(now: Instant, then: Instant) -> Option<Duration> {
    now.checked_duration_since(then)
}

fn epoch_ms_for_instant(instant: Instant) -> i64 {
    let system_now = SystemTime::now();
    let instant_now = Instant::now();
    let system = if instant >= instant_now {
        system_now + instant.duration_since(instant_now)
    } else {
        system_now
            .checked_sub(instant_now.duration_since(instant))
            .unwrap_or(UNIX_EPOCH)
    };
    match system.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}
