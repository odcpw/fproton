//! Every §7 check, as a **pure function of captured [`Probes`]**.
//!
//! Nothing in this file spawns a process, reads a file or looks at the clock: a
//! detector sees only what `probes.rs` captured, so the nine detectors that parse
//! structured text are exercised by the fixtures under `tests/fixtures/doctor/`, and
//! the one-line predicates by the table test at the bottom.
//!
//! A detector never applies anything. When a repair exists it names a [`PlanKind`];
//! `mutate.rs` turns that into the actual [`Action`](super::mutate::Action)s.

use fproton_core::secrets;

use super::probes::{Probes, SERVERLIST_MIN_LOGICALS};
use super::DoctorGroup;

/// The safety class of a check's repair (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// fproton-owned state; runs under `--fix`; reverts by file backup.
    A,
    /// fproton-created integration; runs under `--fix`; reverts by a recorded inverse.
    B,
    /// Foreign state, reversible; needs `--fix --allow-keyring --yes`.
    C,
    /// Never auto-fixed.
    D,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::A => "A",
            Class::B => "B",
            Class::C => "C",
            Class::D => "D",
        }
    }
    pub fn describes(self) -> &'static str {
        match self {
            Class::A => "fproton-owned state; reverts by file backup",
            Class::B => "fproton-created integration; reverts by a recorded inverse",
            Class::C => "foreign state, reversible; needs --allow-keyring --yes",
            Class::D => "never auto-fixed; the detail names the command for a human",
        }
    }
}

/// How bad a non-pass is for this check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }
}

/// A check's verdict (§7 `status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl CheckStatus {
    pub fn name(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
            CheckStatus::Skip => "skip",
        }
    }
}

/// The repairs `mutate.rs` knows how to plan. A detector only names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    CreateStateDir,
    ResetStateJson,
    ClearPending,
    NormaliseSettings,
    RebuildIndex,
    LinkPlugin,
    EnablePlugin,
    MergeMenu,
    DisableOldPlugin,
    KillswitchDown,
    SetDefaultAlias,
}

impl PlanKind {
    pub fn name(self) -> &'static str {
        match self {
            PlanKind::CreateStateDir => "create_state_dir",
            PlanKind::ResetStateJson => "reset_state_json",
            PlanKind::ClearPending => "clear_pending",
            PlanKind::NormaliseSettings => "normalise_settings",
            PlanKind::RebuildIndex => "rebuild_index",
            PlanKind::LinkPlugin => "link_plugin",
            PlanKind::EnablePlugin => "enable_plugin",
            PlanKind::MergeMenu => "merge_menu",
            PlanKind::DisableOldPlugin => "disable_old_plugin",
            PlanKind::KillswitchDown => "killswitch_down",
            PlanKind::SetDefaultAlias => "set_default_alias",
        }
    }
}

/// What one detector answers.
#[derive(Clone, Debug)]
pub struct Verdict {
    pub status: CheckStatus,
    /// The short machine reason behind a non-pass (`phantom_collection`, `link_down`).
    pub reason: Option<&'static str>,
    pub detail: String,
    /// A command for the human. Always set when a repair exists, fixable or not.
    pub fix_cmd: Option<String>,
    /// Set only when the doctor itself can repair this.
    pub plan: Option<PlanKind>,
}

impl Verdict {
    pub fn pass(detail: impl Into<String>) -> Verdict {
        Verdict {
            status: CheckStatus::Pass,
            reason: None,
            detail: detail.into(),
            fix_cmd: None,
            plan: None,
        }
    }
    pub fn warn(reason: &'static str, detail: impl Into<String>) -> Verdict {
        Verdict {
            status: CheckStatus::Warn,
            reason: Some(reason),
            detail: detail.into(),
            fix_cmd: None,
            plan: None,
        }
    }
    pub fn fail(reason: &'static str, detail: impl Into<String>) -> Verdict {
        Verdict {
            status: CheckStatus::Fail,
            reason: Some(reason),
            detail: detail.into(),
            fix_cmd: None,
            plan: None,
        }
    }
    pub fn skip(reason: &'static str, detail: impl Into<String>) -> Verdict {
        Verdict {
            status: CheckStatus::Skip,
            reason: Some(reason),
            detail: detail.into(),
            fix_cmd: None,
            plan: None,
        }
    }
    /// A command a human can run. Does not make the check fixable.
    #[must_use]
    pub fn cmd(mut self, cmd: impl Into<String>) -> Verdict {
        self.fix_cmd = Some(cmd.into());
        self
    }
    /// The doctor can repair this; the command shown is the fproton invocation.
    #[must_use]
    pub fn fixable(mut self, plan: PlanKind, cmd: impl Into<String>) -> Verdict {
        self.plan = Some(plan);
        self.fix_cmd = Some(cmd.into());
        self
    }
}

/// One check, statically declared. `doctor capabilities` prints this table verbatim.
pub struct CheckDef {
    pub id: &'static str,
    pub group: DoctorGroup,
    pub class: Class,
    pub severity: Severity,
    pub title: &'static str,
    pub detect: fn(&Probes) -> Verdict,
}

