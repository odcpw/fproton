//! Connect targets and the engine entry points (DESIGN.md §4, §8).
//!
//! A [`Target`] is exactly one of the eight things `fproton connect` accepts. It is
//! validated **before** it can reach an argv (§2): the parser is the only door, and
//! `Display` round-trips it back to the exact string the CLI takes, so
//! History → Enter → reconnect needs no re-parsing.
//!
//! The connect/disconnect/reconnect bodies are Phase 1b; their signatures are frozen.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// What `fproton connect` was asked to reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// No argument: Proton picks.
    Fastest,
    /// Two-letter country code, e.g. `CH`.
    Country(String),
    /// A logical server name, e.g. `CH#461`, `IS-DE#1`, `US-CO#21-TOR`.
    Server(String),
    /// `city:"New York"`.
    City(String),
    P2p,
    SecureCore,
    Tor,
    Random,
}

/// The eight target forms, for `--help`, `capabilities` and the plugin's validator.
pub const TARGET_FORMS: &[&str] = &[
    "fastest",
    "CC",
    "CC#N",
    "city:NAME",
    "p2p",
    "secure-core",
    "tor",
    "random",
];

/// `^[A-Z]{2}$` (§2).
pub fn is_country_code(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|c| c.is_ascii_uppercase())
}

/// `^[A-Z]{2}(-(?:[A-Z]{2}|FREE))?(#[0-9]{1,5})?(-TOR)?$`, max 12 chars (§2).
/// Verified against all 18 221 `Name` values in Proton's serverlist.
pub fn is_server_name(s: &str) -> bool {
    if s.len() < 2 || s.len() > 12 || !s.is_ascii() {
        return false;
    }
    let b = s.as_bytes();
    if !(b[0].is_ascii_uppercase() && b[1].is_ascii_uppercase()) {
        return false;
    }
    let mut rest = &s[2..];
    rest = rest.strip_suffix("-TOR").unwrap_or(rest);
    if let Some(hash) = rest.rfind('#') {
        let (head, number) = rest.split_at(hash);
        let number = &number[1..];
        if number.is_empty() || number.len() > 5 || !number.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
        rest = head;
    }
    if rest.is_empty() {
        return true;
    }
    match rest.strip_prefix('-') {
        Some("FREE") => true,
        Some(cc) => is_country_code(cc),
        None => false,
    }
}

/// `^[\p{L} .'-]{1,40}$` (§2).
pub fn is_city_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=40).contains(&n)
        && s.chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '.' | '\'' | '-'))
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> Result<Target, Error> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Target::Fastest);
        }
        match s.to_ascii_lowercase().as_str() {
            "fastest" | "best" | "default" => return Ok(Target::Fastest),
            "p2p" => return Ok(Target::P2p),
            "secure-core" | "securecore" | "secure_core" => return Ok(Target::SecureCore),
            "tor" => return Ok(Target::Tor),
            "random" => return Ok(Target::Random),
            _ => {}
        }
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("city:") || lower.starts_with("city=") {
            let city = &s[5..];
            let city = city.trim().trim_matches('"').trim_matches('\'').trim();
            if !is_city_name(city) {
                return Err(Error::usage(format!(
                    "'{city}' is not a city name (letters, spaces, . ' - and at most 40 characters)"
                ))
                .with_hint("fproton cities --country CH"));
            }
            return Ok(Target::City(city.to_string()));
        }
        let upper = s.to_ascii_uppercase();
        if is_country_code(&upper) {
            return Ok(Target::Country(upper));
        }
        if is_server_name(&upper) {
            return Ok(Target::Server(upper));
        }
        Err(Error::usage(format!(
            "'{s}' is not a connect target; use one of: {}",
            TARGET_FORMS.join(" · ")
        ))
        .with_hint(search_hint(s)))
    }
}

