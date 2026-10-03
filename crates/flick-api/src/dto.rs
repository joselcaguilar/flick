//! DTOs for Flick's local HTTP and WebSocket API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// RFC 9457 problem response with a stable Flick error code.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProblemJson {
    /// Problem type URI.
    #[schema(example = "about:blank")]
    pub r#type: String,
    /// Short title.
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Human-readable detail.
    pub detail: String,
    /// Stable snake_case code.
    pub code: String,
}

/// Health check response used by the desktop supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Health status.
    pub status: String,
    /// Engine/API version.
    pub version: String,
    /// Uptime in whole seconds.
    pub uptime_s: u64,
}

/// Engine status shown on the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EngineStatus {
    /// Whether recognition is paused.
    pub paused: bool,
    /// Optional pause expiry as RFC 3339.
    pub paused_until: Option<String>,
    /// Camera statuses.
    pub cameras: Vec<CameraStatus>,
    /// Home Assistant connection state.
    pub ha: HaStatus,
    /// Per-stage latency summaries.
    pub stages: Vec<StageLatency>,
    /// macOS camera permission, when known.
    pub camera_permission: Option<String>,
}

/// Pause request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PauseRequest {
    /// Duration in seconds. Null/omitted pauses until resumed.
    pub duration_s: Option<u64>,
}

/// Per-stage latency.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StageLatency {
    /// Stage name.
    pub stage: String,
    /// p50 latency in milliseconds.
    pub p50_ms: f64,
    /// p95 latency in milliseconds.
    pub p95_ms: f64,
}

/// Generic key/value settings map.
pub type SettingsMap = BTreeMap<String, Value>;

/// OpenAPI schema wrapper for settings maps.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SettingsDto {
    /// Arbitrary JSON setting values keyed by stable setting name.
    #[serde(flatten)]
    #[schema(value_type = Object)]
    pub values: SettingsMap,
}

/// Discovered Home Assistant instance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaDiscovery {
    /// Display name.
    pub name: String,
    /// Base URL.
    pub base_url: String,
    /// HA UUID when known.
    pub uuid: Option<String>,
    /// HA version when known.
    pub version: Option<String>,
}

/// Request to connect Home Assistant.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaConnectRequest {
    /// Base URL.
    pub base_url: String,
    /// Long-lived access token.
    pub token: String,
    /// Optional pinned certificate SHA-256.
    pub trust_cert_sha256: Option<String>,
}

/// Persisted Home Assistant instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HaInstance {
    /// ULID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Base URL.
    pub base_url: String,
    /// HA UUID.
    pub ha_uuid: Option<String>,
    /// Authentication kind.
    pub auth_kind: String,
    /// HA version.
    pub ha_version: Option<String>,
    /// Default instance flag.
    pub is_default: bool,
    /// Creation time.
    pub created_at: String,
    /// Update time.
    pub updated_at: String,
    /// Home URL, preferred on trusted Wi-Fi networks or when found on the LAN.
    #[serde(default)]
    pub internal_url: Option<String>,
    /// Wi-Fi networks on which the Home URL is used.
    #[serde(default)]
    pub trusted_ssids: Vec<String>,
}

/// Home Assistant status response.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HaStatus {
    /// disconnected, connecting, ready or auth_failed.
    pub state: String,
    /// HA version.
    pub ha_version: Option<String>,
    /// Connected instance.
    pub instance: Option<HaInstance>,
    /// Why the last connection attempt failed, while reconnecting.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Active route: home or remote.
    #[serde(default)]
    pub connection: Option<String>,
    /// HTTP URL currently in use.
    #[serde(default)]
    pub active_url: Option<String>,
    /// Wi-Fi network reported by the desktop shell, when known.
    #[serde(default)]
    pub network_ssid: Option<String>,
}

/// Change to how Flick reaches Home Assistant. Omitted fields are kept.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HaConnectionUpdate {
    /// Remote URL, used anywhere.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Home URL. An empty string clears it.
    #[serde(default)]
    pub internal_url: Option<String>,
    /// Wi-Fi networks on which the Home URL is used.
    #[serde(default)]
    pub trusted_ssids: Option<Vec<String>>,
}

/// Client certificate presented when a server requires mutual TLS (mTLS).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HaClientCertificate {
    /// Whether a certificate is installed.
    pub installed: bool,
    /// Subject common name.
    #[serde(default)]
    pub subject: Option<String>,
    /// Issuer common name.
    #[serde(default)]
    pub issuer: Option<String>,
    /// Expiry time (RFC 3339).
    #[serde(default)]
    pub not_after: Option<String>,
    /// SHA-256 fingerprint, colon-separated hex.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Whether the certificate has expired.
    #[serde(default)]
    pub expired: bool,
}

/// Client certificate import.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HaClientCertificateUpload {
    /// Base64 file contents: a `.p12`/`.pfx` bundle, or PEM certificate(s) plus private key.
    pub data: String,
    /// Password for a `.p12`/`.pfx` bundle.
    #[serde(default)]
    pub password: Option<String>,
}

