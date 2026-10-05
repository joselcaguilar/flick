use std::{collections::VecDeque, time::Duration};

use flick_core::{FaceKeypoints, HandObservation};
use nalgebra::{DMatrix, Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CameraIntrinsics,
    math::{
        Mat3, Mat6, Vec3, Vec6, a3, median, nearest_rotation, rotation_from_scaled_axis,
        scaled_axis_from_rotation, unit_or_z, v3,
    },
};

/// Current ray/triangulation estimator version.
pub const DEFAULT_ESTIMATOR_VERSION: &str = "spatial.ray.v1";

/// Requested pointing ray model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RayModel {
    /// Try eye-rooted first and automatically fall back to finger-only.
    Auto,
    /// Prefer eye-rooted ray.
    Eye,
    /// Use index MCP → index tip only.
    Finger,
}

/// Actual ray source used for one estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaySource {
    /// Eye midpoint or dominant eye through the index fingertip.
    EyeRooted,
    /// Index MCP through index fingertip.
    FingerOnly,
}

/// Dominant-eye preference for eye-rooted rays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DominantEye {
    /// Midpoint between both eyes.
    Center,
    /// Left eye from the user's perspective.
    Left,
    /// Right eye from the user's perspective.
    Right,
}

/// Settings for ray estimation and filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RayEstimatorSettings {
    /// Ray model preference.
    pub model: RayModel,
    /// Dominant-eye preference.
    pub dominant_eye: DominantEye,
    /// Assumed interpupillary distance in meters.
    pub interpupillary_distance_m: f32,
    /// Minimum accepted face detector confidence when present.
    pub min_face_confidence: f32,
    /// One-Euro minimum cutoff.
    pub min_cutoff: f32,
    /// One-Euro speed coefficient.
    pub beta: f32,
    /// One-Euro derivative cutoff.
    pub d_cutoff: f32,
    /// Median decision window.
    pub median_window: Duration,
}

impl Default for RayEstimatorSettings {
    fn default() -> Self {
        Self {
            model: RayModel::Auto,
            dominant_eye: DominantEye::Center,
            interpupillary_distance_m: 0.063,
            min_face_confidence: 0.5,
            min_cutoff: 1.0,
            beta: 0.02,
            d_cutoff: 1.0,
            median_window: Duration::from_millis(250),
        }
    }
}

/// A pointing ray in camera coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointingRay {
    /// Ray origin in meters, camera frame.
    pub origin: [f32; 3],
    /// Unit direction in camera frame.
    pub direction: [f32; 3],
    /// Ray model that produced this estimate.
    pub source: RaySource,
    /// Estimator version.
    pub estimator_version: String,
    /// Reprojection error of the hand pose in pixels.
    pub reprojection_error_px: f32,
}

impl PointingRay {
    /// Creates a ray, normalizing the direction.
    #[must_use]
    pub fn new(origin: [f32; 3], direction: [f32; 3], source: RaySource) -> Self {
        Self {
            origin,
            direction: a3(unit_or_z(v3(direction))),
            source,
            estimator_version: DEFAULT_ESTIMATOR_VERSION.to_owned(),
            reprojection_error_px: 0.0,
        }
    }
}

/// Estimated 3D hand landmark positions in camera coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandPose {
    /// Camera-space hand landmarks in meters.
    pub camera_landmarks: [[f32; 3]; 21],
    /// Mean reprojection error in pixels.
    pub reprojection_error_px: f32,
}

/// Ray-estimation failure.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum RayEstimateError {
    /// Fewer than six valid hand correspondences were available.
    #[error("not enough valid hand landmarks")]
    NotEnoughLandmarks,
    /// The hand pose could not be solved.
    #[error("hand pose solve failed")]
    PoseSolveFailed,
    /// The requested eye-rooted ray was impossible and finger fallback was disabled.
    #[error("face keypoints are unavailable for eye-rooted ray")]
    MissingFace,
}

/// Stateful ray estimator with One-Euro and 250 ms median filtering.
#[derive(Debug, Clone)]
pub struct RayEstimator {
    intrinsics: CameraIntrinsics,
    settings: RayEstimatorSettings,
    origin_filter: OneEuroVec3,
    direction_filter: OneEuroVec3,
    samples: VecDeque<RaySample>,
}

impl RayEstimator {
    /// Creates an estimator for one camera.
    #[must_use]
    pub fn new(intrinsics: CameraIntrinsics, settings: RayEstimatorSettings) -> Self {
        Self {
            intrinsics,
            origin_filter: OneEuroVec3::new(settings.min_cutoff, settings.beta, settings.d_cutoff),
            direction_filter: OneEuroVec3::new(
                settings.min_cutoff,
                settings.beta,
                settings.d_cutoff,
            ),
            samples: VecDeque::with_capacity(32),
            settings,
        }
    }

