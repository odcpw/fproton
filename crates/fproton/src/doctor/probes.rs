//! The **only** place the doctor spawns a process, reads a file or talks to D-Bus
//! (DESIGN.md §7). Everything here produces captured bytes; `checks.rs` is a set of
//! pure functions over the result.
//!
//! Every probe is bounded: commands run under `fproton_core::proc::run_with_deadline`
//! (its own process group, SIGTERM then SIGKILL), files are read with their size
//! recorded, and the 24 MB serverlist is reduced to counts — never carried as bytes.
//!
//! A probe never fails a run. A command that cannot be spawned, times out or errors
//! is recorded as such and the detector decides what that means.

use std::path::Path;
use std::time::{Duration, Instant};

use fproton_core::{index, proc, secrets, settings, state, status, AliasProbe, Context};
use serde::{Deserialize, Serialize};

use super::DoctorGroup;

/// `protonvpn config list`'s deadline in the `cli_session_probe` check (§7).
pub const SESSION_PROBE_SEC: u64 = 8;
/// `protonvpn --help` must answer inside this (§7; it is 0.4 s on this machine).
pub const HELP_FAST_SEC: u64 = 3;
/// A probe artifact is truncated at 64 KiB with a trailer (§7).
pub const PROBE_TRUNCATE: usize = 64 * 1024;
/// Below this many logical servers the serverlist is not a real list (§7).
pub const SERVERLIST_MIN_LOGICALS: u64 = 1_000;

/// One captured external command.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CmdProbe {
    pub argv: Vec<String>,
    pub code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// Set when the command could not be spawned at all (missing binary, …).
    pub error: Option<String>,
    /// False when this probe was not taken because its group was not selected.
    pub taken: bool,
}

impl CmdProbe {
    /// Exit 0, not killed, spawned.
    pub fn success(&self) -> bool {
        self.taken && self.error.is_none() && !self.timed_out && self.code == Some(0)
    }
    /// The most useful one-line explanation of a non-success.
    pub fn failure_detail(&self) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        if self.timed_out {
            return format!("killed at its deadline after {} ms", self.duration_ms);
        }
        let tail = self
            .stderr
            .lines()
            .chain(self.stdout.lines())
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no output")
            .trim()
            .to_string();
        match self.code {
            Some(c) => format!("exit {c}: {tail}"),
            None => format!("killed by a signal: {tail}"),
        }
    }
    /// A not-taken placeholder, so a detector can say `skip` rather than guess.
    #[cfg(test)]
    pub fn untaken() -> CmdProbe {
        CmdProbe::default()
    }
}

/// One captured file. `text` is `None` for a file the doctor deliberately does not read.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileProbe {
    pub path: String,
    pub exists: bool,
    pub len: u64,
    /// RFC 3339 with local offset.
    pub mtime: Option<String>,
    pub text: Option<String>,
    pub error: Option<String>,
    pub taken: bool,
}

impl FileProbe {
    fn read(path: &Path) -> FileProbe {
        let mut p = FileProbe {
            path: path.display().to_string(),
            taken: true,
            ..FileProbe::default()
        };
        match std::fs::metadata(path) {
            Ok(m) => {
                p.exists = true;
                p.len = m.len();
                p.mtime = m
                    .modified()
                    .ok()
                    .and_then(fproton_core::rfc3339_from_system_time);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return p,
            Err(e) => {
                p.error = Some(e.to_string());
                return p;
            }
        }
        match std::fs::read_to_string(path) {
            Ok(t) => p.text = Some(t),
            Err(e) => p.error = Some(e.to_string()),
        }
        p
    }

    /// Metadata only: used for `/sys/module/wireguard` and for anything whose body
    /// the doctor must not carry.
    fn stat(path: &Path) -> FileProbe {
        let mut p = FileProbe {
            path: path.display().to_string(),
            taken: true,
            ..FileProbe::default()
        };
        match std::fs::metadata(path) {
            Ok(m) => {
                p.exists = true;
                p.len = m.len();
                p.mtime = m
                    .modified()
                    .ok()
                    .and_then(fproton_core::rfc3339_from_system_time);
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => p.error = Some(e.to_string()),
            Err(_) => {}
        }
        p
    }
}

/// What `~/.config/omarchy/plugins/oliver.protonvpn` is, if anything.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LinkProbe {
    pub path: String,
    pub exists: bool,
    pub is_symlink: bool,
    /// `readlink(2)`, verbatim.
    pub target: Option<String>,
    /// The fully resolved path — what §7 says to validate.
    pub resolved: Option<String>,
    /// Where `setup --plugin` would point it.
    pub want_target: String,
    pub taken: bool,
}

/// One Secret Service collection, as seen over the bus. Never opens a keyring file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CollectionProbe {
    pub path: String,
    /// False when a property GET answers "Object does not exist at path".
    pub exported: bool,
    pub label: Option<String>,
    pub locked: Option<bool>,
    /// Item paths this collection returns for `service=Proton`.
    pub proton_items: Vec<String>,
}

