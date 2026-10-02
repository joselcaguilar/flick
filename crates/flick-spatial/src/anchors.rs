use flick_core::{AnchorId, AnchorKind, AnchorStatus};
use nalgebra::Matrix3;
use serde::{Deserialize, Serialize, de, ser::SerializeMap};
use thiserror::Error;

use crate::{
    PointingRay, RaySource,
    math::{Mat3, Vec3, a3, angle_deg as vec_angle_deg, unit_or_z, v3},
};

const PARALLEL_FALLBACK_DEG: f32 = 5.0;
/// Rays from one seat cross near the hand, not at the device (spec 09 §4.1).
const MIN_TRIANGULATION_BASELINE_M: f32 = 0.5;
/// A triangulated device must sit at least this far ahead of every teaching origin.
const MIN_TARGET_DISTANCE_M: f32 = 0.3;
/// Rays that miss their closest point by more than this never converged on one device.
const MAX_TRIANGULATION_RESIDUAL_DEG: f32 = 10.0;
const DISTINCTIVENESS_WARN_DEG: f32 = 15.0;

/// Opaque `anchors.verb_params` JSON interpreted by `flick-ha` verb resolution.
pub type VerbParams = serde_json::Value;

/// Target bound to a taught anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeachTarget {
    /// Home Assistant entity id.
    Entity(String),
    /// Home Assistant device id.
    Device(String),
    /// Home Assistant area id.
    Area(String),
}

impl Serialize for TeachTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::Entity(entity_id) => map.serialize_entry("entity_id", entity_id)?,
            Self::Device(device_id) => map.serialize_entry("device_id", device_id)?,
            Self::Area(area_id) => map.serialize_entry("area_id", area_id)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TeachTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct TargetJson {
            entity_id: Option<String>,
            device_id: Option<String>,
            area_id: Option<String>,
        }

        let target = TargetJson::deserialize(deserializer)?;
        let present = target.entity_id.iter().count()
            + target.device_id.iter().count()
            + target.area_id.iter().count();
        if present != 1 {
            return Err(de::Error::custom(
                "target must contain exactly one of entity_id, device_id or area_id",
            ));
        }
        if let Some(entity_id) = target.entity_id {
            Ok(Self::Entity(entity_id))
        } else if let Some(device_id) = target.device_id {
            Ok(Self::Device(device_id))
        } else if let Some(area_id) = target.area_id {
            Ok(Self::Area(area_id))
        } else {
            unreachable!("present count was validated")
        }
    }
}

/// Spatial representation of a taught device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AnchorGeometry {
    /// Triangulated 3D point in camera coordinates.
    Point3d {
        /// Position in meters.
        position: [f32; 3],
        /// 3×3 covariance matrix, row-major.
        covariance: [[f32; 3]; 3],
    },
    /// Direction fallback valid near the teaching origin.
    Direction {
        /// Unit direction in camera coordinates.
        direction: [f32; 3],
        /// Mean teaching origin in meters.
        teach_origin: [f32; 3],
        /// 3×3 covariance matrix, row-major.
        covariance: [[f32; 3]; 3],
    },
}

impl AnchorGeometry {
    /// Returns the core persistence kind.
    #[must_use]
    pub const fn kind(&self) -> AnchorKind {
        match self {
            Self::Point3d { .. } => AnchorKind::Point3d,
            Self::Direction { .. } => AnchorKind::Direction,
        }
    }
}

/// A taught target anchor in a place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// Stable anchor id.
    pub id: AnchorId,
    /// Display name.
    pub name: String,
    /// HA entity/device/area target.
    pub target: TeachTarget,
    /// Resolution domain (`fan`, `light`, …).
    pub domain: String,
    /// Geometry used for scoring.
    pub geometry: AnchorGeometry,
    /// Angular uncertainty used to widen hover tolerance.
    pub uncertainty_deg: f32,
    /// Per-anchor targeted verb parameters such as taught fan levels.
    pub verb_params: VerbParams,
    /// Current persistence status.
    pub status: AnchorStatus,
    /// Ray/triangulation version that produced this anchor.
    pub estimator_version: String,
    /// Ray model the anchor was taught with; live aim uses the same model when it can.
    #[serde(default = "legacy_ray_source")]
    pub ray_source: RaySource,
    /// Finger-only model of an eye-rooted anchor, aimed at while no face is in view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finger_aim: Option<FingerAim>,
}

