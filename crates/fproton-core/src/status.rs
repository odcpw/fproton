//! `Status` — schema 1, the contract for the plugin, the TUI and agents (DESIGN.md §5).
//!
//! Shape rules, enforced by the types below:
//! * **every key is always present**; unknown is `null` — never an absent key, never
//!   `{}`, never a zero standing in for unknown (hence `Option<bool>` in [`Installed`]);
//! * timestamps are RFC 3339 **with local offset** (jiff);
//! * durations are integers named `*_sec`.
//!
//! The deriving code is Phase 1b. [`State::derive`] (the §5 truth table) is real here
//! because every consumer depends on it being a total function.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::index::IndexInfo;

/// Schema version of every fproton JSON envelope.
pub const SCHEMA_VERSION: u32 = 1;

/// Stable warning ids (§5). Listed by `capabilities`; the plugin styles on them.
pub const WARNING_IDS: &[&str] = &[
    "cli_hang_suspected",
    "serverlist_stale",
    "serverlist_missing",
    "protections_unknown",
    "conflicting_tunnel",
    "killswitch_orphan",
    "stale_pending",
    "not_signed_in",
    "secret_service_broken",
];

/// The five states. There is no `unknown`: `state` is a total function of
/// link-up, a live `pending`, and `last_error` (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Connected,
    Connecting,
    Disconnecting,
    Disconnected,
    Error,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Connected => "connected",
            State::Connecting => "connecting",
            State::Disconnecting => "disconnecting",
            State::Disconnected => "disconnected",
            State::Error => "error",
        }
    }

    /// The §5 truth table. `pending` must already have been checked for liveness
    /// (an expired pending is dropped into `last_error` before this is called).
    pub fn derive(link_up: bool, live_pending: Option<PendingAction>, has_error: bool) -> State {
        match (link_up, live_pending) {
            (true, Some(PendingAction::Disconnect)) => State::Disconnecting,
            (true, _) => State::Connected,
            (false, Some(PendingAction::Connect)) | (false, Some(PendingAction::Reconnect)) => {
                State::Connecting
            }
            (false, Some(PendingAction::Disconnect)) => State::Disconnecting,
            (false, None) if has_error => State::Error,
            (false, None) => State::Disconnected,
        }
    }
}

/// Which external pieces are present. `null` = not probed in this build.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    /// `/usr/bin/protonvpn` (proton-vpn-cli).
    pub cli: Option<bool>,
    /// `proton.VPN.service` active.
    pub daemon: Option<bool>,
    /// The GTK desktop app.
    pub gtk_app: Option<bool>,
    /// The kernel `wireguard` module.
    pub wireguard: Option<bool>,
    pub networkmanager: Option<bool>,
}

/// The tunnel device and the NetworkManager profile behind it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Link {
    pub up: bool,
    /// `proton0`, or `null` when no device is known.
    pub device: Option<String>,
    /// The NetworkManager profile name, matched by UUID — never by name order (§5).
    pub nm_connection: Option<String>,
    /// RFC 3339; `null` while the link is down.
    pub since: Option<String>,
    pub uptime_sec: u64,
    /// `EntryIP:port` from the persistence file, e.g. `146.70.226.194:443`.
    pub endpoint: Option<String>,
}

/// The server behind the current (or last) connection, joined to the index (§5).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub country: Option<String>,
    pub country_name: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub load: Option<u8>,
    pub score: Option<f64>,
    pub tier: Option<u8>,
    /// `null` when the name is not in the index (never `[]`, which would mean "none").
    pub features: Option<Vec<String>>,
    pub features_raw: Option<u32>,
    pub entry_ip: Option<String>,
    pub exit_ip: Option<String>,
}

/// sysfs counters; `null` whenever `/sys/class/net/<device>` is absent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Traffic {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// `null` until a second sample exists.
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    pub sampled_at: String,
}

/// `supported` is false whenever the engine is not `cli` (§5).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct KillSwitch {
    pub active: bool,
    pub supported: bool,
}

/// The last successful `fproton port`, kept in state. `status` never sends a packet.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortForwarding {
    pub port: Option<u16>,
    pub expires_at: Option<String>,
}

