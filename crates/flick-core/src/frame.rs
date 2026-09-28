//! Camera frame and source metadata types.

use std::{sync::Arc, time::SystemTime};

use serde::{Deserialize, Serialize};

use crate::CameraId;

/// Pixel storage used by a [`Frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    /// 24-bit RGB, one byte per channel.
    Rgb8,
    /// 32-bit BGRA, one byte per channel.
    Bgra8,
    /// NV12 YUV with an interleaved UV plane.
    Nv12,
}

/// The configured camera source kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A local webcam or Continuity Camera.
    Local,
    /// An RTSP or RTSPS network stream.
    Rtsp,
    /// A deterministic file or image-sequence source.
    File,
}

/// Static information about a frame source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Flick camera id.
    pub id: CameraId,
    /// Source implementation kind.
    pub kind: SourceKind,
    /// Captured frame width in pixels.
    pub width: u32,
    /// Captured frame height in pixels.
    pub height: u32,
    /// Nominal frame rate.
    pub fps: u32,
    /// Whether input frames are mirrored before hand normalization.
    pub mirror: bool,
}

/// One captured camera frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Flick camera id.
    pub camera_id: CameraId,
    /// Monotonic sequence number from this source.
    pub seq: u64,
    /// Monotonic capture timestamp.
    pub captured_at: std::time::Instant,
    /// Wall-clock capture timestamp.
    pub wall_ts: SystemTime,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel format of [`Self::data`].
    pub format: PixelFormat,
    /// Frame bytes owned by the capture stage.
    pub data: Arc<[u8]>,
}
