//! XDG state: the pending action, history, recents, the CLI-hang breaker and the
//! caches `status` reads (DESIGN.md §3, §5). Types frozen here; the store is Phase 1b.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::engine::Target;
use crate::error::Error;
use crate::status::{CliState, Pending, PortForwarding, ProtonConfig, Traffic};

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

/// The on-disk shape of `state.json`. Phase 1b owns the reader/writer; the shape is
/// frozen so the doctor's Class A fixers and the TUI agree about what they repair.
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
}

// ---------------------------------------------------------------- frozen API

/// The newest `limit` history entries, newest first.
pub fn history(_ctx: &Context, _limit: usize) -> Result<Vec<HistoryEntry>, Error> {
    Err(Error::unimplemented("state::history"))
}

/// Recently used targets, newest first, de-duplicated. Feeds the panel's `recents`.
pub fn recents(_ctx: &Context) -> Result<Vec<Target>, Error> {
    Err(Error::unimplemented("state::recents"))
}

/// The pending action, if one was recorded (`elapsed_sec` recomputed on read).
pub fn pending(_ctx: &Context) -> Result<Option<Pending>, Error> {
    Err(Error::unimplemented("state::pending"))
}

/// Clear a stale pending action (doctor Class A `stale_pending_action`).
pub fn clear_pending(_ctx: &Context) -> Result<(), Error> {
    Err(Error::unimplemented("state::clear_pending"))
}

/// Proton's settings as of the last successful `config list` (`config get` reads this).
pub fn config_cache(_ctx: &Context) -> Result<Option<ProtonConfig>, Error> {
    Err(Error::unimplemented("state::config_cache"))
}