/// Finger-only geometry taught alongside an eye-rooted anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FingerAim {
    /// Geometry fitted to the finger-only teaching rays.
    pub geometry: AnchorGeometry,
    /// Angular uncertainty of that geometry.
    pub uncertainty_deg: f32,
}

const fn legacy_ray_source() -> RaySource {
    RaySource::FingerOnly
}

impl Anchor {
    /// Returns the core persistence kind.
    #[must_use]
    pub const fn kind(&self) -> AnchorKind {
        self.geometry.kind()
    }

    /// Direction from a ray origin toward this anchor.
    #[must_use]
    pub fn direction_from(&self, origin: [f32; 3]) -> [f32; 3] {
        match &self.geometry {
            AnchorGeometry::Point3d { position, .. } => a3(unit_or_z(v3(*position) - v3(origin))),
            AnchorGeometry::Direction { direction, .. } => *direction,
        }
    }
}

/// One median teaching observation from a spot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeachObservation {
    /// Spot index from the teach flow.
    pub spot_index: u32,
    /// Median pointing ray captured at the spot.
    pub ray: PointingRay,
    /// Number of frames represented by this median ray.
    pub frames: u32,
    /// Within-spot jitter in degrees.
    pub ray_jitter_deg: f32,
}

/// Minimal stored observation form used for OTA recompute.
pub type RayObservation = TeachObservation;

/// Quality metrics returned by teach helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorQuality {
    /// 0–1 confidence from ray count, residual and jitter.
    pub confidence: f32,
    /// Mean residual angular error in degrees.
    pub residual_deg: f32,
    /// Maximum pairwise teaching ray angle in degrees.
    pub max_ray_separation_deg: f32,
    /// Whether the geometry fell back to `direction` because the rays could not triangulate a
    /// device (taught from one seat, near-parallel, or never converged ahead of the user).
    pub direction_fallback: bool,
}

/// Pairwise distinctiveness warning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistinctivenessWarning {
    /// Newly taught anchor id.
    pub anchor_id: AnchorId,
    /// Existing anchor id that is too close from the teaching position.
    pub other_anchor_id: AnchorId,
    /// Angular separation in degrees.
    pub separation_deg: f32,
}

/// Final teach result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeachingOutcome {
    /// Derived anchor.
    pub anchor: Anchor,
    /// Quality metrics.
    pub quality: AnchorQuality,
    /// Distinctiveness warnings below 15°.
    pub distinctiveness_warnings: Vec<DistinctivenessWarning>,
}

/// Anchor scoring output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnchorScore {
    /// Angular error between ray and anchor.
    pub angular_error_deg: f32,
    /// Gaussian score in `[0, 1]`.
    pub score: f32,
    /// Angular uncertainty used by the score.
    pub sigma_deg: f32,
}

/// Teaching failure.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum TeachingError {
    /// No teaching observations were captured.
    #[error("at least one teaching observation is required")]
    NoObservations,
    /// Triangulation matrix was singular.
    #[error("teaching rays could not be triangulated")]
    SingularTriangulation,
}

/// Stateful helper for collecting teaching observations.
#[derive(Debug, Clone)]
pub struct TeachSession {
    id: AnchorId,
    name: String,
    target: TeachTarget,
    domain: String,
    observations: Vec<TeachObservation>,
    estimator_version: String,
}

