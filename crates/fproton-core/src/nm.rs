//! NetworkManager: profiles, link state and sysfs traffic (DESIGN.md §1.3, §5).
//!
//! Every nmcli call is argv-only, under the `nmcli` deadline, and works without sudo
//! or a polkit prompt on this machine. `nmcli --show-secrets` is **never** invoked.
//! Parsing is fixture-driven (Phase 1b); the types are frozen here.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;
use crate::status::Traffic;

/// One row of `nmcli -t -f NAME,UUID,TYPE,DEVICE,ACTIVE connection show`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NmConnection {
    pub name: String,
    pub uuid: String,
    /// `wireguard`, `dummy`, `wifi`, `tun`, …
    pub kind: String,
    /// Empty when the profile is not attached to a device.
    pub device: Option<String>,
    pub active: bool,
}

impl NmConnection {
    /// The CLI names its profiles `ProtonVPN <server>` (§1.3).
    pub fn is_proton_profile(&self) -> bool {
        self.name.starts_with("ProtonVPN ") && self.kind == "wireguard"
    }
    /// The server name inside a `ProtonVPN <name>` profile.
    pub fn proton_server_name(&self) -> Option<&str> {
        self.name.strip_prefix("ProtonVPN ")
    }
}

/// The tunnel device as NetworkManager sees it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LinkState {
    pub up: bool,
    pub device: Option<String>,
    pub nm_connection: Option<String>,
    /// The profile's UUID — the only safe join key to Proton's persistence file (§5).
    pub uuid: Option<String>,
    pub since: Option<String>,
    /// `pvpn-killswitch-ipv6` is active.
    pub killswitch_active: bool,
    /// An active tun/wireguard device that is not `proton0` (tailscale0 here).
    pub conflicting_tunnels: Vec<String>,
}

// ---------------------------------------------------------------- frozen API

/// Every profile NetworkManager knows, from one `nmcli` call.
pub fn connections(_ctx: &Context) -> Result<Vec<NmConnection>, Error> {
    Err(Error::unimplemented("nm::connections"))
}

/// Parse the `-t` (colon-separated) connection listing. Pure; fixture-tested in 1b.
pub fn parse_connections(_text: &str) -> Result<Vec<NmConnection>, Error> {
    Err(Error::unimplemented("nm::parse_connections"))
}

/// Link state derived from one `connections()` call — `status` never runs a second.
pub fn link_state(_ctx: &Context) -> Result<LinkState, Error> {
    Err(Error::unimplemented("nm::link_state"))
}

/// `/sys/class/net/<device>/statistics/{rx,tx}_bytes`; `Ok(None)` when the device is gone.
pub fn traffic(_ctx: &Context, _device: &str) -> Result<Option<Traffic>, Error> {
    Err(Error::unimplemented("nm::traffic"))
}

/// `nmcli connection down <name>` (used by `disconnect --force`).
pub fn connection_down(_ctx: &Context, _name: &str, _cancel: &CancelToken) -> Result<(), Error> {
    Err(Error::unimplemented("nm::connection_down"))
}

/// `nmcli device disconnect <device>` (used by `disconnect --force`).
pub fn device_disconnect(
    _ctx: &Context,
    _device: &str,
    _cancel: &CancelToken,
) -> Result<(), Error> {
    Err(Error::unimplemented("nm::device_disconnect"))
}
