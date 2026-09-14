//! `fproton` — Proton VPN for Omarchy: CLI, doctor, FrankenTUI and the backend of the
//! `oliver.protonvpn` bar widget.
//!
//! This file owns the whole command surface of DESIGN.md §4, the dispatch, and the
//! exit-code mapping of §4a. It was frozen at the Phase 1a gate: later phases fill in
//! `fproton-core`, `tui::run`, `doctor::run` and `setup::run` without touching it.

mod doctor;
mod output;
mod setup;
mod tui;

use clap::{Parser, Subcommand, ValueEnum};

use fproton_core::agent;
use fproton_core::engine::{self, Target};
use fproton_core::index::{self, Feature, Query, Sort, Tier};
use fproton_core::settings::{self, PROTON_CONFIG_SET_KEYS};
use fproton_core::status::ProtonConfig;
use fproton_core::{cli, natpmp, state, wg, CancelToken, Context, Error};

use output::{Out, Paint};

const ABOUT: &str = "Proton VPN for Omarchy: CLI, doctor, TUI and bar-widget backend.";

const LONG_ABOUT: &str = "Proton VPN for Omarchy.

Bare `fproton` opens the TUI on a terminal; without a TTY it prints `status --json`.
Every command accepts --json, and a non-TTY stdout implies it. Agents: start with
`fproton robot-docs` and `fproton capabilities`.

`status` never spawns the Proton CLI, so it answers even while that CLI is wedged.";

const AFTER_HELP: &str = "Exit codes: 0 ok · 1 failure · 2 usage · 3 safety (needs --yes) · 4 backend ·
5 upstream (the Proton CLI errored or was killed at its deadline) · 6 not found ·
7 ambiguous · 8 timeout · 9 not signed in · 10 index · 11/12/13 doctor · 130 interrupted.

Connect targets (one positional): fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random";

#[derive(Parser)]
#[command(
    name = "fproton",
    version,
    about = ABOUT,
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP,
    disable_help_subcommand = true
)]
struct Cli {
    // The six global flags carry a display_order above any command's own arguments so
    // that `fproton servers --help` reads as "what this command does" and then "what
    // every command does", instead of interleaving the two.
    /// Machine-readable JSON on stdout (errors too, with the same exit code)
    #[arg(long, global = true, display_order = 100)]
    json: bool,
    /// Clamp every external deadline to N seconds (no effect on commands that spawn nothing)
    #[arg(long, global = true, value_name = "SECONDS", display_order = 101)]
    timeout: Option<u64>,
    /// Diagnostics on stderr; repeat for more
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count, display_order = 102)]
    verbose: u8,
    /// Never emit colour (also honoured: NO_COLOR, CI, a non-TTY stdout)
    #[arg(long, global = true, display_order = 103)]
    no_color: bool,
    /// Confirm an irreversible action: disconnect --force, wg swap, setup, doctor --fix
    #[arg(long, global = true, display_order = 104)]
    yes: bool,
    /// Print what would happen and change nothing (same four commands as --yes)
    #[arg(long, global = true, display_order = 105)]
    dry_run: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// The whole Status document (schema 1). Never spawns the Proton CLI.
    Status,
    /// Connect to a target
    #[command(after_help = "TARGET is exactly one of:
  fastest       let Proton pick (default)
  CC            a country, e.g. CH
  CC#N          one server, e.g. CH#461
  city:NAME     e.g. city:\"New York\"
  p2p | secure-core | tor | random")]
    Connect {
        /// fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random
        target: Option<String>,
        /// Block until the tunnel is up (exit 8 if it never is)
        #[arg(long)]
        wait: bool,
    },
    /// Disconnect
    Disconnect {
        /// Tear proton0 and the IPv6 leak guard down through nmcli (needs --yes)
        #[arg(long)]
        force: bool,
    },
    /// Reconnect the last target
    Reconnect {
        /// Pick the next server in the same country, by score
        #[arg(long)]
        next: bool,
    },
    /// Servers from the local index snapshot
    #[command(
        alias = "ls",
        after_help = "Upstream `protonvpn servers` only prints a URL; this reads Proton's own
