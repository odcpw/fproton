//! Output policy and rendering (DESIGN.md §2 "JSON contract").
//!
//! * `--json`, or a non-TTY stdout, means one JSON object on stdout and nothing else.
//! * stdout is data; progress and diagnostics go to stderr.
//! * Colour is off when stdout is not a TTY, with `--no-color`, or when `NO_COLOR`
//!   or `CI` is set.
//!
//! Core never prints: every byte a user sees is written here.

use fproton_core::index::{CityRow, CountryRow, IndexStats, ServerRow};
use fproton_core::state::HistoryEntry;
use fproton_core::status::{Severity, Status};
use fproton_core::Error;
use unicode_width::UnicodeWidthStr;

/// True when stdout is a terminal.
pub fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(1) == 1 }
}

/// ANSI attributes, used only through [`Out::paint`].
#[derive(Clone, Copy)]
pub enum Paint {
    Dim,
    Green,
    Yellow,
    Red,
}

impl Paint {
    fn code(self) -> &'static str {
        match self {
            Paint::Dim => "\x1b[2m",
            Paint::Green => "\x1b[32m",
            Paint::Yellow => "\x1b[33m",
            Paint::Red => "\x1b[31m",
        }
    }
}

/// The resolved output mode for one invocation.
#[derive(Clone, Copy, Debug)]
pub struct Out {
    json: bool,
    color: bool,
    verbose: u8,
}

/// `--no-color` is a flag of `main.rs`, but `setup`, `doctor` and the TUI build their
/// own [`Out`] from a `Context`, which does not carry it. `Out::new` records the flag
/// here the first time it runs — always in `main` — so every later `Out` in the same
/// process honours it too.
static NO_COLOR_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Out {
    /// Apply the whole policy once, at startup.
    pub fn new(json_flag: bool, no_color: bool, verbose: u8) -> Out {
        use std::sync::atomic::Ordering;
        if no_color {
            NO_COLOR_FLAG.store(true, Ordering::SeqCst);
        }
        let tty = stdout_is_tty();
        let env_no_color =
            std::env::var_os("NO_COLOR").is_some() || std::env::var_os("CI").is_some();
        Out {
            // Non-TTY stdout implies --json for every command (§2).
            json: json_flag || !tty,
            color: tty && !NO_COLOR_FLAG.load(Ordering::SeqCst) && !env_no_color,
            verbose,
        }
    }

    /// Machine output was requested (or implied).
    pub fn json(&self) -> bool {
        self.json
    }

    /// Force JSON for a command that is always machine output (`capabilities`).
    pub fn forced_json(mut self) -> Out {
        self.json = true;
        self
    }

    pub fn paint(&self, text: &str, p: Paint) -> String {
        if self.color {
            format!("{}{text}\x1b[0m", p.code())
        } else {
            text.to_string()
        }
    }

    /// One JSON object, or the human rendering. The only place either is printed.
    pub fn emit(&self, value: serde_json::Value, human: impl FnOnce(&Out)) {
        if self.json {
            println!("{value}");
        } else {
            human(self);
        }
    }

    /// A line of data on stdout.
    pub fn line(&self, text: impl AsRef<str>) {
        println!("{}", text.as_ref());
    }

    /// A diagnostic on stderr. Never part of the data stream.
    pub fn note(&self, text: impl AsRef<str>) {
        eprintln!("{}", text.as_ref());
    }

    /// A diagnostic shown only with `-v`.
    pub fn verbose_note(&self, text: impl AsRef<str>) {
        if self.verbose > 0 {
            eprintln!("{}", text.as_ref());
        }
    }

    /// Print an error the way the caller asked for it and return its exit code.
    pub fn report(&self, e: &Error) -> i32 {
        if self.json {
            println!("{}", e.json());
        } else {
            eprintln!(
                "{}: {}",
                self.paint(&format!("fproton: error[{}]", e.code_name()), Paint::Red),
                e
            );
            eprintln!("  hint: {}", e.hint());
        }
        e.exit_code()
    }
}

// ---------------------------------------------------------------- formatters

/// `1.5 GB`, `842 kB` — decimal units, the way a transfer meter reads.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `1.2 MB/s`.
pub fn human_rate(bps: f64) -> String {
    format!("{}/s", human_bytes(bps.max(0.0) as u64))
}

