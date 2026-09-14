//! Connect targets and the engine entry points (DESIGN.md §4, §8).
//!
//! A [`Target`] is exactly one of the eight things `fproton connect` accepts. It is
//! validated **before** it can reach an argv (§2): the parser is the only door, and
//! `Display` round-trips it back to the exact string the CLI takes, so
//! History → Enter → reconnect needs no re-parsing.
//!
//! The connect/disconnect/reconnect bodies are Phase 1b; their signatures are frozen.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// What `fproton connect` was asked to reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// No argument: Proton picks.
    Fastest,
    /// Two-letter country code, e.g. `CH`.
    Country(String),
    /// A logical server name, e.g. `CH#461`, `IS-DE#1`, `US-CO#21-TOR`.
    Server(String),
    /// `city:"New York"`.
    City(String),
    P2p,
    SecureCore,
    Tor,
    Random,
}

/// The eight target forms, for `--help`, `capabilities` and the plugin's validator.
pub const TARGET_FORMS: &[&str] = &[
    "fastest",
    "CC",
    "CC#N",
    "city:NAME",
    "p2p",
    "secure-core",
    "tor",
    "random",
];

/// `^[A-Z]{2}$` (§2).
pub fn is_country_code(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|c| c.is_ascii_uppercase())
}

/// `^[A-Z]{2}(-(?:[A-Z]{2}|FREE))?(#[0-9]{1,5})?(-TOR)?$`, max 12 chars (§2).
/// Verified against all 18 221 `Name` values in Proton's serverlist.
pub fn is_server_name(s: &str) -> bool {
    if s.len() < 2 || s.len() > 12 || !s.is_ascii() {
        return false;
    }
    let b = s.as_bytes();
    if !(b[0].is_ascii_uppercase() && b[1].is_ascii_uppercase()) {
        return false;
    }
    let mut rest = &s[2..];
    rest = rest.strip_suffix("-TOR").unwrap_or(rest);
    if let Some(hash) = rest.rfind('#') {
        let (head, number) = rest.split_at(hash);
        let number = &number[1..];
        if number.is_empty() || number.len() > 5 || !number.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
        rest = head;
    }
    if rest.is_empty() {
        return true;
    }
    match rest.strip_prefix('-') {
        Some("FREE") => true,
        Some(cc) => is_country_code(cc),
        None => false,
    }
}

/// `^[\p{L} .'-]{1,40}$` (§2).
pub fn is_city_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=40).contains(&n)
        && s.chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '.' | '\'' | '-'))
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> Result<Target, Error> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Target::Fastest);
        }
        match s.to_ascii_lowercase().as_str() {
            "fastest" | "best" | "default" => return Ok(Target::Fastest),
            "p2p" => return Ok(Target::P2p),
            "secure-core" | "securecore" | "secure_core" => return Ok(Target::SecureCore),
            "tor" => return Ok(Target::Tor),
            "random" => return Ok(Target::Random),
            _ => {}
        }
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("city:") || lower.starts_with("city=") {
            let city = &s[5..];
            let city = city.trim().trim_matches('"').trim_matches('\'').trim();
            if !is_city_name(city) {
                return Err(Error::usage(format!(
                    "'{city}' is not a city name (letters, spaces, . ' - and at most 40 characters)"
                ))
                .with_hint("fproton cities --country CH"));
            }
            return Ok(Target::City(city.to_string()));
        }
        let upper = s.to_ascii_uppercase();
        if is_country_code(&upper) {
            return Ok(Target::Country(upper));
        }
        if is_server_name(&upper) {
            return Ok(Target::Server(upper));
        }
        Err(Error::usage(format!(
            "'{s}' is not a connect target; use one of: {}",
            TARGET_FORMS.join(" · ")
        ))
        .with_hint(format!("fproton servers --search {}", shell_safe_hint(s))))
    }
}

/// Keep a rejected value out of the hint when it contains anything shell-shaped.
/// The hint is printed, never executed, but it must stay copy-pasteable and honest.
fn shell_safe_hint(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        && !s.is_empty()
    {
        s.to_string()
    } else {
        "<text>".to_string()
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Fastest => f.write_str("fastest"),
            Target::Country(c) => f.write_str(c),
            Target::Server(n) => f.write_str(n),
            Target::City(c) => write!(f, "city:{c}"),
            Target::P2p => f.write_str("p2p"),
            Target::SecureCore => f.write_str("secure-core"),
            Target::Tor => f.write_str("tor"),
            Target::Random => f.write_str("random"),
        }
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Target, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Target {
    /// The kind name used by `capabilities` and the plugin's validator.
    pub fn kind(&self) -> &'static str {
        match self {
            Target::Fastest => "fastest",
            Target::Country(_) => "country",
            Target::Server(_) => "server",
            Target::City(_) => "city",
            Target::P2p => "p2p",
            Target::SecureCore => "secure-core",
            Target::Tor => "tor",
            Target::Random => "random",
        }
    }
}

