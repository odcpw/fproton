//! fproton's own settings (`~/.config/fproton/config.toml`) and the Proton
//! config-key whitelist (DESIGN.md §4 `config`).
//!
//! The whitelist is the single table both `fproton config set` and the plugin's
//! toggles validate against; a test asserts every pair appears in the captured
//! `protonvpn config set <K> --help` fixtures (Phase 0/1b).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::paths::Paths;

/// External-process deadlines in seconds (§2). `--timeout <s>` overrides all of them.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Timeouts {
    /// Any `protonvpn` probe (`info`, `config list`, `signout`).
    pub probe_sec: u64,
    /// `protonvpn connect`.
    pub connect_sec: u64,
    /// `protonvpn disconnect`.
    pub disconnect_sec: u64,
    /// `protonvpn config set`.
    pub config_set_sec: u64,
    /// Any `nmcli` call.
    pub nmcli_sec: u64,
    /// Any `busctl` call.
    pub busctl_sec: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            probe_sec: 15,
            connect_sec: 90,
            disconnect_sec: 25,
            config_set_sec: 20,
            nmcli_sec: 10,
            busctl_sec: 1,
        }
    }
}

impl Timeouts {
    /// Clamp every deadline to `secs` (the global `--timeout` flag).
    pub fn override_all(&mut self, secs: u64) {
        let s = secs.max(1);
        self.probe_sec = s;
        self.connect_sec = s;
        self.disconnect_sec = s;
        self.config_set_sec = s;
        self.nmcli_sec = s;
        self.busctl_sec = s;
    }
    pub fn probe(&self) -> Duration {
        Duration::from_secs(self.probe_sec)
    }
    pub fn connect(&self) -> Duration {
        Duration::from_secs(self.connect_sec)
    }
    pub fn disconnect(&self) -> Duration {
        Duration::from_secs(self.disconnect_sec)
    }
    pub fn config_set(&self) -> Duration {
        Duration::from_secs(self.config_set_sec)
    }
    pub fn nmcli(&self) -> Duration {
        Duration::from_secs(self.nmcli_sec)
    }
    pub fn busctl(&self) -> Duration {
        Duration::from_secs(self.busctl_sec)
    }
}

/// `~/.config/fproton/config.toml`. Unknown keys are kept out by `deny_unknown_fields`
/// so `settings::validate` (doctor Class A) can report them instead of silently ignoring.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// The only engine in v0.1 is `cli`.
    pub engine: String,
    /// Target used by a bare `fproton connect` and the bar widget's toggle.
    pub default_target: String,
    /// Desktop notifications from the plugin. Off in v0.1 (§0).
    pub notify: bool,
    pub timeouts: Timeouts,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            engine: "cli".to_string(),
            default_target: "fastest".to_string(),
            notify: false,
            timeouts: Timeouts::default(),
        }
    }
}

/// Settable fproton keys, in `settings --help` order.
pub const SETTING_KEYS: &[&str] = &[
    "engine",
    "default_target",
    "notify",
    "timeouts.probe_sec",
    "timeouts.connect_sec",
    "timeouts.disconnect_sec",
    "timeouts.config_set_sec",
    "timeouts.nmcli_sec",
    "timeouts.busctl_sec",
];

/// Engines fproton knows. `wg` is an env-gated experiment, never an engine (§4).
pub const ENGINES: &[&str] = &["cli"];

/// Proton settings fproton may **write** via `protonvpn config set <K> <V>`,
/// with the exact upstream value vocabulary (verified on proton-vpn-cli 1.0.3, §4).
pub const PROTON_CONFIG_SET_KEYS: &[(&str, &[&str])] = &[
    (
        "netshield",
        &["off", "malware-only", "malware-ads-trackers"],
    ),
    ("kill-switch", &["off", "standard"]),
    ("port-forwarding", &["off", "on"]),
    ("ipv6", &["off", "on"]),
    ("vpn-accelerator", &["off", "on"]),
    ("moderate-nat", &["off", "on"]),
];