/// Current network as seen by the desktop shell.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct NetworkReport {
    /// Wi-Fi SSID, or null when unknown or not on Wi-Fi.
    #[serde(default)]
    pub ssid: Option<String>,
}

/// Home Assistant area picker item.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaArea {
    /// Area id.
    pub area_id: String,
    /// Name.
    pub name: String,
    /// Floor id.
    pub floor_id: Option<String>,
}

/// Home Assistant entity picker item.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaEntity {
    /// Entity id.
    pub entity_id: String,
    /// Friendly name.
    pub name: String,
    /// Domain.
    pub domain: String,
    /// Area id.
    pub area_id: Option<String>,
    /// Device id.
    pub device_id: Option<String>,
    /// Current state.
    pub state: Option<String>,
    /// Device class.
    pub device_class: Option<String>,
    /// Supported features bitmask.
    pub supported_features: Option<u64>,
    /// Raw attributes.
    #[schema(value_type = Object)]
    pub attributes: Value,
}

/// Home Assistant service schema subset.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaServiceSchema {
    /// Domain.
    pub domain: String,
    /// Services keyed by service name.
    #[schema(value_type = Object)]
    pub services: Value,
}

/// Action JSON accepted by API routes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionDto {
    /// Direct Home Assistant service call.
    CallService {
        /// Domain.
        domain: String,
        /// Service.
        service: String,
        /// Target object.
        target: ActionTargetDto,
        /// Service data.
        #[serde(default)]
        #[schema(value_type = Object)]
        data: Value,
        /// Optional preset metadata.
        #[serde(skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
    },
    /// Dial action.
    Dial {
        /// Entity id or `$selected`.
        entity_id: String,
        /// brightness_pct, volume_level, position, percentage or temperature.
        property: String,
        /// Gain.
        gain: f64,
        /// Optional min.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schema(value_type = Object)]
        min: Option<Value>,
        /// Optional max.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schema(value_type = Object)]
        max: Option<Value>,
    },
    /// Targeted verb action.
    Verb {
        /// up, down, on, off, stop, toggle or level_set.
        verb: String,
        /// One-based level for level_set.
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<u32>,
    },
}

/// Home Assistant target.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ActionTargetDto {
    /// Entity ids.
    pub entity_id: Option<Vec<String>>,
    /// Device ids.
    pub device_id: Option<Vec<String>>,
    /// Area ids.
    pub area_id: Option<Vec<String>>,
}

/// Result of testing or dispatching an action.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActionOutcomeDto {
    /// sent, ok, error, timeout, stale or suppressed.
    pub status: String,
    /// Stable error code.
    pub error_code: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// Home Assistant context id.
    pub ha_context_id: Option<String>,
    /// Latency object.
    pub latency: Option<LatencyBreakdown>,
}

/// Latency breakdown stored in the activity log.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LatencyBreakdown {
    /// Detection latency.
    pub detect_ms: Option<f64>,
    /// Dispatch latency.
    pub dispatch_ms: Option<f64>,
    /// Home Assistant latency.
    pub ha_ms: Option<f64>,
}

/// Available camera format.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CameraFormat {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Frames per second.
    pub fps: u32,
    /// Pixel format.
    pub format: String,
}

/// Available camera.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AvailableCamera {
    /// Stable device reference.
    pub device_ref: String,
    /// Name.
    pub name: String,
    /// Camera kind.
    pub kind: String,
    /// Supported formats.
    pub formats: Vec<CameraFormat>,
}

/// Camera create request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CameraCreate {
    /// Name.
    pub name: String,
    /// local, rtsp or file.
    pub kind: String,
    /// OS device id or file path.
    pub device_ref: Option<String>,
    /// Redacted URL for RTSP.
    pub url_redacted: Option<String>,
}

/// Camera patch request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct CameraPatch {
    /// Name.
    pub name: Option<String>,
    /// Enabled flag.
    pub enabled: Option<bool>,
    /// Mirror flag.
    pub mirror: Option<bool>,
    /// Rotation.
    pub rotation: Option<i32>,
    /// Active fps.
    pub active_fps: Option<u32>,
    /// Idle fps.
    pub idle_fps: Option<u32>,
    /// Max hands.
    pub max_hands: Option<u32>,
    /// Normalized ROI.
    #[schema(value_type = Object)]
    pub roi: Option<Value>,
}

/// Camera record.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Camera {
    /// ULID.
    pub id: String,
    /// Name.
    pub name: String,
    /// Camera kind.
    pub kind: String,
    /// Device reference.
    pub device_ref: Option<String>,
    /// Redacted URL.
    pub url_redacted: Option<String>,
    /// Enabled.
    pub enabled: bool,
    /// Mirror.
    pub mirror: bool,
    /// Rotation.
    pub rotation: i32,
    /// Active fps.
    pub active_fps: u32,
    /// Idle fps.
    pub idle_fps: u32,
    /// Max hands.
    pub max_hands: u32,
    /// ROI.
    #[schema(value_type = Object)]
    pub roi: Option<Value>,
    /// Creation time.
    pub created_at: String,
    /// Update time.
    pub updated_at: String,
}

