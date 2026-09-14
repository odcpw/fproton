//! Read-only Secret Service probes over `busctl` (DESIGN.md §5, §7).
//!
//! **Never opens a `*.keyring` file**, never unlocks, never restarts a daemon, never
//! reads an item's secret: the only calls are `ReadAlias`, `Collections`, a `Label`/
//! `Locked` property GET and `SearchItems`. The `SetAlias` **write** lives in the
//! doctor's `mutate.rs`, not here.
//!
//! Everything returns object paths and labels. An item path is not a secret, and no
//! call here can produce one: the Secret Service method that returns item secrets is
//! never invoked, and a unit test greps this module for its name.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::proc::{run_with_deadline, Output};

/// D-Bus names, spelled once.
pub const SERVICE: &str = "org.freedesktop.secrets";
pub const OBJECT: &str = "/org/freedesktop/secrets";
pub const IFACE_SERVICE: &str = "org.freedesktop.Secret.Service";
pub const IFACE_COLLECTION: &str = "org.freedesktop.Secret.Collection";
/// The attribute Proton stores its session under (`service=Proton`).
pub const PROTON_ATTRIBUTE: (&str, &str) = ("service", "Proton");

/// The busctl binary.
pub const PROGRAM: &str = "busctl";
/// Every call here is a local property read; 1 s is a generous deadline (§2).
const DEADLINE: Duration = Duration::from_secs(1);
/// busctl's own timeout, so it gives up before fproton has to kill it.
const BUS_TIMEOUT: &str = "--timeout=1";

/// What `ReadAlias("default")` plus a `Label`/`Locked` GET says about the default
/// collection. On this machine today the alias resolves to a path with **no exported
/// object**: `exported == false` (§1.2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AliasProbe {
    pub alias: String,
    /// The object path the alias points at, if any.
    pub path: Option<String>,
    /// False when a property GET answers "Object does not exist at path".
    pub exported: bool,
    pub label: Option<String>,
    pub locked: Option<bool>,
    /// The raw busctl output, for the doctor's probe artifacts. Never holds a secret.
    pub raw: String,
}

/// The error prefix gnome-keyring's phantom collection produces. The path in the real
/// message is wrapped in **typographic** quotes (U+201C/U+201D), so only the prefix
/// may be matched.
pub const PHANTOM_PREFIX: &str = "Object does not exist at path";

/// True when a busctl failure is the phantom-collection error, not a real fault.
pub fn is_phantom_error(text: &str) -> bool {
    text.contains(PHANTOM_PREFIX)
}

fn busctl(args: &[&str]) -> Result<Output, Error> {
    let mut argv = vec![PROGRAM, "--user", BUS_TIMEOUT];
    argv.extend_from_slice(args);
    run_with_deadline(&argv, DEADLINE, None)
}

/// Fail a probe with the busctl detail attached, as exit 5.
fn upstream(what: &str, out: &Output) -> Error {
    if out.timed_out {
        return Error::upstream(format!("busctl {what} was killed at its 1s deadline"))
            .with_hint("fproton doctor --only keyring");
    }
    Error::upstream(format!("busctl {what}: {}", out.stderr_tail()))
        .with_hint("fproton doctor --only keyring")
}

