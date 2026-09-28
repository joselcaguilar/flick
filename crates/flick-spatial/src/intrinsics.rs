use serde::{Deserialize, Serialize};

/// Version string for catalog-provided FOV table entries.
pub const DEFAULT_INTRINSICS_VERSION: &str = "catalog.fov.v1";

/// Source of camera intrinsics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrinsicsSource {
    /// Entry shipped by the signed catalog FOV table.
    FovTable,
    /// Sane built-in default when the camera is unknown.
    Default,
    /// Caller supplied calibrated intrinsics.
    Calibrated,
}

/// Pinhole camera intrinsics in pixel units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraIntrinsics {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Focal length in horizontal pixel units.
    pub fx: f32,
    /// Focal length in vertical pixel units.
    pub fy: f32,
    /// Principal point x in pixels.
    pub cx: f32,
    /// Principal point y in pixels.
    pub cy: f32,
    /// Horizontal field-of-view in degrees, retained for diagnostics.
    pub hfov_deg: f32,
    /// Source used to derive the intrinsics.
    pub source: IntrinsicsSource,
    /// Catalog/calibration version.
    pub version: String,
}

impl CameraIntrinsics {
    /// Builds intrinsics from a horizontal FOV and centered principal point.
    #[must_use]
    pub fn from_horizontal_fov(
        width: u32,
        height: u32,
        hfov_deg: f32,
        source: IntrinsicsSource,
        version: impl Into<String>,
    ) -> Self {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let hfov = hfov_deg.clamp(10.0, 170.0).to_radians();
        let fx = (w * 0.5) / (hfov * 0.5).tan();
        let fy = fx;
        Self {
            width,
            height,
            fx,
            fy,
            cx: w * 0.5,
            cy: h * 0.5,
            hfov_deg,
            source,
            version: version.into(),
        }
    }

    /// Sane unknown-camera default from the targeting spec: 70° horizontal FOV.
    #[must_use]
    pub fn sane_default(width: u32, height: u32) -> Self {
        Self::from_horizontal_fov(
            width,
            height,
            70.0,
            IntrinsicsSource::Default,
            DEFAULT_INTRINSICS_VERSION,
        )
    }

    /// Default for Apple MacBook built-in cameras at 1280×720.
    #[must_use]
    pub fn macbook_builtin_default() -> Self {
        Self::from_horizontal_fov(
            1280,
            720,
            68.0,
            IntrinsicsSource::FovTable,
            DEFAULT_INTRINSICS_VERSION,
        )
    }

    /// Looks up a catalog FOV entry by device key and resolution.
    #[must_use]
    pub fn from_fov_table(device_key: &str, width: u32, height: u32) -> Option<Self> {
        FOV_TABLE
            .iter()
            .find(|entry| entry.device_key == device_key)
            .map(|entry| {
                Self::from_horizontal_fov(
                    width,
                    height,
                    entry.hfov_deg,
                    IntrinsicsSource::FovTable,
                    entry.version,
                )
            })
    }

    /// Converts a normalized image point to normalized camera coordinates.
    #[must_use]
    pub fn normalized_camera_xy(&self, image_xy: [f32; 2]) -> [f32; 2] {
        let px = image_xy[0] * self.width.max(1) as f32;
        let py = image_xy[1] * self.height.max(1) as f32;
        [(px - self.cx) / self.fx, (py - self.cy) / self.fy]
    }

    /// Projects a camera-space point to normalized image coordinates.
    #[must_use]
    pub fn project(&self, point: [f32; 3]) -> Option<[f32; 2]> {
        if point[2] <= 1.0e-5 || !point[2].is_finite() {
            return None;
        }
        let px = self.fx * point[0] / point[2] + self.cx;
        let py = self.fy * point[1] / point[2] + self.cy;
        Some([
            px / self.width.max(1) as f32,
            py / self.height.max(1) as f32,
        ])
    }

    /// Returns a camera-space unit direction through a normalized image point.
    #[must_use]
    pub fn bearing(&self, image_xy: [f32; 2]) -> [f32; 3] {
        let [x, y] = self.normalized_camera_xy(image_xy);
        let norm = (x * x + y * y + 1.0).sqrt().max(1.0e-6);
        [x / norm, y / norm, 1.0 / norm]
    }
}

/// A catalog FOV table entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFov {
    /// Stable catalog key supplied by capture/device discovery.
    pub device_key: &'static str,
    /// Horizontal FOV in degrees.
    pub hfov_deg: f32,
    /// Entry version.
    pub version: &'static str,
}

/// Minimal built-in FOV table; OTA catalog entries can replace or extend it.
pub const FOV_TABLE: &[CameraFov] = &[
    CameraFov {
        device_key: "apple.macbook_builtin",
        hfov_deg: 68.0,
        version: DEFAULT_INTRINSICS_VERSION,
    },
    CameraFov {
        device_key: "apple.studio_display",
        hfov_deg: 122.0,
        version: DEFAULT_INTRINSICS_VERSION,
    },
    CameraFov {
        device_key: "generic.webcam_70deg",
        hfov_deg: 70.0,
        version: DEFAULT_INTRINSICS_VERSION,
    },
];
