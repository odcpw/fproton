//! The server index: Proton's 24 MB `serverlist.json` reduced to a postcard snapshot
//! at `$XDG_CACHE_HOME/fproton/index.bin` (DESIGN.md §4 `index`).
//!
//! `format_version` is bumped on **any** change to a serialized struct below —
//! postcard is not self-describing, so a stale snapshot must be detectable.
//!
//! On-disk layout, three postcard documents written back to back so a reader can stop
//! after the first two:
//!
//! ```text
//! [ IndexHeader ][ IndexMeta ][ countries ][ cities ][ servers ]
//!   ~120 bytes     ~20 bytes    149 rows     195 rows    18 221 rows
//! ```
//!
//! `status` reads only the first 4 KiB — header and meta — and never touches the
//! payload or the 24 MB source (§4: `status` never rebuilds).

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// Magic bytes at the head of `index.bin`.
pub const INDEX_MAGIC: [u8; 4] = *b"FPIX";
/// Bump on any change to `IndexHeader`, `IndexMeta`, `ServerRow`, `CountryRow` or `CityRow`.
pub const INDEX_FORMAT_VERSION: u32 = 2;

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

/// The second postcard document: the few facts `status` needs that are not in the
/// §4 header contract, kept out of the payload so reading them costs one 4 KiB read.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexMeta {
    /// Proton's `ExpirationTime` (Unix seconds), when the source carried one.
    pub serverlist_expires_unix: Option<i64>,
    /// Proton's `LoadsExpirationTime` (Unix seconds).
    pub loads_expire_unix: Option<i64>,
    pub country_count: u32,
    pub city_count: u32,
}

/// One stored server. Deliberately **not** [`ServerRow`]: the derived fields
/// (`features`, `country_name`) are recomputed for the handful of rows a query
/// returns instead of being decoded for all 18 221 — that is what keeps `servers`
/// inside its 20 ms budget.
#[derive(Clone, Debug, Serialize)]
struct StoredServer {
    name: String,
    country: String,
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    load: u8,
    score: f64,
    tier: u8,
    features_raw: u32,
    entry_ip: Option<String>,
    exit_ip: Option<String>,
    status: u8,
}

/// The read side of [`StoredServer`], borrowing every string straight out of the
/// mapped file. postcard encodes `String` and `&str` identically, so the two structs
/// share one wire format — a test round-trips them to prove it.
#[derive(Clone, Debug, Deserialize)]
struct StoredServerRef<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow)]
    country: &'a str,
    #[serde(borrow)]
    city: Option<&'a str>,
    lat: Option<f64>,
    lon: Option<f64>,
    load: u8,
    score: f64,
    tier: u8,
    features_raw: u32,
    #[serde(borrow)]
    entry_ip: Option<&'a str>,
    #[serde(borrow)]
    exit_ip: Option<&'a str>,
    status: u8,
}

impl StoredServerRef<'_> {
    /// Expand one stored row into the public [`ServerRow`].
    fn to_row(&self) -> ServerRow {
        ServerRow {
            name: self.name.to_string(),
            country: self.country.to_string(),
            country_name: country_name(self.country).map(str::to_string),
            city: self.city.map(str::to_string),
            lat: self.lat,
            lon: self.lon,
            load: self.load,
            score: self.score,
            tier: self.tier,
            features: features_from_raw(self.features_raw),
            features_raw: self.features_raw,
            entry_ip: self.entry_ip.map(str::to_string),
            exit_ip: self.exit_ip.map(str::to_string),
            status: self.status,
        }
    }
}

impl From<&ServerRow> for StoredServer {
    fn from(r: &ServerRow) -> StoredServer {
        StoredServer {
            name: r.name.clone(),
            country: r.country.clone(),
            city: r.city.clone(),
            lat: r.lat,
            lon: r.lon,
            load: r.load,
            score: r.score,
            tier: r.tier,
            features_raw: r.features_raw,
            entry_ip: r.entry_ip.clone(),
            exit_ip: r.exit_ip.clone(),
            status: r.status,
        }
    }
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

/// How the snapshot on disk relates to Proton's file right now. `status` reports this
/// as a warning; every query rebuilds on anything but [`Freshness::Fresh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// Header matches the source's mtime and length.
    Fresh,
    /// No snapshot yet, or one this build cannot read.
    Missing,
    /// A snapshot built from a different (usually older) serverlist.json.
    Stale,
    /// The snapshot cannot be judged because Proton's file is gone.
    SourceMissing,
}

// ---------------------------------------------------------------- source parsing

#[derive(Deserialize)]
struct RawList {
    #[serde(rename = "LogicalServers")]
    logical_servers: Vec<RawLogical>,
    #[serde(rename = "MaxTier")]
    max_tier: Option<u8>,
    /// Unix-epoch **float** (§1.3).
    #[serde(rename = "ExpirationTime")]
    expiration_time: Option<f64>,
    #[serde(rename = "LoadsExpirationTime")]
    loads_expiration_time: Option<f64>,
}

