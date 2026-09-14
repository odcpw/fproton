//! The FrankenTUI app: Status · Servers · History (DESIGN.md §7a).
//!
//! Phase 1a froze [`TuiArgs`] and [`run`]; the Phase-2 TUI agent fills this module in
//! (and may add fields to `TuiArgs`, but not change `run`'s signature or touch
//! `main.rs`). The TUI owns its `Context` clone and calls `fproton-core` in process —
//! it never spawns `fproton`.

use clap::Args;
use fproton_core::{Context, Error};

/// Which screen to open on. Consumed by the Phase-2 TUI agent.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Status,
    Servers,
    History,
}

/// `fproton tui [--servers] [--history]`.
#[derive(Args, Debug, Default, Clone)]
pub struct TuiArgs {
    /// Open on the Servers screen
    #[arg(long)]
    pub servers: bool,
    /// Open on the History screen
    #[arg(long)]
    pub history: bool,
}

#[allow(dead_code)]
impl TuiArgs {
    /// The screen the flags ask for.
    pub fn screen(&self) -> Screen {
        if self.servers {
            Screen::Servers
        } else if self.history {
            Screen::History
        } else {
            Screen::Status
        }
    }
}

/// Run the TUI. Returns the process exit code: 0 on a clean quit, 130 on Ctrl+C, and
/// otherwise the §4a code of the session's last failed action (§7a). Never invents one.
pub fn run(_args: TuiArgs, _ctx: Context) -> anyhow::Result<i32> {
    Err(Error::unimplemented("the TUI").into())
}
