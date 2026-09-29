//! Shared public data types for the HA crate.

use std::{fmt, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::watch;
use url::Url;

use crate::{ClientIdentity, HaError};

/// Default request timeout from the HA spec.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Default stale-action guard while disconnected.
pub const DEFAULT_STALE_ACTION: Duration = Duration::from_secs(2);
/// Default JSON ping cadence.
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(20);
/// Default ping response timeout.
pub const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(10);

/// Connection options for [`crate::HaClient`].
#[derive(Clone)]
pub struct HaConnectionConfig {
    /// WebSocket URL (`ws://.../api/websocket`) or HTTP(S) base URL.
    pub url: String,
    /// Long-lived access token. This value is redacted from [`Debug`].
    pub token: String,
    /// Per-request timeout.
    pub request_timeout: Duration,
    /// How long a disconnected action may wait before being dropped.
    pub stale_action: Duration,
    /// JSON ping interval.
    pub ping_interval: Duration,
    /// Ping result timeout before reconnect.
    pub ping_timeout: Duration,
    /// Initial reconnect backoff.
    pub reconnect_initial: Duration,
    /// Maximum reconnect backoff.
    pub reconnect_max: Duration,
    /// Optional SHA-256 certificate fingerprint pin for self-signed HA.
    /// Applies to [`Self::url`] only.
    pub cert_sha256: Option<String>,
    /// Optional LAN WebSocket URL ("Home URL"), tried first when Flick is on
    /// the home network. [`Self::url`] is the remote URL.
    pub internal_url: Option<String>,
    /// Wi-Fi networks that count as home. When the current SSID is unknown or
    /// this list is empty, home is confirmed via mDNS instead.
    pub trusted_ssids: Vec<String>,
    /// HA instance UUID, used to recognise this instance over mDNS.
    pub ha_uuid: Option<String>,
    /// Current Wi-Fi SSID reported by the host (`None` when unknown).
    pub network: Option<watch::Receiver<Option<String>>>,
    /// Client certificate presented when a `wss://` server asks for one (mTLS).
    pub client_identity: Option<Arc<ClientIdentity>>,
}

impl HaConnectionConfig {
    /// Creates a config from an HA base URL or WebSocket URL and LLAT.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Result<Self, HaError> {
        let url = websocket_url(&url.into())?;
        Ok(Self {
            url,
            token: token.into(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            stale_action: DEFAULT_STALE_ACTION,
            ping_interval: DEFAULT_PING_INTERVAL,
            ping_timeout: DEFAULT_PING_TIMEOUT,
            reconnect_initial: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(30),
            cert_sha256: None,
            internal_url: None,
            trusted_ssids: Vec::new(),
            ha_uuid: None,
            network: None,
            client_identity: None,
        })
    }

    /// Sets the LAN ("Home") URL; `None` or blank clears it.
    pub fn with_internal_url(mut self, raw: Option<&str>) -> Result<Self, HaError> {
        self.internal_url = match raw.map(str::trim).filter(|raw| !raw.is_empty()) {
            Some(raw) => Some(websocket_url(raw)?),
            None => None,
        };
        Ok(self)
    }
}

impl fmt::Debug for HaConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HaConnectionConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("request_timeout", &self.request_timeout)
            .field("stale_action", &self.stale_action)
            .field("ping_interval", &self.ping_interval)
            .field("ping_timeout", &self.ping_timeout)
            .field("reconnect_initial", &self.reconnect_initial)
            .field("reconnect_max", &self.reconnect_max)
            .field(
                "cert_sha256",
                &self.cert_sha256.as_ref().map(|_| "<sha256>"),
            )
            .field("internal_url", &self.internal_url)
            .field("trusted_ssids", &self.trusted_ssids)
            .field("ha_uuid", &self.ha_uuid)
            .field(
                "client_identity",
                &self.client_identity.as_ref().map(|id| &id.info().subject),
            )
            .finish_non_exhaustive()
    }
}

