//! The server index: Proton's 24 MB `serverlist.json` reduced to a postcard snapshot
//! at `$XDG_CACHE_HOME/fproton/index.bin` (DESIGN.md §4 `index`).
//!
//! Types and the snapshot header are frozen here; the reader/writer is Phase 1b.
//! `format_version` is bumped on **any** change to a serialized struct below —
//! postcard is not self-describing, so a stale snapshot must be detectable.

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// Magic bytes at the head of `index.bin`.
pub const INDEX_MAGIC: [u8; 4] = *b"FPIX";
/// Bump on any change to `IndexHeader`, `ServerRow`, `CountryRow` or `CityRow`.
pub const INDEX_FORMAT_VERSION: u32 = 1;

/// Proton's `Features` bitmask (§1.3). The raw integer is always kept alongside.
pub const FEATURE_BITS: &[(u32, &str)] = &[
    (1, "secure-core"),
    (2, "tor"),
    (4, "p2p"),
    (8, "streaming"),
    (16, "ipv6"),
];

/// Expand a `Features` bitmask into stable label strings, lowest bit first.
pub fn features_from_raw(raw: u32) -> Vec<String> {
    FEATURE_BITS
        .iter()
        .filter(|(bit, _)| raw & bit != 0)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

/// A filterable feature (`servers --feature`). `ipv6` is not a filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    P2p,
    SecureCore,
    Tor,
    Streaming,
}

impl Feature {
    pub fn bit(self) -> u32 {
        match self {
            Feature::SecureCore => 1,
            Feature::Tor => 2,
            Feature::P2p => 4,
            Feature::Streaming => 8,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Feature::P2p => "p2p",
            Feature::SecureCore => "secure-core",
            Feature::Tor => "tor",
            Feature::Streaming => "streaming",
        }
    }
}

/// Account entitlement. Only 0 and 2 occur in Proton's data (§1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Plus,
}

impl Tier {
    pub fn level(self) -> u8 {
        match self {
            Tier::Free => 0,
            Tier::Plus => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Tier::Free => "free",
            Tier::Plus => "plus",
        }
    }
}

/// Row order for `servers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Load,
    #[default]
    Score,
    Name,
}

impl Sort {
    pub fn name(self) -> &'static str {
        match self {
            Sort::Load => "load",
            Sort::Score => "score",
            Sort::Name => "name",
        }
    }
}

/// Everything `fproton servers` can ask for.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Query {
    pub country: Option<String>,
    pub city: Option<String>,
    pub feature: Option<Feature>,
    pub tier: Option<Tier>,
    pub search: Option<String>,
    pub sort: Sort,
    pub limit: Option<usize>,
}

/// One logical server, as `servers` prints it and the TUI lists it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerRow {
    pub name: String,
    pub country: String,
    pub country_name: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub load: u8,
    pub score: f64,
    pub tier: u8,
    pub features: Vec<String>,
    pub features_raw: u32,
    pub entry_ip: Option<String>,
    pub exit_ip: Option<String>,
    /// Proton's `Status`: 1 = usable.
    pub status: u8,
}

/// One country, as `countries` prints it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CountryRow {
    pub country: String,
    pub country_name: Option<String>,
    pub servers: u32,
    pub cities: u32,
    pub min_load: Option<u8>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub has_free: bool,
}

/// One (country, city) pair, as `cities` prints it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CityRow {
    pub city: String,
    pub country: String,
    pub servers: u32,
    pub min_load: Option<u8>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub has_free: bool,
}

/// The snapshot header (§4 `index`). A snapshot whose magic, version, source mtime
/// or source length does not match is **stale, not corrupt**: ignored and rebuilt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexHeader {
    pub magic: [u8; 4],
    pub format_version: u32,
    pub source_path: String,
    pub source_mtime_unix_nanos: i128,
    pub source_len: u64,
    /// RFC 3339 with local offset.
    pub built_at: String,
    pub server_count: u32,
    pub max_tier: Option<u8>,
}

/// What a rebuild reports (`index --rebuild --json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexStats {
    pub servers: u32,
    pub countries: u32,
    pub cities: u32,
    pub max_tier: Option<u8>,
    pub source_path: String,
    pub source_len: u64,
    /// RFC 3339 with local offset.
    pub source_mtime: Option<String>,
    pub built_at: String,
    pub path: String,
    pub bytes: u64,
    pub duration_ms: u64,
    /// False when the snapshot was already fresh and nothing was written.
    pub rebuilt: bool,
}

/// What `status` reports about the index without ever rebuilding it (§5 `index`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexInfo {
    pub servers: Option<u32>,
    pub built_from_mtime: Option<String>,
    pub serverlist_expires_at: Option<String>,
    pub loads_expired: Option<bool>,
}

impl IndexInfo {
    /// Everything unknown: no snapshot, no serverlist.
    pub fn unknown() -> IndexInfo {
        IndexInfo {
            servers: None,
            built_from_mtime: None,
            serverlist_expires_at: None,
            loads_expired: None,
        }
    }
}

// ---------------------------------------------------------------- frozen API

/// Matching servers, ordered by `q.sort`. Rebuilds a stale snapshot first (< 20 ms warm).
pub fn servers(_ctx: &Context, _q: &Query) -> Result<Vec<ServerRow>, Error> {
    Err(Error::unimplemented("index::servers"))
}

/// Every country in the snapshot, with counts and the lowest load.
pub fn countries(_ctx: &Context) -> Result<Vec<CountryRow>, Error> {
    Err(Error::unimplemented("index::countries"))
}

/// Every city of one country (all cities when `country` is empty).
pub fn cities(_ctx: &Context, _country: &str) -> Result<Vec<CityRow>, Error> {
    Err(Error::unimplemented("index::cities"))
}

/// Rebuild the snapshot from `serverlist.json` (tmp file + `rename(2)`; no lock).
pub fn rebuild(_ctx: &Context, _cancel: &CancelToken) -> Result<IndexStats, Error> {
    Err(Error::unimplemented("index::rebuild"))
}

/// Header facts only; never rebuilds, never parses the 24 MB source (`status` uses this).
pub fn info(_ctx: &Context) -> Result<IndexInfo, Error> {
    Err(Error::unimplemented("index::info"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_feature_mask_expands_the_way_the_design_says() {
        // 28 = p2p | streaming | ipv6 is the common value on this machine (§1.3).
        assert_eq!(features_from_raw(28), vec!["p2p", "streaming", "ipv6"]);
        assert_eq!(features_from_raw(0), Vec::<String>::new());
        assert_eq!(features_from_raw(1), vec!["secure-core"]);
        assert_eq!(features_from_raw(2), vec!["tor"]);
    }

    #[test]
    fn feature_bits_agree_with_the_labels() {
        for f in [
            Feature::P2p,
            Feature::SecureCore,
            Feature::Tor,
            Feature::Streaming,
        ] {
            assert_eq!(features_from_raw(f.bit()), vec![f.name().to_string()]);
        }
    }
}
