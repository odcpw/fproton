//! Typed errors: exit code, stable `code_name` and a runnable hint (DESIGN.md §4a).
//!
//! Every frozen core function returns `Result<_, Error>` so the binary can map a
//! failure to its exit code without downcasting. Each variant carries an optional
//! hint that overrides the default one for its class; `hint()` always answers with
//! a command the caller can paste.

/// Exit-code dictionary (§4a). Printed verbatim by `capabilities` and `robot-docs`.
pub const EXIT_CODES: &[(i32, &str, &str)] = &[
    (0, "ok", "success"),
    (
        1,
        "failure",
        "unexpected failure (bug or I/O); retrying rarely helps",
    ),
    (2, "usage", "bad arguments, unknown config key or value"),
    (
        3,
        "safety",
        "refused without --yes; the hint names the flag",
    ),
    (
        4,
        "backend",
        "protonvpn not installed, NetworkManager down, or a polkit denial",
    ),
    (
        5,
        "upstream",
        "protonvpn/nmcli errored or was killed at its deadline (the CLI hang lands here)",
    ),
    (
        6,
        "not_found",
        "unknown target, server, country or city; no tunnel for `port`",
    ),
    (7, "ambiguous", "the target matched several servers"),
    (8, "timeout", "--wait ran out; nothing is broken"),
    (9, "not_signed_in", "no Proton session"),
    (10, "index", "server index missing or corrupt"),
    (11, "doctor", "doctor found failures"),
    (
        12,
        "doctor_partial",
        "a fixer failed, or the fix was partial",
    ),
    (
        13,
        "doctor_refused",
        "unsafe precondition or undo integrity failure",
    ),
    (130, "interrupted", "SIGINT"),
];

/// A failure with an exit code (§4a), a stable name and a runnable hint.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// 1 — unexpected (bug, I/O).
    #[error("{msg}")]
    Failure { msg: String, hint: Option<String> },
    /// 2 — bad arguments, unknown config key/value.
    #[error("{msg}")]
    Usage { msg: String, hint: Option<String> },
    /// 3 — refused without `--yes`.
    #[error("{msg}")]
    Safety { msg: String, hint: Option<String> },
    /// 4 — protonvpn/NetworkManager missing or a polkit denial.
    #[error("{msg}")]
    Backend { msg: String, hint: Option<String> },
    /// 5 — an external command errored or was killed at its deadline.
    #[error("{msg}")]
    Upstream { msg: String, hint: Option<String> },
    /// 6 — unknown target/server/country/city, or no tunnel.
    #[error("{msg}")]
    NotFound { msg: String, hint: Option<String> },
    /// 7 — the target matched several servers.
    #[error("{msg}")]
    Ambiguous { msg: String, hint: Option<String> },
    /// 8 — a `--wait` ran out; nothing is broken.
    #[error("{msg}")]
    Timeout { msg: String, hint: Option<String> },
    /// 9 — no Proton session.
    #[error("{msg}")]
    NotSignedIn { msg: String, hint: Option<String> },
    /// 10 — the server index is missing or unusable.
    #[error("{msg}")]
    Index { msg: String, hint: Option<String> },
    /// 11 — the doctor found failures.
    #[error("{msg}")]
    Doctor { msg: String, hint: Option<String> },
    /// 12 — a fixer failed, or fixed only part of the problem.
    #[error("{msg}")]
    DoctorPartial { msg: String, hint: Option<String> },
    /// 13 — an unsafe precondition, or an undo integrity failure.
    #[error("{msg}")]
    DoctorRefused { msg: String, hint: Option<String> },
    /// 130 — SIGINT, or a cancelled action (`CancelToken`).
    #[error("interrupted")]
    Interrupted,
}

macro_rules! constructors {
    ($($name:ident => $variant:ident),* $(,)?) => {
        $(
            #[doc = concat!("Build an `Error::", stringify!($variant), "`.")]
            pub fn $name(msg: impl Into<String>) -> Error {
                Error::$variant { msg: msg.into(), hint: None }
            }
        )*
    };
}

impl Error {
    constructors! {
        failure => Failure,
        usage => Usage,
        safety => Safety,
        backend => Backend,
        upstream => Upstream,
        not_found => NotFound,
        ambiguous => Ambiguous,
        timeout => Timeout,
        not_signed_in => NotSignedIn,
        index => Index,
        doctor => Doctor,
        doctor_partial => DoctorPartial,
        doctor_refused => DoctorRefused,
    }

    /// A function that is declared by the frozen API but not implemented in this build.
    /// Never presented as success: the caller sees exit 1 and a message naming the gap.
    pub fn unimplemented(what: &str) -> Error {
        Error::failure(format!("{what} is not implemented in this build"))
            .with_hint("fproton capabilities   # what this build actually does")
    }