server list from disk and filters it locally."
    )]
    Servers {
        /// Two-letter country code, e.g. CH
        #[arg(long, value_name = "CC")]
        country: Option<String>,
        /// City name, e.g. Zurich
        #[arg(long, value_name = "NAME")]
        city: Option<String>,
        /// Only servers carrying this feature
        #[arg(long, value_enum)]
        feature: Option<FeatureArg>,
        /// Only servers at this entitlement level
        #[arg(long, value_enum)]
        tier: Option<TierArg>,
        /// Case-insensitive substring of the name, country or city
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Row order
        #[arg(long, value_enum, default_value = "score")]
        sort: SortArg,
        /// Keep only the first N rows
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Countries, with server counts and the lowest load
    Countries,
    /// Cities, optionally for one country
    Cities {
        /// Two-letter country code, e.g. CH
        #[arg(long, value_name = "CC")]
        country: Option<String>,
    },
    /// Build or describe the server-index snapshot
    Index {
        /// Rebuild even when the snapshot is fresh
        #[arg(long)]
        rebuild: bool,
        /// Print the snapshot's facts without rebuilding
        #[arg(long)]
        stats: bool,
    },
    /// Proton's own settings, through the Proton CLI
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// fproton's settings (~/.config/fproton/config.toml)
    Settings {
        #[command(subcommand)]
        cmd: Option<SettingsCmd>,
    },
    /// One-shot NAT-PMP port-forwarding request through the tunnel
    Port,
    /// Past connect/disconnect actions
    History {
        /// Keep only the newest N entries
        #[arg(long, default_value_t = 20, value_name = "N")]
        limit: usize,
    },
    /// Recently used targets, ready to hand back to `connect`
    Recents,
    /// Proton account name (cached); entitlement is status.max_tier
    Account,
    /// Sign in interactively in this terminal
    Signin {
        /// Proton username; prompted for when omitted
        user: Option<String>,
    },
    /// Sign out
    Signout,
    /// EXPERIMENT (needs FPROTON_EXPERIMENTAL_WG=1): NetworkManager WireGuard profiles
    Wg {
        #[command(subcommand)]
        cmd: WgCmd,
    },
    /// Diagnose the installation, and optionally repair it
    Doctor(doctor::DoctorArgs),
    /// Desktop integration: plugin link, bar placement, menu entries
    Setup(setup::SetupArgs),
    /// Terminal UI: Status · Servers · History
    Tui(tui::TuiArgs),
    /// The machine contract: commands, exit codes, keys, files
    Capabilities,
    /// Agent handbook, plain text
    #[command(alias = "docs")]
    RobotDocs {
        /// Only `guide` exists today
        section: Option<String>,
    },
    /// Version and contract version
    Version,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Read every Proton setting (runs `protonvpn config list` under a deadline)
    List,
    /// One setting, answered from the cache written by `config list`
    Get { key: String },
    /// Change one setting (runs `protonvpn config set` under a deadline)
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// One setting
    Get { key: String },
    /// Change one setting
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum WgCmd {
    /// Activate an existing ProtonVPN profile
    Up { name: Option<String> },
    /// Clone a profile and point the clone at another server (needs --yes)
    Swap { name: String, target: String },
    /// List ProtonVPN WireGuard profiles
    List,
}

#[derive(Clone, Copy, ValueEnum)]
enum FeatureArg {
    P2p,
    SecureCore,
    Tor,
    Streaming,
}

#[derive(Clone, Copy, ValueEnum)]
enum TierArg {
    Free,
    Plus,
}

#[derive(Clone, Copy, ValueEnum)]
enum SortArg {
    Load,
    Score,
    Name,
}

impl From<FeatureArg> for Feature {
    fn from(f: FeatureArg) -> Feature {
        match f {
            FeatureArg::P2p => Feature::P2p,
            FeatureArg::SecureCore => Feature::SecureCore,
            FeatureArg::Tor => Feature::Tor,
            FeatureArg::Streaming => Feature::Streaming,
        }
    }
}

impl From<TierArg> for Tier {
    fn from(t: TierArg) -> Tier {
        match t {
            TierArg::Free => Tier::Free,
            TierArg::Plus => Tier::Plus,
        }
    }
}

impl From<SortArg> for Sort {
    fn from(s: SortArg) -> Sort {
        match s {
            SortArg::Load => Sort::Load,
            SortArg::Score => Sort::Score,
            SortArg::Name => Sort::Name,
        }
    }
}

/// Turn clap's own rendering into one line: keep `error:` and `tip:`, drop the ANSI,
/// the `Usage:` block and the "For more information" footer (the hint replaces it).
fn clap_message(e: &clap::Error) -> String {
    let rendered = e.render().to_string();
    let mut plain = String::with_capacity(rendered.len());
    let mut chars = rendered.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for line in plain.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Usage:") || line.starts_with("For more information")
        {
            continue;
        }
        match parts.last_mut() {
            // clap wraps a list (`the following required arguments were not
            // provided:` then one indented item per line) — fold it back up.
            Some(last) if last.ends_with(':') => {
                last.push(' ');
                last.push_str(line);
            }
            _ => parts.push(line.trim_start_matches("error: ").to_string()),
        }
    }
    if parts.is_empty() {
        "invalid arguments".to_string()
    } else {
        parts.join("; ")
    }
}

