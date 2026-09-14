//! The official `protonvpn` driver (DESIGN.md §1.2, §4).
//!
//! Everything here spawns proton-vpn-cli **under a process-group deadline**: the CLI
//! hangs on every session-touching command on this machine. A deadline kill is exit 5
//! and arms the 60 s `cli.hang` backoff. `status` never calls into this module.
//!
//! Only two text parsers exist, both fixture-driven: `info` and `config list` (§1.3).
//! Every argv below is asserted against the captured `protonvpn … --help` output by a
//! unit test, so a flag can never drift from what the installed CLI accepts.

use std::time::Duration;

use crate::context::Context;
use crate::engine::Target;
use crate::error::Error;
use crate::proc::{run_with_deadline, CancelToken, Output};
use crate::state;
use crate::status::{Account, ProtonConfig};

/// The upstream binary name.
pub const PROGRAM: &str = "protonvpn";

/// The argv `protonvpn connect` takes for one target (§4).
///
/// `fastest` passes no argument at all; a server name is the **positional**
/// `SERVER_NAME`; everything else is a flag. `--securecore` is one word.
pub fn connect_args(target: &Target) -> Vec<String> {
    let mut args = vec!["connect".to_string()];
    match target {
        Target::Fastest => {}
        Target::Country(cc) => args.extend(["--country".to_string(), cc.clone()]),
        Target::City(city) => args.extend(["--city".to_string(), city.clone()]),
        Target::Server(name) => args.push(name.clone()),
        Target::P2p => args.push("--p2p".to_string()),
        Target::SecureCore => args.push("--securecore".to_string()),
        Target::Tor => args.push("--tor".to_string()),
        Target::Random => args.push("--random".to_string()),
    }
    args
}

/// Refuse early when the binary is not installed at all (exit 4, §4a).
fn require_program() -> Result<(), Error> {
    if crate::proc::which(PROGRAM).is_some() {
        return Ok(());
    }
    Err(
        Error::backend(format!("{PROGRAM} is not installed or not on PATH"))
            .with_hint("omarchy pkg add proton-vpn-cli"),
    )
}

/// Run one `protonvpn` subcommand under `deadline`, honouring the hang breaker.
///
/// * the breaker is armed → exit 5 immediately, without spawning anything (§2);
/// * a deadline kill → exit 5, and the breaker is armed for 60 s;
/// * a non-zero exit that says "not signed in" → exit 9;
/// * anything else non-zero → exit 5 with the CLI's own last stderr line.
pub fn run(
    ctx: &Context,
    args: &[&str],
    deadline: Duration,
    cancel: &CancelToken,
) -> Result<Output, Error> {
    require_program()?;
    let stored = state::load(ctx)?;
    if let Some(left) = state::cli_backoff_remaining(&stored) {
        return Err(remember(ctx, state::cli_backoff_error(left)));
    }

    let mut argv = vec![PROGRAM];
    argv.extend_from_slice(args);
    let out = run_with_deadline(&argv, deadline, Some(cancel))?;

    if out.timed_out {
        let _ = state::note_cli_hang(ctx);
        return Err(remember(
            ctx,
            Error::upstream(format!(
                "{PROGRAM} {} was killed at its {}s deadline",
                args.join(" "),
                deadline.as_secs()
            ))
            .with_hint("fproton doctor --only cli"),
        ));
    }
    if !out.success() {
        let text = format!("{}\n{}", out.stdout_string(), out.stderr_string());
        if looks_signed_out(&text) {
            return Err(Error::not_signed_in(format!(
                "{PROGRAM} {}: no Proton session",
                args.join(" ")
            ))
            .with_hint("fproton signin"));
        }
        let detail = out.stderr_tail();
        let detail = if detail.is_empty() {
            out.stdout_string().lines().last().unwrap_or("").to_string()
        } else {
            detail
        };
        return Err(remember(
            ctx,
            Error::upstream(format!(
                "{PROGRAM} {} exited {}: {detail}",
                args.join(" "),
                out.code
                    .map(|c| c.to_string())
                    .unwrap_or("on a signal".into())
            ))
            .with_hint("fproton doctor --only cli"),
        ));
    }
    // The CLI answered: the breaker is cleared and `status.cli.last_ok_at` moves.
    let _ = state::note_cli_ok(ctx);
    Ok(out)
}

/// Record a CLI failure as `status.last_error` (§5) and hand the error straight back.
/// The next action clears it; a cache write never changes the error the caller sees.
fn remember(ctx: &Context, e: Error) -> Error {
    let _ = state::set_last_error(ctx, &e, None);
    e
}

/// Upstream's wording when there is no session (exit 9 rather than 5).
fn looks_signed_out(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("not signed in")
        || t.contains("no active session")
        || t.contains("please sign in")
        || t.contains("you are not logged in")
}

// ---------------------------------------------------------------- frozen API