/// Camera runtime status.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CameraStatus {
    /// Camera id.
    pub camera_id: String,
    /// starting, running, idle, reconnecting, permission_denied, error, disabled or stopped.
    pub state: String,
    /// Current FPS.
    pub fps: Option<f64>,
    /// Error string.
    pub error: Option<String>,
}

/// MJPEG ticket response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PreviewTicket {
    /// Relative stream URL.
    pub url: String,
    /// Expiry as RFC 3339.
    pub expires_at: String,
}

/// Gesture create request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureCreate {
    /// Name.
    pub name: String,
    /// static or motion, optional for auto-detect.
    pub kind: Option<String>,
    /// Required hands.
    pub hands_required: Option<u32>,
    /// any, left or right.
    pub hand_constraint: String,
    /// Icon.
    pub icon: Option<String>,
}

/// Gesture patch request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct GesturePatch {
    /// Name.
    pub name: Option<String>,
    /// Icon.
    pub icon: Option<String>,
    /// Threshold.
    pub threshold: Option<f64>,
    /// Enabled.
    pub enabled: Option<bool>,
    /// Hand constraint.
    pub hand_constraint: Option<String>,
}

/// Gesture record.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Gesture {
    /// Gesture id.
    pub id: String,
    /// Source.
    pub source: String,
    /// static, motion, dial or negative.
    pub kind: String,
    /// Required hands.
    pub hands_required: u32,
    /// Name.
    pub name: String,
    /// Icon.
    pub icon: Option<String>,
    /// Hand constraint.
    pub hand_constraint: String,
    /// Threshold.
    pub threshold: Option<f64>,
    /// Enabled.
    pub enabled: bool,
    /// Sample count.
    pub sample_count: u32,
    /// Accuracy estimate.
    pub accuracy: Option<f64>,
    /// Distinctiveness estimate.
    pub distinctiveness: Option<f64>,
}

/// Capture request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CaptureRequest {
    /// Camera id.
    pub camera_id: Option<String>,
    /// positive or negative.
    pub kind: String,
    /// Number of takes.
    pub takes: u32,
    /// Take duration in milliseconds.
    pub take_ms: u32,
}

/// Capture session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CaptureSession {
    /// Session id.
    pub id: String,
    /// Gesture id.
    pub gesture_id: String,
    /// Camera id.
    pub camera_id: Option<String>,
    /// positive or negative.
    pub kind: String,
    /// Target takes.
    pub target_takes: u32,
    /// Status.
    pub status: String,
    /// Creation time.
    pub created_at: String,
}

/// Motion take summary.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MotionTake {
    /// Take id.
    pub id: String,
    /// Take index.
    pub take_index: u32,
    /// Hands.
    pub hands: u32,
    /// Normalized trajectory for glyphs.
    pub trajectory: Vec<Vec<f32>>,
    /// Quality.
    pub quality: f64,
    /// Created time.
    pub created_at: String,
}

/// Type override.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureTypePatch {
    /// Gesture kind.
    pub kind: String,
    /// Required hands.
    pub hands_required: u32,
}

/// Landmark sample summary.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Sample {
    /// Sample id.
    pub id: String,
    /// Take index.
    pub take_index: u32,
    /// Hand.
    pub hand: String,
    /// Landmarks image, 21 triples.
    pub landmarks_image: Vec<[f32; 3]>,
    /// Quality.
    pub quality: f64,
    /// Created time.
    pub created_at: String,
}

/// Classifier training request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct TrainRequest {
    /// proto_knn or logreg.
    pub algorithm: Option<String>,
}

/// Classifier report.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClassifierReport {
    /// Model id.
    pub id: Option<String>,
    /// Algorithm.
    pub algorithm: String,
    /// Embedder version.
    pub embedder_version: String,
    /// Active flag.
    pub active: bool,
    /// Metrics JSON.
    #[schema(value_type = Object)]
    pub metrics: Value,
    /// Trained time.
    pub trained_at: Option<String>,
}

/// Mapping target mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetModeDto {
    /// Global mapping.
    Global,
    /// Mapping for a specific anchor.
    Anchor,
    /// Mapping for any selected device in a domain.
    Domain,
}

