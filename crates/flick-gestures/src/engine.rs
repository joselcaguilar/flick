//! Public gesture-engine facade.

use flick_core::{
    BuiltinGesture, GestureCandidate, GestureEvent, GestureId, HandFrame, SelectionState,
};
use smallvec::SmallVec;

use crate::{
    fsm::{FsmUpdate, GestureMapping, TargetMode, TriggerConfig, TriggerFsmSet, TriggerMode},
    motion::{BuiltinMotionRecognizer, MotionAxis, MotionConfig},
    tier0::{GeometricLandmarkScorer, Tier0Config, Tier0Recognizer},
};

/// Candidate summary for UI confidence rings and Studio live testing.
#[derive(Debug, Clone, PartialEq)]
pub struct UiCandidate {
    /// Candidate gesture id.
    pub gesture_id: GestureId,
    /// Track id that produced the candidate.
    pub track_id: u32,
    /// Confidence in `[0, 1]`.
    pub confidence: f32,
    /// Vote or recognizer progress in `[0, 1]`.
    pub progress: f32,
    /// Optional dial value.
    pub value: Option<f32>,
}

/// Result of updating [`GestureEngine`] with one frame.
#[derive(Debug, Clone, Default)]
pub struct EngineUpdate {
    /// Debounced gesture events.
    pub events: Vec<GestureEvent>,
    /// Candidates suitable for UI display.
    pub candidates: Vec<UiCandidate>,
    /// Suppression information for the debug view.
    pub fsm: FsmUpdate,
}

/// Configuration for one camera's gesture engine.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureEngineConfig {
    /// Tier 0 recognizer configuration.
    pub tier0: Tier0Config,
    /// Built-in motion recognizer configuration.
    pub motion: MotionConfig,
    /// Trigger FSM configuration and mappings.
    pub trigger: TriggerConfig,
}

impl Default for GestureEngineConfig {
    fn default() -> Self {
        Self {
            tier0: Tier0Config::default(),
            motion: MotionConfig::default(),
            trigger: TriggerConfig {
                mappings: builtin_replay_mappings(),
                ..TriggerConfig::default()
            },
        }
    }
}

impl GestureEngineConfig {
    /// Builds config from the settings JSON keys owned by this crate.
    #[must_use]
    pub fn from_settings(settings: &serde_json::Value, mappings: Vec<GestureMapping>) -> Self {
        let mut config = Self::default();
        if let Some(vote) = settings.get("detection.vote") {
            if let Some(n) = vote.get("n").and_then(serde_json::Value::as_u64) {
                config.trigger.vote_n = n as usize;
            }
            if let Some(m) = vote.get("m").and_then(serde_json::Value::as_u64) {
                config.trigger.vote_m = m as usize;
            }
            if let Some(s) = vote.get("selected_n").and_then(serde_json::Value::as_u64) {
                config.trigger.selected_vote_n = s as usize;
            }
        }
        if let Some(axis) = settings
            .get("gestures.two_hand_separate.axis")
            .and_then(serde_json::Value::as_str)
        {
            config.motion.two_hand_axis = match axis {
                "vertical" => MotionAxis::Vertical,
                "horizontal" => MotionAxis::Horizontal,
                _ => MotionAxis::Any,
            };
        }
        if let Some(params) = settings.get("gestures.params")
            && let Some(threshold) = params
                .get("tier0_threshold")
                .and_then(serde_json::Value::as_f64)
        {
            config.tier0.threshold = threshold as f32;
        }
        config.trigger.mappings = mappings;
        config
    }
}

/// Per-camera gesture engine.
#[derive(Debug, Clone)]
pub struct GestureEngine {
    tier0: Tier0Recognizer<GeometricLandmarkScorer>,
    motion: BuiltinMotionRecognizer,
    fsm: TriggerFsmSet,
    latched_motion: Vec<LatchedCandidate>,
    latch_frames: usize,
}

impl GestureEngine {
    /// Creates an engine from config.
    #[must_use]
    pub fn new(config: GestureEngineConfig) -> Self {
        let latch_frames = config.trigger.vote_n.max(1);
        Self {
            tier0: Tier0Recognizer::new(GeometricLandmarkScorer, config.tier0),
            motion: BuiltinMotionRecognizer::new(config.motion),
            fsm: TriggerFsmSet::new(config.trigger),
            latched_motion: Vec::new(),
            latch_frames,
        }
    }