#[derive(Deserialize)]
struct RawLogical {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ExitCountry")]
    exit_country: String,
    #[serde(rename = "Tier")]
    tier: u8,
    #[serde(rename = "Features")]
    features: u32,
    #[serde(rename = "City")]
    city: Option<String>,
    #[serde(rename = "Score")]
    score: f64,
    #[serde(rename = "Load")]
    load: u8,
    #[serde(rename = "Status")]
    status: u8,
    #[serde(rename = "Location")]
    location: Option<RawLocation>,
    #[serde(rename = "Servers", default)]
    servers: Vec<RawPhysical>,
}

#[derive(Deserialize)]
struct RawLocation {
    #[serde(rename = "Lat")]
    lat: Option<f64>,
    #[serde(rename = "Long")]
    long: Option<f64>,
}

#[derive(Deserialize)]
struct RawPhysical {
    #[serde(rename = "EntryIP")]
    entry_ip: Option<String>,
    #[serde(rename = "ExitIP")]
    exit_ip: Option<String>,
}

/// ISO 3166-1 alpha-2 → the name `country_name` reports, plus the two codes Proton
/// uses that ISO does not assign (`UK` for the United Kingdom, `XK` for Kosovo).
/// Generated from `/usr/share/iso-codes/json/iso_3166-1.json` (iso-codes 4.19).
const COUNTRY_NAMES: &[(&str, &str)] = &[
    ("AD", "Andorra"),
    ("AE", "United Arab Emirates"),
    ("AF", "Afghanistan"),
    ("AG", "Antigua and Barbuda"),
    ("AI", "Anguilla"),
    ("AL", "Albania"),
    ("AM", "Armenia"),
    ("AO", "Angola"),
    ("AQ", "Antarctica"),
    ("AR", "Argentina"),
    ("AS", "American Samoa"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("AW", "Aruba"),
    ("AX", "Åland Islands"),
    ("AZ", "Azerbaijan"),
    ("BA", "Bosnia and Herzegovina"),
    ("BB", "Barbados"),
    ("BD", "Bangladesh"),
    ("BE", "Belgium"),
    ("BF", "Burkina Faso"),
    ("BG", "Bulgaria"),
    ("BH", "Bahrain"),
    ("BI", "Burundi"),
    ("BJ", "Benin"),
    ("BL", "Saint Barthélemy"),
    ("BM", "Bermuda"),
    ("BN", "Brunei Darussalam"),
    ("BO", "Bolivia"),
    ("BQ", "Bonaire"),
    ("BR", "Brazil"),
    ("BS", "Bahamas"),
    ("BT", "Bhutan"),
    ("BV", "Bouvet Island"),
    ("BW", "Botswana"),
    ("BY", "Belarus"),
    ("BZ", "Belize"),
    ("CA", "Canada"),
    ("CC", "Cocos (Keeling) Islands"),
    ("CD", "Congo"),
    ("CF", "Central African Republic"),
    ("CG", "Congo"),
    ("CH", "Switzerland"),
    ("CI", "Côte d'Ivoire"),
    ("CK", "Cook Islands"),
    ("CL", "Chile"),
    ("CM", "Cameroon"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("CR", "Costa Rica"),
    ("CU", "Cuba"),
    ("CV", "Cabo Verde"),
    ("CW", "Curaçao"),
    ("CX", "Christmas Island"),
    ("CY", "Cyprus"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("DJ", "Djibouti"),
    ("DK", "Denmark"),
    ("DM", "Dominica"),
    ("DO", "Dominican Republic"),
    ("DZ", "Algeria"),
    ("EC", "Ecuador"),
    ("EE", "Estonia"),
    ("EG", "Egypt"),
    ("EH", "Western Sahara"),
    ("ER", "Eritrea"),
    ("ES", "Spain"),
    ("ET", "Ethiopia"),
    ("FI", "Finland"),
    ("FJ", "Fiji"),
    ("FK", "Falkland Islands (Malvinas)"),
    ("FM", "Micronesia"),
    ("FO", "Faroe Islands"),
    ("FR", "France"),
    ("GA", "Gabon"),
    ("GB", "United Kingdom"),
    ("GD", "Grenada"),
    ("GE", "Georgia"),
    ("GF", "French Guiana"),
    ("GG", "Guernsey"),
    ("GH", "Ghana"),
    ("GI", "Gibraltar"),
    ("GL", "Greenland"),
    ("GM", "Gambia"),
    ("GN", "Guinea"),
    ("GP", "Guadeloupe"),
    ("GQ", "Equatorial Guinea"),
    ("GR", "Greece"),
    ("GS", "South Georgia and the South Sandwich Islands"),
    ("GT", "Guatemala"),
    ("GU", "Guam"),
    ("GW", "Guinea-Bissau"),
    ("GY", "Guyana"),
    ("HK", "Hong Kong"),
    ("HM", "Heard Island and McDonald Islands"),
    ("HN", "Honduras"),
    ("HR", "Croatia"),
    ("HT", "Haiti"),
    ("HU", "Hungary"),
    ("ID", "Indonesia"),
    ("IE", "Ireland"),
    ("IL", "Israel"),
    ("IM", "Isle of Man"),
    ("IN", "India"),
    ("IO", "British Indian Ocean Territory"),
    ("IQ", "Iraq"),
    ("IR", "Iran"),
    ("IS", "Iceland"),
    ("IT", "Italy"),
    ("JE", "Jersey"),
    ("JM", "Jamaica"),
    ("JO", "Jordan"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("KG", "Kyrgyzstan"),
    ("KH", "Cambodia"),
    ("KI", "Kiribati"),
    ("KM", "Comoros"),
    ("KN", "Saint Kitts and Nevis"),
    ("KP", "North Korea"),
    ("KR", "South Korea"),
    ("KW", "Kuwait"),
    ("KY", "Cayman Islands"),
    ("KZ", "Kazakhstan"),
    ("LA", "Laos"),
    ("LB", "Lebanon"),
    ("LC", "Saint Lucia"),
    ("LI", "Liechtenstein"),
    ("LK", "Sri Lanka"),
    ("LR", "Liberia"),
    ("LS", "Lesotho"),
    ("LT", "Lithuania"),
    ("LU", "Luxembourg"),
    ("LV", "Latvia"),
    ("LY", "Libya"),
    ("MA", "Morocco"),
    ("MC", "Monaco"),
    ("MD", "Moldova"),
    ("ME", "Montenegro"),
    ("MF", "Saint Martin (French part)"),
    ("MG", "Madagascar"),
    ("MH", "Marshall Islands"),
    ("MK", "North Macedonia"),
    ("ML", "Mali"),
    ("MM", "Myanmar"),
    ("MN", "Mongolia"),
    ("MO", "Macao"),
    ("MP", "Northern Mariana Islands"),
    ("MQ", "Martinique"),
    ("MR", "Mauritania"),
    ("MS", "Montserrat"),
    ("MT", "Malta"),
    ("MU", "Mauritius"),
    ("MV", "Maldives"),
    ("MW", "Malawi"),
    ("MX", "Mexico"),
    ("MY", "Malaysia"),
    ("MZ", "Mozambique"),
    ("NA", "Namibia"),
    ("NC", "New Caledonia"),
    ("NE", "Niger"),
    ("NF", "Norfolk Island"),
    ("NG", "Nigeria"),
    ("NI", "Nicaragua"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NP", "Nepal"),
    ("NR", "Nauru"),
    ("NU", "Niue"),
    ("NZ", "New Zealand"),
    ("OM", "Oman"),
    ("PA", "Panama"),
    ("PE", "Peru"),
    ("PF", "French Polynesia"),
    ("PG", "Papua New Guinea"),
    ("PH", "Philippines"),
    ("PK", "Pakistan"),
    ("PL", "Poland"),
    ("PM", "Saint Pierre and Miquelon"),
    ("PN", "Pitcairn"),
    ("PR", "Puerto Rico"),
    ("PS", "Palestine, State of"),
    ("PT", "Portugal"),
    ("PW", "Palau"),
    ("PY", "Paraguay"),
    ("QA", "Qatar"),
    ("RE", "Réunion"),
    ("RO", "Romania"),
    ("RS", "Serbia"),
    ("RU", "Russian Federation"),
    ("RW", "Rwanda"),
    ("SA", "Saudi Arabia"),
    ("SB", "Solomon Islands"),
    ("SC", "Seychelles"),
    ("SD", "Sudan"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("SH", "Saint Helena"),
    ("SI", "Slovenia"),
    ("SJ", "Svalbard and Jan Mayen"),
    ("SK", "Slovakia"),
    ("SL", "Sierra Leone"),
    ("SM", "San Marino"),
    ("SN", "Senegal"),
    ("SO", "Somalia"),
    ("SR", "Suriname"),
    ("SS", "South Sudan"),
    ("ST", "Sao Tome and Principe"),
    ("SV", "El Salvador"),
    ("SX", "Sint Maarten (Dutch part)"),
    ("SY", "Syria"),
    ("SZ", "Eswatini"),
    ("TC", "Turks and Caicos Islands"),
    ("TD", "Chad"),
    ("TF", "French Southern Territories"),
    ("TG", "Togo"),
    ("TH", "Thailand"),
    ("TJ", "Tajikistan"),
    ("TK", "Tokelau"),
    ("TL", "Timor-Leste"),
    ("TM", "Turkmenistan"),
    ("TN", "Tunisia"),
    ("TO", "Tonga"),
    ("TR", "Türkiye"),
    ("TT", "Trinidad and Tobago"),
    ("TV", "Tuvalu"),
    ("TW", "Taiwan"),
    ("TZ", "Tanzania"),
    ("UA", "Ukraine"),
    ("UG", "Uganda"),
    ("UK", "United Kingdom"),
    ("UM", "United States Minor Outlying Islands"),
    ("US", "United States"),
    ("UY", "Uruguay"),
    ("UZ", "Uzbekistan"),
    ("VA", "Holy See (Vatican City State)"),
    ("VC", "Saint Vincent and the Grenadines"),
    ("VE", "Venezuela"),
    ("VG", "Virgin Islands, British"),
    ("VI", "Virgin Islands, U.S."),
    ("VN", "Vietnam"),
    ("VU", "Vanuatu"),
    ("WF", "Wallis and Futuna"),
    ("WS", "Samoa"),
    ("XK", "Kosovo"),
    ("YE", "Yemen"),
    ("YT", "Mayotte"),
    ("ZA", "South Africa"),
    ("ZM", "Zambia"),
    ("ZW", "Zimbabwe"),
];

/// The English name of a two-letter country code, when one is known.
pub fn country_name(code: &str) -> Option<&'static str> {
    COUNTRY_NAMES
        .binary_search_by(|(c, _)| (*c).cmp(code))
        .ok()
        .map(|i| COUNTRY_NAMES[i].1)
}

/// Turn Proton's parsed JSON into the rows the snapshot stores.
fn rows_from_raw(raw: RawList) -> (Vec<ServerRow>, Vec<CountryRow>, Vec<CityRow>) {
    let mut servers: Vec<ServerRow> = Vec::with_capacity(raw.logical_servers.len());
    for l in raw.logical_servers {
        let (lat, lon) = match &l.location {
            Some(loc) => (loc.lat, loc.long),
            None => (None, None),
        };
        let first = l.servers.first();
        servers.push(ServerRow {
            country_name: country_name(&l.exit_country).map(str::to_string),
            country: l.exit_country,
            city: l.city.filter(|c| !c.is_empty()),
            lat,
            lon,
            load: l.load,
            score: l.score,
            tier: l.tier,
            features: features_from_raw(l.features),
            features_raw: l.features,
            entry_ip: first.and_then(|p| p.entry_ip.clone()),
            exit_ip: first.and_then(|p| p.exit_ip.clone()),
            status: l.status,
            name: l.name,
        });
    }

    // Countries and cities are folded in one pass over the rows, in a BTreeMap so the
    // output order is deterministic between rebuilds.
    struct Agg {
        servers: u32,
        min_load: Option<u8>,
        lat: Option<f64>,
        lon: Option<f64>,
        has_free: bool,
    }
    impl Agg {
        fn add(&mut self, row: &ServerRow) {
            self.servers += 1;
            self.has_free |= row.tier == 0;
            if self.min_load.is_none_or(|m| row.load < m) {
                self.min_load = Some(row.load);
                // The coordinates of the country's or city's least-loaded server:
                // one real point, never an average of several.
                self.lat = row.lat;
                self.lon = row.lon;
            }
        }
        fn new() -> Agg {
            Agg {
                servers: 0,
                min_load: None,
                lat: None,
                lon: None,
                has_free: false,
            }
        }
    }

    let mut by_country: BTreeMap<String, Agg> = BTreeMap::new();
    let mut by_city: BTreeMap<(String, String), Agg> = BTreeMap::new();
    for row in &servers {
        by_country
            .entry(row.country.clone())
            .or_insert_with(Agg::new)
            .add(row);
        if let Some(city) = &row.city {
            by_city
                .entry((row.country.clone(), city.clone()))
                .or_insert_with(Agg::new)
                .add(row);
        }
    }

    let cities: Vec<CityRow> = by_city
        .into_iter()
        .map(|((country, city), a)| CityRow {
            city,
            country,
            servers: a.servers,
            min_load: a.min_load,
            lat: a.lat,
            lon: a.lon,
            has_free: a.has_free,
        })
        .collect();

    let countries: Vec<CountryRow> = by_country
        .into_iter()
        .map(|(country, a)| CountryRow {
            cities: cities.iter().filter(|c| c.country == country).count() as u32,
            country_name: country_name(&country).map(str::to_string),
            country,
            servers: a.servers,
            min_load: a.min_load,
            lat: a.lat,
            lon: a.lon,
            has_free: a.has_free,
        })
        .collect();

    (servers, countries, cities)
}

// ---------------------------------------------------------------- snapshot I/O

fn source_facts(ctx: &Context) -> Option<(i128, u64)> {
    let meta = std::fs::metadata(ctx.paths.serverlist()).ok()?;
    let mtime = meta.modified().ok()?;
    let nanos = match mtime.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    };
    Some((nanos, meta.len()))
}

/// Read just the header and the meta block: one 4 KiB read, never the payload.
fn read_head(ctx: &Context) -> Option<(IndexHeader, IndexMeta)> {
    let mut file = std::fs::File::open(ctx.paths.index_file()).ok()?;
    let mut buf = vec![0u8; 4096];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    let (header, rest) = postcard::take_from_bytes::<IndexHeader>(&buf).ok()?;
    if header.magic != INDEX_MAGIC || header.format_version != INDEX_FORMAT_VERSION {
        return None;
    }
    let (meta, _) = postcard::take_from_bytes::<IndexMeta>(rest).ok()?;
    Some((header, meta))
}

/// A snapshot this build can read: the file's bytes and the offset at which the
/// country block begins (the header and meta blocks are already validated).
struct Snapshot {
    bytes: Vec<u8>,
    offset: usize,
}

/// The bytes of a snapshot whose header this build understands, with the offset at
/// which the country block begins. `Ok(None)` = no usable snapshot (stale, not an
/// error); `Err(Error::Index)` = a header-matching file this build cannot read.
fn read_bytes(ctx: &Context) -> Result<Option<Snapshot>, Error> {
    let path = ctx.paths.index_file();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::index(format!("cannot read {}: {e}", path.display()))
                .with_hint("fproton index --rebuild"))
        }
    };
    let Ok((header, rest)) = postcard::take_from_bytes::<IndexHeader>(&bytes) else {
        return Ok(None); // not one of ours: stale, not corrupt
    };
    if header.magic != INDEX_MAGIC || header.format_version != INDEX_FORMAT_VERSION {
        return Ok(None);
    }
    let Ok((_meta, rest)) = postcard::take_from_bytes::<IndexMeta>(rest) else {
        return Ok(None);
    };
    let offset = bytes.len() - rest.len();
    Ok(Some(Snapshot { bytes, offset }))
}

/// A header-matching snapshot that will not deserialise is exit 10 (§4).
fn broken(ctx: &Context, what: &str, detail: impl std::fmt::Display) -> Error {
    Error::index(format!(
        "{} has an unreadable {what} block: {detail}",
        ctx.paths.index_file().display()
    ))
    .with_hint("fproton index --rebuild")
}

/// How the snapshot relates to Proton's file right now. Never rebuilds, never parses
/// the source: `status` calls this.
pub fn freshness(ctx: &Context) -> Freshness {
    let Some((nanos, len)) = source_facts(ctx) else {
        return Freshness::SourceMissing;
    };
    match read_head(ctx) {
        None => Freshness::Missing,
        Some((h, _)) if h.source_mtime_unix_nanos == nanos && h.source_len == len => {
            Freshness::Fresh
        }
        Some(_) => Freshness::Stale,
    }
}

/// Make sure the snapshot on disk is fresh, rebuilding it when it is not (§4).
fn ensure_fresh(ctx: &Context) -> Result<(), Error> {
    if freshness(ctx) != Freshness::Fresh {
        rebuild(ctx, &CancelToken::new())?;
    }
    Ok(())
}

/// A query with its case folding done once, so the hot loop only compares.
struct Prepared {
    country: Option<String>,
    city: Option<String>,
    search: Option<String>,
    feature: Option<Feature>,
    tier: Option<Tier>,
}

impl Prepared {
    fn new(q: &Query) -> Prepared {
        Prepared {
            country: q.country.as_ref().map(|c| c.trim().to_ascii_uppercase()),
            city: q.city.as_ref().map(|c| c.trim().to_lowercase()),
            search: q.search.as_ref().map(|s| s.trim().to_lowercase()),
            feature: q.feature,
            tier: q.tier,
        }
    }

    /// The one predicate both the borrowed and the owned path use.
    fn matches(
        &self,
        name: &str,
        country: &str,
        city: Option<&str>,
        features_raw: u32,
        tier: u8,
    ) -> bool {
        if self.country.as_ref().is_some_and(|c| c != country) {
            return false;
        }
        if let Some(want) = &self.city {
            if !city.is_some_and(|c| eq_fold(c, want)) {
                return false;
            }
        }
        if self.feature.is_some_and(|f| features_raw & f.bit() == 0) {
            return false;
        }
        if self.tier.is_some_and(|t| t.level() != tier) {
            return false;
        }
        if let Some(text) = &self.search {
            let hit = contains_fold(name, text)
                || contains_fold(country, text)
                || country_name(country).is_some_and(|n| contains_fold(n, text))
                || city.is_some_and(|c| contains_fold(c, text));
            if !hit {
                return false;
            }
        }
        true
    }
}

/// Case-insensitive equality against an already-lowercased needle, without allocating
/// on the ASCII path (every Proton country code and most city names are ASCII).
fn eq_fold(haystack: &str, needle_lower: &str) -> bool {
    if haystack.is_ascii() {
        haystack.eq_ignore_ascii_case(needle_lower)
    } else {
        haystack.to_lowercase() == needle_lower
    }
}

/// Case-insensitive substring against an already-lowercased needle. The allocating
/// path is taken only for a non-ASCII name (`Zürich`), not for all 18 221 rows.
fn contains_fold(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle_lower.is_ascii() {
        let (h, n) = (haystack.as_bytes(), needle_lower.as_bytes());
        n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        haystack.to_lowercase().contains(needle_lower)
    }
}

fn order(rows: &mut [ServerRow], q: &Query) {
    match q.sort {
        Sort::Load => rows.sort_by(|a, b| {
            a.load
                .cmp(&b.load)
                .then_with(|| a.score.total_cmp(&b.score))
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        Sort::Score => rows.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        Sort::Name => rows.sort_by(|a, b| natural_cmp(&a.name, &b.name)),
    }
}

// ---------------------------------------------------------------- frozen API

/// Matching servers, ordered by `q.sort`. Rebuilds a stale snapshot first.
///
/// The server block is decoded **borrowing** from the file's bytes, and only the rows
/// that match are turned into owned [`ServerRow`]s — 18 221 rows are scanned without
/// 18 221 allocations.
pub fn servers(ctx: &Context, q: &Query) -> Result<Vec<ServerRow>, Error> {
    ensure_fresh(ctx)?;
    let Some(snap) = read_bytes(ctx)? else {
        return Err(Error::index("the index snapshot vanished as it was read")
            .with_hint("fproton index --rebuild"));
    };
    let (_, rest) = skip_blocks(ctx, &snap.bytes[snap.offset..])?;
    let stored: Vec<StoredServerRef> =
        postcard::from_bytes(rest).map_err(|e| broken(ctx, "server", e))?;

    let prepared = Prepared::new(q);
    let mut rows: Vec<ServerRow> = stored
        .iter()
        .filter(|s| prepared.matches(s.name, s.country, s.city, s.features_raw, s.tier))
        .map(StoredServerRef::to_row)
        .collect();
    order(&mut rows, q);
    if let Some(limit) = q.limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

/// The two small blocks that precede the server block.
type Blocks = (Vec<CountryRow>, Vec<CityRow>);

/// Decode the two small blocks that precede the servers, returning them and the rest.
fn skip_blocks<'a>(ctx: &Context, rest: &'a [u8]) -> Result<(Blocks, &'a [u8]), Error> {
    let (countries, rest) = postcard::take_from_bytes::<Vec<CountryRow>>(rest)
        .map_err(|e| broken(ctx, "country", e))?;
    let (cities, rest) =
        postcard::take_from_bytes::<Vec<CityRow>>(rest).map_err(|e| broken(ctx, "city", e))?;
    Ok(((countries, cities), rest))
}

/// The pure half of [`servers`]: filtering and ordering, with no I/O. Kept for tests
/// and for callers that already hold rows (the TUI re-filters in memory).
pub fn filter_servers(rows: Vec<ServerRow>, q: &Query) -> Vec<ServerRow> {
    let prepared = Prepared::new(q);
    let mut rows: Vec<ServerRow> = rows
        .into_iter()
        .filter(|r| {
            prepared.matches(
                &r.name,
                &r.country,
                r.city.as_deref(),
                r.features_raw,
                r.tier,
            )
        })
        .collect();

    match q.sort {
        Sort::Load => rows.sort_by(|a, b| {
            a.load
                .cmp(&b.load)
                .then_with(|| a.score.total_cmp(&b.score))
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        Sort::Score => rows.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| natural_cmp(&a.name, &b.name))
        }),
        Sort::Name => rows.sort_by(|a, b| natural_cmp(&a.name, &b.name)),
    }
    if let Some(limit) = q.limit {
        rows.truncate(limit);
    }
    rows
}

/// `CH#9` before `CH#461` before `CH#1022`: compare the `#` number numerically.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn split(name: &str) -> (&str, u32, &str) {
        match name.find('#') {
            Some(i) => {
                let rest = &name[i + 1..];
                let digits: usize = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
                (
                    &name[..i],
                    rest[..digits].parse().unwrap_or(0),
                    &rest[digits..],
                )
            }
            None => (name, 0, ""),
        }
    }
    let (ap, an, asuf) = split(a);
    let (bp, bn, bsuf) = split(b);
    ap.cmp(bp).then(an.cmp(&bn)).then(asuf.cmp(bsuf))
}

