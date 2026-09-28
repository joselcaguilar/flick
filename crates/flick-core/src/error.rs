//! Shared error enums for Flick library crates.

use thiserror::Error;

/// Capture source failures.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// The camera or stream disconnected.
    #[error("capture source disconnected")]
    Disconnected,
    /// The requested source is unavailable or permission was denied.
    #[error("capture source unavailable: {0}")]
    Unavailable(String),
    /// Camera permission is denied, restricted or still unavailable after prompting.
    #[error("camera permission denied: {0}")]
    PermissionDenied(String),
    /// The source produced an unsupported pixel format or dimensions.
    #[error("unsupported capture format: {0}")]
    UnsupportedFormat(String),
    /// A frame could not be decoded.
    #[error("frame decode failed: {0}")]
    Decode(String),
    /// Capture timed out.
    #[error("capture timed out")]
    Timeout,
    /// Any other capture error.
    #[error("capture error: {0}")]
    Other(String),
}

/// Vision pipeline failures.
#[derive(Debug, Error)]
pub enum VisionError {
    /// Required model or pack was not available.
    #[error("model unavailable: {0}")]
    ModelUnavailable(String),
    /// Model output did not match the expected shape.
    #[error("invalid model output: {0}")]
    InvalidModelOutput(String),
    /// Inference backend failed.
    #[error("inference failed: {0}")]
    Inference(String),
    /// Input frame was unsupported by the pipeline.
    #[error("unsupported frame: {0}")]
    UnsupportedFrame(String),
}

/// Gesture-recognition failures.
#[derive(Debug, Error)]
pub enum GestureError {
    /// Gesture id or catalog entry is invalid.
    #[error("invalid gesture: {0}")]
    InvalidGesture(String),
    /// Training data is insufficient.
    #[error("insufficient training data: {0}")]
    InsufficientData(String),
    /// Recognition model failed.
    #[error("gesture recognizer failed: {0}")]
    Recognizer(String),
}

/// Targeting and spatial failures.
#[derive(Debug, Error)]
pub enum TargetingError {
    /// The active place needs realignment.
    #[error("place needs realignment")]
    NeedsRealign,
    /// The anchor cannot be resolved.
    #[error("anchor not found: {0}")]
    AnchorNotFound(String),
    /// Geometry computation failed.
    #[error("geometry failed: {0}")]
    Geometry(String),
}

/// Home Assistant integration failures.
#[derive(Debug, Error)]
pub enum HaError {
    /// Authentication failed.
    #[error("Home Assistant authentication failed")]
    AuthFailed,
    /// The socket is disconnected.
    #[error("Home Assistant disconnected")]
    Disconnected,
    /// Home Assistant returned an error code.
    #[error("Home Assistant error {code}: {message}")]
    Service { code: String, message: String },
    /// Request timed out.
    #[error("Home Assistant request timed out")]
    Timeout,
}

/// Storage failures.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Database operation failed.
    #[error("database error: {0}")]
    Database(String),
    /// Stored JSON could not be decoded.
    #[error("invalid stored JSON: {0}")]
    InvalidJson(String),
    /// Migration failed.
    #[error("migration failed: {0}")]
    Migration(String),
}

/// API-layer failures.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Request was not authenticated.
    #[error("unauthorized")]
    Unauthorized,
    /// Request origin or host was forbidden.
    #[error("forbidden")]
    Forbidden,
    /// Request body failed validation.
    #[error("validation failed: {0}")]
    Validation(String),
}

/// Pack update failures.
#[derive(Debug, Error)]
pub enum PackError {
    /// Signature verification failed.
    #[error("signature verification failed")]
    Signature,
    /// Hash verification failed.
    #[error("sha256 verification failed")]
    Sha256,
    /// Pack metadata was stale or expired.
    #[error("stale update metadata")]
    StaleMetadata,
    /// Pack self-test failed.
    #[error("pack self-test failed: {0}")]
    SelfTest(String),
}

/// Engine bootstrap and supervisor failures.
#[derive(Debug, Error)]
pub enum EngineError {
    /// Configuration failed.
    #[error("configuration error: {0}")]
    Config(String),
    /// Worker failed.
    #[error("worker failed: {0}")]
    Worker(String),
    /// Shutdown timed out.
    #[error("shutdown timed out")]
    ShutdownTimeout,
}