/// Every §7 check, in report order.
pub const CHECKS: &[CheckDef] = &[
    // ---- cli
    CheckDef {
        id: "cli_installed",
        group: DoctorGroup::Cli,
        class: Class::D,
        severity: Severity::Error,
        title: "the official protonvpn CLI is installed",
        detect: cli_installed,
    },
    CheckDef {
        id: "cli_help_fast",
        group: DoctorGroup::Cli,
        class: Class::D,
        severity: Severity::Warn,
        title: "`protonvpn --help` answers quickly",
        detect: cli_help_fast,
    },
    CheckDef {
        id: "cli_session_probe",
        group: DoctorGroup::Cli,
        class: Class::D,
        severity: Severity::Error,
        title: "a session-touching CLI command answers",
        detect: cli_session_probe,
    },
    CheckDef {
        id: "daemon_active",
        group: DoctorGroup::Cli,
        class: Class::D,
        severity: Severity::Warn,
        title: "proton.VPN.service is running",
        detect: daemon_active,
    },
    // ---- keyring
    CheckDef {
        id: "secret_service_on_bus",
        group: DoctorGroup::Keyring,
        class: Class::D,
        severity: Severity::Error,
        title: "a Secret Service owns org.freedesktop.secrets",
        detect: secret_service_on_bus,
    },
    CheckDef {
        id: "secret_service_default_alias",
        group: DoctorGroup::Keyring,
        class: Class::C,
        severity: Severity::Error,
        title: "the `default` collection alias resolves",
        detect: secret_service_default_alias,
    },
    CheckDef {
        id: "proton_session_item",
        group: DoctorGroup::Keyring,
        class: Class::D,
        severity: Severity::Error,
        title: "a Proton session item exists in the keyring",
        detect: proton_session_item,
    },
    // ---- network
    CheckDef {
        id: "networkmanager_running",
        group: DoctorGroup::Network,
        class: Class::D,
        severity: Severity::Error,
        title: "NetworkManager is running",
        detect: networkmanager_running,
    },
    CheckDef {
        id: "wireguard_available",
        group: DoctorGroup::Network,
        class: Class::D,
        severity: Severity::Error,
        title: "the kernel wireguard module is loaded",
        detect: wireguard_available,
    },
    CheckDef {
        id: "conflicting_tunnel",
        group: DoctorGroup::Network,
        class: Class::D,
        severity: Severity::Warn,
        title: "no other tunnel contests the default route",
        detect: conflicting_tunnel,
    },
    CheckDef {
        id: "stale_proton_profiles",
        group: DoctorGroup::Network,
        class: Class::D,
        severity: Severity::Warn,
        title: "the CLI's leftover WireGuard profiles",
        detect: stale_proton_profiles,
    },
    CheckDef {
        id: "killswitch_orphan",
        group: DoctorGroup::Network,
        class: Class::B,
        severity: Severity::Warn,
        title: "the IPv6 leak guard is not left up alone",
        detect: killswitch_orphan,
    },
    // ---- cache
    CheckDef {
        id: "serverlist_present",
        group: DoctorGroup::Cache,
        class: Class::D,
        severity: Severity::Error,
        title: "Proton's server list is present and parses",
        detect: serverlist_present,
    },
    CheckDef {
        id: "serverlist_expired",
        group: DoctorGroup::Cache,
        class: Class::D,
        severity: Severity::Warn,
        title: "the server list has not expired",
        detect: serverlist_expired,
    },
    CheckDef {
        id: "serverlist_loads_stale",
        group: DoctorGroup::Cache,
        class: Class::D,
        severity: Severity::Info,
        title: "the per-server loads are current",
        detect: serverlist_loads_stale,
    },
    CheckDef {
        id: "index_fresh",
        group: DoctorGroup::Cache,
        class: Class::A,
        severity: Severity::Warn,
        title: "fproton's index snapshot matches the source",
        detect: index_fresh,
    },
    // ---- install
    CheckDef {
        id: "state_dir",
        group: DoctorGroup::Install,
        class: Class::A,
        severity: Severity::Error,
        title: "the state directory exists and is writable",
        detect: state_dir,
    },
    CheckDef {
        id: "stale_pending_action",
        group: DoctorGroup::Install,
        class: Class::A,
        severity: Severity::Warn,
        title: "no action is recorded past its deadline",
        detect: stale_pending_action,
    },
    CheckDef {
        id: "settings_valid",
        group: DoctorGroup::Install,
        class: Class::A,
        severity: Severity::Warn,
        title: "config.toml holds values fproton accepts",
        detect: settings_valid,
    },
    CheckDef {
        id: "binary_on_path",
        group: DoctorGroup::Install,
        class: Class::D,
        severity: Severity::Warn,
        title: "~/.local/bin/fproton is what the shell finds",
        detect: binary_on_path,
    },
    CheckDef {
        id: "plugin_linked",
        group: DoctorGroup::Install,
        class: Class::B,
        severity: Severity::Warn,
        title: "the bar widget is linked into Omarchy",
        detect: plugin_linked,
    },
    CheckDef {
        id: "plugin_enabled",
        group: DoctorGroup::Install,
        class: Class::B,
        severity: Severity::Warn,
        title: "the bar widget is enabled",
        detect: plugin_enabled,
    },
    CheckDef {
        id: "menu_entries",
        group: DoctorGroup::Install,
        class: Class::B,
        severity: Severity::Info,
        title: "the Omarchy menu carries fproton's entries",
        detect: menu_entries,
    },
    CheckDef {
        id: "old_plugin_enabled",
        group: DoctorGroup::Install,
        class: Class::B,
        severity: Severity::Warn,
        title: "the replaced plugin is disabled",
        detect: old_plugin_enabled,
    },
    // ---- connected
    CheckDef {
        id: "dns_when_connected",
        group: DoctorGroup::Connected,
        class: Class::D,
        severity: Severity::Error,
        title: "the tunnel's DNS is Proton's resolver",
        detect: dns_when_connected,
    },
    CheckDef {
        id: "tunnel_route_ok",
        group: DoctorGroup::Connected,
        class: Class::D,
        severity: Severity::Error,
        title: "the default route goes through the tunnel",
        detect: tunnel_route_ok,
    },
];

// ------------------------------------------------------------------ cli group

fn cli_installed(p: &Probes) -> Verdict {
    let Some(path) = &p.which_protonvpn else {
        return Verdict::fail("not_installed", "protonvpn is not on PATH")
            .cmd("omarchy pkg add proton-vpn-cli");
    };
    let pkg = p
        .pacman_proton
        .success()
        .then(|| p.pacman_proton.stdout.trim().to_string())
        .filter(|s| !s.is_empty());
    match pkg {
        Some(v) => Verdict::pass(format!("{path} ({v})")),
        None => Verdict::pass(format!("{path} (not owned by a pacman package)")),
    }
}

fn cli_help_fast(p: &Probes) -> Verdict {
    if !p.cli_help.taken {
        return Verdict::skip("not_probed", "the cli group was not selected");
    }
    if p.cli_help.timed_out {
        return Verdict::fail(
            "help_hang",
            format!(
                "`protonvpn --help` was killed after {} ms; even the argument parser is blocked",
                p.cli_help.duration_ms
            ),
        )
        .cmd("fproton doctor --only keyring");
    }
    if !p.cli_help.success() {
        return Verdict::fail(
            "help_failed",
            format!("`protonvpn --help`: {}", p.cli_help.failure_detail()),
        );
    }
    Verdict::pass(format!("answered in {} ms", p.cli_help.duration_ms))
}

/// The one session-touching command the doctor runs, always under a deadline (§1.2).
fn cli_session_probe(p: &Probes) -> Verdict {
    if !p.cli_session.taken {
        return Verdict::skip("not_probed", "the cli group was not selected");
    }
    if p.cli_session.timed_out {
        return Verdict::fail(
            "hang",
            format!(
                "`protonvpn config list` was killed at its deadline after {} ms. \
                 python-keyring resolves the Secret Service `default` collection on every \
                 call; when that alias points at a collection with no exported object the \
                 call blocks on a gcr prompt that never maps",
                p.cli_session.duration_ms
            ),
        )
        .cmd("fproton doctor --only keyring");
    }
    if p.cli_session.success() {
        let rows = p
            .cli_session
            .stdout
            .lines()
            .filter(|l| l.contains("  ") && !l.trim().is_empty())
            .count();
        return Verdict::pass(format!(
            "answered in {} ms with {rows} table lines",
            p.cli_session.duration_ms
        ));
    }
    let text = format!("{}\n{}", p.cli_session.stdout, p.cli_session.stderr).to_ascii_lowercase();
    if text.contains("not logged in") || text.contains("sign in") || text.contains("not signed in")
    {
        return Verdict::fail("not_signed_in", "the CLI reports no Proton session")
            .cmd("fproton signin");
    }
    Verdict::fail(
        "cli_error",
        format!(
            "`protonvpn config list`: {}",
            p.cli_session.failure_detail()
        ),
    )
}

