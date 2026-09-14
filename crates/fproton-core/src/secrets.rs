//! Read-only Secret Service probes over `busctl` (DESIGN.md §5, §7).
//!
//! **Never opens a `*.keyring` file**, never unlocks, never restarts a daemon, never
//! reads an item's secret: the only calls are `ReadAlias`, `Collections`, a `Label`/
//! `Locked` property GET and `SearchItems`. The `SetAlias` **write** lives in the
//! doctor's `mutate.rs`, not here.

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// D-Bus names, spelled once.
pub const SERVICE: &str = "org.freedesktop.secrets";
pub const OBJECT: &str = "/org/freedesktop/secrets";
pub const IFACE_SERVICE: &str = "org.freedesktop.Secret.Service";
pub const IFACE_COLLECTION: &str = "org.freedesktop.Secret.Collection";
/// The attribute Proton stores its session under (`service=Proton`).
pub const PROTON_ATTRIBUTE: (&str, &str) = ("service", "Proton");

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

// ---------------------------------------------------------------- frozen API

/// `ReadAlias s default`, then `get-property … Label Locked` on the result.
pub fn read_alias_default() -> Result<AliasProbe, Error> {
    Err(Error::unimplemented("secrets::read_alias_default"))
}

/// The `Collections` property: every collection object path.
pub fn collections() -> Result<Vec<String>, Error> {
    Err(Error::unimplemented("secrets::collections"))
}

/// `SearchItems a{ss} 1 service Proton` → unlocked and locked item paths, concatenated.
/// Feeds `status.signed_in` (~6 ms, no unlock, never spawns protonvpn).
pub fn search_proton_items() -> Result<Vec<String>, Error> {
    Err(Error::unimplemented("secrets::search_proton_items"))
}
