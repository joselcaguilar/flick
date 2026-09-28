//! Built-in motion recognizers and custom DTW templates.

use std::{collections::HashMap, time::Duration};

use flick_core::{
    BuiltinGesture, GestureCandidate, GestureId, HandFrame, HandObservation, Handedness,
    traits::GestureRecognizer,
};
use smallvec::SmallVec;

use crate::features::{
    Finger, Point2, finger_extended, image_point, index_tip, palm_center, palm_size_image,
    pinch_point, pinch_ratio,
};

/// Axis filter for `builtin.two_hand_separate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionAxis {
    /// Vertical or horizontal separation can match.
    Any,
    /// Only mostly vertical separation can match.
    Vertical,
    /// Only mostly horizontal separation can match.
    Horizontal,
}

/// Built-in motion recognizer settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionConfig {
    /// Minimum swipe displacement in palm sizes.
    pub swipe_min_palm: f32,
    /// Minimum swipe straightness.
    pub swipe_straightness: f32,
    /// Minimum swipe duration.
    pub swipe_min_ms: u64,
    /// Maximum swipe duration.
    pub swipe_max_ms: u64,
    /// Pinch enter ratio.
    pub pinch_enter: f32,
    /// Pinch exit ratio.
    pub pinch_exit: f32,
    /// Pinch hold time before firing.
    pub pinch_enter_ms: u64,
    /// Two-hand axis setting.
    pub two_hand_axis: MotionAxis,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            swipe_min_palm: 3.0,
            swipe_straightness: 0.8,
            swipe_min_ms: 120,
            swipe_max_ms: 700,
            pinch_enter: 0.25,
            pinch_exit: 0.35,
            pinch_enter_ms: 150,
            two_hand_axis: MotionAxis::Any,
        }
    }
}

/// Built-in swipe, pinch-dial, circle and two-hand recognizer.
#[derive(Debug, Clone)]
pub struct BuiltinMotionRecognizer {
    config: MotionConfig,
    tracks: HashMap<u32, MotionTrack>,
    two_hand: TwoHandState,
}

impl BuiltinMotionRecognizer {
    /// Creates the recognizer.
    #[must_use]
    pub fn new(config: MotionConfig) -> Self {
        Self {
            config,
            tracks: HashMap::new(),
            two_hand: TwoHandState::default(),
        }
    }

    /// Updates the recognizer with one hand frame.
    #[must_use]
    pub fn update_frame(&mut self, frame: &HandFrame) -> SmallVec<[GestureCandidate; 8]> {
        let mut candidates = SmallVec::new();
        for hand in &frame.hands {
            let sample = MotionSample::from_hand(frame.captured_at, hand);
            let track = self.tracks.entry(hand.track_id).or_default();
            track.push(sample);
            if let Some(candidate) = track.detect_swipe(hand, &self.config) {
                candidates.push(candidate);
            }
            if let Some(candidate) = track.detect_pinch(hand, frame.captured_at, &self.config) {
                candidates.push(candidate);
            }
            if let Some(candidate) = track.detect_circle(hand) {
                candidates.push(candidate);
            }
        }
        if let Some(candidate) = self.detect_two_hand(frame) {
            candidates.push(candidate);
        }
        candidates
    }

    fn detect_two_hand(&mut self, frame: &HandFrame) -> Option<GestureCandidate> {
        if frame.hands.len() != 2 {
            self.two_hand = TwoHandState::default();
            return None;
        }
        let left = &frame.hands[0];
        let right = &frame.hands[1];
        let left_center = palm_center(left);
        let right_center = palm_center(right);
        let avg_palm = (palm_size_image(left) + palm_size_image(right)) * 0.5;
        let distance = left_center.distance(right_center);
        let dx = (left_center.x - right_center.x).abs();
        let dy = (left_center.y - right_center.y).abs();
        let stacked = distance <= 1.5 * avg_palm && dy >= dx * 0.6;
        if stacked && self.two_hand.start.is_none() {
            self.two_hand.start = Some(TwoHandStart {
                at: frame.captured_at,
                left: left_center,
                right: right_center,
                distance,
            });
            return None;
        }
        let start = self.two_hand.start?;
        let age = frame.captured_at.duration_since(start.at);
        if age < Duration::from_millis(150) {
            return None;
        }
        if age > Duration::from_millis(800) {
            self.two_hand = TwoHandState::default();
            return None;
        }
        let growth = distance - start.distance;
        if growth < 3.0 * avg_palm {
            return None;
        }
        let left_velocity = Point2 {
            x: left_center.x - start.left.x,
            y: left_center.y - start.left.y,
        };
        let right_velocity = Point2 {
            x: right_center.x - start.right.x,
            y: right_center.y - start.right.y,
        };
        if cosine2(left_velocity, right_velocity) > -0.5 {
            return None;
        }
        let axis_ok = match self.config.two_hand_axis {
            MotionAxis::Any => true,
            MotionAxis::Vertical => dy >= dx,
            MotionAxis::Horizontal => dx >= dy,
        };
        if !axis_ok {
            return None;
        }
        self.two_hand = TwoHandState::default();
        Some(GestureCandidate {
            gesture_id: GestureId::Builtin(BuiltinGesture::TwoHandSeparate),
            track_id: 0,
            hand: Handedness::Right,
            confidence: 0.92,
            progress: Some(1.0),
            value: None,
        })
    }
}

