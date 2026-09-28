//! SQLite adapter for `flick-spatial` targeting persistence.

use std::{str::FromStr, sync::Arc};

use flick_core::{AnchorId, AnchorKind, AnchorStatus, CameraId, Handedness, PlaceId, StoreError};
use flick_spatial::{
    Anchor, AnchorGeometry, AnchorObservationRecord, AnchorRecord, PlaceRecord, PlaceStatus,
    TargetingStore, TargetingStoreError, TeachTarget,
};
use flick_store::Store;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};

/// SQLite-backed targeting store implemented in the engine lane over `flick-store`.
#[derive(Clone)]
pub struct SqliteTargetingStore {
    store: Arc<Store>,
}

impl SqliteTargetingStore {
    /// Creates a targeting store adapter over an opened Flick store.
    #[must_use]
    pub const fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Fallible place upsert used by runtime code that wants storage errors.
    pub fn try_upsert_place(&self, place: &PlaceRecord) -> flick_store::Result<()> {
        let scene_signature = to_blob(place.scene_signature.as_slice())?;
        let intrinsics = to_text(&place.intrinsics)?;
        let mut conn = self.store.connection();
        let tx = conn.transaction().map_err(database_error)?;
        tx.execute(
            "INSERT OR IGNORE INTO cameras \
             (id, name, kind, enabled, mirror, rotation, active_fps, idle_fps, max_hands, created_at, updated_at) \
             VALUES (?1, ?2, 'local', 1, 1, 0, 30, 5, 2, ?3, ?4)",
            params![
                place.camera_id.to_string(),
                format!("Camera {}", place.camera_id),
                place.created_at,
                place.updated_at,
            ],
        )
        .map_err(database_error)?;
        if place.active {
            tx.execute(
                "UPDATE places SET active = 0, updated_at = ?2 WHERE camera_id = ?1",
                params![place.camera_id.to_string(), place.updated_at],
            )
            .map_err(database_error)?;
        }
        tx.execute(
            "INSERT INTO places \
             (id, camera_id, name, scene_signature, embedder_version, intrinsics, status, active, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(id) DO UPDATE SET \
             camera_id = excluded.camera_id, name = excluded.name, scene_signature = excluded.scene_signature, \
             embedder_version = excluded.embedder_version, intrinsics = excluded.intrinsics, status = excluded.status, \
             active = excluded.active, updated_at = excluded.updated_at",
            params![
                place.id.to_string(),
                place.camera_id.to_string(),
                place.name,
                scene_signature,
                place.embedder_version,
                intrinsics,
                place_status_str(place.status),
                bool_i64(place.active),
                place.created_at,
                place.updated_at,
            ],
        )
        .map_err(database_error)?;
        tx.commit().map_err(database_error)
    }

    /// Fallible place lookup.
    pub fn try_place(&self, id: PlaceId) -> flick_store::Result<Option<PlaceRecord>> {
        let conn = self.store.connection();
        let raw = conn
            .query_row(
                "SELECT id, camera_id, name, scene_signature, embedder_version, intrinsics, status, active, created_at, updated_at \
                 FROM places WHERE id = ?1",
                params![id.to_string()],
                raw_place,
            )
            .optional()
            .map_err(database_error)?;
        raw.map(PlaceRecord::try_from).transpose()
    }

