//! Who owns the default route, from `ip -j route` (about 6 ms for both calls).
//!
//! An active tunnel device is not a conflict by itself: Tailscale without an exit
//! node routes only the tailnet (`100.x/32` rows in its own table 52) and coexists
//! with a Proton full tunnel. The two real conflicts are (a) another tunnel that
//! owns a default route in the main table — a second full VPN — and (b) a
//! Tailscale exit node, which installs `default dev tailscale0` in table 52 and
//! claims every packet through the `lookup 52` rule. Both are read here, read-only.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc;

pub const PROGRAM: &str = "ip";
/// The policy-routing table Tailscale populates.
pub const TAILSCALE_TABLE: &str = "52";

/// Default-route ownership as `ip -j route` reports it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteFacts {
    /// Devices carrying a `default` route in the main table.
    pub main_default_devs: Vec<String>,
    /// Table 52 holds a `default` route — a Tailscale exit node is in use.
    pub tailscale_exit_node: bool,
}

/// Why a tunnel conflicts with a Proton tunnel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictReason {
    /// The device owns a default route in the main table (a second full VPN).
    DefaultRoute,
    /// Tailscale is using an exit node (default route in table 52).
    ExitNode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub device: String,
    pub reason: ConflictReason,
}

/// `ip -j route show default` and `ip -j route show table 52`, under the nmcli
/// deadline. Any failure is `Ok(default)`: this only ever downgrades a warning,
/// and `status` must never fail because `ip` did.
pub fn probe(ctx: &Context) -> Result<RouteFacts, Error> {
    let deadline = Duration::from_secs(ctx.timeouts.nmcli_sec.max(1));
    let main =
        proc::run_with_deadline(&[PROGRAM, "-j", "route", "show", "default"], deadline, None)
            .ok()
            .filter(|o| o.code == Some(0) && !o.timed_out)
            .map(|o| o.stdout_string())
            .and_then(|t| default_devs(&t).ok())
            .unwrap_or_default();
    let ts = proc::run_with_deadline(
        &[PROGRAM, "-j", "route", "show", "table", TAILSCALE_TABLE],
        deadline,
        None,
    )
    .ok()
    .filter(|o| o.code == Some(0) && !o.timed_out)
    .map(|o| o.stdout_string())
    .and_then(|t| has_default(&t).ok())
    .unwrap_or(false);
    Ok(RouteFacts {
        main_default_devs: main,
        tailscale_exit_node: ts,
    })
}

#[derive(Deserialize)]
struct RouteRow {
    #[serde(default)]
    dst: String,
    #[serde(default)]
    dev: String,
}

fn rows(json: &str) -> Result<Vec<RouteRow>, Error> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(json).map_err(|e| Error::failure(format!("ip -j route: {e}")))
}

/// Devices that own a `default` route in a `ip -j route show default` listing.
pub fn default_devs(json: &str) -> Result<Vec<String>, Error> {
    let mut devs: Vec<String> = rows(json)?
        .into_iter()
        .filter(|r| r.dst == "default" && !r.dev.is_empty())
        .map(|r| r.dev)
        .collect();
    devs.sort();
    devs.dedup();
    Ok(devs)
}

/// Whether a `ip -j route show table N` listing contains a `default` route.
pub fn has_default(json: &str) -> Result<bool, Error> {
    Ok(rows(json)?.iter().any(|r| r.dst == "default"))
}

/// The pure rule: which of the active tunnel devices (never `proton0`) actually
/// contest a Proton tunnel.
pub fn conflicts(active_tunnels: &[String], facts: &RouteFacts) -> Vec<Conflict> {
    let mut out = Vec::new();
    for dev in active_tunnels {
        if dev == crate::TUNNEL_DEVICE {
            continue;
        }
        if facts.main_default_devs.iter().any(|d| d == dev) {
            out.push(Conflict {
                device: dev.clone(),
                reason: ConflictReason::DefaultRoute,
            });
        } else if dev.starts_with("tailscale") && facts.tailscale_exit_node {
            out.push(Conflict {
                device: dev.clone(),
                reason: ConflictReason::ExitNode,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured on the owner's machine 2026-09-15: no exit node.
    const MAIN: &str = r#"[{"dst":"default","gateway":"192.168.1.1","dev":"wlp0s20f3","protocol":"dhcp","prefsrc":"192.168.1.204","metric":600,"flags":[]}]"#;
    const TABLE52_NO_EXIT: &str = r#"[{"dst":"100.69.56.84","dev":"tailscale0","flags":[]},{"dst":"100.81.187.69","dev":"tailscale0","flags":[]}]"#;
    const TABLE52_EXIT: &str = r#"[{"dst":"default","dev":"tailscale0","flags":[]},{"dst":"100.69.56.84","dev":"tailscale0","flags":[]}]"#;

    #[test]
    fn the_captured_tables_show_no_conflict_without_an_exit_node() {
        let facts = RouteFacts {
            main_default_devs: default_devs(MAIN).unwrap(),
            tailscale_exit_node: has_default(TABLE52_NO_EXIT).unwrap(),
        };
        assert_eq!(facts.main_default_devs, vec!["wlp0s20f3".to_string()]);
        assert!(!facts.tailscale_exit_node);
        assert!(conflicts(&["tailscale0".to_string()], &facts).is_empty());
    }

    #[test]
    fn an_exit_node_is_a_conflict() {
        let facts = RouteFacts {
            main_default_devs: default_devs(MAIN).unwrap(),
            tailscale_exit_node: has_default(TABLE52_EXIT).unwrap(),
        };
        let c = conflicts(&["tailscale0".to_string()], &facts);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].reason, ConflictReason::ExitNode);
    }

    #[test]
    fn another_tunnel_owning_the_default_route_is_a_conflict() {
        let main = r#"[{"dst":"default","dev":"wg-mullvad","flags":[]},{"dst":"default","gateway":"192.168.1.1","dev":"wlp0s20f3","metric":600,"flags":[]}]"#;
        let facts = RouteFacts {
            main_default_devs: default_devs(main).unwrap(),
            tailscale_exit_node: false,
        };
        let c = conflicts(
            &["wg-mullvad".to_string(), "tailscale0".to_string()],
            &facts,
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].device, "wg-mullvad");
        assert_eq!(c[0].reason, ConflictReason::DefaultRoute);
    }

    #[test]
    fn proton0_itself_never_conflicts_and_empty_tables_parse() {
        assert!(!has_default("").unwrap());
        assert!(default_devs("[]").unwrap().is_empty());
        let facts = RouteFacts {
            main_default_devs: vec!["proton0".to_string()],
            tailscale_exit_node: false,
        };
        assert!(conflicts(&["proton0".to_string()], &facts).is_empty());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(default_devs("not json").is_err());
    }
}