impl CollectionProbe {
    /// The keyring file basename the `default` pointer file would need for this
    /// collection (`/…/collection/Default` → `Default`). Never opened, only named.
    pub fn keyring_basename(&self) -> Option<&str> {
        self.path.rsplit('/').next().filter(|s| !s.is_empty())
    }
}

/// Counts and timestamps from Proton's 24 MB list. **The body is never carried** (§7).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerlistFacts {
    pub path: String,
    pub exists: bool,
    pub size: u64,
    pub mtime: Option<String>,
    pub logicals: Option<u64>,
    pub max_tier: Option<u8>,
    /// Proton's `ExpirationTime` (Unix seconds).
    pub expires_unix: Option<i64>,
    /// Proton's `LoadsExpirationTime` (Unix seconds).
    pub loads_expire_unix: Option<i64>,
    pub parse_error: Option<String>,
    pub taken: bool,
}

/// Just the top-level keys the two cache checks need, so the 24 MB body is parsed
/// once and immediately dropped.
#[derive(Deserialize)]
struct RawTop {
    #[serde(rename = "LogicalServers")]
    logical_servers: Vec<serde::de::IgnoredAny>,
    #[serde(rename = "MaxTier")]
    max_tier: Option<u8>,
    #[serde(rename = "ExpirationTime")]
    expiration_time: Option<f64>,
    #[serde(rename = "LoadsExpirationTime")]
    loads_expiration_time: Option<f64>,
}

/// The pure half of the serverlist probe: the same function runs over the live 24 MB
/// file and over `tests/fixtures/doctor/serverlist_*/input/serverlist.json`.
pub fn serverlist_facts_from_bytes(
    path: &str,
    size: u64,
    mtime: Option<String>,
    bytes: &[u8],
) -> ServerlistFacts {
    let mut facts = ServerlistFacts {
        path: path.to_string(),
        exists: true,
        size,
        mtime,
        taken: true,
        ..ServerlistFacts::default()
    };
    match serde_json::from_slice::<RawTop>(bytes) {
        Ok(raw) => {
            facts.logicals = Some(raw.logical_servers.len() as u64);
            facts.max_tier = raw.max_tier;
            facts.expires_unix = raw.expiration_time.map(|t| t as i64);
            facts.loads_expire_unix = raw.loads_expiration_time.map(|t| t as i64);
        }
        Err(e) => facts.parse_error = Some(e.to_string()),
    }
    facts
}

/// What the snapshot looks like next to Proton's file (`index_fresh`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexFacts {
    pub path: String,
    pub exists: bool,
    pub size: u64,
    /// `fresh` · `missing` · `stale` · `source_missing`.
    pub freshness: String,
    pub servers: Option<u32>,
    pub built_from_mtime: Option<String>,
    pub taken: bool,
}

/// Is the fproton state directory usable?
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DirProbe {
    pub path: String,
    pub exists: bool,
    pub is_dir: bool,
    pub writable: bool,
    pub taken: bool,
}

