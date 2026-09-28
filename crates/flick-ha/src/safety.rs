//! Safety classification for Home Assistant actions.

use std::collections::BTreeSet;

/// Safety class assigned to a concrete HA service target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyClass {
    /// The service cannot be mapped.
    Denied,
    /// The service requires global safety opt-in, ack and confirmation gesture.
    Sensitive,
    /// The service is allowed normally.
    Normal,
}

/// Concrete service metadata used by the validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyInput<'a> {
    /// HA domain.
    pub domain: &'a str,
    /// HA service.
    pub service: &'a str,
    /// Optional target entity domain when different from the service domain.
    pub entity_domain: Option<&'a str>,
    /// Optional HA device_class.
    pub device_class: Option<&'a str>,
}

/// Catalog-provided tightening rules.
#[derive(Debug, Clone, Default)]
pub struct SafetyCatalog {
    denied: BTreeSet<String>,
    sensitive: BTreeSet<String>,
}

impl SafetyCatalog {
    /// Adds a denied `domain.service` or `domain.*` rule.
    pub fn deny(&mut self, pattern: impl Into<String>) {
        self.denied.insert(pattern.into());
    }

    /// Adds a sensitive `domain.service` or `domain.*` rule.
    pub fn sensitive(&mut self, pattern: impl Into<String>) {
        self.sensitive.insert(pattern.into());
    }
}

/// Validator implementing `03-home-assistant-integration.md` §7.
#[derive(Debug, Clone, Default)]
pub struct SafetyValidator {
    catalog: SafetyCatalog,
}

impl SafetyValidator {
    /// Creates a validator with only the built-in rules.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a validator with a catalog that can only tighten rules.
    #[must_use]
    pub fn with_catalog(catalog: SafetyCatalog) -> Self {
        Self { catalog }
    }

    /// Classifies a concrete HA service call.
    #[must_use]
    pub fn classify(&self, input: &SafetyInput<'_>) -> SafetyClass {
        let key = format!("{}.{}", input.domain, input.service);
        if matches_pattern(&self.catalog.denied, input.domain, input.service)
            || built_in_denied(input.domain, input.service)
        {
            return SafetyClass::Denied;
        }
        if matches_pattern(&self.catalog.sensitive, input.domain, input.service)
            || built_in_sensitive(input)
        {
            return SafetyClass::Sensitive;
        }
        let _ = key;
        SafetyClass::Normal
    }
}

fn matches_pattern(patterns: &BTreeSet<String>, domain: &str, service: &str) -> bool {
    let exact = format!("{domain}.{service}");
    let wildcard = format!("{domain}.*");
    patterns.contains(&exact) || patterns.contains(&wildcard)
}

fn built_in_denied(domain: &str, service: &str) -> bool {
    match domain {
        "homeassistant" => {
            service == "restart" || service == "stop" || service.starts_with("reload")
        }
        "hassio" | "backup" | "recorder" | "system_log" => true,
        _ => false,
    }
}

fn built_in_sensitive(input: &SafetyInput<'_>) -> bool {
    match input.domain {
        "lock" | "alarm_control_panel" | "valve" | "siren" | "shell_command" | "rest_command" => {
            true
        }
        "cover" => matches!(input.device_class, Some("garage" | "door" | "gate")),
        _ => matches!(
            input.entity_domain,
            Some("lock" | "alarm_control_panel" | "valve" | "siren")
        ),
    }
}
