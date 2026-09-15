//! NetworkManager: profiles, link state and sysfs traffic (DESIGN.md §1.3, §5).
//!
//! Every nmcli call is argv-only, under the `nmcli` deadline, and works without sudo
//! or a polkit prompt on this machine. `nmcli --show-secrets` is **never** invoked.
//! Parsing is fixture-driven; the types were frozen at the Phase 1a gate.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::{run_with_deadline, CancelToken, Output};
use crate::status::Traffic;
use crate::{KILLSWITCH_PROFILE, TUNNEL_DEVICE};

/// The nmcli binary, spelled once.
pub const PROGRAM: &str = "nmcli";

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
    /// Active tun/wireguard devices other than `proton0` (tailscale0 here). Being
    /// up is not a conflict by itself; `route::conflicts` decides (§5).
    pub conflicting_tunnels: Vec<String>,
}

/// Tunnel-shaped connection types, for the `conflicting_tunnel` warning (§5).
const TUNNEL_KINDS: &[&str] = &["wireguard", "tun", "vpn", "ip-tunnel"];

/// Turn one nmcli exit into a typed error (§4a): a deadline kill is 5, a polkit
/// denial is 4; a missing binary already came back from `proc` as 4.
fn check(out: Output, what: &str, deadline: Duration) -> Result<Output, Error> {
    if out.timed_out {
        return Err(Error::upstream(format!(
            "{what} was killed at its {}s deadline",
            deadline.as_secs()
        ))
        .with_hint("fproton doctor --only network"));
    }
    if !out.success() {
        let tail = out.stderr_tail();
        let lower = tail.to_ascii_lowercase();
        if lower.contains("not authorized") || lower.contains("polkit") {
            return Err(Error::backend(format!("{what}: {tail}"))
                .with_hint("nmcli general permissions   # network-control must be yes"));
        }
        let status = out
            .code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "on a signal".to_string());
        return Err(Error::upstream(format!("{what} exited {status}: {tail}"))
            .with_hint("fproton doctor --only network"));
    }
    Ok(out)
}

fn nmcli(ctx: &Context, args: &[&str], cancel: Option<&CancelToken>) -> Result<Output, Error> {
    let mut argv = vec![PROGRAM];
    argv.extend_from_slice(args);
    let deadline = ctx.timeouts.nmcli();
    let out = run_with_deadline(&argv, deadline, cancel)?;
    check(out, &format!("nmcli {}", args.join(" ")), deadline)
}

// ---------------------------------------------------------------- frozen API

/// Every profile NetworkManager knows, from one `nmcli` call.
pub fn connections(ctx: &Context) -> Result<Vec<NmConnection>, Error> {
    let out = nmcli(
        ctx,
        &[
            "-t",
            "-f",
            "NAME,UUID,TYPE,DEVICE,ACTIVE",
            "connection",
            "show",
        ],
        None,
    )?;
    parse_connections(&out.stdout_string())
}

/// Parse the `-t` (colon-separated) connection listing.
///
/// nmcli escapes a literal `:` as `\:` and a literal `\` as `\\` in terse mode, so
/// the split honours the backslash instead of using `str::split(':')`.
pub fn parse_connections(text: &str) -> Result<Vec<NmConnection>, Error> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_terse(line);
        if fields.len() != 5 {
            return Err(Error::upstream(format!(
                "nmcli printed {} fields where 5 were expected: {line}",
                fields.len()
            ))
            .with_hint("fproton doctor --only network"));
        }
        rows.push(NmConnection {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind: fields[2].clone(),
            device: Some(fields[3].clone()).filter(|d| !d.is_empty()),
            active: fields[4] == "yes",
        });
    }
    Ok(rows)
}

/// Split one terse nmcli line on unescaped colons, unescaping as it goes.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ':' => fields.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    fields.push(current);
    fields
}

/// Link state derived from one `connections()` call — `status` never runs a second.
pub fn link_state(ctx: &Context) -> Result<LinkState, Error> {
    Ok(link_state_from_connections(&connections(ctx)?))
}

/// The pure half of [`link_state`], so `status` can reuse its single nmcli call.
pub fn link_state_from_connections(conns: &[NmConnection]) -> LinkState {
    let mut link = LinkState::default();
    for c in conns {
        if !c.active {
            continue;
        }
        let device = c.device.as_deref().unwrap_or_default();
        if device == TUNNEL_DEVICE {
            link.up = true;
            link.device = Some(TUNNEL_DEVICE.to_string());
            link.nm_connection = Some(c.name.clone());
            link.uuid = Some(c.uuid.clone());
        } else if c.name == KILLSWITCH_PROFILE {
            link.killswitch_active = true;
        } else if TUNNEL_KINDS.contains(&c.kind.as_str()) && !device.is_empty() {
            link.conflicting_tunnels.push(device.to_string());
        }
    }
    if !link.up {
        link.device = std::path::Path::new("/sys/class/net")
            .join(TUNNEL_DEVICE)
            .exists()
            .then(|| TUNNEL_DEVICE.to_string());
    }
    link
}

/// `/sys/class/net/<device>/statistics/{rx,tx}_bytes`; `Ok(None)` when the device is gone.
///
/// `rx_bps`/`tx_bps` stay `null` here: only the caller holding the previous sample
/// (`status`, from state) can derive a rate.
pub fn traffic(ctx: &Context, device: &str) -> Result<Option<Traffic>, Error> {
    let dir = ctx.paths.sysfs_device(device).join("statistics");
    if !dir.is_dir() {
        return Ok(None);
    }
    let read = |name: &str| -> Option<u64> {
        std::fs::read_to_string(dir.join(name))
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    };
    match (read("rx_bytes"), read("tx_bytes")) {
        (Some(rx_bytes), Some(tx_bytes)) => Ok(Some(Traffic {
            rx_bytes,
            tx_bytes,
            rx_bps: None,
            tx_bps: None,
            sampled_at: crate::now_rfc3339(),
        })),
        _ => Ok(None),
    }
}

