//! Every pixel the TUI draws (DESIGN.md §7a): the three screens, the chrome, the
//! help overlay and the resize tiers. `view` returns immediately on a zero area and
//! never leaves a screen blank — each empty, error and hang state has its own text.

use ftui::layout::{Constraint, Flex, Rect, Sides};
use ftui::text::wrap::WrapMode;
use ftui::text::{Line, Span, Text};
use ftui::widgets::block::Block;
use ftui::widgets::borders::BorderType;
use ftui::widgets::paragraph::Paragraph;
use ftui::widgets::progress::{MiniBar, MiniBarColors};
use ftui::widgets::sparkline::Sparkline;
use ftui::widgets::table::{Row as TableRow, Table};
use ftui::widgets::virtualized::{RenderItem, VirtualizedList};
use ftui::widgets::{StatefulWidget, StatusItem, StatusLine, Tab, Tabs, Widget};
use ftui::{Frame, Style};
use unicode_width::UnicodeWidthStr;

use fproton_core::{ServerRow, Status};

use super::theme::Theme;
use super::{Screen, Tui, KEYS, SPINNER};

/// Which layout the terminal has room for (§7a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tier {
    /// ≥ 100 columns: two-column Status, sparkline.
    Wide,
    /// 60-99 columns: one column, no sparkline.
    Medium,
    /// < 60 columns or < 16 rows: status line and hero only.
    Narrow,
}

pub(super) fn tier_of(b: Rect) -> Tier {
    if b.height < 16 || b.width < 60 {
        Tier::Narrow
    } else if b.width >= 100 {
        Tier::Wide
    } else {
        Tier::Medium
    }
}

pub(super) fn view(m: &Tui, frame: &mut Frame) {
    let b = frame.bounds();
    if b.width == 0 || b.height == 0 {
        return;
    }
    let th = &m.theme;
    if b.width < 30 || b.height < 8 {
        // One line when it fits; wrapped rather than cut when the terminal is
        // narrower than the sentence, so the size it needs stays readable.
        Paragraph::new("fproton — terminal too small (need 30x8)")
            .style(th.warn())
            .wrap(WrapMode::Word)
            .render(Rect::new(b.x, b.y, b.width, b.height.min(3)), frame);
        return;
    }
    let tier = tier_of(b);
    let rows = Flex::vertical()
        .constraints([Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)])
        .split(b);
    view_tabs(m, rows[0], frame, tier);
    match (tier, m.screen) {
        (Tier::Narrow, _) => {
            let inner = Flex::vertical()
                .constraints([Constraint::Fill, Constraint::Fixed(1)])
                .split(rows[1]);
            hero(m, inner[0], frame);
            Paragraph::new("widen to 60×16 for the Servers and History tables")
                .style(th.dimmed())
                .wrap(WrapMode::None)
                .render(inner[1], frame);
        }
        (_, Screen::Status) => status_screen(m, rows[1], frame, tier),
        (_, Screen::Servers) => servers_screen(m, rows[1], frame),
        (_, Screen::History) => history_screen(m, rows[1], frame),
    }
    view_status_line(m, rows[2], frame);
    if m.show_help {
        help(m, frame);
    }
}

// ---------------------------------------------------------------- chrome

fn view_tabs(m: &Tui, area: Rect, frame: &mut Frame, tier: Tier) {
    let th = &m.theme;
    if tier == Tier::Narrow {
        let mut spans = Vec::new();
        for (i, label) in ["1", "2", "3"].iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("·", th.dimmed()));
            }
            let active = i == m.screen.index();
            spans.push(Span::styled(
                (*label).to_string(),
                if active { th.accented() } else { th.dimmed() },
            ));
        }
        spans.push(Span::styled(
            format!("  {}", screen_name(m.screen)),
            th.title(),
        ));
        Paragraph::new(Text::from_line(Line::from_spans(spans)))
            .wrap(WrapMode::None)
            .render(area, frame);
        return;
    }

    let cols = Flex::horizontal()
        .constraints([Constraint::Fill, Constraint::FitContent])
        .split(area);
    let tabs = Tabs::new([
        Tab::new(" 1 Status "),
        Tab::new(" 2 Servers "),
        Tab::new(" 3 History "),
    ])
    .style(th.dimmed())
    .active_style(th.selected())
    .separator("");
    StatefulWidget::render(&tabs, cols[0], frame, &mut m.tabs.borrow_mut());

    let mut spans = Vec::new();
    if m.poll_inflight || m.action.is_some() {
        spans.push(Span::styled(
            format!("{} ", SPINNER[m.tick % SPINNER.len()]),
            th.dimmed(),
        ));
    }
    if let Some(s) = &m.status {
        spans.push(Span::styled(format!("{} ", s.engine), th.dimmed()));
    }
    spans.push(Span::styled(format!("{} ", th.name), th.dimmed()));
    let line = Line::from_spans(spans);
    let w = line.width() as u16;
    let r = Rect::new(
        area.x + area.width.saturating_sub(w + 1),
        area.y,
        w.min(area.width),
        1,
    );
    Paragraph::new(Text::from_line(line))
        .wrap(WrapMode::None)
        .render(r, frame);
}

