//! Landmark geometry and feature-vector extraction.

use flick_core::{HandObservation, Handedness};

/// Feature version used when a 128-dimensional model embedding is present.
pub const FEATURE_VERSION_EMBEDDING: &str = "embedding.128.v1";

/// Feature version used for normalized landmark fallback features.
pub const FEATURE_VERSION_LANDMARKS: &str = "landmarks.angles.v1";

const WRIST: usize = 0;
const THUMB_CMC: usize = 1;
const THUMB_TIP: usize = 4;
const INDEX_MCP: usize = 5;
const INDEX_PIP: usize = 6;
const INDEX_DIP: usize = 7;
const INDEX_TIP: usize = 8;
const MIDDLE_MCP: usize = 9;
const MIDDLE_PIP: usize = 10;
const MIDDLE_DIP: usize = 11;
const MIDDLE_TIP: usize = 12;
const RING_MCP: usize = 13;
const RING_PIP: usize = 14;
const RING_DIP: usize = 15;
const RING_TIP: usize = 16;
const PINKY_MCP: usize = 17;
const PINKY_PIP: usize = 18;
const PINKY_DIP: usize = 19;
const PINKY_TIP: usize = 20;

/// A feature vector for static few-shot classification.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVector {
    /// Feature-version string. Models are retrained when this changes.
    pub version: &'static str,
    /// L2-normalized feature values.
    pub values: Vec<f32>,
}

impl FeatureVector {
    /// Builds a feature vector from a hand observation.
    #[must_use]
    pub fn from_hand(hand: &HandObservation) -> Self {
        if let Some(embedding) = hand.embedding {
            let mut values = embedding.to_vec();
            l2_normalize(&mut values);
            return Self {
                version: FEATURE_VERSION_EMBEDDING,
                values,
            };
        }

        let scale = palm_size(hand).max(1.0e-4);
        let wrist = hand.world[WRIST];
        let mirror = matches!(hand.hand, Handedness::Left);
        let mut values = Vec::with_capacity(63 + 5);
        for point in hand.world {
            let mut x = (point[0] - wrist[0]) / scale;
            if mirror {
                x = -x;
            }
            values.push(x);
            values.push((point[1] - wrist[1]) / scale);
            values.push((point[2] - wrist[2]) / scale);
        }
        values.push(finger_angle_feature(hand, Finger::Thumb));
        values.push(finger_angle_feature(hand, Finger::Index));
        values.push(finger_angle_feature(hand, Finger::Middle));
        values.push(finger_angle_feature(hand, Finger::Ring));
        values.push(finger_angle_feature(hand, Finger::Pinky));
        l2_normalize(&mut values);
        Self {
            version: FEATURE_VERSION_LANDMARKS,
            values,
        }
    }