fn daemon_active(p: &Probes) -> Verdict {
    if !p.daemon_active.taken {
        return Verdict::skip("not_probed", "the cli group was not selected");
    }
    let word = p.daemon_active.stdout.trim().to_string();
    if word == "active" {
        return Verdict::pass("proton.VPN.service is active");
    }
    Verdict::warn(
        "inactive",
        format!(
            "proton.VPN.service is '{}' (split tunnelling needs it)",
            if word.is_empty() { "unknown" } else { &word }
        ),
    )
    .cmd("systemctl status proton.VPN.service")
}

// -------------------------------------------------------------- keyring group

fn secret_service_on_bus(p: &Probes) -> Verdict {
    if !p.busctl_list.taken {
        return Verdict::skip("not_probed", "the keyring group was not selected");
    }
    if !p.busctl_list.success() {
        return Verdict::fail(
            "busctl_failed",
            format!("`busctl --user list`: {}", p.busctl_list.failure_detail()),
        );
    }
    let owner = p
        .busctl_list
        .stdout
        .lines()
        .find(|l| l.starts_with(secrets::SERVICE));
    match owner {
        None => Verdict::fail(
            "no_provider",
            "nothing owns org.freedesktop.secrets on the session bus",
        )
        .cmd("systemctl --user status gnome-keyring-daemon"),
        Some(line) => {
            let who = line.split_whitespace().nth(2).unwrap_or("a process");
            let others: Vec<&str> = p
                .busctl_list
                .stdout
                .lines()
                .filter(|l| l.starts_with("org.kde.secretservicecompat"))
                .map(|_| "org.kde.secretservicecompat")
                .collect();
            let mut detail = format!("owned by {who}");
            if !others.is_empty() {
                detail.push_str(" (org.kde.secretservicecompat is also activatable)");
            }
            Verdict::pass(detail)
        }
    }
}