/// Mapping create request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MappingCreate {
    /// Name.
    pub name: String,
    /// Gesture id.
    pub gesture_id: String,
    /// Hand.
    #[serde(default = "default_any")]
    pub hand: String,
    /// Allow two hands.
    #[serde(default)]
    pub allow_two_hands: bool,
    /// Camera ids.
    #[serde(default)]
    pub camera_ids: Vec<String>,
    /// Target mode.
    #[serde(default = "default_target_mode")]
    pub target_mode: TargetModeDto,
    /// Anchor id for anchor mappings.
    pub anchor_id: Option<String>,
    /// Domain for domain mappings.
    pub target_domain: Option<String>,
    /// tap, hold, repeat or dial.
    #[serde(default = "default_tap")]
    pub mode: String,
    /// Hold duration.
    pub hold_ms: Option<u64>,
    /// Repeat interval.
    pub repeat_ms: Option<u64>,
    /// Cooldown.
    pub cooldown_ms: Option<u64>,
    /// Require armed.
    #[serde(default)]
    pub require_armed: bool,
    /// Active hours JSON.
    #[schema(value_type = Object)]
    pub active_hours: Option<Value>,
    /// Action.
    pub action: ActionDto,
    /// Sensitive ack.
    #[serde(default)]
    pub sensitive_ack: bool,
    /// Confirm gesture id.
    pub confirm_gesture_id: Option<String>,
    /// Feedback JSON.
    #[schema(value_type = Object)]
    pub feedback: Option<Value>,
    /// Sort order.
    pub sort_order: Option<i64>,
}

fn default_any() -> String {
    "any".to_owned()
}

fn default_tap() -> String {
    "tap".to_owned()
}

const fn default_target_mode() -> TargetModeDto {
    TargetModeDto::Global
}

/// Mapping patch request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct MappingPatch {
    /// Name.
    pub name: Option<String>,
    /// Enabled.
    pub enabled: Option<bool>,
    /// Action.
    pub action: Option<ActionDto>,
    /// Target mode.
    pub target_mode: Option<TargetModeDto>,
    /// Anchor id.
    pub anchor_id: Option<String>,
    /// Domain.
    pub target_domain: Option<String>,
    /// Sensitive ack.
    pub sensitive_ack: Option<bool>,
    /// Gesture id that fires the mapping.
    pub gesture_id: Option<String>,
}

/// Mapping record.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Mapping {
    /// Mapping id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Enabled.
    pub enabled: bool,
    /// Gesture id.
    pub gesture_id: String,
    /// Hand.
    pub hand: String,
    /// Camera ids.
    pub camera_ids: Vec<String>,
    /// Target mode.
    pub target_mode: TargetModeDto,
    /// Anchor id.
    pub anchor_id: Option<String>,
    /// Target domain.
    pub target_domain: Option<String>,
    /// Mode.
    pub mode: String,
    /// Action.
    pub action: ActionDto,
    /// Sensitive.
    pub sensitive: bool,
    /// Sensitive ack.
    pub sensitive_ack: bool,
    /// Confirm gesture id.
    pub confirm_gesture_id: Option<String>,
    /// Feedback JSON.
    #[schema(value_type = Object)]
    pub feedback: Value,
    /// Sort order.
    pub sort_order: i64,
    /// Creation time.
    pub created_at: String,
    /// Update time.
    pub updated_at: String,
}

/// Mapping order request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MappingOrder(pub Vec<String>);

/// Activity query response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityPage {
    /// Items.
    pub items: Vec<ActivityItem>,
    /// Next cursor.
    pub next_before: Option<String>,
}

/// Activity item.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityItem {
    /// Activity id.
    pub id: String,
    /// Timestamp.
    pub ts: String,
    /// Camera id.
    pub camera_id: Option<String>,
    /// Gesture id.
    pub gesture_id: Option<String>,
    /// Recognizer confidence in `0..=1`.
    pub confidence: Option<f64>,
    /// Mapping id.
    pub mapping_id: Option<String>,
    /// Anchor id.
    pub anchor_id: Option<String>,
    /// Action summary.
    pub action_summary: Option<String>,
    /// Status.
    pub status: String,
    /// Reason.
    pub reason: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// Latency.
    pub latency: Option<LatencyBreakdown>,
}

/// Place record without signature bytes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Place {
    /// Place id.
    pub id: String,
    /// Camera id.
    pub camera_id: String,
    /// Name.
    pub name: String,
    /// Embedder version.
    pub embedder_version: String,
    /// Intrinsics JSON.
    #[schema(value_type = Object)]
    pub intrinsics: Value,
    /// ok or needs_realign.
    pub status: String,
    /// Active flag.
    pub active: bool,
    /// Created time.
    pub created_at: String,
    /// Updated time.
    pub updated_at: String,
}

/// Place patch.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlacePatch {
    /// Name.
    pub name: String,
}

/// Realign session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RealignSession {
    /// Session id.
    pub id: String,
    /// Anchor prompts.
    pub prompts: Vec<String>,
}

/// Realign point request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RealignPointRequest {
    /// Anchor id.
    pub anchor_id: String,
}

/// Realign point response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RealignPointResponse {
    /// Captured.
    pub captured: bool,
    /// Residual degrees.
    pub residual_deg: Option<f64>,
}