    /// Cosine similarity to another already-normalized feature vector.
    #[must_use]
    pub fn cosine(&self, other: &Self) -> f32 {
        cosine_values(&self.values, &other.values)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finger {
    Thumb,
    Index,
    Middle,
    Ring,
    Pinky,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Point2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Point3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point2 {
    pub fn distance(self, other: Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx.mul_add(dx, dy * dy)).sqrt()
    }
}

impl Point3 {
    pub fn dot(self, other: Self) -> f32 {
        self.x.mul_add(other.x, self.y.mul_add(other.y, self.z * other.z))
    }

    pub fn norm(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
}

pub(crate) fn world_point(hand: &HandObservation, index: usize) -> Point3 {
    let point = hand.world[index];
    Point3 {
        x: point[0],
        y: point[1],
        z: point[2],
    }
}

pub(crate) fn image_point(hand: &HandObservation, index: usize) -> Point2 {
    let point = hand.image[index];
    Point2 {
        x: point[0],
        y: point[1],
    }
}

pub(crate) fn palm_center(hand: &HandObservation) -> Point2 {
    let indices = [WRIST, INDEX_MCP, MIDDLE_MCP, RING_MCP, PINKY_MCP];
    let mut x = 0.0;
    let mut y = 0.0;
    for index in indices {
        let point = image_point(hand, index);
        x += point.x;
        y += point.y;
    }
    Point2 {
        x: x / indices.len() as f32,
        y: y / indices.len() as f32,
    }
}

pub(crate) fn pinch_point(hand: &HandObservation) -> Point2 {
    let thumb = image_point(hand, THUMB_TIP);
    let index = image_point(hand, INDEX_TIP);
    Point2 {
        x: (thumb.x + index.x) * 0.5,
        y: (thumb.y + index.y) * 0.5,
    }
}

pub(crate) fn index_tip(hand: &HandObservation) -> Point2 {
    image_point(hand, INDEX_TIP)
}

pub(crate) fn palm_size(hand: &HandObservation) -> f32 {
    world_point(hand, WRIST)
        .sub(world_point(hand, MIDDLE_MCP))
        .norm()
        .max(1.0e-4)
}

pub(crate) fn palm_size_image(hand: &HandObservation) -> f32 {
    image_point(hand, WRIST)
        .distance(image_point(hand, MIDDLE_MCP))
        .max(1.0e-4)
}

pub(crate) fn pinch_ratio(hand: &HandObservation) -> f32 {
    image_point(hand, THUMB_TIP).distance(image_point(hand, INDEX_TIP)) / palm_size_image(hand)
}

pub(crate) fn finger_extended(hand: &HandObservation, finger: Finger) -> bool {
    match finger {
        Finger::Thumb => thumb_extended(hand),
        Finger::Index | Finger::Middle | Finger::Ring | Finger::Pinky => {
            let (mcp, pip, dip, tip) = finger_indices(finger);
            let pip_angle = joint_angle(
                world_point(hand, mcp),
                world_point(hand, pip),
                world_point(hand, dip),
            );
            let dip_angle = joint_angle(
                world_point(hand, pip),
                world_point(hand, dip),
                world_point(hand, tip),
            );
            let wrist_distance = world_point(hand, tip)
                .sub(world_point(hand, WRIST))
                .norm();
            let pip_distance = world_point(hand, pip)
                .sub(world_point(hand, WRIST))
                .norm();
            pip_angle >= 155.0 && dip_angle >= 145.0 && wrist_distance > pip_distance * 1.25
        }
    }
}

pub(crate) fn finger_curled(hand: &HandObservation, finger: Finger) -> bool {
    if matches!(finger, Finger::Thumb) {
        return !thumb_extended(hand);
    }
    let (_, pip, _, tip) = finger_indices(finger);
    let tip_distance = world_point(hand, tip)
        .sub(world_point(hand, WRIST))
        .norm();
    let pip_distance = world_point(hand, pip)
        .sub(world_point(hand, WRIST))
        .norm();
    tip_distance <= pip_distance * 1.08 || !finger_extended(hand, finger)
}

pub(crate) fn thumb_direction_y(hand: &HandObservation) -> f32 {
    world_point(hand, THUMB_TIP).y - world_point(hand, THUMB_CMC).y
}

pub(crate) fn l2_normalize(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 1.0e-6 {
        for value in values {
            *value /= norm;
        }
    }
}

pub(crate) fn cosine_values(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(l, r)| l * r)
        .sum::<f32>()
}

pub(crate) fn distance_values(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return f32::INFINITY;
    }
    left.iter()
        .zip(right.iter())
        .map(|(l, r)| {
            let delta = l - r;
            delta * delta
        })
        .sum::<f32>()
        .sqrt()
}

fn thumb_extended(hand: &HandObservation) -> bool {
    let tip = world_point(hand, THUMB_TIP);
    let cmc = world_point(hand, THUMB_CMC);
    let wrist = world_point(hand, WRIST);
    tip.sub(cmc).norm() > cmc.sub(wrist).norm() * 0.85
}

fn finger_angle_feature(hand: &HandObservation, finger: Finger) -> f32 {
    if matches!(finger, Finger::Thumb) {
        return if finger_extended(hand, finger) { 1.0 } else { 0.0 };
    }
    let (mcp, pip, dip, _tip) = finger_indices(finger);
    (joint_angle(
        world_point(hand, mcp),
        world_point(hand, pip),
        world_point(hand, dip),
    ) / 180.0)
        .clamp(0.0, 1.0)
}

fn joint_angle(a: Point3, b: Point3, c: Point3) -> f32 {
    let ba = a.sub(b);
    let bc = c.sub(b);
    let denom = (ba.norm() * bc.norm()).max(1.0e-6);
    let cosine = (ba.dot(bc) / denom).clamp(-1.0, 1.0);
    cosine.acos().to_degrees()
}

fn finger_indices(finger: Finger) -> (usize, usize, usize, usize) {
    match finger {
        Finger::Thumb => (THUMB_CMC, 2, 3, THUMB_TIP),
        Finger::Index => (INDEX_MCP, INDEX_PIP, INDEX_DIP, INDEX_TIP),
        Finger::Middle => (MIDDLE_MCP, MIDDLE_PIP, MIDDLE_DIP, MIDDLE_TIP),
        Finger::Ring => (RING_MCP, RING_PIP, RING_DIP, RING_TIP),
        Finger::Pinky => (PINKY_MCP, PINKY_PIP, PINKY_DIP, PINKY_TIP),
    }
}