/// The two small blocks, without touching the server block behind them.
fn small_blocks(ctx: &Context) -> Result<(Vec<CountryRow>, Vec<CityRow>), Error> {
    ensure_fresh(ctx)?;
    let Some(snap) = read_bytes(ctx)? else {
        return Err(Error::index("the index snapshot vanished as it was read")
            .with_hint("fproton index --rebuild"));
    };
    let (blocks, _) = skip_blocks(ctx, &snap.bytes[snap.offset..])?;
    Ok(blocks)
}

/// Every country in the snapshot, with counts and the lowest load.
pub fn countries(ctx: &Context) -> Result<Vec<CountryRow>, Error> {
    Ok(small_blocks(ctx)?.0)
}

/// Every city of one country (all cities when `country` is empty).
pub fn cities(ctx: &Context, country: &str) -> Result<Vec<CityRow>, Error> {
    let want = country.trim().to_ascii_uppercase();
    let mut rows = small_blocks(ctx)?.1;
    if !want.is_empty() {
        rows.retain(|c| c.country == want);
    }
    Ok(rows)
}

/// Rebuild the snapshot from `serverlist.json` (tmp file + `rename(2)`; no lock).
pub fn rebuild(ctx: &Context, cancel: &CancelToken) -> Result<IndexStats, Error> {
    let started = std::time::Instant::now();
    let source = ctx.paths.serverlist();
    let (mtime_nanos, source_len) = source_facts(ctx).ok_or_else(|| {
        Error::index(format!(
            "Proton's server list is missing at {}",
            source.display()
        ))
        .with_hint("protonvpn connect   # only the official CLI refreshes that file")
    })?;

    let bytes = std::fs::read(&source).map_err(|e| {
        Error::index(format!("cannot read {}: {e}", source.display()))
            .with_hint("fproton doctor --only cache")
    })?;
    if cancel.is_cancelled() {
        return Err(Error::Interrupted);
    }
    let raw: RawList = serde_json::from_slice(&bytes).map_err(|e| {
        Error::index(format!(
            "{} is not Proton's server list: {e}",
            source.display()
        ))
        .with_hint("fproton doctor --only cache")
    })?;
    let max_tier = raw.max_tier;
    let serverlist_expires_unix = raw.expiration_time.map(|t| t as i64);
    let loads_expire_unix = raw.loads_expiration_time.map(|t| t as i64);
    if cancel.is_cancelled() {
        return Err(Error::Interrupted);
    }

    let (servers, countries, cities) = rows_from_raw(raw);
    let built_at = crate::now_rfc3339();
    let header = IndexHeader {
        magic: INDEX_MAGIC,
        format_version: INDEX_FORMAT_VERSION,
        source_path: source.display().to_string(),
        source_mtime_unix_nanos: mtime_nanos,
        source_len,
        built_at: built_at.clone(),
        server_count: servers.len() as u32,
        max_tier,
    };
    let meta = IndexMeta {
        serverlist_expires_unix,
        loads_expire_unix,
        country_count: countries.len() as u32,
        city_count: cities.len() as u32,
    };
    let stats = IndexStats {
        servers: header.server_count,
        countries: meta.country_count,
        cities: meta.city_count,
        max_tier,
        source_path: header.source_path.clone(),
        source_len,
        source_mtime: crate::rfc3339_from_unix((mtime_nanos / 1_000_000_000) as i64),
        built_at,
        path: ctx.paths.index_file().display().to_string(),
        bytes: 0,
        duration_ms: 0,
        rebuilt: true,
    };
    // Small blocks first, so `countries`/`cities` never decode the server block.
    let stored: Vec<StoredServer> = servers.iter().map(StoredServer::from).collect();
    let mut out = postcard::to_stdvec(&header)
        .map_err(|e| Error::index(format!("cannot encode the index header: {e}")))?;
    for (what, block) in [
        ("meta", postcard::to_stdvec(&meta)),
        ("country", postcard::to_stdvec(&countries)),
        ("city", postcard::to_stdvec(&cities)),
        ("server", postcard::to_stdvec(&stored)),
    ] {
        out.extend(
            block
                .map_err(|e| Error::index(format!("cannot encode the index {what} block: {e}")))?,
        );
    }

    ctx.paths.ensure_dirs()?;
    let path = ctx.paths.index_file();
    let tmp = path.with_extension(format!("bin.tmp.{}", std::process::id()));
    std::fs::write(&tmp, &out)
        .map_err(|e| Error::index(format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::index(format!("cannot replace {}: {e}", path.display()))
    })?;

    Ok(IndexStats {
        bytes: out.len() as u64,
        duration_ms: started.elapsed().as_millis() as u64,
        ..stats
    })
}