impl GestureRecognizer for BuiltinMotionRecognizer {
    fn id(&self) -> &'static str {
        "motion.builtin"
    }

    fn update(&mut self, hands: &HandFrame) -> SmallVec<[GestureCandidate; 4]> {
        let mut out = SmallVec::new();
        for candidate in self.update_frame(hands) {
            out.push(candidate);
        }
        out
    }
}

#[derive(Debug, Clone, Copy)]
struct MotionSample {
    at: std::time::Instant,
    palm: Point2,
    index: Point2,
    pinch: Point2,
    pinch_ratio: f32,
    palm_size: f32,
    open_palm: bool,
}

impl MotionSample {
    fn from_hand(at: std::time::Instant, hand: &HandObservation) -> Self {
        let open_palm = [Finger::Index, Finger::Middle, Finger::Ring, Finger::Pinky]
            .iter()
            .all(|finger| finger_extended(hand, *finger));
        Self {
            at,
            palm: palm_center(hand),
            index: index_tip(hand),
            pinch: pinch_point(hand),
            pinch_ratio: pinch_ratio(hand),
            palm_size: palm_size_image(hand),
            open_palm,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct MotionTrack {
    samples: Vec<MotionSample>,
    last_swipe_at: Option<std::time::Instant>,
    pinch_since: Option<std::time::Instant>,
    pinch_active: bool,
    pinch_start_y: f32,
    circle_last_turn: i32,
}

impl MotionTrack {
    fn push(&mut self, sample: MotionSample) {
        self.samples.push(sample);
        let cutoff = sample.at - Duration::from_millis(2_500);
        self.samples.retain(|item| item.at >= cutoff);
    }

    fn detect_swipe(
        &mut self,
        hand: &HandObservation,
        config: &MotionConfig,
    ) -> Option<GestureCandidate> {
        let latest = *self.samples.last()?;
        if self
            .last_swipe_at
            .is_some_and(|last| latest.at.duration_since(last) < Duration::from_millis(600))
        {
            return None;
        }
        for first in &self.samples {
            let dt = latest.at.duration_since(first.at).as_millis() as u64;
            if dt < config.swipe_min_ms || dt > config.swipe_max_ms {
                continue;
            }
            let dx = latest.palm.x - first.palm.x;
            let dy = latest.palm.y - first.palm.y;
            let net = dx.hypot(dy);
            let palm_units = net / latest.palm_size.max(1.0e-4);
            if palm_units < config.swipe_min_palm
                || straightness(&self.samples) < config.swipe_straightness
            {
                continue;
            }
            let open_count = self
                .samples
                .iter()
                .filter(|sample| sample.open_palm)
                .count();
            if open_count * 5 < self.samples.len() * 3 {
                continue;
            }
            let gesture = if dx.abs() >= dy.abs() {
                if dx >= 0.0 {
                    BuiltinGesture::SwipeRight
                } else {
                    BuiltinGesture::SwipeLeft
                }
            } else if dy >= 0.0 {
                BuiltinGesture::SwipeDown
            } else {
                BuiltinGesture::SwipeUp
            };
            self.last_swipe_at = Some(latest.at);
            return Some(GestureCandidate {
                gesture_id: GestureId::Builtin(gesture),
                track_id: hand.track_id,
                hand: hand.hand,
                confidence: 0.88,
                progress: Some(1.0),
                value: None,
            });
        }
        None
    }

    fn detect_pinch(
        &mut self,
        hand: &HandObservation,
        now: std::time::Instant,
        config: &MotionConfig,
    ) -> Option<GestureCandidate> {
        let latest = *self.samples.last()?;
        if latest.pinch_ratio < config.pinch_enter {
            let since = self.pinch_since.get_or_insert(now);
            if !self.pinch_active
                && now.duration_since(*since) >= Duration::from_millis(config.pinch_enter_ms)
            {
                self.pinch_active = true;
                self.pinch_start_y = latest.pinch.y;
            }
        } else if latest.pinch_ratio > config.pinch_exit {
            self.pinch_since = None;
            self.pinch_active = false;
            return None;
        }
        if !self.pinch_active {
            return None;
        }
        let delta = ((self.pinch_start_y - latest.pinch.y) / 0.5).clamp(-1.0, 1.0);
        Some(GestureCandidate {
            gesture_id: GestureId::Builtin(BuiltinGesture::PinchDial),
            track_id: hand.track_id,
            hand: hand.hand,
            confidence: 0.90,
            progress: Some(1.0),
            value: Some(delta),
        })
    }

    fn detect_circle(&mut self, hand: &HandObservation) -> Option<GestureCandidate> {
        let latest = *self.samples.last()?;
        let recent: Vec<MotionSample> = self
            .samples
            .iter()
            .copied()
            .filter(|sample| latest.at.duration_since(sample.at) <= Duration::from_millis(1_500))
            .collect();
        if recent.len() < 12 {
            return None;
        }
        let centroid = centroid(recent.iter().map(|sample| sample.index));
        let radii: Vec<f32> = recent
            .iter()
            .map(|sample| sample.index.distance(centroid))
            .collect();
        let mean_radius = radii.iter().sum::<f32>() / radii.len() as f32;
        if mean_radius < latest.palm_size {
            return None;
        }
        let variance = radii
            .iter()
            .map(|radius| {
                let delta = radius - mean_radius;
                delta * delta
            })
            .sum::<f32>()
            / radii.len() as f32;
        if variance.sqrt() / mean_radius > 0.35 {
            return None;
        }
        let turns = signed_turns(&recent, centroid);
        let full_turns = if turns >= 300.0 {
            (turns / 360.0).round() as i32
        } else if turns <= -300.0 {
            (turns / 360.0).round() as i32
        } else {
            0
        };
        if full_turns == 0 {
            if turns.abs() < 120.0 {
                self.circle_last_turn = 0;
            }
            return None;
        }
        if full_turns == self.circle_last_turn {
            return None;
        }
        self.circle_last_turn = full_turns;
        let gesture = if full_turns > 0 {
            BuiltinGesture::CircleCw
        } else {
            BuiltinGesture::CircleCcw
        };
        Some(GestureCandidate {
            gesture_id: GestureId::Builtin(gesture),
            track_id: hand.track_id,
            hand: hand.hand,
            confidence: 0.89,
            progress: Some(1.0),
            value: None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct TwoHandStart {
    at: std::time::Instant,
    left: Point2,
    right: Point2,
    distance: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct TwoHandState {
    start: Option<TwoHandStart>,
}

/// Auto-detected gesture kind for Studio takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoGestureKind {
    /// Low-energy take suitable for static Tier 1 training.
    Static,
    /// One-hand motion template.
    Motion,
    /// Two-hand motion template.
    TwoHand,
}

/// Raw motion take represented as normalized channel points.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionTake {
    /// Gesture id that this take belongs to.
    pub gesture_id: GestureId,
    /// Channel points. Each row is a feature vector for one timestamp.
    pub points: Vec<Vec<f32>>,
    /// Number of hands in the original take.
    pub hands: usize,
}

impl MotionTake {
    /// Builds a single-hand take from hand-frame landmarks.
    #[must_use]
    pub fn from_single_hand(gesture_id: GestureId, frames: &[HandObservation]) -> Self {
        let Some(first) = frames.first() else {
            return Self {
                gesture_id,
                points: Vec::new(),
                hands: 1,
            };
        };
        let origin = palm_center(first);
        let scale = palm_size_image(first).max(1.0e-4);
        let points = frames
            .iter()
            .map(|hand| {
                let palm = palm_center(hand);
                let index = image_point(hand, 8);
                vec![
                    (index.x - origin.x) / scale,
                    (index.y - origin.y) / scale,
                    (palm.x - origin.x) / scale,
                    (palm.y - origin.y) / scale,
                ]
            })
            .collect();
        Self {
            gesture_id,
            points,
            hands: 1,
        }
    }

    /// Estimates the Studio auto type from the take trajectory.
    #[must_use]
    pub fn auto_kind(&self) -> AutoGestureKind {
        if self.hands >= 2 {
            return AutoGestureKind::TwoHand;
        }
        if path_length(&self.points) < 0.5 {
            AutoGestureKind::Static
        } else {
            AutoGestureKind::Motion
        }
    }
}

/// A resampled motion template.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionTemplate {
    /// Gesture id matched by the template.
    pub gesture_id: GestureId,
    /// Resampled and normalized feature points.
    pub points: Vec<Vec<f32>>,
    /// Distance threshold for a match.
    pub threshold: f32,
}

/// Match result for a custom template.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemplateMatch {
    /// Gesture id that matched.
    pub gesture_id: GestureId,
    /// DTW distance.
    pub distance: f32,
    /// Strength in `[0, 1]`.
    pub score: f32,
}

/// Motion-template quality metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionQualityReport {
    /// Leave-one-take-out accuracy.
    pub loto_accuracy: f32,
    /// Nearest confusions as `(gesture, confused_with, distance)`.
    pub confusions: Vec<(GestureId, GestureId, f32)>,
    /// Detected kind per gesture.
    pub auto_types: Vec<(GestureId, AutoGestureKind)>,
}

/// A set of custom motion/two-hand DTW templates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MotionTemplateSet {
    /// Feature-version string used to build the templates.
    pub feature_version: String,
    /// Trained templates.
    pub templates: Vec<MotionTemplate>,
}

impl MotionTemplateSet {
    /// Trains templates from positive takes and returns quality metrics.
    #[must_use]
    pub fn train(
        takes: &[MotionTake],
        feature_version: impl Into<String>,
    ) -> (Self, MotionQualityReport) {
        let set = Self {
            feature_version: feature_version.into(),
            templates: build_templates(takes),
        };
        let report = set.quality(takes);
        (set, report)
    }

    /// Returns whether templates should be rebuilt for a new feature version.
    #[must_use]
    pub fn needs_rebuild(&self, feature_version: &str) -> bool {
        self.feature_version != feature_version
    }

    /// Matches one trajectory against the template set.
    #[must_use]
    pub fn match_take(&self, take: &MotionTake) -> Option<TemplateMatch> {
        let points = resample(&take.points, 32);
        let mut distances: Vec<(GestureId, f32, f32)> = self
            .templates
            .iter()
            .map(|template| {
                (
                    template.gesture_id,
                    dtw_distance(&points, &template.points),
                    template.threshold,
                )
            })
            .collect();
        distances.sort_by(|left, right| left.1.total_cmp(&right.1));
        let (gesture_id, distance, threshold) = *distances.first()?;
        let margin_ok = distances
            .get(1)
            .is_none_or(|second| second.1 >= distance * 1.2);
        if distance <= threshold && margin_ok {
            Some(TemplateMatch {
                gesture_id,
                distance,
                score: (1.0 - distance / threshold.max(1.0e-4)).clamp(0.0, 1.0),
            })
        } else {
            None
        }
    }

    fn quality(&self, takes: &[MotionTake]) -> MotionQualityReport {
        let mut correct = 0usize;
        let mut total = 0usize;
        let mut confusions = Vec::new();
        for (index, take) in takes.iter().enumerate() {
            let other_takes: Vec<MotionTake> = takes
                .iter()
                .enumerate()
                .filter_map(|(other_index, other)| {
                    if other_index == index {
                        None
                    } else {
                        Some(other.clone())
                    }
                })
                .collect();
            if other_takes.is_empty() {
                continue;
            }
            let held_out_set = Self {
                feature_version: self.feature_version.clone(),
                templates: build_templates(&other_takes),
            };
            if let Some(prediction) = held_out_set.match_take(take) {
                total += 1;
                if prediction.gesture_id == take.gesture_id {
                    correct += 1;
                } else {
                    confusions.push((take.gesture_id, prediction.gesture_id, prediction.distance));
                }
            } else {
                total += 1;
            }
        }
        let auto_types = takes
            .iter()
            .map(|take| (take.gesture_id, take.auto_kind()))
            .collect();
        MotionQualityReport {
            loto_accuracy: if total == 0 {
                1.0
            } else {
                correct as f32 / total as f32
            },
            confusions,
            auto_types,
        }
    }
}

fn build_templates(takes: &[MotionTake]) -> Vec<MotionTemplate> {
    let mut templates = Vec::new();
    for take in takes {
        let points = resample(&take.points, 32);
        let threshold = threshold_for_take(take, takes);
        templates.push(MotionTemplate {
            gesture_id: take.gesture_id,
            points,
            threshold,
        });
    }
    templates
}

fn straightness(samples: &[MotionSample]) -> f32 {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return 0.0;
    };
    let net = first.palm.distance(last.palm);
    let path = samples
        .windows(2)
        .map(|pair| pair[0].palm.distance(pair[1].palm))
        .sum::<f32>();
    if path <= 1.0e-6 { 0.0 } else { net / path }
}

fn centroid(points: impl Iterator<Item = Point2>) -> Point2 {
    let mut count = 0usize;
    let mut x = 0.0;
    let mut y = 0.0;
    for point in points {
        count += 1;
        x += point.x;
        y += point.y;
    }
    if count == 0 {
        return Point2::default();
    }
    Point2 {
        x: x / count as f32,
        y: y / count as f32,
    }
}

fn signed_turns(samples: &[MotionSample], center: Point2) -> f32 {
    samples
        .windows(2)
        .map(|pair| {
            let a0 = (pair[0].index.y - center.y).atan2(pair[0].index.x - center.x);
            let a1 = (pair[1].index.y - center.y).atan2(pair[1].index.x - center.x);
            let mut delta = a1 - a0;
            while delta > std::f32::consts::PI {
                delta -= 2.0 * std::f32::consts::PI;
            }
            while delta < -std::f32::consts::PI {
                delta += 2.0 * std::f32::consts::PI;
            }
            delta.to_degrees()
        })
        .sum()
}

fn cosine2(left: Point2, right: Point2) -> f32 {
    let dot = left.x.mul_add(right.x, left.y * right.y);
    let norms = (left.x.hypot(left.y) * right.x.hypot(right.y)).max(1.0e-6);
    dot / norms
}

fn path_length(points: &[Vec<f32>]) -> f32 {
    points
        .windows(2)
        .map(|pair| euclidean(&pair[0], &pair[1]))
        .sum()
}

fn threshold_for_take(take: &MotionTake, takes: &[MotionTake]) -> f32 {
    let own = resample(&take.points, 32);
    let mut distances: Vec<f32> = takes
        .iter()
        .filter(|other| other.gesture_id == take.gesture_id && !std::ptr::eq(*other, take))
        .map(|other| dtw_distance(&own, &resample(&other.points, 32)))
        .collect();
    if distances.is_empty() {
        return 1.0;
    }
    distances.sort_by(f32::total_cmp);
    (distances.iter().sum::<f32>() / distances.len() as f32 * 1.3).max(0.15)
}

fn resample(points: &[Vec<f32>], target_len: usize) -> Vec<Vec<f32>> {
    if points.is_empty() || target_len == 0 {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0].clone(); target_len];
    }
    let mut out = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let pos = i as f32 * (points.len() - 1) as f32 / (target_len - 1).max(1) as f32;
        let left = pos.floor() as usize;
        let right = (left + 1).min(points.len() - 1);
        let t = pos - left as f32;
        let mut row = Vec::with_capacity(points[left].len());
        for dim in 0..points[left].len() {
            let right_value = points[right].get(dim).copied().unwrap_or(0.0);
            row.push(points[left][dim] * (1.0 - t) + right_value * t);
        }
        out.push(row);
    }
    normalize_trajectory(&mut out);
    out
}

