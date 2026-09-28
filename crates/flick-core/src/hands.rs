//! Hand landmark observations produced by the vision pipeline.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::CameraId;

/// The user's physical hand after mirror normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    /// Left physical hand.
    Left,
    /// Right physical hand.
    Right,
}

/// A normalized rectangle in frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectF {
    /// Left coordinate in `[0, 1]`.
    pub x: f32,
    /// Top coordinate in `[0, 1]`.
    pub y: f32,
    /// Width in normalized frame units.
    pub w: f32,
    /// Height in normalized frame units.
    pub h: f32,
}

/// Per-stage latency measurements for a processed frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StageTimings {
    /// Capture and pixel conversion time in milliseconds.
    pub capture_ms: f32,
    /// Palm detection time in milliseconds.
    pub palm_ms: f32,
    /// Hand landmark inference time in milliseconds.
    pub landmarks_ms: f32,
    /// Tracking and smoothing time in milliseconds.
    pub tracking_ms: f32,
    /// Gesture embedding and canned classifier time in milliseconds.
    pub embedding_ms: f32,
    /// Targeting ray and anchor scoring time in milliseconds.
    pub targeting_ms: f32,
    /// Total vision pipeline time in milliseconds.
    pub total_ms: f32,
}

/// Six BlazeFace keypoints used for eye-rooted targeting rays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceKeypoints {
    /// Keypoints in normalized image coordinates.
    pub points: [[f32; 2]; 6],
    /// Optional detector confidence.
    pub confidence: Option<f32>,
}

/// One tracked hand in a processed frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandObservation {
    /// Stable track id within one camera stream.
    pub track_id: u32,
    /// User's physical hand.
    pub hand: Handedness,
    /// Handedness confidence from the landmarker.
    pub handedness_score: f32,
    /// Landmark presence score.
    pub presence: f32,
    /// Twenty-one image-space landmarks in normalized frame coordinates.
    pub image: [[f32; 3]; 21],
    /// Twenty-one world landmarks in meters, hand-centered.
    pub world: [[f32; 3]; 21],
    /// Normalized hand bounding box.
    pub bbox: RectF,
    /// Optional 128-dimensional gesture embedding.
    #[serde(
        default,
        with = "embedding_serde",
        skip_serializing_if = "Option::is_none"
    )]
    pub embedding: Option<[f32; 128]>,
    /// Optional Tier 0 canned-classifier scores in spec label order.
    #[serde(
        default,
        with = "canned_scores_serde",
        skip_serializing_if = "Option::is_none"
    )]
    pub canned_scores: Option<[f32; 8]>,
}

/// All tracked hands for one processed camera frame.
#[derive(Debug, Clone)]
pub struct HandFrame {
    /// Flick camera id.
    pub camera_id: CameraId,
    /// Frame sequence number.
    pub seq: u64,
    /// Monotonic capture timestamp from the source frame.
    pub captured_at: std::time::Instant,
    /// Tracked hand observations.
    pub hands: SmallVec<[HandObservation; 2]>,
    /// Stage timings for status and benchmarks.
    pub timings: StageTimings,
}

mod embedding_serde {
    use serde::{Deserialize, Serialize, de};

    pub fn serialize<S>(value: &Option<[f32; 128]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match value {
            Some(embedding) => embedding.as_slice().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[f32; 128]>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let Some(values) = Option::<Vec<f32>>::deserialize(deserializer)? else {
            return Ok(None);
        };
        if values.len() != 128 {
            return Err(de::Error::invalid_length(
                values.len(),
                &"128 float embedding values",
            ));
        }
        let mut embedding = [0.0_f32; 128];
        embedding.copy_from_slice(&values);
        Ok(Some(embedding))
    }
}

mod canned_scores_serde {
    use serde::{Deserialize, Serialize, de};

    pub fn serialize<S>(value: &Option<[f32; 8]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match value {
            Some(scores) => scores.as_slice().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[f32; 8]>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let Some(values) = Option::<Vec<f32>>::deserialize(deserializer)? else {
            return Ok(None);
        };
        if values.len() != 8 {
            return Err(de::Error::invalid_length(
                values.len(),
                &"8 canned gesture score values",
            ));
        }
        let mut scores = [0.0_f32; 8];
        scores.copy_from_slice(&values);
        Ok(Some(scores))
    }
}