/// The check this machine fails today: `fail:phantom_collection` (§1.2).
fn secret_service_default_alias(p: &Probes) -> Verdict {
    let Some(alias) = &p.alias else {
        return Verdict::skip("not_probed", "the keyring group was not selected");
    };
    let holders: Vec<&super::probes::CollectionProbe> = p
        .collections
        .iter()
        .filter(|c| !c.proton_items.is_empty())
        .collect();
    let holder_names = |sep: &str| -> String {
        holders
            .iter()
            .map(|c| c.label.clone().unwrap_or_else(|| c.path.clone()))
            .collect::<Vec<_>>()
            .join(sep)
    };
    let live = p
        .collections
        .iter()
        .map(|c| {
            format!(
                "{}{}",
                c.path.rsplit('/').next().unwrap_or(&c.path),
                if c.exported { "" } else { " (no object)" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let Some(path) = &alias.path else {
        return Verdict::fail(
            "no_alias",
            format!("the `default` alias is unset; collections on the bus: {live}"),
        )
        .cmd("fproton doctor --fix --allow-keyring --yes --only keyring");
    };
    // The 16-byte pointer file that decides the alias at the next login. It holds a
    // keyring *basename*, never key material, and no `*.keyring` file is opened (§2).
    let pointer = p
        .keyring_pointer
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("; the login pointer file names '{t}'"))
        .unwrap_or_default();
    if !alias.exported {
        let detail = format!(
            "the `default` alias points at {path}, which is listed in Collections but has \
             no exported object — every python-keyring call blocks on it. Collections: {live}. \
             A `service=Proton` item lives in: {}{pointer}",
            if holders.is_empty() {
                "none of them".to_string()
            } else {
                holder_names(", ")
            }
        );
        return Verdict::fail("phantom_collection", detail).fixable(
            PlanKind::SetDefaultAlias,
            "fproton doctor --fix --allow-keyring --yes --only keyring",
        );
    }
    if alias.locked == Some(true) {
        return Verdict::fail(
            "locked",
            format!(
                "the `default` alias resolves to {path} ({}), which is locked",
                alias.label.clone().unwrap_or_default()
            ),
        )
        .cmd("run a desktop keyring unlock; fproton never unlocks a collection");
    }
    Verdict::pass(format!(
        "`default` → {path} ({}), unlocked",
        alias.label.clone().unwrap_or_else(|| "no label".into())
    ))
}

fn proton_session_item(p: &Probes) -> Verdict {
    if let Some(e) = &p.proton_items_error {
        return Verdict::fail("search_failed", format!("SearchItems service=Proton: {e}"));
    }
    if !p.collections.is_empty() || !p.proton_items.is_empty() {
        if p.proton_items.is_empty() {
            return Verdict::fail(
                "not_signed_in",
                "no keyring item carries service=Proton: this machine has no Proton session",
            )
            .cmd("fproton signin");
        }
        let collections: Vec<String> = p
            .proton_items
            .iter()
            .filter_map(|i| i.rsplit_once('/').map(|(head, _)| head.to_string()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|c| c.rsplit('/').next().unwrap_or(&c).to_string())
            .collect();
        return Verdict::pass(format!(
            "{} item(s) with service=Proton in {}",
            p.proton_items.len(),
            collections.join(", ")
        ));
    }
    Verdict::skip("not_probed", "the keyring group was not selected")
}

// -------------------------------------------------------------- network group

fn networkmanager_running(p: &Probes) -> Verdict {
    if !p.networkmanager_active.taken {
        return Verdict::skip("not_probed", "the network group was not selected");
    }
    let word = p.networkmanager_active.stdout.trim();
    if word == "active" {
        return Verdict::pass("NetworkManager is active");
    }
    Verdict::fail(
        "inactive",
        format!(
            "NetworkManager is '{}'; fproton drives every tunnel through it",
            if word.is_empty() { "unknown" } else { word }
        ),
    )
    .cmd("systemctl status NetworkManager")
}

fn wireguard_available(p: &Probes) -> Verdict {
    if !p.wireguard_module.taken {
        return Verdict::skip("not_probed", "the network group was not selected");
    }
    if p.wireguard_module.exists {
        return Verdict::pass("/sys/module/wireguard is present");
    }
    Verdict::fail(
        "module_missing",
        "the kernel wireguard module is not loaded; a Proton tunnel cannot come up",
    )
    .cmd("sudo modprobe wireguard")
}

/// Parse the terse nmcli listing the same way `fproton_core::nm` does.
fn nm_rows(p: &Probes) -> Option<Vec<fproton_core::nm::NmConnection>> {
    if !p.nmcli_connections.taken || !p.nmcli_connections.success() {
        return None;
    }
    fproton_core::nm::parse_connections(&p.nmcli_connections.stdout).ok()
}

fn conflicting_tunnel(p: &Probes) -> Verdict {
    let Some(rows) = nm_rows(p) else {
        return Verdict::skip(
            "no_nmcli",
            if p.nmcli_connections.taken {
                format!(
                    "nmcli did not answer: {}",
                    p.nmcli_connections.failure_detail()
                )
            } else {
                "the network group was not selected".to_string()
            },
        );
    };
    let link = fproton_core::nm::link_state_from_connections(&rows);
    if link.conflicting_tunnels.is_empty() {
        return Verdict::pass("no other tunnel device is active");
    }
    // Up is not a conflict. Only a device owning a default route, or a Tailscale
    // exit node (a default in table 52), contests a Proton tunnel (§5, route.rs).
    if !p.ip_route_default.taken || !p.ip_route_table52.taken {
        return Verdict::skip(
            "no_ip_route",
            format!(
                "{} active, but `ip -j route` was not taken so default-route ownership is unknown",
                link.conflicting_tunnels.join(", ")
            ),
        );
    }
    let parse = |probe: &crate::doctor::probes::CmdProbe| -> Option<String> {
        probe.success().then(|| probe.stdout.clone())
    };
    let (Some(main), Some(t52)) = (parse(&p.ip_route_default), parse(&p.ip_route_table52)) else {
        return Verdict::skip(
            "ip_failed",
            format!(
                "`ip -j route` failed: {} / {}",
                p.ip_route_default.failure_detail(),
                p.ip_route_table52.failure_detail()
            ),
        );
    };
    let facts = fproton_core::route::RouteFacts {
        main_default_devs: fproton_core::route::default_devs(&main).unwrap_or_default(),
        tailscale_exit_node: fproton_core::route::has_default(&t52).unwrap_or(false),
    };
    let conflicts = fproton_core::route::conflicts(&link.conflicting_tunnels, &facts);
    if conflicts.is_empty() {
        let tailnet = link
            .conflicting_tunnels
            .iter()
            .any(|d| d.starts_with("tailscale"));
        return Verdict::pass(if tailnet {
            format!(
                "{} is up without an exit node: it routes only the tailnet and coexists with a Proton tunnel (the kill switch, if on, will block direct peer traffic while connected)",
                link.conflicting_tunnels.join(", ")
            )
        } else {
            format!(
                "{} is up but owns no default route",
                link.conflicting_tunnels.join(", ")
            )
        });
    }
    let mut parts = Vec::new();
    let mut cmd = "fproton doctor --only network";
    for c in &conflicts {
        match c.reason {
            fproton_core::route::ConflictReason::ExitNode => {
                parts.push(format!(
                    "{} is using a Tailscale exit node, a second full VPN that will fight a Proton tunnel for the default route and DNS",
                    c.device
                ));
                cmd = "tailscale set --exit-node=";
            }
            fproton_core::route::ConflictReason::DefaultRoute => parts.push(format!(
                "{} owns the default route and will contest it with a Proton tunnel",
                c.device
            )),
        }
    }
    Verdict::warn("other_tunnel", parts.join("; ")).cmd(cmd)
}

/// Report-only, always: the profile carries the WireGuard key and fproton cannot
/// regenerate it (§1.3). The command is printed for the human, never run.
fn stale_proton_profiles(p: &Probes) -> Verdict {
    let Some(rows) = nm_rows(p) else {
        return Verdict::skip("no_nmcli", "nmcli did not answer");
    };
    let current = p
        .connection_persistence
        .text
        .as_deref()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| {
            v.get("connection_id")
                .and_then(|c| c.as_str())
                .map(str::to_string)
        });
    let stale: Vec<&fproton_core::nm::NmConnection> = rows
        .iter()
        .filter(|c| c.is_proton_profile() && !c.active && Some(&c.uuid) != current.as_ref())
        .collect();
    if stale.is_empty() {
        return Verdict::pass(format!(
            "{} ProtonVPN profile(s), none stale",
            rows.iter().filter(|c| c.is_proton_profile()).count()
        ));
    }
    let names: Vec<&str> = stale.iter().map(|c| c.name.as_str()).collect();
    let cmds: Vec<String> = stale
        .iter()
        .map(|c| format!("nmcli connection delete '{}'", c.name))
        .collect();
    let detail = format!(
        "{} inactive ProtonVPN profile(s) the last connection does not name: {}",
        stale.len(),
        names.join(", ")
    );
    // ≥ 3 is the reporting threshold (§7); fewer is normal accumulation.
    if stale.len() >= 3 {
        Verdict::warn("accumulated", detail).cmd(cmds.join(" && "))
    } else {
        Verdict::pass(detail).cmd(cmds.join(" && "))
    }
}

/// The only fproton mutation that can expose traffic, so all four preconditions of
/// §7 must hold before it is even offered.
fn killswitch_orphan(p: &Probes) -> Verdict {
    let Some(rows) = nm_rows(p) else {
        return Verdict::skip("no_nmcli", "nmcli did not answer");
    };
    let link = fproton_core::nm::link_state_from_connections(&rows);
    if !link.killswitch_active {
        return Verdict::pass("the IPv6 leak guard is down");
    }
    if link.up {
        return Verdict::pass("the leak guard is up alongside a live tunnel, as it should be");
    }
    let mut missing: Vec<String> = Vec::new();
    if p.cached_kill_switch.is_none() {
        missing.push("no cached Proton kill-switch setting (run `fproton config list`)".into());
    }
    if p.pending.is_some() {
        missing.push("an fproton action is in flight".into());
    }
    if p.last_connect_sec_ago.is_some_and(|s| s < 60) {
        missing.push("a connect finished less than 60 s ago".into());
    }
    match p.killswitch_orphan_seen_sec_ago {
        None => missing.push("this is the first run that has seen it".into()),
        Some(s) if s < 30 => missing.push(format!("first seen only {s} s ago")),
        Some(_) => {}
    }
    let base = format!(
        "{} is active while {} is down",
        fproton_core::KILLSWITCH_PROFILE,
        fproton_core::TUNNEL_DEVICE
    );
    if missing.is_empty() {
        Verdict::warn(
            "orphan",
            format!("{base}; taking it down removes an IPv6 leak guard"),
        )
        .fixable(
            PlanKind::KillswitchDown,
            "fproton doctor --fix --yes --only network",
        )
    } else {
        Verdict::warn(
            "orphan_unsafe",
            format!("{base}; reported only: {}", missing.join("; ")),
        )
        .cmd(format!(
            "nmcli connection down {}",
            fproton_core::KILLSWITCH_PROFILE
        ))
    }
}

// ---------------------------------------------------------------- cache group

fn serverlist_present(p: &Probes) -> Verdict {
    if !p.serverlist.taken {
        return Verdict::skip("not_probed", "the cache group was not selected");
    }
    if !p.serverlist.exists {
        return Verdict::fail(
            "missing",
            format!(
                "{} is absent; only the official CLI writes it",
                p.serverlist.path
            ),
        )
        .cmd("protonvpn connect   # the CLI refreshes its own cache");
    }
    if let Some(e) = &p.serverlist.parse_error {
        return Verdict::fail(
            "unparsable",
            format!(
                "{} ({} bytes) does not parse: {e}",
                p.serverlist.path, p.serverlist.size
            ),
        );
    }
    let n = p.serverlist.logicals.unwrap_or(0);
    if n < SERVERLIST_MIN_LOGICALS {
        return Verdict::fail(
            "too_small",
            format!("only {n} logical servers (a real list has ≥ {SERVERLIST_MIN_LOGICALS})"),
        );
    }
    Verdict::pass(format!(
        "{n} logical servers, {} bytes, MaxTier {}",
        p.serverlist.size,
        p.serverlist
            .max_tier
            .map(|t| t.to_string())
            .unwrap_or_else(|| "unknown".into())
    ))
}

fn serverlist_expired(p: &Probes) -> Verdict {
    if !p.serverlist.taken {
        return Verdict::skip("not_probed", "the cache group was not selected");
    }
    if !p.serverlist.exists {
        return Verdict::skip("no_serverlist", "there is no server list to date");
    }
    let Some(expires) = p.serverlist.expires_unix else {
        return Verdict::skip(
            "no_expiry",
            "the list carries no ExpirationTime; nothing to judge",
        );
    };
    let age = p.now_unix - expires;
    if age <= 0 {
        return Verdict::pass(format!("valid for another {} min", (-age) / 60));
    }
    // Proton's client refreshes the list at every connect, so a list a few hours past
    // its expiry is the normal state between connects. An expired list is served, not
    // refused (§4 `index`); only a day-old one is worth a warning.
    if age <= 86_400 {
        return Verdict::pass(format!(
            "expired {} h {} min ago; still served, and the next connect refreshes it",
            age / 3600,
            (age % 3600) / 60
        ));
    }
    Verdict::warn(
        "expired",
        format!(
            "expired {} h {} min ago; fproton still serves it, only the CLI can refresh it",
            age / 3600,
            (age % 3600) / 60
        ),
    )
    .cmd("protonvpn connect   # the CLI refreshes its own cache")
}

fn serverlist_loads_stale(p: &Probes) -> Verdict {
    if !p.serverlist.taken {
        return Verdict::skip("not_probed", "the cache group was not selected");
    }
    let Some(expires) = p.serverlist.loads_expire_unix else {
        return Verdict::skip("no_expiry", "the list carries no LoadsExpirationTime");
    };
    let age = p.now_unix - expires;
    if age <= 0 {
        return Verdict::pass(format!("loads current for another {} min", (-age) / 60));
    }
    // Normal between connects: Proton's client refreshes loads only while it runs.
    // Information, never a finding.
    Verdict::pass(
        format!(
            "per-server loads went stale {} h ago; `load` columns are last known, refreshed by the next connect",
            age / 3600
        ),
    )
}

fn index_fresh(p: &Probes) -> Verdict {
    if !p.index.taken {
        return Verdict::skip("not_probed", "the cache group was not selected");
    }
    match p.index.freshness.as_str() {
        "fresh" => Verdict::pass(format!(
            "{} servers, built from the source's current mtime",
            p.index.servers.unwrap_or(0)
        )),
        "source_missing" => Verdict::skip(
            "no_serverlist",
            "Proton's serverlist.json is absent; the snapshot cannot be judged",
        ),
        "missing" => Verdict::warn(
            "missing",
            format!("no snapshot at {}; queries will build one", p.index.path),
        )
        .fixable(PlanKind::RebuildIndex, "fproton index --rebuild"),
        _ => Verdict::warn(
            "stale",
            format!(
                "the snapshot was built from an older serverlist.json (built from {})",
                p.index
                    .built_from_mtime
                    .clone()
                    .unwrap_or_else(|| "unknown".into())
            ),
        )
        .fixable(PlanKind::RebuildIndex, "fproton index --rebuild"),
    }
}

// -------------------------------------------------------------- install group

fn state_dir(p: &Probes) -> Verdict {
    if !p.state_dir.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if !p.state_dir.exists {
        return Verdict::fail("missing", format!("{} does not exist", p.state_dir.path)).fixable(
            PlanKind::CreateStateDir,
            "fproton doctor --fix --yes --only install",
        );
    }
    if !p.state_dir.is_dir {
        return Verdict::fail(
            "not_a_directory",
            format!("{} exists but is not a directory", p.state_dir.path),
        )
        .cmd(format!("ls -l {}", p.state_dir.path));
    }
    if !p.state_dir.writable {
        return Verdict::fail(
            "not_writable",
            format!("{} is not writable by this user", p.state_dir.path),
        )
        .cmd(format!("ls -ld {}", p.state_dir.path));
    }
    if p.state_json.exists {
        let parses = p
            .state_json
            .text
            .as_deref()
            .map(|t| serde_json::from_str::<serde_json::Value>(t).is_ok())
            .unwrap_or(false);
        if !parses {
            return Verdict::fail(
                "unparsable_state",
                format!(
                    "{} does not parse as JSON; every field in it is a cache",
                    p.state_json.path
                ),
            )
            .fixable(
                PlanKind::ResetStateJson,
                "fproton doctor --fix --yes --only install",
            );
        }
    }
    Verdict::pass(format!("{} is writable", p.state_dir.path))
}

fn stale_pending_action(p: &Probes) -> Verdict {
    if !p.state_dir.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    let Some(pending) = &p.pending else {
        return Verdict::pass("no action is recorded as in flight");
    };
    if pending.is_live() {
        return Verdict::pass(format!(
            "{} {} is in flight ({}s of {}s)",
            pending.action.name(),
            pending.target.clone().unwrap_or_else(|| "-".into()),
            pending.elapsed_sec,
            pending.deadline_sec
        ));
    }
    Verdict::warn(
        "stale_pending",
        format!(
            "{} {} has been recorded for {}s against a {}s deadline; the run was killed or crashed",
            pending.action.name(),
            pending.target.clone().unwrap_or_else(|| "-".into()),
            pending.elapsed_sec,
            pending.deadline_sec
        ),
    )
    .fixable(
        PlanKind::ClearPending,
        "fproton doctor --fix --yes --only install",
    )
}

fn settings_valid(p: &Probes) -> Verdict {
    if !p.state_dir.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if p.settings_problems.is_empty() {
        return Verdict::pass(if p.settings_file.exists {
            format!("{} holds values fproton accepts", p.settings_file.path)
        } else {
            "no config.toml; every setting is at its default".to_string()
        });
    }
    Verdict::warn("invalid", p.settings_problems.join("; ")).fixable(
        PlanKind::NormaliseSettings,
        "fproton doctor --fix --yes --only install",
    )
}

fn binary_on_path(p: &Probes) -> Verdict {
    if !p.state_dir.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    match (&p.which_fproton, p.local_bin_fproton_exists) {
        (Some(found), true) if found == &p.local_bin_fproton => {
            Verdict::pass(format!("{found} is first on PATH"))
        }
        (Some(found), true) => Verdict::warn(
            "shadowed",
            format!(
                "PATH finds {found}, but the Omarchy shell's PATH only has {}; \
                 the bar widget would run a different binary",
                p.local_bin_fproton
            ),
        )
        .cmd(format!("cp {found} {}", p.local_bin_fproton)),
        (None, true) => Verdict::warn(
            "not_on_path",
            format!(
                "{} exists but PATH does not find an `fproton` at all",
                p.local_bin_fproton
            ),
        )
        .cmd("export PATH=\"$HOME/.local/bin:$PATH\""),
        (_, false) => Verdict::warn(
            "not_installed",
            format!(
                "{} is absent; the Omarchy shell's PATH has ~/.local/bin but not ~/.cargo/bin",
                p.local_bin_fproton
            ),
        )
        .cmd("scripts/install.sh --yes"),
    }
}

fn plugin_linked(p: &Probes) -> Verdict {
    if !p.plugin_link.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if !p.plugin_link.exists {
        return Verdict::warn("missing", format!("{} does not exist", p.plugin_link.path))
            .fixable(PlanKind::LinkPlugin, "fproton setup --plugin --yes");
    }
    match &p.plugin_link.resolved {
        Some(resolved) if resolved == &p.plugin_link.want_target => {
            Verdict::pass(format!("{} → {resolved}", p.plugin_link.path))
        }
        Some(resolved) => Verdict::warn(
            "elsewhere",
            format!(
                "{} resolves to {resolved}, not to this repo's {}",
                p.plugin_link.path, p.plugin_link.want_target
            ),
        )
        .cmd(format!("ls -l {}", p.plugin_link.path)),
        None => Verdict::warn(
            "dangling",
            format!("{} does not resolve to anything", p.plugin_link.path),
        )
        .fixable(PlanKind::LinkPlugin, "fproton setup --plugin --yes"),
    }
}

/// The `STATE` column of `omarchy plugin list` for one id.
fn plugin_state<'a>(listing: &'a str, id: &str) -> Option<&'a str> {
    listing.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == id).then(|| fields.next().unwrap_or(""))
    })
}

