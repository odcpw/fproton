//! `Status` — schema 1, the contract for the plugin, the TUI and agents (DESIGN.md §5).
//!
//! Shape rules, enforced by the types below:
//! * **every key is always present**; unknown is `null` — never an absent key, never
//!   `{}`, never a zero standing in for unknown (hence `Option<bool>` in [`Installed`]);
//! * timestamps are RFC 3339 **with local offset** (jiff);
//! * durations are integers named `*_sec`.
//!
//! [`State::derive`] is the §5 truth table, and it is a total function: there is no
//! `unknown` state.
//!
//! [`status`] never spawns `protonvpn` and never rebuilds the index. Its three
//! external probes — one `nmcli`, one `systemctl is-active`, one `busctl` — run on
//! their own threads, so the wall clock is the slowest of them rather than their sum.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::index::IndexInfo;
use crate::nm;
use crate::state::{self, StateFile};

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

// ---------------------------------------------------------------- probes

/// Proton's last connection, as the CLI persisted it (§1.3). Read-only, and never
/// the fields that carry key material: `x25519pk` and `server_id` are not declared
/// here, so they cannot reach any output.
#[derive(Debug, Deserialize)]
struct Persistence {
    /// The **NetworkManager UUID** of that server's profile (§5).
    connection_id: Option<String>,
    server: Option<PersistenceServer>,
}

#[derive(Debug, Deserialize)]
struct PersistenceServer {
    server_name: Option<String>,
    server_ip: Option<String>,
    wireguard_ports: Option<PersistencePorts>,
}

#[derive(Debug, Deserialize)]
struct PersistencePorts {
    #[serde(default)]
    udp: Vec<u16>,
}

impl Persistence {
    fn read(ctx: &Context) -> Option<Persistence> {
        let text = std::fs::read_to_string(ctx.paths.connection_persistence()).ok()?;
        serde_json::from_str(&text).ok()
    }
    /// `146.70.226.194:443` — the endpoint §5 shows for CH#461.
    fn endpoint(&self) -> Option<String> {
        let server = self.server.as_ref()?;
        let ip = server.server_ip.as_ref()?;
        let port = server.wireguard_ports.as_ref()?.udp.first()?;
        Some(format!("{ip}:{port}"))
    }
    fn server_name(&self) -> Option<&str> {
        self.server.as_ref()?.server_name.as_deref()
    }
}

/// `systemctl is-active proton.VPN.service`; `None` when systemctl itself is absent.
fn daemon_active(ctx: &Context) -> Option<bool> {
    let out = crate::proc::run_with_deadline(
        &["systemctl", "is-active", "proton.VPN.service"],
        ctx.timeouts.nmcli(),
        None,
    )
    .ok()?;
    if out.timed_out {
        return None;
    }
    // `is-active` exits non-zero for every state but active, and prints the state.
    Some(out.stdout_string().trim() == "active")
}