    /// Returns the estimator settings.
    #[must_use]
    pub fn settings(&self) -> &RayEstimatorSettings {
        &self.settings
    }

    /// Returns the camera intrinsics.
    #[must_use]
    pub fn intrinsics(&self) -> &CameraIntrinsics {
        &self.intrinsics
    }

    /// Estimates a filtered pointing ray for a hand observation.
    pub fn estimate(
        &mut self,
        hand: &HandObservation,
        face: Option<&FaceKeypoints>,
        at: std::time::Instant,
    ) -> Result<PointingRay, RayEstimateError> {
        let pose = estimate_hand_pose(hand, &self.intrinsics)?;
        let raw = self.raw_ray(hand, &pose, face)?;
        let origin = self.origin_filter.filter(v3(raw.origin), at);
        let direction = self
            .direction_filter
            .filter(unit_or_z(v3(raw.direction)), at);
        let filtered = PointingRay {
            origin: a3(origin),
            direction: a3(unit_or_z(direction)),
            source: raw.source,
            estimator_version: raw.estimator_version,
            reprojection_error_px: raw.reprojection_error_px,
        };
        self.push_and_median(filtered, at)
    }

    /// Estimates an unfiltered hand pose in camera coordinates.
    pub fn estimate_pose(&self, hand: &HandObservation) -> Result<HandPose, RayEstimateError> {
        estimate_hand_pose(hand, &self.intrinsics)
    }

    fn raw_ray(
        &self,
        hand: &HandObservation,
        pose: &HandPose,
        face: Option<&FaceKeypoints>,
    ) -> Result<PointingRay, RayEstimateError> {
        let mcp = v3(pose.camera_landmarks[5]);
        let tip = v3(pose.camera_landmarks[8]);
        let finger_direction = unit_or_z(tip - mcp);
        let finger_ray = PointingRay {
            origin: a3(mcp),
            direction: a3(finger_direction),
            source: RaySource::FingerOnly,
            estimator_version: DEFAULT_ESTIMATOR_VERSION.to_owned(),
            reprojection_error_px: pose.reprojection_error_px,
        };

        if self.settings.model == RayModel::Finger {
            return Ok(finger_ray);
        }

        match face.and_then(|face| self.eye_origin(face)) {
            Some(eye_origin) => Ok(PointingRay {
                origin: a3(eye_origin),
                direction: a3(unit_or_z(tip - eye_origin)),
                source: RaySource::EyeRooted,
                estimator_version: DEFAULT_ESTIMATOR_VERSION.to_owned(),
                reprojection_error_px: pose.reprojection_error_px,
            }),
            None if self.settings.model == RayModel::Eye => Err(RayEstimateError::MissingFace),
            None => {
                let _ = hand;
                Ok(finger_ray)
            }
        }
    }

    fn eye_origin(&self, face: &FaceKeypoints) -> Option<Vec3> {
        if face
            .confidence
            .is_some_and(|confidence| confidence < self.settings.min_face_confidence)
        {
            return None;
        }
        eye_point(
            face,
            &self.intrinsics,
            self.settings.interpupillary_distance_m,
            self.settings.dominant_eye,
        )
    }

    /// Forgets filtered history, so the next hand's ray isn't blended with the last one's.
    pub fn reset(&mut self) {
        self.origin_filter.reset();
        self.direction_filter.reset();
        self.samples.clear();
    }

    fn push_and_median(
        &mut self,
        ray: PointingRay,
        at: std::time::Instant,
    ) -> Result<PointingRay, RayEstimateError> {
        if self.samples.len() == 32 {
            let _ = self.samples.pop_front();
        }
        self.samples.push_back(RaySample { at, ray });
        while self
            .samples
            .front()
            .is_some_and(|sample| at.duration_since(sample.at) > self.settings.median_window)
        {
            let _ = self.samples.pop_front();
        }
        Ok(self.median_ray())
    }

