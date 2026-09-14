//! EXPERIMENT (DESIGN.md §4 `wg`): CLI-free reconnect through NetworkManager.
//!
//! Env-gated behind `FPROTON_EXPERIMENTAL_WG=1`, built last, never load-bearing:
//! nothing in §5, §6, §7 or any gate depends on it. `up` activates an existing
//! `ProtonVPN *` profile; `swap` clones one and retargets the clone's peer at another
//! server. **No private key is ever read**: NetworkManager copies it itself, and the
//! peer's X25519 key is the server's public key, which Proton publishes.
//!
//! Kill criterion and expiry are in DESIGN.md §4; delete this file when it fires.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::engine::Target;
use crate::error::Error;
use crate::nm;
use crate::proc::{run_with_deadline, CancelToken};

/// The gate. Every `wg` subcommand refuses with exit 2 unless this is set.
pub const ENV_GATE: &str = "FPROTON_EXPERIMENTAL_WG";
/// Printed by every `wg` invocation, in both human and JSON output.
pub const EXPERIMENTAL_NOTE: &str = "experimental: unverified on this machine";
/// The UDP port Proton's profiles use for the peer endpoint (§1.3).
const PEER_PORT: u16 = 443;

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

/// `nmcli` under its deadline. Errors name the subcommand, never the peer string.
fn nmcli(ctx: &Context, args: &[&str], what: &str, cancel: &CancelToken) -> Result<String, Error> {
    let mut argv = vec![nm::PROGRAM];
    argv.extend_from_slice(args);
    let out = run_with_deadline(&argv, ctx.timeouts.nmcli(), Some(cancel))?;
    if out.timed_out {
        return Err(
            Error::upstream(format!("{what} was killed at its deadline"))
                .with_hint("fproton wg list"),
        );
    }
    if !out.success() {
        return Err(
            Error::upstream(format!("{what} failed: {}", out.stderr_tail()))
                .with_hint("fproton wg list"),
        );
    }
    Ok(out.stdout_string())
}

/// The `endpoint=<ip>:<port>` of a profile's single peer, if nmcli reports one.
fn endpoint_of(ctx: &Context, uuid: &str, cancel: &CancelToken) -> Option<String> {
    let text = nmcli(
        ctx,
        &["-t", "-f", "wireguard.peers", "connection", "show", uuid],
        "nmcli connection show",
        cancel,
    )
    .ok()?;
    text.split_whitespace()
        .find_map(|token| token.strip_prefix("endpoint=").map(str::to_string))
}

/// Every `ProtonVPN *` profile, with its peer endpoint.
fn profiles(ctx: &Context, cancel: &CancelToken) -> Result<Vec<WgProfile>, Error> {
    Ok(nm::connections(ctx)?
        .into_iter()
        .filter(nm::NmConnection::is_proton_profile)
        .map(|c| WgProfile {
            server: c.proton_server_name().map(str::to_string),
            endpoint: endpoint_of(ctx, &c.uuid, cancel),
            clone_of: c.name.strip_suffix(" (fproton)").map(str::to_string),
            name: c.name,
            uuid: c.uuid,
            active: c.active,
        })
        .collect())
}

/// One profile by name, or the only one there is.
fn pick(profiles: Vec<WgProfile>, name: Option<&str>) -> Result<WgProfile, Error> {
    match name {
        Some(want) => profiles
            .into_iter()
            .find(|p| p.name == want || p.server.as_deref() == Some(want))
            .ok_or_else(|| {
                Error::not_found(format!("no ProtonVPN profile called '{want}'"))
                    .with_hint("FPROTON_EXPERIMENTAL_WG=1 fproton wg list")
            }),
        None => match profiles.len() {
            0 => Err(Error::not_found(
                "NetworkManager has no ProtonVPN profile; only the official CLI creates one",
            )
            .with_hint("fproton connect fastest")),
            1 => Ok(profiles.into_iter().next().expect("exactly one")),
            _ => Err(Error::ambiguous(format!(
                "{} ProtonVPN profiles exist; name the one to use",
                profiles.len()
            ))
            .with_hint("FPROTON_EXPERIMENTAL_WG=1 fproton wg list")),
        },
    }
}

/// One server's entry IP and **public** key, from Proton's own list. The index stores
/// no keys, so this is the one place that re-reads the 24 MB file.
fn peer_of(ctx: &Context, server: &str) -> Result<(String, String), Error> {
    #[derive(Deserialize)]
    struct List {
        #[serde(rename = "LogicalServers")]
        logical_servers: Vec<Logical>,
    }
    #[derive(Deserialize)]
    struct Logical {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Servers")]
        servers: Vec<Physical>,
    }
    #[derive(Deserialize)]
    struct Physical {
        #[serde(rename = "EntryIP")]
        entry_ip: Option<String>,
        #[serde(rename = "X25519PublicKey")]
        public_key: Option<String>,
    }
    let path = ctx.paths.serverlist();
    let bytes = std::fs::read(&path)
        .map_err(|e| Error::index(format!("cannot read {}: {e}", path.display())))?;
    let list: List = serde_json::from_slice(&bytes).map_err(|e| {
        Error::index(format!(
            "{} is not Proton's server list: {e}",
            path.display()
        ))
    })?;
    list.logical_servers
        .into_iter()
        .find(|l| l.name.eq_ignore_ascii_case(server))
        .and_then(|l| {
            let p = l.servers.into_iter().next()?;
            Some((p.entry_ip?, p.public_key?))
        })
        .ok_or_else(|| {
            Error::not_found(format!(
                "{server} has no usable entry address in Proton's list"
            ))
            .with_hint("fproton index --rebuild")
        })
}

