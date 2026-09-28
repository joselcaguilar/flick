//! Bootstrap `config.toml` structures.

use serde::{Deserialize, Serialize};

/// Engine bootstrap configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Socket bind address. Desktop mode uses loopback.
    pub bind: String,
    /// API port. `0` means random for the desktop sidecar.
    pub port: u16,
    /// Default log filter when `FLICK_LOG` is not set.
    pub log_level: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".to_owned(),
            port: 0,
            log_level: "info".to_owned(),
        }
    }
}

/// Supported inference execution-provider choices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvider {
    /// Auto-benchmark providers and use the fastest per model.
    #[default]
    Auto,
    /// ONNX Runtime CoreML execution provider.
    Coreml,
    /// ONNX Runtime DirectML execution provider.
    Directml,
    /// CUDA execution provider.
    Cuda,
    /// OpenVINO execution provider.
    Openvino,
    /// XNNPACK execution provider.
    Xnnpack,
    /// CPU execution provider.
    Cpu,
}

/// Inference bootstrap configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Execution provider preference.
    pub execution_provider: ExecutionProvider,
    /// ORT intra-op thread count.
    pub intra_threads: u32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            execution_provider: ExecutionProvider::Auto,
            intra_threads: 2,
        }
    }
}

/// Bootstrap paths configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PathsConfig {
    /// Empty means bundled resources; otherwise model pack root.
    pub models_dir: String,
}

/// Complete bootstrap config loaded from `<data>/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BootstrapConfig {
    /// Engine networking and logging config.
    #[serde(default)]
    pub engine: EngineConfig,
    /// Inference provider config.
    #[serde(default)]
    pub inference: InferenceConfig,
    /// Bootstrap path overrides.
    #[serde(default)]
    pub paths: PathsConfig,
}