/// Realign commit response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RealignCommitResponse {
    /// Applied flag.
    pub applied: bool,
    /// Residual degrees.
    pub residual_deg: f64,
    /// Anchor ids needing reteach.
    pub needs_reteach: Vec<String>,
}

/// Anchor record.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Anchor {
    /// Anchor id.
    pub id: String,
    /// Place id.
    pub place_id: String,
    /// Name.
    pub name: String,
    /// Target JSON.
    #[schema(value_type = Object)]
    pub target: Value,
    /// Domain.
    pub domain: String,
    /// point3d, direction or region2d.
    pub kind: String,
    /// Verb params.
    #[schema(value_type = Object)]
    pub verb_params: Value,
    /// Sensitive.
    pub sensitive: bool,
    /// Sensitive ack.
    pub sensitive_ack: bool,
    /// ok, needs_realign or needs_reteach.
    pub status: String,
    /// Supported verbs for UI.
    pub verbs: Vec<VerbBinding>,
    /// Last used time.
    pub last_used_at: Option<String>,
    /// Created time.
    pub created_at: String,
    /// Updated time.
    pub updated_at: String,
    /// Camera that taught the anchor; extra spots must come from it.
    #[serde(default)]
    pub camera_id: Option<String>,
}

/// Verb binding shown for a selected target.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VerbBinding {
    /// Gesture id.
    pub gesture_id: String,
    /// Label.
    pub label: String,
}

/// Anchor patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct AnchorPatch {
    /// Name.
    pub name: Option<String>,
    /// Verb params JSON.
    #[schema(value_type = Object)]
    pub verb_params: Option<Value>,
}

/// Anchor test response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AnchorTestResponse {
    /// Selected flag.
    pub selected: bool,
    /// Angular error.
    pub angular_error_deg: f64,
    /// Runner-up anchor id.
    pub runner_up: Option<String>,
}

/// Teach request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachRequest {
    /// Camera id.
    pub camera_id: String,
    /// Target object with exactly one entity_id/device_id/area_id.
    #[schema(value_type = Object)]
    pub target: Value,
    /// Existing anchor id for reteach.
    pub anchor_id: Option<String>,
    /// Keep the anchor's taught spots and add new ones (requires `anchor_id`).
    #[serde(default)]
    pub append: bool,
}

/// Teach session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachSession {
    /// Session id.
    pub id: String,
    /// Camera id.
    pub camera_id: String,
    /// Target JSON.
    #[schema(value_type = Object)]
    pub target: Value,
    /// Existing anchor id.
    pub anchor_id: Option<String>,
    /// Prompt text.
    pub prompt: String,
}

/// Teach spot response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachSpotResponse {
    /// Spot index.
    pub spot_index: u32,
    /// Ray jitter.
    pub ray_jitter_deg: f64,
    /// Confidence.
    pub confidence: f64,
    /// direction or point3d.
    pub kind: String,
    /// Residual.
    pub residual_deg: Option<f64>,
}

/// Teach level request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachLevelRequest {
    /// One-based level.
    pub level: u32,
}

/// Teach level response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachLevelResponse {
    /// Taught levels.
    pub levels: Vec<f64>,
    /// Current percentage.
    pub current_percentage: f64,
}

/// Teach commit request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachCommitRequest {
    /// Anchor name.
    pub name: Option<String>,
    /// Verb mappings to create.
    pub verbs: Vec<TeachVerb>,
}

/// Teach verb mapping.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachVerb {
    /// Gesture id.
    pub gesture_id: String,
    /// Action.
    pub action: ActionDto,
}

/// Teach commit response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachCommitResponse {
    /// Created anchor.
    pub anchor: Anchor,
    /// Mapping ids.
    pub mapping_ids: Vec<String>,
    /// Distinctiveness warnings.
    pub distinctiveness_warnings: Vec<String>,
}

/// Setup assistant suggestion request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupSuggestRequest {
    /// Camera id.
    pub camera_id: String,
    /// local or foundry.
    pub provider: String,
}

/// Setup assistant suggestion.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupSuggestion {
    /// Label.
    pub label: String,
    /// Bounding box.
    pub bbox: [f32; 4],
    /// Candidate entity ids.
    pub candidates: Vec<String>,
}

/// Update state.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateStateDto {
    /// Channel.
    pub channel: String,
    /// App update state.
    pub app: AppUpdateState,
    /// Model/catalog packs.
    pub packs: Vec<ModelPack>,
    /// Index version.
    pub index_version: Option<u64>,
    /// Last check time.
    pub last_check_at: Option<String>,
    /// Busy reason.
    pub busy_reason: Option<String>,
}

/// App update state.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AppUpdateState {
    /// Current version.
    pub current: String,
    /// Available version.
    pub available: Option<String>,
    /// State.
    pub state: String,
    /// Rollback target.
    pub rollback_to: Option<String>,
}

/// Update install/rollback request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateActionRequest {
    /// app or pack.
    pub kind: String,
    /// Pack id for pack actions.
    pub id: Option<String>,
}