/// Every Proton setting `config list` prints, in the upstream table's row order.
/// `custom-dns` and `anonymous-crash-reports` are parsed but not settable in v0.1.
pub const PROTON_CONFIG_KEYS: &[&str] = &[
    "netshield",
    "kill-switch",
    "port-forwarding",
    "custom-dns",
    "vpn-accelerator",
    "moderate-nat",
    "ipv6",
    "anonymous-crash-reports",
];

/// Validate a `fproton config set K V` pair against the whitelist.
/// There is no `protocol` setting and no `kill-switch permanent` (§4).
pub fn validate_proton_config(key: &str, value: &str) -> Result<(), Error> {
    let Some((_, values)) = PROTON_CONFIG_SET_KEYS.iter().find(|(k, _)| *k == key) else {
        let known: Vec<&str> = PROTON_CONFIG_SET_KEYS.iter().map(|(k, _)| *k).collect();
        let readable = if PROTON_CONFIG_KEYS.contains(&key) {
            format!("'{key}' is read-only in v0.1 (it appears in `fproton config list`)")
        } else {
            format!("unknown Proton setting '{key}'")
        };
        return Err(
            Error::usage(format!("{readable}; settable: {}", known.join(", ")))
                .with_hint("fproton config list --json"),
        );
    };
    if !values.contains(&value) {
        return Err(Error::usage(format!(
            "'{value}' is not a value for '{key}'; allowed: {}",
            values.join(", ")
        ))
        .with_hint(format!("fproton config set {key} {}", values[0])));
    }
    Ok(())
}

impl Settings {
    /// Read `config.toml`, or the defaults when it does not exist.
    pub fn load(paths: &Paths) -> Result<Settings, Error> {
        let path = paths.config_file();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => {
                return Err(Error::failure(format!(
                    "cannot read {}: {e}",
                    path.display()
                )))
            }
        };
        toml::from_str(&text).map_err(|e| {
            Error::usage(format!("{}: {e}", path.display()))
                .with_hint("fproton settings   # shows the effective values")
        })
    }

    /// Write `config.toml` (creating the directory).
    pub fn save(&self, paths: &Paths) -> Result<(), Error> {
        paths.ensure_dirs()?;
        let path = paths.config_file();
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::failure(format!("cannot serialise settings: {e}")))?;
        std::fs::write(&path, text)
            .map_err(|e| Error::failure(format!("cannot write {}: {e}", path.display())))
    }

    /// One key, in `SETTING_KEYS` spelling.
    pub fn get(&self, key: &str) -> Result<String, Error> {
        Ok(match key {
            "engine" => self.engine.clone(),
            "default_target" => self.default_target.clone(),
            "notify" => self.notify.to_string(),
            "timeouts.probe_sec" => self.timeouts.probe_sec.to_string(),
            "timeouts.connect_sec" => self.timeouts.connect_sec.to_string(),
            "timeouts.disconnect_sec" => self.timeouts.disconnect_sec.to_string(),
            "timeouts.config_set_sec" => self.timeouts.config_set_sec.to_string(),
            "timeouts.nmcli_sec" => self.timeouts.nmcli_sec.to_string(),
            "timeouts.busctl_sec" => self.timeouts.busctl_sec.to_string(),
            other => {
                return Err(Error::usage(format!(
                    "unknown setting '{other}'; known: {}",
                    SETTING_KEYS.join(", ")
                ))
                .with_hint("fproton settings --json"))
            }
        })
    }

    /// Set one key from its string form, validating the value.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        let secs = |v: &str| -> Result<u64, Error> {
            v.parse::<u64>()
                .ok()
                .filter(|n| (1..=600).contains(n))
                .ok_or_else(|| {
                    Error::usage(format!("'{v}' is not a deadline in seconds (1-600)"))
                        .with_hint(format!("fproton settings set {key} 15"))
                })
        };
        match key {
            "engine" => {
                if !ENGINES.contains(&value) {
                    return Err(Error::usage(format!(
                        "unknown engine '{value}'; known: {}",
                        ENGINES.join(", ")
                    ))
                    .with_hint("fproton settings set engine cli"));
                }
                self.engine = value.to_string();
            }
            "default_target" => {
                // Rejects anything `fproton connect` would not accept.
                let _ = value.parse::<crate::engine::Target>()?;
                self.default_target = value.to_string();
            }
            "notify" => {
                self.notify = match value {
                    "on" | "true" | "yes" => true,
                    "off" | "false" | "no" => false,
                    other => {
                        return Err(Error::usage(format!("'{other}' is not on/off"))
                            .with_hint("fproton settings set notify off"))
                    }
                }
            }
            "timeouts.probe_sec" => self.timeouts.probe_sec = secs(value)?,
            "timeouts.connect_sec" => self.timeouts.connect_sec = secs(value)?,
            "timeouts.disconnect_sec" => self.timeouts.disconnect_sec = secs(value)?,
            "timeouts.config_set_sec" => self.timeouts.config_set_sec = secs(value)?,
            "timeouts.nmcli_sec" => self.timeouts.nmcli_sec = secs(value)?,
            "timeouts.busctl_sec" => self.timeouts.busctl_sec = secs(value)?,
            other => {
                return Err(Error::usage(format!(
                    "unknown setting '{other}'; known: {}",
                    SETTING_KEYS.join(", ")
                ))
                .with_hint("fproton settings --json"))
            }
        }
        Ok(())
    }
}

