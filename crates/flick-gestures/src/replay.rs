//! Landmark JSONL replay harness.

use std::{path::Path, str::FromStr, time::Duration};

use flick_core::{
    CameraId, GestureEvent, GestureId, GesturePhase, HandFrame, HandObservation, SelectionState,
    StageTimings,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use thiserror::Error;

use crate::{EngineUpdate, GestureEngine, GestureEngineConfig};

/// One JSONL hand-frame record.
///
/// `t_ms` is an offset from the start of the fixture because `Instant` is not
/// serializable. Each line is independent and deterministic across platforms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandFrameRecord {
    /// Camera id for the frame.
    pub camera_id: CameraId,
    /// Sequence number.
    pub seq: u64,
    /// Milliseconds since the fixture start.
    pub t_ms: u64,
    /// Tracked hands.
    #[serde(default)]
    pub hands: Vec<HandObservation>,
    /// Optional stage timings.
    #[serde(default)]
    pub timings: StageTimings,
}

impl HandFrameRecord {
    /// Converts a record into a runtime [`HandFrame`].
    #[must_use]
    pub fn into_frame(&self, base: std::time::Instant) -> HandFrame {
        HandFrame {
            camera_id: self.camera_id,
            seq: self.seq,
            captured_at: base + Duration::from_millis(self.t_ms),
            hands: SmallVec::from_vec(self.hands.clone()),
            timings: self.timings,
        }
    }

    /// Converts a runtime frame to a JSONL record.
    #[must_use]
    pub fn from_frame(base: std::time::Instant, frame: &HandFrame) -> Self {
        Self {
            camera_id: frame.camera_id,
            seq: frame.seq,
            t_ms: frame.captured_at.duration_since(base).as_millis() as u64,
            hands: frame.hands.iter().cloned().collect(),
            timings: frame.timings,
        }
    }
}

/// Compact expected event used by `*.expected.json` files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedEvent {
    /// Event time in fixture milliseconds.
    pub t_ms: u64,
    /// Gesture id.
    pub gesture_id: GestureId,
    /// Event phase.
    pub phase: GesturePhase,
    /// Selected target anchor as a string, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Continuous value rounded to milli-units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_milli: Option<i32>,
}

/// Replay result.
#[derive(Debug, Clone, Default)]
pub struct ReplayOutcome {
    /// Observed event sequence.
    pub events: Vec<ExpectedEvent>,
    /// Last engine update for diagnostics.
    pub last_update: Option<EngineUpdate>,
}

/// Replay and comparison failures.
#[derive(Debug, Error)]
pub enum ReplayError {
    /// File I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON decoding failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Gesture id parsing failed.
    #[error("gesture id error: {0}")]
    GestureId(String),
    /// Expected and actual events differed.
    #[error("replay mismatch\nexpected: {expected:#?}\nactual: {actual:#?}")]
    Mismatch {
        /// Expected events.
        expected: Vec<ExpectedEvent>,
        /// Actual events.
        actual: Vec<ExpectedEvent>,
    },
}

/// Runner that feeds JSONL records through [`GestureEngine`].
#[derive(Debug, Clone)]
pub struct ReplayRunner {
    engine: GestureEngine,
}

impl ReplayRunner {
    /// Creates a runner from an engine config.
    #[must_use]
    pub fn new(config: GestureEngineConfig) -> Self {
        Self {
            engine: GestureEngine::new(config),
        }
    }

    /// Replays records with a constant selection state.
    #[must_use]
    pub fn run_records(
        &mut self,
        records: &[HandFrameRecord],
        selection: &SelectionState,
    ) -> ReplayOutcome {
        let base = std::time::Instant::now();
        let mut outcome = ReplayOutcome::default();
        for record in records {
            let frame = record.into_frame(base);
            let update = self.engine.update(&frame, selection);
            outcome.events.extend(
                update
                    .events
                    .iter()
                    .map(|event| expected_from_event(base, event)),
            );
            outcome.last_update = Some(update);
        }
        outcome
    }

    /// Replays a JSONL file.
    pub fn run_path(
        &mut self,
        path: impl AsRef<Path>,
        selection: &SelectionState,
    ) -> Result<ReplayOutcome, ReplayError> {
        let content = std::fs::read_to_string(path)?;
        let records = read_jsonl_str(&content)?;
        Ok(self.run_records(&records, selection))
    }
}

/// Parses HandFrame JSONL content.
pub fn read_jsonl_str(content: &str) -> Result<Vec<HandFrameRecord>, ReplayError> {
    let mut records = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        records.push(serde_json::from_str(line)?);
    }
    Ok(records)
}

/// Loads expected events from JSON.
pub fn read_expected_str(content: &str) -> Result<Vec<ExpectedEvent>, ReplayError> {
    serde_json::from_str(content).map_err(ReplayError::from)
}

/// Compares replay output to expected events.
pub fn compare_expected(
    expected: &[ExpectedEvent],
    actual: &[ExpectedEvent],
) -> Result<(), ReplayError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ReplayError::Mismatch {
            expected: expected.to_vec(),
            actual: actual.to_vec(),
        })
    }
}

/// Parses a gesture id for fixture helpers.
pub fn parse_gesture_id(value: &str) -> Result<GestureId, ReplayError> {
    GestureId::from_str(value).map_err(|err| ReplayError::GestureId(err.to_string()))
}

fn expected_from_event(base: std::time::Instant, event: &GestureEvent) -> ExpectedEvent {
    ExpectedEvent {
        t_ms: event.fired_at.duration_since(base).as_millis() as u64,
        gesture_id: event.gesture_id,
        phase: event.phase,
        target: event.target.map(|target| target.to_string()),
        value_milli: event.value.map(|value| (value * 1_000.0).round() as i32),
    }
}
