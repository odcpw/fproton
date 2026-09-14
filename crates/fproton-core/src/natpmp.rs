//! One-shot NAT-PMP (RFC 6886) port forwarding through the tunnel (DESIGN.md §4 `port`).
//!
//! std UDP only, to 10.2.0.1:5351 inside the tunnel, 60 s lifetime. `status` never
//! sends a packet; only `fproton port` does. Codec and query are Phase 1b.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// The NAT-PMP gateway inside a Proton tunnel (§1.3).
pub const GATEWAY: &str = "10.2.0.1";
pub const PORT: u16 = 5351;
/// Proton hands out 60 s leases; an external script must renew them.
pub const LIFETIME_SEC: u32 = 60;

/// A granted mapping, as `fproton port --json` prints it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortMapping {
    pub port: u16,
    pub internal_port: u16,
    /// RFC 3339 with local offset.
    pub expires_at: String,
    pub lifetime_sec: u32,
    pub gateway: String,
    /// `udp` and `tcp` are requested separately; Proton grants the same port for both.
    pub protocol: String,
}

// ---------------------------------------------------------------- frozen API

/// Ask the gateway for a mapping. Exit 6 when there is no tunnel.
pub fn request_port(_ctx: &Context, _cancel: &CancelToken) -> Result<PortMapping, Error> {
    Err(Error::unimplemented("natpmp::request_port"))
}
