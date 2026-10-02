use std::collections::HashMap;

use flick_core::{AnchorId, AnchorKind, AnchorStatus, CameraId, Handedness, PlaceId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CameraIntrinsics, FingerAim, IntrinsicsSource, RaySource, TeachTarget, VerbParams};

/// Place persistence status, matching `places.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceStatus {
    /// Place matches the current scene signature.
    Ok,
    /// Camera moved and anchors are paused until re-aligned.
    NeedsRealign,
}

/// Camera intrinsics JSON stored in `places.intrinsics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredIntrinsics {
    /// Horizontal field of view in degrees.
    pub hfov_deg: f32,
    /// Principal point x in pixels.
    pub cx: f32,
    /// Principal point y in pixels.
    pub cy: f32,
    /// Intrinsics source.
    pub source: IntrinsicsSource,
    /// Catalog/calibration version.
    pub version: String,
}

impl From<&CameraIntrinsics> for StoredIntrinsics {
    fn from(value: &CameraIntrinsics) -> Self {
        Self {
            hfov_deg: value.hfov_deg,
            cx: value.cx,
            cy: value.cy,
            source: value.source.clone(),
            version: value.version.clone(),
        }
    }
}

/// One row in the `places` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceRecord {
    /// `places.id`.
    pub id: PlaceId,
    /// `places.camera_id`.
    pub camera_id: CameraId,
    /// `places.name`.
    pub name: String,
    /// `places.scene_signature`, a 384-float embedding.
    #[serde(with = "crate::places::signature_serde")]
    pub scene_signature: [f32; 384],
    /// `places.embedder_version`.
    pub embedder_version: String,
    /// `places.intrinsics`.
    pub intrinsics: StoredIntrinsics,
    /// `places.status`.
    pub status: PlaceStatus,
    /// `places.active`.
    pub active: bool,
    /// `places.created_at` in unix epoch milliseconds.
    pub created_at: i64,
    /// `places.updated_at` in unix epoch milliseconds.
    pub updated_at: i64,
}

/// One row in the `anchors` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorRecord {
    /// `anchors.id`.
    pub id: AnchorId,
    /// `anchors.place_id`.
    pub place_id: PlaceId,
    /// `anchors.name`.
    pub name: String,
    /// `anchors.target` JSON with exactly one Home Assistant target key.
    pub target: TeachTarget,
    /// `anchors.domain`.
    pub domain: String,
    /// `anchors.kind`.
    pub kind: AnchorKind,
    /// `anchors.position`, present for `point3d`/`region2d`.
    pub position: Option<[f32; 3]>,
    /// `anchors.direction`, present for `direction`.
    pub direction: Option<[f32; 3]>,
    /// `anchors.teach_origin`, present for `direction`.
    pub teach_origin: Option<[f32; 3]>,
    /// `anchors.covariance`, row-major.
    pub covariance: Option<[[f32; 3]; 3]>,
    /// `anchors.uncertainty_deg`.
    pub uncertainty_deg: f32,
    /// `anchors.verb_params`.
    pub verb_params: VerbParams,
    /// `anchors.sensitive`.
    pub sensitive: bool,
    /// `anchors.sensitive_ack`.
    pub sensitive_ack: bool,
    /// `anchors.estimator_version`.
    pub estimator_version: String,
    /// `anchors.ray_source`.
    pub ray_source: RaySource,
    /// `anchors.finger_aim` JSON, the finger-only fallback of an eye-rooted anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finger_aim: Option<FingerAim>,
    /// `anchors.status`.
    pub status: AnchorStatus,
    /// `anchors.last_used_at` in unix epoch milliseconds.
    pub last_used_at: Option<i64>,
    /// `anchors.created_at` in unix epoch milliseconds.
    pub created_at: i64,
    /// `anchors.updated_at` in unix epoch milliseconds.
    pub updated_at: i64,
}

/// One row in the `anchor_observations` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorObservationRecord {
    /// `anchor_observations.id`.
    pub id: String,
    /// `anchor_observations.anchor_id`.
    pub anchor_id: AnchorId,
    /// `anchor_observations.spot_index`.
    pub spot_index: u32,
    /// `anchor_observations.frames`.
    pub frames: u32,
    /// `anchor_observations.hand`.
    pub hand: Handedness,
    /// `anchor_observations.landmarks_image` as one 21×3 landmark array per frame.
    pub landmarks_image: Vec<[[f32; 3]; 21]>,
    /// `anchor_observations.landmarks_world` as one 21×3 landmark array per frame.
    pub landmarks_world: Vec<[[f32; 3]; 21]>,
    /// `anchor_observations.face_keypoints` as one BlazeFace keypoint array per frame.
    pub face_keypoints: Option<Vec<[[f32; 2]; 6]>>,
    /// `anchor_observations.intrinsics_version`.
    pub intrinsics_version: String,
    /// `anchor_observations.created_at` in unix epoch milliseconds.
    pub created_at: i64,
}

/// Persistence failures surfaced by the spatial repository boundary.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TargetingStoreError {
    /// A requested place id was not present.
    #[error("place not found: {0}")]
    PlaceNotFound(PlaceId),
    /// A requested anchor id was not present.
    #[error("anchor not found: {0}")]
    AnchorNotFound(AnchorId),
}

