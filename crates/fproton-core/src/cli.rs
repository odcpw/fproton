//! The official `protonvpn` driver (DESIGN.md §1.2, §4).
//!
//! Everything here spawns proton-vpn-cli **under a process-group deadline**: the CLI
//! hangs on every session-touching command on this machine. A deadline kill is exit 5
//! and arms the 60 s `cli.hang` backoff. `status` never calls into this module.
//!
//! Only two text parsers exist, both fixture-driven: `info` and `config list` (§1.3).

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;
use crate::status::{Account, ProtonConfig};

/// The upstream binary name.
pub const PROGRAM: &str = "protonvpn";

// ---------------------------------------------------------------- frozen API

/// `protonvpn config list` under the probe deadline, parsed and cached in state.
pub fn config_list(_ctx: &Context, _cancel: &CancelToken) -> Result<ProtonConfig, Error> {
    Err(Error::unimplemented("cli::config_list"))
}

/// `protonvpn config set <key> <value>` under the `config_set` deadline.
/// The pair must already have passed `settings::validate_proton_config`.
pub fn config_set(
    _ctx: &Context,
    _key: &str,
    _value: &str,
    _cancel: &CancelToken,
) -> Result<(), Error> {
    Err(Error::unimplemented("cli::config_set"))
}

/// `protonvpn info` under the probe deadline; parses the single `Account: '<name>'` line.
pub fn account(_ctx: &Context, _cancel: &CancelToken) -> Result<Account, Error> {
    Err(Error::unimplemented("cli::account"))
}

/// `protonvpn signin <user>` in the current terminal, interactive, **no deadline** (§2).
pub fn signin(_ctx: &Context, _user: &str) -> Result<(), Error> {
    Err(Error::unimplemented("cli::signin"))
}

/// `protonvpn signout` under the probe deadline.
pub fn signout(_ctx: &Context, _cancel: &CancelToken) -> Result<(), Error> {
    Err(Error::unimplemented("cli::signout"))
}

/// Parse `protonvpn info` output (one `Account: '<name>'` line; upstream prints no plan).
pub fn parse_info(_text: &str) -> Result<String, Error> {
    Err(Error::unimplemented("cli::parse_info"))
}

/// Parse the `config list` table: split on runs of >= 2 spaces, anchored by the `---` rule.
pub fn parse_config_list(_text: &str) -> Result<ProtonConfig, Error> {
    Err(Error::unimplemented("cli::parse_config_list"))
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

    #[test]
    fn usernames_are_validated_before_they_reach_argv() {
        assert!(is_valid_username("oliver@example.com"));
        assert!(is_valid_username("user.name+tag"));
        assert!(!is_valid_username(""));
        assert!(!is_valid_username("; rm -rf"));
        assert!(!is_valid_username("a b"));
        assert!(!is_valid_username(&"x".repeat(65)));
    }
}