impl TeachSession {
    /// Starts a teach session for one target.
    #[must_use]
    pub fn new(
        id: AnchorId,
        name: impl Into<String>,
        target: TeachTarget,
        domain: impl Into<String>,
        estimator_version: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            target,
            domain: domain.into(),
            observations: Vec::with_capacity(3),
            estimator_version: estimator_version.into(),
        }
    }

    /// Adds one median observation from a teaching spot.
    pub fn add_observation(&mut self, observation: TeachObservation) {
        self.observations.push(observation);
    }

    /// Returns the captured observations.
    #[must_use]
    pub fn observations(&self) -> &[TeachObservation] {
        &self.observations
    }

    /// Number of spots captured so far; one spot holds one observation per steady ray model.
    #[must_use]
    pub fn spots(&self) -> u32 {
        self.observations.last().map_or(0, |obs| obs.spot_index)
    }

    /// Builds the anchor and quality report.
    pub fn finish(&self, existing: &[Anchor]) -> Result<TeachingOutcome, TeachingError> {
        build_anchor(
            self.id,
            self.name.clone(),
            self.target.clone(),
            self.domain.clone(),
            &self.observations,
            self.estimator_version.clone(),
            existing,
        )
    }

    /// Computes angular error for the live “Point at it again” check.
    #[must_use]
    pub fn live_angular_error(anchor: &Anchor, ray: &PointingRay) -> f32 {
        angular_error_deg(anchor, ray)
    }
}

/// Recomputes an anchor from stored ray observations when the estimator version changes.
pub fn recompute_anchor(
    previous: &Anchor,
    observations: &[TeachObservation],
    estimator_version: impl Into<String>,
    existing: &[Anchor],
) -> Result<TeachingOutcome, TeachingError> {
    build_anchor(
        previous.id,
        previous.name.clone(),
        previous.target.clone(),
        previous.domain.clone(),
        observations,
        estimator_version.into(),
        existing,
    )
}

fn build_anchor(
    id: AnchorId,
    name: String,
    target: TeachTarget,
    domain: String,
    observations: &[TeachObservation],
    estimator_version: String,
    existing: &[Anchor],
) -> Result<TeachingOutcome, TeachingError> {
    if observations.is_empty() {
        return Err(TeachingError::NoObservations);
    }
    // A spot holds one observation per ray model it saw steadily. Eye-rooted rays build the
    // anchor; finger-only rays become its fallback for when no face is in view.
    let (eye, finger): (Vec<_>, Vec<_>) = observations
        .iter()
        .cloned()
        .partition(|obs| obs.ray.source == RaySource::EyeRooted);
    let (primary, ray_source) = if eye.is_empty() {
        (finger.as_slice(), RaySource::FingerOnly)
    } else {
        (eye.as_slice(), RaySource::EyeRooted)
    };
    let (geometry, quality, uncertainty_deg) = fit_geometry(primary)?;
    let finger_aim = if eye.is_empty() || finger.is_empty() {
        None
    } else {
        fit_geometry(&finger)
            .ok()
            .map(|(geometry, _, uncertainty_deg)| FingerAim {
                geometry,
                uncertainty_deg,
            })
    };
    let anchor = Anchor {
        id,
        name,
        target,
        domain,
        geometry,
        uncertainty_deg,
        verb_params: serde_json::json!({}),
        status: AnchorStatus::Ok,
        estimator_version,
        ray_source,
        finger_aim,
    };
    let distinctiveness_warnings = distinctiveness_warnings(&anchor, existing, primary);
    Ok(TeachingOutcome {
        anchor,
        quality,
        distinctiveness_warnings,
    })
}

/// Fits a `point3d` when rays from separate spots converge, otherwise a `direction`.
fn fit_geometry(
    observations: &[TeachObservation],
) -> Result<(AnchorGeometry, AnchorQuality, f32), TeachingError> {
    let max_sep = max_pairwise_ray_angle(observations);
    let triangulated = if observations.len() >= 2
        && max_sep >= PARALLEL_FALLBACK_DEG
        && max_origin_baseline(observations) >= MIN_TRIANGULATION_BASELINE_M
    {
        triangulate(observations)?
    } else {
        None
    };
    let (geometry, quality) =
        triangulated.unwrap_or_else(|| direction_anchor(observations, max_sep));
    let uncertainty_deg = quality
        .residual_deg
        .max(mean_jitter(observations))
        .clamp(1.0, 20.0);
    Ok((geometry, quality, uncertainty_deg))
}

