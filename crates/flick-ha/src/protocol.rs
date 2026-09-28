//! Home Assistant protocol helpers.

use std::collections::HashMap;

use serde_json::Value;

use crate::EntityState;

/// Entity subscription delta parsed from HA's compressed `a`/`c`/`r` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityDelta {
    /// Entity was added or fully replaced.
    Added(EntityState),
    /// Entity changed.
    Changed(EntityState),
    /// Entity was removed from the subscription stream.
    Removed(String),
}

/// Parses one HA `subscribe_entities` compressed event into the supplied state map.
///
/// The parser accepts both raw event payloads (`{"a": ...}`) and full HA
/// messages (`{"type":"event","event":{"a": ...}}`). It supports:
/// `a` adds, `c` changes with `+` merges and `-` attribute removals, and `r`
/// removals.
#[must_use]
pub fn parse_compressed_entities(
    message: &Value,
    states: &mut HashMap<String, EntityState>,
) -> Vec<EntityDelta> {
    let payload = message
        .get("event")
        .and_then(|event| event.get("event").or(Some(event)))
        .unwrap_or(message);

    let mut deltas = Vec::new();

    if let Some(adds) = payload.get("a").and_then(Value::as_object) {
        for (entity_id, value) in adds {
            if let Some(entity) = compressed_state(entity_id, value, states.get(entity_id)) {
                states.insert(entity_id.clone(), entity.clone());
                deltas.push(EntityDelta::Added(entity));
            }
        }
    }

    if let Some(changes) = payload.get("c").and_then(Value::as_object) {
        for (entity_id, value) in changes {
            let Some(existing) = states.get(entity_id).cloned() else {
                if let Some(entity) = compressed_state(entity_id, value, None) {
                    states.insert(entity_id.clone(), entity.clone());
                    deltas.push(EntityDelta::Changed(entity));
                }
                continue;
            };
            let mut entity = existing;
            if let Some(plus) = value.get("+") {
                merge_compressed(&mut entity, plus);
            } else {
                merge_compressed(&mut entity, value);
            }
            if let Some(remove) = value.get("-").and_then(Value::as_array) {
                for key in remove.iter().filter_map(Value::as_str) {
                    entity.attributes.remove(key);
                }
            }
            states.insert(entity_id.clone(), entity.clone());
            deltas.push(EntityDelta::Changed(entity));
        }
    }

    if let Some(removes) = payload.get("r").and_then(Value::as_array) {
        for entity_id in removes.iter().filter_map(Value::as_str) {
            states.remove(entity_id);
            deltas.push(EntityDelta::Removed(entity_id.to_owned()));
        }
    }

    deltas
}

fn compressed_state(
    entity_id: &str,
    value: &Value,
    previous: Option<&EntityState>,
) -> Option<EntityState> {
    if value.get("entity_id").is_some() && value.get("state").is_some() {
        return serde_json::from_value(value.clone()).ok();
    }
    let mut entity = previous.cloned().unwrap_or_else(|| EntityState {
        entity_id: entity_id.to_owned(),
        state: "unknown".to_owned(),
        attributes: serde_json::Map::new(),
        last_changed: None,
        last_updated: None,
    });
    merge_compressed(&mut entity, value);
    Some(entity)
}

fn merge_compressed(entity: &mut EntityState, value: &Value) {
    if let Some(state) = value.get("s").and_then(Value::as_str) {
        entity.state = state.to_owned();
    }
    if let Some(attrs) = value.get("a").and_then(Value::as_object) {
        for (key, value) in attrs {
            entity.attributes.insert(key.clone(), value.clone());
        }
    }
    if let Some(last_changed) = value.get("lc").and_then(Value::as_str) {
        entity.last_changed = Some(last_changed.to_owned());
    }
    if let Some(last_updated) = value.get("lu").and_then(Value::as_str) {
        entity.last_updated = Some(last_updated.to_owned());
    }
}

/// Extracts a HA result context id from a successful result object.
#[must_use]
pub fn context_id(result: &Value) -> Option<String> {
    result
        .get("context")
        .and_then(|ctx| ctx.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// Returns a redacted HA error tuple from a failed result message.
#[must_use]
pub fn result_error(value: &Value) -> (Option<&str>, Option<&str>) {
    let error = value.get("error");
    let code = error
        .and_then(|err| err.get("code"))
        .and_then(Value::as_str);
    let message = error
        .and_then(|err| err.get("message"))
        .and_then(Value::as_str);
    (code, message)
}
