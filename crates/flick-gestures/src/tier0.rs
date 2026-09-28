//! Tier 0 built-in gesture scoring.

use std::{collections::HashMap, time::Duration};

use flick_core::{
    BuiltinGesture, CannedGestureScores, GestureCandidate, GestureId, HandFrame, HandObservation,
    traits::GestureRecognizer,
};
use smallvec::SmallVec;

use crate::features::{Finger, finger_curled, finger_extended, thumb_direction_y};

/// Tier 0 threshold and margin settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tier0Config {
    /// Minimum confidence for a built-in candidate.
    pub threshold: f32,
    /// Required margin between the best and second-best labels.
    pub margin: f32,
    /// Stable duration required for `builtin.point`.
    pub point_stable_ms: u64,
}

impl Default for Tier0Config {
    fn default() -> Self {
        Self {
            threshold: 0.70,
            margin: 0.15,
            point_stable_ms: 200,
        }
    }
}

/// A single Tier 0 score. `gesture == None` is the canned negative class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tier0Score {
    /// Built-in gesture for this score, or `None` for the negative class.
    pub gesture: Option<BuiltinGesture>,
    /// Confidence in `[0, 1]`.
    pub score: f32,
}

/// Small scoring interface that hides the source of Tier 0 scores.
pub trait Tier0Scorer: Send + 'static {
    /// Stable scorer id.
    fn id(&self) -> &'static str;

    /// Scores a hand observation.
    fn score(&self, hand: &HandObservation) -> SmallVec<[Tier0Score; 8]>;
}

/// Reads MediaPipe canned-classifier scores from [`HandObservation::canned_scores`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CannedClassifierScorer;

impl Tier0Scorer for CannedClassifierScorer {
    fn id(&self) -> &'static str {
        "tier0.canned"
    }

    fn score(&self, hand: &HandObservation) -> SmallVec<[Tier0Score; 8]> {
        let Some(scores) = hand.canned_scores else {
            return SmallVec::new();
        };
        canned_scores_to_tier0(scores)
    }
}

/// Geometric scorer that works from landmarks without a model classifier.
#[derive(Debug, Default, Clone, Copy)]
pub struct GeometricLandmarkScorer;

impl Tier0Scorer for GeometricLandmarkScorer {
    fn id(&self) -> &'static str {
        "tier0.geometric"
    }

    fn score(&self, hand: &HandObservation) -> SmallVec<[Tier0Score; 8]> {
        let index = finger_extended(hand, Finger::Index);
        let middle = finger_extended(hand, Finger::Middle);
        let ring = finger_extended(hand, Finger::Ring);
        let pinky = finger_extended(hand, Finger::Pinky);
        let thumb = finger_extended(hand, Finger::Thumb);

        let index_curled = finger_curled(hand, Finger::Index);
        let middle_curled = finger_curled(hand, Finger::Middle);
        let ring_curled = finger_curled(hand, Finger::Ring);
        let pinky_curled = finger_curled(hand, Finger::Pinky);

        let mut scores = SmallVec::new();
        let curled_count = [index_curled, middle_curled, ring_curled, pinky_curled]
            .iter()
            .filter(|value| **value)
            .count();
        let extended_count = [index, middle, ring, pinky]
            .iter()
            .filter(|value| **value)
            .count();

        if extended_count == 4 {
            scores.push(Tier0Score {
                gesture: Some(BuiltinGesture::OpenPalm),
                score: 0.92,
            });
        }
        if curled_count == 4 {
            scores.push(Tier0Score {
                gesture: Some(BuiltinGesture::ClosedFist),
                score: 0.90,
            });
        }
        if index && middle && ring_curled && pinky_curled {
            scores.push(Tier0Score {
                gesture: Some(BuiltinGesture::Victory),
                score: 0.91,
            });
        }
        if index && middle_curled && ring_curled && pinky_curled {
            scores.push(Tier0Score {
                gesture: Some(BuiltinGesture::Point),
                score: 0.94,
            });
            if thumb_direction_y(hand) < -0.03 {
                scores.push(Tier0Score {
                    gesture: Some(BuiltinGesture::PointingUp),
                    score: 0.73,
                });
            }
        }
        if thumb && index_curled && middle_curled && ring_curled && pinky_curled {
            let dy = thumb_direction_y(hand);
            if dy < -0.04 {
                scores.push(Tier0Score {
                    gesture: Some(BuiltinGesture::ThumbUp),
                    score: 0.93,
                });
            } else if dy > 0.04 {
                scores.push(Tier0Score {
                    gesture: Some(BuiltinGesture::ThumbDown),
                    score: 0.93,
                });
            }
        }
        if thumb && index && pinky && middle_curled && ring_curled {
            scores.push(Tier0Score {
                gesture: Some(BuiltinGesture::ILoveYou),
                score: 0.90,
            });
        }
        if scores.is_empty() {
            scores.push(Tier0Score {
                gesture: None,
                score: 0.80,
            });
        }
        scores
    }
}