/// The command path the user was reaching for, e.g. `["config", "set"]`, so that both
/// the message and the hint can name it. Empty when no command was recognised.
fn clap_path(argv: &[std::ffi::OsString]) -> Vec<&'static str> {
    const SUBS: [&str; 23] = [
        "status",
        "connect",
        "disconnect",
        "reconnect",
        "servers",
        "countries",
        "cities",
        "index",
        "config",
        "settings",
        "port",
        "history",
        "recents",
        "account",
        "signin",
        "signout",
        "wg",
        "doctor",
        "setup",
        "tui",
        "capabilities",
        "robot-docs",
        "version",
    ];
    // The commands that themselves take a subcommand, and what those are.
    const NESTED: [(&str, &[&str]); 4] = [
        ("config", &["list", "get", "set"]),
        ("settings", &["get", "set"]),
        ("wg", &["up", "swap", "list"]),
        ("doctor", &["undo", "capabilities"]),
    ];
    let mut path: Vec<&'static str> = Vec::new();
    for arg in argv.iter().skip(1) {
        let Some(arg) = arg.to_str() else { continue };
        if arg.starts_with('-') {
            continue;
        }
        match path.as_slice() {
            [] => {
                let name = match arg {
                    "ls" => "servers",
                    "docs" => "robot-docs",
                    other => match SUBS.iter().find(|s| **s == other) {
                        Some(s) => s,
                        None => return path,
                    },
                };
                path.push(name);
            }
            [top] => {
                let Some((_, subs)) = NESTED.iter().find(|(n, _)| n == top) else {
                    return path;
                };
                match subs.iter().find(|s| **s == arg) {
                    Some(s) => path.push(s),
                    None => return path,
                }
            }
            _ => return path,
        }
    }
    path
}

/// Always a command that actually runs (§2: hints are runnable).
fn clap_hint(path: &[&str]) -> String {
    if path.is_empty() {
        "fproton --help".to_string()
    } else {
        format!("fproton {} --help", path.join(" "))
    }
}