fn view_status_line(m: &Tui, area: Rect, frame: &mut Frame) {
    let th = &m.theme;
    StatusLine::new()
        .style(th.dimmed())
        .right(StatusItem::key_hint("?", "help"))
        .render(area, frame);

    let state = m.status.as_ref().map_or("disconnected", |s| s.state.name());
    let mut spans = vec![
        Span::styled("●", th.state(state)),
        Span::styled(format!(" {state} "), th.text()),
    ];
    let server = m
        .status
        .as_ref()
        .and_then(|s| s.server.as_ref())
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "—".into());
    spans.push(Span::styled(format!("{server} "), th.accented()));
    if let Some(s) = &m.status {
        if s.link.up {
            spans.push(Span::styled(
                format!("{} ", uptime(s.link.uptime_sec)),
                th.dimmed(),
            ));
        }
        if let Some(t) = &s.traffic {
            spans.push(Span::styled(
                format!("↓{} ↑{} ", rate(t.rx_bps), rate(t.tx_bps)),
                th.dimmed(),
            ));
        }
    }
    if let Some((_, action, started)) = &m.action {
        spans.push(Span::styled("│ ", th.dimmed()));
        spans.push(Span::styled(
            format!(
                "{} {} {}s ",
                SPINNER[m.tick % SPINNER.len()],
                action.label(),
                started.elapsed().as_secs()
            ),
            th.warn(),
        ));
    }
    if !m.note.is_empty() {
        spans.push(Span::styled("│ ", th.dimmed()));
        spans.push(Span::styled(
            m.note.clone(),
            if m.note_err { th.err() } else { th.ok() },
        ));
    }
    let r = Rect::new(area.x, area.y, area.width.saturating_sub(8), 1);
    Paragraph::new(Text::from_line(Line::from_spans(spans)))
        .wrap(WrapMode::None)
        .render(r, frame);
}

// ---------------------------------------------------------------- Status

fn status_screen(m: &Tui, area: Rect, frame: &mut Frame, tier: Tier) {
    if area.height == 0 {
        return;
    }
    if tier == Tier::Wide {
        let cols = Flex::horizontal()
            .constraints([Constraint::Percentage(54.0), Constraint::Fill])
            .split(area);
        let left = Flex::vertical()
            .constraints([Constraint::Fill, Constraint::Fixed(9)])
            .split(cols[0]);
        hero(m, left[0], frame);
        traffic(m, left[1], frame, true);
        let right = Flex::vertical()
            .constraints([Constraint::Fixed(8), Constraint::Fill, Constraint::Fixed(3)])
            .split(cols[1]);
        protections(m, right[0], frame);
        warnings(m, right[1], frame);
        recents(m, right[2], frame);
    } else {
        // §7a's Status row requires "Warnings: every entry with text + hint" on this
        // screen, and its Resize rule removes only the sparkline at 60-99 columns.
        // The single-column branch used to drop the whole Warnings panel, so at the
        // very common 80x24 a `warn` such as `conflicting_tunnel` was invisible.
        // `warnings()` self-guards on `area.height < 3`, so a short terminal still
        // degrades safely rather than drawing a broken box.
        // A 24-row terminal leaves 22 rows for five panels, so the fixed slots shrink
        // rather than one panel disappearing: a `warn` the user cannot see is worse
        // than a Protections panel with one line fewer.
        let tight = area.height < 25;
        let rows = Flex::vertical()
            .constraints(if tight {
                [
                    Constraint::Fixed(8),
                    Constraint::Fixed(3),
                    Constraint::Fixed(5),
                    Constraint::Fill,
                    Constraint::Fixed(3),
                ]
            } else {
                [
                    Constraint::Fixed(10),
                    Constraint::Fixed(5),
                    Constraint::Fixed(7),
                    Constraint::Fill,
                    Constraint::Fixed(3),
                ]
            })
            .split(area);
        hero(m, rows[0], frame);
        traffic(m, rows[1], frame, false);
        protections(m, rows[2], frame);
        warnings(m, rows[3], frame);
        recents(m, rows[4], frame);
    }
}

