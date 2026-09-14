//! Desktop integration (DESIGN.md §4 `setup`): idempotent, and never a parse-and-rewrite
//! of the user's own files.
//!
//! Phase 1a froze [`SetupArgs`] and [`run`]; Phase 1b implements them. `--menu` inserts
//! the entries of `assets/menu.jsonc` textually into
//! `~/.config/omarchy/extensions/omarchy-menu.jsonc`, after a verbatim backup: a serde
//! round-trip would delete every comment in a file the owner maintains by hand.

use clap::Args;
use fproton_core::{Context, Error};

/// The menu entries `--menu` merges. Single-line objects: the merge inserts lines.
// Consumed by the Phase-1b `--menu` merge.
#[allow(dead_code)]
pub const MENU_TEMPLATE: &str = include_str!("../assets/menu.jsonc");

/// `fproton setup [--plugin] [--menu] [--disable-old-plugin] [--all]`.
#[derive(Args, Debug, Default, Clone)]
pub struct SetupArgs {
    /// Link and enable the bar widget, and place it before omarchy.tailscale
    #[arg(long)]
    pub plugin: bool,
    /// Merge fproton's entries into the Omarchy menu
    #[arg(long)]
    pub menu: bool,
    /// Disable the old io.github.grichard99.omaproton-vpn plugin if it is enabled
    #[arg(long)]
    pub disable_old_plugin: bool,
    /// All of the above
    #[arg(long)]
    pub all: bool,
    /// Set from the global --yes (not a flag of its own).
    #[clap(skip)]
    pub yes: bool,
    /// Set from the global --dry-run (not a flag of its own).
    #[clap(skip)]
    pub dry_run: bool,
}

/// Run the requested integration steps. Idempotent: a second run changes nothing.
pub fn run(_args: SetupArgs, _ctx: Context) -> anyhow::Result<i32> {
    Err(Error::unimplemented("setup").into())
}