/// Header facts only; never rebuilds, never parses the 24 MB source (`status` uses this).
pub fn info(ctx: &Context) -> Result<IndexInfo, Error> {
    let Some((header, meta)) = read_head(ctx) else {
        return Ok(IndexInfo::unknown());
    };
    let now = crate::state::unix_now() as i64;
    Ok(IndexInfo {
        servers: Some(header.server_count),
        built_from_mtime: crate::rfc3339_from_unix(
            (header.source_mtime_unix_nanos / 1_000_000_000) as i64,
        ),
        serverlist_expires_at: meta
            .serverlist_expires_unix
            .and_then(crate::rfc3339_from_unix),
        loads_expired: meta.loads_expire_unix.map(|t| t <= now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = include_str!("../../../tests/fixtures/serverlist-small.json");

    fn small_rows() -> (Vec<ServerRow>, Vec<CountryRow>, Vec<CityRow>) {
        let raw: RawList = serde_json::from_str(SMALL).expect("the fixture parses");
        rows_from_raw(raw)
    }

    fn test_ctx(name: &str) -> (Context, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "fproton-index-test-{name}-{}",
            crate::state::unix_now()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = Context::for_test(&dir).expect("test context");
        (ctx, dir)
    }

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

    #[test]
    fn the_country_table_is_sorted_and_knows_protons_own_codes() {
        assert!(
            COUNTRY_NAMES.windows(2).all(|w| w[0].0 < w[1].0),
            "the table is binary-searched, so it must stay sorted"
        );
        assert_eq!(country_name("CH"), Some("Switzerland"));
        assert_eq!(country_name("US"), Some("United States"));
        // Proton ships UK and XK, which ISO 3166-1 does not assign.
        assert_eq!(country_name("UK"), Some("United Kingdom"));
        assert_eq!(country_name("XK"), Some("Kosovo"));
        assert_eq!(country_name("ZZ"), None);
    }

    #[test]
    fn the_fixture_builds_the_real_ch461_row() {
        let (servers, _, _) = small_rows();
        assert_eq!(servers.len(), 38, "the fixture holds 38 logical servers");
        let ch = servers.iter().find(|s| s.name == "CH#461").expect("CH#461");
        // Every literal here is the real row in tests/fixtures/serverlist-small.json.
        assert_eq!(ch.country, "CH");
        assert_eq!(ch.country_name.as_deref(), Some("Switzerland"));
        assert_eq!(ch.city.as_deref(), Some("Zurich"));
        assert_eq!(ch.load, 63);
        assert_eq!(ch.score, 1.8623468195);
        assert_eq!(ch.tier, 2);
        assert_eq!(ch.features_raw, 28);
        assert_eq!(ch.features, vec!["p2p", "streaming", "ipv6"]);
        assert_eq!(ch.lat, Some(47.37));
        assert_eq!(ch.lon, Some(8.54));
        assert_eq!(ch.entry_ip.as_deref(), Some("146.70.226.194"));
        assert_eq!(ch.exit_ip.as_deref(), Some("146.70.226.194"));
        assert_eq!(ch.status, 1);
    }

    #[test]
    fn every_fixture_name_is_a_valid_connect_target() {
        let (servers, _, _) = small_rows();
        for s in &servers {
            assert!(
                crate::engine::is_server_name(&s.name),
                "{} must match the §2 server-name regex",
                s.name
            );
        }
    }

    #[test]
    fn secure_core_rows_are_filed_under_their_exit_country() {
        let (servers, _, _) = small_rows();
        let is_de = servers
            .iter()
            .find(|s| s.name == "IS-DE#1")
            .expect("IS-DE#1");
        assert_eq!(is_de.country, "DE", "the user's exit is Germany");
        assert_eq!(is_de.features_raw & Feature::SecureCore.bit(), 1);
    }

    #[test]
    fn country_and_city_rows_aggregate_the_fixture() {
        let (servers, countries, cities) = small_rows();
        let ch = countries.iter().find(|c| c.country == "CH").expect("CH");
        let ch_servers = servers.iter().filter(|s| s.country == "CH").count() as u32;
        assert_eq!(ch.servers, ch_servers);
        assert_eq!(
            ch.min_load,
            servers
                .iter()
                .filter(|s| s.country == "CH")
                .map(|s| s.load)
                .min()
        );
        assert_eq!(
            ch.cities,
            cities.iter().filter(|c| c.country == "CH").count() as u32
        );
        assert!(countries.windows(2).all(|w| w[0].country <= w[1].country));
        let free = countries.iter().find(|c| c.has_free);
        assert!(free.is_some(), "the fixture carries an NL-FREE server");
    }

    #[test]
    fn filters_and_sorting_do_what_the_flags_say() {
        let (servers, _, _) = small_rows();
        let q = Query {
            country: Some("ch".into()),
            sort: Sort::Load,
            ..Default::default()
        };
        let rows = filter_servers(servers.clone(), &q);
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r.country == "CH"));
        assert!(rows.windows(2).all(|w| w[0].load <= w[1].load));

        let tor = filter_servers(
            servers.clone(),
            &Query {
                feature: Some(Feature::Tor),
                ..Default::default()
            },
        );
        assert!(tor.iter().all(|r| r.features.contains(&"tor".to_string())));

        let free = filter_servers(
            servers.clone(),
            &Query {
                tier: Some(Tier::Free),
                ..Default::default()
            },
        );
        assert!(free.iter().all(|r| r.tier == 0));

        let searched = filter_servers(
            servers.clone(),
            &Query {
                search: Some("zurich".into()),
                ..Default::default()
            },
        );
        assert!(searched.iter().all(|r| r.city.as_deref() == Some("Zurich")));

        let limited = filter_servers(
            servers,
            &Query {
                limit: Some(2),
                ..Default::default()
            },
        );
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn server_names_sort_numerically_not_lexicographically() {
        let mut names = ["CH#1022", "CH#9", "CH#461"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["CH#9", "CH#461", "CH#1022"]);
        assert_eq!(
            natural_cmp("US-CO#21-TOR", "US-CO#3-TOR"),
            std::cmp::Ordering::Greater
        );
    }

    /// A full round trip through the real snapshot format, built from the fixture.
    #[test]
    fn a_snapshot_round_trips_and_a_source_change_makes_it_stale() {
        let (ctx, dir) = test_ctx("roundtrip");
        let mut ctx = ctx;
        let source = dir.join("serverlist.json");
        std::fs::write(&source, SMALL).unwrap();
        ctx.paths.proton_cache_dir = dir.clone();
        assert_eq!(ctx.paths.serverlist(), source);

        assert_eq!(freshness(&ctx), Freshness::Missing);
        let stats = rebuild(&ctx, &CancelToken::new()).expect("rebuild");
        assert_eq!(stats.servers, 38);
        assert_eq!(stats.max_tier, Some(2));
        assert!(stats.bytes > 0);
        assert_eq!(freshness(&ctx), Freshness::Fresh);

        let info = info(&ctx).expect("info");
        assert_eq!(info.servers, Some(38));
        assert!(info.built_from_mtime.is_some());
        assert!(info.serverlist_expires_at.is_some());
        assert_eq!(
            info.loads_expired,
            Some(true),
            "the fixture expired in 2026"
        );

        let rows = servers(
            &ctx,
            &Query {
                country: Some("CH".into()),
                ..Default::default()
            },
        )
        .expect("servers");
        assert!(rows.iter().any(|r| r.name == "CH#461"));
        assert!(!countries(&ctx).unwrap().is_empty());
        assert!(cities(&ctx, "CH")
            .unwrap()
            .iter()
            .all(|c| c.country == "CH"));

        // Touching the source changes its mtime: the snapshot is stale, not corrupt.
        std::fs::write(&source, SMALL).unwrap();
        assert_eq!(freshness(&ctx), Freshness::Stale);
        // …and any query rebuilds it silently.
        assert!(!servers(&ctx, &Query::default()).unwrap().is_empty());
        assert_eq!(freshness(&ctx), Freshness::Fresh);

        // A snapshot from another format version is ignored, never an error.
        let mut bytes = std::fs::read(ctx.paths.index_file()).unwrap();
        bytes[4] = 99;
        std::fs::write(ctx.paths.index_file(), &bytes).unwrap();
        assert_eq!(freshness(&ctx), Freshness::Missing);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_serverlist_is_exit_ten_not_a_panic() {
        let (mut ctx, dir) = test_ctx("nosource");
        ctx.paths.proton_cache_dir = dir.join("absent");
        assert_eq!(freshness(&ctx), Freshness::SourceMissing);
        let e = rebuild(&ctx, &CancelToken::new()).unwrap_err();
        assert_eq!(e.exit_code(), 10);
        assert!(info(&ctx).unwrap().servers.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