    /// Fallible places-by-camera lookup.
    pub fn try_places_for_camera(
        &self,
        camera_id: CameraId,
    ) -> flick_store::Result<Vec<PlaceRecord>> {
        let conn = self.store.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, camera_id, name, scene_signature, embedder_version, intrinsics, status, active, created_at, updated_at \
                 FROM places WHERE camera_id = ?1 ORDER BY created_at",
            )
            .map_err(database_error)?;
        let rows = stmt
            .query_map(params![camera_id.to_string()], raw_place)
            .map_err(database_error)?;
        rows.map(|row| row.map_err(database_error).and_then(PlaceRecord::try_from))
            .collect()
    }

    /// Fallible anchor upsert.
    pub fn try_upsert_anchor(&self, anchor: &AnchorRecord) -> flick_store::Result<()> {
        if self.try_place(anchor.place_id)?.is_none() {
            return Err(StoreError::Database(format!(
                "place not found: {}",
                anchor.place_id
            )));
        }
        let position = optional_blob(anchor.position.as_ref())?;
        let direction = optional_blob(anchor.direction.as_ref())?;
        let teach_origin = optional_blob(anchor.teach_origin.as_ref())?;
        let covariance = optional_blob(anchor.covariance.as_ref())?;
        let target = to_text(&anchor.target)?;
        let verb_params = to_text(&anchor.verb_params)?;
        let conn = self.store.connection();
        conn.execute(
            "INSERT INTO anchors \
             (id, place_id, name, target, domain, kind, position, direction, teach_origin, covariance, \
              uncertainty_deg, verb_params, sensitive, sensitive_ack, estimator_version, status, last_used_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19) \
             ON CONFLICT(id) DO UPDATE SET \
             place_id = excluded.place_id, name = excluded.name, target = excluded.target, domain = excluded.domain, \
             kind = excluded.kind, position = excluded.position, direction = excluded.direction, teach_origin = excluded.teach_origin, \
             covariance = excluded.covariance, uncertainty_deg = excluded.uncertainty_deg, verb_params = excluded.verb_params, \
             sensitive = excluded.sensitive, sensitive_ack = excluded.sensitive_ack, estimator_version = excluded.estimator_version, \
             status = excluded.status, last_used_at = excluded.last_used_at, updated_at = excluded.updated_at",
            params![
                anchor.id.to_string(),
                anchor.place_id.to_string(),
                anchor.name,
                target,
                anchor.domain,
                anchor_kind_str(anchor.kind),
                position,
                direction,
                teach_origin,
                covariance,
                anchor.uncertainty_deg,
                verb_params,
                bool_i64(anchor.sensitive),
                bool_i64(anchor.sensitive_ack),
                anchor.estimator_version,
                anchor_status_str(anchor.status),
                anchor.last_used_at,
                anchor.created_at,
                anchor.updated_at,
            ],
        )
        .map_err(database_error)?;
        Ok(())
    }

    /// Fallible anchor lookup.
    pub fn try_anchor(&self, id: AnchorId) -> flick_store::Result<Option<AnchorRecord>> {
        let conn = self.store.connection();
        let raw = conn
            .query_row(
                "SELECT id, place_id, name, target, domain, kind, position, direction, teach_origin, covariance, \
                 uncertainty_deg, verb_params, sensitive, sensitive_ack, estimator_version, status, last_used_at, created_at, updated_at \
                 FROM anchors WHERE id = ?1",
                params![id.to_string()],
                raw_anchor,
            )
            .optional()
            .map_err(database_error)?;
        raw.map(AnchorRecord::try_from).transpose()
    }

    /// Fallible anchors-by-place lookup.
    pub fn try_anchors_for_place(
        &self,
        place_id: PlaceId,
    ) -> flick_store::Result<Vec<AnchorRecord>> {
        let conn = self.store.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, place_id, name, target, domain, kind, position, direction, teach_origin, covariance, \
                 uncertainty_deg, verb_params, sensitive, sensitive_ack, estimator_version, status, last_used_at, created_at, updated_at \
                 FROM anchors WHERE place_id = ?1 ORDER BY created_at",
            )
            .map_err(database_error)?;
        let rows = stmt
            .query_map(params![place_id.to_string()], raw_anchor)
            .map_err(database_error)?;
        rows.map(|row| row.map_err(database_error).and_then(AnchorRecord::try_from))
            .collect()
    }

    /// Fallible observation upsert.
    pub fn try_upsert_observation(
        &self,
        observation: &AnchorObservationRecord,
    ) -> flick_store::Result<()> {
        if self.try_anchor(observation.anchor_id)?.is_none() {
            return Err(StoreError::Database(format!(
                "anchor not found: {}",
                observation.anchor_id
            )));
        }
        let landmarks_image = to_blob(&observation.landmarks_image)?;
        let landmarks_world = to_blob(&observation.landmarks_world)?;
        let face_keypoints = optional_blob(observation.face_keypoints.as_ref())?;
        let conn = self.store.connection();
        conn.execute(
            "INSERT INTO anchor_observations \
             (id, anchor_id, spot_index, frames, hand, landmarks_image, landmarks_world, face_keypoints, intrinsics_version, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(id) DO UPDATE SET \
             anchor_id = excluded.anchor_id, spot_index = excluded.spot_index, frames = excluded.frames, hand = excluded.hand, \
             landmarks_image = excluded.landmarks_image, landmarks_world = excluded.landmarks_world, face_keypoints = excluded.face_keypoints, \
             intrinsics_version = excluded.intrinsics_version, created_at = excluded.created_at",
            params![
                observation.id,
                observation.anchor_id.to_string(),
                observation.spot_index,
                observation.frames,
                hand_str(observation.hand),
                landmarks_image,
                landmarks_world,
                face_keypoints,
                observation.intrinsics_version,
                observation.created_at,
            ],
        )
        .map_err(database_error)?;
        Ok(())
    }

    /// Fallible observations-by-anchor lookup.
    pub fn try_observations_for_anchor(
        &self,
        anchor_id: AnchorId,
    ) -> flick_store::Result<Vec<AnchorObservationRecord>> {
        let conn = self.store.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, anchor_id, spot_index, frames, hand, landmarks_image, landmarks_world, face_keypoints, intrinsics_version, created_at \
                 FROM anchor_observations WHERE anchor_id = ?1 ORDER BY spot_index",
            )
            .map_err(database_error)?;
        let rows = stmt
            .query_map(params![anchor_id.to_string()], raw_observation)
            .map_err(database_error)?;
        rows.map(|row| {
            row.map_err(database_error)
                .and_then(AnchorObservationRecord::try_from)
        })
        .collect()
    }
}

