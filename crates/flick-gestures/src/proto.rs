//! Few-shot static gesture classifier using prototypes plus k-NN votes.

use std::collections::{BTreeMap, BTreeSet};

use flick_core::{GestureError, GestureId};

use crate::features::{FeatureVector, cosine_values, distance_values};

/// A labeled static-gesture sample.
#[derive(Debug, Clone, PartialEq)]
pub struct LabeledSample {
    /// Gesture id for the sample. `system.none` is the negative class.
    pub gesture_id: GestureId,
    /// Feature vector, either embedding or normalized landmark fallback.
    pub feature: FeatureVector,
    /// Optional take index for LOTO metrics.
    pub take_index: Option<usize>,
}

/// ProtoKnn tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtoKnnConfig {
    /// Number of nearest samples voting.
    pub k: usize,
    /// Softmax temperature for prototype similarities.
    pub tau: f32,
    /// Multiplier applied to per-class spread for open-set rejection.
    pub open_set_sigma: f32,
    /// Default per-gesture threshold.
    pub threshold: f32,
}

impl Default for ProtoKnnConfig {
    fn default() -> Self {
        Self {
            k: 5,
            tau: 0.05,
            open_set_sigma: 1.5,
            threshold: 0.75,
        }
    }
}

/// Prediction from [`ProtoKnn`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtoPrediction {
    /// Winning gesture, or `None` after open-set rejection.
    pub gesture_id: Option<GestureId>,
    /// Confidence score in `[0, 1]`.
    pub score: f32,
    /// Distance to the winning prototype.
    pub distance: f32,
}

/// A confusion pair for Studio quality reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confusion {
    /// Gesture being evaluated.
    pub gesture_id: GestureId,
    /// Nearest confusable gesture.
    pub confused_with: GestureId,
    /// Similarity to the confusable gesture.
    pub similarity: f32,
}

/// Per-class Studio metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMetrics {
    /// Gesture id.
    pub gesture_id: GestureId,
    /// Leave-one-take-out accuracy for this class.
    pub loto_accuracy: f32,
    /// Distinctiveness, `1 - max cosine similarity` to another prototype.
    pub distinctiveness: f32,
}

/// Training report shown in Gesture Studio.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoKnnReport {
    /// Overall leave-one-take-out accuracy.
    pub loto_accuracy: f32,
    /// Per-class metrics.
    pub per_class: Vec<ClassMetrics>,
    /// Nearest confusions.
    pub confusions: Vec<Confusion>,
}

/// Few-shot static classifier.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtoKnn {
    /// Feature version that produced this model.
    pub feature_version: &'static str,
    config: ProtoKnnConfig,
    prototypes: Vec<Prototype>,
    samples: Vec<LabeledSample>,
}

impl ProtoKnn {
    /// Trains a model and computes Studio quality metrics.
    pub fn train(
        samples: Vec<LabeledSample>,
        config: ProtoKnnConfig,
    ) -> Result<(Self, ProtoKnnReport), GestureError> {
        let model = Self::train_model(samples, config)?;
        let report = model.report()?;
        Ok((model, report))
    }

    /// Predicts a gesture for a feature vector.
    #[must_use]
    pub fn predict(&self, feature: &FeatureVector) -> ProtoPrediction {
        if self.prototypes.is_empty() || feature.values.is_empty() {
            return ProtoPrediction {
                gesture_id: None,
                score: 0.0,
                distance: f32::INFINITY,
            };
        }
        let mut proto_scores: Vec<(GestureId, f32, f32)> = self
            .prototypes
            .iter()
            .map(|prototype| {
                let similarity = cosine_values(&prototype.values, &feature.values);
                (prototype.gesture_id, similarity, 1.0 - similarity)
            })
            .collect();
        proto_scores.sort_by(|left, right| right.1.total_cmp(&left.1));
        let Some((winner, _similarity, distance)) = proto_scores.first().copied() else {
            return ProtoPrediction {
                gesture_id: None,
                score: 0.0,
                distance: f32::INFINITY,
            };
        };
        let prototype_score = softmax_score(winner, &proto_scores, self.config.tau);
        let knn_score = self.knn_fraction(winner, feature);
        let score = 0.5_f32.mul_add(prototype_score, 0.5 * knn_score);
        let spread = self
            .prototypes
            .iter()
            .find(|prototype| prototype.gesture_id == winner)
            .map_or(0.0, |prototype| prototype.spread);
        let rejected = winner == GestureId::SystemNone
            || score < self.config.threshold
            || distance > spread.max(0.05) * self.config.open_set_sigma;
        ProtoPrediction {
            gesture_id: if rejected { None } else { Some(winner) },
            score,
            distance,
        }
    }

    /// Returns whether the model must be rebuilt for a new feature version.
    #[must_use]
    pub fn needs_rebuild(&self, feature_version: &str) -> bool {
        self.feature_version != feature_version
    }

