//! XDG state: the pending action, history, recents, the CLI-hang breaker and the
//! caches `status` reads (DESIGN.md §3, §5).
//!
//! One file, `$XDG_STATE_HOME/fproton/state.json`, written atomically
//! (`state.json.tmp.<pid>` then `rename(2)`) so a concurrent reader never sees half a
//! document and a crash never truncates the old one. A state file that cannot be
//! parsed is treated as empty rather than fatal: state is a cache, not a source of
//! truth, and `fproton status` must keep answering.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::engine::Target;
use crate::error::Error;
use crate::status::{Account, CliState, Pending, PortForwarding, ProtonConfig, Traffic};

/// How many history entries survive a write.
pub const HISTORY_CAP: usize = 200;
/// How many distinct targets `recents` keeps.
pub const RECENTS_CAP: usize = 10;
/// How long a deadline-killed `protonvpn` short-circuits CLI-backed commands (§2).
pub const CLI_BACKOFF_SEC: u64 = 60;
/// How long the Secret Service answer is cached (§5 `signed_in`).
pub const SIGNED_IN_CACHE_SEC: u64 = 60;

/// What an entry of `fproton history` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryAction {
    Connect,
    Disconnect,
    Reconnect,
}

impl HistoryAction {
    pub fn name(self) -> &'static str {
        match self {
            HistoryAction::Connect => "connect",
            HistoryAction::Disconnect => "disconnect",
            HistoryAction::Reconnect => "reconnect",
        }
    }
}

/// How it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryResult {
    Ok,
    Error,
    Cancelled,
    Timeout,
}

impl HistoryResult {
    pub fn name(self) -> &'static str {
        match self {
            HistoryResult::Ok => "ok",
            HistoryResult::Error => "error",
            HistoryResult::Cancelled => "cancelled",
            HistoryResult::Timeout => "timeout",
        }
    }
}

/// One row of `fproton history` and of the TUI's History screen (§5).
///
/// `target` is the exact string `fproton connect` accepts, so History → Enter →
/// reconnect needs no re-parsing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// RFC 3339 with local offset.
    pub at: String,
    pub action: HistoryAction,
    pub target: Option<String>,
    pub server: Option<String>,
    pub country: Option<String>,
    pub engine: String,
    pub result: HistoryResult,
    pub code_name: Option<String>,
    pub duration_sec: u64,
}

/// The on-disk shape of `state.json`. The doctor's Class A fixers and the TUI agree
/// about what they repair through this struct; every field is `#[serde(default)]`,
/// so an older or newer file still loads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StateFile {
    pub pending: Option<Pending>,
    pub history: Vec<HistoryEntry>,
    pub recents: Vec<String>,
    pub cli: CliState,
    pub config: Option<ProtonConfig>,
    pub port_forwarding: PortForwarding,
    /// Previous sysfs sample, so `traffic.*_bps` can be derived.
    pub traffic: Option<Traffic>,
    /// `signed_in` from the Secret Service probe, cached for 60 s.
    pub signed_in: Option<bool>,
    pub signed_in_at: Option<String>,
    /// When `killswitch_orphan` was first observed (its fixer needs a prior run, §7).
    pub killswitch_orphan_seen_at: Option<String>,
    /// UUID of the profile the link was carrying when fproton first saw it up.
    pub link_uuid: Option<String>,
    /// When fproton first observed the current link up (§5 `link.since`).
    pub link_up_since: Option<String>,
    /// The last error, as `status.last_error` reports it.
    pub last_error: Option<crate::status::LastError>,
    /// The Proton account name from the last successful `protonvpn info` (§5).
    pub account: Option<crate::status::Account>,
}

// ---------------------------------------------------------------- the store

/// Unix seconds now, for the small amount of arithmetic this module does.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse an RFC 3339 timestamp fproton itself wrote back into Unix seconds.
pub fn unix_from_rfc3339(text: &str) -> Option<i64> {
    text.parse::<jiff::Timestamp>().ok().map(|t| t.as_second())
}

