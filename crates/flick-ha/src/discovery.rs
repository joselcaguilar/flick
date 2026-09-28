//! Home Assistant mDNS discovery.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};

use crate::HaError;

const HA_SERVICE: &str = "_home-assistant._tcp.local.";

/// A Home Assistant instance discovered with Bonjour/mDNS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredInstance {
    /// HA location name.
    pub name: String,
    /// HTTP base URL from TXT records or host/port.
    pub base_url: String,
    /// HA UUID when advertised.
    pub uuid: Option<String>,
    /// HA version when advertised.
    pub version: Option<String>,
}

/// Browses `_home-assistant._tcp.local.` and de-duplicates by UUID.
pub async fn discover_instances(timeout: Duration) -> Result<Vec<DiscoveredInstance>, HaError> {
    tokio::task::spawn_blocking(move || discover_blocking(timeout))
        .await
        .map_err(|err| HaError::Mdns(err.to_string()))?
}

fn discover_blocking(timeout: Duration) -> Result<Vec<DiscoveredInstance>, HaError> {
    let daemon = ServiceDaemon::new().map_err(|err| HaError::Mdns(err.to_string()))?;
    let receiver = daemon
        .browse(HA_SERVICE)
        .map_err(|err| HaError::Mdns(err.to_string()))?;
    let deadline = Instant::now() + timeout;
    let mut by_key = HashMap::new();

    while Instant::now() < deadline {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let info = info.as_ref();
                let uuid = property(info, "uuid");
                let name = property(info, "location_name")
                    .unwrap_or_else(|| info.get_fullname().trim_end_matches('.').to_owned());
                let version = property(info, "version");
                let base_url = property(info, "internal_url")
                    .or_else(|| property(info, "base_url"))
                    .or_else(|| first_url(info))
                    .unwrap_or_else(|| {
                        format!(
                            "http://{}:{}",
                            info.get_hostname().trim_end_matches('.'),
                            info.get_port()
                        )
                    });
                let key = uuid.clone().unwrap_or_else(|| base_url.clone());
                by_key.insert(
                    key,
                    DiscoveredInstance {
                        name,
                        base_url,
                        uuid,
                        version,
                    },
                );
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    let mut instances: Vec<_> = by_key.into_values().collect();
    instances.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(instances)
}

fn property(info: &mdns_sd::ResolvedService, key: &str) -> Option<String> {
    info.get_property_val_str(key).map(ToOwned::to_owned)
}

fn first_url(info: &mdns_sd::ResolvedService) -> Option<String> {
    info.get_addresses()
        .iter()
        .next()
        .map(|addr| format!("http://{addr}:{}", info.get_port()))
}