/// Every `"…"`-quoted token of a busctl reply, in order. busctl prints object paths
/// and strings quoted; counts and booleans are bare, so this yields exactly the paths.
fn quoted(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        match after.find('"') {
            Some(end) => {
                found.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    found
}

/// Parse `o "/org/freedesktop/secrets/collection/X"`.
pub fn parse_object_path(text: &str) -> Option<String> {
    quoted(text)
        .into_iter()
        .next()
        .filter(|p| p.starts_with('/'))
}

/// Parse `ao 3 "…" "…" "…"` or `aoao 2 "…" "…" 0` into every path it names.
pub fn parse_object_paths(text: &str) -> Vec<String> {
    quoted(text)
        .into_iter()
        .filter(|p| p.starts_with('/'))
        .collect()
}

/// Parse the two-line `get-property … Label Locked` reply (`s "Default"` / `b false`).
pub fn parse_label_locked(text: &str) -> (Option<String>, Option<bool>) {
    let mut label = None;
    let mut locked = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("s ") {
            label = quoted(rest).into_iter().next();
        } else if let Some(rest) = line.strip_prefix("b ") {
            locked = match rest.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    (label, locked)
}

// ---------------------------------------------------------------- frozen API

/// `ReadAlias s default`, then `get-property … Label Locked` on the result.
pub fn read_alias_default() -> Result<AliasProbe, Error> {
    let out = busctl(&[
        "call",
        SERVICE,
        OBJECT,
        IFACE_SERVICE,
        "ReadAlias",
        "s",
        "default",
    ])?;
    if !out.success() {
        return Err(upstream("ReadAlias default", &out));
    }
    let alias_reply = out.stdout_string();
    let path = parse_object_path(&alias_reply)
        // `/` is the Secret Service's "no such alias" answer.
        .filter(|p| p != "/");
    let mut probe = AliasProbe {
        alias: "default".to_string(),
        path: path.clone(),
        exported: false,
        label: None,
        locked: None,
        raw: alias_reply.trim_end().to_string(),
    };
    let Some(path) = path else {
        return Ok(probe);
    };

    let get = busctl(&[
        "get-property",
        SERVICE,
        &path,
        IFACE_COLLECTION,
        "Label",
        "Locked",
    ])?;
    let reply = if get.success() {
        get.stdout_string()
    } else {
        get.stderr_string()
    };
    probe.raw = format!("{}\n{}", probe.raw, reply.trim_end());
    if get.success() {
        probe.exported = true;
        let (label, locked) = parse_label_locked(&reply);
        probe.label = label;
        probe.locked = locked;
    } else if !is_phantom_error(&reply) && !get.timed_out {
        // A real fault (the bus went away, the name vanished) is not "phantom".
        return Err(upstream("get-property Label Locked", &get));
    }
    Ok(probe)
}

/// The `Collections` property: every collection object path.
pub fn collections() -> Result<Vec<String>, Error> {
    let out = busctl(&[
        "get-property",
        SERVICE,
        OBJECT,
        IFACE_SERVICE,
        "Collections",
    ])?;
    if !out.success() {
        return Err(upstream("get-property Collections", &out));
    }
    Ok(parse_object_paths(&out.stdout_string()))
}

/// `SearchItems a{ss} 1 service Proton` → unlocked and locked item paths, concatenated.
/// Feeds `status.signed_in` (~6 ms, no unlock, never spawns protonvpn).
pub fn search_proton_items() -> Result<Vec<String>, Error> {
    let (attr, value) = PROTON_ATTRIBUTE;
    let out = busctl(&[
        "call",
        SERVICE,
        OBJECT,
        IFACE_SERVICE,
        "SearchItems",
        "a{ss}",
        "1",
        attr,
        value,
    ])?;
    if !out.success() {
        return Err(upstream("SearchItems service=Proton", &out));
    }
    Ok(parse_object_paths(&out.stdout_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const READALIAS: &str = include_str!("../../../tests/fixtures/busctl-readalias-default.txt");
    const COLLECTIONS: &str = include_str!("../../../tests/fixtures/busctl-collections.txt");
    const LABEL_OK: &str =
        include_str!("../../../tests/fixtures/busctl-getprop-label-locked-default.txt");
    const LABEL_PHANTOM: &str =
        include_str!("../../../tests/fixtures/busctl-getprop-label-phantom.txt");
    const SEARCH: &str =
        include_str!("../../../tests/fixtures/busctl-searchitems-service-proton.txt");

    #[test]
    fn the_captured_readalias_reply_yields_the_phantom_collection_path() {
        assert_eq!(
            parse_object_path(READALIAS).as_deref(),
            Some("/org/freedesktop/secrets/collection/Default_5fkeyring")
        );
    }

    #[test]
    fn the_captured_collections_reply_lists_all_three_collections() {
        let paths = parse_object_paths(COLLECTIONS);
        assert_eq!(paths.len(), 3);
        assert!(paths
            .iter()
            .any(|p| p.ends_with("/collection/Default_5fkeyring")));
        assert!(paths.iter().any(|p| p.ends_with("/collection/Default")));
        assert!(paths.iter().any(|p| p.ends_with("/collection/session")));
    }

    #[test]
    fn the_captured_label_reply_parses_into_label_and_lock_state() {
        assert_eq!(
            parse_label_locked(LABEL_OK),
            (Some("Default".to_string()), Some(false))
        );
    }

    #[test]
    fn the_phantom_error_is_matched_by_prefix_not_by_its_typographic_quotes() {
        assert!(is_phantom_error(LABEL_PHANTOM));
        assert!(
            LABEL_PHANTOM.contains('\u{201c}'),
            "the capture really does use typographic quotes"
        );
        assert!(!is_phantom_error("Failed to activate service: timed out"));
    }

    #[test]
    fn the_captured_search_reply_returns_both_proton_item_paths() {
        let paths = parse_object_paths(SEARCH);
        assert_eq!(
            paths,
            vec![
                "/org/freedesktop/secrets/collection/Default/1".to_string(),
                "/org/freedesktop/secrets/collection/Default/2".to_string(),
            ],
            "unlocked and locked arrays are concatenated"
        );
    }

    #[test]
    fn no_probe_can_return_an_item_secret() {
        // Everything this module parses is a path or a label: a defence by argv, not
        // by pattern. The calls that could reach key material are never spelled in the
        // module body (the test block below is excluded, or it would match itself).
        let source = include_str!("secrets.rs");
        let body = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in ["GetSecrets", "Unlock", "keyrings", "--show-secrets"] {
            assert!(
                !body.contains(forbidden),
                "secrets.rs must never name '{forbidden}'"
            );
        }
    }

    /// Live, on this machine: the real Secret Service, over the real session bus.
    /// Asserts only structural facts, so it keeps passing after the owner repairs the
    /// keyring (today `exported` is false; tomorrow it should be true).
    #[test]
    fn live_secret_service_answers_every_read_only_probe() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no session bus: skipping the live Secret Service probe");
            return;
        }
        if crate::proc::which(PROGRAM).is_none() {
            eprintln!("no busctl: skipping the live Secret Service probe");
            return;
        }
        let probe = read_alias_default().expect("ReadAlias answers");
        assert_eq!(probe.alias, "default");
        assert!(
            probe.path.as_deref().is_some_and(|p| p.starts_with('/')),
            "the default alias resolves to some path: {probe:?}"
        );
        if probe.exported {
            assert!(probe.label.is_some() && probe.locked.is_some());
        }
        let cols = collections().expect("Collections answers");
        assert!(!cols.is_empty(), "at least one collection exists");
        // Never asserts that a Proton item exists: that depends on the session.
        let items = search_proton_items().expect("SearchItems answers");
        assert!(items.iter().all(|p| p.starts_with('/')));
    }
}