/// Proton's own settings, cached from the last successful `config list` (§5).
/// A row the parser cannot map is stored verbatim; a key missing from the table is
/// `null`, never absent.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProtonConfig {
    pub netshield: Option<String>,
    #[serde(rename = "kill-switch")]
    pub kill_switch: Option<String>,
    #[serde(rename = "port-forwarding")]
    pub port_forwarding: Option<String>,
    #[serde(rename = "custom-dns")]
    pub custom_dns: Option<String>,
    #[serde(rename = "vpn-accelerator")]
    pub vpn_accelerator: Option<String>,
    #[serde(rename = "moderate-nat")]
    pub moderate_nat: Option<String>,
    pub ipv6: Option<String>,
    #[serde(rename = "anonymous-crash-reports")]
    pub anonymous_crash_reports: Option<String>,
    /// RFC 3339 of the `config list` run this came from.
    pub cached_at: Option<String>,
}

impl ProtonConfig {
    /// One value by its upstream key name.
    pub fn get(&self, key: &str) -> Option<&str> {
        match key {
            "netshield" => self.netshield.as_deref(),
            "kill-switch" => self.kill_switch.as_deref(),
            "port-forwarding" => self.port_forwarding.as_deref(),
            "custom-dns" => self.custom_dns.as_deref(),
            "vpn-accelerator" => self.vpn_accelerator.as_deref(),
            "moderate-nat" => self.moderate_nat.as_deref(),
            "ipv6" => self.ipv6.as_deref(),
            "anonymous-crash-reports" => self.anonymous_crash_reports.as_deref(),
            _ => None,
        }
    }
    /// Set one value by its upstream key name; false when the key is unknown.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> bool {
        let v = Some(value.into());
        match key {
            "netshield" => self.netshield = v,
            "kill-switch" => self.kill_switch = v,
            "port-forwarding" => self.port_forwarding = v,
            "custom-dns" => self.custom_dns = v,
            "vpn-accelerator" => self.vpn_accelerator = v,
            "moderate-nat" => self.moderate_nat = v,
            "ipv6" => self.ipv6 = v,
            "anonymous-crash-reports" => self.anonymous_crash_reports = v,
            _ => return false,
        }
        true
    }
}

/// Proton account facts. Upstream prints no plan; entitlement is `max_tier` (§4).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub cached_at: String,
}

/// Which action is in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingAction {
    Connect,
    Disconnect,
    Reconnect,
}

impl PendingAction {
    pub fn name(self) -> &'static str {
        match self {
            PendingAction::Connect => "connect",
            PendingAction::Disconnect => "disconnect",
            PendingAction::Reconnect => "reconnect",
        }
    }
}

/// The optimistic-UI record: written by core **before** the engine spawns and removed
/// when it exits. Live while `elapsed_sec < deadline_sec` (§5).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub action: PendingAction,
    pub target: Option<String>,
    pub started_at: String,
    pub deadline_sec: u64,
    pub elapsed_sec: u64,
    pub pid: Option<u32>,
}

impl Pending {
    /// A pending action that has not yet exceeded its own deadline.
    pub fn is_live(&self) -> bool {
        self.elapsed_sec < self.deadline_sec
    }
}

/// The CLI-hang circuit breaker (§2): after a deadline kill, CLI-backed commands
/// exit 5 immediately for 60 s.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CliState {
    pub last_ok_at: Option<String>,
    pub hang_suspected: bool,
    pub backoff_until: Option<String>,
}

/// The last failure, using the §4a codes and a runnable hint. Any new action clears it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastError {
    pub at: String,
    pub code: i32,
    pub code_name: String,
    pub text: String,
    pub hint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

/// One entry of `status.warnings`. `id` is from [`WARNING_IDS`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Warning {
    pub id: String,
    pub severity: Severity,
    pub text: String,
    pub hint: String,
}

/// The whole §5 document. Serialised as-is by `fproton status --json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub ok: bool,
    pub schema: u32,
    /// RFC 3339 with local offset.
    pub generated_at: String,
    pub state: State,
    pub engine: String,
    pub installed: Installed,
    pub signed_in: Option<bool>,
    pub max_tier: Option<u8>,
    pub account: Option<Account>,
    pub link: Link,
    pub server: Option<Server>,
    pub traffic: Option<Traffic>,
    pub kill_switch: KillSwitch,
    pub port_forwarding: PortForwarding,
    pub config: Option<ProtonConfig>,
    pub index: IndexInfo,
    pub pending: Option<Pending>,
    pub cli: CliState,
    pub last_error: Option<LastError>,
    pub warnings: Vec<Warning>,
    /// Runnable next steps.
    pub commands: Vec<String>,
}