/// Store boundary used by runtime crates; SQLite is implemented by the store lane.
pub trait TargetingStore {
    /// Inserts or replaces one persisted place row.
    fn upsert_place(&mut self, place: PlaceRecord);

    /// Returns one place by id.
    fn place(&self, id: PlaceId) -> Option<PlaceRecord>;

    /// Returns all places for one camera.
    fn places_for_camera(&self, camera_id: CameraId) -> Vec<PlaceRecord>;

    /// Deletes a place and cascades its anchors and observations.
    fn delete_place(&mut self, id: PlaceId) -> Result<(), TargetingStoreError>;

    /// Inserts or replaces one persisted anchor row.
    fn upsert_anchor(&mut self, anchor: AnchorRecord) -> Result<(), TargetingStoreError>;

    /// Returns one anchor by id.
    fn anchor(&self, id: AnchorId) -> Option<AnchorRecord>;

    /// Returns anchors for one place.
    fn anchors_for_place(&self, place_id: PlaceId) -> Vec<AnchorRecord>;

    /// Deletes an anchor and cascades its observations.
    fn delete_anchor(&mut self, id: AnchorId) -> Result<(), TargetingStoreError>;

    /// Inserts or replaces one persisted teaching observation row.
    fn upsert_observation(
        &mut self,
        observation: AnchorObservationRecord,
    ) -> Result<(), TargetingStoreError>;

    /// Returns observations for one anchor sorted by `spot_index`.
    fn observations_for_anchor(&self, anchor_id: AnchorId) -> Vec<AnchorObservationRecord>;
}

/// In-memory targeting store for unit tests and replay harnesses.
#[derive(Debug, Clone, Default)]
pub struct InMemoryTargetingStore {
    places: HashMap<PlaceId, PlaceRecord>,
    anchors: HashMap<AnchorId, AnchorRecord>,
    observations: HashMap<String, AnchorObservationRecord>,
}

impl TargetingStore for InMemoryTargetingStore {
    fn upsert_place(&mut self, place: PlaceRecord) {
        if place.active {
            for existing in self
                .places
                .values_mut()
                .filter(|p| p.camera_id == place.camera_id)
            {
                existing.active = false;
            }
        }
        self.places.insert(place.id, place);
    }

    fn place(&self, id: PlaceId) -> Option<PlaceRecord> {
        self.places.get(&id).cloned()
    }

    fn places_for_camera(&self, camera_id: CameraId) -> Vec<PlaceRecord> {
        let mut places = self
            .places
            .values()
            .filter(|place| place.camera_id == camera_id)
            .cloned()
            .collect::<Vec<_>>();
        places.sort_by_key(|place| place.created_at);
        places
    }

    fn delete_place(&mut self, id: PlaceId) -> Result<(), TargetingStoreError> {
        if self.places.remove(&id).is_none() {
            return Err(TargetingStoreError::PlaceNotFound(id));
        }
        let anchor_ids = self
            .anchors
            .values()
            .filter(|anchor| anchor.place_id == id)
            .map(|anchor| anchor.id)
            .collect::<Vec<_>>();
        for anchor_id in anchor_ids {
            self.anchors.remove(&anchor_id);
            self.observations
                .retain(|_, obs| obs.anchor_id != anchor_id);
        }
        Ok(())
    }

    fn upsert_anchor(&mut self, anchor: AnchorRecord) -> Result<(), TargetingStoreError> {
        if !self.places.contains_key(&anchor.place_id) {
            return Err(TargetingStoreError::PlaceNotFound(anchor.place_id));
        }
        self.anchors.insert(anchor.id, anchor);
        Ok(())
    }

    fn anchor(&self, id: AnchorId) -> Option<AnchorRecord> {
        self.anchors.get(&id).cloned()
    }

    fn anchors_for_place(&self, place_id: PlaceId) -> Vec<AnchorRecord> {
        let mut anchors = self
            .anchors
            .values()
            .filter(|anchor| anchor.place_id == place_id)
            .cloned()
            .collect::<Vec<_>>();
        anchors.sort_by_key(|anchor| anchor.created_at);
        anchors
    }

    fn delete_anchor(&mut self, id: AnchorId) -> Result<(), TargetingStoreError> {
        if self.anchors.remove(&id).is_none() {
            return Err(TargetingStoreError::AnchorNotFound(id));
        }
        self.observations.retain(|_, obs| obs.anchor_id != id);
        Ok(())
    }

    fn upsert_observation(
        &mut self,
        observation: AnchorObservationRecord,
    ) -> Result<(), TargetingStoreError> {
        if !self.anchors.contains_key(&observation.anchor_id) {
            return Err(TargetingStoreError::AnchorNotFound(observation.anchor_id));
        }
        self.observations
            .insert(observation.id.clone(), observation);
        Ok(())
    }

    fn observations_for_anchor(&self, anchor_id: AnchorId) -> Vec<AnchorObservationRecord> {
        let mut observations = self
            .observations
            .values()
            .filter(|obs| obs.anchor_id == anchor_id)
            .cloned()
            .collect::<Vec<_>>();
        observations.sort_by_key(|obs| obs.spot_index);
        observations
    }
}