/// Recognizer wrapper that applies threshold, margin and point-pose stability.
#[derive(Debug, Clone)]
pub struct Tier0Recognizer<S> {
    scorer: S,
    config: Tier0Config,
    point_since: HashMap<u32, std::time::Instant>,
}

impl<S: Tier0Scorer> Tier0Recognizer<S> {
    /// Creates a Tier 0 recognizer around a scorer.
    #[must_use]
    pub fn new(scorer: S, config: Tier0Config) -> Self {
        Self {
            scorer,
            config,
            point_since: HashMap::new(),
        }
    }

    /// Returns the recognizer configuration.
    #[must_use]
    pub const fn config(&self) -> Tier0Config {
        self.config
    }

    /// Scores one frame and returns recognizer candidates.
    #[must_use]
    pub fn update_frame(&mut self, hands: &HandFrame) -> SmallVec<[GestureCandidate; 4]> {
        let mut candidates = SmallVec::new();
        for hand in &hands.hands {
            let scores = self.scorer.score(hand);
            let Some((top, second)) = top_two(&scores) else {
                self.point_since.remove(&hand.track_id);
                continue;
            };
            let Some(gesture) = top.gesture else {
                self.point_since.remove(&hand.track_id);
                continue;
            };
            if top.score < self.config.threshold || top.score - second < self.config.margin {
                if !matches!(gesture, BuiltinGesture::Point) {
                    self.point_since.remove(&hand.track_id);
                }
                continue;
            }

            if matches!(gesture, BuiltinGesture::Point) {
                let entry = self
                    .point_since
                    .entry(hand.track_id)
                    .or_insert(hands.captured_at);
                if hands.captured_at.duration_since(*entry)
                    < Duration::from_millis(self.config.point_stable_ms)
                {
                    continue;
                }
            } else {
                self.point_since.remove(&hand.track_id);
            }

            candidates.push(GestureCandidate {
                gesture_id: GestureId::Builtin(gesture),
                track_id: hand.track_id,
                hand: hand.hand,
                confidence: top.score,
                progress: Some((top.score / self.config.threshold).clamp(0.0, 1.0)),
                value: None,
            });
        }
        candidates
    }
}

impl<S: Tier0Scorer> GestureRecognizer for Tier0Recognizer<S> {
    fn id(&self) -> &'static str {
        self.scorer.id()
    }

    fn update(&mut self, hands: &HandFrame) -> SmallVec<[GestureCandidate; 4]> {
        self.update_frame(hands)
    }
}

fn canned_scores_to_tier0(scores: CannedGestureScores) -> SmallVec<[Tier0Score; 8]> {
    let mut result = SmallVec::new();
    result.push(Tier0Score {
        gesture: None,
        score: scores.scores[CannedGestureScores::NONE],
    });
    for gesture in [
        BuiltinGesture::ClosedFist,
        BuiltinGesture::OpenPalm,
        BuiltinGesture::PointingUp,
        BuiltinGesture::ThumbDown,
        BuiltinGesture::ThumbUp,
        BuiltinGesture::Victory,
        BuiltinGesture::ILoveYou,
    ] {
        if let Some(score) = scores.score_for(gesture) {
            result.push(Tier0Score {
                gesture: Some(gesture),
                score,
            });
        }
    }
    result
}

fn top_two(scores: &[Tier0Score]) -> Option<(Tier0Score, f32)> {
    let mut best: Option<Tier0Score> = None;
    let mut second = 0.0;
    for score in scores {
        if best.is_none_or(|candidate| score.score > candidate.score) {
            if let Some(candidate) = best {
                second = candidate.score;
            }
            best = Some(*score);
        } else if score.score > second {
            second = score.score;
        }
    }
    best.map(|candidate| (candidate, second))
}
