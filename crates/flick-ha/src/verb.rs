//! Pure targeted verb resolution and fan level rules.

use flick_core::{Action, ActionTarget, DialProperty, Verb};
use serde_json::{Value, json};
use thiserror::Error;

use crate::EntityState;

const FAN_SET_SPEED: u64 = 1;
const FAN_TURN_OFF: u64 = 16;
const FAN_TURN_ON: u64 = 32;

/// Target selected by pointing and passed to targeted verb resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct VerbTarget {
    /// Entity state for the selected entity.
    pub entity: EntityState,
    /// Taught one-based fan/light/etc. levels, in ascending service-data units.
    pub levels: Vec<f64>,
}

impl VerbTarget {
    /// Creates a selected target from an entity and taught levels.
    #[must_use]
    pub fn new(entity: EntityState, levels: Vec<f64>) -> Self {
        Self { entity, levels }
    }
}

/// Metadata returned with a resolved verb.
#[derive(Debug, Clone, PartialEq)]
pub struct VerbResolution {
    /// Concrete action to dispatch.
    pub action: Action,
    /// Human-readable HUD summary.
    pub summary: String,
}

/// Fan level plan used by UI previews and tests.
#[derive(Debug, Clone, PartialEq)]
pub enum FanLevelPlan {
    /// Send a concrete percentage through `fan.turn_on`.
    Percentage(u8),
    /// Already at the top/bottom level; keep current value.
    AlreadyAtLimit,
    /// State is unavailable; send the HA relative service.
    Relative(&'static str),
    /// Below level 1; turn the fan off.
    TurnOff,
}

/// Verb resolution errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VerbResolutionError {
    /// The verb is not supported for the selected domain.
    #[error("verb unsupported for domain {0}")]
    UnsupportedDomain(String),
    /// The action needs a taught level that does not exist.
    #[error("level {level} has not been taught")]
    LevelNotTaught { level: u32 },
    /// The selected entity does not expose the HA feature bit required.
    #[error("required Home Assistant feature is missing: {0}")]
    MissingFeature(&'static str),
}

/// Resolves a core verb into a concrete Home Assistant service action.
pub fn resolve_verb(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    match target.entity.domain() {
        "fan" => resolve_fan(target, verb, level),
        "light" => resolve_light(target, verb, level),
        "media_player" => resolve_media(target, verb, level),
        "cover" => resolve_cover(target, verb, level),
        "climate" => resolve_climate(target, verb, level),
        "switch" | "input_boolean" => resolve_switch_like(target, verb),
        other => Err(VerbResolutionError::UnsupportedDomain(other.to_owned())),
    }
}

/// Records a teach-flow "Use current speed" value and returns sorted levels.
pub fn record_current_fan_level(
    mut levels: Vec<f64>,
    level: u32,
    entity: &EntityState,
) -> Result<Vec<f64>, VerbResolutionError> {
    let percentage = entity
        .attr_f64("percentage")
        .ok_or(VerbResolutionError::LevelNotTaught { level })?;
    let step = fan_step(entity);
    if levels
        .iter()
        .any(|existing| (existing - percentage).abs() <= 0.5 * step)
    {
        return Err(VerbResolutionError::LevelNotTaught { level });
    }
    let index = usize::try_from(level.saturating_sub(1))
        .map_err(|_| VerbResolutionError::LevelNotTaught { level })?;
    if levels.len() <= index {
        levels.resize(index + 1, 0.0);
    }
    levels[index] = percentage;
    levels.retain(|value| *value > 0.0);
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(levels)
}

fn resolve_fan(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    match verb {
        Verb::On => fan_on_off(target, "turn_on", FAN_TURN_ON),
        Verb::Off | Verb::Stop => fan_on_off(target, "turn_off", FAN_TURN_OFF),
        Verb::Toggle => Ok(call(&target.entity, "toggle", json!({}), "Fan → toggle")),
        Verb::LevelSet => {
            ensure_feature(target, FAN_SET_SPEED, "SET_SPEED")?;
            let level = level.unwrap_or(1);
            let percentage = level_at(target, level)?;
            Ok(call(
                &target.entity,
                "turn_on",
                json!({ "percentage": percentage }),
                format!("Fan → speed {level}"),
            ))
        }
        Verb::Up => {
            ensure_feature(target, FAN_SET_SPEED, "SET_SPEED")?;
            if target.entity.state == "off" {
                let percentage = level_at(target, 1)?;
                return Ok(call(
                    &target.entity,
                    "turn_on",
                    json!({ "percentage": percentage }),
                    "Fan → speed 1",
                ));
            }
            match next_fan_level(target) {
                FanLevelPlan::Percentage(percentage) => Ok(call(
                    &target.entity,
                    "turn_on",
                    json!({ "percentage": percentage }),
                    "Fan → speed up",
                )),
                FanLevelPlan::AlreadyAtLimit => Ok(call(
                    &target.entity,
                    "turn_on",
                    json!({ "percentage": current_percentage(target).unwrap_or(100.0).round() as u8 }),
                    "Fan → already at max",
                )),
                FanLevelPlan::Relative(service) => {
                    Ok(call(&target.entity, service, json!({}), "Fan → speed up"))
                }
                FanLevelPlan::TurnOff => {
                    Ok(call(&target.entity, "turn_off", json!({}), "Fan → off"))
                }
            }
        }
        Verb::Down => {
            ensure_feature(target, FAN_SET_SPEED, "SET_SPEED")?;
            match previous_fan_level(target) {
                FanLevelPlan::Percentage(percentage) => Ok(call(
                    &target.entity,
                    "turn_on",
                    json!({ "percentage": percentage }),
                    "Fan → speed down",
                )),
                FanLevelPlan::AlreadyAtLimit | FanLevelPlan::TurnOff => {
                    Ok(call(&target.entity, "turn_off", json!({}), "Fan → off"))
                }
                FanLevelPlan::Relative(service) => {
                    Ok(call(&target.entity, service, json!({}), "Fan → speed down"))
                }
            }
        }
    }
}

fn resolve_light(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    Ok(match verb {
        Verb::Up => {
            if target.entity.state == "off" {
                call(&target.entity, "turn_on", json!({}), "Light → on")
            } else {
                call(
                    &target.entity,
                    "turn_on",
                    json!({ "brightness_step_pct": 20 }),
                    "Light → brighter",
                )
            }
        }
        Verb::Down => call(
            &target.entity,
            "turn_on",
            json!({ "brightness_step_pct": -20 }),
            "Light → dimmer",
        ),
        Verb::On => call(&target.entity, "turn_on", json!({}), "Light → on"),
        Verb::Off | Verb::Stop => call(&target.entity, "turn_off", json!({}), "Light → off"),
        Verb::Toggle => call(&target.entity, "toggle", json!({}), "Light → toggle"),
        Verb::LevelSet => {
            let value = level_value(target, level.unwrap_or(1))?;
            call(
                &target.entity,
                "turn_on",
                json!({ "brightness_pct": value }),
                "Light → level",
            )
        }
    })
}

fn resolve_media(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    Ok(match verb {
        Verb::Up => call(&target.entity, "volume_up", json!({}), "Media → volume up"),
        Verb::Down => call(
            &target.entity,
            "volume_down",
            json!({}),
            "Media → volume down",
        ),
        Verb::On => call(&target.entity, "media_play", json!({}), "Media → play"),
        Verb::Off | Verb::Stop => call(&target.entity, "media_pause", json!({}), "Media → pause"),
        Verb::Toggle => call(
            &target.entity,
            "media_play_pause",
            json!({}),
            "Media → play/pause",
        ),
        Verb::LevelSet => {
            let value = f64::from(level_value(target, level.unwrap_or(1))?) / 100.0;
            call(
                &target.entity,
                "volume_set",
                json!({ "volume_level": value }),
                "Media → volume",
            )
        }
    })
}

fn resolve_cover(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    Ok(match verb {
        Verb::Up | Verb::On => call(&target.entity, "open_cover", json!({}), "Cover → open"),
        Verb::Down | Verb::Off => call(&target.entity, "close_cover", json!({}), "Cover → close"),
        Verb::Stop => call(&target.entity, "stop_cover", json!({}), "Cover → stop"),
        Verb::Toggle => call(&target.entity, "toggle", json!({}), "Cover → toggle"),
        Verb::LevelSet => {
            let value = level_value(target, level.unwrap_or(1))?;
            call(
                &target.entity,
                "set_cover_position",
                json!({ "position": value }),
                "Cover → position",
            )
        }
    })
}

fn resolve_climate(
    target: &VerbTarget,
    verb: Verb,
    level: Option<u32>,
) -> Result<VerbResolution, VerbResolutionError> {
    let step = target.entity.attr_f64("target_temp_step").unwrap_or(0.5);
    let current = target.entity.attr_f64("temperature").unwrap_or(20.0);
    Ok(match verb {
        Verb::Up => call(
            &target.entity,
            "set_temperature",
            json!({ "temperature": current + step }),
            "Climate → warmer",
        ),
        Verb::Down => call(
            &target.entity,
            "set_temperature",
            json!({ "temperature": current - step }),
            "Climate → cooler",
        ),
        Verb::On => call(&target.entity, "turn_on", json!({}), "Climate → on"),
        Verb::Off | Verb::Stop => call(&target.entity, "turn_off", json!({}), "Climate → off"),
        Verb::Toggle => {
            return Err(VerbResolutionError::UnsupportedDomain(
                "climate.toggle".to_owned(),
            ));
        }
        Verb::LevelSet => {
            let value = level_value(target, level.unwrap_or(1))?;
            call(
                &target.entity,
                "set_temperature",
                json!({ "temperature": value }),
                "Climate → temperature",
            )
        }
    })
}

fn resolve_switch_like(
    target: &VerbTarget,
    verb: Verb,
) -> Result<VerbResolution, VerbResolutionError> {
    Ok(match verb {
        Verb::Up | Verb::On => call(&target.entity, "turn_on", json!({}), "Switch → on"),
        Verb::Down | Verb::Off | Verb::Stop => {
            call(&target.entity, "turn_off", json!({}), "Switch → off")
        }
        Verb::Toggle => call(&target.entity, "toggle", json!({}), "Switch → toggle"),
        Verb::LevelSet => {
            return Err(VerbResolutionError::UnsupportedDomain(
                target.entity.domain().to_owned(),
            ));
        }
    })
}

/// Resolves `$selected` dial actions to the selected entity id.
#[must_use]
pub fn resolve_selected_dial(
    target: &VerbTarget,
    property: DialProperty,
    gain: f64,
    min: Option<Value>,
    max: Option<Value>,
) -> Action {
    Action::Dial {
        entity_id: target.entity.entity_id.clone(),
        property,
        gain,
        min,
        max,
    }
}

/// Computes default/taught fan levels.
#[must_use]
pub fn fan_levels(target: &VerbTarget) -> Vec<f64> {
    if !target.levels.is_empty() {
        return target.levels.clone();
    }
    let step = fan_step(&target.entity);
    let n = (100.0 / step).round();
    if n <= 10.0 {
        let count = n.max(1.0) as u32;
        (1..=count)
            .map(|idx| {
                if idx == count {
                    100.0
                } else {
                    (f64::from(idx) * step).round()
                }
            })
            .collect()
    } else if let Some(percentage) = target
        .entity
        .attr_f64("percentage")
        .filter(|_| target.entity.state != "off")
    {
        vec![percentage.max(step).max(1.0)]
    } else {
        vec![step.max(1.0)]
    }
}

/// Computes the next fan level according to §6.3.
#[must_use]
pub fn next_fan_level(target: &VerbTarget) -> FanLevelPlan {
    let Some(current) = current_percentage(target) else {
        return FanLevelPlan::Relative("increase_speed");
    };
    let step = fan_step(&target.entity);
    let levels = fan_levels(target);
    for level in &levels {
        if *level > current + 0.5 * step {
            return FanLevelPlan::Percentage(clamp_pct(*level));
        }
    }
    FanLevelPlan::AlreadyAtLimit
}

/// Computes the previous fan level according to §6.3.
#[must_use]
pub fn previous_fan_level(target: &VerbTarget) -> FanLevelPlan {
    let Some(current) = current_percentage(target) else {
        return FanLevelPlan::Relative("decrease_speed");
    };
    let step = fan_step(&target.entity);
    let levels = fan_levels(target);
    let mut previous = None;
    for level in &levels {
        if *level < current - 0.5 * step {
            previous = Some(*level);
        }
    }
    previous.map_or(FanLevelPlan::TurnOff, |value| {
        FanLevelPlan::Percentage(clamp_pct(value))
    })
}

fn fan_on_off(
    target: &VerbTarget,
    service: &'static str,
    feature: u64,
) -> Result<VerbResolution, VerbResolutionError> {
    if target.entity.supported_features() & feature == 0 {
        return Ok(call(&target.entity, "toggle", json!({}), "Fan → toggle"));
    }
    Ok(call(
        &target.entity,
        service,
        json!({}),
        if service == "turn_on" {
            "Fan → on"
        } else {
            "Fan → off"
        },
    ))
}

fn ensure_feature(
    target: &VerbTarget,
    bit: u64,
    name: &'static str,
) -> Result<(), VerbResolutionError> {
    if target.entity.supported_features() & bit == 0 {
        Err(VerbResolutionError::MissingFeature(name))
    } else {
        Ok(())
    }
}

fn level_at(target: &VerbTarget, level: u32) -> Result<u8, VerbResolutionError> {
    let levels = fan_levels(target);
    let index = usize::try_from(level.saturating_sub(1))
        .map_err(|_| VerbResolutionError::LevelNotTaught { level })?;
    levels
        .get(index)
        .copied()
        .map(clamp_pct)
        .ok_or(VerbResolutionError::LevelNotTaught { level })
}

fn level_value(target: &VerbTarget, level: u32) -> Result<u8, VerbResolutionError> {
    let index = usize::try_from(level.saturating_sub(1))
        .map_err(|_| VerbResolutionError::LevelNotTaught { level })?;
    target
        .levels
        .get(index)
        .copied()
        .map(clamp_pct)
        .ok_or(VerbResolutionError::LevelNotTaught { level })
}

fn current_percentage(target: &VerbTarget) -> Option<f64> {
    if target.entity.state == "unknown" || target.entity.state == "unavailable" {
        None
    } else {
        target.entity.attr_f64("percentage")
    }
}

fn fan_step(entity: &EntityState) -> f64 {
    entity
        .attr_f64("percentage_step")
        .filter(|step| *step > 0.0)
        .unwrap_or(1.0)
}

fn clamp_pct(value: f64) -> u8 {
    value.round().clamp(1.0, 100.0) as u8
}

fn call(
    entity: &EntityState,
    service: &str,
    data: Value,
    summary: impl Into<String>,
) -> VerbResolution {
    VerbResolution {
        action: Action::CallService {
            domain: entity.domain().to_owned(),
            service: service.to_owned(),
            target: ActionTarget {
                entity_id: Some(vec![entity.entity_id.clone()]),
                device_id: None,
                area_id: None,
            },
            data,
            preset: None,
        },
        summary: summary.into(),
    }
}
