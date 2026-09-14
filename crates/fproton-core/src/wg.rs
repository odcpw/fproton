//! EXPERIMENT (DESIGN.md §4 `wg`): CLI-free reconnect through NetworkManager.
//!
//! Env-gated behind `FPROTON_EXPERIMENTAL_WG=1`, built last, never load-bearing:
//! nothing in §5, §6, §7 or any gate depends on it. `up` activates an existing
//! `ProtonVPN *` profile; `swap` clones one and retargets the clone's peer at another
//! server. **No key material is ever read**: NetworkManager copies the key itself.
//!
//! Kill criterion and expiry are in DESIGN.md §4; delete this file when it fires.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::engine::Target;
use crate::error::Error;
use crate::proc::CancelToken;

/// The gate. Every `wg` subcommand refuses with exit 2 unless this is set.
pub const ENV_GATE: &str = "FPROTON_EXPERIMENTAL_WG";
/// Printed by every `wg` invocation, in both human and JSON output.
pub const EXPERIMENTAL_NOTE: &str = "experimental: unverified on this machine";

/// True when `FPROTON_EXPERIMENTAL_WG=1` is set.
pub fn enabled() -> bool {
    std::env::var(ENV_GATE).as_deref() == Ok("1")
}

/// The refusal every `wg` subcommand returns when the gate is closed.
pub fn gate_error() -> Error {
    Error::usage("`wg` is an unverified experiment and is gated off")
        .with_hint("FPROTON_EXPERIMENTAL_WG=1 fproton wg list")
}

/// A `ProtonVPN *` WireGuard profile as `wg list` shows it. No key fields, ever.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WgProfile {
    pub name: String,
    pub uuid: String,
    pub server: Option<String>,
    pub endpoint: Option<String>,
    pub active: bool,
    /// True when fproton's `swap` created this clone.
    pub clone_of: Option<String>,
}

// ---------------------------------------------------------------- frozen API

/// Activate an existing `ProtonVPN *` profile (the CLI's own, which holds the key).
pub fn up(_ctx: &Context, _name: Option<&str>, _cancel: &CancelToken) -> Result<WgProfile, Error> {
    Err(Error::unimplemented("wg::up"))
}

/// Clone a profile and retarget the clone's peer at another server from the index.
pub fn swap(
    _ctx: &Context,
    _name: &str,
    _target: &Target,
    _cancel: &CancelToken,
) -> Result<WgProfile, Error> {
    Err(Error::unimplemented("wg::swap"))
}

/// Every `ProtonVPN *` profile NetworkManager knows.
pub fn list(_ctx: &Context) -> Result<Vec<WgProfile>, Error> {
    Err(Error::unimplemented("wg::list"))
}
