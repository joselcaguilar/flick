//! Registry snapshots and picker filtering.

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::EntityState;

/// Entity row shown in HA pickers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryEntity {
    /// Entity id.
    pub entity_id: String,
    /// Friendly name.
    pub name: String,
    /// Domain prefix.
    pub domain: String,
    /// Area id from entity or device registry.
    pub area_id: Option<String>,
    /// Device id from entity registry.
    pub device_id: Option<String>,
    /// Current HA state.
    pub state: String,
    /// HA device class.
    pub device_class: Option<String>,
    /// Supported features bitset.
    pub supported_features: u64,
}

/// HA area metadata used for grouping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaInfo {
    /// Area id.
    pub area_id: String,
    /// Display name.
    pub name: String,
    /// Optional floor id.
    pub floor_id: Option<String>,
}

/// HA device metadata, used to find the HA device of the computer running Flick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device id.
    pub id: String,
    /// Name reported by the integration.
    pub name: Option<String>,
    /// The user's rename in HA.
    pub name_by_user: Option<String>,
    /// Area id.
    pub area_id: Option<String>,
    /// Model.
    pub model: Option<String>,
}

impl DeviceInfo {
    /// Name shown in HA: the user's rename, else the integration's name.
    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.name_by_user.as_deref().or(self.name.as_deref())
    }
}

/// A picker group, including the synthetic `No area` group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaGroup {
    /// Area id, or `None` for `No area`.
    pub area_id: Option<String>,
    /// Display name.
    pub name: String,
    /// Entities in the group.
    pub entities: Vec<RegistryEntity>,
}

/// Persistable HA registry/cache snapshot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    /// Entities merged from `get_states` and registries.
    pub entities: Vec<RegistryEntity>,
    /// Raw `get_services` response.
    #[serde(default)]
    pub services: Value,
    /// Areas for grouping.
    pub areas: Vec<AreaInfo>,
    /// Device registry metadata.
    #[serde(default)]
    pub devices: Vec<DeviceInfo>,
    /// Snapshot timestamp in Unix epoch milliseconds.
    pub fetched_at_ms: u64,
}

impl RegistrySnapshot {
    /// Filters entities in memory and groups them by area, with `No area` last.
    #[must_use]
    pub fn filter(
        &self,
        domain: Option<&str>,
        area_id: Option<&str>,
        q: Option<&str>,
    ) -> Vec<AreaGroup> {
        let query = q.map(str::to_lowercase);
        let mut entities: Vec<RegistryEntity> = self
            .entities
            .iter()
            .filter(|entity| domain.is_none_or(|domain| entity.domain == domain))
            .filter(|entity| area_id.is_none_or(|area| entity.area_id.as_deref() == Some(area)))
            .filter(|entity| {
                query.as_ref().is_none_or(|q| {
                    entity.name.to_lowercase().contains(q)
                        || entity.entity_id.to_lowercase().contains(q)
                })
            })
            .cloned()
            .collect();
        entities.sort_by_key(|entity| entity.name.to_lowercase());

        let mut area_names: HashMap<Option<String>, String> = self
            .areas
            .iter()
            .map(|area| (Some(area.area_id.clone()), area.name.clone()))
            .collect();
        area_names.insert(None, "No area".to_owned());

        let mut grouped: HashMap<Option<String>, Vec<RegistryEntity>> = HashMap::new();
        for entity in entities {
            grouped
                .entry(entity.area_id.clone())
                .or_default()
                .push(entity);
        }

        let mut groups: Vec<AreaGroup> = grouped
            .into_iter()
            .map(|(area_id, entities)| AreaGroup {
                name: area_names
                    .get(&area_id)
                    .cloned()
                    .unwrap_or_else(|| area_id.clone().unwrap_or_else(|| "No area".to_owned())),
                area_id,
                entities,
            })
            .collect();
        groups.sort_by(|a, b| match (&a.area_id, &b.area_id) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(_), Some(_)) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        groups
    }
}