fn plugin_enabled(p: &Probes) -> Verdict {
    if !p.omarchy_plugin_list.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if !p.omarchy_plugin_list.success() {
        return Verdict::skip(
            "no_omarchy",
            format!(
                "`omarchy plugin list`: {}",
                p.omarchy_plugin_list.failure_detail()
            ),
        );
    }
    match plugin_state(&p.omarchy_plugin_list.stdout, super::probes::PLUGIN_ID) {
        Some("enabled") => Verdict::pass(format!("{} is enabled", super::probes::PLUGIN_ID)),
        Some(other) => Verdict::warn(
            "disabled",
            format!("{} is '{other}'", super::probes::PLUGIN_ID),
        )
        .fixable(PlanKind::EnablePlugin, "fproton setup --plugin --yes"),
        None => Verdict::warn(
            "not_listed",
            format!(
                "{} is not in `omarchy plugin list`; link it first",
                super::probes::PLUGIN_ID
            ),
        )
        .fixable(PlanKind::EnablePlugin, "fproton setup --plugin --yes"),
    }
}

fn menu_entries(p: &Probes) -> Verdict {
    if !p.menu_file.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if p.menu_missing_ids.is_empty() {
        return Verdict::pass(if p.menu_file.exists {
            format!("every fproton entry is already in {}", p.menu_file.path)
        } else {
            "no menu entries to add".to_string()
        });
    }
    Verdict::warn(
        "missing_entries",
        format!(
            "{} of fproton's menu entries are absent from {}: {}",
            p.menu_missing_ids.len(),
            p.menu_file.path,
            p.menu_missing_ids.join(", ")
        ),
    )
    .fixable(PlanKind::MergeMenu, "fproton setup --menu --yes")
}