/// Parse, or turn clap's failure into the §2 error envelope so that a piping agent
/// gets one JSON object for *every* failure, a bad flag value included.
fn parse_or_report() -> Cli {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            // `--help` and `--version` are successes clap reports as errors.
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                e.exit();
            }
            // The policy has to be read off the raw argv: there is no parsed Cli yet.
            let flag = |name: &str| argv.iter().any(|a| a == name);
            let out = Out::new(flag("--json"), flag("--no-color"), 0);
            let path = clap_path(&argv);
            // A bare `fproton config` makes clap render the whole help page; say what
            // is actually missing instead of pasting a screenful into `error`.
            let message = if e.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
                match path.as_slice() {
                    [top] => format!("`fproton {top}` needs a subcommand"),
                    _ => "a subcommand is required".to_string(),
                }
            } else {
                clap_message(&e)
            };
            let err = Error::usage(message).with_hint(clap_hint(&path));
            std::process::exit(out.report(&err));
        }
    }
}

/// The signal a handler saw, or 0. An `AtomicI32` store is async-signal-safe; nothing
/// else in the handler is allowed to be.
static SIGNALLED: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

extern "C" fn note_signal(sig: libc::c_int) {
    SIGNALLED.store(sig, std::sync::atomic::Ordering::SeqCst);
}