/// Seconds elapsed since an RFC 3339 timestamp; `None` when it cannot be read.
pub fn seconds_since(text: &str) -> Option<u64> {
    let then = unix_from_rfc3339(text)?;
    Some((unix_now() as i64 - then).max(0) as u64)
}

/// Read `state.json`. A missing or unreadable file is an empty state, never an error:
/// everything in it is a cache, and `status` must answer regardless.
pub fn load(ctx: &Context) -> Result<StateFile, Error> {
    let path = ctx.paths.state_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StateFile::default()),
        Err(_) => return Ok(StateFile::default()),
    };
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

/// Write `state.json` atomically: temp file in the same directory, then `rename(2)`.
pub fn save(ctx: &Context, state: &StateFile) -> Result<(), Error> {
    ctx.paths.ensure_dirs()?;
    let path = ctx.paths.state_file();
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let text = serde_json::to_string_pretty(state)
        .map_err(|e| Error::failure(format!("cannot serialise state: {e}")))?;
    std::fs::write(&tmp, text)
        .map_err(|e| Error::failure(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::failure(format!("cannot replace {}: {e}", path.display()))
    })
}

/// Read, change, write back. The whole file is rewritten, so a caller never has to
/// think about partial updates.
pub fn update<T>(ctx: &Context, f: impl FnOnce(&mut StateFile) -> T) -> Result<T, Error> {
    let mut state = load(ctx)?;
    let value = f(&mut state);
    save(ctx, &state)?;
    Ok(value)
}

/// Recompute `elapsed_sec` against the wall clock, so a `Pending` read back from
/// disk answers `is_live()` honestly.
pub fn refresh_pending(pending: &mut Pending) {
    if let Some(elapsed) = seconds_since(&pending.started_at) {
        pending.elapsed_sec = elapsed;
    }
}

/// Record an action as in flight (`engine` calls this before spawning, §8).
pub fn set_pending(ctx: &Context, pending: Pending) -> Result<(), Error> {
    update(ctx, |s| {
        s.pending = Some(pending);
        // §5: any new action clears the previous error.
        s.last_error = None;
    })
}

/// Append one history entry (newest last on disk; `history()` reverses it).
pub fn push_history(ctx: &Context, entry: HistoryEntry) -> Result<(), Error> {
    update(ctx, |s| {
        s.history.push(entry);
        let len = s.history.len();
        if len > HISTORY_CAP {
            s.history.drain(..len - HISTORY_CAP);
        }
    })
}

/// Remember a target the user actually asked for, newest first, de-duplicated.
pub fn push_recent(ctx: &Context, target: &Target) -> Result<(), Error> {
    let text = target.to_string();
    update(ctx, |s| {
        s.recents.retain(|t| t != &text);
        s.recents.insert(0, text);
        s.recents.truncate(RECENTS_CAP);
    })
}

/// Record the last failure (§5 `last_error`).
pub fn set_last_error(ctx: &Context, error: &Error, text: Option<String>) -> Result<(), Error> {
    let entry = crate::status::LastError {
        at: crate::now_rfc3339(),
        code: error.exit_code(),
        code_name: error.code_name().to_string(),
        text: text.unwrap_or_else(|| error.to_string()),
        hint: error.hint(),
    };
    update(ctx, |s| s.last_error = Some(entry))
}

/// The Proton CLI answered: clear the breaker.
pub fn note_cli_ok(ctx: &Context) -> Result<(), Error> {
    update(ctx, |s| {
        s.cli = CliState {
            last_ok_at: Some(crate::now_rfc3339()),
            hang_suspected: false,
            backoff_until: None,
        };
    })
}

/// The Proton CLI was killed at its deadline: arm the 60 s breaker (§2).
pub fn note_cli_hang(ctx: &Context) -> Result<(), Error> {
    let until = crate::rfc3339_from_unix(unix_now() as i64 + CLI_BACKOFF_SEC as i64);
    update(ctx, |s| {
        s.cli.hang_suspected = true;
        s.cli.backoff_until = until;
    })
}