fn old_plugin_enabled(p: &Probes) -> Verdict {
    if !p.omarchy_plugin_list.taken {
        return Verdict::skip("not_probed", "the install group was not selected");
    }
    if !p.omarchy_plugin_list.success() {
        return Verdict::skip("no_omarchy", "`omarchy plugin list` did not answer");
    }
    match plugin_state(&p.omarchy_plugin_list.stdout, super::probes::OLD_PLUGIN_ID) {
        Some("enabled") => Verdict::warn(
            "still_enabled",
            format!(
                "{} is still enabled and will fight fproton for the bar slot",
                super::probes::OLD_PLUGIN_ID
            ),
        )
        .fixable(
            PlanKind::DisableOldPlugin,
            "fproton setup --disable-old-plugin --yes",
        ),
        Some(other) => Verdict::pass(format!("{} is '{other}'", super::probes::OLD_PLUGIN_ID)),
        None => Verdict::pass("the replaced plugin is not installed"),
    }
}

// ------------------------------------------------------------ connected group

fn dns_when_connected(p: &Probes) -> Verdict {
    if !p.resolvectl_dns.taken {
        return Verdict::skip(
            "link_down",
            format!(
                "{} does not exist; this check needs a live tunnel and has never run on this machine",
                fproton_core::TUNNEL_DEVICE
            ),
        );
    }
    if !p.resolvectl_dns.success() {
        return Verdict::fail(
            "resolvectl_failed",
            format!("`resolvectl dns`: {}", p.resolvectl_dns.failure_detail()),
        );
    }
    if p.resolvectl_dns.stdout.contains("10.2.0.1") {
        return Verdict::pass(format!(
            "{} uses Proton's resolver 10.2.0.1",
            fproton_core::TUNNEL_DEVICE
        ));
    }
    Verdict::fail(
        "wrong_dns",
        format!(
            "{} does not list 10.2.0.1: {}",
            fproton_core::TUNNEL_DEVICE,
            p.resolvectl_dns.stdout.trim()
        ),
    )
    .cmd("resolvectl status proton0")
}