impl TargetingStore for SqliteTargetingStore {
    fn upsert_place(&mut self, place: PlaceRecord) {
        if let Err(err) = self.try_upsert_place(&place) {
            tracing::warn!(error = %err, place_id = %place.id, "failed to upsert targeting place");
        }
    }

    fn place(&self, id: PlaceId) -> Option<PlaceRecord> {
        self.try_place(id)
            .inspect_err(|err| tracing::warn!(error = %err, place_id = %id, "failed to load targeting place"))
            .ok()
            .flatten()
    }

    fn places_for_camera(&self, camera_id: CameraId) -> Vec<PlaceRecord> {
        self.try_places_for_camera(camera_id)
            .inspect_err(|err| tracing::warn!(error = %err, camera_id = %camera_id, "failed to load targeting places"))
            .unwrap_or_default()
    }

    fn delete_place(&mut self, id: PlaceId) -> Result<(), TargetingStoreError> {
        let conn = self.store.connection();
        let deleted = conn
            .execute("DELETE FROM places WHERE id = ?1", params![id.to_string()])
            .map_err(|err| {
                tracing::warn!(error = %err, place_id = %id, "failed to delete targeting place");
                TargetingStoreError::PlaceNotFound(id)
            })?;
        if deleted == 0 {
            Err(TargetingStoreError::PlaceNotFound(id))
        } else {
            Ok(())
        }
    }

    fn upsert_anchor(&mut self, anchor: AnchorRecord) -> Result<(), TargetingStoreError> {
        if self.try_place(anchor.place_id).ok().flatten().is_none() {
            return Err(TargetingStoreError::PlaceNotFound(anchor.place_id));
        }
        self.try_upsert_anchor(&anchor).map_err(|err| {
            tracing::warn!(error = %err, anchor_id = %anchor.id, "failed to upsert targeting anchor");
            TargetingStoreError::AnchorNotFound(anchor.id)
        })
    }

    fn anchor(&self, id: AnchorId) -> Option<AnchorRecord> {
        self.try_anchor(id)
            .inspect_err(|err| tracing::warn!(error = %err, anchor_id = %id, "failed to load targeting anchor"))
            .ok()
            .flatten()
    }

    fn anchors_for_place(&self, place_id: PlaceId) -> Vec<AnchorRecord> {
        self.try_anchors_for_place(place_id)
            .inspect_err(|err| tracing::warn!(error = %err, place_id = %place_id, "failed to load targeting anchors"))
            .unwrap_or_default()
    }

    fn delete_anchor(&mut self, id: AnchorId) -> Result<(), TargetingStoreError> {
        let conn = self.store.connection();
        let deleted = conn
            .execute("DELETE FROM anchors WHERE id = ?1", params![id.to_string()])
            .map_err(|err| {
                tracing::warn!(error = %err, anchor_id = %id, "failed to delete targeting anchor");
                TargetingStoreError::AnchorNotFound(id)
            })?;
        if deleted == 0 {
            Err(TargetingStoreError::AnchorNotFound(id))
        } else {
            Ok(())
        }
    }

