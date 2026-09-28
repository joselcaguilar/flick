use flick_core::{Action, ActionTarget, DialProperty, Verb};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Anchor, TeachTarget};

const FAN_SET_SPEED: u64 = 1;
const FAN_TURN_OFF: u64 = 16;
const FAN_TURN_ON: u64 = 32;

/// Per-anchor verb parameters stored in `anchors.verb_params`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerbParams {
    /// Taught percentage or brightness levels, ascending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<u8>,
}

/// Entity state and attributes needed to resolve targeted verbs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetEntityState {
    /// Home Assistant entity id.
    pub entity_id: String,
    /// Home Assistant state string, when subscribed.
    pub state: Option<String>,
    /// Numeric supported feature bitset from HA attributes.
    pub supported_features: Option<u64>,
    /// Fan/light current percentage-like value.
    pub percentage: Option<f32>,
    /// Fan `percentage_step` attribute.
    pub percentage_step: Option<f32>,
    /// Current light brightness percent.
    pub brightness_pct: Option<f32>,
    /// Climate target temperature.
    pub temperature: Option<f32>,
    /// Climate target temperature step.
    pub target_temp_step: Option<f32>,
}

/// Failure while resolving a targeted verb into a concrete action.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum VerbResolutionError {
    /// The selected target cannot produce an entity service target for this verb.
    #[error("targeted verb requires an entity target")]
    RequiresEntityTarget,
    /// The requested taught level is missing.
    #[error("level {level} is not taught")]
    LevelNotTaught {
        /// One-based level requested by the mapping.
        level: u32,
    },
    /// The domain does not define the requested verb.
    #[error("verb {verb:?} is unsupported for domain {domain}")]
    UnsupportedVerb {
        /// Selected anchor domain.
        domain: String,
        /// Requested verb.
        verb: Verb,
    },
    /// The entity lacks the needed Home Assistant feature bit.
    #[error("entity is missing required feature bit {feature}")]
    MissingFeature {
        /// Required feature bit.
        feature: u64,
    },
}

/// Resolves a targeted [`Verb`] against the selected anchor and optional HA state.
pub fn resolve_verb(
    anchor: &Anchor,
    verb: Verb,
    level: Option<u32>,
    entity_state: Option<&TargetEntityState>,
) -> Result<Action, VerbResolutionError> {
    match anchor.domain.as_str() {
        "fan" => resolve_fan(anchor, verb, level, entity_state),
        "light" => resolve_light(anchor, verb, level),
        "media_player" => resolve_media_player(anchor, verb),
        "cover" => resolve_cover(anchor, verb, level),
        "climate" => resolve_climate(anchor, verb, level, entity_state),
        "switch" | "input_boolean" => resolve_switch_like(anchor, verb),
        domain => Err(VerbResolutionError::UnsupportedVerb {
            domain: domain.to_owned(),
            verb,
        }),
    }
}

/// Resolves a targeted [`Action::Verb`] and leaves concrete actions unchanged.
pub fn resolve_targeted_action(
    anchor: &Anchor,
    action: &Action,
    entity_state: Option<&TargetEntityState>,
) -> Result<Action, VerbResolutionError> {
    match action {
        Action::Verb { verb, level } => resolve_verb(anchor, *verb, *level, entity_state),
        Action::Dial {
            entity_id,
            property,
            gain,
            min,
            max,
        } if entity_id == "$selected" => Ok(Action::Dial {
            entity_id: selected_entity_id(anchor)?.to_owned(),
            property: *property,
            gain: *gain,
            min: min.clone(),
            max: max.clone(),
        }),
        other => Ok(other.clone()),
    }
}

fn resolve_fan(
    anchor: &Anchor,
    verb: Verb,
    level: Option<u32>,
    entity_state: Option<&TargetEntityState>,
) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    match verb {
        Verb::LevelSet => fan_level_action(anchor, level.unwrap_or(1), entity_state),
        Verb::Up => {
            ensure_fan_feature(entity_state, FAN_SET_SPEED)?;
            if entity_state.is_some_and(is_off) {
                return fan_level_action(anchor, 1, entity_state);
            }
            let Some(state) = entity_state else {
                return Ok(call_service("fan", "increase_speed", target, json!({})));
            };
            let levels = fan_levels(anchor, Some(state));
            let Some(current) = state.percentage else {
                return Ok(call_service("fan", "increase_speed", target, json!({})));
            };
            let step = fan_step(state);
            let next = levels
                .iter()
                .copied()
                .find(|level| f32::from(*level) > current + 0.5 * step)
                .or_else(|| levels.last().copied())
                .unwrap_or_else(|| fallback_level(state));
            Ok(call_service(
                "fan",
                "turn_on",
                target,
                json!({ "percentage": next }),
            ))
        }
        Verb::Down => {
            ensure_fan_feature(entity_state, FAN_SET_SPEED)?;
            let Some(state) = entity_state else {
                return Ok(call_service("fan", "decrease_speed", target, json!({})));
            };
            let Some(current) = state.percentage else {
                return Ok(call_service("fan", "decrease_speed", target, json!({})));
            };
            let step = fan_step(state);
            let levels = fan_levels(anchor, Some(state));
            if let Some(previous) = levels
                .iter()
                .rev()
                .copied()
                .find(|level| f32::from(*level) < current - 0.5 * step)
            {
                Ok(call_service(
                    "fan",
                    "turn_on",
                    target,
                    json!({ "percentage": previous }),
                ))
            } else {
                ensure_fan_feature(entity_state, FAN_TURN_OFF)?;
                Ok(call_service("fan", "turn_off", target, json!({})))
            }
        }
        Verb::On => {
            ensure_fan_feature(entity_state, FAN_TURN_ON)?;
            Ok(call_service("fan", "turn_on", target, json!({})))
        }
        Verb::Off | Verb::Stop => {
            ensure_fan_feature(entity_state, FAN_TURN_OFF)?;
            Ok(call_service("fan", "turn_off", target, json!({})))
        }
        Verb::Toggle => Ok(call_service("fan", "toggle", target, json!({}))),
    }
}