/// `protonvpn config list` under the probe deadline, parsed and cached in state.
pub fn config_list(ctx: &Context, cancel: &CancelToken) -> Result<ProtonConfig, Error> {
    let out = run(ctx, &["config", "list"], ctx.timeouts.probe(), cancel)?;
    let config = parse_config_list(&out.stdout_string())?;
    state::set_config_cache(ctx, &config)?;
    // Read it back so `cached_at` is the value every consumer will see.
    Ok(state::config_cache(ctx)?.unwrap_or(config))
}

/// `protonvpn config set <key> <value>` under the `config_set` deadline.
/// The pair must already have passed `settings::validate_proton_config`.
pub fn config_set(
    ctx: &Context,
    key: &str,
    value: &str,
    cancel: &CancelToken,
) -> Result<(), Error> {
    crate::settings::validate_proton_config(key, value)?;
    run(
        ctx,
        &["config", "set", key, value],
        ctx.timeouts.config_set(),
        cancel,
    )?;
    // The cache is now a lie about this key; correct just that key rather than
    // dropping everything the last `config list` learned.
    if let Ok(Some(mut cached)) = state::config_cache(ctx) {
        cached.set(key, value);
        state::set_config_cache(ctx, &cached)?;
    }
    Ok(())
}

/// `protonvpn info` under the probe deadline; parses the single `Account: '<name>'` line.
pub fn account(ctx: &Context, cancel: &CancelToken) -> Result<Account, Error> {
    let out = run(ctx, &["info"], ctx.timeouts.probe(), cancel)?;
    let name = parse_info(&out.stdout_string())?;
    state::set_account(ctx, &name)
}

/// `protonvpn signin <user>` in the current terminal, interactive, **no deadline** (§2).
///
/// This is the one place fproton lets a child keep the terminal: the CLI prompts for a
/// password and a TOTP code, and fproton never sees either. Nothing is captured.
pub fn signin(ctx: &Context, user: &str) -> Result<(), Error> {
    require_program()?;
    if !is_valid_username(user) {
        return Err(
            Error::usage("a Proton username is 1-64 characters of A-Z a-z 0-9 . _ + @ -")
                .with_hint("fproton signin <username>"),
        );
    }
    let status = std::process::Command::new(PROGRAM)
        .arg("signin")
        .arg(user)
        .status()
        .map_err(|e| {
            Error::backend(format!("cannot run {PROGRAM} signin: {e}"))
                .with_hint("omarchy pkg add proton-vpn-cli")
        })?;
    if !status.success() {
        return Err(Error::upstream(format!(
            "{PROGRAM} signin exited {}",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or("on a signal".into())
        ))
        .with_hint("fproton doctor --only keyring"));
    }
    // A fresh session invalidates the cached answer `status.signed_in` holds.
    let _ = state::set_signed_in(ctx, None);
    Ok(())
}

/// `protonvpn signout` under the probe deadline.
pub fn signout(ctx: &Context, cancel: &CancelToken) -> Result<(), Error> {
    run(ctx, &["signout"], ctx.timeouts.probe(), cancel)?;
    let _ = state::set_signed_in(ctx, Some(false));
    Ok(())
}

/// Parse `protonvpn info` output (one `Account: '<name>'` line; upstream prints no plan).
pub fn parse_info(text: &str) -> Result<String, Error> {
    for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
        let Some(rest) = line.trim().strip_prefix("Account:") else {
            continue;
        };
        let name = rest.trim().trim_matches('\'').trim_matches('"').trim();
        if !name.is_empty() {
            return Ok(name.to_string());
        }
    }
    Err(
        Error::upstream("`protonvpn info` printed no `Account:` line")
            .with_hint("fproton doctor --only cli"),
    )
}

/// Parse the `config list` table.
///
/// The column boundary is taken from the `---  ---` rule line of **this** output, not
/// from a constant: the paid and free-tier renders have different widths, and
/// `custom-dns` carries a two-space run inside its own value (`on  [1.1.1.1, …]`).
pub fn parse_config_list(text: &str) -> Result<ProtonConfig, Error> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let rule = lines
        .iter()
        .position(|l| l.starts_with("---") && l.contains("  "))
        .ok_or_else(|| {
            Error::upstream("`protonvpn config list` printed no table")
                .with_hint("fproton doctor --only cli")
        })?;
    let width = lines[rule]
        .find("  ")
        .expect("the rule line was matched on a two-space run");

    let mut config = ProtonConfig::default();
    let mut rows = 0;
    for line in &lines[rule + 1..] {
        if line.trim().is_empty() {
            break; // the table ends at the blank line before the footer
        }
        if line.len() <= width {
            continue;
        }
        let (key, value) = line.split_at(width);
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            continue;
        }
        // A value the vocabulary does not cover (`Upgrade to enable`, a custom-dns row
        // with addresses) is stored verbatim, as §5 requires.
        if config.set(key, value) {
            rows += 1;
        }
    }
    if rows == 0 {
        return Err(Error::upstream(
            "`protonvpn config list` printed a table with no known settings",
        )
        .with_hint("fproton doctor --only cli"));
    }
    Ok(config)
}