/// Seconds left on the CLI backoff, or `None` when CLI commands may run.
pub fn cli_backoff_remaining(state: &StateFile) -> Option<u64> {
    let until = state.cli.backoff_until.as_deref()?;
    let left = unix_from_rfc3339(until)? - unix_now() as i64;
    (left > 0).then_some(left as u64)
}

/// The error a CLI-backed command gets while the breaker is armed (§2): exit 5,
/// immediately, with the doctor hint. `--force-cli` is not in v0.1; `--timeout`
/// does not lift it, and every e2e case runs in its own state dir because of that.
pub fn cli_backoff_error(seconds_left: u64) -> Error {
    Error::upstream(format!(
        "the Proton CLI was killed at its deadline less than {CLI_BACKOFF_SEC}s ago; \
         refusing to spawn it again for {seconds_left}s"
    ))
    .with_hint("fproton doctor --only cli")
}

/// Cache a successful `config list` (§5 `config`).
pub fn set_config_cache(ctx: &Context, config: &ProtonConfig) -> Result<(), Error> {
    let mut config = config.clone();
    config.cached_at = Some(crate::now_rfc3339());
    update(ctx, |s| s.config = Some(config))
}

/// Cache the account name from a successful `protonvpn info` (§5 `account`).
pub fn set_account(ctx: &Context, name: &str) -> Result<Account, Error> {
    let account = Account {
        name: name.to_string(),
        cached_at: crate::now_rfc3339(),
    };
    let stored = account.clone();
    update(ctx, |s| s.account = Some(stored))?;
    Ok(account)
}

/// Cache the Secret Service answer for [`SIGNED_IN_CACHE_SEC`] (§5 `signed_in`).
pub fn set_signed_in(ctx: &Context, signed_in: Option<bool>) -> Result<(), Error> {
    let at = crate::now_rfc3339();
    update(ctx, |s| {
        s.signed_in = signed_in;
        s.signed_in_at = Some(at);
    })
}

/// True while the cached `signed_in` answer may still be used.
pub fn signed_in_is_fresh(state: &StateFile) -> bool {
    state
        .signed_in_at
        .as_deref()
        .and_then(seconds_since)
        .is_some_and(|age| age < SIGNED_IN_CACHE_SEC)
}

// ---------------------------------------------------------------- frozen API

/// The newest `limit` history entries, newest first.
pub fn history(ctx: &Context, limit: usize) -> Result<Vec<HistoryEntry>, Error> {
    let state = load(ctx)?;
    Ok(state.history.into_iter().rev().take(limit).collect())
}

/// Recently used targets, newest first, de-duplicated. Feeds the panel's `recents`.
///
/// A stored string that no longer parses (a target form that was dropped) is skipped
/// rather than failing the call: `recents` is a convenience list.
pub fn recents(ctx: &Context) -> Result<Vec<Target>, Error> {
    let state = load(ctx)?;
    Ok(state
        .recents
        .iter()
        .filter_map(|t| t.parse::<Target>().ok())
        .collect())
}

/// The pending action, if one was recorded (`elapsed_sec` recomputed on read).
pub fn pending(ctx: &Context) -> Result<Option<Pending>, Error> {
    let state = load(ctx)?;
    Ok(state.pending.map(|mut p| {
        refresh_pending(&mut p);
        p
    }))
}

/// Clear a stale pending action (doctor Class A `stale_pending_action`).
pub fn clear_pending(ctx: &Context) -> Result<(), Error> {
    update(ctx, |s| s.pending = None)
}