/// A search hint for a rejected value that is always a command you can actually run.
///
/// Two rules pull against each other: §2 says every hint is a runnable command, and a
/// rejected value must not be pasted into one when it is shell-shaped. Keeping the
/// value's leading safe run satisfies both — `CH#461 foo` hints `--search CH`, which
/// is runnable *and* finds what the caller meant — and a value with no safe prefix at
/// all falls back to bare `fproton servers`. A `<text>` placeholder is not a command.
pub(crate) fn search_hint(s: &str) -> String {
    let safe: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        "fproton servers".to_string()
    } else {
        format!("fproton servers --search {safe}")
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Fastest => f.write_str("fastest"),
            Target::Country(c) => f.write_str(c),
            Target::Server(n) => f.write_str(n),
            Target::City(c) => write!(f, "city:{c}"),
            Target::P2p => f.write_str("p2p"),
            Target::SecureCore => f.write_str("secure-core"),
            Target::Tor => f.write_str("tor"),
            Target::Random => f.write_str("random"),
        }
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Target, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Target {
    /// The kind name used by `capabilities` and the plugin's validator.
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Fastest => "fastest",
            Target::Country(_) => "country",
            Target::Server(_) => "server",
            Target::City(_) => "city",
            Target::P2p => "p2p",
            Target::SecureCore => "secure-core",
            Target::Tor => "tor",
            Target::Random => "random",
        }
    }
}

/// Build a target from a server name that came from the index or from history.
pub fn target_for_server(name: &str) -> Result<Target, Error> {
    let upper = name.trim().to_ascii_uppercase();
    if is_server_name(&upper) {
        Ok(Target::Server(upper))
    } else {
        Err(
            Error::not_found(format!("'{name}' is not a Proton server name"))
                .with_hint(search_hint(name)),
        )
    }
}

/// What a completed connect reports (§8).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connected {
    /// The target as given, in its canonical spelling.
    pub target: String,
    /// The server Proton actually picked, when it could be resolved.
    pub server: Option<String>,
    pub country: Option<String>,
    /// Always `cli` in v0.1.
    pub engine: String,
    /// Wall clock of the engine call.
    pub duration_sec: u64,
    /// True when the tunnel was already up on that target and nothing was changed.
    pub already_connected: bool,
}

// ---------------------------------------------------------------- bookkeeping