    /// Updates recognizers and FSM for one frame.
    #[must_use]
    pub fn update(&mut self, frame: &HandFrame, selection: &SelectionState) -> EngineUpdate {
        let mut candidates: SmallVec<[GestureCandidate; 16]> = SmallVec::new();
        candidates.extend(self.tier0.update_frame(frame));
        let motion_candidates = self.motion.update_frame(frame);
        for candidate in motion_candidates {
            if is_latched_motion(candidate.gesture_id) {
                self.latched_motion.push(LatchedCandidate {
                    candidate: candidate.clone(),
                    remaining: self.latch_frames,
                });
            }
            candidates.push(candidate);
        }
        for latched in &mut self.latched_motion {
            if latched.remaining > 0 {
                candidates.push(latched.candidate.clone());
                latched.remaining -= 1;
            }
        }
        self.latched_motion.retain(|latched| latched.remaining > 0);

        let fsm = self.fsm.update(frame, &candidates, selection);
        EngineUpdate {
            events: fsm.events.clone(),
            candidates: candidates.iter().map(UiCandidate::from).collect(),
            fsm,
        }
    }
}

#[derive(Debug, Clone)]
struct LatchedCandidate {
    candidate: GestureCandidate,
    remaining: usize,
}

impl From<&GestureCandidate> for UiCandidate {
    fn from(candidate: &GestureCandidate) -> Self {
        Self {
            gesture_id: candidate.gesture_id,
            track_id: candidate.track_id,
            confidence: candidate.confidence,
            progress: candidate
                .progress
                .unwrap_or(candidate.confidence)
                .clamp(0.0, 1.0),
            value: candidate.value,
        }
    }
}

fn is_latched_motion(gesture_id: GestureId) -> bool {
    matches!(
        gesture_id,
        GestureId::Builtin(
            BuiltinGesture::SwipeLeft
                | BuiltinGesture::SwipeRight
                | BuiltinGesture::SwipeUp
                | BuiltinGesture::SwipeDown
                | BuiltinGesture::CircleCw
                | BuiltinGesture::CircleCcw
                | BuiltinGesture::TwoHandSeparate
        )
    )
}

fn builtin_replay_mappings() -> Vec<GestureMapping> {
    let mut mappings = Vec::new();
    for gesture in [
        BuiltinGesture::ClosedFist,
        BuiltinGesture::OpenPalm,
        BuiltinGesture::PointingUp,
        BuiltinGesture::ThumbUp,
        BuiltinGesture::ThumbDown,
        BuiltinGesture::Victory,
        BuiltinGesture::ILoveYou,
        BuiltinGesture::SwipeLeft,
        BuiltinGesture::SwipeRight,
        BuiltinGesture::SwipeUp,
        BuiltinGesture::SwipeDown,
        BuiltinGesture::CircleCw,
        BuiltinGesture::CircleCcw,
        BuiltinGesture::TwoHandSeparate,
    ] {
        let mut mapping = GestureMapping::tap(GestureId::Builtin(gesture));
        mapping.cooldown_ms = match gesture {
            BuiltinGesture::SwipeLeft
            | BuiltinGesture::SwipeRight
            | BuiltinGesture::SwipeUp
            | BuiltinGesture::SwipeDown => 600,
            BuiltinGesture::CircleCw | BuiltinGesture::CircleCcw => 700,
            BuiltinGesture::TwoHandSeparate => 800,
            _ => 1_000,
        };
        if matches!(gesture, BuiltinGesture::TwoHandSeparate) {
            mapping.allow_two_hands = true;
        }
        if matches!(
            gesture,
            BuiltinGesture::ThumbUp
                | BuiltinGesture::ThumbDown
                | BuiltinGesture::SwipeLeft
                | BuiltinGesture::SwipeRight
                | BuiltinGesture::SwipeUp
                | BuiltinGesture::SwipeDown
                | BuiltinGesture::CircleCw
                | BuiltinGesture::CircleCcw
                | BuiltinGesture::TwoHandSeparate
        ) {
            mapping.target_mode = TargetMode::Either;
        }
        mappings.push(mapping);
    }
    let mut dial = GestureMapping::tap(GestureId::Builtin(BuiltinGesture::PinchDial));
    dial.mode = TriggerMode::Dial;
    dial.cooldown_ms = 0;
    dial.allow_two_hands = true;
    dial.target_mode = TargetMode::Either;
    mappings.push(dial);

    let mut circle_any = GestureMapping::tap(GestureId::Builtin(BuiltinGesture::CircleAny));
    circle_any.target_mode = TargetMode::Either;
    circle_any.cooldown_ms = 700;
    mappings.push(circle_any);
    mappings
}