impl Status {
    /// A document with every key present and everything unknown set to `null`.
    /// `state` follows the §5 table from the (empty) inputs, so it is `disconnected`.
    pub fn empty(engine: &str, generated_at: String) -> Status {
        Status {
            ok: true,
            schema: SCHEMA_VERSION,
            generated_at,
            state: State::Disconnected,
            engine: engine.to_string(),
            installed: Installed::default(),
            signed_in: None,
            max_tier: None,
            account: None,
            link: Link::default(),
            server: None,
            traffic: None,
            kill_switch: KillSwitch {
                active: false,
                supported: engine == "cli",
            },
            port_forwarding: PortForwarding::default(),
            config: None,
            index: IndexInfo::unknown(),
            pending: None,
            cli: CliState::default(),
            last_error: None,
            warnings: Vec::new(),
            commands: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- frozen API

/// The whole of §5 from NetworkManager + state + index + a Secret Service probe.
///
/// Never spawns `protonvpn`, never rebuilds the index, budget < 50 ms warm.
///
/// **This build is the Phase-1a API freeze**: nothing is probed yet. The document is
/// schema-valid with every key present, and says so in `last_error` rather than
/// reporting a state it did not measure.
pub fn status(ctx: &Context) -> Result<Status, Error> {
    let mut s = Status::empty(&ctx.settings.engine, crate::now_rfc3339());
    let e = Error::unimplemented("status");
    s.ok = false;
    s.state = State::Error;
    s.last_error = Some(LastError {
        at: s.generated_at.clone(),
        code: e.exit_code(),
        code_name: e.code_name().to_string(),
        text: e.to_string(),
        hint: e.hint(),
    });
    s.commands = vec!["fproton capabilities".to_string()];
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_truth_table_is_total() {
        use PendingAction::*;
        assert_eq!(
            State::derive(true, Some(Disconnect), true),
            State::Disconnecting
        );
        assert_eq!(State::derive(true, None, true), State::Connected);
        assert_eq!(State::derive(true, Some(Connect), false), State::Connected);
        assert_eq!(State::derive(false, Some(Connect), true), State::Connecting);
        assert_eq!(
            State::derive(false, Some(Reconnect), false),
            State::Connecting
        );
        assert_eq!(State::derive(false, None, true), State::Error);
        assert_eq!(State::derive(false, None, false), State::Disconnected);
    }

    #[test]
    fn every_key_is_present_in_the_serialised_document() {
        let s = Status::empty("cli", "2026-09-14T22:30:00+02:00".into());
        let v = serde_json::to_value(&s).unwrap();
        for key in [
            "ok",
            "schema",
            "generated_at",
            "state",
            "engine",
            "installed",
            "signed_in",
            "max_tier",
            "account",
            "link",
            "server",
            "traffic",
            "kill_switch",
            "port_forwarding",
            "config",
            "index",
            "pending",
            "cli",
            "last_error",
            "warnings",
            "commands",
        ] {
            assert!(v.get(key).is_some(), "status.{key} must always be present");
        }
        for key in ["cli", "daemon", "gtk_app", "wireguard", "networkmanager"] {
            assert!(
                v["installed"][key].is_null(),
                "unknown must be null, not false"
            );
        }
        for key in [
            "up",
            "device",
            "nm_connection",
            "since",
            "uptime_sec",
            "endpoint",
        ] {
            assert!(v["link"].get(key).is_some(), "link.{key} must be present");
        }
        assert_eq!(v["schema"], 1);
        assert_eq!(v["state"], "disconnected");
    }

    #[test]
    fn proton_config_keys_use_the_upstream_spelling() {
        let mut c = ProtonConfig::default();
        assert!(c.set("kill-switch", "standard"));
        assert!(!c.set("protocol", "wireguard"));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kill-switch"], "standard");
        assert!(v["port-forwarding"].is_null());
        assert_eq!(c.get("kill-switch"), Some("standard"));
    }

    #[test]
    fn a_pending_action_is_live_only_inside_its_deadline() {
        let mut p = Pending {
            action: PendingAction::Connect,
            target: Some("fastest".into()),
            started_at: "2026-09-14T22:29:58+02:00".into(),
            deadline_sec: 90,
            elapsed_sec: 2,
            pid: Some(12345),
        };
        assert!(p.is_live());
        p.elapsed_sec = 90;
        assert!(!p.is_live());
    }
}
