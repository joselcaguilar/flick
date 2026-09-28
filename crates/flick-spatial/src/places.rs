use flick_core::{AnchorId, PlaceId};
use nalgebra::Matrix3;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::math::{Mat3, a3, angle_deg, unit_or_z, v3};

/// Cosine similarity between two scene signatures.
pub type Similarity = f32;

/// Place metadata needed by the matcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceDescriptor {
    /// Place id.
    pub id: PlaceId,
    /// Human name, such as "Bedroom desk".
    pub name: String,
    /// DINOv2-small scene signature, person/hand masked by flick-vision.
    #[serde(with = "signature_serde")]
    pub scene_signature: [f32; 384],
}

/// Settings for scene-signature place matching.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaceMatcherSettings {
    /// Cosine similarity threshold for same/switch decisions.
    pub threshold: f32,
}

impl Default for PlaceMatcherSettings {
    fn default() -> Self {
        Self { threshold: 0.90 }
    }
}

/// Result of matching a current scene signature to known places.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum PlaceMatch {
    /// Current active place still matches.
    Ok {
        /// Active place id.
        place_id: PlaceId,
        /// Cosine similarity to the active signature.
        similarity: Similarity,
    },
    /// Another known place matched better and should become active.
    Switched {
        /// New active place id.
        place_id: PlaceId,
        /// Cosine similarity to the selected place.
        similarity: Similarity,
    },
    /// No known place matched above the threshold; anchors should pause.
    NeedsRealign {
        /// Best similarity observed, even though it was too low.
        similarity: Similarity,
    },
}

/// Scene-signature matcher for one camera.
#[derive(Debug, Clone)]
pub struct PlaceMatcher {
    settings: PlaceMatcherSettings,
    places: Vec<PlaceDescriptor>,
    active: Option<PlaceId>,
}

impl PlaceMatcher {
    /// Creates a matcher with known places and an optional active place id.
    #[must_use]
    pub fn new(
        places: Vec<PlaceDescriptor>,
        active: Option<PlaceId>,
        settings: PlaceMatcherSettings,
    ) -> Self {
        Self {
            settings,
            places,
            active,
        }
    }

    /// Replaces the known places set.
    pub fn set_places(&mut self, places: Vec<PlaceDescriptor>, active: Option<PlaceId>) {
        self.places = places;
        self.active = active;
    }

    /// Matches the current scene signature.
    #[must_use]
    pub fn match_signature(&self, current: &[f32; 384]) -> PlaceMatch {
        let mut best: Option<(PlaceId, Similarity)> = None;
        for place in &self.places {
            let sim = cosine_384(&place.scene_signature, current);
            if best.is_none_or(|(_, best_sim)| sim > best_sim) {
                best = Some((place.id, sim));
            }
        }
        let Some((best_id, best_similarity)) = best else {
            return PlaceMatch::NeedsRealign { similarity: 0.0 };
        };
        if let Some(active_id) = self.active {
            if let Some(active_place) = self.places.iter().find(|place| place.id == active_id) {
                let active_similarity = cosine_384(&active_place.scene_signature, current);
                if active_similarity >= self.settings.threshold {
                    return PlaceMatch::Ok {
                        place_id: active_id,
                        similarity: active_similarity,
                    };
                }
            }
        }
        if best_similarity >= self.settings.threshold {
            PlaceMatch::Switched {
                place_id: best_id,
                similarity: best_similarity,
            }
        } else {
            PlaceMatch::NeedsRealign {
                similarity: best_similarity,
            }
        }
    }
}

/// Returns true when the current scene similarity should pause targeting.
#[must_use]
pub fn needs_realign(similarity: Similarity, threshold: f32) -> bool {
    similarity < threshold
}

/// Pair of old anchor direction and newly observed direction for Kabsch/Wahba realignment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RealignPair {
    /// Anchor being re-pointed.
    pub anchor_id: AnchorId,
    /// Direction in the old place/camera pose.
    pub old_direction: [f32; 3],
    /// Direction observed after the camera moved.
    pub new_direction: [f32; 3],
}

