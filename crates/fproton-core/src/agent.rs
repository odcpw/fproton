//! The machine-facing contract: `capabilities` (JSON) and `robot-docs` (text).
//!
//! Same shape as `fbittorrent/src/agent.rs`. Everything here is data — the binary
//! prints it; this crate stays silent.

use serde::Serialize;

use crate::context::Context;
use crate::engine::TARGET_FORMS;
use crate::error::EXIT_CODES;
use crate::index::INDEX_FORMAT_VERSION;
use crate::settings::{PROTON_CONFIG_KEYS, PROTON_CONFIG_SET_KEYS, SETTING_KEYS};
use crate::status::WARNING_IDS;
use crate::{CONTRACT_VERSION, NAME, SCHEMA_VERSION, VERSION};

/// Which phase of DESIGN.md §8 this binary was built at. Later phases update it.
pub const BUILD_STAGE: &str = "v0.1-integrated";
/// Said in `capabilities`, `robot-docs` and nowhere else: what this build really does.
/// The three tiers are DESIGN.md §4b; nothing here may claim a RED row works.
pub const BUILD_NOTE: &str = "Every command is implemented. What differs is how much of each has been verified on the build machine, whose Proton CLI is wedged by a broken Secret Service default collection. VERIFIED END TO END: status, servers, countries, cities, index, settings, history, recents, capabilities, robot-docs, version, tui, doctor, `setup --dry-run`, `disconnect --force` (a no-op while both NetworkManager profiles are inactive), port's no-tunnel error, signin's argument validation. ONLY THE ERROR AND DEADLINE PATHS VERIFIED: connect, disconnect, reconnect, config list/get/set, account, signout — each drives the official Proton CLI, so on this machine they exit 5 when it is killed at its deadline. NEVER VERIFIED, DO NOT ASSUME THEY WORK: a successful connect or reconnect, a successful port-forwarding request, every `wg` subcommand (an experiment behind FPROTON_EXPERIMENTAL_WG=1), and the doctor's dns_when_connected and tunnel_route_ok checks, which skip while proton0 is down.";

/// One row of the `commands` array in `capabilities`.
#[derive(Serialize)]
pub struct CommandDoc {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub summary: &'static str,
    /// Supports `--json` (and is implied by a non-TTY stdout).
    pub json: bool,
    /// Changes something outside fproton's own state.
    pub mutating: bool,
    /// Spawns the official `protonvpn` CLI, which can hang (§1.2).
    pub needs_cli: bool,
}

