//! Errors and result mapping for Home Assistant operations.

use flick_core::{ActionOutcome, ActionStatus};
use thiserror::Error;

/// Errors produced by the HA client before a dispatcher outcome exists.
#[derive(Debug, Error)]
pub enum HaError {
    /// The configured URL could not be parsed or converted to `/api/websocket`.
    #[error("invalid Home Assistant URL: {0}")]
    InvalidUrl(String),
    /// The WebSocket transport failed.
    #[error("websocket error: {0}")]
    WebSocket(String),
    /// The server sent a malformed or unexpected message.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Home Assistant rejected the token.
    #[error("Home Assistant authentication failed: {0}")]
    AuthInvalid(String),
    /// The client is disconnected.
    #[error("Home Assistant is disconnected")]
    Disconnected,
    /// A request exceeded its deadline.
    #[error("Home Assistant request timed out")]
    Timeout,
    /// The background client task has stopped.
    #[error("Home Assistant client task stopped")]
    ClientStopped,
    /// A channel between the handle and actor closed unexpectedly.
    #[error("Home Assistant client channel closed")]
    ChannelClosed,
    /// The action cannot be sent directly to Home Assistant.
    #[error("unsupported Home Assistant action")]
    UnsupportedAction,
    /// JSON serialization or parsing failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Keychain access failed.
    #[error("keychain error: {0}")]
    Keyring(String),
    /// mDNS discovery failed.
    #[error("mDNS error: {0}")]
    Mdns(String),
}

/// Maps a Home Assistant error code to the stable Flick error code and message.
#[must_use]
pub fn map_ha_error(code: Option<&str>, message: Option<&str>) -> (String, String) {
    match code {
        Some("not_found") => (
            "error.not_found".to_owned(),
            "Service or device not found — check the mapping".to_owned(),
        ),
        Some("invalid_format" | "service_validation_error") => (
            "error.invalid".to_owned(),
            message
                .unwrap_or("Home Assistant rejected the service data")
                .to_owned(),
        ),
        Some("unauthorized") => (
            "error.unauthorized".to_owned(),
            "Flick's HA user can't do this".to_owned(),
        ),
        Some("home_assistant_error") => (
            "error.ha".to_owned(),
            message
                .unwrap_or("Home Assistant reported an error")
                .to_owned(),
        ),
        Some(other) => (
            format!("error.{other}"),
            message
                .unwrap_or("Home Assistant didn't respond")
                .to_owned(),
        ),
        None => (
            "error.unknown".to_owned(),
            "Home Assistant didn't respond".to_owned(),
        ),
    }
}

/// Builds an [`ActionOutcome`] for a mapped HA error.
#[must_use]
pub fn error_outcome(
    code: Option<&str>,
    message: Option<&str>,
    latency_ms: Option<u64>,
) -> ActionOutcome {
    let (error_code, user_message) = map_ha_error(code, message);
    ActionOutcome {
        activity_id: None,
        status: ActionStatus::Error,
        error_code: Some(error_code),
        message: Some(user_message),
        ha_context_id: None,
        latency_ms,
    }
}

/// Outcome used when a disconnected action exceeded the stale-action guard.
#[must_use]
pub fn stale_outcome() -> ActionOutcome {
    ActionOutcome {
        activity_id: None,
        status: ActionStatus::Stale,
        error_code: Some("stale".to_owned()),
        message: Some("Dropped because Home Assistant was disconnected".to_owned()),
        ha_context_id: None,
        latency_ms: None,
    }
}

/// Outcome used when a request exceeds its deadline.
#[must_use]
pub fn timeout_outcome() -> ActionOutcome {
    ActionOutcome {
        activity_id: None,
        status: ActionStatus::Timeout,
        error_code: Some("timeout".to_owned()),
        message: Some("Home Assistant did not confirm in time".to_owned()),
        ha_context_id: None,
        latency_ms: None,
    }
}