/// Returns `None` when the rays never converged ahead of the user, so the caller falls back to
/// a direction anchor instead of storing a point at the hand.
fn triangulate(
    observations: &[TeachObservation],
) -> Result<Option<(AnchorGeometry, AnchorQuality)>, TeachingError> {
    let mut a = Mat3::zeros();
    let mut b = Vec3::zeros();
    for obs in observations {
        let origin = v3(obs.ray.origin);
        let direction = unit_or_z(v3(obs.ray.direction));
        let projector = Mat3::identity() - direction * direction.transpose();
        a += projector;
        b += projector * origin;
    }
    let Some(position) = a.lu().solve(&b) else {
        return Err(TeachingError::SingularTriangulation);
    };
    let ahead_of_every_origin = observations.iter().all(|obs| {
        unit_or_z(v3(obs.ray.direction)).dot(&(position - v3(obs.ray.origin)))
            >= MIN_TARGET_DISTANCE_M
    });
    let residual_deg = mean_angular_residual(position, observations);
    if !ahead_of_every_origin || residual_deg > MAX_TRIANGULATION_RESIDUAL_DEG {
        return Ok(None);
    }
    let residual_m = mean_ray_distance(position, observations);
    let avg_distance = observations
        .iter()
        .map(|obs| (position - v3(obs.ray.origin)).norm())
        .sum::<f32>()
        / observations.len() as f32;
    let variance = residual_m * residual_m;
    let covariance = a.try_inverse().unwrap_or_else(Mat3::identity) * variance.max(1.0e-6);
    let uncertainty_deg = (residual_m / avg_distance.max(0.1)).atan().to_degrees();
    let quality = AnchorQuality {
        confidence: confidence(observations.len(), residual_deg, mean_jitter(observations)),
        residual_deg: residual_deg.max(uncertainty_deg),
        max_ray_separation_deg: max_pairwise_ray_angle(observations),
        direction_fallback: false,
    };
    Ok(Some((
        AnchorGeometry::Point3d {
            position: a3(position),
            covariance: mat_to_array(covariance),
        },
        quality,
    )))
}

fn direction_anchor(
    observations: &[TeachObservation],
    max_sep: f32,
) -> (AnchorGeometry, AnchorQuality) {
    // Inverse-variance weights keep one shaky spot from dragging a steady one off target.
    let mut direction = Vec3::zeros();
    let mut origin = Vec3::zeros();
    let mut total_weight = 0.0;
    for obs in observations {
        let weight = obs.ray_jitter_deg.max(1.0).powi(-2);
        direction += unit_or_z(v3(obs.ray.direction)) * weight;
        origin += v3(obs.ray.origin) * weight;
        total_weight += weight;
    }
    direction = unit_or_z(direction);
    origin /= total_weight;
    let residual_deg = observations
        .iter()
        .map(|obs| vec_angle_deg(direction, v3(obs.ray.direction)))
        .sum::<f32>()
        / observations.len() as f32;
    let quality = AnchorQuality {
        confidence: confidence(observations.len(), residual_deg, mean_jitter(observations)) * 0.75,
        residual_deg: residual_deg.max(mean_jitter(observations)),
        max_ray_separation_deg: max_sep,
        direction_fallback: true,
    };
    (
        AnchorGeometry::Direction {
            direction: a3(direction),
            teach_origin: a3(origin),
            covariance: mat_to_array(Mat3::identity() * residual_deg.to_radians().powi(2)),
        },
        quality,
    )
}

/// Computes the angular error between a pointing ray and an anchor.
#[must_use]
pub fn angular_error_deg(anchor: &Anchor, ray: &PointingRay) -> f32 {
    geometry_error_deg(&anchor.geometry, ray)
}

fn geometry_error_deg(geometry: &AnchorGeometry, ray: &PointingRay) -> f32 {
    match geometry {
        AnchorGeometry::Point3d { position, .. } => {
            vec_angle_deg(v3(ray.direction), v3(*position) - v3(ray.origin))
        }
        AnchorGeometry::Direction { direction, .. } => {
            vec_angle_deg(v3(ray.direction), v3(*direction))
        }
    }
}

/// Scores one anchor with the `09-device-targeting` Gaussian angular score.
#[must_use]
pub fn score_anchor(anchor: &Anchor, ray: &PointingRay) -> AnchorScore {
    score_geometry(&anchor.geometry, anchor.uncertainty_deg, ray)
}