fn resolve_light(
    anchor: &Anchor,
    verb: Verb,
    level: Option<u32>,
) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    match verb {
        Verb::Up => Ok(call_service(
            "light",
            "turn_on",
            target,
            json!({ "brightness_step_pct": 20 }),
        )),
        Verb::Down => Ok(call_service(
            "light",
            "turn_on",
            target,
            json!({ "brightness_step_pct": -20 }),
        )),
        Verb::On => Ok(call_service("light", "turn_on", target, json!({}))),
        Verb::Off | Verb::Stop => Ok(call_service("light", "turn_off", target, json!({}))),
        Verb::Toggle => Ok(call_service("light", "toggle", target, json!({}))),
        Verb::LevelSet => {
            let value = level_value(anchor, level.unwrap_or(1))?;
            Ok(call_service(
                "light",
                "turn_on",
                target,
                json!({ "brightness_pct": value }),
            ))
        }
    }
}

fn resolve_media_player(anchor: &Anchor, verb: Verb) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    match verb {
        Verb::Up => Ok(call_service("media_player", "volume_up", target, json!({}))),
        Verb::Down => Ok(call_service(
            "media_player",
            "volume_down",
            target,
            json!({}),
        )),
        Verb::On => Ok(call_service(
            "media_player",
            "media_play",
            target,
            json!({}),
        )),
        Verb::Off | Verb::Stop => Ok(call_service(
            "media_player",
            "media_pause",
            target,
            json!({}),
        )),
        Verb::Toggle => Ok(call_service(
            "media_player",
            "media_play_pause",
            target,
            json!({}),
        )),
        Verb::LevelSet => {
            let value = f64::from(level_value(anchor, 1)?) / 100.0;
            Ok(call_service(
                "media_player",
                "volume_set",
                target,
                json!({ "volume_level": value }),
            ))
        }
    }
}

fn resolve_cover(
    anchor: &Anchor,
    verb: Verb,
    level: Option<u32>,
) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    match verb {
        Verb::Up | Verb::On => Ok(call_service("cover", "open_cover", target, json!({}))),
        Verb::Down | Verb::Off => Ok(call_service("cover", "close_cover", target, json!({}))),
        Verb::Stop => Ok(call_service("cover", "stop_cover", target, json!({}))),
        Verb::Toggle => Ok(call_service("cover", "toggle", target, json!({}))),
        Verb::LevelSet => {
            let value = level_value(anchor, level.unwrap_or(1))?;
            Ok(call_service(
                "cover",
                "set_cover_position",
                target,
                json!({ "position": value }),
            ))
        }
    }
}

fn resolve_climate(
    anchor: &Anchor,
    verb: Verb,
    level: Option<u32>,
    entity_state: Option<&TargetEntityState>,
) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    match verb {
        Verb::Up | Verb::Down => {
            let current = entity_state
                .and_then(|state| state.temperature)
                .unwrap_or(20.0);
            let step = entity_state
                .and_then(|state| state.target_temp_step)
                .unwrap_or(0.5);
            let temperature = if verb == Verb::Up {
                current + step
            } else {
                current - step
            };
            Ok(call_service(
                "climate",
                "set_temperature",
                target,
                json!({ "temperature": temperature }),
            ))
        }
        Verb::On => Ok(call_service("climate", "turn_on", target, json!({}))),
        Verb::Off | Verb::Stop => Ok(call_service("climate", "turn_off", target, json!({}))),
        Verb::LevelSet => {
            let value = level_value(anchor, level.unwrap_or(1))?;
            Ok(call_service(
                "climate",
                "set_temperature",
                target,
                json!({ "temperature": value }),
            ))
        }
        Verb::Toggle => Err(VerbResolutionError::UnsupportedVerb {
            domain: anchor.domain.clone(),
            verb,
        }),
    }
}

