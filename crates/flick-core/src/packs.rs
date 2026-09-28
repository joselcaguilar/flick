//! OTA model/catalog pack identity and activation types.

use serde::{Deserialize, Serialize};

/// OTA pack kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackKind {
    /// Model pack containing ONNX assets and metadata.
    Model,
    /// Catalog pack containing parameters, presets and safety additions.
    Catalog,
}

/// Monotonic generation marker for hot-swapped packs.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct PackGeneration(pub u64);

/// Active pack metadata cloned by hot-path consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivePack {
    /// Pack id such as `hand-landmarker` or `catalog`.
    pub id: String,
    /// Pack version string from signed metadata.
    pub version: String,
    /// Pack kind.
    pub kind: PackKind,
    /// Current generation marker.
    pub generation: PackGeneration,
    /// Capabilities provided by this pack.
    pub provides: Vec<String>,
    /// Pack sha256 if installed from OTA.
    pub sha256: Option<String>,
}