/// Ctrl+C must cancel the action, not abandon it.
///
/// With the default disposition, SIGINT killed fproton outright: the `protonvpn` child
/// runs in its own process group, so the terminal's SIGINT never reached it, the tunnel
/// kept coming up unattended, and `pending` was left behind with no history row. The
/// handler only sets a flag; a watcher thread turns that into a `CancelToken::cancel`,
/// which `engine::connect` already polls every 25 ms — so the child group is torn down,
/// `finish()` records a `cancelled` row, `pending` is cleared, and §4a's exit 130 comes
/// from fproton rather than from the signal.
fn install_cancel_on_signal(cancel: &CancelToken) {
    unsafe {
        libc::signal(libc::SIGINT, note_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, note_signal as *const () as libc::sighandler_t);
    }
    let cancel = cancel.clone();
    std::thread::spawn(move || {
        let sig = loop {
            let sig = SIGNALLED.load(std::sync::atomic::Ordering::SeqCst);
            if sig != 0 {
                break sig;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        };
        cancel.cancel();
        // Never become unkillable: a command that polls no cancel token (`status`), or
        // an action that will not unwind, still dies on the default disposition.
        std::thread::sleep(std::time::Duration::from_secs(3));
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    });
}

fn main() {
    // Behave like a Unix tool when stdout is a closed pipe (`fproton servers | head`).
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = parse_or_report();
    let out = Out::new(cli.json, cli.no_color, cli.verbose);
    match run(cli, out) {
        Ok(code) => std::process::exit(code),
        Err(e) => std::process::exit(out.report(&e)),
    }
}

/// Anything that escaped a `tui`/`doctor`/`setup` run keeps its §4a code when it has one.
fn from_anyhow(e: anyhow::Error) -> Error {
    match e.downcast::<Error>() {
        Ok(typed) => typed,
        Err(other) => Error::failure(format!("{other:#}")),
    }
}

/// Stamp the envelope keys every fproton JSON object carries (§2).
fn ok(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(map) = value.as_object_mut() {
        map.insert("ok".into(), serde_json::json!(true));
        map.insert(
            "schema".into(),
            serde_json::json!(fproton_core::SCHEMA_VERSION),
        );
    }
    value
}

/// Refuse an irreversible action that was not confirmed, naming what it would run (§2).
fn require_yes(yes: bool, what: &str, argv_preview: &[&str]) -> Result<(), Error> {
    if yes {
        return Ok(());
    }
    Err(Error::safety(format!(
        "{what} needs --yes; it would run: {}",
        argv_preview.join(" && ")
    ))
    .with_hint(format!("{what} --yes   # or add --dry-run to see the plan")))
}

fn run(cli: Cli, out: Out) -> Result<i32, Error> {
    let ctx = Context::load()?.with_flags(out.json(), cli.verbose, cli.timeout);
    let cancel = CancelToken::new();
    install_cancel_on_signal(&cancel);
    let dry_run = cli.dry_run;
    let yes = cli.yes;
    out.verbose_note(format!(
        "state {} · config {} · cache {} · deadlines probe {}s connect {}s",
        ctx.paths.state_dir.display(),
        ctx.paths.config_dir.display(),
        ctx.paths.cache_dir.display(),
        ctx.timeouts.probe_sec,
        ctx.timeouts.connect_sec
    ));

    // Bare invocation: the TUI on a terminal, `status --json` without one (§4).
    let cmd = match cli.cmd {
        Some(c) => c,
        None if output::stdout_is_tty() => Cmd::Tui(tui::TuiArgs::default()),
        None => Cmd::Status,
    };

    match cmd {
        Cmd::Status => {
            let s = fproton_core::status(&ctx)?;
            out.emit(serde_json::to_value(&s)?, |o| output::status_human(o, &s));
            Ok(0)
        }

        Cmd::Connect { target, wait } => {
            let target: Target = target
                .unwrap_or_else(|| ctx.settings.default_target.clone())
                .parse()?;
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "connect", "target": target.to_string(), "wait": wait})),
                    |o| o.line(format!("would connect to {target}")),
                );
                return Ok(0);
            }
            let c = engine::connect(&ctx, &target, &cancel)?;
            out.emit(
                ok(serde_json::json!({"action": "connect", "target": target.to_string(), "connected": c})),
                |o| {
                    o.line(format!(
                        "connected to {} in {}s",
                        c.server.as_deref().unwrap_or(&c.target),
                        c.duration_sec
                    ))
                },
            );
            Ok(0)
        }

        Cmd::Disconnect { force } => {
            let plan = [
                "nmcli device disconnect proton0",
                "nmcli connection down pvpn-killswitch-ipv6",
            ];
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "disconnect", "force": force, "argv": plan})),
                    |o| {
                        if force {
                            for p in plan {
                                o.line(format!("would run: {p}"));
                            }
                        } else {
                            o.line("would run: protonvpn disconnect");
                        }
                    },
                );
                return Ok(0);
            }
            if force {
                require_yes(yes, "fproton disconnect --force", &plan)?;
            }
            engine::disconnect(&ctx, force, &cancel)?;
            out.emit(
                ok(serde_json::json!({"action": "disconnect", "force": force})),
                |o| o.line("disconnected"),
            );
            Ok(0)
        }

        Cmd::Reconnect { next } => {
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "reconnect", "next": next})),
                    |o| o.line("would reconnect the last target"),
                );
                return Ok(0);
            }
            let c = engine::reconnect(&ctx, next, &cancel)?;
            out.emit(
                ok(serde_json::json!({"action": "reconnect", "next": next, "connected": c})),
                |o| {
                    o.line(format!(
                        "reconnected to {}",
                        c.server.as_deref().unwrap_or(&c.target)
                    ))
                },
            );
            Ok(0)
        }

        Cmd::Servers {
            country,
            city,
            feature,
            tier,
            search,
            sort,
            limit,
        } => {
            let country = match country {
                Some(c) => Some(validated_country(&c)?),
                None => None,
            };
            let q = Query {
                country,
                city,
                feature: feature.map(Into::into),
                tier: tier.map(Into::into),
                search,
                sort: sort.into(),
                limit,
            };
            let rows = index::servers(&ctx, &q)?;
            let built = index::info(&ctx).ok().and_then(|i| i.built_from_mtime);
            out.emit(
                ok(serde_json::json!({"count": rows.len(), "built_from_mtime": built, "servers": rows})),
                |o| output::servers_human(o, &rows, built.as_deref()),
            );
            Ok(0)
        }

        Cmd::Countries => {
            let rows = index::countries(&ctx)?;
            out.emit(
                ok(serde_json::json!({"count": rows.len(), "countries": rows})),
                |o| output::countries_human(o, &rows),
            );
            Ok(0)
        }

        Cmd::Cities { country } => {
            let country = match country {
                Some(c) => validated_country(&c)?,
                None => String::new(),
            };
            let rows = index::cities(&ctx, &country)?;
            out.emit(
                ok(serde_json::json!({"count": rows.len(), "country": country, "cities": rows})),
                |o| output::cities_human(o, &rows),
            );
            Ok(0)
        }

        Cmd::Index { rebuild, stats } => {
            if stats && !rebuild {
                let info = index::info(&ctx)?;
                out.emit(ok(serde_json::json!({"index": info})), |o| {
                    o.line(format!(
                        "{} servers · built from serverlist.json of {}",
                        info.servers
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "?".into()),
                        info.built_from_mtime.as_deref().unwrap_or("—")
                    ))
                });
                return Ok(0);
            }
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "index", "rebuild": rebuild})),
                    |o| o.line("would rebuild the index snapshot"),
                );
                return Ok(0);
            }
            let s = index::rebuild(&ctx, &cancel)?;
            out.emit(ok(serde_json::json!({"index": s})), |o| {
                output::index_human(o, &s)
            });
            Ok(0)
        }

        Cmd::Config { cmd } => match cmd {
            ConfigCmd::List => {
                let c = cli::config_list(&ctx, &cancel)?;
                out.emit(ok(serde_json::json!({"config": c})), |o| {
                    print_proton_config(o, &c)
                });
                Ok(0)
            }
            ConfigCmd::Get { key } => {
                let Some(c) = state::config_cache(&ctx)? else {
                    return Err(Error::upstream(
                        "no cached Proton settings; `config list` has not succeeded yet",
                    )
                    .with_hint("fproton config list"));
                };
                let value = c.get(&key).map(|v| v.to_string());
                out.emit(ok(serde_json::json!({"key": key, "value": value})), |o| {
                    o.line(value.clone().unwrap_or_else(|| "—".into()))
                });
                Ok(0)
            }
            ConfigCmd::Set { key, value } => {
                settings::validate_proton_config(&key, &value)?;
                if dry_run {
                    out.emit(
                        ok(serde_json::json!({"dry_run": true, "action": "config set", "key": key, "value": value})),
                        |o| o.line(format!("would run: protonvpn config set {key} {value}")),
                    );
                    return Ok(0);
                }
                cli::config_set(&ctx, &key, &value, &cancel)?;
                out.emit(
                    ok(serde_json::json!({"action": "config set", "key": key, "value": value})),
                    |o| o.line(format!("{key} = {value}")),
                );
                Ok(0)
            }
        },

        Cmd::Settings { cmd } => match cmd {
            None => {
                let problems = settings::validate(&ctx).unwrap_or_default();
                out.emit(
                    ok(serde_json::json!({
                        "path": ctx.paths.config_file().display().to_string(),
                        "settings": ctx.settings,
                        "problems": problems,
                    })),
                    |o| {
                        o.line(format!("# {}", ctx.paths.config_file().display()));
                        o.line(
                            toml::to_string_pretty(&ctx.settings)
                                .unwrap_or_default()
                                .trim_end(),
                        );
                        for p in &problems {
                            o.note(format!("problem: {p}"));
                        }
                    },
                );
                Ok(0)
            }
            Some(SettingsCmd::Get { key }) => {
                let value = ctx.settings.get(&key)?;
                out.emit(ok(serde_json::json!({"key": key, "value": value})), |o| {
                    o.line(value.clone())
                });
                Ok(0)
            }
            Some(SettingsCmd::Set { key, value }) => {
                let mut s = ctx.settings.clone();
                s.set(&key, &value)?;
                if dry_run {
                    out.emit(
                        ok(serde_json::json!({"dry_run": true, "action": "settings set", "key": key, "value": value})),
                        |o| o.line(format!("would set {key} = {value}")),
                    );
                    return Ok(0);
                }
                s.save(&ctx.paths)?;
                out.emit(
                    ok(serde_json::json!({"action": "settings set", "key": key, "value": value})),
                    |o| o.line(format!("{key} = {value}")),
                );
                Ok(0)
            }
        },

        Cmd::Port => {
            let m = natpmp::request_port(&ctx, &cancel)?;
            out.emit(ok(serde_json::json!({"port_forwarding": m})), |o| {
                o.line(format!("port {} until {}", m.port, m.expires_at))
            });
            Ok(0)
        }

        Cmd::History { limit } => {
            let rows = state::history(&ctx, limit)?;
            out.emit(
                ok(serde_json::json!({"count": rows.len(), "history": rows})),
                |o| output::history_human(o, &rows),
            );
            Ok(0)
        }

        Cmd::Recents => {
            let targets = state::recents(&ctx)?;
            let strings: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
            out.emit(
                ok(serde_json::json!({"count": strings.len(), "recents": strings})),
                |o| {
                    for t in &strings {
                        o.line(t);
                    }
                },
            );
            Ok(0)
        }

        Cmd::Account => {
            let a = cli::account(&ctx, &cancel)?;
            out.emit(ok(serde_json::json!({"account": a})), |o| {
                o.line(format!("{} (cached {})", a.name, a.cached_at))
            });
            Ok(0)
        }

        Cmd::Signin { user } => {
            let user = match user {
                Some(u) => u,
                None => prompt_username(&out)?,
            };
            if !cli::is_valid_username(&user) {
                return Err(Error::usage(
                    "a Proton username is 1-64 characters of A-Z a-z 0-9 . _ + @ -",
                )
                .with_hint("fproton signin <username>"));
            }
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "signin", "user": user})),
                    |o| o.line(format!("would run: protonvpn signin {user}")),
                );
                return Ok(0);
            }
            // §2 grants `signin` the only deadline-free spawn on the grounds that it is
            // "interactive, only from a terminal". Enforce that precondition: with no
            // terminal the child's password prompt lands in the stdout that is implied
            // `--json`, `--timeout` is ignored, and the call hangs until something
            // outside fproton kills it.
            if !output::stdout_is_tty() {
                return Err(
                    Error::usage("signin is interactive and needs a terminal").with_hint(
                        "omarchy-launch-floating-terminal-with-presentation fproton signin",
                    ),
                );
            }
            cli::signin(&ctx, &user)?;
            out.emit(
                ok(serde_json::json!({"action": "signin", "user": user})),
                |o| o.line("signed in"),
            );
            Ok(0)
        }

        Cmd::Signout => {
            if dry_run {
                out.emit(
                    ok(serde_json::json!({"dry_run": true, "action": "signout"})),
                    |o| o.line("would run: protonvpn signout"),
                );
                return Ok(0);
            }
            cli::signout(&ctx, &cancel)?;
            out.emit(ok(serde_json::json!({"action": "signout"})), |o| {
                o.line("signed out")
            });
            Ok(0)
        }

        Cmd::Wg { cmd } => {
            if !wg::enabled() {
                return Err(wg::gate_error());
            }
            out.note(wg::EXPERIMENTAL_NOTE);
            match cmd {
                WgCmd::List => {
                    let ps = wg::list(&ctx)?;
                    out.emit(
                        ok(serde_json::json!({"experimental": true, "count": ps.len(), "profiles": ps})),
                        |o| {
                            for p in &ps {
                                o.line(format!(
                                    "{}  {}",
                                    p.name,
                                    if p.active { "active" } else { "" }
                                ));
                            }
                        },
                    );
                    Ok(0)
                }
                WgCmd::Up { name } => {
                    if dry_run {
                        out.emit(
                            ok(serde_json::json!({"dry_run": true, "action": "wg up", "profile": name})),
                            |o| o.line("would activate the ProtonVPN profile"),
                        );
                        return Ok(0);
                    }
                    let p = wg::up(&ctx, name.as_deref(), &cancel)?;
                    out.emit(
                        ok(serde_json::json!({"experimental": true, "action": "wg up", "profile": p})),
                        |o| o.line(format!("activated {}", p.name)),
                    );
                    Ok(0)
                }
                WgCmd::Swap { name, target } => {
                    let target: Target = target.parse()?;
                    if dry_run {
                        out.emit(
                            ok(serde_json::json!({"dry_run": true, "action": "wg swap", "profile": name, "target": target.to_string()})),
                            |o| o.line(format!("would clone {name} and point it at {target}")),
                        );
                        return Ok(0);
                    }
                    require_yes(
                        yes,
                        "fproton wg swap",
                        &["nmcli connection clone", "nmcli connection modify"],
                    )?;
                    let p = wg::swap(&ctx, &name, &target, &cancel)?;
                    out.emit(
                        ok(serde_json::json!({"experimental": true, "action": "wg swap", "profile": p})),
                        |o| o.line(format!("swapped {} to {target}", p.name)),
                    );
                    Ok(0)
                }
            }
        }

        Cmd::Doctor(mut args) => {
            args.yes = yes;
            args.dry_run = dry_run;
            doctor::run(args, ctx).map_err(from_anyhow)
        }

        Cmd::Setup(mut args) => {
            args.yes = yes;
            args.dry_run = dry_run;
            setup::run(args, ctx).map_err(from_anyhow)
        }

        Cmd::Tui(args) => {
            if !output::stdout_is_tty() {
                return Err(
                    Error::usage("the TUI needs a terminal; stdout is not a TTY")
                        .with_hint("fproton status --json"),
                );
            }
            tui::run(args, ctx).map_err(from_anyhow)
        }

        Cmd::Capabilities => {
            // Always machine output, whatever the flags say.
            out.forced_json()
                .emit(agent::capabilities_json(&ctx), |_| {});
            Ok(0)
        }

        Cmd::RobotDocs { section } => {
            let section = section.unwrap_or_else(|| "guide".into());
            if section != "guide" {
                return Err(Error::usage(format!("unknown docs section '{section}'"))
                    .with_hint("fproton robot-docs guide"));
            }
            print!("{}", agent::ROBOT_GUIDE);
            Ok(0)
        }

        Cmd::Version => {
            out.emit(
                ok(serde_json::json!({
                    "tool": fproton_core::NAME,
                    "version": fproton_core::VERSION,
                    "contract_version": fproton_core::CONTRACT_VERSION,
                    "build_stage": agent::BUILD_STAGE,
                })),
                |o| o.line(format!("fproton {}", fproton_core::VERSION)),
            );
            Ok(0)
        }
    }
}