/// The Proton GTK app, by the desktop files its packages install. A file test, not a
/// spawn: `status` has a 50 ms budget.
fn gtk_app_installed() -> bool {
    [
        "/usr/share/applications/protonvpn-app.desktop",
        "/usr/share/applications/protonvpn.desktop",
        "/usr/bin/protonvpn-app",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

/// Turn the raw `pending` record into (live pending, error it became).
fn settle_pending(pending: Option<Pending>) -> (Option<Pending>, Option<LastError>) {
    let Some(p) = pending else {
        return (None, None);
    };
    if p.is_live() {
        return (Some(p), None);
    }
    // A pending past its deadline is a crashed or killed run (§5): it becomes the
    // last error and stays on disk for `doctor --fix` to clear.
    let target = p.target.clone().unwrap_or_else(|| "fastest".to_string());
    let text = format!("{} {target} exceeded {} s", p.action.name(), p.deadline_sec);
    let e = Error::upstream(text.clone()).with_hint("fproton doctor --fix");
    (
        Some(p.clone()),
        Some(LastError {
            at: crate::now_rfc3339(),
            code: e.exit_code(),
            code_name: e.code_name().to_string(),
            text,
            hint: e.hint(),
        }),
    )
}

/// Bytes per second between two sysfs samples, or `None` when a rate cannot be told
/// honestly (no earlier sample, no elapsed time, or a counter that went backwards).
fn rate(previous: u64, current: u64, seconds: f64) -> Option<f64> {
    if seconds <= 0.0 || current < previous {
        return None;
    }
    Some((current - previous) as f64 / seconds)
}

fn warn(id: &str, severity: Severity, text: impl Into<String>, hint: impl Into<String>) -> Warning {
    Warning {
        id: id.to_string(),
        severity,
        text: text.into(),
        hint: hint.into(),
    }
}

// ---------------------------------------------------------------- frozen API

/// The whole of §5 from NetworkManager + state + index + a Secret Service probe.
///
/// Never spawns `protonvpn`, never rebuilds the index, budget < 50 ms warm.
pub fn status(ctx: &Context) -> Result<Status, Error> {
    let generated_at = crate::now_rfc3339();
    let mut s = Status::empty(&ctx.settings.engine, generated_at.clone());
    let mut state: StateFile = state::load(ctx)?;
    let mut state_changed = false;

    // ---- the three external probes, in parallel -------------------------------
    let need_signed_in = !state::signed_in_is_fresh(&state);
    let (connections, daemon, proton_items, disk) = std::thread::scope(|scope| {
        let nm_probe = scope.spawn(|| nm::connections(ctx));
        let daemon_probe = scope.spawn(|| daemon_active(ctx));
        let secrets_probe =
            scope.spawn(|| need_signed_in.then(crate::secrets::search_proton_items));
        // Proton's persistence file names the last server without asking nmcli, so the
        // index join runs beside the nmcli call rather than after it.
        let disk_probe = scope.spawn(|| {
            let persistence = Persistence::read(ctx);
            let row = persistence
                .as_ref()
                .and_then(|p| p.server_name())
                .and_then(|name| crate::index::find_server(ctx, name).ok().flatten());
            (persistence, row, crate::index::summary(ctx))
        });
        (
            nm_probe.join(),
            daemon_probe.join(),
            secrets_probe.join(),
            disk_probe.join(),
        )
    });
    let joined = |what: &str| Error::failure(format!("the {what} probe panicked"));
    let connections = connections.map_err(|_| joined("NetworkManager"))?;
    let daemon = daemon.map_err(|_| joined("daemon"))?;
    let proton_items = proton_items.map_err(|_| joined("Secret Service"))?;
    let (persistence, persisted_row, index) = disk.map_err(|_| joined("index"))?;

    // ---- installed ------------------------------------------------------------
    s.installed = Installed {
        cli: Some(crate::proc::which(crate::cli::PROGRAM).is_some()),
        daemon,
        gtk_app: Some(gtk_app_installed()),
        wireguard: Some(std::path::Path::new("/sys/module/wireguard").exists()),
        networkmanager: Some(crate::proc::which(nm::PROGRAM).is_some()),
    };

    // ---- signed_in ------------------------------------------------------------
    s.signed_in = match proton_items {
        Some(Ok(items)) => {
            let answer = Some(!items.is_empty());
            state.signed_in = answer;
            state.signed_in_at = Some(generated_at.clone());
            state_changed = true;
            answer
        }
        // A busctl failure is "unknown", never "signed out" (§5).
        Some(Err(_)) => None,
        None => state.signed_in,
    };

    // ---- link and server ------------------------------------------------------
    let mut nm_error = None;
    let link_state = match connections {
        Ok(conns) => {
            let link = nm::link_state_from_connections(&conns);
            s.kill_switch.active = link.killswitch_active;
            Some((link, conns))
        }
        Err(e) => {
            nm_error = Some(e);
            None
        }
    };
    let (link, conns) = match link_state {
        Some((link, conns)) => (link, conns),
        None => (nm::LinkState::default(), Vec::new()),
    };

    s.link.up = link.up;
    s.link.device = link.device.clone();
    s.link.nm_connection = link.nm_connection.clone();

    // §5 resolution order: (1) the active profile, (2) the persistence file by UUID,
    // (3) nothing at all. Never a guess, never a match by name order.
    let mut server_name: Option<String> = None;
    if link.up {
        server_name = link
            .nm_connection
            .as_deref()
            .and_then(|n| n.strip_prefix("ProtonVPN ").map(str::to_string));
        if let Some(p) = &persistence {
            if p.connection_id.as_deref() == link.uuid.as_deref() {
                s.link.endpoint = p.endpoint();
            }
        }
    } else if let Some(p) = &persistence {
        if let Some(uuid) = p.connection_id.as_deref() {
            if let Some(profile) = conns.iter().find(|c| c.uuid == uuid) {
                s.link.nm_connection = Some(profile.name.clone());
            }
        }
        server_name = p.server_name().map(str::to_string);
        s.link.endpoint = p.endpoint();
    }

    // Uptime is measured from the first moment fproton saw *this* profile up: neither
    // NetworkManager's terse listing nor sysfs carries a trustworthy start time.
    if link.up {
        if state.link_uuid != link.uuid || state.link_up_since.is_none() {
            state.link_uuid = link.uuid.clone();
            state.link_up_since = Some(generated_at.clone());
            state_changed = true;
        }
        s.link.since = state.link_up_since.clone();
        s.link.uptime_sec = s
            .link
            .since
            .as_deref()
            .and_then(state::seconds_since)
            .unwrap_or(0);
    } else if state.link_up_since.is_some() || state.link_uuid.is_some() {
        state.link_up_since = None;
        state.link_uuid = None;
        state_changed = true;
    }

    if let Some(name) = server_name {
        // The index join is best-effort and never rebuilds: an unknown name keeps its
        // name and leaves every derived field null (§5). The row was usually looked up
        // beside the nmcli call; only an active profile the persistence file does not
        // name costs a second pass.
        let row = match persisted_row {
            Some(r) if r.name.eq_ignore_ascii_case(&name) => Some(r),
            _ => crate::index::find_server(ctx, &name).unwrap_or(None),
        };
        s.server = Some(match row {
            Some(r) => Server {
                name: r.name,
                country: Some(r.country),
                country_name: r.country_name,
                city: r.city,
                lat: r.lat,
                lon: r.lon,
                load: Some(r.load),
                score: Some(r.score),
                tier: Some(r.tier),
                features: Some(r.features),
                features_raw: Some(r.features_raw),
                entry_ip: r.entry_ip,
                exit_ip: r.exit_ip,
            },
            None => Server {
                name,
                country: None,
                country_name: None,
                city: None,
                lat: None,
                lon: None,
                load: None,
                score: None,
                tier: None,
                features: None,
                features_raw: None,
                entry_ip: None,
                exit_ip: None,
            },
        });
    }

    // ---- traffic --------------------------------------------------------------
    if link.up {
        if let Some(device) = s.link.device.as_deref() {
            if let Ok(Some(mut sample)) = nm::traffic(ctx, device) {
                if let Some(prev) = &state.traffic {
                    let seconds = state::unix_from_rfc3339(&sample.sampled_at)
                        .zip(state::unix_from_rfc3339(&prev.sampled_at))
                        .map(|(now, then)| (now - then) as f64)
                        .unwrap_or(0.0);
                    sample.rx_bps = rate(prev.rx_bytes, sample.rx_bytes, seconds);
                    sample.tx_bps = rate(prev.tx_bytes, sample.tx_bytes, seconds);
                }
                state.traffic = Some(Traffic {
                    rx_bps: None,
                    tx_bps: None,
                    ..sample.clone()
                });
                state_changed = true;
                s.traffic = Some(sample);
            }
        }
    } else if state.traffic.is_some() {
        state.traffic = None;
        state_changed = true;
    }

    // ---- caches ---------------------------------------------------------------
    s.config = state.config.clone();
    s.account = state.account.clone();
    s.port_forwarding = match (&state.port_forwarding, link.up) {
        (pf, true) => {
            let expired = pf
                .expires_at
                .as_deref()
                .and_then(state::unix_from_rfc3339)
                .is_some_and(|t| t <= state::unix_now() as i64);
            if expired {
                PortForwarding::default()
            } else {
                pf.clone()
            }
        }
        // §5: both null whenever the link is down.
        (_, false) => PortForwarding::default(),
    };
    s.cli = state.cli.clone();

    // ---- index ----------------------------------------------------------------
    s.index = index.info.clone();
    s.max_tier = index.max_tier;
    let freshness = index.freshness;

    // ---- pending, state and last_error ----------------------------------------
    let mut pending = state.pending.clone();
    if let Some(p) = &mut pending {
        state::refresh_pending(p);
    }
    let (pending, from_pending) = settle_pending(pending);
    let live_pending = pending.as_ref().filter(|p| p.is_live()).map(|p| p.action);
    s.pending = pending.clone();
    s.last_error = from_pending.or_else(|| state.last_error.clone());
    if let Some(e) = &nm_error {
        s.ok = false;
        s.last_error = Some(LastError {
            at: generated_at.clone(),
            code: e.exit_code(),
            code_name: e.code_name().to_string(),
            text: e.to_string(),
            hint: e.hint(),
        });
    }
    s.state = State::derive(link.up, live_pending, s.last_error.is_some());

    // ---- warnings -------------------------------------------------------------
    if let Some(left) = state::cli_backoff_remaining(&state) {
        s.warnings.push(warn(
            "cli_hang_suspected",
            Severity::Warn,
            format!(
                "the Proton CLI was killed at its deadline; CLI-backed commands are refused for {left}s"
            ),
            "fproton doctor --only cli",
        ));
    }
    match freshness {
        crate::index::Freshness::SourceMissing => s.warnings.push(warn(
            "serverlist_missing",
            Severity::Warn,
            "Proton's server list is not on disk; only the official CLI can fetch it",
            "fproton doctor --only cache",
        )),
        crate::index::Freshness::Missing | crate::index::Freshness::Stale => s.warnings.push(warn(
            "serverlist_stale",
            Severity::Info,
            "the server index was built from an older copy of Proton's server list",
            "fproton index --rebuild",
        )),
        crate::index::Freshness::Fresh => {
            if s.index.loads_expired == Some(true) {
                s.warnings.push(warn(
                    "serverlist_stale",
                    Severity::Info,
                    "the server loads in Proton's list have expired; they are shown as last seen",
                    "fproton index --rebuild",
                ));
            }
        }
    }
    if s.config.is_none() {
        s.warnings.push(warn(
            "protections_unknown",
            Severity::Info,
            "kill switch, NetShield and port forwarding are unknown: only the Proton CLI knows them",
            "fproton config list",
        ));
    }
    for device in &link.conflicting_tunnels {
        let hint = if device.starts_with("tailscale") {
            "tailscale set --exit-node="
        } else {
            "fproton doctor --only network"
        };
        s.warnings.push(warn(
            "conflicting_tunnel",
            Severity::Warn,
            format!(
                "{device} is up and will contest the default route and DNS with a Proton tunnel"
            ),
            hint,
        ));
    }
    if link.killswitch_active && !link.up {
        s.warnings.push(warn(
            "killswitch_orphan",
            Severity::Warn,
            format!(
                "{} is active while {} is down",
                crate::KILLSWITCH_PROFILE,
                crate::TUNNEL_DEVICE
            ),
            "fproton doctor --only network",
        ));
    }
    if pending.as_ref().is_some_and(|p| !p.is_live()) {
        s.warnings.push(warn(
            "stale_pending",
            Severity::Warn,
            "a recorded action outlived its deadline and left no result",
            "fproton doctor --fix",
        ));
    }
    match s.signed_in {
        Some(false) => s.warnings.push(warn(
            "not_signed_in",
            Severity::Warn,
            "no Proton session was found in the Secret Service",
            "fproton signin",
        )),
        None => s.warnings.push(warn(
            "secret_service_broken",
            Severity::Warn,
            "the Secret Service did not answer, so the session state is unknown",
            "fproton doctor --only keyring",
        )),
        Some(true) => {}
    }

    // ---- commands -------------------------------------------------------------
    s.commands = match s.state {
        State::Connected => vec!["fproton disconnect".to_string()],
        State::Connecting | State::Disconnecting => vec!["fproton status --json".to_string()],
        State::Disconnected => vec![format!("fproton connect {}", ctx.settings.default_target)],
        State::Error => vec!["fproton doctor".to_string()],
    };
    if s.warnings.iter().any(|w| w.id == "serverlist_stale") {
        s.commands.push("fproton index --rebuild".to_string());
    }
    if s.signed_in == Some(false) {
        s.commands.push("fproton signin".to_string());
    }

    if state_changed {
        // Never write back the snapshot taken *before* the probes. In the tens of
        // milliseconds they take, another process — or, in the TUI, the action thread
        // running beside the 1 s poll — may have written `pending`, a history row, a
        // recent or a `last_error`, and a whole-file save would silently revert it.
        // Commit only the five fields `status` owns, through `update`, which re-reads
        // under the advisory lock immediately before it writes.
        // A cache write must never be the reason `status` fails.
        let _ = state::update(ctx, |s| {
            s.signed_in = state.signed_in;
            s.signed_in_at = state.signed_in_at.clone();
            s.link_uuid = state.link_uuid.clone();
            s.link_up_since = state.link_up_since.clone();
            s.traffic = state.traffic.clone();
        });
    }
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

    /// A real `status()` on this machine, against Proton's own fixture data: the index
    /// is built from `tests/fixtures/serverlist-small.json` and the last connection
    /// from `tests/fixtures/connection_persistence-redacted.json`, so every
    /// server-derived literal below is the real CH#461 row (§5).
    #[test]
    fn status_resolves_the_last_server_from_the_persistence_file_by_uuid() {
        let dir = std::env::temp_dir().join(format!(
            "fproton-status-test-{}-{}",
            std::process::id(),
            state::unix_now()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut ctx = Context::for_test(&dir).expect("test context");
        let proton = dir.join("proton");
        std::fs::create_dir_all(proton.join("connection")).unwrap();
        std::fs::write(
            proton.join("serverlist.json"),
            include_str!("../../../tests/fixtures/serverlist-small.json"),
        )
        .unwrap();
        std::fs::write(
            proton.join("connection/connection_persistence.json"),
            include_str!("../../../tests/fixtures/connection_persistence-redacted.json"),
        )
        .unwrap();
        ctx.paths.proton_cache_dir = proton;
        crate::index::rebuild(&ctx, &crate::proc::CancelToken::new()).expect("index");

        let s = status(&ctx).expect("status answers");
        let v = serde_json::to_value(&s).unwrap();

        assert_eq!(v["schema"], 1);
        assert_eq!(v["engine"], "cli");
        assert_eq!(v["max_tier"], 2, "the fixture's MaxTier");

        // The last connection, resolved by NetworkManager UUID (§5).
        let server = &v["server"];
        assert_eq!(server["name"], "CH#461");
        assert_eq!(server["country"], "CH");
        assert_eq!(server["country_name"], "Switzerland");
        assert_eq!(server["city"], "Zurich");
        assert_eq!(server["load"], 63);
        assert_eq!(server["score"], 1.8623468195);
        assert_eq!(server["tier"], 2);
        assert_eq!(server["features_raw"], 28);
        assert_eq!(server["features"][0], "p2p");
        assert_eq!(server["entry_ip"], "146.70.226.194");
        assert_eq!(v["link"]["endpoint"], "146.70.226.194:443");

        // `state` is whatever the truth table says about the real machine, never a
        // sixth value, and every key of the document is present.
        let state_name = v["state"].as_str().unwrap().to_string();
        assert!([
            "connected",
            "connecting",
            "disconnecting",
            "disconnected",
            "error"
        ]
        .contains(&state_name.as_str()));
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

        // No zero standing in for unknown: what this build probes is a real boolean,
        // and what it cannot know stays null.
        for key in ["cli", "gtk_app", "wireguard", "networkmanager"] {
            assert!(
                v["installed"][key].is_boolean(),
                "installed.{key} is probed, so it must be a real boolean"
            );
        }
        if v["link"]["up"] == false {
            assert!(v["traffic"].is_null(), "no device, no traffic numbers");
            assert!(v["port_forwarding"]["port"].is_null());
        }
        // The index is reported, never rebuilt, by status.
        assert_eq!(v["index"]["servers"], 38);

        // §2: no secret-shaped string may appear anywhere in the document.
        let text = serde_json::to_string(&v).unwrap();
        for forbidden in ["PrivateKey", "RefreshToken", "AccessToken", "x25519pk"] {
            assert!(!text.contains(forbidden), "status leaked '{forbidden}'");
        }
        assert!(
            !text.contains("0ym2_1QG0"),
            "status leaked Proton's server_id from the persistence file"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pending_action_past_its_deadline_becomes_the_last_error() {
        let stale = Pending {
            action: PendingAction::Connect,
            target: Some("CH#461".into()),
            started_at: "2026-09-14T22:29:58+02:00".into(),
            deadline_sec: 90,
            elapsed_sec: 91,
            pid: Some(12345),
        };
        let (kept, error) = settle_pending(Some(stale));
        let error = error.expect("a dead pending becomes an error");
        assert_eq!(error.code, 5);
        assert_eq!(error.code_name, "upstream");
        assert_eq!(error.text, "connect CH#461 exceeded 90 s");
        assert!(kept.is_some(), "the record stays for `doctor --fix`");

        let live = Pending {
            elapsed_sec: 2,
            ..kept.unwrap()
        };
        let (kept, error) = settle_pending(Some(live));
        assert!(error.is_none());
        assert!(kept.unwrap().is_live());
    }

    #[test]
    fn a_rate_is_only_reported_when_two_samples_really_bracket_it() {
        assert_eq!(rate(1_000, 3_000, 2.0), Some(1_000.0));
        assert_eq!(rate(1_000, 3_000, 0.0), None, "no elapsed time, no rate");
        assert_eq!(
            rate(3_000, 1_000, 2.0),
            None,
            "a counter reset is not a rate"
        );
    }
}