/// Scores any anchor geometry, such as an eye-rooted anchor's [`FingerAim`].
#[must_use]
pub fn score_geometry(
    geometry: &AnchorGeometry,
    uncertainty_deg: f32,
    ray: &PointingRay,
) -> AnchorScore {
    let theta = geometry_error_deg(geometry, ray);
    let sigma = uncertainty_deg.max(4.0);
    let score = (-(theta * theta) / (2.0 * sigma * sigma))
        .exp()
        .clamp(0.0, 1.0);
    AnchorScore {
        angular_error_deg: theta,
        score,
        sigma_deg: sigma,
    }
}

fn distinctiveness_warnings(
    anchor: &Anchor,
    existing: &[Anchor],
    observations: &[TeachObservation],
) -> Vec<DistinctivenessWarning> {
    let mut warnings = Vec::new();
    for other in existing {
        if other.id == anchor.id || other.status != AnchorStatus::Ok {
            continue;
        }
        let separation = observations
            .iter()
            .map(|obs| {
                let origin = obs.ray.origin;
                let a = anchor.direction_from(origin);
                let b = other.direction_from(origin);
                vec_angle_deg(v3(a), v3(b))
            })
            .fold(f32::INFINITY, f32::min);
        if separation < DISTINCTIVENESS_WARN_DEG {
            warnings.push(DistinctivenessWarning {
                anchor_id: anchor.id,
                other_anchor_id: other.id,
                separation_deg: separation,
            });
        }
    }
    warnings
}

fn mean_ray_distance(point: Vec3, observations: &[TeachObservation]) -> f32 {
    observations
        .iter()
        .map(|obs| {
            let origin = v3(obs.ray.origin);
            let direction = unit_or_z(v3(obs.ray.direction));
            ((Mat3::identity() - direction * direction.transpose()) * (point - origin)).norm()
        })
        .sum::<f32>()
        / observations.len() as f32
}

fn mean_angular_residual(point: Vec3, observations: &[TeachObservation]) -> f32 {
    observations
        .iter()
        .map(|obs| vec_angle_deg(v3(obs.ray.direction), point - v3(obs.ray.origin)))
        .sum::<f32>()
        / observations.len() as f32
}

fn max_origin_baseline(observations: &[TeachObservation]) -> f32 {
    let mut max_distance = 0.0;
    for i in 0..observations.len() {
        for j in (i + 1)..observations.len() {
            max_distance = f32::max(
                max_distance,
                (v3(observations[i].ray.origin) - v3(observations[j].ray.origin)).norm(),
            );
        }
    }
    max_distance
}

fn max_pairwise_ray_angle(observations: &[TeachObservation]) -> f32 {
    let mut max_angle = 0.0;
    for i in 0..observations.len() {
        for j in (i + 1)..observations.len() {
            max_angle = f32::max(
                max_angle,
                vec_angle_deg(
                    v3(observations[i].ray.direction),
                    v3(observations[j].ray.direction),
                ),
            );
        }
    }
    max_angle
}

fn mean_jitter(observations: &[TeachObservation]) -> f32 {
    observations
        .iter()
        .map(|obs| obs.ray_jitter_deg)
        .sum::<f32>()
        / observations.len().max(1) as f32
}

fn confidence(count: usize, residual_deg: f32, jitter_deg: f32) -> f32 {
    let count_score = (count as f32 / 3.0).min(1.0);
    let residual_score = (1.0 - residual_deg / 20.0).clamp(0.0, 1.0);
    let jitter_score = (1.0 - jitter_deg / 15.0).clamp(0.0, 1.0);
    (0.35 * count_score + 0.45 * residual_score + 0.20 * jitter_score).clamp(0.0, 1.0)
}

fn mat_to_array(matrix: Matrix3<f32>) -> [[f32; 3]; 3] {
    [
        [matrix[(0, 0)], matrix[(0, 1)], matrix[(0, 2)]],
        [matrix[(1, 0)], matrix[(1, 1)], matrix[(1, 2)]],
        [matrix[(2, 0)], matrix[(2, 1)], matrix[(2, 2)]],
    ]
}
