//! Finds the Home Assistant device of the computer running Flick, so local cameras default to its area.

use std::{process::Command, sync::OnceLock};

use flick_ha::DeviceInfo;

/// How the Home Assistant companion app knows this computer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostIdentity {
    /// Computer name, e.g. `Jose’s MacBook Air`.
    pub name: Option<String>,
    /// Hardware model, e.g. `Mac17,4`.
    pub model: Option<String>,
}

impl HostIdentity {
    /// Reads this computer's identity once per process.
    pub fn current() -> &'static Self {
        static HOST: OnceLock<HostIdentity> = OnceLock::new();
        HOST.get_or_init(Self::detect)
    }

    #[cfg(target_os = "macos")]
    fn detect() -> Self {
        Self {
            name: command_output("/usr/sbin/scutil", &["--get", "ComputerName"]),
            model: command_output("/usr/sbin/sysctl", &["-n", "hw.model"]),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn detect() -> Self {
        Self {
            name: std::env::var("COMPUTERNAME")
                .ok()
                .or_else(|| command_output("hostname", &[])),
            model: None,
        }
    }

    /// The HA device named like this computer, either by its integration or by the user's rename.
    /// The model only breaks ties: a same-model device under another name is someone else's.
    #[must_use]
    pub fn ha_device<'a>(&self, devices: &'a [DeviceInfo]) -> Option<&'a DeviceInfo> {
        let name = name_key(self.name.as_deref()?);
        let named = |device: &&DeviceInfo| {
            [device.name.as_deref(), device.name_by_user.as_deref()]
                .into_iter()
                .flatten()
                .any(|candidate| name_key(candidate) == name)
        };
        let same_model = |device: &&DeviceInfo| {
            matches!(
                (self.model.as_deref(), device.model.as_deref()),
                (Some(host), Some(model)) if host.eq_ignore_ascii_case(model)
            )
        };
        devices
            .iter()
            .filter(named)
            .find(same_model)
            .or_else(|| devices.iter().find(named))
    }
}

/// Case, spacing and curly apostrophes don't make two names different.
fn name_key(name: &str) -> String {
    name.split_whitespace()
        .map(|word| word.replace(['\u{2018}', '\u{2019}', '\u{02BC}'], "'"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str, name_by_user: Option<&str>, area: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            name_by_user: name_by_user.map(ToOwned::to_owned),
            area_id: Some(area.to_owned()),
            model: Some("Mac17,4".to_owned()),
        }
    }

    #[test]
    fn finds_this_mac_by_name_but_never_by_model_alone() {
        let devices = [
            device("laura", "Laura’s MacBook Air", None, "salon"),
            device("jose", "Jose's MacBook Air", Some("Estudio Mac"), "estudio"),
        ];
        let host = HostIdentity {
            name: Some("Jose’s  MacBook Air".to_owned()),
            model: Some("Mac17,4".to_owned()),
        };
        assert_eq!(
            host.ha_device(&devices).map(|d| d.id.as_str()),
            Some("jose")
        );

        let unregistered = HostIdentity {
            name: Some("Work Mac".to_owned()),
            model: Some("Mac17,4".to_owned()),
        };
        assert!(unregistered.ha_device(&devices).is_none());
    }
}