fn tunnel_route_ok(p: &Probes) -> Verdict {
    if !p.ip_route.taken {
        return Verdict::skip(
            "link_down",
            format!(
                "{} does not exist; this check needs a live tunnel and has never run on this machine",
                fproton_core::TUNNEL_DEVICE
            ),
        );
    }
    if !p.ip_route.success() {
        return Verdict::fail(
            "ip_failed",
            format!("`ip route get 1.1.1.1`: {}", p.ip_route.failure_detail()),
        );
    }
    let line = p.ip_route.stdout.lines().next().unwrap_or("").trim();
    if line.contains(&format!("dev {}", fproton_core::TUNNEL_DEVICE)) {
        return Verdict::pass(format!(
            "1.1.1.1 routes via {}",
            fproton_core::TUNNEL_DEVICE
        ));
    }
    Verdict::fail(
        "route_outside",
        format!("1.1.1.1 does not route through the tunnel: {line}"),
    )
    .cmd("ip route get 1.1.1.1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::probes::{CmdProbe, DirProbe, FileProbe, IndexFacts, LinkProbe, Probes};

    fn cmd(stdout: &str, code: i32) -> CmdProbe {
        CmdProbe {
            argv: vec!["x".into()],
            code: Some(code),
            timed_out: false,
            duration_ms: 3,
            stdout: stdout.to_string(),
            stderr: String::new(),
            error: None,
            taken: true,
        }
    }

    fn install_probes() -> Probes {
        Probes {
            state_dir: DirProbe {
                path: "/tmp/s".into(),
                exists: true,
                is_dir: true,
                writable: true,
                taken: true,
            },
            omarchy_plugin_list: cmd("", 0),
            plugin_link: LinkProbe {
                path: "/l".into(),
                want_target: "/repo".into(),
                taken: true,
                ..LinkProbe::default()
            },
            menu_file: FileProbe {
                path: "/m".into(),
                exists: true,
                taken: true,
                ..FileProbe::default()
            },
            ..Probes::default()
        }
    }

    /// The one-line predicates §7 does not give a fixture: field value → status.
    #[test]
    fn the_table_of_one_line_predicates() {
        struct Case {
            name: &'static str,
            probes: Probes,
            id: &'static str,
            want: CheckStatus,
            want_reason: Option<&'static str>,
        }
        let cases = vec![
            Case {
                name: "protonvpn on PATH",
                probes: Probes {
                    which_protonvpn: Some("/usr/bin/protonvpn".into()),
                    pacman_proton: cmd("proton-vpn-cli 1.0.3-1\n", 0),
                    ..Probes::default()
                },
                id: "cli_installed",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "protonvpn absent",
                probes: Probes::default(),
                id: "cli_installed",
                want: CheckStatus::Fail,
                want_reason: Some("not_installed"),
            },
            Case {
                name: "--help is fast",
                probes: Probes {
                    cli_help: cmd("Usage: protonvpn\n", 0),
                    ..Probes::default()
                },
                id: "cli_help_fast",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "--help was killed",
                probes: Probes {
                    cli_help: CmdProbe {
                        timed_out: true,
                        taken: true,
                        duration_ms: 3000,
                        ..CmdProbe::default()
                    },
                    ..Probes::default()
                },
                id: "cli_help_fast",
                want: CheckStatus::Fail,
                want_reason: Some("help_hang"),
            },
            Case {
                name: "daemon active",
                probes: Probes {
                    daemon_active: cmd("active\n", 0),
                    ..Probes::default()
                },
                id: "daemon_active",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "daemon inactive",
                probes: Probes {
                    daemon_active: cmd("inactive\n", 3),
                    ..Probes::default()
                },
                id: "daemon_active",
                want: CheckStatus::Warn,
                want_reason: Some("inactive"),
            },
            Case {
                name: "NetworkManager active",
                probes: Probes {
                    networkmanager_active: cmd("active\n", 0),
                    ..Probes::default()
                },
                id: "networkmanager_running",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "NetworkManager down",
                probes: Probes {
                    networkmanager_active: cmd("failed\n", 3),
                    ..Probes::default()
                },
                id: "networkmanager_running",
                want: CheckStatus::Fail,
                want_reason: Some("inactive"),
            },
            Case {
                name: "wireguard loaded",
                probes: Probes {
                    wireguard_module: FileProbe {
                        path: "/sys/module/wireguard".into(),
                        exists: true,
                        taken: true,
                        ..FileProbe::default()
                    },
                    ..Probes::default()
                },
                id: "wireguard_available",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "wireguard missing",
                probes: Probes {
                    wireguard_module: FileProbe {
                        path: "/sys/module/wireguard".into(),
                        taken: true,
                        ..FileProbe::default()
                    },
                    ..Probes::default()
                },
                id: "wireguard_available",
                want: CheckStatus::Fail,
                want_reason: Some("module_missing"),
            },
            Case {
                name: "secrets on the bus",
                probes: Probes {
                    busctl_list: cmd(
                        "org.freedesktop.secrets 1056004 gnome-keyring-d oliver :1.2 - -\n",
                        0,
                    ),
                    ..Probes::default()
                },
                id: "secret_service_on_bus",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "nobody owns the secrets name",
                probes: Probes {
                    busctl_list: cmd("org.freedesktop.systemd1 1 systemd oliver :1.0 - -\n", 0),
                    ..Probes::default()
                },
                id: "secret_service_on_bus",
                want: CheckStatus::Fail,
                want_reason: Some("no_provider"),
            },
            Case {
                name: "index fresh",
                probes: Probes {
                    index: IndexFacts {
                        freshness: "fresh".into(),
                        servers: Some(18221),
                        taken: true,
                        ..IndexFacts::default()
                    },
                    ..Probes::default()
                },
                id: "index_fresh",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "index stale",
                probes: Probes {
                    index: IndexFacts {
                        freshness: "stale".into(),
                        taken: true,
                        ..IndexFacts::default()
                    },
                    ..Probes::default()
                },
                id: "index_fresh",
                want: CheckStatus::Warn,
                want_reason: Some("stale"),
            },
            Case {
                name: "state dir missing",
                probes: Probes {
                    state_dir: DirProbe {
                        path: "/tmp/nope".into(),
                        taken: true,
                        ..DirProbe::default()
                    },
                    ..Probes::default()
                },
                id: "state_dir",
                want: CheckStatus::Fail,
                want_reason: Some("missing"),
            },
            Case {
                name: "state.json is not JSON",
                probes: Probes {
                    state_json: FileProbe {
                        path: "/tmp/s/state.json".into(),
                        exists: true,
                        text: Some("{oops".into()),
                        taken: true,
                        ..FileProbe::default()
                    },
                    ..install_probes()
                },
                id: "state_dir",
                want: CheckStatus::Fail,
                want_reason: Some("unparsable_state"),
            },
            Case {
                name: "no pending action",
                probes: install_probes(),
                id: "stale_pending_action",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "binary not installed",
                probes: install_probes(),
                id: "binary_on_path",
                want: CheckStatus::Warn,
                want_reason: Some("not_installed"),
            },
            Case {
                name: "plugin not linked",
                probes: install_probes(),
                id: "plugin_linked",
                want: CheckStatus::Warn,
                want_reason: Some("missing"),
            },
            Case {
                name: "plugin not listed",
                probes: install_probes(),
                id: "plugin_enabled",
                want: CheckStatus::Warn,
                want_reason: Some("not_listed"),
            },
            Case {
                name: "old plugin disabled",
                probes: Probes {
                    omarchy_plugin_list: cmd(
                        "io.github.grichard99.omaproton-vpn disabled third-party bar-widget X\n",
                        0,
                    ),
                    ..install_probes()
                },
                id: "old_plugin_enabled",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "old plugin still enabled",
                probes: Probes {
                    omarchy_plugin_list: cmd(
                        "io.github.grichard99.omaproton-vpn enabled third-party bar-widget X\n",
                        0,
                    ),
                    ..install_probes()
                },
                id: "old_plugin_enabled",
                want: CheckStatus::Warn,
                want_reason: Some("still_enabled"),
            },
            Case {
                name: "dns check skips without a tunnel",
                probes: Probes::default(),
                id: "dns_when_connected",
                want: CheckStatus::Skip,
                want_reason: Some("link_down"),
            },
            Case {
                name: "route check skips without a tunnel",
                probes: Probes::default(),
                id: "tunnel_route_ok",
                want: CheckStatus::Skip,
                want_reason: Some("link_down"),
            },
            Case {
                name: "route through the tunnel",
                probes: Probes {
                    ip_route: cmd("1.1.1.1 dev proton0 src 10.2.0.2 uid 1000\n", 0),
                    ..Probes::default()
                },
                id: "tunnel_route_ok",
                want: CheckStatus::Pass,
                want_reason: None,
            },
            Case {
                name: "route outside the tunnel",
                probes: Probes {
                    ip_route: cmd("1.1.1.1 via 192.168.1.1 dev wlp0s20f3 src 192.168.1.5\n", 0),
                    ..Probes::default()
                },
                id: "tunnel_route_ok",
                want: CheckStatus::Fail,
                want_reason: Some("route_outside"),
            },
        ];

        for case in cases {
            let def = CHECKS
                .iter()
                .find(|c| c.id == case.id)
                .unwrap_or_else(|| panic!("no check {}", case.id));
            let v = (def.detect)(&case.probes);
            assert_eq!(
                v.status, case.want,
                "{}: {} said {:?} ({})",
                case.name, case.id, v.status, v.detail
            );
            assert_eq!(v.reason, case.want_reason, "{}: reason", case.name);
            assert!(!v.detail.is_empty(), "{}: empty detail", case.name);
        }
    }

    /// The nine detectors §7 requires a captured-input fixture for, plus the
    /// variants whose branch cannot be produced on this machine without changing
    /// the desktop. Each directory's `input/` is fed through the *same* parsers the
    /// live probe uses, so this exercises the real parse path, not a re-implementation.
    #[test]
    fn every_fixture_directory_produces_the_verdict_it_declares() {
        #[derive(serde::Deserialize)]
        struct Expect {
            check: String,
            status: String,
            reason: Option<String>,
            fixable: bool,
            #[serde(default)]
            detail_contains: Vec<String>,
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("repo root")
            .join("tests/fixtures/doctor");
        let mut seen: Vec<String> = Vec::new();
        let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
            .unwrap_or_else(|e| panic!("no {}: {e}", root.display()))
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        assert!(
            !dirs.is_empty(),
            "no doctor fixtures under {}",
            root.display()
        );

        for dir in dirs {
            let name = dir.file_name().unwrap().to_string_lossy().to_string();
            let expect: Expect = serde_json::from_str(
                &std::fs::read_to_string(dir.join("expect.json"))
                    .unwrap_or_else(|e| panic!("{name}/expect.json: {e}")),
            )
            .unwrap_or_else(|e| panic!("{name}/expect.json: {e}"));
            let probes = crate::doctor::probes::from_fixture_dir(&dir.join("input"));
            let def = CHECKS
                .iter()
                .find(|c| c.id == expect.check)
                .unwrap_or_else(|| panic!("{name}: no check {}", expect.check));
            let v = (def.detect)(&probes);
            assert_eq!(v.status.name(), expect.status, "{name}: {}", v.detail);
            assert_eq!(
                v.reason.map(str::to_string),
                expect.reason,
                "{name}: {}",
                v.detail
            );
            assert_eq!(v.plan.is_some(), expect.fixable, "{name}: fixable");
            for needle in &expect.detail_contains {
                assert!(
                    v.detail.contains(needle.as_str()),
                    "{name}: detail is missing '{needle}':\n{}",
                    v.detail
                );
            }
            seen.push(expect.check);
        }

        // §7: every detector that parses structured text has a fixture.
        for id in [
            "cli_session_probe",
            "secret_service_default_alias",
            "proton_session_item",
            "conflicting_tunnel",
            "stale_proton_profiles",
            "killswitch_orphan",
            "serverlist_present",
            "serverlist_expired",
            "settings_valid",
        ] {
            assert!(seen.iter().any(|s| s == id), "no fixture covers {id}");
        }
    }

    #[test]
    fn every_check_id_is_unique_and_every_group_is_covered() {
        let mut ids: Vec<&str> = CHECKS.iter().map(|c| c.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate check id");
        for group in super::super::GROUPS {
            assert!(
                CHECKS.iter().any(|c| c.group == *group),
                "no check in group {}",
                group.name()
            );
        }
    }

    #[test]
    fn only_class_a_b_and_c_checks_ever_name_a_plan() {
        // A Class D check may print a `fix_cmd` for a human but must never be fixable.
        let probes = Probes::default();
        for def in CHECKS {
            let v = (def.detect)(&probes);
            if def.class == Class::D {
                assert!(v.plan.is_none(), "{} is class D but named a plan", def.id);
            }
        }
    }

    #[test]
    fn a_detector_is_pure_over_its_probes() {
        let probes = Probes::default();
        for def in CHECKS {
            let a = (def.detect)(&probes);
            let b = (def.detect)(&probes);
            assert_eq!(a.status, b.status, "{} is not deterministic", def.id);
            assert_eq!(a.detail, b.detail, "{} is not deterministic", def.id);
        }
    }
}