/// `3d 04h`, `2h 07m`, `45s`.
pub fn human_uptime(secs: u64) -> String {
    let (d, h, m, s) = (
        secs / 86400,
        (secs % 86400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    if d > 0 {
        format!("{d}d {h:02}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Pad to `width` display columns (CJK- and emoji-safe).
pub fn pad(text: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(text);
    if w >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - w))
    }
}

fn dash(v: Option<impl ToString>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "—".to_string())
}

// ---------------------------------------------------------------- renderers

/// `fproton status` in human form.
pub fn status_human(out: &Out, s: &Status) {
    let badge = match s.state {
        fproton_core::State::Connected => out.paint("connected", Paint::Green),
        fproton_core::State::Connecting | fproton_core::State::Disconnecting => {
            out.paint(s.state.name(), Paint::Yellow)
        }
        fproton_core::State::Error => out.paint("error", Paint::Red),
        fproton_core::State::Disconnected => out.paint("disconnected", Paint::Dim),
    };
    let server = s
        .server
        .as_ref()
        .map(|sv| {
            let place = match (&sv.city, &sv.country) {
                (Some(c), Some(cc)) => format!(" ({c}, {cc})"),
                (None, Some(cc)) => format!(" ({cc})"),
                _ => String::new(),
            };
            format!("{}{place}", sv.name)
        })
        .unwrap_or_else(|| "—".into());
    out.line(format!("{badge}  {server}"));
    if s.link.up {
        out.line(format!(
            "  link       {} up {}",
            s.link.device.as_deref().unwrap_or("—"),
            human_uptime(s.link.uptime_sec)
        ));
    }
    if let Some(t) = &s.traffic {
        let rates = match (t.rx_bps, t.tx_bps) {
            (Some(rx), Some(tx)) => format!("  ({} ↓, {} ↑)", human_rate(rx), human_rate(tx)),
            _ => String::new(),
        };
        out.line(format!(
            "  traffic    ↓ {}  ↑ {}{rates}",
            human_bytes(t.rx_bytes),
            human_bytes(t.tx_bytes)
        ));
    }
    if let Some(p) = &s.pending {
        out.line(format!(
            "  pending    {} {} — {}s / {}s",
            p.action.name(),
            p.target.as_deref().unwrap_or(""),
            p.elapsed_sec,
            p.deadline_sec
        ));
    }
    match &s.config {
        Some(c) => out.line(format!(
            "  protections kill-switch {} · netshield {} · port-forwarding {}",
            dash(c.kill_switch.as_deref()),
            dash(c.netshield.as_deref()),
            dash(c.port_forwarding.as_deref())
        )),
        None => out.line(format!(
            "  protections {}",
            out.paint("unknown — the Proton CLI has not answered", Paint::Dim)
        )),
    }
    for w in &s.warnings {
        let tag = match w.severity {
            Severity::Error => out.paint("error", Paint::Red),
            Severity::Warn => out.paint("warn ", Paint::Yellow),
            Severity::Info => out.paint("info ", Paint::Dim),
        };
        out.line(format!("  {tag} {} ({})", w.text, w.id));
        out.line(format!("        hint: {}", w.hint));
    }
    if let Some(e) = &s.last_error {
        out.line(format!(
            "  {} {}",
            out.paint("last error", Paint::Red),
            e.text
        ));
        out.line(format!("        hint: {}", e.hint));
    }
    for c in &s.commands {
        out.line(format!("  next       {c}"));
    }
}

/// `fproton servers` in human form.
pub fn servers_human(out: &Out, rows: &[ServerRow], built_from: Option<&str>) {
    if rows.is_empty() {
        out.line("no servers match");
        return;
    }
    out.line(out.paint(
        &format!(
            "{} {} {} {} {} {}",
            pad("NAME", 13),
            pad("COUNTRY", 8),
            pad("CITY", 18),
            pad("LOAD", 5),
            pad("SCORE", 8),
            "FEATURES"
        ),
        Paint::Dim,
    ));
    for r in rows {
        out.line(format!(
            "{} {} {} {} {} {}",
            pad(&r.name, 13),
            pad(&r.country, 8),
            pad(r.city.as_deref().unwrap_or("—"), 18),
            pad(&format!("{}%", r.load), 5),
            pad(&format!("{:.2}", r.score), 8),
            r.features.join(",")
        ));
    }
    if let Some(m) = built_from {
        out.note(format!("index built from serverlist.json of {m}"));
    }
}

/// `fproton countries` in human form.
pub fn countries_human(out: &Out, rows: &[CountryRow]) {
    for r in rows {
        out.line(format!(
            "{} {} {} {} {}",
            pad(&r.country, 4),
            pad(r.country_name.as_deref().unwrap_or("—"), 22),
            pad(&format!("{} servers", r.servers), 14),
            pad(&format!("{} cities", r.cities), 12),
            r.min_load
                .map(|l| format!("min load {l}%"))
                .unwrap_or_default()
        ));
    }
}

/// `fproton cities` in human form.
pub fn cities_human(out: &Out, rows: &[CityRow]) {
    for r in rows {
        out.line(format!(
            "{} {} {} {}",
            pad(&r.country, 4),
            pad(&r.city, 24),
            pad(&format!("{} servers", r.servers), 14),
            r.min_load
                .map(|l| format!("min load {l}%"))
                .unwrap_or_default()
        ));
    }
}

/// `fproton history` in human form.
pub fn history_human(out: &Out, rows: &[HistoryEntry]) {
    if rows.is_empty() {
        out.line("no history yet");
        return;
    }
    for r in rows {
        let result = match r.result {
            fproton_core::state::HistoryResult::Ok => out.paint("ok", Paint::Green),
            fproton_core::state::HistoryResult::Cancelled => out.paint("cancelled", Paint::Dim),
            _ => out.paint(r.result.name(), Paint::Red),
        };
        let outcome = format!("{result} ({}s)", r.duration_sec);
        out.line(format!(
            "{} {} {} {} {outcome}",
            pad(&r.at, 26),
            pad(r.action.name(), 11),
            pad(r.target.as_deref().unwrap_or("—"), 14),
            pad(r.server.as_deref().unwrap_or("—"), 12),
        ));
    }
}

/// `fproton index` in human form.
pub fn index_human(out: &Out, s: &IndexStats) {
    out.line(format!(
        "{} servers · {} countries · {} cities{}",
        s.servers,
        s.countries,
        s.cities,
        if s.rebuilt {
            "  (rebuilt)"
        } else {
            "  (already fresh)"
        }
    ));
    out.line(format!("  snapshot   {} ({} bytes)", s.path, s.bytes));
    out.line(format!(
        "  source     {} ({})",
        s.source_path,
        s.source_mtime.as_deref().unwrap_or("—")
    ));
    out.line(format!("  built in   {} ms", s.duration_ms));
}