fn panel<'a>(th: &Theme, title: &'a str, focused: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            th.border_focused()
        } else {
            th.border()
        })
        .title(title)
        .padding(Sides::horizontal(1))
}

fn hero(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 3 || area.width < 8 {
        return;
    }
    let th = &m.theme;
    let state = m.status.as_ref().map_or("disconnected", |s| s.state.name());
    let is_error = state == "error" || m.status_err.is_some();
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if is_error {
            th.err()
        } else {
            th.border_focused()
        })
        .title(" Proton VPN ")
        .padding(Sides::horizontal(1));
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut load_row: Option<(u16, f64)> = None;

    if let Some(e) = &m.status_err {
        lines.push(Line::styled(format!("status failed: {e}"), th.err()));
        lines.push(Line::styled("press r to try again", th.dimmed()));
    } else if m.status.as_ref().and_then(|s| s.signed_in) == Some(false) {
        lines.push(Line::styled("Not signed in.", th.warn()));
        lines.push(Line::styled(
            "Press S to open a sign-in terminal.",
            th.text(),
        ));
    } else if let Some(s) = &m.status {
        lines.push(Line::from_spans([
            Span::styled("● ", th.state(state)),
            Span::styled(phrase(s), th.title()),
        ]));
        match &s.server {
            Some(srv) => {
                let where_to = [
                    srv.country_name.clone().or_else(|| srv.country.clone()),
                    srv.city.clone(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                lines.push(Line::from_spans([
                    Span::styled(format!("{} ", srv.name), th.accented()),
                    Span::styled(where_to, th.text()),
                ]));
                if let Some(load) = srv.load {
                    load_row = Some((lines.len() as u16, f64::from(load) / 100.0));
                    lines.push(Line::styled(
                        format!("load              {load}%"),
                        th.dimmed(),
                    ));
                }
                let mut chips = vec![match srv.tier {
                    Some(0) => "free".to_string(),
                    Some(t) => format!("tier {t}"),
                    None => "tier ?".to_string(),
                }];
                chips.extend(srv.features.clone().unwrap_or_default());
                lines.push(Line::styled(chips.join(" · "), th.dimmed()));
            }
            None => lines.push(Line::styled(
                "no server on record yet — press c to connect",
                th.dimmed(),
            )),
        }
        if let Some(ep) = &s.link.endpoint {
            lines.push(Line::from_spans([
                Span::styled("endpoint  ", th.dimmed()),
                Span::styled(ep.clone(), th.text()),
            ]));
        }
        lines.push(Line::from_spans([
            Span::styled("link      ", th.dimmed()),
            Span::styled(
                if s.link.up {
                    format!("up {}", uptime(s.link.uptime_sec))
                } else {
                    "down".into()
                },
                th.text(),
            ),
        ]));
        if let Some(p) = &s.pending {
            lines.push(Line::styled(
                format!(
                    "{} {} {}s / {}s · Esc to cancel",
                    SPINNER[m.tick % SPINNER.len()],
                    p.action.name(),
                    p.elapsed_sec,
                    p.deadline_sec
                ),
                th.warn(),
            ));
        } else if let Some((_, action, started)) = &m.action {
            lines.push(Line::styled(
                format!(
                    "{} {} {}s / {}s · Esc to cancel",
                    SPINNER[m.tick % SPINNER.len()],
                    action.label(),
                    started.elapsed().as_secs(),
                    action.deadline_sec(&m.ctx)
                ),
                th.warn(),
            ));
        }
        if let Some(e) = &s.last_error {
            // The hero is a fixed-width panel and these two are the only free-form
            // lines in it, so they are clipped with an ellipsis rather than cut.
            let w = inner.width as usize;
            lines.push(Line::styled(
                clip(&format!("{} ({})", e.text, e.code_name), w),
                th.err(),
            ));
            lines.push(Line::styled(
                clip(&format!("hint: {}", e.hint), w),
                th.dimmed(),
            ));
        }
        if state == "error" && !s.commands.is_empty() {
            lines.push(Line::styled(s.commands.join("   "), th.dimmed()));
        }
    } else {
        lines.push(Line::styled("reading status…", th.dimmed()));
    }

    Paragraph::new(Text::from_lines(lines))
        .wrap(WrapMode::None)
        .render(inner, frame);

    if let Some((row, value)) = load_row {
        if row < inner.height && inner.width > 26 {
            let bar = MiniBar::new(value, 12)
                .colors(MiniBarColors::new(th.red, th.yellow, th.green, th.green));
            bar.render(Rect::new(inner.x + 5, inner.y + row, 12, 1), frame);
        }
    }
}

fn traffic(m: &Tui, area: Rect, frame: &mut Frame, sparkline: bool) {
    if area.height < 3 {
        return;
    }
    let th = &m.theme;
    let block = panel(th, " Traffic ", false);
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height == 0 {
        return;
    }
    let Some(t) = m.status.as_ref().and_then(|s| s.traffic.as_ref()) else {
        Paragraph::new("traffic unavailable (no proton0 device)")
            .style(th.dimmed())
            .wrap(WrapMode::None)
            .render(inner, frame);
        return;
    };
    let lines = vec![
        Line::from_spans([
            Span::styled("↓ ", th.dimmed()),
            Span::styled(bytes(t.rx_bytes), th.text()),
            Span::styled(format!("  {}", rate(t.rx_bps)), th.dimmed()),
        ]),
        Line::from_spans([
            Span::styled("↑ ", th.dimmed()),
            Span::styled(bytes(t.tx_bytes), th.text()),
            Span::styled(format!("  {}", rate(t.tx_bps)), th.dimmed()),
        ]),
    ];
    let rows = Flex::vertical()
        .constraints([Constraint::Fixed(2), Constraint::Fill])
        .split(inner);
    Paragraph::new(Text::from_lines(lines))
        .wrap(WrapMode::None)
        .render(rows[0], frame);
    if sparkline && rows[1].height > 0 {
        let data: Vec<f64> = m.rx.iter().copied().collect();
        if data.len() > 1 {
            Sparkline::new(&data).style(th.ok()).render(rows[1], frame);
        } else {
            Paragraph::new("rx sparkline fills as samples arrive")
                .style(th.dimmed())
                .wrap(WrapMode::None)
                .render(rows[1], frame);
        }
    }
}

fn protections(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 3 {
        return;
    }
    let th = &m.theme;
    let block = panel(th, " Protections ", false);
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height == 0 {
        return;
    }
    let mut lines: Vec<Line> = Vec::new();
    // §7a: the CLI breaker gets "a persistent yellow row and `c/R/n` refused with the
    // same text". The string comes from `cli_breaker_note`, which the refusal also
    // uses, so the two cannot drift.
    if let Some(note) = m.cli_breaker_note() {
        lines.push(Line::styled(note, th.warn()));
    }
    match m.status.as_ref() {
        None => lines.push(Line::styled("reading status…", th.dimmed())),
        Some(s) => {
            // `kill_switch.active` is the CLI's `pvpn-killswitch-ipv6` leak-guard
            // profile, which it raises for every connection regardless of the
            // kill-switch *setting* (that one is `config.kill-switch`, below).
            lines.push(Line::from_spans([
                Span::styled("leak guard   ", th.dimmed()),
                Span::styled(
                    if s.kill_switch.active {
                        "up (ipv6 leak guard profile)"
                    } else {
                        "down"
                    },
                    if s.kill_switch.active {
                        th.ok()
                    } else {
                        th.text()
                    },
                ),
            ]));
            match &s.config {
                None => {
                    lines.push(Line::styled(
                        "settings unknown — the Proton CLI is not responding",
                        th.warn(),
                    ));
                    lines.push(Line::styled("hint: fproton doctor", th.dimmed()));
                }
                Some(c) => {
                    for key in [
                        "netshield",
                        "kill-switch",
                        "port-forwarding",
                        "vpn-accelerator",
                        "moderate-nat",
                        "ipv6",
                    ] {
                        lines.push(Line::from_spans([
                            Span::styled(format!("{key:<16}"), th.dimmed()),
                            Span::styled(c.get(key).unwrap_or("—").to_string(), th.text()),
                        ]));
                    }
                }
            }
            if let (Some(port), Some(exp)) =
                (s.port_forwarding.port, s.port_forwarding.expires_at.clone())
            {
                lines.push(Line::styled(
                    format!("forwarded port {port} until {exp}"),
                    th.ok(),
                ));
            }
        }
    }
    // Word-wrapped, like the Warnings panel: the breaker row carries its own runnable
    // hint ("— run fproton doctor") and truncating it would drop exactly the half the
    // reader needs. Every other line here is short enough to be unaffected.
    Paragraph::new(Text::from_lines(lines))
        .wrap(WrapMode::Word)
        .render(inner, frame);
}

fn warnings(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 3 {
        return;
    }
    let th = &m.theme;
    let block = panel(th, " Warnings ", false);
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height == 0 {
        return;
    }
    let empty: Vec<fproton_core::Warning> = Vec::new();
    let list = m.status.as_ref().map_or(&empty, |s| &s.warnings);
    if list.is_empty() {
        Paragraph::new("none")
            .style(th.dimmed())
            .wrap(WrapMode::None)
            .render(inner, frame);
        return;
    }
    // Most severe first: at 60-99 columns this panel can be three rows tall, and the
    // entry that gets cut must never be the `error` or the `warn`.
    let mut ordered: Vec<&fproton_core::Warning> = list.iter().collect();
    ordered.sort_by_key(|w| match w.severity {
        fproton_core::Severity::Error => 0u8,
        fproton_core::Severity::Warn => 1,
        fproton_core::Severity::Info => 2,
    });
    let mut lines = Vec::new();
    for w in ordered {
        let style = match w.severity {
            fproton_core::Severity::Error => th.err(),
            fproton_core::Severity::Warn => th.warn(),
            fproton_core::Severity::Info => th.text(),
        };
        lines.push(Line::from_spans([
            Span::styled(format!("{} ", w.id), style),
            Span::styled(w.text.clone(), th.text()),
        ]));
        if !w.hint.is_empty() {
            lines.push(Line::styled(format!("  hint: {}", w.hint), th.dimmed()));
        }
    }
    Paragraph::new(Text::from_lines(lines))
        .wrap(WrapMode::Word)
        .render(inner, frame);
}

fn recents(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 3 {
        return;
    }
    let th = &m.theme;
    let block = panel(th, " Recents ", false);
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height == 0 {
        return;
    }
    let text = if m.recents.is_empty() {
        "none yet".to_string()
    } else {
        m.recents.join("  ·  ")
    };
    Paragraph::new(text)
        .style(if m.recents.is_empty() {
            th.dimmed()
        } else {
            th.text()
        })
        .wrap(WrapMode::None)
        .render(inner, frame);
}

/// The one-line state phrase the hero leads with.
fn phrase(s: &Status) -> String {
    match s.state.name() {
        "connected" => "Connected".into(),
        "connecting" => "Connecting…".into(),
        "disconnecting" => "Disconnecting…".into(),
        "error" => "Last action failed".into(),
        _ => "Not connected".into(),
    }
}

// ---------------------------------------------------------------- Servers

/// Column widths, derived from the panel width so 60 and 200 columns both read.
#[derive(Clone, Copy)]
struct Cols {
    name: usize,
    country: usize,
    city: usize,
    load: usize,
    score: usize,
    tier: usize,
    features: usize,
}

fn cols_for(width: u16) -> Cols {
    let w = width as usize;
    let name = 13;
    let load = 12;
    let score = 7;
    let tier = 5;
    let fixed = name + load + score + tier + 5;
    let rest = w.saturating_sub(fixed);
    let country = rest.clamp(6, 16);
    let city = rest.saturating_sub(country).min(16);
    let features = rest.saturating_sub(country + city);
    Cols {
        name,
        country,
        city,
        load,
        score,
        tier,
        features,
    }
}

/// Truncate to a display width with an ellipsis, without padding.
fn clip(s: &str, n: usize) -> String {
    if n == 0 || s.width() <= n {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + 1 >= n {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// Truncate to a display width, with an ellipsis when it does not fit.
fn fit(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if s.width() <= n {
        return format!("{s:<n$}");
    }
    let mut out = String::new();
    for c in s.chars() {
        if out.width() + 1 >= n {
            break;
        }
        out.push(c);
    }
    out.push('…');
    format!("{out:<n$}")
}

struct ServerItem<'a> {
    row: &'a ServerRow,
    th: &'a Theme,
    cols: Cols,
}

impl RenderItem for ServerItem<'_> {
    fn render(&self, area: Rect, frame: &mut Frame, selected: bool, _skip_rows: u16) {
        let th = self.th;
        let c = self.cols;
        let r = self.row;
        let bar = MiniBar::new(f64::from(r.load) / 100.0, c.load.saturating_sub(5) as u16);
        let load_cell = format!("{} {:>3}%", bar.render_string(), r.load);
        let spans = if selected {
            let text = format!(
                "{} {} {} {} {:>score$.2} {:<tier$} {}",
                fit(&r.name, c.name),
                fit(r.country_name.as_deref().unwrap_or(&r.country), c.country),
                fit(r.city.as_deref().unwrap_or(""), c.city),
                fit(&load_cell, c.load),
                r.score,
                tier_name(r.tier),
                fit(&r.features.join(" "), c.features),
                score = c.score,
                tier = c.tier,
            );
            vec![Span::styled(text, th.selected())]
        } else {
            vec![
                Span::styled(format!("{} ", fit(&r.name, c.name)), th.accented()),
                Span::styled(
                    format!(
                        "{} ",
                        fit(r.country_name.as_deref().unwrap_or(&r.country), c.country)
                    ),
                    th.text(),
                ),
                Span::styled(
                    format!("{} ", fit(r.city.as_deref().unwrap_or(""), c.city)),
                    th.dimmed(),
                ),
                Span::styled(
                    format!("{} ", fit(&load_cell, c.load)),
                    load_style(th, r.load),
                ),
                Span::styled(
                    format!("{:>score$.2} ", r.score, score = c.score),
                    th.dimmed(),
                ),
                Span::styled(
                    format!("{:<tier$} ", tier_name(r.tier), tier = c.tier),
                    th.dimmed(),
                ),
                Span::styled(fit(&r.features.join(" "), c.features), th.dimmed()),
            ]
        };
        Paragraph::new(Text::from_line(Line::from_spans(spans)))
            .wrap(WrapMode::None)
            .render(area, frame);
    }
}

fn tier_name(tier: u8) -> &'static str {
    if tier == 0 {
        "free"
    } else {
        "plus"
    }
}

fn load_style(th: &Theme, load: u8) -> Style {
    match load {
        0..=49 => th.ok(),
        50..=79 => th.warn(),
        _ => th.err(),
    }
}

fn servers_screen(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 4 {
        return;
    }
    let th = &m.theme;
    let total = m
        .status
        .as_ref()
        .and_then(|s| s.index.servers)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".into());
    let title = format!(" Servers  {}/{} shown ", m.servers.len(), total);
    let block = panel(th, &title, true);
    let inner = block.inner(area);
    block.render(area, frame);
    if inner.height < 3 {
        return;
    }
    let rows = Flex::vertical()
        .constraints([Constraint::Fixed(1), Constraint::Fixed(1), Constraint::Fill])
        .split(inner);

    // Filter row: the box on the left, the cycling chips on the right.
    let chips = format!(
        "f:{}  t:{}  o:{}",
        m.feature.map_or("any", |f| f.name()),
        m.tier.map_or("any", |t| t.name()),
        m.sort.name()
    );
    let chip_w = chips.width() as u16;
    let filter_w = rows[0].width.saturating_sub(chip_w + 3);
    Paragraph::new(Text::from_line(Line::from_spans([Span::styled(
        "/ ",
        if m.filter_open {
            th.accented()
        } else {
            th.dimmed()
        },
    )])))
    .wrap(WrapMode::None)
    .render(rows[0], frame);
    m.filter.render(
        Rect::new(rows[0].x + 2, rows[0].y, filter_w.max(1), 1),
        frame,
    );
    Paragraph::new(chips.clone())
        .style(th.dimmed())
        .wrap(WrapMode::None)
        .render(
            Rect::new(
                rows[0].x + rows[0].width.saturating_sub(chip_w),
                rows[0].y,
                chip_w.min(rows[0].width),
                1,
            ),
            frame,
        );

    let c = cols_for(rows[1].width);
    let header = format!(
        "{} {} {} {} {:>score$} {:<tier$} {}",
        fit("NAME", c.name),
        fit("COUNTRY", c.country),
        fit("CITY", c.city),
        fit("LOAD", c.load),
        "SCORE",
        "TIER",
        fit("FEATURES", c.features),
        score = c.score,
        tier = c.tier,
    );
    Paragraph::new(header)
        .style(th.dimmed())
        .wrap(WrapMode::None)
        .render(rows[1], frame);

    if let Some(e) = &m.servers_err {
        let text = if e.code_name == "index" {
            format!(
                "No server index. Press i to build it (fproton index --rebuild)\n\n{}\nhint: {}",
                e.text, e.hint
            )
        } else {
            format!("{}\nhint: {}", e.text, e.hint)
        };
        Paragraph::new(text)
            .style(th.err())
            .wrap(WrapMode::Word)
            .render(rows[2], frame);
        return;
    }
    if m.servers.is_empty() {
        let text = if m.servers_loaded {
            // Esc never edits the filter: it leaves the box when open, and cancels a
            // running action (or does nothing) when closed. Advertise only `/`.
            "No servers match this filter. / to edit it, f/t/o to widen the chips."
        } else {
            "loading the index…"
        };
        Paragraph::new(text)
            .style(th.dimmed())
            .wrap(WrapMode::Word)
            .render(rows[2], frame);
        return;
    }

    let items: Vec<ServerItem> = m
        .servers
        .iter()
        .map(|row| ServerItem { row, th, cols: c })
        .collect();
    let list = VirtualizedList::new(&items)
        .style(th.text())
        .highlight_style(th.selected())
        .fixed_height(1);
    StatefulWidget::render(&list, rows[2], frame, &mut m.list.borrow_mut());
}

// ---------------------------------------------------------------- History

fn history_screen(m: &Tui, area: Rect, frame: &mut Frame) {
    if area.height < 4 {
        return;
    }
    let th = &m.theme;
    if let Some(e) = &m.history_err {
        let block = panel(th, " History ", true);
        let inner = block.inner(area);
        block.render(area, frame);
        Paragraph::new(format!("history is unreadable: {e}"))
            .style(th.err())
            .wrap(WrapMode::Word)
            .render(inner, frame);
        return;
    }
    if m.history.is_empty() {
        let block = panel(th, " History ", true);
        let inner = block.inner(area);
        block.render(area, frame);
        Paragraph::new("Nothing yet. Every connect, reconnect and disconnect lands here.")
            .style(th.dimmed())
            .wrap(WrapMode::Word)
            .render(inner, frame);
        return;
    }
    let rows: Vec<TableRow> = m
        .history
        .iter()
        .map(|h| {
            TableRow::new([
                short_time(&h.at),
                h.action.name().to_string(),
                h.target.clone().unwrap_or_else(|| "—".into()),
                h.server.clone().unwrap_or_else(|| "—".into()),
                h.country.clone().unwrap_or_else(|| "—".into()),
                h.engine.clone(),
                match h.code_name.as_deref() {
                    Some(code) => format!("{} ({code})", h.result.name()),
                    None => h.result.name().to_string(),
                },
                format!("{}s", h.duration_sec),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Fixed(17),
            Constraint::Fixed(11),
            Constraint::Fixed(14),
            Constraint::Fixed(13),
            Constraint::Fixed(8),
            Constraint::Fixed(7),
            Constraint::Fill,
            Constraint::Fixed(6),
        ],
    )
    .header(
        TableRow::new([
            "WHEN", "ACTION", "TARGET", "SERVER", "COUNTRY", "ENGINE", "RESULT", "TOOK",
        ])
        .style(th.dimmed()),
    )
    .block(panel(
        th,
        " History — Enter reconnects to that target ",
        true,
    ))
    .style(th.text())
    .highlight_style(th.selected());
    StatefulWidget::render(&table, area, frame, &mut m.table.borrow_mut());
}

// ---------------------------------------------------------------- help

fn help(m: &Tui, frame: &mut Frame) {
    let th = &m.theme;
    let b = frame.bounds();
    let w = 88.min(b.width);
    let h = (KEYS.len() as u16 + 4).min(b.height);
    if w < 20 || h < 4 {
        return;
    }
    let area = Rect::new(b.x + (b.width - w) / 2, b.y + (b.height - h) / 2, w, h);
    let blank = Text::from_lines((0..area.height).map(|_| {
        Line::styled(
            " ".repeat(area.width as usize),
            Style::new().bg(th.selection),
        )
    }));
    Paragraph::new(blank)
        .wrap(WrapMode::None)
        .render(area, frame);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border_focused())
        .title(" Keys — any key closes ")
        .padding(Sides::all(1))
        .style(Style::new().bg(th.selection));
    let inner = block.inner(area);
    block.render(area, frame);
    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(k, a)| {
            Line::from_spans([
                Span::styled(format!("{k:<24}"), th.key().bg(th.selection)),
                Span::styled((*a).to_string(), th.text().bg(th.selection)),
            ])
        })
        .collect();
    Paragraph::new(Text::from_lines(lines))
        .wrap(WrapMode::None)
        .render(inner, frame);
}

// ---------------------------------------------------------------- formatting

fn screen_name(s: Screen) -> &'static str {
    match s {
        Screen::Status => "Status",
        Screen::Servers => "Servers",
        Screen::History => "History",
    }
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn rate(bps: Option<f64>) -> String {
    match bps {
        None => "—".into(),
        Some(v) => format!("{}/s", bytes(v.max(0.0) as u64)),
    }
}

fn uptime(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// `2026-09-14T22:30:00+02:00` → `09-14 22:30:00`; anything else is passed through.
fn short_time(rfc3339: &str) -> String {
    match (rfc3339.find('T'), rfc3339.len() >= 19) {
        (Some(t), true) if t >= 10 => format!("{} {}", &rfc3339[5..10], &rfc3339[t + 1..t + 9]),
        _ => rfc3339.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resize_tiers_are_the_ones_design_names() {
        assert_eq!(tier_of(Rect::new(0, 0, 120, 40)), Tier::Wide);
        assert_eq!(tier_of(Rect::new(0, 0, 100, 16)), Tier::Wide);
        assert_eq!(tier_of(Rect::new(0, 0, 99, 16)), Tier::Medium);
        assert_eq!(tier_of(Rect::new(0, 0, 60, 16)), Tier::Medium);
        assert_eq!(tier_of(Rect::new(0, 0, 59, 40)), Tier::Narrow);
        assert_eq!(tier_of(Rect::new(0, 0, 120, 15)), Tier::Narrow);
    }

    #[test]
    fn columns_stay_inside_the_panel_at_every_width() {
        for w in [60u16, 80, 100, 160, 200] {
            let c = cols_for(w);
            let used = c.name + c.country + c.city + c.load + c.score + c.tier + c.features + 5;
            assert!(used <= w as usize + 1, "{w} cols overflowed: {used}");
        }
    }

    #[test]
    fn clip_shortens_without_padding() {
        assert_eq!(clip("short", 20), "short");
        assert_eq!(clip("a very long sentence", 10).width(), 10);
        assert!(clip("a very long sentence", 10).ends_with('…'));
        assert_eq!(clip("anything", 0), "anything");
    }

    #[test]
    fn fit_pads_and_truncates_to_the_exact_width() {
        assert_eq!(fit("CH#461", 8), "CH#461  ");
        assert_eq!(fit("Switzerland", 6).width(), 6);
        assert!(fit("Switzerland", 6).contains('…'));
        assert_eq!(fit("anything", 0), "");
    }

    #[test]
    fn human_numbers_read_like_the_cli_ones() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(2048), "2.0 kB");
        assert_eq!(rate(None), "—");
        assert_eq!(rate(Some(1536.0)), "1.5 kB/s");
        assert_eq!(uptime(0), "0s");
        assert_eq!(uptime(3725), "1h 02m");
        assert_eq!(uptime(65), "1m 05s");
    }

    #[test]
    fn short_time_keeps_the_day_and_the_clock() {
        assert_eq!(short_time("2026-09-14T22:30:00+02:00"), "09-14 22:30:00");
        assert_eq!(short_time("nonsense"), "nonsense");
    }
}