/// Rigid transform applied to anchors during realignment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform3 {
    /// Rotation matrix, row-major.
    pub rotation: [[f32; 3]; 3],
    /// Translation in meters. Zero for the two-anchor direction-only MVP.
    pub translation: [f32; 3],
}

impl Transform3 {
    /// Applies the transform to a direction.
    #[must_use]
    pub fn transform_direction(&self, direction: [f32; 3]) -> [f32; 3] {
        let r = array_to_mat(self.rotation);
        a3(unit_or_z(r * v3(direction)))
    }

    /// Applies the transform to a point.
    #[must_use]
    pub fn transform_point(&self, point: [f32; 3]) -> [f32; 3] {
        let r = array_to_mat(self.rotation);
        a3(r * v3(point) + v3(self.translation))
    }
}

/// Result of a realignment solve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealignResult {
    /// Transform to apply to anchors.
    pub transform: Transform3,
    /// Mean angular residual in degrees.
    pub residual_deg: f32,
    /// Whether the residual is within the spec's ≤ 5° OK band.
    pub ok: bool,
}

/// Realignment failure.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum RealignError {
    /// At least two re-pointed anchors are required.
    #[error("at least two re-pointed anchors are required")]
    NotEnoughPairs,
    /// The Kabsch solve failed.
    #[error("realignment rotation solve failed")]
    SolveFailed,
}

/// Solves the Kabsch/Wahba rotation from at least two re-pointed anchors.
pub fn realign(pairs: &[RealignPair]) -> Result<RealignResult, RealignError> {
    if pairs.len() < 2 {
        return Err(RealignError::NotEnoughPairs);
    }
    let mut h = Mat3::zeros();
    for pair in pairs {
        let old = unit_or_z(v3(pair.old_direction));
        let new = unit_or_z(v3(pair.new_direction));
        h += new * old.transpose();
    }
    let svd = h.svd(true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else {
        return Err(RealignError::SolveFailed);
    };
    let mut fix = Mat3::identity();
    if (u * v_t).determinant() < 0.0 {
        fix[(2, 2)] = -1.0;
    }
    let rotation = u * fix * v_t;
    let residual = pairs
        .iter()
        .map(|pair| angle_deg(rotation * unit_or_z(v3(pair.old_direction)), v3(pair.new_direction)))
        .sum::<f32>()
        / pairs.len() as f32;
    Ok(RealignResult {
        transform: Transform3 {
            rotation: mat_to_array(rotation),
            translation: [0.0, 0.0, 0.0],
        },
        residual_deg: residual,
        ok: residual <= 5.0,
    })
}

mod signature_serde {
    use serde::{Deserialize, Serialize, de};

    pub fn serialize<S>(value: &[f32; 384], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        value.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[f32; 384], D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let values = Vec::<f32>::deserialize(deserializer)?;
        if values.len() != 384 {
            return Err(de::Error::invalid_length(values.len(), &"384 float scene-signature values"));
        }
        let mut out = [0.0_f32; 384];
        out.copy_from_slice(&values);
        Ok(out)
    }
}

fn cosine_384(a: &[f32; 384], b: &[f32; 384]) -> f32 {
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na <= 1.0e-12 || nb <= 1.0e-12 {
        0.0
    } else {
        (dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0)
    }
}

fn mat_to_array(matrix: Matrix3<f32>) -> [[f32; 3]; 3] {
    [
        [matrix[(0, 0)], matrix[(0, 1)], matrix[(0, 2)]],
        [matrix[(1, 0)], matrix[(1, 1)], matrix[(1, 2)]],
        [matrix[(2, 0)], matrix[(2, 1)], matrix[(2, 2)]],
    ]
}

fn array_to_mat(array: [[f32; 3]; 3]) -> Matrix3<f32> {
    Matrix3::new(
        array[0][0], array[0][1], array[0][2], array[1][0], array[1][1], array[1][2],
        array[2][0], array[2][1], array[2][2],
    )
}