/// Validate a username against `^[A-Za-z0-9._+@-]{1,64}$` before it reaches argv (§4).
pub fn is_valid_username(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 64
        && user
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'+' | b'@' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = include_str!("../../../tests/fixtures/protonvpn-info.txt");
    const CONFIG_LIST: &str = include_str!("../../../tests/fixtures/protonvpn-config-list.txt");
    const CONFIG_LIST_FREE: &str =
        include_str!("../../../tests/fixtures/protonvpn-config-list-free-tier.txt");
    const CONNECT_HELP: &str = include_str!("../../../tests/fixtures/protonvpn-help-connect.txt");
    const TOP_HELP: &str = include_str!("../../../tests/fixtures/protonvpn-help-top.txt");

    #[test]
    fn usernames_are_validated_before_they_reach_argv() {
        assert!(is_valid_username("oliver@example.com"));
        assert!(is_valid_username("user.name+tag"));
        assert!(!is_valid_username(""));
        assert!(!is_valid_username("; rm -rf"));
        assert!(!is_valid_username("a b"));
        assert!(!is_valid_username(&"x".repeat(65)));
    }

    #[test]
    fn every_connect_argv_is_spelled_the_way_the_captured_help_spells_it() {
        let cases = [
            (Target::Fastest, vec!["connect"]),
            (
                Target::Country("CH".into()),
                vec!["connect", "--country", "CH"],
            ),
            (
                Target::City("New York".into()),
                vec!["connect", "--city", "New York"],
            ),
            (Target::Server("CH#461".into()), vec!["connect", "CH#461"]),
            (Target::P2p, vec!["connect", "--p2p"]),
            (Target::SecureCore, vec!["connect", "--securecore"]),
            (Target::Tor, vec!["connect", "--tor"]),
            (Target::Random, vec!["connect", "--random"]),
        ];
        for (target, expected) in cases {
            assert_eq!(connect_args(&target), expected, "argv for {target}");
            for arg in expected.iter().filter(|a| a.starts_with("--")) {
                assert!(
                    CONNECT_HELP.contains(arg),
                    "`protonvpn connect --help` does not mention {arg}"
                );
            }
        }
        // The flag that is easy to get wrong: one word, no hyphen (§1.2).
        assert!(CONNECT_HELP.contains("--securecore"));
        assert!(!CONNECT_HELP.contains("--secure-core"));
    }

    #[test]
    fn every_subcommand_fproton_drives_exists_upstream() {
        for sub in [
            "signin",
            "signout",
            "info",
            "connect",
            "disconnect",
            "config",
        ] {
            assert!(
                TOP_HELP.contains(sub),
                "`protonvpn --help` does not list '{sub}'"
            );
        }
    }

    #[test]
    fn the_info_fixture_yields_the_account_name_and_nothing_else() {
        assert_eq!(parse_info(INFO).unwrap(), "user@example.com");
        assert_eq!(parse_info("Account: \"a@b.c\"").unwrap(), "a@b.c");
        // Upstream prints no plan; a missing line is exit 5, never a guess.
        assert_eq!(parse_info("Something else").unwrap_err().exit_code(), 5);
    }

    #[test]
    fn the_paid_config_fixture_parses_into_every_upstream_key() {
        let c = parse_config_list(CONFIG_LIST).expect("the paid fixture parses");
        assert_eq!(c.netshield.as_deref(), Some("malware-ads-trackers"));
        assert_eq!(c.kill_switch.as_deref(), Some("standard"));
        assert_eq!(c.port_forwarding.as_deref(), Some("off"));
        assert_eq!(c.vpn_accelerator.as_deref(), Some("on"));
        assert_eq!(c.moderate_nat.as_deref(), Some("off"));
        assert_eq!(c.ipv6.as_deref(), Some("off"));
        assert_eq!(c.anonymous_crash_reports.as_deref(), Some("on"));
        // The value with a two-space run inside it survives whole.
        assert_eq!(c.custom_dns.as_deref(), Some("on  [1.1.1.1, 8.8.8.8]"));
        // The footer is not a row.
        assert!(!format!("{c:?}").contains("Use 'protonvpn"));
    }

    #[test]
    fn the_free_tier_fixture_parses_despite_its_different_column_width() {
        let c = parse_config_list(CONFIG_LIST_FREE).expect("the free-tier fixture parses");
        assert_eq!(c.netshield.as_deref(), Some("Upgrade to enable"));
        assert_eq!(c.kill_switch.as_deref(), Some("standard"));
        assert_eq!(c.ipv6.as_deref(), Some("off"));
    }

    #[test]
    fn a_table_with_no_rule_line_is_an_error_not_an_empty_config() {
        assert_eq!(
            parse_config_list("Current configuration\n")
                .unwrap_err()
                .exit_code(),
            5
        );
    }

    #[test]
    fn upstreams_signed_out_wording_maps_to_exit_nine() {
        assert!(looks_signed_out("Error: You are not signed in."));
        assert!(!looks_signed_out("Error: connection failed"));
    }
}