/// Resolve and tier-check a target against the index, so an impossible connect costs
/// nothing and says why (§4: unknown → 6, several matches → 7, above the tier → 6).
///
/// When there is no usable index the target is passed through unchecked: refusing a
/// connect because fproton could not read Proton's cache would be worse than letting
/// the CLI answer for itself.
fn resolve(ctx: &Context, target: &Target) -> Result<(), Error> {
    let max_tier = crate::index::header(ctx).and_then(|h| h.max_tier);
    match target {
        Target::Server(name) => {
            let Some(row) = crate::index::find_server(ctx, name).unwrap_or(None) else {
                if crate::index::header(ctx).is_none() {
                    return Ok(());
                }
                return Err(
                    Error::not_found(format!("no Proton server is called '{name}'"))
                        .with_hint(search_hint(name)),
                );
            };
            if let Some(max) = max_tier {
                if row.tier > max {
                    return Err(Error::not_found(format!(
                        "{name} is a tier-{} server and this account is tier {max}",
                        row.tier
                    ))
                    .with_hint("fproton servers --tier free"));
                }
            }
            Ok(())
        }
        Target::Country(cc) => {
            let Ok(countries) = crate::index::countries(ctx) else {
                return Ok(());
            };
            if countries.is_empty() || countries.iter().any(|c| &c.country == cc) {
                Ok(())
            } else {
                Err(Error::not_found(format!("Proton has no servers in '{cc}'"))
                    .with_hint("fproton countries"))
            }
        }
        Target::City(city) => {
            let Ok(cities) = crate::index::cities(ctx, "") else {
                return Ok(());
            };
            if cities.is_empty() {
                return Ok(());
            }
            let matches: Vec<&crate::index::CityRow> = cities
                .iter()
                .filter(|c| c.city.eq_ignore_ascii_case(city))
                .collect();
            match matches.len() {
                0 => Err(
                    Error::not_found(format!("Proton has no servers in '{city}'"))
                        .with_hint("fproton cities"),
                ),
                1 => Ok(()),
                _ => {
                    let where_ = matches
                        .iter()
                        .map(|c| c.country.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(
                        Error::ambiguous(format!("'{city}' exists in several countries: {where_}"))
                            .with_hint(format!(
                                "fproton servers --city \"{city}\" --country {}",
                                matches[0].country
                            )),
                    )
                }
            }
        }
        _ => Ok(()),
    }
}

/// Which server the tunnel is on right now, if any.
fn current_server(ctx: &Context) -> Option<(String, Option<String>)> {
    let link = crate::nm::link_state(ctx).ok()?;
    if !link.up {
        return None;
    }
    let name = link
        .nm_connection
        .as_deref()?
        .strip_prefix("ProtonVPN ")?
        .to_string();
    let country = crate::index::find_server(ctx, &name)
        .unwrap_or(None)
        .map(|r| r.country);
    Some((name, country))
}

/// How an action ended, for the history row (§5 `HistoryEntry`).
fn result_of(outcome: &Result<(), Error>) -> (crate::state::HistoryResult, Option<String>) {
    use crate::state::HistoryResult as R;
    match outcome {
        Ok(()) => (R::Ok, None),
        Err(Error::Interrupted) => (R::Cancelled, Some("interrupted".to_string())),
        Err(e) if e.exit_code() == 5 && e.to_string().contains("deadline") => {
            (R::Timeout, Some(e.code_name().to_string()))
        }
        Err(e) => (R::Error, Some(e.code_name().to_string())),
    }
}

/// Record `pending` before the engine spawns anything (§8).
fn begin(
    ctx: &Context,
    action: crate::status::PendingAction,
    target: Option<&Target>,
    deadline_sec: u64,
) -> std::time::Instant {
    // The pid is not known until the child has been reaped, so §5's `pending.pid`
    // stays null for the life of the record rather than carrying a guess.
    let _ = crate::state::set_pending(
        ctx,
        crate::status::Pending {
            action,
            target: target.map(|t| t.to_string()),
            started_at: crate::now_rfc3339(),
            deadline_sec,
            elapsed_sec: 0,
            pid: None,
        },
    );
    std::time::Instant::now()
}

/// Clear `pending`, append the history row, and remember the error (§5).
#[allow(clippy::too_many_arguments)]
fn finish(
    ctx: &Context,
    action: crate::state::HistoryAction,
    target: Option<&Target>,
    server: Option<String>,
    country: Option<String>,
    started: std::time::Instant,
    outcome: &Result<(), Error>,
) -> u64 {
    let duration_sec = started.elapsed().as_secs();
    let (result, code_name) = result_of(outcome);
    let _ = crate::state::clear_pending(ctx);
    let _ = crate::state::push_history(
        ctx,
        crate::state::HistoryEntry {
            at: crate::now_rfc3339(),
            action,
            target: target.map(|t| t.to_string()),
            server,
            country,
            engine: ctx.settings.engine.clone(),
            result,
            code_name,
            duration_sec,
        },
    );
    if let Err(e) = outcome {
        let _ = crate::state::set_last_error(ctx, e, None);
    }
    duration_sec
}

// ---------------------------------------------------------------- frozen API

/// Connect to `target`.
///
/// Writes `pending` to state **before** spawning and appends the history entry when
/// the engine exits — callers (main.rs and the TUI) must not duplicate either.
/// Polls `cancel` at least every 250 ms; on cancel it SIGTERM/SIGKILLs the child
/// group and returns `Err(Error::Interrupted)`. Never touches stdout/stderr.
pub fn connect(ctx: &Context, target: &Target, cancel: &CancelToken) -> Result<Connected, Error> {
    resolve(ctx, target)?;

    // Already there? Only an exact server target can be answered without spawning:
    // for `fastest` and the feature targets, only Proton knows what "there" means.
    if let (Target::Server(name), Some((current, country))) = (target, current_server(ctx)) {
        if current.eq_ignore_ascii_case(name) {
            return Ok(Connected {
                target: target.to_string(),
                server: Some(current),
                country,
                engine: ctx.settings.engine.clone(),
                duration_sec: 0,
                already_connected: true,
            });
        }
    }

    let deadline = ctx.timeouts.connect();
    let started = begin(
        ctx,
        crate::status::PendingAction::Connect,
        Some(target),
        deadline.as_secs(),
    );
    let args = crate::cli::connect_args(target);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = crate::cli::run(ctx, &argv, deadline, cancel).map(|_| ());

    let (server, country) = match &outcome {
        Ok(()) => current_server(ctx).map_or((None, None), |(s, c)| (Some(s), c)),
        Err(_) => (None, None),
    };
    let duration_sec = finish(
        ctx,
        crate::state::HistoryAction::Connect,
        Some(target),
        server.clone(),
        country.clone(),
        started,
        &outcome,
    );
    outcome?;
    let _ = crate::state::push_recent(ctx, target);
    Ok(Connected {
        target: target.to_string(),
        server,
        country,
        engine: ctx.settings.engine.clone(),
        duration_sec,
        already_connected: false,
    })
}

/// Reconnect to the last target; `next` picks the next server in the same country by score.
pub fn reconnect(ctx: &Context, next: bool, cancel: &CancelToken) -> Result<Connected, Error> {
    let stored = crate::state::load(ctx)?;
    let last: Option<Target> = stored
        .recents
        .first()
        .and_then(|t| t.parse::<Target>().ok())
        .or_else(|| {
            stored
                .history
                .iter()
                .rev()
                .find(|h| h.action == crate::state::HistoryAction::Connect)
                .and_then(|h| h.target.as_deref())
                .and_then(|t| t.parse::<Target>().ok())
        });
    let Some(last) = last else {
        return Err(Error::not_found(
            "fproton has not connected to anything yet, so there is nothing to reconnect",
        )
        .with_hint("fproton connect fastest"));
    };

    let target = if next { next_server(ctx, &last)? } else { last };
    // Resolve BEFORE `begin`, exactly as `connect` does. `begin` writes the `pending`
    // record and only `finish` clears it, so an early return between the two leaves a
    // record nothing will ever clear: `status` reports `connecting` for the whole 90 s
    // deadline, then fabricates a `last_error` and a `stale_pending` warning that
    // needs `doctor --fix`. There must be no `?` between `begin` and `finish`.
    resolve(ctx, &target)?;
    // `reconnect` records itself as a reconnect; `connect` would record a connect.
    let deadline = ctx.timeouts.connect();
    let started = begin(
        ctx,
        crate::status::PendingAction::Reconnect,
        Some(&target),
        deadline.as_secs(),
    );
    let args = crate::cli::connect_args(&target);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = crate::cli::run(ctx, &argv, deadline, cancel).map(|_| ());
    let (server, country) = match &outcome {
        Ok(()) => current_server(ctx).map_or((None, None), |(s, c)| (Some(s), c)),
        Err(_) => (None, None),
    };
    let duration_sec = finish(
        ctx,
        crate::state::HistoryAction::Reconnect,
        Some(&target),
        server.clone(),
        country.clone(),
        started,
        &outcome,
    );
    outcome?;
    let _ = crate::state::push_recent(ctx, &target);
    Ok(Connected {
        target: target.to_string(),
        server,
        country,
        engine: ctx.settings.engine.clone(),
        duration_sec,
        already_connected: false,
    })
}

/// The next server in the same country, by score, wrapping at the end of the list.
fn next_server(ctx: &Context, last: &Target) -> Result<Target, Error> {
    let anchor = match last {
        Target::Server(name) => name.clone(),
        _ => current_server(ctx).map(|(name, _)| name).ok_or_else(|| {
            Error::not_found("`--next` needs a known current server to step away from")
                .with_hint("fproton connect CH#461")
        })?,
    };
    let row = crate::index::find_server(ctx, &anchor)?.ok_or_else(|| {
        Error::not_found(format!("'{anchor}' is not in the server index"))
            .with_hint("fproton index --rebuild")
    })?;
    let rows = crate::index::servers(
        ctx,
        &crate::index::Query {
            country: Some(row.country.clone()),
            sort: crate::index::Sort::Score,
            ..Default::default()
        },
    )?;
    let usable: Vec<&crate::index::ServerRow> = rows
        .iter()
        .filter(|r| r.status == 1 && r.tier <= row.tier)
        .collect();
    if usable.is_empty() {
        return Err(
            Error::not_found(format!("no other usable server in {}", row.country))
                .with_hint("fproton servers --country CH"),
        );
    }
    let here = usable
        .iter()
        .position(|r| r.name.eq_ignore_ascii_case(&anchor));
    let pick = match here {
        Some(i) => usable[(i + 1) % usable.len()],
        None => usable[0],
    };
    Ok(Target::Server(pick.name.clone()))
}

/// Disconnect. `force` tears `proton0` and the killswitch profile down through nmcli.
///
/// `--force` is idempotent: tearing down a device that is already down, or a profile
/// that is not active, is success, so it stays a safe no-op on a machine with nothing
/// connected (§4b GREEN).
pub fn disconnect(ctx: &Context, force: bool, cancel: &CancelToken) -> Result<(), Error> {
    let deadline = if force {
        ctx.timeouts.nmcli()
    } else {
        ctx.timeouts.disconnect()
    };
    let started = begin(
        ctx,
        crate::status::PendingAction::Disconnect,
        None,
        deadline.as_secs(),
    );
    let server = current_server(ctx);
    let outcome = if force {
        crate::nm::device_disconnect(ctx, crate::TUNNEL_DEVICE, cancel)
            .and_then(|()| crate::nm::connection_down(ctx, crate::KILLSWITCH_PROFILE, cancel))
    } else {
        crate::cli::run(ctx, &["disconnect"], deadline, cancel).map(|_| ())
    };
    finish(
        ctx,
        crate::state::HistoryAction::Disconnect,
        None,
        server.as_ref().map(|(s, _)| s.clone()),
        server.and_then(|(_, c)| c),
        started,
        &outcome,
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reconnect_that_cannot_resolve_leaves_no_pending_record() {
        // `begin` writes `pending` and only `finish` clears it, so resolving after
        // `begin` left a record nothing would ever clear: `status` then reported
        // `connecting` for 90 s and invented a `last_error` at the end of it.
        //
        // The index is pointed at the small fixture so `resolve` has a real answer.
        // Without one it degrades to `Ok(())` and this test would spawn a real
        // `protonvpn connect` — which §1.2 says must never happen unbounded.
        let dir = std::env::temp_dir().join(format!(
            "fproton-engine-test-{}-{}",
            std::process::id(),
            crate::state::unix_now()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut ctx = Context::for_test(&dir).expect("test context");
        let proton = dir.join("proton");
        std::fs::create_dir_all(&proton).unwrap();
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/serverlist-small.json"
            ),
            proton.join("serverlist.json"),
        )
        .unwrap();
        ctx.paths.proton_cache_dir = proton;
        crate::index::rebuild(&ctx, &CancelToken::new()).expect("index from the fixture");

        let target = Target::Server("CH#99999".into());
        assert!(
            resolve(&ctx, &target).is_err(),
            "precondition: the index must be loaded and must NOT know CH#99999 — \
             otherwise this test spawns the real Proton CLI"
        );
        crate::state::push_recent(&ctx, &target).unwrap();
        let err = reconnect(&ctx, false, &CancelToken::new()).unwrap_err();
        assert_eq!(err.exit_code(), 6, "an unknown server is not_found");
        assert!(
            crate::state::pending(&ctx).unwrap().is_none(),
            "a refused reconnect must not leave a pending action behind"
        );
        assert!(crate::state::history(&ctx, 10).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_server_names_parse() {
        for name in [
            "CH#461",
            "IS-DE#1",
            "US-CA#1022",
            "NL-FREE#15",
            "US-CO#21-TOR",
            "JP#1",
        ] {
            assert!(is_server_name(name), "{name} must be a server name");
            assert_eq!(name.parse::<Target>().unwrap(), Target::Server(name.into()));
        }
    }

    #[test]
    fn injection_shaped_input_is_rejected() {
        for bad in [
            "; rm -rf",
            "--",
            "CH#461 foo",
            "$(id)",
            "CH#4611111",
            "ch#461#1",
            "CH#",
            "A",
            "ABCDEFGHIJKLMNOP",
        ] {
            assert!(
                bad.parse::<Target>().is_err() || !is_server_name(&bad.to_ascii_uppercase()),
                "{bad} must not become a target"
            );
        }
        let e = "; rm -rf".parse::<Target>().unwrap_err();
        assert_eq!(e.exit_code(), 2);
        // §2: the hint is a runnable command, and a shell-shaped value never reaches it.
        assert_eq!(e.hint(), "fproton servers");
        // A value with a safe prefix keeps it, so the hint actually finds what was meant,
        // and stops at the first character that is not (here `#`, which would comment out
        // the rest of the line if the hint were pasted into a shell).
        assert_eq!(
            "CH#461 foo".parse::<Target>().unwrap_err().hint(),
            "fproton servers --search CH"
        );
        for shell_shaped in ["; rm -rf", "$(id)", "`id`", "a|b", "--", "&& reboot"] {
            let hint = shell_shaped.parse::<Target>().unwrap_err().hint();
            assert!(
                !hint.contains(['$', '`', ';', '|', '&', '(', ')', '#']),
                "hint for {shell_shaped:?} leaked a shell metacharacter: {hint}"
            );
        }
    }

    #[test]
    fn every_target_form_round_trips() {
        let cases = [
            ("fastest", Target::Fastest),
            ("CH", Target::Country("CH".into())),
            ("CH#461", Target::Server("CH#461".into())),
            ("city:New York", Target::City("New York".into())),
            ("p2p", Target::P2p),
            ("secure-core", Target::SecureCore),
            ("tor", Target::Tor),
            ("random", Target::Random),
        ];
        for (text, target) in cases {
            assert_eq!(text.parse::<Target>().unwrap(), target, "parsing {text}");
            assert_eq!(target.to_string(), text, "displaying {text}");
            assert_eq!(target.to_string().parse::<Target>().unwrap(), target);
        }
    }

    #[test]
    fn lowercase_and_quoted_input_is_normalised_not_rejected() {
        assert_eq!(
            "ch".parse::<Target>().unwrap(),
            Target::Country("CH".into())
        );
        assert_eq!(
            "ch#461".parse::<Target>().unwrap(),
            Target::Server("CH#461".into())
        );
        assert_eq!(
            "city:\"New York\"".parse::<Target>().unwrap(),
            Target::City("New York".into())
        );
        assert_eq!("securecore".parse::<Target>().unwrap(), Target::SecureCore);
    }

    #[test]
    fn city_names_with_odd_letters_are_allowed_but_metacharacters_are_not() {
        assert!(is_city_name("Zürich"));
        assert!(is_city_name("São Paulo"));
        assert!(is_city_name("Coeur d'Alene"));
        assert!(!is_city_name("New York; rm -rf"));
        assert!(!is_city_name(""));
        assert!(!is_city_name(&"x".repeat(41)));
    }

    #[test]
    fn target_for_server_rejects_non_servers() {
        assert_eq!(
            target_for_server("ch#461").unwrap(),
            Target::Server("CH#461".into())
        );
        assert_eq!(target_for_server("fastest").unwrap_err().exit_code(), 6);
    }

    #[test]
    fn targets_serialise_as_the_string_the_cli_accepts() {
        let t = Target::City("New York".into());
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"city:New York\"");
        let back: Target = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