fn normalize_trajectory(points: &mut [Vec<f32>]) {
    let Some(first) = points.first().cloned() else {
        return;
    };
    for point in points.iter_mut() {
        for (index, value) in point.iter_mut().enumerate() {
            *value -= first.get(index).copied().unwrap_or(0.0);
        }
    }
    let scale = points
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(0.0_f32, f32::max)
        .max(1.0);
    for point in points {
        for value in point {
            *value /= scale;
        }
    }
}

fn dtw_distance(left: &[Vec<f32>], right: &[Vec<f32>]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return f32::INFINITY;
    }
    let n = left.len();
    let m = right.len();
    let band = ((n.max(m) as f32) * 0.2).ceil() as usize + 1;
    let mut prev = vec![f32::INFINITY; m + 1];
    let mut curr = vec![f32::INFINITY; m + 1];
    prev[0] = 0.0;
    for i in 1..=n {
        let start = i.saturating_sub(band).max(1);
        let end = (i + band).min(m);
        curr.fill(f32::INFINITY);
        for j in start..=end {
            let cost = euclidean(&left[i - 1], &right[j - 1]);
            curr[j] = cost + prev[j].min(curr[j - 1]).min(prev[j - 1]);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m] / (n + m) as f32
}

fn euclidean(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            let delta = left - right;
            delta * delta
        })
        .sum::<f32>()
        .sqrt()
}