/// Converts HA HTTP(S) base URLs to WebSocket API URLs.
pub fn websocket_url(raw: &str) -> Result<String, HaError> {
    let mut url = Url::parse(raw).map_err(|err| HaError::InvalidUrl(err.to_string()))?;
    match url.scheme() {
        "http" => url
            .set_scheme("ws")
            .map_err(|()| HaError::InvalidUrl(raw.to_owned()))?,
        "https" => url
            .set_scheme("wss")
            .map_err(|()| HaError::InvalidUrl(raw.to_owned()))?,
        "ws" | "wss" => {}
        other => return Err(HaError::InvalidUrl(format!("unsupported scheme {other}"))),
    }
    if url.path() == "/" || url.path().is_empty() {
        url.set_path("/api/websocket");
    }
    Ok(url.to_string())
}

/// Converts a WebSocket API URL back to the HA base URL users recognise.
#[must_use]
pub fn http_base_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_owned();
    };
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        other => other,
    }
    .to_owned();
    let _ = url.set_scheme(&scheme);
    if url.path() == "/api/websocket" {
        url.set_path("");
    }
    url.to_string().trim_end_matches('/').to_owned()
}

/// Which Home Assistant URL the client is currently connected through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HaRoute {
    /// HA base URL (`http(s)://host:port`).
    pub url: String,
    /// `true` for the Home (LAN) URL, `false` for the Remote URL.
    pub internal: bool,
}

/// Connection lifecycle state surfaced to the API and UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum HaStatus {
    /// No socket is currently open.
    Disconnected,
    /// A socket is opening and authenticating.
    Connecting,
    /// The socket is authenticated and command-ready.
    Ready {
        /// HA version from the auth handshake.
        ha_version: Option<String>,
    },
    /// HA rejected the token; reconnects stop until credentials change.
    AuthFailed {
        /// Redacted user-readable auth failure.
        message: String,
    },
    /// Waiting before the next reconnect attempt.
    Reconnecting {
        /// One-based reconnect attempt counter.
        attempt: u32,
        /// Why the previous connection attempt failed, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
    },
}

/// A Home Assistant entity state used by registries, subscriptions and verb resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityState {
    /// Entity id, e.g. `fan.ventilador_dormitorio`.
    pub entity_id: String,
    /// HA state string.
    pub state: String,
    /// HA attributes object.
    #[serde(default)]
    pub attributes: Map<String, Value>,
    /// ISO timestamp when HA last changed state.
    #[serde(default)]
    pub last_changed: Option<String>,
    /// ISO timestamp when HA last updated state or attributes.
    #[serde(default)]
    pub last_updated: Option<String>,
}

impl EntityState {
    /// Returns the domain prefix before the first dot.
    #[must_use]
    pub fn domain(&self) -> &str {
        self.entity_id
            .split_once('.')
            .map_or("", |(domain, _)| domain)
    }

    /// Friendly display name from attributes or the entity id.
    #[must_use]
    pub fn friendly_name(&self) -> String {
        self.attributes
            .get("friendly_name")
            .and_then(Value::as_str)
            .map_or_else(|| self.entity_id.clone(), ToOwned::to_owned)
    }

    /// Numeric supported_features bitset from HA attributes.
    #[must_use]
    pub fn supported_features(&self) -> u64 {
        self.attributes
            .get("supported_features")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    /// Optional HA device_class from attributes.
    #[must_use]
    pub fn device_class(&self) -> Option<&str> {
        self.attributes.get("device_class").and_then(Value::as_str)
    }

    /// Reads a numeric attribute as f64.
    #[must_use]
    pub fn attr_f64(&self, name: &str) -> Option<f64> {
        self.attributes.get(name).and_then(Value::as_f64)
    }
}

/// Events emitted by the HA client broadcast stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum HaEvent {
    /// Connection status changed.
    Status { status: HaStatus },
    /// A subscribed entity changed.
    Entity { entity: EntityState },
    /// HA reported that a registry cache should refresh.
    RegistryInvalidated { kind: String },
    /// A service call finished.
    ActionResult { outcome: flick_core::ActionOutcome },
}

/// Service call record used by the mock server and tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceCallRecord {
    /// HA domain.
    pub domain: String,
    /// HA service.
    pub service: String,
    /// HA target object.
    #[serde(default)]
    pub target: Value,
    /// HA service_data object.
    #[serde(default)]
    pub service_data: Value,
}
