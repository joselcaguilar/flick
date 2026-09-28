//! Gesture recognition, replay and training primitives for Flick.
//!
//! The public entry point is [`GestureEngine`]. It owns the built-in Tier 0
//! recognizer, motion recognizers and the [`TriggerFsmSet`] for one camera
//! stream. Engine callers feed one [`flick_core::HandFrame`] at a time and the
//! current [`flick_core::SelectionState`]:
//!
//! ```ignore
//! let (events, candidates) = engine.update(&hand_frame, &selection_state);
//! ```
//!
//! `candidates` are suitable for HUD/Studio confidence displays. `events` are
//! debounced [`flick_core::GestureEvent`] values whose `target` field is filled
//! whenever the supplied selection state is `Selected`.
//!
//! The crate also exposes reusable pieces for the Studio and future engine
//! runner:
//! - [`replay`] reads the JSONL HandFrame fixture format and compares events to
//!   `*.expected.json` files.
//! - [`ProtoKnn`] trains few-shot static custom gestures from embeddings or
//!   normalized landmarks and returns LOTO quality/confusion metrics.
//! - [`MotionTemplateSet`] builds custom motion/two-hand DTW templates from
//!   recorded takes and reports held-out quality.
//! - [`TriggerFsmSet`] can be tested or embedded independently of recognizers.

pub mod engine;
pub mod features;
pub mod fsm;
pub mod motion;
pub mod proto;
pub mod replay;
pub mod tier0;

pub use engine::{EngineUpdate, GestureEngine, GestureEngineConfig, UiCandidate};
pub use features::{FeatureVector, FEATURE_VERSION_EMBEDDING, FEATURE_VERSION_LANDMARKS};
pub use fsm::{
    ArmConfig, FsmUpdate, GestureMapping, HandConstraint, PauseGestureConfig, SuppressedCandidate,
    TargetMode, TriggerConfig, TriggerFsmSet, TriggerMode,
};
pub use motion::{
    AutoGestureKind, BuiltinMotionRecognizer, MotionAxis, MotionConfig, MotionQualityReport,
    MotionTake, MotionTemplate, MotionTemplateSet, TemplateMatch,
};
pub use proto::{
    ClassMetrics, Confusion, LabeledSample, ProtoKnn, ProtoKnnConfig, ProtoKnnReport,
    ProtoPrediction,
};
pub use replay::{
    ExpectedEvent, HandFrameRecord, ReplayError, ReplayOutcome, ReplayRunner, compare_expected,
    read_jsonl_str,
};
pub use tier0::{
    CannedClassifierScorer, GeometricLandmarkScorer, Tier0Config, Tier0Recognizer, Tier0Score,
    Tier0Scorer,
};