/// Country codes reach argv and the index; validate before either (§2).
fn validated_country(cc: &str) -> Result<String, Error> {
    let upper = cc.trim().to_ascii_uppercase();
    if engine::is_country_code(&upper) {
        Ok(upper)
    } else {
        Err(
            Error::usage(format!("'{cc}' is not a two-letter country code"))
                .with_hint("fproton countries"),
        )
    }
}

/// The upstream `signin` positional is required, so ask for it when it was omitted.
fn prompt_username(out: &Out) -> Result<String, Error> {
    if !output::stdout_is_tty() {
        return Err(
            Error::usage("signin needs a username when stdout is not a terminal")
                .with_hint("fproton signin <username>"),
        );
    }
    out.note("Proton username: ");
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
        .map_err(|e| Error::failure(format!("cannot read the username: {e}")))?;
    let user = line.trim().to_string();
    if user.is_empty() {
        return Err(Error::usage("no username given").with_hint("fproton signin <username>"));
    }
    Ok(user)
}

fn print_proton_config(out: &Out, c: &ProtonConfig) {
    for (key, _) in PROTON_CONFIG_SET_KEYS {
        out.line(format!(
            "{} {}",
            output::pad(key, 18),
            c.get(key).unwrap_or("—")
        ));
    }
    for key in ["custom-dns", "anonymous-crash-reports"] {
        out.line(format!(
            "{} {} {}",
            output::pad(key, 18),
            c.get(key).unwrap_or("—"),
            out.paint("(read-only in v0.1)", Paint::Dim)
        ));
    }
    if let Some(at) = &c.cached_at {
        out.note(format!("cached at {at}"));
    }
}