    fn upsert_observation(
        &mut self,
        observation: AnchorObservationRecord,
    ) -> Result<(), TargetingStoreError> {
        if self
            .try_anchor(observation.anchor_id)
            .ok()
            .flatten()
            .is_none()
        {
            return Err(TargetingStoreError::AnchorNotFound(observation.anchor_id));
        }
        self.try_upsert_observation(&observation).map_err(|err| {
            tracing::warn!(error = %err, observation_id = observation.id, "failed to upsert targeting observation");
            TargetingStoreError::AnchorNotFound(observation.anchor_id)
        })
    }

    fn observations_for_anchor(&self, anchor_id: AnchorId) -> Vec<AnchorObservationRecord> {
        self.try_observations_for_anchor(anchor_id)
            .inspect_err(|err| tracing::warn!(error = %err, anchor_id = %anchor_id, "failed to load targeting observations"))
            .unwrap_or_default()
    }
}

/// Converts a persistence row into a spatial selector anchor.
pub fn anchor_record_to_anchor(record: &AnchorRecord) -> Option<Anchor> {
    let geometry = match record.kind {
        AnchorKind::Point3d => AnchorGeometry::Point3d {
            position: record.position?,
            covariance: record.covariance.unwrap_or([[0.0; 3]; 3]),
        },
        AnchorKind::Direction => AnchorGeometry::Direction {
            direction: record.direction?,
            teach_origin: record.teach_origin.unwrap_or([0.0, 0.0, 0.0]),
            covariance: record.covariance.unwrap_or([[0.0; 3]; 3]),
        },
        AnchorKind::Region2d => return None,
    };
    Some(Anchor {
        id: record.id,
        name: record.name.clone(),
        target: record.target.clone(),
        domain: record.domain.clone(),
        geometry,
        uncertainty_deg: record.uncertainty_deg,
        verb_params: record.verb_params.clone(),
        status: record.status,
        estimator_version: record.estimator_version.clone(),
    })
}

/// Converts a spatial anchor into a persistence row.
#[must_use]
pub fn anchor_to_record(
    place_id: PlaceId,
    anchor: &Anchor,
    sensitive: bool,
    sensitive_ack: bool,
    now_ms: i64,
) -> AnchorRecord {
    let (position, direction, teach_origin, covariance) = match &anchor.geometry {
        AnchorGeometry::Point3d {
            position,
            covariance,
        } => (Some(*position), None, None, Some(*covariance)),
        AnchorGeometry::Direction {
            direction,
            teach_origin,
            covariance,
        } => (
            None,
            Some(*direction),
            Some(*teach_origin),
            Some(*covariance),
        ),
    };
    AnchorRecord {
        id: anchor.id,
        place_id,
        name: anchor.name.clone(),
        target: anchor.target.clone(),
        domain: anchor.domain.clone(),
        kind: anchor.kind(),
        position,
        direction,
        teach_origin,
        covariance,
        uncertainty_deg: anchor.uncertainty_deg,
        verb_params: anchor.verb_params.clone(),
        sensitive,
        sensitive_ack,
        estimator_version: anchor.estimator_version.clone(),
        status: anchor.status,
        last_used_at: None,
        created_at: now_ms,
        updated_at: now_ms,
    }
}