    fn train_model(
        samples: Vec<LabeledSample>,
        config: ProtoKnnConfig,
    ) -> Result<Self, GestureError> {
        let Some(first) = samples.first() else {
            return Err(GestureError::InsufficientData(
                "at least one labeled sample is required".to_owned(),
            ));
        };
        let feature_version = first.feature.version;
        if samples
            .iter()
            .any(|sample| sample.feature.version != feature_version)
        {
            return Err(GestureError::InvalidGesture(
                "all samples must use the same feature version".to_owned(),
            ));
        }
        let mut by_class: BTreeMap<String, Vec<&LabeledSample>> = BTreeMap::new();
        for sample in &samples {
            by_class
                .entry(sample.gesture_id.to_string())
                .or_default()
                .push(sample);
        }
        let mut prototypes = Vec::new();
        for class_samples in by_class.values() {
            let Some(first_sample) = class_samples.first() else {
                continue;
            };
            let len = first_sample.feature.values.len();
            let mut values = vec![0.0; len];
            for sample in class_samples {
                for (index, value) in sample.feature.values.iter().enumerate() {
                    values[index] += value;
                }
            }
            for value in &mut values {
                *value /= class_samples.len() as f32;
            }
            crate::features::l2_normalize(&mut values);
            let mut distances: Vec<f32> = class_samples
                .iter()
                .map(|sample| 1.0 - cosine_values(&values, &sample.feature.values))
                .collect();
            distances.sort_by(f32::total_cmp);
            let spread_index = ((distances.len().saturating_sub(1)) as f32 * 0.9).round() as usize;
            let spread = distances.get(spread_index).copied().unwrap_or(0.05).max(0.05);
            prototypes.push(Prototype {
                gesture_id: first_sample.gesture_id,
                values,
                spread,
            });
        }
        Ok(Self {
            feature_version,
            config,
            prototypes,
            samples,
        })
    }

    fn knn_fraction(&self, winner: GestureId, feature: &FeatureVector) -> f32 {
        let mut distances: Vec<(GestureId, f32)> = self
            .samples
            .iter()
            .map(|sample| {
                (
                    sample.gesture_id,
                    distance_values(&sample.feature.values, &feature.values),
                )
            })
            .collect();
        distances.sort_by(|left, right| left.1.total_cmp(&right.1));
        let k = self.config.k.min(distances.len()).max(1);
        let votes = distances
            .iter()
            .take(k)
            .filter(|(gesture_id, _)| *gesture_id == winner)
            .count();
        votes as f32 / k as f32
    }

    fn report(&self) -> Result<ProtoKnnReport, GestureError> {
        let mut correct = 0usize;
        let mut total = 0usize;
        let mut class_totals: BTreeMap<GestureId, (usize, usize)> = BTreeMap::new();
        for index in 0..self.samples.len() {
            let mut held_out = self.samples.clone();
            let sample = held_out.remove(index);
            let model = Self::train_model(held_out, self.config)?;
            let prediction = model.predict(&sample.feature);
            let class_entry = class_totals.entry(sample.gesture_id).or_insert((0, 0));
            class_entry.1 += 1;
            total += 1;
            if prediction.gesture_id == Some(sample.gesture_id) {
                correct += 1;
                class_entry.0 += 1;
            }
        }

        let mut per_class = Vec::new();
        for prototype in &self.prototypes {
            if prototype.gesture_id == GestureId::SystemNone {
                continue;
            }
            let (class_correct, class_total) = class_totals
                .get(&prototype.gesture_id)
                .copied()
                .unwrap_or((0, 0));
            per_class.push(ClassMetrics {
                gesture_id: prototype.gesture_id,
                loto_accuracy: if class_total == 0 {
                    1.0
                } else {
                    class_correct as f32 / class_total as f32
                },
                distinctiveness: self.distinctiveness(prototype.gesture_id),
            });
        }

        Ok(ProtoKnnReport {
            loto_accuracy: if total == 0 {
                1.0
            } else {
                correct as f32 / total as f32
            },
            per_class,
            confusions: self.confusions(),
        })
    }

    fn distinctiveness(&self, gesture_id: GestureId) -> f32 {
        let Some(prototype) = self
            .prototypes
            .iter()
            .find(|prototype| prototype.gesture_id == gesture_id)
        else {
            return 0.0;
        };
        let nearest = self
            .prototypes
            .iter()
            .filter(|other| other.gesture_id != gesture_id)
            .map(|other| cosine_values(&prototype.values, &other.values))
            .fold(0.0_f32, f32::max);
        (1.0 - nearest).clamp(0.0, 1.0)
    }

    fn confusions(&self) -> Vec<Confusion> {
        let mut out = Vec::new();
        for prototype in &self.prototypes {
            if prototype.gesture_id == GestureId::SystemNone {
                continue;
            }
            let mut others: Vec<(GestureId, f32)> = self
                .prototypes
                .iter()
                .filter(|other| other.gesture_id != prototype.gesture_id)
                .map(|other| {
                    (
                        other.gesture_id,
                        cosine_values(&prototype.values, &other.values),
                    )
                })
                .collect();
            others.sort_by(|left, right| right.1.total_cmp(&left.1));
            if let Some((gesture_id, similarity)) = others.first().copied() {
                out.push(Confusion {
                    gesture_id: prototype.gesture_id,
                    confused_with: gesture_id,
                    similarity,
                });
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Prototype {
    gesture_id: GestureId,
    values: Vec<f32>,
    spread: f32,
}

fn softmax_score(winner: GestureId, scores: &[(GestureId, f32, f32)], tau: f32) -> f32 {
    let stable_tau = tau.max(1.0e-4);
    let max_score = scores
        .iter()
        .map(|(_, similarity, _)| *similarity / stable_tau)
        .fold(f32::NEG_INFINITY, f32::max);
    let mut denominator = 0.0;
    let mut numerator = 0.0;
    let mut seen = BTreeSet::new();
    for (gesture_id, similarity, _) in scores {
        if !seen.insert(*gesture_id) {
            continue;
        }
        let value = ((*similarity / stable_tau) - max_score).exp();
        denominator += value;
        if *gesture_id == winner {
            numerator += value;
        }
    }
    if denominator <= 1.0e-6 {
        0.0
    } else {
        numerator / denominator
    }
}