/// Every command of DESIGN.md §4, in `--help` order.
pub const COMMANDS: &[CommandDoc] = &[
    CommandDoc { name: "status", aliases: &[], summary: "The whole Status document (schema 1). Never spawns protonvpn.", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "connect", aliases: &[], summary: "Connect to a target: fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "disconnect", aliases: &[], summary: "Disconnect; --force tears proton0 down through nmcli and needs --yes", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "reconnect", aliases: &[], summary: "Reconnect the last target; --next picks the next server in that country", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "servers", aliases: &["ls"], summary: "Server rows from the local index, filtered and sorted", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "countries", aliases: &[], summary: "Country rows: server counts, lowest load, free availability", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "cities", aliases: &[], summary: "City rows, optionally for one country", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "index", aliases: &[], summary: "Rebuild or describe the server-index snapshot", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "config", aliases: &[], summary: "Proton's own settings: list | get K | set K V (via the CLI)", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "settings", aliases: &[], summary: "fproton's settings: show | get K | set K V (local file)", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "port", aliases: &[], summary: "One-shot NAT-PMP port-forwarding request through the tunnel", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "history", aliases: &[], summary: "Past connect/disconnect actions with result and duration", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "recents", aliases: &[], summary: "Recently used targets, ready to hand back to `connect`", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "account", aliases: &[], summary: "Proton account name (cached); entitlement is status.max_tier", json: true, mutating: false, needs_cli: true },
    CommandDoc { name: "signin", aliases: &[], summary: "Interactive sign-in in this terminal (no deadline)", json: false, mutating: true, needs_cli: true },
    CommandDoc { name: "signout", aliases: &[], summary: "Sign out", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "wg", aliases: &[], summary: "EXPERIMENT, gated by FPROTON_EXPERIMENTAL_WG=1: up | swap | list", json: true, mutating: true, needs_cli: false },
    CommandDoc { name: "doctor", aliases: &[], summary: "Diagnose the install; --fix applies a small set of reversible repairs", json: true, mutating: true, needs_cli: true },
    CommandDoc { name: "setup", aliases: &[], summary: "Idempotent desktop integration: plugin link, bar placement, menu entries", json: true, mutating: true, needs_cli: false },
    CommandDoc { name: "tui", aliases: &[], summary: "FrankenTUI: Status · Servers · History (needs a terminal)", json: false, mutating: false, needs_cli: false },
    CommandDoc { name: "capabilities", aliases: &[], summary: "This document", json: true, mutating: false, needs_cli: false },
    CommandDoc { name: "robot-docs", aliases: &["docs"], summary: "Agent handbook, plain text", json: false, mutating: false, needs_cli: false },
    CommandDoc { name: "version", aliases: &[], summary: "Version and contract version", json: true, mutating: false, needs_cli: false },
];

/// The machine contract. Printed by `fproton capabilities`.
pub fn capabilities_json(ctx: &Context) -> serde_json::Value {
    let exit_codes: Vec<serde_json::Value> = EXIT_CODES
        .iter()
        .map(|(code, name, meaning)| serde_json::json!({"code": code, "name": name, "meaning": meaning}))
        .collect();
    let proton_config: Vec<serde_json::Value> = PROTON_CONFIG_SET_KEYS
        .iter()
        .map(|(k, values)| serde_json::json!({"key": k, "values": values}))
        .collect();
    serde_json::json!({
        "ok": true,
        "schema": SCHEMA_VERSION,
        "tool": NAME,
        "version": VERSION,
        "contract_version": CONTRACT_VERSION,
        "build_stage": BUILD_STAGE,
        "build_note": BUILD_NOTE,
        "description": "Proton VPN for Omarchy: CLI, doctor, FrankenTUI and the backend of the oliver.protonvpn bar widget.",
        "conventions": {
            "json": "--json is global; a non-TTY stdout implies it. One JSON object per invocation.",
            "errors": "{ok:false,schema,code,code_name,error,hint} on stdout in JSON mode, with the same exit code",
            "stdout": "data only; progress and diagnostics go to stderr",
            "colour": "off when stdout is not a TTY, with --no-color, or when NO_COLOR or CI is set",
            "bare_invocation": "on a TTY opens the TUI; without one prints `status --json`",
            "destructive": "`disconnect --force`, `wg swap`, `setup --disable-old-plugin` and `doctor --fix` need --yes; all offer --dry-run",
            "deadlines": "every external command runs in its own process group with a deadline; --timeout <s> clamps all of them",
            "never": "fproton never opens a *.keyring file, NetworkManager secrets or WireGuard keys, and never contacts Proton's API; the one file it may rewrite under keyrings/ is the `default` pointer, whose content is a keyring basename",
        },
        "connect_targets": TARGET_FORMS,
        "status_schema": SCHEMA_VERSION,
        "status_warning_ids": WARNING_IDS,
        "index_format_version": INDEX_FORMAT_VERSION,
        "proton_config_keys": proton_config,
        "proton_config_readable_keys": PROTON_CONFIG_KEYS,
        "settings_keys": SETTING_KEYS,
        "commands": COMMANDS,
        "exit_codes": exit_codes,
        "env": {
            "FPROTON_STATE_DIR": "override the state directory (tests)",
            "FPROTON_CONFIG_DIR": "override the config directory (tests)",
            "FPROTON_CACHE_DIR": "override the cache directory (tests)",
            "FPROTON_EXPERIMENTAL_WG": "set to 1 to unlock the unverified `wg` experiment",
            "NO_COLOR": "disables colour",
            "CI": "disables colour",
        },
        "files": {
            "config": ctx.paths.config_file().display().to_string(),
            "state": ctx.paths.state_file().display().to_string(),
            "index": ctx.paths.index_file().display().to_string(),
            "serverlist": ctx.paths.serverlist().display().to_string(),
        },
        "agent_workflow": [
            "fproton status --json            # state, server, warnings, next commands",
            "fproton servers --country CH --json --limit 10",
            "fproton connect CH#461 --json    # or fastest | p2p | secure-core | tor | random",
            "fproton doctor --json            # exit 11 when something is wrong",
        ],
    })
}

/// The agent handbook. `fproton robot-docs` prints this verbatim.
pub const ROBOT_GUIDE: &str = r#"fproton — agent handbook
========================

Proton VPN for Omarchy. One binary: CLI, doctor, TUI, and the backend of the
`oliver.protonvpn` bar widget.

BUILD STAGE
  Every command is implemented; they differ in how much has been verified.
  Verified end to end on this machine: status, servers, countries, cities, index,
  settings, history, recents, capabilities, robot-docs, version, tui, doctor,
  setup --dry-run, disconnect --force (a no-op while nothing is up) and port's
  no-tunnel error.
  Verified only on their timeout/error path, because the official Proton CLI hangs on
  this machine: connect, reconnect, disconnect, config list|get|set, account, signout.
  Never verified, and claimed nowhere: a successful connect or reconnect, a granted
  forwarded port, every `wg` subcommand, and the doctor's dns_when_connected and
  tunnel_route_ok checks, which skip while proton0 is down.

FIRST CALL
  fproton status --json      Everything about the current state, in one object.
  fproton capabilities       The machine contract: commands, codes, keys, files.

OUTPUT RULES
  * --json is a global flag, and a non-TTY stdout implies it. One object per call.
  * stdout is data. Progress and diagnostics go to stderr.
  * Errors in JSON mode are {"ok":false,"schema":1,"code":N,"code_name":"…",
    "error":"…","hint":"<runnable command>"} on stdout, with N as the exit code.
  * Every human-mode error ends with a `hint:` line naming a command you can run.
  * Colour is off when stdout is not a TTY, with --no-color, or when NO_COLOR or CI is set.

EXIT CODES
  0  ok              8  timeout          (a --wait ran out; nothing is broken)
  1  failure         9  not_signed_in    (run `fproton signin`)
  2  usage          10  index            (run `fproton index --rebuild`)
  3  safety         11  doctor           (the doctor found failures)
  4  backend        12  doctor_partial
  5  upstream       13  doctor_refused
  6  not_found     130  interrupted
  7  ambiguous

  5 is where the official Proton CLI lands when it errors *or when fproton kills it at
  its deadline*. That CLI hangs on this machine (a broken Secret Service default
  collection), so treat exit 5 from connect/config/account as "the CLI did not answer",
  not as "the VPN failed". `fproton doctor` explains it.

CONNECT TARGETS  (exactly one positional; no --country/--city/--feature flags)
  fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random
  e.g. fproton connect CH · fproton connect CH#461 · fproton connect "city:New York"
  Targets are validated before they reach an argv. Anything else is exit 2.

READING THE STATE
  `status.state` is one of connected, connecting, disconnecting, disconnected, error —
  a total function of the link, a live `pending` action and `last_error`. Every key of
  the document is always present; unknown is null, never an absent key and never 0.
  `status.warnings[]` carries {id, severity, text, hint} with stable ids.
  `status.pending` gives {action, target, started_at, deadline_sec, elapsed_sec, pid}:
  derive progress and "taking long" from those fields alone.

SAFETY
  Irreversible actions (`disconnect --force`, `wg swap`, `setup --disable-old-plugin`,
  `doctor --fix`) refuse without --yes and print the exact argv they would run; all of
  them accept --dry-run. The doctor never deletes anything.

WHAT FPROTON NEVER DOES
  It never opens a *.keyring file, NetworkManager secrets or WireGuard keys; never runs
  a shell (argv only, everywhere); never contacts Proton's API; and never leaves a child
  process running past its deadline. The one named exception: the doctor's
  `secret_service_default_alias` fixer rewrites `~/.local/share/keyrings/default`, a
  16-byte pointer file whose content is a keyring basename, never key material.

TYPICAL SESSION
  fproton status --json
  fproton servers --country CH --sort load --limit 10 --json
  fproton connect CH#461 --json
  fproton port --json                 # forwarded port, only while a tunnel is up
  fproton disconnect --json
  fproton doctor --json               # exit 11 when something needs attention
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_lists_every_command_and_code() {
        let ctx = Context::load().expect("context");
        let v = capabilities_json(&ctx);
        assert_eq!(v["ok"], true);
        assert_eq!(v["schema"], 1);
        assert_eq!(v["commands"].as_array().unwrap().len(), COMMANDS.len());
        assert_eq!(v["exit_codes"].as_array().unwrap().len(), EXIT_CODES.len());
        assert!(v["files"]["state"]
            .as_str()
            .unwrap()
            .ends_with("state.json"));
    }

    #[test]
    fn the_handbook_documents_every_exit_code_name() {
        for (_, name, _) in EXIT_CODES {
            assert!(
                ROBOT_GUIDE.contains(name),
                "robot-docs must document exit code '{name}'"
            );
        }
        for form in TARGET_FORMS {
            assert!(
                ROBOT_GUIDE.contains(form),
                "robot-docs must list target '{form}'"
            );
        }
    }
}