fn resolve_switch_like(anchor: &Anchor, verb: Verb) -> Result<Action, VerbResolutionError> {
    let target = action_target(&anchor.target);
    let domain = anchor.domain.as_str();
    match verb {
        Verb::Up | Verb::On => Ok(call_service(domain, "turn_on", target, json!({}))),
        Verb::Down | Verb::Off | Verb::Stop => {
            Ok(call_service(domain, "turn_off", target, json!({})))
        }
        Verb::Toggle => Ok(call_service(domain, "toggle", target, json!({}))),
        Verb::LevelSet => Err(VerbResolutionError::UnsupportedVerb {
            domain: anchor.domain.clone(),
            verb,
        }),
    }
}

fn fan_level_action(
    anchor: &Anchor,
    requested_level: u32,
    entity_state: Option<&TargetEntityState>,
) -> Result<Action, VerbResolutionError> {
    ensure_fan_feature(entity_state, FAN_SET_SPEED)?;
    let target = action_target(&anchor.target);
    let levels = fan_levels(anchor, entity_state);
    let Some(percentage) = requested_level
        .checked_sub(1)
        .and_then(|idx| levels.get(idx as usize))
        .copied()
    else {
        return Err(VerbResolutionError::LevelNotTaught {
            level: requested_level,
        });
    };
    Ok(call_service(
        "fan",
        "turn_on",
        target,
        json!({ "percentage": percentage }),
    ))
}

fn fan_levels(anchor: &Anchor, entity_state: Option<&TargetEntityState>) -> Vec<u8> {
    if !anchor.verb_params.levels.is_empty() {
        return normalized_levels(anchor.verb_params.levels.clone());
    }
    let Some(state) = entity_state else {
        return vec![1];
    };
    let step = fan_step(state);
    let n = (100.0 / step).round().clamp(1.0, 100.0) as u32;
    if n <= 10 {
        let mut levels = (1..=n)
            .map(|idx| ((idx as f32) * step).round().clamp(1.0, 100.0) as u8)
            .collect::<Vec<_>>();
        if let Some(last) = levels.last_mut() {
            *last = 100;
        }
        normalized_levels(levels)
    } else {
        vec![fallback_level(state)]
    }
}

fn fallback_level(state: &TargetEntityState) -> u8 {
    state
        .percentage
        .filter(|value| *value > 0.0 && !is_off(state))
        .unwrap_or_else(|| fan_step(state).max(1.0))
        .round()
        .clamp(1.0, 100.0) as u8
}

fn normalized_levels(mut levels: Vec<u8>) -> Vec<u8> {
    levels.retain(|level| (1..=100).contains(level));
    levels.sort_unstable();
    levels.dedup();
    levels
}

fn level_value(anchor: &Anchor, requested_level: u32) -> Result<u8, VerbResolutionError> {
    requested_level
        .checked_sub(1)
        .and_then(|idx| anchor.verb_params.levels.get(idx as usize))
        .copied()
        .ok_or(VerbResolutionError::LevelNotTaught {
            level: requested_level,
        })
}

fn ensure_fan_feature(
    entity_state: Option<&TargetEntityState>,
    feature: u64,
) -> Result<(), VerbResolutionError> {
    if entity_state
        .and_then(|state| state.supported_features)
        .is_some_and(|features| features & feature == 0)
    {
        return Err(VerbResolutionError::MissingFeature { feature });
    }
    Ok(())
}

fn fan_step(state: &TargetEntityState) -> f32 {
    state.percentage_step.unwrap_or(1.0).clamp(1.0, 100.0)
}

fn is_off(state: &TargetEntityState) -> bool {
    state
        .state
        .as_deref()
        .is_some_and(|state| state == "off" || state == "unavailable")
}

fn selected_entity_id(anchor: &Anchor) -> Result<&str, VerbResolutionError> {
    match &anchor.target {
        TeachTarget::Entity(entity_id) => Ok(entity_id),
        TeachTarget::Device(_) | TeachTarget::Area(_) => {
            Err(VerbResolutionError::RequiresEntityTarget)
        }
    }
}

fn action_target(target: &TeachTarget) -> ActionTarget {
    match target {
        TeachTarget::Entity(entity_id) => ActionTarget {
            entity_id: Some(vec![entity_id.clone()]),
            device_id: None,
            area_id: None,
        },
        TeachTarget::Device(device_id) => ActionTarget {
            entity_id: None,
            device_id: Some(vec![device_id.clone()]),
            area_id: None,
        },
        TeachTarget::Area(area_id) => ActionTarget {
            entity_id: None,
            device_id: None,
            area_id: Some(vec![area_id.clone()]),
        },
    }
}

fn call_service(domain: &str, service: &str, target: ActionTarget, data: Value) -> Action {
    Action::CallService {
        domain: domain.to_owned(),
        service: service.to_owned(),
        target,
        data,
        preset: None,
    }
}

/// Returns the default dial property for a target domain.
#[must_use]
pub fn default_dial_property(domain: &str) -> Option<DialProperty> {
    match domain {
        "fan" => Some(DialProperty::Percentage),
        "light" => Some(DialProperty::BrightnessPct),
        "media_player" => Some(DialProperty::VolumeLevel),
        "cover" => Some(DialProperty::Position),
        "climate" => Some(DialProperty::Temperature),
        _ => None,
    }
}