/// Everything the detectors are allowed to see.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Probes {
    pub taken_at: String,
    pub groups: Vec<String>,

    // ---- cli
    pub which_protonvpn: Option<String>,
    pub pacman_proton: CmdProbe,
    pub cli_help: CmdProbe,
    pub cli_session: CmdProbe,
    pub daemon_active: CmdProbe,

    // ---- network
    pub networkmanager_active: CmdProbe,
    pub wireguard_module: FileProbe,
    pub nmcli_connections: CmdProbe,
    pub connection_persistence: FileProbe,
    pub resolvectl_dns: CmdProbe,
    pub ip_route: CmdProbe,

    // ---- keyring
    pub busctl_list: CmdProbe,
    pub busctl_readalias: CmdProbe,
    pub busctl_alias_property: CmdProbe,
    pub alias: Option<AliasProbe>,
    pub collections: Vec<CollectionProbe>,
    pub collections_error: Option<String>,
    pub proton_items: Vec<String>,
    pub proton_items_error: Option<String>,
    pub keyring_pointer: FileProbe,

    // ---- cache
    pub serverlist: ServerlistFacts,
    pub index: IndexFacts,

    // ---- install
    pub state_dir: DirProbe,
    pub state_json: FileProbe,
    pub pending: Option<status::Pending>,
    pub settings_file: FileProbe,
    pub settings_problems: Vec<String>,
    pub which_fproton: Option<String>,
    pub local_bin_fproton: String,
    pub local_bin_fproton_exists: bool,
    pub plugin_link: LinkProbe,
    pub omarchy_plugin_list: CmdProbe,
    pub menu_file: FileProbe,
    pub menu_missing_ids: Vec<String>,

    // ---- derived facts the killswitch_orphan preconditions need (§7)
    pub killswitch_orphan_seen_at: Option<String>,
    pub killswitch_orphan_seen_sec_ago: Option<u64>,
    pub last_connect_sec_ago: Option<u64>,
    pub cached_kill_switch: Option<String>,

    /// Now, in Unix seconds, so every time comparison is a pure function of the probes.
    pub now_unix: i64,
}

/// The plugin id fproton installs, and the one it replaces (§1.1).
pub const PLUGIN_ID: &str = "oliver.protonvpn";
pub const OLD_PLUGIN_ID: &str = "io.github.grichard99.omaproton-vpn";

fn run(argv: &[&str], deadline: Duration) -> CmdProbe {
    let started = Instant::now();
    let mut probe = CmdProbe {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        taken: true,
        ..CmdProbe::default()
    };
    match proc::run_with_deadline(argv, deadline, None) {
        Ok(out) => {
            probe.code = out.code;
            probe.timed_out = out.timed_out;
            probe.stdout = out.stdout_string();
            probe.stderr = out.stderr_string();
        }
        Err(e) => probe.error = Some(e.to_string()),
    }
    probe.duration_ms = started.elapsed().as_millis() as u64;
    probe
}

fn busctl(args: &[&str], deadline: Duration) -> CmdProbe {
    let mut argv = vec![secrets::PROGRAM, "--user", "--timeout=1"];
    argv.extend_from_slice(args);
    run(&argv, deadline)
}

/// `access(2)` rather than a touch-and-delete: a detect-only run must write nothing
/// at all, so the state dir is byte-identical (and mtime-identical) afterwards.
fn writable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path that lives across the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

