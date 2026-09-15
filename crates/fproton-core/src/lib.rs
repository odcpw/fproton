//! fproton's engine: everything the CLI, the TUI, the doctor and the bar widget share.
//!
//! **This crate is silent.** No function here writes to stdout or stderr; all output
//! is produced by the binary (`output.rs`); a Phase-3 gate greps this crate for print macros.
//!
//! The public surface was frozen at the Phase 1a gate (DESIGN.md §8): later phases fill
//! bodies in, they do not change signatures. Every frozen function returns
//! `Result<_, Error>` so the binary maps a failure to its §4a exit code without
//! downcasting.

pub mod agent;
pub mod cli;
pub mod context;
pub mod engine;
pub mod error;
pub mod index;
pub mod natpmp;
pub mod nm;
pub mod paths;
pub mod proc;
pub mod route;
pub mod secrets;
pub mod settings;
pub mod state;
pub mod status;
pub mod wg;

pub use context::Context;
pub use engine::{Connected, Target};
pub use error::{Error, EXIT_CODES};
pub use index::{CityRow, CountryRow, IndexInfo, IndexStats, Query, ServerRow};
pub use paths::Paths;
pub use proc::{CancelToken, Output};
pub use secrets::AliasProbe;
pub use settings::{Settings, Timeouts};
pub use state::HistoryEntry;
pub use status::{
    Account, CliState, Installed, KillSwitch, LastError, Link, Pending, PendingAction,
    PortForwarding, ProtonConfig, Server, Severity, State, Status, Traffic, Warning,
    SCHEMA_VERSION, WARNING_IDS,
};

/// The tool's name, as it appears in argv, hints and the plugin.
pub const NAME: &str = "fproton";
/// The workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The machine contract's version, independent of the tool version.
pub const CONTRACT_VERSION: &str = "1.0.0";
/// The tunnel device the Proton CLI creates (§1.3).
pub const TUNNEL_DEVICE: &str = "proton0";
/// The IPv6 leak-guard profile the Proton CLI creates (§1.3).
pub const KILLSWITCH_PROFILE: &str = "pvpn-killswitch-ipv6";

/// The whole of §5, without ever spawning `protonvpn` (see [`status::status`]).
pub fn status(ctx: &Context) -> Result<Status, Error> {
    status::status(ctx)
}

/// Now, as RFC 3339 with the **local** offset and second precision (§5).
pub fn now_rfc3339() -> String {
    let zoned = jiff::Zoned::now();
    let ts = zoned.timestamp();
    let ts = ts.round(jiff::Unit::Second).unwrap_or(ts);
    ts.display_with_offset(zoned.offset()).to_string()
}

/// A `SystemTime` (a file mtime, say) as RFC 3339 with the local offset.
pub fn rfc3339_from_system_time(t: std::time::SystemTime) -> Option<String> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    rfc3339_from_unix(secs)
}

/// A Unix epoch second (Proton's `ExpirationTime` floats) as RFC 3339, local offset.
pub fn rfc3339_from_unix(seconds: i64) -> Option<String> {
    let ts = jiff::Timestamp::from_second(seconds).ok()?;
    let offset = jiff::tz::TimeZone::system().to_offset(ts);
    Some(ts.display_with_offset(offset).to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamps_are_rfc3339_with_a_local_offset_and_no_zulu() {
        let now = super::now_rfc3339();
        assert_eq!(
            now.len(),
            25,
            "expected 2026-09-14T22:30:00+02:00, got {now}"
        );
        assert!(
            !now.ends_with('Z'),
            "§5 wants a local offset, not Zulu: {now}"
        );
        assert!(now.contains('T'));
        let epoch = super::rfc3339_from_unix(0).expect("epoch");
        assert!(epoch.starts_with("19") || epoch.starts_with("20"));
    }
}
