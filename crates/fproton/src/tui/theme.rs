//! Terminal colors and emphasis derived from the active Omarchy theme.

use ftui::{PackedRgba, Style};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: PackedRgba,
    pub panel: PackedRgba,
    pub rule: PackedRgba,
    pub fg: PackedRgba,
    pub dim: PackedRgba,
    pub accent: PackedRgba,
    pub selection: PackedRgba,
    pub red: PackedRgba,
    pub green: PackedRgba,
    pub yellow: PackedRgba,
    pub name: String,
}

fn mix(base: PackedRgba, ink: PackedRgba, percent: u16) -> PackedRgba {
    let channel =
        |a: u8, b: u8| ((u16::from(a) * (100 - percent) + u16::from(b) * percent) / 100) as u8;
    PackedRgba::rgb(
        channel(base.r(), ink.r()),
        channel(base.g(), ink.g()),
        channel(base.b(), ink.b()),
    )
}

impl Default for Theme {
    fn default() -> Self {
        let bg = PackedRgba::rgb(0x11, 0x1a, 0x17);
        let fg = PackedRgba::rgb(0xe4, 0xe9, 0xdc);
        let accent = PackedRgba::rgb(0xaf, 0xd2, 0xa4);
        Self {
            bg,
            fg,
            accent,
            panel: mix(bg, fg, 6),
            dim: mix(bg, fg, 68),
            rule: mix(bg, fg, 27),
            selection: mix(bg, accent, 24),
            red: PackedRgba::rgb(0xa5, 0x55, 0x55),
            green: PackedRgba::rgb(0x8a, 0xa8, 0x72),
            yellow: PackedRgba::rgb(0xc9, 0xa5, 0x54),
            name: "default".into(),
        }
    }
}

pub fn colors_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_default()
        .join("omarchy/current/theme/colors.toml")
}

pub fn parse_hex(s: &str) -> Option<PackedRgba> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 || !h.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(PackedRgba::rgb(r, g, b))
}

impl Theme {
    pub fn load() -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            let c = PackedRgba::TRANSPARENT;
            return Self {
                bg: c,
                fg: c,
                panel: c,
                rule: c,
                dim: c,
                accent: c,
                selection: c,
                red: c,
                green: c,
                yellow: c,
                name: "terminal".into(),
            };
        }
        let mut t = Theme::default();
        let Ok(raw) = std::fs::read_to_string(colors_path()) else {
            return t;
        };
        let Ok(v) = raw.parse::<toml::Table>() else {
            return t;
        };
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).and_then(parse_hex);
        if let Some(c) = get("foreground") {
            t.fg = c;
        }
        if let Some(c) = get("accent") {
            t.accent = c;
        }
        if let Some(c) = get("background") {
            t.bg = c;
        }
        t.panel = mix(t.bg, t.fg, 6);
        t.dim = mix(t.bg, t.fg, 68);
        t.rule = mix(t.bg, t.fg, 27);
        t.selection = mix(t.bg, t.accent, 24);
        if let Some(c) = get("red") {
            t.red = c;
        }
        if let Some(c) = get("green") {
            t.green = c;
        }
        if let Some(c) = get("yellow") {
            t.yellow = c;
        }
        t.name = std::fs::read_link(colors_path().parent().unwrap())
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "omarchy".into());
        t
    }

    pub fn text(&self) -> Style {
        Style::new().fg(self.fg)
    }
    pub fn dimmed(&self) -> Style {
        Style::new().fg(self.dim)
    }
    pub fn accented(&self) -> Style {
        Style::new().fg(self.accent).bold()
    }
    pub fn selected(&self) -> Style {
        if self.bg == PackedRgba::TRANSPARENT {
            return Style::new().reverse().bold();
        }
        Style::new().bg(self.selection).fg(self.fg).bold()
    }
    pub fn cursor(&self) -> Style {
        if self.bg == PackedRgba::TRANSPARENT {
            return Style::new().reverse();
        }
        Style::new().bg(self.accent).fg(self.bg)
    }
    pub fn border(&self) -> Style {
        Style::new().fg(self.rule)
    }
    pub fn border_focused(&self) -> Style {
        Style::new().fg(self.accent)
    }
    pub fn title(&self) -> Style {
        Style::new().fg(self.fg).bold()
    }
    pub fn ok(&self) -> Style {
        Style::new().fg(self.green)
    }
    pub fn warn(&self) -> Style {
        Style::new().fg(self.yellow)
    }
    pub fn err(&self) -> Style {
        Style::new().fg(self.red)
    }
    pub fn key(&self) -> Style {
        Style::new().fg(self.accent).bold()
    }

    /// The §5 state badge: green connected, yellow in flight, dim disconnected, red error.
    /// Anything unrecognised is dim, never uncoloured.
    pub fn state(&self, s: &str) -> Style {
        match s {
            "connected" => Style::new().fg(self.green).bold(),
            "connecting" | "disconnecting" => Style::new().fg(self.yellow).bold(),
            "error" => Style::new().fg(self.red).bold(),
            _ => Style::new().fg(self.dim).bold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_name_has_its_own_badge_colour() {
        let t = Theme::default();
        assert_eq!(t.state("connected").fg, Some(t.green));
        assert_eq!(t.state("connecting").fg, Some(t.yellow));
        assert_eq!(t.state("disconnecting").fg, Some(t.yellow));
        assert_eq!(t.state("error").fg, Some(t.red));
        assert_eq!(t.state("disconnected").fg, Some(t.dim));
        assert_eq!(t.state("nonsense").fg, Some(t.dim));
    }

    #[test]
    fn hex_parsing_tolerates_the_hash_and_trailing_alpha() {
        assert_eq!(parse_hex("#ff8000"), Some(PackedRgba::rgb(255, 128, 0)));
        assert_eq!(parse_hex("ff8000ff"), Some(PackedRgba::rgb(255, 128, 0)));
        assert_eq!(parse_hex("#fff"), None);
    }
}