    fn median_ray(&self) -> PointingRay {
        let len = self.samples.len().min(32);
        let mut ox = [0.0_f32; 32];
        let mut oy = [0.0_f32; 32];
        let mut oz = [0.0_f32; 32];
        let mut dx = [0.0_f32; 32];
        let mut dy = [0.0_f32; 32];
        let mut dz = [0.0_f32; 32];
        let mut reprojection = 0.0;
        let mut source = RaySource::FingerOnly;
        for (idx, sample) in self.samples.iter().take(len).enumerate() {
            ox[idx] = sample.ray.origin[0];
            oy[idx] = sample.ray.origin[1];
            oz[idx] = sample.ray.origin[2];
            dx[idx] = sample.ray.direction[0];
            dy[idx] = sample.ray.direction[1];
            dz[idx] = sample.ray.direction[2];
            reprojection += sample.ray.reprojection_error_px;
            source = sample.ray.source;
        }
        let origin = [
            median(&mut ox[..len]),
            median(&mut oy[..len]),
            median(&mut oz[..len]),
        ];
        let direction = a3(unit_or_z(Vec3::new(
            median(&mut dx[..len]),
            median(&mut dy[..len]),
            median(&mut dz[..len]),
        )));
        PointingRay {
            origin,
            direction,
            source,
            estimator_version: DEFAULT_ESTIMATOR_VERSION.to_owned(),
            reprojection_error_px: if len == 0 {
                0.0
            } else {
                reprojection / len as f32
            },
        }
    }
}

#[derive(Debug, Clone)]
struct RaySample {
    at: std::time::Instant,
    ray: PointingRay,
}

#[derive(Debug, Clone)]
struct OneEuroVec3 {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x: LowPassVec3,
    dx: LowPassVec3,
    last: Option<std::time::Instant>,
}

impl OneEuroVec3 {
    fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x: LowPassVec3::default(),
            dx: LowPassVec3::default(),
            last: None,
        }
    }

    /// The next sample starts a fresh filter.
    fn reset(&mut self) {
        self.last = None;
    }

    fn filter(&mut self, value: Vec3, at: std::time::Instant) -> Vec3 {
        let Some(last) = self.last else {
            self.last = Some(at);
            self.x.reset(value);
            self.dx.reset(Vec3::zeros());
            return value;
        };
        let dt = at
            .duration_since(last)
            .as_secs_f32()
            .clamp(1.0 / 240.0, 1.0);
        self.last = Some(at);
        let previous = self.x.value.unwrap_or(value);
        let derivative = (value - previous) / dt;
        let edx = self.dx.filter(derivative, alpha(self.d_cutoff, dt));
        let cutoff = self.min_cutoff + self.beta * edx.norm();
        self.x.filter(value, alpha(cutoff, dt))
    }
}

#[derive(Debug, Clone, Default)]
struct LowPassVec3 {
    value: Option<Vec3>,
}

impl LowPassVec3 {
    fn reset(&mut self, value: Vec3) {
        self.value = Some(value);
    }

    fn filter(&mut self, value: Vec3, alpha: f32) -> Vec3 {
        let filtered = match self.value {
            Some(previous) => alpha * value + (1.0 - alpha) * previous,
            None => value,
        };
        self.value = Some(filtered);
        filtered
    }
}

fn alpha(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(1.0e-4));
    1.0 / (1.0 + tau / dt.max(1.0e-4))
}

/// A pointing hand's wrist stays within arm's reach of its owner's eyes, plus range noise.
const MAX_EYE_WRIST_M: f32 = 1.2;