/// The server a `swap` target names: the exact one, or the best one under it.
fn server_for(ctx: &Context, target: &Target) -> Result<String, Error> {
    if let Target::Server(name) = target {
        return Ok(name.clone());
    }
    let q = crate::index::Query {
        country: match target {
            Target::Country(cc) => Some(cc.clone()),
            _ => None,
        },
        city: match target {
            Target::City(c) => Some(c.clone()),
            _ => None,
        },
        sort: crate::index::Sort::Score,
        limit: Some(1),
        ..Default::default()
    };
    crate::index::servers(ctx, &q)?
        .into_iter()
        .next()
        .map(|r| r.name)
        .ok_or_else(|| {
            Error::not_found(format!("nothing in the index matches {target}"))
                .with_hint("fproton servers --limit 5")
        })
}

// ---------------------------------------------------------------- frozen API

/// Activate an existing `ProtonVPN *` profile (the CLI's own, which holds the key).
pub fn up(ctx: &Context, name: Option<&str>, cancel: &CancelToken) -> Result<WgProfile, Error> {
    let profile = pick(profiles(ctx, cancel)?, name)?;
    nmcli(
        ctx,
        &["connection", "up", "uuid", &profile.uuid],
        "nmcli connection up",
        cancel,
    )?;
    Ok(WgProfile {
        active: true,
        ..profile
    })
}

/// Clone a profile and retarget the clone's peer at another server from the index.
pub fn swap(
    ctx: &Context,
    name: &str,
    target: &Target,
    cancel: &CancelToken,
) -> Result<WgProfile, Error> {
    let source = pick(profiles(ctx, cancel)?, Some(name))?;
    let server = server_for(ctx, target)?;
    let (entry_ip, public_key) = peer_of(ctx, &server)?;
    let clone_name = format!("ProtonVPN {server} (fproton)");

    nmcli(
        ctx,
        &["connection", "clone", "uuid", &source.uuid, &clone_name],
        "nmcli connection clone",
        cancel,
    )?;
    // NetworkManager copies the private key itself; fproton never sees it.
    let peer = format!("{public_key} endpoint={entry_ip}:{PEER_PORT} allowed-ips=0.0.0.0/0;::/0");
    nmcli(
        ctx,
        &[
            "connection",
            "modify",
            &clone_name,
            "wireguard.peers",
            &peer,
        ],
        "nmcli connection modify (peer)",
        cancel,
    )?;
    let uuid = profiles(ctx, cancel)?
        .into_iter()
        .find(|p| p.name == clone_name)
        .map(|p| p.uuid)
        .unwrap_or_default();
    Ok(WgProfile {
        name: clone_name,
        uuid,
        server: Some(server),
        endpoint: Some(format!("{entry_ip}:{PEER_PORT}")),
        active: false,
        clone_of: Some(source.name),
    })
}

/// Every `ProtonVPN *` profile NetworkManager knows.
pub fn list(ctx: &Context) -> Result<Vec<WgProfile>, Error> {
    profiles(ctx, &CancelToken::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, uuid: &str) -> WgProfile {
        WgProfile {
            name: format!("ProtonVPN {name}"),
            uuid: uuid.into(),
            server: Some(name.into()),
            endpoint: Some("146.70.226.194:443".into()),
            active: false,
            clone_of: None,
        }
    }

    #[test]
    fn the_gate_is_closed_and_says_how_to_open_it() {
        let e = gate_error();
        assert_eq!(e.exit_code(), 2);
        assert!(e.hint().contains(ENV_GATE));
    }

    #[test]
    fn picking_says_which_way_it_failed_and_never_prints_a_key() {
        assert_eq!(pick(Vec::new(), None).unwrap_err().exit_code(), 6);
        let two = vec![profile("CH#461", "a"), profile("DE#12", "b")];
        assert_eq!(pick(two.clone(), None).unwrap_err().exit_code(), 7);
        assert_eq!(pick(two.clone(), Some("CH#461")).unwrap().uuid, "a");
        assert_eq!(
            pick(two.clone(), Some("ProtonVPN DE#12")).unwrap().uuid,
            "b"
        );
        let json = serde_json::to_string(&two[0]).unwrap();
        assert!(!json.contains("key") && !json.contains("Key"));
    }
}