impl Probes {
    /// Capture everything the selected groups need. Never returns an error: a probe
    /// that could not be taken is recorded as such.
    pub fn capture(ctx: &Context, only: Option<DoctorGroup>) -> Probes {
        let want = |g: DoctorGroup| only.is_none() || only == Some(g);
        let mut p = Probes {
            taken_at: fproton_core::now_rfc3339(),
            now_unix: unix_now(),
            groups: match only {
                Some(g) => vec![g.name().to_string()],
                None => super::GROUPS.iter().map(|g| g.name().to_string()).collect(),
            },
            ..Probes::default()
        };
        let nmcli_deadline = ctx.timeouts.nmcli();
        let bus_deadline = ctx.timeouts.busctl().max(Duration::from_secs(1));
        let probe_deadline = ctx.timeouts.probe();

        if want(DoctorGroup::Cli) {
            p.which_protonvpn =
                proc::which(fproton_core::cli::PROGRAM).map(|path| path.display().to_string());
            p.pacman_proton = run(&["pacman", "-Q", "proton-vpn-cli"], probe_deadline);
            p.cli_help = run(
                &[fproton_core::cli::PROGRAM, "--help"],
                probe_deadline.min(Duration::from_secs(HELP_FAST_SEC)),
            );
            // The one session-touching command the doctor runs, always bounded (§1.2).
            p.cli_session = run(
                &[fproton_core::cli::PROGRAM, "config", "list"],
                probe_deadline.min(Duration::from_secs(SESSION_PROBE_SEC)),
            );
            p.daemon_active = run(
                &["systemctl", "is-active", "proton.VPN.service"],
                probe_deadline,
            );
        }

        let needs_nm = want(DoctorGroup::Network) || want(DoctorGroup::Connected);
        if needs_nm {
            p.nmcli_connections = run(
                &[
                    fproton_core::nm::PROGRAM,
                    "-t",
                    "-f",
                    "NAME,UUID,TYPE,DEVICE,ACTIVE",
                    "connection",
                    "show",
                ],
                nmcli_deadline,
            );
        }
        if want(DoctorGroup::Network) {
            p.networkmanager_active = run(
                &["systemctl", "is-active", "NetworkManager"],
                probe_deadline,
            );
            p.wireguard_module = FileProbe::stat(Path::new("/sys/module/wireguard"));
            p.connection_persistence = FileProbe::read(&ctx.paths.connection_persistence());
        }
        if want(DoctorGroup::Connected) {
            // Both connected checks are `skip:link_down` unless the device exists (§4b RED).
            let device = fproton_core::TUNNEL_DEVICE;
            if ctx.paths.sysfs_device(device).exists() {
                p.resolvectl_dns = run(&["resolvectl", "dns", device], probe_deadline);
                p.ip_route = run(&["ip", "route", "get", "1.1.1.1"], probe_deadline);
            }
        }

        if want(DoctorGroup::Keyring) {
            p.busctl_list = run(&[secrets::PROGRAM, "--user", "list"], bus_deadline);
            p.capture_keyring(bus_deadline);
            // Metadata only: the pointer file's 16 bytes are a keyring *basename*,
            // never key material, and no `*.keyring` file is ever opened (§2).
            p.keyring_pointer = FileProbe::read(&ctx.paths.keyring_default_pointer());
        }

        if want(DoctorGroup::Cache) {
            p.serverlist = serverlist_probe(&ctx.paths.serverlist());
            let summary = index::summary(ctx);
            p.index = IndexFacts {
                path: ctx.paths.index_file().display().to_string(),
                exists: ctx.paths.index_file().exists(),
                size: std::fs::metadata(ctx.paths.index_file())
                    .map(|m| m.len())
                    .unwrap_or(0),
                freshness: freshness_name(summary.freshness).to_string(),
                servers: summary.info.servers,
                built_from_mtime: summary.info.built_from_mtime,
                taken: true,
            };
        }

        if want(DoctorGroup::Install) {
            p.capture_install(ctx, probe_deadline);
        }

        // Preconditions for the gated `killswitch_orphan` fixer (§7) — read from
        // fproton's own state, which is the only place they are recorded.
        if let Ok(st) = state::load(ctx) {
            p.pending = st.pending.clone();
            p.cached_kill_switch = st
                .config
                .as_ref()
                .and_then(|c| c.get("kill-switch").map(|s| s.to_string()));
            p.killswitch_orphan_seen_at = st.killswitch_orphan_seen_at.clone();
            p.killswitch_orphan_seen_sec_ago = st
                .killswitch_orphan_seen_at
                .as_deref()
                .and_then(state::seconds_since);
            p.last_connect_sec_ago = st
                .history
                .iter()
                .find(|h| {
                    matches!(
                        h.action,
                        state::HistoryAction::Connect | state::HistoryAction::Reconnect
                    )
                })
                .and_then(|h| state::seconds_since(&h.at));
        }
        p
    }