/// Picks the face of the person pointing: the one whose eyes sit nearest the hand's wrist in 3D.
/// With several people in view, the eye-rooted ray then starts at the pointer's own eyes.
/// `None` when no face is within arm's reach, so aiming falls back to the finger.
#[must_use]
pub fn owner_face<'a>(
    faces: &'a [FaceKeypoints],
    hand: &HandObservation,
    intrinsics: &CameraIntrinsics,
    interpupillary_distance_m: f32,
) -> Option<&'a FaceKeypoints> {
    let wrist = v3(estimate_hand_pose(hand, intrinsics).ok()?.camera_landmarks[0]);
    faces
        .iter()
        .filter_map(|face| {
            let eyes = eye_point(
                face,
                intrinsics,
                interpupillary_distance_m,
                DominantEye::Center,
            )?;
            let distance = (eyes - wrist).norm();
            (distance <= MAX_EYE_WRIST_M).then_some((face, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(face, _)| face)
}

/// Eye position in camera space, ranged by the assumed interpupillary distance.
fn eye_point(
    face: &FaceKeypoints,
    intrinsics: &CameraIntrinsics,
    interpupillary_distance_m: f32,
    eye: DominantEye,
) -> Option<Vec3> {
    let left = intrinsics.normalized_camera_xy(face.points[0]);
    let right = intrinsics.normalized_camera_xy(face.points[1]);
    let dx = left[0] - right[0];
    let dy = left[1] - right[1];
    let angular_width = (dx * dx + dy * dy).sqrt();
    if angular_width <= 1.0e-4 || !angular_width.is_finite() {
        return None;
    }
    let depth = (interpupillary_distance_m / angular_width).clamp(0.2, 5.0);
    let chosen = match eye {
        DominantEye::Center => [(left[0] + right[0]) * 0.5, (left[1] + right[1]) * 0.5],
        DominantEye::Left => left,
        DominantEye::Right => right,
    };
    Some(Vec3::new(chosen[0] * depth, chosen[1] * depth, depth))
}

fn estimate_hand_pose(
    hand: &HandObservation,
    intrinsics: &CameraIntrinsics,
) -> Result<HandPose, RayEstimateError> {
    let pose = solve_pose_dlt_lm(&hand.world, &hand.image, intrinsics)?;
    let mut camera_landmarks = [[0.0_f32; 3]; 21];
    for (idx, point) in hand.world.iter().enumerate() {
        camera_landmarks[idx] = a3(pose.rotation * v3(*point) + pose.translation);
    }
    Ok(HandPose {
        camera_landmarks,
        reprojection_error_px: pose.reprojection_error_px,
    })
}

#[derive(Debug, Clone)]
struct PoseSolution {
    rotation: Mat3,
    translation: Vec3,
    reprojection_error_px: f32,
}

fn solve_pose_dlt_lm(
    world: &[[f32; 3]; 21],
    image: &[[f32; 3]; 21],
    intrinsics: &CameraIntrinsics,
) -> Result<PoseSolution, RayEstimateError> {
    let valid_count = world
        .iter()
        .zip(image.iter())
        .filter(|(w, i)| w.iter().all(|v| v.is_finite()) && i[0].is_finite() && i[1].is_finite())
        .count();
    if valid_count < 6 {
        return Err(RayEstimateError::NotEnoughLandmarks);
    }

    let mut a = DMatrix::<f32>::zeros(valid_count * 2, 12);
    let mut row = 0;
    for (w, i) in world.iter().zip(image.iter()) {
        if !(w.iter().all(|v| v.is_finite()) && i[0].is_finite() && i[1].is_finite()) {
            continue;
        }
        let [x, y] = intrinsics.normalized_camera_xy([i[0], i[1]]);
        let wx = w[0];
        let wy = w[1];
        let wz = w[2];
        a[(row, 0)] = wx;
        a[(row, 1)] = wy;
        a[(row, 2)] = wz;
        a[(row, 3)] = 1.0;
        a[(row, 8)] = -x * wx;
        a[(row, 9)] = -x * wy;
        a[(row, 10)] = -x * wz;
        a[(row, 11)] = -x;
        row += 1;
        a[(row, 4)] = wx;
        a[(row, 5)] = wy;
        a[(row, 6)] = wz;
        a[(row, 7)] = 1.0;
        a[(row, 8)] = -y * wx;
        a[(row, 9)] = -y * wy;
        a[(row, 10)] = -y * wz;
        a[(row, 11)] = -y;
        row += 1;
    }

    let svd = a.svd(true, true);
    let Some(v_t) = svd.v_t else {
        return Err(RayEstimateError::PoseSolveFailed);
    };
    if v_t.nrows() == 0 || v_t.ncols() != 12 {
        return Err(RayEstimateError::PoseSolveFailed);
    }
    let p = v_t.row(v_t.nrows() - 1);
    let mut m = Matrix3::new(p[0], p[1], p[2], p[4], p[5], p[6], p[8], p[9], p[10]);
    let mut t = Vector3::new(p[3], p[7], p[11]);
    let scale = ((m.row(0).norm() + m.row(1).norm() + m.row(2).norm()) / 3.0).max(1.0e-6);
    m /= scale;
    t /= scale;
    let mean_z = world
        .iter()
        .map(|point| (m * v3(*point) + t).z)
        .sum::<f32>()
        / 21.0;
    if mean_z < 0.0 {
        m = -m;
        t = -t;
    }
    let Some(rotation) = nearest_rotation(m) else {
        return Err(RayEstimateError::PoseSolveFailed);
    };
    let mut params = Vec6::zeros();
    params
        .fixed_rows_mut::<3>(0)
        .copy_from(&scaled_axis_from_rotation(rotation));
    params.fixed_rows_mut::<3>(3).copy_from(&t);
    params = refine_pose(params, world, image, intrinsics);
    let rotation = rotation_from_scaled_axis(params.fixed_rows::<3>(0).into_owned());
    let translation = params.fixed_rows::<3>(3).into_owned();
    Ok(PoseSolution {
        rotation,
        translation,
        reprojection_error_px: reprojection_error_px(
            rotation,
            translation,
            world,
            image,
            intrinsics,
        ),
    })
}

fn refine_pose(
    mut params: Vec6,
    world: &[[f32; 3]; 21],
    image: &[[f32; 3]; 21],
    intrinsics: &CameraIntrinsics,
) -> Vec6 {
    let mut damping = 1.0e-3;
    let mut current = residuals(params, world, image, intrinsics);
    let mut current_cost = cost(&current);
    for _ in 0..8 {
        let mut jtj = Mat6::zeros();
        let mut jtr = Vec6::zeros();
        for param_idx in 0..6 {
            let step = if param_idx < 3 { 1.0e-4 } else { 1.0e-3 };
            let mut plus = params;
            plus[param_idx] += step;
            let mut minus = params;
            minus[param_idx] -= step;
            let r_plus = residuals(plus, world, image, intrinsics);
            let r_minus = residuals(minus, world, image, intrinsics);
            for residual_idx in 0..42 {
                let deriv = (r_plus[residual_idx] - r_minus[residual_idx]) / (2.0 * step);
                jtr[param_idx] += deriv * current[residual_idx];
                for other_idx in 0..=param_idx {
                    let other_deriv = derivative_component(
                        other_idx,
                        params,
                        world,
                        image,
                        intrinsics,
                        residual_idx,
                    );
                    jtj[(param_idx, other_idx)] += deriv * other_deriv;
                }
            }
        }
        for r in 0..6 {
            for c in 0..r {
                jtj[(c, r)] = jtj[(r, c)];
            }
            jtj[(r, r)] += damping;
        }
        let rhs = -jtr;
        let Some(delta) = jtj.lu().solve(&rhs) else {
            break;
        };
        if delta.norm() < 1.0e-5 {
            break;
        }
        let candidate = params + delta;
        let candidate_residuals = residuals(candidate, world, image, intrinsics);
        let candidate_cost = cost(&candidate_residuals);
        if candidate_cost < current_cost {
            params = candidate;
            current = candidate_residuals;
            current_cost = candidate_cost;
            damping *= 0.5;
        } else {
            damping *= 2.0;
        }
    }
    params
}

fn derivative_component(
    param_idx: usize,
    params: Vec6,
    world: &[[f32; 3]; 21],
    image: &[[f32; 3]; 21],
    intrinsics: &CameraIntrinsics,
    residual_idx: usize,
) -> f32 {
    let step = if param_idx < 3 { 1.0e-4 } else { 1.0e-3 };
    let mut plus = params;
    plus[param_idx] += step;
    let mut minus = params;
    minus[param_idx] -= step;
    let r_plus = residuals(plus, world, image, intrinsics);
    let r_minus = residuals(minus, world, image, intrinsics);
    (r_plus[residual_idx] - r_minus[residual_idx]) / (2.0 * step)
}

fn residuals(
    params: Vec6,
    world: &[[f32; 3]; 21],
    image: &[[f32; 3]; 21],
    intrinsics: &CameraIntrinsics,
) -> [f32; 42] {
    let rotation = rotation_from_scaled_axis(params.fixed_rows::<3>(0).into_owned());
    let translation = params.fixed_rows::<3>(3).into_owned();
    let mut out = [0.0_f32; 42];
    for (idx, (w, i)) in world.iter().zip(image.iter()).enumerate() {
        let camera = rotation * v3(*w) + translation;
        let [target_x, target_y] = intrinsics.normalized_camera_xy([i[0], i[1]]);
        if camera.z <= 1.0e-4 || !camera.z.is_finite() {
            out[idx * 2] = 10.0;
            out[idx * 2 + 1] = 10.0;
        } else {
            out[idx * 2] = camera.x / camera.z - target_x;
            out[idx * 2 + 1] = camera.y / camera.z - target_y;
        }
    }
    out
}

fn cost(residuals: &[f32; 42]) -> f32 {
    residuals.iter().map(|r| r * r).sum::<f32>()
}

fn reprojection_error_px(
    rotation: Mat3,
    translation: Vec3,
    world: &[[f32; 3]; 21],
    image: &[[f32; 3]; 21],
    intrinsics: &CameraIntrinsics,
) -> f32 {
    let mut sum = 0.0;
    let mut count = 0_u32;
    for (w, i) in world.iter().zip(image.iter()) {
        let camera = rotation * v3(*w) + translation;
        if let Some(projected) = intrinsics.project(a3(camera)) {
            let dx = (projected[0] - i[0]) * intrinsics.width.max(1) as f32;
            let dy = (projected[1] - i[1]) * intrinsics.height.max(1) as f32;
            sum += (dx * dx + dy * dy).sqrt();
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}
