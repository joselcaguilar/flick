//! Runtime configuration loading for the engine sidecar.

use std::{env, fs, path::PathBuf};

use directories::BaseDirs;
use flick_core::{BootstrapConfig, ExecutionProvider};
use thiserror::Error;

/// Desktop application identifier used for platform data directories.
pub const APP_ID: &str = "app.flick.desktop";

/// Errors returned while loading runtime configuration.
#[derive(Debug, Error)]
pub enum ConfigLoadError {
    /// The platform data directory could not be determined.
    #[error("could not determine platform data directory")]
    MissingDataDir,
    /// The configuration file could not be read.
    #[error("failed to read config file {path}: {source}")]
    ReadConfig {
        /// Path being read.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The TOML configuration could not be parsed.
    #[error("failed to parse config file {path}: {source}")]
    ParseConfig {
        /// Path being parsed.
        path: PathBuf,
        /// TOML parse error.
        #[source]
        source: toml::de::Error,
    },
    /// An environment or CLI override was invalid.
    #[error("invalid override {name}: {value}")]
    InvalidOverride {
        /// Override name.
        name: &'static str,
        /// Override value.
        value: String,
    },
}

/// CLI values that override file and environment configuration.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// Run as a desktop sidecar.
    pub sidecar: bool,
    /// Enable development mode defaults.
    pub dev: bool,
    /// Override data directory.
    pub data_dir: Option<PathBuf>,
    /// Override API port.
    pub port: Option<u16>,
    /// Override fake camera source.
    pub fake_camera: Option<PathBuf>,
    /// Force mock Home Assistant.
    pub mock_ha: Option<bool>,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Bootstrap config from config.toml plus overrides.
    pub bootstrap: BootstrapConfig,
    /// Platform data directory.
    pub data_dir: PathBuf,
    /// Path to replay video/directory, if any.
    pub fake_camera: Option<PathBuf>,
    /// Path to replay landmark JSONL, if any.
    pub fake_landmarks: Option<PathBuf>,
    /// Development update endpoint, if set.
    pub update_url: Option<String>,
    /// Whether this process is managed as a desktop sidecar.
    pub sidecar: bool,
    /// Whether development mode is enabled.
    pub dev: bool,
    /// Whether the engine should start and use mock Home Assistant.
    pub mock_ha: bool,
}

/// Loads runtime configuration from the platform data directory and environment.
pub fn load() -> Result<RuntimeConfig, ConfigLoadError> {
    load_with_overrides(&CliOverrides::default())
}

/// Loads runtime configuration and applies CLI overrides last.
pub fn load_with_overrides(overrides: &CliOverrides) -> Result<RuntimeConfig, ConfigLoadError> {
    let mut data_dir = match env_path("FLICK_DATA_DIR") {
        Some(path) => path,
        None => default_data_dir()?,
    };
    if let Some(path) = &overrides.data_dir {
        data_dir = path.clone();
    }

    let config_path = data_dir.join("config.toml");
    let mut bootstrap = if config_path.exists() {
        let raw =
            fs::read_to_string(&config_path).map_err(|source| ConfigLoadError::ReadConfig {
                path: config_path.clone(),
                source,
            })?;
        toml::from_str(&raw).map_err(|source| ConfigLoadError::ParseConfig {
            path: config_path.clone(),
            source,
        })?
    } else {
        BootstrapConfig::default()
    };

    if let Ok(value) = env::var("FLICK_LOG") {
        bootstrap.engine.log_level = value;
    }
    if let Some(port) = env_u16("FLICK_PORT")? {
        bootstrap.engine.port = port;
    }
    if let Ok(value) = env::var("FLICK_EP") {
        bootstrap.inference.execution_provider = parse_execution_provider(&value)?;
    }

    if let Some(port) = overrides.port {
        bootstrap.engine.port = port;
    }
    if overrides.dev && bootstrap.engine.port == 0 {
        bootstrap.engine.port = 7871;
    }
    if !overrides.sidecar && !overrides.dev && bootstrap.engine.port == 0 {
        bootstrap.engine.port = 7870;
    }

    let fake_camera = overrides
        .fake_camera
        .clone()
        .or_else(|| env_path("FLICK_FAKE_CAMERA"));
    let fake_landmarks = env_path("FLICK_FAKE_LANDMARKS");
    let update_url = env::var("FLICK_UPDATE_URL")
        .ok()
        .filter(|value| !value.is_empty());
    let mock_ha = overrides
        .mock_ha
        .or_else(|| env_bool("FLICK_MOCK_HA"))
        .unwrap_or(overrides.dev);

    Ok(RuntimeConfig {
        bootstrap,
        data_dir,
        fake_camera,
        fake_landmarks,
        update_url,
        sidecar: overrides.sidecar,
        dev: overrides.dev,
        mock_ha,
    })
}

fn default_data_dir() -> Result<PathBuf, ConfigLoadError> {
    BaseDirs::new()
        .map(|dirs| dirs.data_dir().join(APP_ID))
        .ok_or(ConfigLoadError::MissingDataDir)
}

fn env_path(name: &'static str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn env_u16(name: &'static str) -> Result<Option<u16>, ConfigLoadError> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => value
            .parse::<u16>()
            .map(Some)
            .map_err(|_| ConfigLoadError::InvalidOverride { name, value }),
        Ok(_) | Err(_) => Ok(None),
    }
}

fn env_bool(name: &'static str) -> Option<bool> {
    let value = env::var(name).ok()?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_execution_provider(value: &str) -> Result<ExecutionProvider, ConfigLoadError> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(ExecutionProvider::Auto),
        "coreml" => Ok(ExecutionProvider::Coreml),
        "directml" => Ok(ExecutionProvider::Directml),
        "cuda" => Ok(ExecutionProvider::Cuda),
        "openvino" => Ok(ExecutionProvider::Openvino),
        "xnnpack" => Ok(ExecutionProvider::Xnnpack),
        "cpu" => Ok(ExecutionProvider::Cpu),
        _ => Err(ConfigLoadError::InvalidOverride {
            name: "FLICK_EP",
            value: value.to_owned(),
        }),
    }
}