/// Model/catalog pack.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelPack {
    /// Pack id.
    pub id: String,
    /// Version.
    pub version: String,
    /// model or catalog.
    pub kind: String,
    /// State.
    pub state: String,
    /// Provides.
    pub provides: Vec<String>,
    /// SHA-256.
    pub sha256: String,
    /// Source.
    pub source: String,
    /// On-demand.
    pub on_demand: bool,
    /// Reject reason.
    pub reject_reason: Option<String>,
    /// Installed time.
    pub installed_at: String,
    /// Activated time.
    pub activated_at: Option<String>,
}

/// Gesture pack export request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PackExportRequest {
    /// Gesture ids.
    pub gesture_ids: Vec<String>,
    /// Include mapping templates.
    pub include_mapping_templates: bool,
}

/// Gesture pack JSON document.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GesturePackDto {
    /// Format marker.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Name.
    pub name: String,
    /// Raw payload.
    #[schema(value_type = Object)]
    pub payload: Value,
}

/// WebSocket client command.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientMessage {
    /// Subscribe to topics.
    Subscribe { topics: Vec<String> },
    /// Unsubscribe from topics.
    Unsubscribe { topics: Vec<String> },
}

/// WebSocket server message. All variants include `ts`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "payload")]
pub enum WsServerMessage {
    /// Initial hello.
    #[serde(rename = "hello")]
    Hello { ts: String, payload: HelloEvent },
    /// Engine status.
    #[serde(rename = "engine.status", alias = "engine_status")]
    EngineStatus {
        ts: String,
        payload: Box<EngineStatus>,
    },
    /// Camera status.
    #[serde(rename = "camera.status", alias = "camera_status")]
    CameraStatus { ts: String, payload: CameraStatus },
    /// Hand landmark stream.
    #[serde(rename = "hands")]
    Hands { ts: String, payload: HandsEvent },
    /// Gesture candidate.
    #[serde(rename = "gesture.candidate", alias = "gesture_candidate")]
    GestureCandidate {
        ts: String,
        payload: GestureCandidateEvent,
    },
    /// Gesture suppressed.
    #[serde(rename = "gesture.suppressed", alias = "gesture_suppressed")]
    GestureSuppressed {
        ts: String,
        payload: GestureSuppressedEvent,
    },
    /// Gesture fired.
    #[serde(rename = "gesture.fired", alias = "gesture_fired")]
    GestureFired {
        ts: String,
        payload: GestureFiredEvent,
    },
    /// Gesture update.
    #[serde(rename = "gesture.update", alias = "gesture_update")]
    GestureUpdate {
        ts: String,
        payload: GestureValueEvent,
    },
    /// Gesture end.
    #[serde(rename = "gesture.end", alias = "gesture_end")]
    GestureEnd {
        ts: String,
        payload: GestureValueEvent,
    },
    /// Armed.
    #[serde(rename = "armed")]
    Armed { ts: String, payload: ArmedEvent },
    /// Disarmed.
    #[serde(rename = "disarmed")]
    Disarmed { ts: String, payload: EmptyEvent },
    /// Confirmation required.
    #[serde(rename = "confirm.required", alias = "confirm_required")]
    ConfirmRequired {
        ts: String,
        payload: ConfirmRequiredEvent,
    },
    /// Action result.
    #[serde(rename = "action.result", alias = "action_result")]
    ActionResult {
        ts: String,
        payload: ActionResultEvent,
    },
    /// Home Assistant status.
    #[serde(rename = "ha.status", alias = "ha_status")]
    HaStatus { ts: String, payload: HaStatusEvent },
    /// Entity state update.
    #[serde(rename = "ha.entity", alias = "ha_entity")]
    HaEntity { ts: String, payload: HaEntityEvent },
    /// Capture progress.
    #[serde(rename = "capture.progress", alias = "capture_progress")]
    CaptureProgress {
        ts: String,
        payload: CaptureProgressEvent,
    },
    /// Engine paused.
    #[serde(rename = "engine.paused", alias = "engine_paused")]
    EnginePaused {
        ts: String,
        payload: EnginePausedEvent,
    },
    /// Engine resumed.
    #[serde(rename = "engine.resumed", alias = "engine_resumed")]
    EngineResumed { ts: String, payload: EmptyEvent },
    /// Target hover.
    #[serde(rename = "target.hover", alias = "target_hover")]
    TargetHover {
        ts: String,
        payload: TargetHoverEvent,
    },
    /// Target selected.
    #[serde(rename = "target.selected", alias = "target_selected")]
    TargetSelected {
        ts: String,
        payload: TargetSelectedEvent,
    },
    /// Target cleared.
    #[serde(rename = "target.cleared", alias = "target_cleared")]
    TargetCleared {
        ts: String,
        payload: TargetClearedEvent,
    },
    /// Target ambiguity.
    #[serde(rename = "target.ambiguous", alias = "target_ambiguous")]
    TargetAmbiguous {
        ts: String,
        payload: TargetAmbiguousEvent,
    },
    /// Place status.
    #[serde(rename = "place.status", alias = "place_status")]
    PlaceStatus {
        ts: String,
        payload: PlaceStatusEvent,
    },
    /// Teach progress.
    #[serde(rename = "teach.progress", alias = "teach_progress")]
    TeachProgress {
        ts: String,
        payload: TeachProgressEvent,
    },
    /// Update available.
    #[serde(rename = "update.available", alias = "update_available")]
    UpdateAvailable {
        ts: String,
        payload: UpdateAvailableEvent,
    },
    /// Update progress.
    #[serde(rename = "update.progress", alias = "update_progress")]
    UpdateProgress {
        ts: String,
        payload: UpdateProgressEvent,
    },
    /// App update ready.
    #[serde(rename = "update.ready", alias = "update_ready")]
    UpdateReady {
        ts: String,
        payload: UpdateReadyEvent,
    },
    /// Model activated.
    #[serde(rename = "model.activated", alias = "model_activated")]
    ModelActivated {
        ts: String,
        payload: ModelActivatedEvent,
    },
    /// Model rolled back.
    #[serde(rename = "model.rolled_back", alias = "model_rolled_back")]
    ModelRolledBack {
        ts: String,
        payload: ModelRolledBackEvent,
    },
    /// Client fell behind and must resync through REST.
    #[serde(rename = "resync")]
    Resync { ts: String, payload: ResyncEvent },
}