/// Build a target from a server name that came from the index or from history.
pub fn target_for_server(name: &str) -> Result<Target, Error> {
    let upper = name.trim().to_ascii_uppercase();
    if is_server_name(&upper) {
        Ok(Target::Server(upper))
    } else {
        Err(
            Error::not_found(format!("'{name}' is not a Proton server name"))
                .with_hint("fproton servers --search <text>"),
        )
    }
}

/// What a completed connect reports (§8).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connected {
    /// The target as given, in its canonical spelling.
    pub target: String,
    /// The server Proton actually picked, when it could be resolved.
    pub server: Option<String>,
    pub country: Option<String>,
    /// Always `cli` in v0.1.
    pub engine: String,
    /// Wall clock of the engine call.
    pub duration_sec: u64,
    /// True when the tunnel was already up on that target and nothing was changed.
    pub already_connected: bool,
}

// ---------------------------------------------------------------- frozen API

/// Connect to `target`.
///
/// Writes `pending` to state **before** spawning and appends the history entry when
/// the engine exits — callers (main.rs and the TUI) must not duplicate either.
/// Polls `cancel` at least every 250 ms; on cancel it SIGTERM/SIGKILLs the child
/// group and returns `Err(Error::Interrupted)`. Never touches stdout/stderr.
pub fn connect(
    _ctx: &Context,
    _target: &Target,
    _cancel: &CancelToken,
) -> Result<Connected, Error> {
    Err(Error::unimplemented("engine::connect"))
}

/// Reconnect to the last target; `next` picks the next server in the same country by score.
pub fn reconnect(_ctx: &Context, _next: bool, _cancel: &CancelToken) -> Result<Connected, Error> {
    Err(Error::unimplemented("engine::reconnect"))
}

/// Disconnect. `force` tears `proton0` and the killswitch profile down through nmcli.
pub fn disconnect(_ctx: &Context, _force: bool, _cancel: &CancelToken) -> Result<(), Error> {
    Err(Error::unimplemented("engine::disconnect"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_server_names_parse() {
        for name in [
            "CH#461",
            "IS-DE#1",
            "US-CA#1022",
            "NL-FREE#15",
            "US-CO#21-TOR",
            "JP#1",
        ] {
            assert!(is_server_name(name), "{name} must be a server name");
            assert_eq!(name.parse::<Target>().unwrap(), Target::Server(name.into()));
        }
    }

    #[test]
    fn injection_shaped_input_is_rejected() {
        for bad in [
            "; rm -rf",
            "--",
            "CH#461 foo",
            "$(id)",
            "CH#4611111",
            "ch#461#1",
            "CH#",
            "A",
            "ABCDEFGHIJKLMNOP",
        ] {
            assert!(
                bad.parse::<Target>().is_err() || !is_server_name(&bad.to_ascii_uppercase()),
                "{bad} must not become a target"
            );
        }
        let e = "; rm -rf".parse::<Target>().unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!(e.hint(), "fproton servers --search <text>");
    }

    #[test]
    fn every_target_form_round_trips() {
        let cases = [
            ("fastest", Target::Fastest),
            ("CH", Target::Country("CH".into())),
            ("CH#461", Target::Server("CH#461".into())),
            ("city:New York", Target::City("New York".into())),
            ("p2p", Target::P2p),
            ("secure-core", Target::SecureCore),
            ("tor", Target::Tor),
            ("random", Target::Random),
        ];
        for (text, target) in cases {
            assert_eq!(text.parse::<Target>().unwrap(), target, "parsing {text}");
            assert_eq!(target.to_string(), text, "displaying {text}");
            assert_eq!(target.to_string().parse::<Target>().unwrap(), target);
        }
    }

    #[test]
    fn lowercase_and_quoted_input_is_normalised_not_rejected() {
        assert_eq!(
            "ch".parse::<Target>().unwrap(),
            Target::Country("CH".into())
        );
        assert_eq!(
            "ch#461".parse::<Target>().unwrap(),
            Target::Server("CH#461".into())
        );
        assert_eq!(
            "city:\"New York\"".parse::<Target>().unwrap(),
            Target::City("New York".into())
        );
        assert_eq!("securecore".parse::<Target>().unwrap(), Target::SecureCore);
    }

    #[test]
    fn city_names_with_odd_letters_are_allowed_but_metacharacters_are_not() {
        assert!(is_city_name("Zürich"));
        assert!(is_city_name("São Paulo"));
        assert!(is_city_name("Coeur d'Alene"));
        assert!(!is_city_name("New York; rm -rf"));
        assert!(!is_city_name(""));
        assert!(!is_city_name(&"x".repeat(41)));
    }

    #[test]
    fn target_for_server_rejects_non_servers() {
        assert_eq!(
            target_for_server("ch#461").unwrap(),
            Target::Server("CH#461".into())
        );
        assert_eq!(target_for_server("fastest").unwrap_err().exit_code(), 6);
    }

    #[test]
    fn targets_serialise_as_the_string_the_cli_accepts() {
        let t = Target::City("New York".into());
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"city:New York\"");
        let back: Target = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