    fn capture_keyring(&mut self, deadline: Duration) {
        self.busctl_readalias = busctl(
            &[
                "call",
                secrets::SERVICE,
                secrets::OBJECT,
                secrets::IFACE_SERVICE,
                "ReadAlias",
                "s",
                "default",
            ],
            deadline,
        );
        let alias_path = if self.busctl_readalias.success() {
            secrets::parse_object_path(&self.busctl_readalias.stdout).filter(|p| p != "/")
        } else {
            None
        };
        if let Some(path) = &alias_path {
            self.busctl_alias_property = busctl(
                &[
                    "get-property",
                    secrets::SERVICE,
                    path,
                    secrets::IFACE_COLLECTION,
                    "Label",
                    "Locked",
                ],
                deadline,
            );
        }
        self.alias = Some(alias_from_raw(
            &self.busctl_readalias,
            &self.busctl_alias_property,
        ));

        match secrets::collections() {
            Ok(paths) => {
                for path in paths {
                    let prop = busctl(
                        &[
                            "get-property",
                            secrets::SERVICE,
                            &path,
                            secrets::IFACE_COLLECTION,
                            "Label",
                            "Locked",
                        ],
                        deadline,
                    );
                    let (label, locked) = if prop.success() {
                        secrets::parse_label_locked(&prop.stdout)
                    } else {
                        (None, None)
                    };
                    let (attr, value) = secrets::PROTON_ATTRIBUTE;
                    let search = busctl(
                        &[
                            "call",
                            secrets::SERVICE,
                            &path,
                            secrets::IFACE_COLLECTION,
                            "SearchItems",
                            "a{ss}",
                            "1",
                            attr,
                            value,
                        ],
                        deadline,
                    );
                    self.collections.push(CollectionProbe {
                        exported: prop.success(),
                        label,
                        locked,
                        proton_items: if search.success() {
                            secrets::parse_object_paths(&search.stdout)
                        } else {
                            Vec::new()
                        },
                        path,
                    });
                }
            }
            Err(e) => self.collections_error = Some(e.to_string()),
        }
        match secrets::search_proton_items() {
            Ok(items) => self.proton_items = items,
            Err(e) => self.proton_items_error = Some(e.to_string()),
        }
    }

    fn capture_install(&mut self, ctx: &Context, deadline: Duration) {
        let dir = &ctx.paths.state_dir;
        self.state_dir = DirProbe {
            path: dir.display().to_string(),
            exists: dir.exists(),
            is_dir: dir.is_dir(),
            writable: dir.is_dir() && writable(dir),
            taken: true,
        };
        self.state_json = FileProbe::read(&ctx.paths.state_file());
        self.settings_file = FileProbe::read(&ctx.paths.config_file());
        self.settings_problems = settings::validate(ctx).unwrap_or_default();
        self.which_fproton = proc::which(fproton_core::NAME).map(|p| p.display().to_string());
        let local = ctx.paths.local_bin.join(fproton_core::NAME);
        self.local_bin_fproton_exists = local.exists();
        self.local_bin_fproton = local.display().to_string();

        let link = ctx.paths.plugin_link();
        let meta = std::fs::symlink_metadata(&link).ok();
        self.plugin_link = LinkProbe {
            path: link.display().to_string(),
            exists: meta.is_some(),
            is_symlink: meta.map(|m| m.file_type().is_symlink()).unwrap_or(false),
            target: std::fs::read_link(&link)
                .ok()
                .map(|t| t.display().to_string()),
            resolved: std::fs::canonicalize(&link)
                .ok()
                .map(|t| t.display().to_string()),
            want_target: super::repo_plugin_dir().display().to_string(),
            taken: true,
        };
        self.omarchy_plugin_list = run(&["omarchy", "plugin", "list"], deadline);

        self.menu_file = FileProbe::read(&ctx.paths.omarchy_menu());
        let have = self
            .menu_file
            .text
            .as_deref()
            .map(crate::setup::ids_in_menu)
            .unwrap_or_default();
        self.menu_missing_ids = crate::setup::template_entries()
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| !have.contains(id))
            .collect();
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn freshness_name(f: index::Freshness) -> &'static str {
    match f {
        index::Freshness::Fresh => "fresh",
        index::Freshness::Missing => "missing",
        index::Freshness::Stale => "stale",
        index::Freshness::SourceMissing => "source_missing",
    }
}