/// Proton's settings as of the last successful `config list` (`config get` reads this).
pub fn config_cache(ctx: &Context) -> Result<Option<ProtonConfig>, Error> {
    Ok(load(ctx)?.config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::PendingAction;

    fn test_ctx(name: &str) -> (Context, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("fproton-state-test-{name}-{}", unix_now()));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = Context::for_test(&dir).expect("test context");
        (ctx, dir)
    }

    #[test]
    fn a_missing_state_file_is_an_empty_state_not_an_error() {
        let (ctx, dir) = test_ctx("missing");
        assert!(load(&ctx).unwrap().history.is_empty());
        assert!(pending(&ctx).unwrap().is_none());
        assert!(history(&ctx, 20).unwrap().is_empty());
        assert!(config_cache(&ctx).unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_state_file_degrades_to_empty_rather_than_failing_status() {
        let (ctx, dir) = test_ctx("corrupt");
        ctx.paths.ensure_dirs().unwrap();
        std::fs::write(ctx.paths.state_file(), "{not json").unwrap();
        assert!(load(&ctx).unwrap().history.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn history_is_newest_first_and_capped() {
        let (ctx, dir) = test_ctx("history");
        for i in 0..(HISTORY_CAP + 5) {
            push_history(
                &ctx,
                HistoryEntry {
                    at: crate::now_rfc3339(),
                    action: HistoryAction::Connect,
                    target: Some(format!("CH#{i}")),
                    server: None,
                    country: Some("CH".into()),
                    engine: "cli".into(),
                    result: HistoryResult::Ok,
                    code_name: None,
                    duration_sec: 1,
                },
            )
            .unwrap();
        }
        let rows = history(&ctx, 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].target.as_deref(), Some("CH#204"));
        assert_eq!(load(&ctx).unwrap().history.len(), HISTORY_CAP);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recents_are_deduplicated_newest_first_and_round_trip_as_targets() {
        let (ctx, dir) = test_ctx("recents");
        for t in ["CH", "p2p", "CH#461", "CH"] {
            push_recent(&ctx, &t.parse::<Target>().unwrap()).unwrap();
        }
        let got: Vec<String> = recents(&ctx)
            .unwrap()
            .iter()
            .map(|t| t.to_string())
            .collect();
        assert_eq!(got, vec!["CH", "CH#461", "p2p"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pending_action_recomputes_its_own_elapsed_time_on_read() {
        let (ctx, dir) = test_ctx("pending");
        let started = crate::rfc3339_from_unix(unix_now() as i64 - 5).unwrap();
        set_pending(
            &ctx,
            Pending {
                action: PendingAction::Connect,
                target: Some("fastest".into()),
                started_at: started,
                deadline_sec: 90,
                elapsed_sec: 0,
                pid: Some(std::process::id()),
            },
        )
        .unwrap();
        let p = pending(&ctx).unwrap().expect("pending");
        assert!(
            (4..=7).contains(&p.elapsed_sec),
            "elapsed {}",
            p.elapsed_sec
        );
        assert!(p.is_live());
        clear_pending(&ctx).unwrap();
        assert!(pending(&ctx).unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_cli_breaker_arms_for_sixty_seconds_and_clears_on_success() {
        let (ctx, dir) = test_ctx("breaker");
        note_cli_hang(&ctx).unwrap();
        let s = load(&ctx).unwrap();
        assert!(s.cli.hang_suspected);
        let left = cli_backoff_remaining(&s).expect("backoff armed");
        assert!((55..=60).contains(&left), "left {left}");
        assert_eq!(cli_backoff_error(left).exit_code(), 5);
        note_cli_ok(&ctx).unwrap();
        let s = load(&ctx).unwrap();
        assert!(!s.cli.hang_suspected);
        assert!(cli_backoff_remaining(&s).is_none());
        assert!(s.last_ok_is_set());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_action_clears_the_last_error() {
        let (ctx, dir) = test_ctx("lasterror");
        set_last_error(&ctx, &Error::upstream("killed at its deadline"), None).unwrap();
        assert!(load(&ctx).unwrap().last_error.is_some());
        set_pending(
            &ctx,
            Pending {
                action: PendingAction::Disconnect,
                target: None,
                started_at: crate::now_rfc3339(),
                deadline_sec: 25,
                elapsed_sec: 0,
                pid: None,
            },
        )
        .unwrap();
        assert!(load(&ctx).unwrap().last_error.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file_behind() {
        let (ctx, dir) = test_ctx("atomic");
        save(&ctx, &StateFile::default()).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&ctx.paths.state_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    impl StateFile {
        fn last_ok_is_set(&self) -> bool {
            self.cli.last_ok_at.is_some()
        }
    }
}