    /// Attach a runnable hint, overriding the default for this class.
    #[must_use]
    pub fn with_hint(self, hint: impl Into<String>) -> Error {
        let h = Some(hint.into());
        match self {
            Error::Failure { msg, .. } => Error::Failure { msg, hint: h },
            Error::Usage { msg, .. } => Error::Usage { msg, hint: h },
            Error::Safety { msg, .. } => Error::Safety { msg, hint: h },
            Error::Backend { msg, .. } => Error::Backend { msg, hint: h },
            Error::Upstream { msg, .. } => Error::Upstream { msg, hint: h },
            Error::NotFound { msg, .. } => Error::NotFound { msg, hint: h },
            Error::Ambiguous { msg, .. } => Error::Ambiguous { msg, hint: h },
            Error::Timeout { msg, .. } => Error::Timeout { msg, hint: h },
            Error::NotSignedIn { msg, .. } => Error::NotSignedIn { msg, hint: h },
            Error::Index { msg, .. } => Error::Index { msg, hint: h },
            Error::Doctor { msg, .. } => Error::Doctor { msg, hint: h },
            Error::DoctorPartial { msg, .. } => Error::DoctorPartial { msg, hint: h },
            Error::DoctorRefused { msg, .. } => Error::DoctorRefused { msg, hint: h },
            Error::Interrupted => Error::Interrupted,
        }
    }

    /// The process exit code for this error (§4a).
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Failure { .. } => 1,
            Error::Usage { .. } => 2,
            Error::Safety { .. } => 3,
            Error::Backend { .. } => 4,
            Error::Upstream { .. } => 5,
            Error::NotFound { .. } => 6,
            Error::Ambiguous { .. } => 7,
            Error::Timeout { .. } => 8,
            Error::NotSignedIn { .. } => 9,
            Error::Index { .. } => 10,
            Error::Doctor { .. } => 11,
            Error::DoctorPartial { .. } => 12,
            Error::DoctorRefused { .. } => 13,
            Error::Interrupted => 130,
        }
    }

    /// The stable machine name for this error class (§4a).
    pub fn code_name(&self) -> &'static str {
        let code = self.exit_code();
        EXIT_CODES
            .iter()
            .find(|(c, _, _)| *c == code)
            .map(|(_, n, _)| *n)
            .unwrap_or("failure")
    }

    /// A runnable next step. Always non-empty: the caller's own hint when one was
    /// attached, otherwise the default for this class.
    pub fn hint(&self) -> String {
        let custom = match self {
            Error::Failure { hint, .. }
            | Error::Usage { hint, .. }
            | Error::Safety { hint, .. }
            | Error::Backend { hint, .. }
            | Error::Upstream { hint, .. }
            | Error::NotFound { hint, .. }
            | Error::Ambiguous { hint, .. }
            | Error::Timeout { hint, .. }
            | Error::NotSignedIn { hint, .. }
            | Error::Index { hint, .. }
            | Error::Doctor { hint, .. }
            | Error::DoctorPartial { hint, .. }
            | Error::DoctorRefused { hint, .. } => hint.clone(),
            Error::Interrupted => None,
        };
        custom.unwrap_or_else(|| self.default_hint().to_string())
    }

    fn default_hint(&self) -> &'static str {
        match self {
            Error::Failure { .. } => "fproton doctor",
            Error::Usage { .. } => "fproton --help",
            Error::Safety { .. } => "re-run with --yes",
            Error::Backend { .. } => "fproton doctor",
            Error::Upstream { .. } => "fproton doctor",
            Error::NotFound { .. } => "fproton countries",
            Error::Ambiguous { .. } => "fproton servers",
            Error::Timeout { .. } => "fproton status --json",
            Error::NotSignedIn { .. } => "fproton signin",
            Error::Index { .. } => "fproton index --rebuild",
            Error::Doctor { .. } => "fproton doctor --fix",
            Error::DoctorPartial { .. } => "fproton doctor",
            Error::DoctorRefused { .. } => "fproton doctor --json",
            Error::Interrupted => "fproton status --json",
        }
    }

    /// True for a cancelled action (`CancelToken`) or SIGINT.
    pub fn is_interrupted(&self) -> bool {
        matches!(self, Error::Interrupted)
    }

    /// The `--json` error envelope (§2): same keys for every command.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "schema": crate::SCHEMA_VERSION,
            "code": self.exit_code(),
            "code_name": self.code_name(),
            "error": self.to_string(),
            "hint": self.hint(),
        })
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::failure(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::failure(format!("JSON: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_names_match_the_dictionary() {
        for (code, name, _) in EXIT_CODES {
            if *code == 0 {
                continue;
            }
            let e = match *code {
                1 => Error::failure("x"),
                2 => Error::usage("x"),
                3 => Error::safety("x"),
                4 => Error::backend("x"),
                5 => Error::upstream("x"),
                6 => Error::not_found("x"),
                7 => Error::ambiguous("x"),
                8 => Error::timeout("x"),
                9 => Error::not_signed_in("x"),
                10 => Error::index("x"),
                11 => Error::doctor("x"),
                12 => Error::doctor_partial("x"),
                13 => Error::doctor_refused("x"),
                130 => Error::Interrupted,
                other => panic!("undocumented code {other}"),
            };
            assert_eq!(e.exit_code(), *code);
            assert_eq!(e.code_name(), *name);
            assert!(!e.hint().is_empty(), "every error needs a hint");
        }
    }

    #[test]
    fn a_custom_hint_wins_and_survives_in_json() {
        let e = Error::not_found("unknown target 'XX'").with_hint("fproton countries --json");
        let v = e.json();
        assert_eq!(v["code"], 6);
        assert_eq!(v["code_name"], "not_found");
        assert_eq!(v["hint"], "fproton countries --json");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "unknown target 'XX'");
    }
}