/// `nmcli connection down <name>` (used by `disconnect --force`).
///
/// "not an active connection" is success: the command is idempotent by contract (§4).
pub fn connection_down(ctx: &Context, name: &str, cancel: &CancelToken) -> Result<(), Error> {
    let deadline = ctx.timeouts.nmcli();
    let out = run_with_deadline(
        &[PROGRAM, "connection", "down", name],
        deadline,
        Some(cancel),
    )?;
    if !out.timed_out && !out.success() && is_already_down(&out.stderr_string()) {
        return Ok(());
    }
    check(out, &format!("nmcli connection down {name}"), deadline).map(|_| ())
}

/// `nmcli device disconnect <device>` (used by `disconnect --force`).
///
/// A device that does not exist, or is already disconnected, is success.
pub fn device_disconnect(ctx: &Context, device: &str, cancel: &CancelToken) -> Result<(), Error> {
    let deadline = ctx.timeouts.nmcli();
    let out = run_with_deadline(
        &[PROGRAM, "device", "disconnect", device],
        deadline,
        Some(cancel),
    )?;
    if !out.timed_out && !out.success() && is_already_down(&out.stderr_string()) {
        return Ok(());
    }
    check(out, &format!("nmcli device disconnect {device}"), deadline).map(|_| ())
}

/// nmcli's own wording for "there was nothing to tear down".
fn is_already_down(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("not an active connection")
        || s.contains("not active")
        || s.contains("no such device")
        || s.contains("unknown connection")
        || s.contains("not found")
        || s.contains("disconnect is not available")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID_FIXTURE: &str =
        include_str!("../../../tests/fixtures/nmcli-connections-uuid-disconnected.txt");

    #[test]
    fn the_captured_listing_parses_into_the_real_profiles() {
        let rows = parse_connections(UUID_FIXTURE).expect("fixture parses");
        assert_eq!(rows.len(), 8, "the capture holds 8 profiles");

        let proton = rows
            .iter()
            .find(|r| r.name == "ProtonVPN CH#461")
            .expect("the Proton profile is in the capture");
        assert_eq!(proton.uuid, "58d6042d-3c68-4ee2-81d6-77bcc608a95c");
        assert_eq!(proton.kind, "wireguard");
        assert_eq!(proton.device, None);
        assert!(!proton.active);
        assert!(proton.is_proton_profile());
        assert_eq!(proton.proton_server_name(), Some("CH#461"));

        let tailscale = rows.iter().find(|r| r.name == "tailscale0").unwrap();
        assert!(tailscale.active);
        assert_eq!(tailscale.device.as_deref(), Some("tailscale0"));
        assert_eq!(tailscale.kind, "tun");

        let ks = rows.iter().find(|r| r.name == KILLSWITCH_PROFILE).unwrap();
        assert_eq!(ks.kind, "dummy");
        assert!(!ks.active);
    }

    #[test]
    fn the_captured_state_is_link_down_with_one_conflicting_tunnel() {
        let rows = parse_connections(UUID_FIXTURE).unwrap();
        let link = link_state_from_connections(&rows);
        assert!(!link.up, "both Proton profiles were inactive at capture");
        assert!(!link.killswitch_active);
        assert_eq!(link.nm_connection, None);
        assert_eq!(link.uuid, None);
        assert_eq!(link.conflicting_tunnels, vec!["tailscale0".to_string()]);
    }

    #[test]
    fn an_active_proton_profile_becomes_an_up_link_with_its_uuid() {
        let text = "ProtonVPN CH#461:58d6042d-3c68-4ee2-81d6-77bcc608a95c:wireguard:proton0:yes\n\
                    pvpn-killswitch-ipv6:e687d04b-b08b-487f-ba55-c8f2183020b5:dummy:ipv6leakintrf0:yes\n";
        let link = link_state_from_connections(&parse_connections(text).unwrap());
        assert!(link.up);
        assert_eq!(link.device.as_deref(), Some("proton0"));
        assert_eq!(link.nm_connection.as_deref(), Some("ProtonVPN CH#461"));
        assert_eq!(
            link.uuid.as_deref(),
            Some("58d6042d-3c68-4ee2-81d6-77bcc608a95c")
        );
        assert!(link.killswitch_active);
        assert!(link.conflicting_tunnels.is_empty());
    }

    #[test]
    fn escaped_colons_in_a_profile_name_survive_the_split() {
        let text = r"Home\: guest:11111111-2222-3333-4444-555555555555:802-11-wireless::no";
        let rows = parse_connections(text).unwrap();
        assert_eq!(rows[0].name, "Home: guest");
        assert_eq!(rows[0].uuid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(rows[0].device, None);
    }

    #[test]
    fn a_short_row_is_an_error_not_a_silent_skip() {
        let e = parse_connections("only:three:fields").unwrap_err();
        assert_eq!(e.exit_code(), 5);
    }

    #[test]
    fn nmcli_wording_for_an_already_down_profile_is_treated_as_success() {
        assert!(is_already_down(
            "Error: 'pvpn-killswitch-ipv6' is not an active connection."
        ));
        assert!(is_already_down("Error: Device 'proton0' not found."));
        assert!(!is_already_down(
            "Error: Not authorized to control networking."
        ));
    }
}