/// Build the `AliasProbe` from the two raw busctl captures. The same function runs
/// over live output and over the fixtures, so the fixture tests the live parse.
pub fn alias_from_raw(readalias: &CmdProbe, property: &CmdProbe) -> AliasProbe {
    let reply = if readalias.success() {
        readalias.stdout.trim_end().to_string()
    } else {
        readalias.stderr.trim_end().to_string()
    };
    let path = if readalias.success() {
        secrets::parse_object_path(&reply).filter(|p| p != "/")
    } else {
        None
    };
    let mut probe = AliasProbe {
        alias: "default".to_string(),
        path,
        exported: false,
        label: None,
        locked: None,
        raw: reply,
    };
    if !property.taken {
        return probe;
    }
    let text = if property.success() {
        property.stdout.trim_end().to_string()
    } else {
        property.stderr.trim_end().to_string()
    };
    probe.raw = format!("{}\n{}", probe.raw, text);
    if property.success() {
        probe.exported = true;
        let (label, locked) = secrets::parse_label_locked(&text);
        probe.label = label;
        probe.locked = locked;
    }
    probe
}

fn serverlist_probe(path: &Path) -> ServerlistFacts {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return ServerlistFacts {
                path: path.display().to_string(),
                taken: true,
                ..ServerlistFacts::default()
            }
        }
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(fproton_core::rfc3339_from_system_time);
    match std::fs::read(path) {
        Ok(bytes) => {
            serverlist_facts_from_bytes(&path.display().to_string(), meta.len(), mtime, &bytes)
        }
        Err(e) => ServerlistFacts {
            path: path.display().to_string(),
            exists: true,
            size: meta.len(),
            mtime,
            parse_error: Some(e.to_string()),
            taken: true,
            ..ServerlistFacts::default()
        },
    }
}

// ------------------------------------------------------------------ artifacts

/// One probe artifact, as it is written to `runs/<id>/probes/<name>.txt`.
pub fn artifact_text(probe: &CmdProbe) -> String {
    let mut s = format!(
        "# argv: {}\n# exit: {} timed_out: {} duration_ms: {}\n",
        probe.argv.join(" "),
        probe
            .code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "none".into()),
        probe.timed_out,
        probe.duration_ms
    );
    if let Some(e) = &probe.error {
        s.push_str(&format!("# error: {e}\n"));
    }
    s.push_str("--- stdout ---\n");
    s.push_str(&probe.stdout);
    if !probe.stdout.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("--- stderr ---\n");
    s.push_str(&probe.stderr);
    if !probe.stderr.ends_with('\n') {
        s.push('\n');
    }
    truncate(s)
}

/// 64 KiB with a trailer naming what was dropped (§7).
pub fn truncate(mut s: String) -> String {
    if s.len() <= PROBE_TRUNCATE {
        return s;
    }
    let dropped = s.len() - PROBE_TRUNCATE;
    // Never split a UTF-8 character.
    let mut cut = PROBE_TRUNCATE;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push_str(&format!(
        "\n--- truncated by fproton doctor: {dropped} more bytes ---\n"
    ));
    s
}

