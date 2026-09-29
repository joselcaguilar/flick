//! Home Assistant integration for Flick.
//!
//! This crate owns Flick's direct Home Assistant WebSocket integration from
//! `docs/spec/03-home-assistant-integration.md`: authentication, request routing,
//! entity subscriptions, registry snapshots, action dispatch, discovery and token
//! storage. It deliberately never talks to the real Home Assistant instance in
//! tests; use [`mock::MockHa`] with the `mock` feature for deterministic protocol
//! tests in other crates.
//!
//! # Public API used by the engine/API crates
//!
//! - [`HaClient::connect`] creates a cheap-to-clone handle backed by one
//!   reconnecting WebSocket task.
//! - [`HaClient::wait_for_auth`] lets callers await the initial `auth_ok` or
//!   `auth_invalid` status before saving credentials.
//! - [`HaClient::status`] returns a `watch` receiver for `disconnected`,
//!   `connecting`, `ready` and `auth_failed` state.
//! - [`HaClient::call`] sends a concrete [`flick_core::Action::CallService`]
//!   and returns an [`flick_core::ActionOutcome`] with HA `context.id` captured.
//! - [`HaClient::subscribe_entity`] reference-counts a `subscribe_entities`
//!   subscription and publishes [`HaEvent::Entity`] updates on [`HaClient::events`].
//! - [`HaClient::refresh_registry`] and [`HaClient::registry`] maintain the cached
//!   states/services/registries snapshot used by pickers.
//! - [`HaClient::resolve_verb`] delegates targeted verb resolution to the pure
//!   [`verb`] module.
//! - [`mock::MockHa::start`] starts a reusable mock HA server and returns
//!   `(url, token, handle)` for integration tests.

pub mod client;
pub mod dial;
pub mod discovery;
pub mod error;
pub mod protocol;
pub mod registry;
pub mod safety;
pub mod secret;
pub mod tls;
pub mod types;
pub mod verb;

#[cfg(feature = "mock")]
pub mod mock;

pub use client::{EntitySubscription, HaClient};
pub use dial::{DialController, DialEvent, DialTarget};
pub use discovery::{DiscoveredInstance, discover_instances};
pub use error::{HaError, map_ha_error};
pub use protocol::{EntityDelta, parse_compressed_entities};
pub use registry::{AreaGroup, RegistryCache, RegistryEntity, RegistrySnapshot};
pub use safety::{SafetyCatalog, SafetyClass, SafetyInput, SafetyValidator};
pub use secret::{KeyringSecretStore, MemorySecretStore, SecretStore};
pub use types::{
    EntityState, HaConnectionConfig, HaEvent, HaRoute, HaStatus, ServiceCallRecord, http_base_url,
};
pub use verb::{
    FanLevelPlan, VerbResolution, VerbResolutionError, VerbTarget, next_fan_level,
    previous_fan_level, record_current_fan_level,
};