impl WsServerMessage {
    /// Returns the event topic used by the subscription filter.
    #[must_use]
    pub fn topic(&self) -> String {
        match self {
            Self::Hello { .. } => "hello".to_owned(),
            Self::EngineStatus { .. }
            | Self::CameraStatus { .. }
            | Self::EnginePaused { .. }
            | Self::EngineResumed { .. } => "status".to_owned(),
            Self::Hands { payload, .. } => format!("hands:{}", payload.camera_id),
            Self::GestureCandidate { .. }
            | Self::GestureSuppressed { .. }
            | Self::GestureFired { .. }
            | Self::GestureUpdate { .. }
            | Self::GestureEnd { .. }
            | Self::Armed { .. }
            | Self::Disarmed { .. } => "gestures".to_owned(),
            Self::ConfirmRequired { .. } | Self::ActionResult { .. } => "actions".to_owned(),
            Self::HaStatus { .. } | Self::HaEntity { .. } => "ha".to_owned(),
            Self::CaptureProgress { .. } => "capture".to_owned(),
            Self::TargetHover { .. }
            | Self::TargetSelected { .. }
            | Self::TargetCleared { .. }
            | Self::TargetAmbiguous { .. }
            | Self::PlaceStatus { .. } => "targeting".to_owned(),
            Self::TeachProgress { .. } => "teach".to_owned(),
            Self::UpdateAvailable { .. }
            | Self::UpdateProgress { .. }
            | Self::UpdateReady { .. }
            | Self::ModelActivated { .. }
            | Self::ModelRolledBack { .. } => "updates".to_owned(),
            Self::Resync { .. } => "resync".to_owned(),
        }
    }
}

/// Empty event payload.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EmptyEvent {}

/// Hello event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HelloEvent {
    /// Engine version.
    pub engine_version: String,
    /// API version.
    pub api: String,
    /// Pro enabled.
    pub pro: bool,
}

/// Hands stream event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandsEvent {
    /// Camera id.
    pub camera_id: String,
    /// Sequence.
    pub seq: u64,
    /// Hands.
    pub hands: Vec<HandEvent>,
    /// Pointing ray.
    pub ray: Option<RayEvent>,
}

/// One hand in the hand stream.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandEvent {
    /// Track id.
    pub track_id: u32,
    /// left or right.
    pub hand: String,
    /// 21 image-space landmarks.
    pub landmarks: Vec<[f32; 3]>,
    /// Bounding box x/y/w/h.
    pub bbox: [f32; 4],
}

/// Ray overlay event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RayEvent {
    /// Origin 2D.
    pub origin2d: [f32; 2],
    /// Tip 2D.
    pub tip2d: [f32; 2],
    /// Ray model.
    pub model: String,
}

/// Gesture candidate event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureCandidateEvent {
    /// Camera id.
    pub camera_id: String,
    /// Track id.
    pub track_id: u32,
    /// Gesture id.
    pub gesture_id: String,
    /// Confidence.
    pub confidence: f64,
    /// Progress.
    pub progress: f64,
}

/// Gesture suppressed event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureSuppressedEvent {
    /// Camera id.
    pub camera_id: String,
    /// Gesture id.
    pub gesture_id: String,
    /// Reason.
    pub reason: String,
}