/// Every command probe, with the name its artifact file takes.
pub fn named_commands(p: &Probes) -> Vec<(&'static str, &CmdProbe)> {
    vec![
        ("pacman-q-proton-vpn-cli", &p.pacman_proton),
        ("protonvpn-help", &p.cli_help),
        ("protonvpn-config-list", &p.cli_session),
        ("systemctl-is-active-proton", &p.daemon_active),
        (
            "systemctl-is-active-networkmanager",
            &p.networkmanager_active,
        ),
        ("nmcli-connections", &p.nmcli_connections),
        ("resolvectl-dns", &p.resolvectl_dns),
        ("ip-route-get", &p.ip_route),
        ("busctl-list", &p.busctl_list),
        ("busctl-readalias-default", &p.busctl_readalias),
        ("busctl-alias-property", &p.busctl_alias_property),
        ("omarchy-plugin-list", &p.omarchy_plugin_list),
    ]
    .into_iter()
    .filter(|(_, probe)| probe.taken)
    .collect()
}

// ------------------------------------------------------------------ fixtures

/// Load a `tests/fixtures/doctor/<check>/input/` directory into `Probes`.
///
/// Each file is **captured output**, read into the same field the live probe fills,
/// and parsed by the same functions — so a fixture exercises the real parse path.
/// `meta.json` carries the exit codes and deadline kills a text file cannot.
#[cfg(test)]
pub fn from_fixture_dir(dir: &Path) -> Probes {
    #[derive(Deserialize, Default)]
    struct Meta {
        #[serde(default)]
        codes: std::collections::BTreeMap<String, i32>,
        #[serde(default)]
        timed_out: Vec<String>,
        #[serde(default)]
        now_unix: Option<i64>,
        #[serde(default)]
        settings_problems: Vec<String>,
        #[serde(default)]
        killswitch_orphan_seen_sec_ago: Option<u64>,
        #[serde(default)]
        last_connect_sec_ago: Option<u64>,
        #[serde(default)]
        cached_kill_switch: Option<String>,
        #[serde(default)]
        pending_live: Option<bool>,
    }

    let meta: Meta = std::fs::read_to_string(dir.join("meta.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();

    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let cmd = |name: &str, argv: &[&str]| -> CmdProbe {
        match read(name) {
            None => CmdProbe::untaken(),
            Some(text) => CmdProbe {
                argv: argv.iter().map(|s| s.to_string()).collect(),
                code: Some(*meta.codes.get(name).unwrap_or(&0)),
                timed_out: meta.timed_out.iter().any(|t| t == name),
                duration_ms: 0,
                // A failing capture puts its message on stderr, like the real thing.
                stdout: if meta.codes.get(name).copied().unwrap_or(0) == 0 {
                    text.clone()
                } else {
                    String::new()
                },
                stderr: if meta.codes.get(name).copied().unwrap_or(0) == 0 {
                    String::new()
                } else {
                    text
                },
                error: None,
                taken: true,
            },
        }
    };

    let mut p = Probes {
        taken_at: "2026-09-14T00:00:00+02:00".to_string(),
        now_unix: meta.now_unix.unwrap_or(0),
        settings_problems: meta.settings_problems,
        killswitch_orphan_seen_sec_ago: meta.killswitch_orphan_seen_sec_ago,
        last_connect_sec_ago: meta.last_connect_sec_ago,
        cached_kill_switch: meta.cached_kill_switch,
        ..Probes::default()
    };
    p.cli_session = cmd(
        "protonvpn-config-list.txt",
        &["protonvpn", "config", "list"],
    );
    p.nmcli_connections = cmd(
        "nmcli-connections.txt",
        &[
            "nmcli",
            "-t",
            "-f",
            "NAME,UUID,TYPE,DEVICE,ACTIVE",
            "connection",
            "show",
        ],
    );
    p.busctl_readalias = cmd("busctl-readalias-default.txt", &["busctl", "ReadAlias"]);
    p.busctl_alias_property = cmd("busctl-alias-property.txt", &["busctl", "get-property"]);
    if p.busctl_readalias.taken {
        p.alias = Some(alias_from_raw(
            &p.busctl_readalias,
            &p.busctl_alias_property,
        ));
    }
    if let Some(text) = read("busctl-searchitems-service-proton.txt") {
        p.proton_items = secrets::parse_object_paths(&text);
    }
    if let Some(text) = read("busctl-collections.txt") {
        for path in secrets::parse_object_paths(&text) {
            let base = path.rsplit('/').next().unwrap_or_default().to_string();
            let prop = read(&format!("busctl-collection-{base}-property.txt"));
            let (label, locked) = prop
                .as_deref()
                .map(secrets::parse_label_locked)
                .unwrap_or((None, None));
            let items = read(&format!("busctl-collection-{base}-searchitems.txt"))
                .map(|t| secrets::parse_object_paths(&t))
                .unwrap_or_default();
            p.collections.push(CollectionProbe {
                exported: prop.is_some(),
                label,
                locked,
                proton_items: items,
                path,
            });
        }
    }
    if let Some(text) = read("connection_persistence.json") {
        p.connection_persistence = FileProbe {
            path: "connection_persistence.json".into(),
            exists: true,
            len: text.len() as u64,
            text: Some(text),
            taken: true,
            ..FileProbe::default()
        };
    }
    if let Ok(bytes) = std::fs::read(dir.join("serverlist.json")) {
        p.serverlist = serverlist_facts_from_bytes(
            "serverlist.json",
            bytes.len() as u64,
            Some("2026-09-14T20:26:43+02:00".into()),
            &bytes,
        );
    } else if meta.codes.contains_key("serverlist.json:missing") {
        p.serverlist = ServerlistFacts {
            path: "serverlist.json".into(),
            taken: true,
            ..ServerlistFacts::default()
        };
    }
    if let Some(text) = read("keyrings-default") {
        p.keyring_pointer = FileProbe {
            path: "~/.local/share/keyrings/default".into(),
            exists: true,
            len: text.len() as u64,
            text: Some(text),
            taken: true,
            ..FileProbe::default()
        };
    }
    if let Some(text) = read("config.toml") {
        // The real parse path: toml → Settings → the core's own validator.
        p.settings_file = FileProbe {
            path: dir.join("config.toml").display().to_string(),
            exists: true,
            len: text.len() as u64,
            text: Some(text.clone()),
            taken: true,
            ..FileProbe::default()
        };
        p.state_dir = DirProbe {
            path: dir.display().to_string(),
            exists: true,
            is_dir: true,
            writable: true,
            taken: true,
        };
        match toml::from_str::<fproton_core::Settings>(&text) {
            Ok(parsed) => {
                let mut ctx = Context::load().expect("a context for the fixture");
                ctx.settings = parsed;
                p.settings_problems = settings::validate(&ctx).unwrap_or_default();
            }
            Err(e) => p.settings_problems = vec![format!("config.toml does not parse: {e}")],
        }
    }
    if let Some(live) = meta.pending_live {
        p.pending = Some(status::Pending {
            action: status::PendingAction::Connect,
            target: Some("fastest".into()),
            started_at: "2026-09-14T22:29:58+02:00".into(),
            deadline_sec: 90,
            elapsed_sec: if live { 2 } else { 900 },
            pid: None,
        });
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_truncated_artifact_says_so_and_stays_utf8() {
        let long = "é".repeat(PROBE_TRUNCATE);
        let cut = truncate(long);
        assert!(cut.contains("--- truncated by fproton doctor"));
        assert!(cut.len() < PROBE_TRUNCATE + 200);
    }

    #[test]
    fn a_short_artifact_is_untouched() {
        assert_eq!(truncate("hello".into()), "hello");
    }

    #[test]
    fn the_serverlist_probe_never_carries_the_body() {
        let facts = serverlist_facts_from_bytes(
            "x.json",
            10,
            None,
            br#"{"LogicalServers":[{"Name":"CH#461"}],"MaxTier":2,"ExpirationTime":1.5}"#,
        );
        assert_eq!(facts.logicals, Some(1));
        assert_eq!(facts.max_tier, Some(2));
        assert_eq!(facts.expires_unix, Some(1));
        let json = serde_json::to_string(&facts).expect("serialises");
        assert!(!json.contains("CH#461"), "the body leaked into the probe");
    }
}