/// Mutable in-memory registry cache.
#[derive(Debug, Clone, Default)]
pub struct RegistryCache {
    snapshot: RegistrySnapshot,
}

impl RegistryCache {
    /// Replaces the snapshot.
    pub fn replace(&mut self, snapshot: RegistrySnapshot) {
        self.snapshot = snapshot;
    }

    /// Returns the current persistable snapshot.
    #[must_use]
    pub fn snapshot(&self) -> RegistrySnapshot {
        self.snapshot.clone()
    }
}

/// Builds a picker snapshot from HA protocol responses.
#[must_use]
pub fn build_snapshot(
    states: Vec<EntityState>,
    services: Value,
    areas_raw: Value,
    devices_raw: Value,
    entity_registry_raw: Value,
) -> RegistrySnapshot {
    let areas = parse_areas(&areas_raw);
    let devices = parse_devices(&devices_raw);
    let device_areas: HashMap<String, String> = devices
        .iter()
        .filter_map(|device| Some((device.id.clone(), device.area_id.clone()?)))
        .collect();
    let entity_meta = parse_entity_display(&entity_registry_raw);

    let entities = states
        .into_iter()
        .map(|state| {
            let meta = entity_meta.get(&state.entity_id);
            let device_id = meta.and_then(|meta| meta.device_id.clone());
            let area_id = meta.and_then(|meta| meta.area_id.clone()).or_else(|| {
                device_id
                    .as_ref()
                    .and_then(|id| device_areas.get(id).cloned())
            });
            let device_class = state.device_class().map(ToOwned::to_owned);
            let supported_features = state.supported_features();
            let domain = state.domain().to_owned();
            let name = meta
                .and_then(|meta| meta.name.clone())
                .unwrap_or_else(|| state.friendly_name());
            RegistryEntity {
                domain,
                name,
                entity_id: state.entity_id,
                area_id,
                device_id,
                state: state.state,
                device_class,
                supported_features,
            }
        })
        .collect();

    RegistrySnapshot {
        entities,
        services,
        areas,
        devices,
        fetched_at_ms: now_ms(),
    }
}

#[derive(Debug, Clone, Default)]
struct EntityMeta {
    device_id: Option<String>,
    area_id: Option<String>,
    name: Option<String>,
}

fn parse_areas(value: &Value) -> Vec<AreaInfo> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|area| {
            let area_id = area
                .get("area_id")
                .or_else(|| area.get("id"))
                .and_then(Value::as_str)?;
            let name = area.get("name").and_then(Value::as_str).unwrap_or(area_id);
            let floor_id = area
                .get("floor_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            Some(AreaInfo {
                area_id: area_id.to_owned(),
                name: name.to_owned(),
                floor_id,
            })
        })
        .collect()
}

fn parse_devices(value: &Value) -> Vec<DeviceInfo> {
    let text = |device: &Value, key: &str| {
        device
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    };
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|device| {
            let id = text(device, "id").or_else(|| text(device, "di"))?;
            Some(DeviceInfo {
                id,
                name: text(device, "name"),
                name_by_user: text(device, "name_by_user"),
                area_id: text(device, "area_id").or_else(|| text(device, "ai")),
                model: text(device, "model"),
            })
        })
        .collect()
}

fn parse_entity_display(value: &Value) -> HashMap<String, EntityMeta> {
    let array = value
        .get("entities")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let mut result = HashMap::new();
    if let Some(array) = array {
        for item in array {
            let Some(entity_id) = item
                .get("entity_id")
                .or_else(|| item.get("ei"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let meta = EntityMeta {
                device_id: item
                    .get("device_id")
                    .or_else(|| item.get("di"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                area_id: item
                    .get("area_id")
                    .or_else(|| item.get("ai"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                name: item
                    .get("name")
                    .or_else(|| item.get("en"))
                    .or_else(|| item.get("name_by_user"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            };
            result.insert(entity_id.to_owned(), meta);
        }
    }
    result
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