/// Doctor Class A `settings_valid`: every problem with the loaded settings,
/// one human sentence each. `Ok(vec![])` means valid.
pub fn validate(ctx: &Context) -> Result<Vec<String>, Error> {
    let mut problems = Vec::new();
    let s = &ctx.settings;
    if !ENGINES.contains(&s.engine.as_str()) {
        problems.push(format!(
            "engine = '{}' is not one of: {}",
            s.engine,
            ENGINES.join(", ")
        ));
    }
    if let Err(e) = s.default_target.parse::<crate::engine::Target>() {
        problems.push(format!("default_target = '{}': {e}", s.default_target));
    }
    for (name, v) in [
        ("probe_sec", s.timeouts.probe_sec),
        ("connect_sec", s.timeouts.connect_sec),
        ("disconnect_sec", s.timeouts.disconnect_sec),
        ("config_set_sec", s.timeouts.config_set_sec),
        ("nmcli_sec", s.timeouts.nmcli_sec),
        ("busctl_sec", s.timeouts.busctl_sec),
    ] {
        if !(1..=600).contains(&v) {
            problems.push(format!("timeouts.{name} = {v} is outside 1-600"));
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design_deadlines() {
        let t = Timeouts::default();
        assert_eq!(
            (
                t.probe_sec,
                t.connect_sec,
                t.disconnect_sec,
                t.config_set_sec,
                t.nmcli_sec,
                t.busctl_sec
            ),
            (15, 90, 25, 20, 10, 1)
        );
    }

    #[test]
    fn timeout_override_clamps_everything() {
        let mut t = Timeouts::default();
        t.override_all(3);
        assert_eq!(t.connect().as_secs(), 3);
        assert_eq!(t.busctl().as_secs(), 3);
    }

    #[test]
    fn proton_whitelist_rejects_what_upstream_does_not_have() {
        assert!(validate_proton_config("netshield", "malware-ads-trackers").is_ok());
        assert!(validate_proton_config("kill-switch", "standard").is_ok());
        // Neither of these exists on proton-vpn-cli 1.0.3 (§4).
        let e = validate_proton_config("kill-switch", "permanent").unwrap_err();
        assert_eq!(e.exit_code(), 2);
        let e = validate_proton_config("protocol", "wireguard").unwrap_err();
        assert_eq!(e.exit_code(), 2);
        // Parsed by `config list`, not settable in v0.1.
        let e = validate_proton_config("custom-dns", "off").unwrap_err();
        assert!(e.to_string().contains("read-only"));
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let mut s = Settings::default();
        s.set("default_target", "CH").unwrap();
        s.set("timeouts.connect_sec", "30").unwrap();
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.get("default_target").unwrap(), "CH");
        assert!(s.set("default_target", "; rm -rf").is_err());
        assert!(s.set("engine", "wg").is_err());
    }
}