/// Gesture fired event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureFiredEvent {
    /// Event id.
    pub event_id: String,
    /// Camera id.
    pub camera_id: String,
    /// Gesture id.
    pub gesture_id: String,
    /// Hand.
    pub hand: String,
    /// Confidence.
    pub confidence: f64,
    /// Mapping ids.
    pub mapping_ids: Vec<String>,
    /// Action summary.
    pub action_summary: String,
}

/// Gesture update/end event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GestureValueEvent {
    /// Event id.
    pub event_id: String,
    /// Value for dial gestures.
    pub value: Option<f64>,
}

/// Armed event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ArmedEvent {
    /// Expiry.
    pub until: String,
}

/// Confirmation required event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConfirmRequiredEvent {
    /// Event id.
    pub event_id: String,
    /// Mapping id.
    pub mapping_id: String,
    /// Confirmation gesture id.
    pub confirm_gesture_id: String,
    /// Expiry.
    pub expires_at: String,
}

/// Action result event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActionResultEvent {
    /// Activity id.
    pub activity_id: String,
    /// Event id.
    pub event_id: String,
    /// Mapping id.
    pub mapping_id: String,
    /// Status.
    pub status: String,
    /// Error code.
    pub error_code: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// Latency.
    pub latency: LatencyBreakdown,
}

/// HA status event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaStatusEvent {
    /// State.
    pub state: String,
    /// HA version.
    pub ha_version: Option<String>,
}

/// HA entity event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HaEntityEvent {
    /// Entity id.
    pub entity_id: String,
    /// State.
    pub state: String,
    /// Attributes.
    #[schema(value_type = Object)]
    pub attributes: Value,
}

/// Capture progress event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CaptureProgressEvent {
    /// Session id.
    pub session_id: String,
    /// Current take.
    pub take: u32,
    /// Total takes.
    pub takes: u32,
    /// Phase.
    pub phase: String,
    /// Quality hint.
    pub quality_hint: Option<String>,
}

/// Engine paused event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EnginePausedEvent {
    /// Optional expiry.
    pub until: Option<String>,
}

/// Target hover event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TargetHoverEvent {
    /// Camera id.
    pub camera_id: String,
    /// Anchor id.
    pub anchor_id: String,
    /// Name.
    pub name: String,
    /// Score.
    pub score: f64,
    /// Dwell progress.
    pub dwell_progress: f64,
    /// Runner-up.
    pub runner_up: Option<String>,
}

/// Target selected event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TargetSelectedEvent {
    /// Camera id.
    pub camera_id: String,
    /// Anchor id.
    pub anchor_id: String,
    /// Name.
    pub name: String,
    /// Domain.
    pub domain: String,
    /// Expiry.
    pub expires_at: String,
    /// Verbs.
    pub verbs: Vec<VerbBinding>,
}

/// Target cleared event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TargetClearedEvent {
    /// Camera id.
    pub camera_id: String,
    /// Anchor id.
    pub anchor_id: String,
    /// timeout, hand_lost, reselected or paused.
    pub reason: String,
}

/// Ambiguous target event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TargetAmbiguousEvent {
    /// Camera id.
    pub camera_id: String,
    /// Anchor ids.
    pub anchor_ids: Vec<String>,
}

/// Place status event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlaceStatusEvent {
    /// Camera id.
    pub camera_id: String,
    /// Place id.
    pub place_id: String,
    /// ok, switched or needs_realign.
    pub state: String,
    /// Similarity.
    pub similarity: f64,
}

/// Teach progress event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TeachProgressEvent {
    /// Session id.
    pub session_id: String,
    /// aiming, capturing, captured or error.
    pub phase: String,
    /// Ray jitter.
    pub ray_jitter_deg: Option<f64>,
    /// Confidence.
    pub confidence: Option<f64>,
    /// Hint.
    pub hint: Option<String>,
}

/// Update available event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateAvailableEvent {
    /// app or pack.
    pub kind: String,
    /// Pack id or app.
    pub id: String,
    /// Version.
    pub version: String,
    /// Size bytes.
    pub size: u64,
}

/// Update progress event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProgressEvent {
    /// app or pack.
    pub kind: String,
    /// Id.
    pub id: Option<String>,
    /// download, verify, selftest, benchmark or shadow.
    pub phase: String,
    /// Bytes downloaded.
    pub bytes: Option<u64>,
    /// Total bytes.
    pub total: Option<u64>,
}

/// Update ready event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateReadyEvent {
    /// app.
    pub kind: String,
    /// Version.
    pub version: String,
}

/// Model activated event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelActivatedEvent {
    /// Pack id.
    pub pack_id: String,
    /// Version.
    pub version: String,
    /// Previous version.
    pub previous: String,
}

/// Model rollback event.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelRolledBackEvent {
    /// Pack id.
    pub pack_id: String,
    /// From version.
    pub from: String,
    /// To version.
    pub to: String,
    /// Reason.
    pub reason: String,
}

/// Resync hint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResyncEvent {
    /// Reason.
    pub reason: String,
    /// Topics needing REST resync.
    pub topics: Vec<String>,
}