#[derive(Debug)]
struct RawPlace {
    id: String,
    camera_id: String,
    name: String,
    scene_signature: Vec<u8>,
    embedder_version: String,
    intrinsics: String,
    status: String,
    active: i64,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<RawPlace> for PlaceRecord {
    type Error = StoreError;

    fn try_from(value: RawPlace) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_place_id(&value.id)?,
            camera_id: parse_camera_id(&value.camera_id)?,
            name: value.name,
            scene_signature: signature_from_blob(&value.scene_signature)?,
            embedder_version: value.embedder_version,
            intrinsics: from_text(&value.intrinsics, "places.intrinsics")?,
            status: parse_place_status(&value.status)?,
            active: value.active != 0,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

#[derive(Debug)]
struct RawAnchor {
    id: String,
    place_id: String,
    name: String,
    target: String,
    domain: String,
    kind: String,
    position: Option<Vec<u8>>,
    direction: Option<Vec<u8>>,
    teach_origin: Option<Vec<u8>>,
    covariance: Option<Vec<u8>>,
    uncertainty_deg: f32,
    verb_params: String,
    sensitive: i64,
    sensitive_ack: i64,
    estimator_version: String,
    status: String,
    last_used_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<RawAnchor> for AnchorRecord {
    type Error = StoreError;

    fn try_from(value: RawAnchor) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_anchor_id(&value.id)?,
            place_id: parse_place_id(&value.place_id)?,
            name: value.name,
            target: from_text::<TeachTarget>(&value.target, "anchors.target")?,
            domain: value.domain,
            kind: parse_anchor_kind(&value.kind)?,
            position: from_optional_blob(value.position, "anchors.position")?,
            direction: from_optional_blob(value.direction, "anchors.direction")?,
            teach_origin: from_optional_blob(value.teach_origin, "anchors.teach_origin")?,
            covariance: from_optional_blob(value.covariance, "anchors.covariance")?,
            uncertainty_deg: value.uncertainty_deg,
            verb_params: from_text(&value.verb_params, "anchors.verb_params")?,
            sensitive: value.sensitive != 0,
            sensitive_ack: value.sensitive_ack != 0,
            estimator_version: value.estimator_version,
            status: parse_anchor_status(&value.status)?,
            last_used_at: value.last_used_at,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

#[derive(Debug)]
struct RawObservation {
    id: String,
    anchor_id: String,
    spot_index: u32,
    frames: u32,
    hand: String,
    landmarks_image: Vec<u8>,
    landmarks_world: Vec<u8>,
    face_keypoints: Option<Vec<u8>>,
    intrinsics_version: String,
    created_at: i64,
}

impl TryFrom<RawObservation> for AnchorObservationRecord {
    type Error = StoreError;

    fn try_from(value: RawObservation) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            anchor_id: parse_anchor_id(&value.anchor_id)?,
            spot_index: value.spot_index,
            frames: value.frames,
            hand: parse_hand(&value.hand)?,
            landmarks_image: from_blob(
                &value.landmarks_image,
                "anchor_observations.landmarks_image",
            )?,
            landmarks_world: from_blob(
                &value.landmarks_world,
                "anchor_observations.landmarks_world",
            )?,
            face_keypoints: from_optional_blob(
                value.face_keypoints,
                "anchor_observations.face_keypoints",
            )?,
            intrinsics_version: value.intrinsics_version,
            created_at: value.created_at,
        })
    }
}

fn raw_place(row: &Row<'_>) -> rusqlite::Result<RawPlace> {
    Ok(RawPlace {
        id: row.get(0)?,
        camera_id: row.get(1)?,
        name: row.get(2)?,
        scene_signature: row.get(3)?,
        embedder_version: row.get(4)?,
        intrinsics: row.get(5)?,
        status: row.get(6)?,
        active: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn raw_anchor(row: &Row<'_>) -> rusqlite::Result<RawAnchor> {
    Ok(RawAnchor {
        id: row.get(0)?,
        place_id: row.get(1)?,
        name: row.get(2)?,
        target: row.get(3)?,
        domain: row.get(4)?,
        kind: row.get(5)?,
        position: row.get(6)?,
        direction: row.get(7)?,
        teach_origin: row.get(8)?,
        covariance: row.get(9)?,
        uncertainty_deg: row.get(10)?,
        verb_params: row.get(11)?,
        sensitive: row.get(12)?,
        sensitive_ack: row.get(13)?,
        estimator_version: row.get(14)?,
        status: row.get(15)?,
        last_used_at: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

fn raw_observation(row: &Row<'_>) -> rusqlite::Result<RawObservation> {
    Ok(RawObservation {
        id: row.get(0)?,
        anchor_id: row.get(1)?,
        spot_index: row.get(2)?,
        frames: row.get(3)?,
        hand: row.get(4)?,
        landmarks_image: row.get(5)?,
        landmarks_world: row.get(6)?,
        face_keypoints: row.get(7)?,
        intrinsics_version: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn to_text<T: Serialize>(value: &T) -> flick_store::Result<String> {
    serde_json::to_string(value).map_err(json_error)
}

fn to_blob<T: Serialize + ?Sized>(value: &T) -> flick_store::Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(json_error)
}

fn optional_blob<T: Serialize>(value: Option<&T>) -> flick_store::Result<Option<Vec<u8>>> {
    value.map(to_blob).transpose()
}

fn from_text<T: DeserializeOwned>(value: &str, column: &str) -> flick_store::Result<T> {
    serde_json::from_str(value).map_err(|err| StoreError::InvalidJson(format!("{column}: {err}")))
}

fn from_blob<T: DeserializeOwned>(value: &[u8], column: &str) -> flick_store::Result<T> {
    serde_json::from_slice(value).map_err(|err| StoreError::InvalidJson(format!("{column}: {err}")))
}

fn from_optional_blob<T: DeserializeOwned>(
    value: Option<Vec<u8>>,
    column: &str,
) -> flick_store::Result<Option<T>> {
    value
        .as_deref()
        .map(|raw| from_blob(raw, column))
        .transpose()
}

fn signature_from_blob(value: &[u8]) -> flick_store::Result<[f32; 384]> {
    let values = from_blob::<Vec<f32>>(value, "places.scene_signature")?;
    if values.len() != 384 {
        return Err(StoreError::InvalidJson(format!(
            "places.scene_signature length {} != 384",
            values.len()
        )));
    }
    let mut signature = [0.0_f32; 384];
    signature.copy_from_slice(&values);
    Ok(signature)
}

fn parse_camera_id(value: &str) -> flick_store::Result<CameraId> {
    CameraId::from_str(value).map_err(|err| StoreError::InvalidJson(err.to_string()))
}

fn parse_place_id(value: &str) -> flick_store::Result<PlaceId> {
    PlaceId::from_str(value).map_err(|err| StoreError::InvalidJson(err.to_string()))
}

fn parse_anchor_id(value: &str) -> flick_store::Result<AnchorId> {
    AnchorId::from_str(value).map_err(|err| StoreError::InvalidJson(err.to_string()))
}

const fn place_status_str(value: PlaceStatus) -> &'static str {
    match value {
        PlaceStatus::Ok => "ok",
        PlaceStatus::NeedsRealign => "needs_realign",
    }
}

fn parse_place_status(value: &str) -> flick_store::Result<PlaceStatus> {
    match value {
        "ok" => Ok(PlaceStatus::Ok),
        "needs_realign" => Ok(PlaceStatus::NeedsRealign),
        other => Err(StoreError::InvalidJson(format!(
            "invalid place status: {other}"
        ))),
    }
}

const fn anchor_kind_str(value: AnchorKind) -> &'static str {
    match value {
        AnchorKind::Point3d => "point3d",
        AnchorKind::Direction => "direction",
        AnchorKind::Region2d => "region2d",
    }
}

fn parse_anchor_kind(value: &str) -> flick_store::Result<AnchorKind> {
    match value {
        "point3d" => Ok(AnchorKind::Point3d),
        "direction" => Ok(AnchorKind::Direction),
        "region2d" => Ok(AnchorKind::Region2d),
        other => Err(StoreError::InvalidJson(format!(
            "invalid anchor kind: {other}"
        ))),
    }
}

const fn anchor_status_str(value: AnchorStatus) -> &'static str {
    match value {
        AnchorStatus::Ok => "ok",
        AnchorStatus::NeedsRealign => "needs_realign",
        AnchorStatus::NeedsReteach => "needs_reteach",
    }
}

fn parse_anchor_status(value: &str) -> flick_store::Result<AnchorStatus> {
    match value {
        "ok" => Ok(AnchorStatus::Ok),
        "needs_realign" => Ok(AnchorStatus::NeedsRealign),
        "needs_reteach" => Ok(AnchorStatus::NeedsReteach),
        other => Err(StoreError::InvalidJson(format!(
            "invalid anchor status: {other}"
        ))),
    }
}

const fn hand_str(value: Handedness) -> &'static str {
    match value {
        Handedness::Left => "left",
        Handedness::Right => "right",
    }
}

fn parse_hand(value: &str) -> flick_store::Result<Handedness> {
    match value {
        "left" => Ok(Handedness::Left),
        "right" => Ok(Handedness::Right),
        other => Err(StoreError::InvalidJson(format!("invalid hand: {other}"))),
    }
}

const fn bool_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn database_error(err: rusqlite::Error) -> StoreError {
    StoreError::Database(err.to_string())
}

fn json_error(err: serde_json::Error) -> StoreError {
    StoreError::InvalidJson(err.to_string())
}
