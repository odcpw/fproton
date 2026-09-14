//! The doctor: detect, report, and apply a small set of reversible repairs (DESIGN.md §7).
//!
//! Phase 1a froze [`DoctorArgs`], [`DoctorCmd`] and [`run`]; the Phase-2 doctor agent
//! fills this module in (adding `checks.rs`, `probes.rs`, `mutate.rs`, `report.rs`).
//! Detectors are pure functions of captured `Probes`; only `probes.rs` spawns anything;
//! every mutation goes through one `mutate(Action)` chokepoint and is undoable.
//! The doctor never deletes a file, a NetworkManager profile or a keyring entry.

use clap::{Args, Subcommand, ValueEnum};
use fproton_core::{Context, Error};

/// The check groups `--only` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DoctorGroup {
    Cli,
    Keyring,
    Network,
    Cache,
    Install,
    Connected,
}

// Used by the Phase-2 doctor agent's report and `doctor capabilities`.
#[allow(dead_code)]
impl DoctorGroup {
    pub fn name(self) -> &'static str {
        match self {
            DoctorGroup::Cli => "cli",
            DoctorGroup::Keyring => "keyring",
            DoctorGroup::Network => "network",
            DoctorGroup::Cache => "cache",
            DoctorGroup::Install => "install",
            DoctorGroup::Connected => "connected",
        }
    }
}

/// `fproton doctor [FLAGS]` / `fproton doctor undo RUN` / `fproton doctor capabilities`.
#[derive(Args, Debug, Default, Clone)]
pub struct DoctorArgs {
    /// Apply the fixable findings (each one reversible with `doctor undo latest`)
    #[arg(long)]
    pub fix: bool,
    /// Also allow the foreign-state keyring fixer (Class C)
    #[arg(long)]
    pub allow_keyring: bool,
    /// Only run one group of checks
    #[arg(long, value_enum, value_name = "GROUP")]
    pub only: Option<DoctorGroup>,
    /// Write the run artifacts even without --fix
    #[arg(long)]
    pub save: bool,
    #[command(subcommand)]
    pub sub: Option<DoctorCmd>,
    /// Set from the global --yes (not a flag of its own).
    #[clap(skip)]
    pub yes: bool,
    /// Set from the global --dry-run (not a flag of its own).
    #[clap(skip)]
    pub dry_run: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DoctorCmd {
    /// Replay a `--fix` run in reverse (exit 13 if a backup is missing or altered)
    Undo {
        /// A run id, or `latest`
        #[arg(default_value = "latest")]
        run: String,
    },
    /// List every check, its group and its safety class
    Capabilities,
}

/// Run the doctor. Exit 0 when everything passes, 11 on failures, 12 when a fixer
/// failed, 13 when a precondition or an undo integrity check refused.
pub fn run(_args: DoctorArgs, _ctx: Context) -> anyhow::Result<i32> {
    Err(Error::unimplemented("the doctor").into())
}
