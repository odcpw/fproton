//! The FrankenTUI app: Status · Servers · History (DESIGN.md §7a).
//!
//! One `Model`, one 1 s `status` poll, one 120 ms tick for the spinner and the filter
//! debounce, and at most one long action at a time — which runs in [`subs::ActionSub`]
//! so `Esc` can cancel it (a `Cmd` cannot be cancelled). Short reads go through
//! `Cmd::task`. The TUI owns a `Context` clone and calls `fproton-core` in process; it
//! never spawns `fproton`, and it never prints: the runtime owns the terminal.

mod screens;
mod subs;
mod theme;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use clap::Args;
use ftui::runtime::{Every, SubId, Subscription};
use ftui::widgets::input::TextInput;
use ftui::widgets::table::TableState;
use ftui::widgets::virtualized::VirtualizedListState;
use ftui::widgets::TabsState;
use ftui::{App, Cmd, Event, KeyCode, KeyEvent, KeyEventKind, Model, Modifiers, Style};

use fproton_core::index::{Feature, Query, Sort, Tier};
use fproton_core::state::HistoryEntry;
use fproton_core::{engine, index, state, status, Context, Error, ServerRow, Status, Target};

use subs::{Action, ActionErr, ActionSub};
use theme::Theme;

/// Which screen to open on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Status,
    Servers,
    History,
}

impl Screen {
    fn index(self) -> usize {
        match self {
            Screen::Status => 0,
            Screen::Servers => 1,
            Screen::History => 2,
        }
    }
    fn from_index(i: usize) -> Screen {
        match i % 3 {
            0 => Screen::Status,
            1 => Screen::Servers,
            _ => Screen::History,
        }
    }
}

/// `fproton tui [--servers] [--history]`.
#[derive(Args, Debug, Default, Clone)]
pub struct TuiArgs {
    /// Open on the Servers screen
    #[arg(long)]
    pub servers: bool,
    /// Open on the History screen
    #[arg(long)]
    pub history: bool,
}

impl TuiArgs {
    /// The screen the flags ask for.
    pub fn screen(&self) -> Screen {
        if self.servers {
            Screen::Servers
        } else if self.history {
            Screen::History
        } else {
            Screen::Status
        }
    }
}

/// The help overlay and the behaviour share this one table, so they cannot drift (§7a).
pub const KEYS: &[(&str, &str)] = &[
    (
        "q  Ctrl+C",
        "quit (twice while an action runs: cancel, then quit)",
    ),
    ("?", "this help"),
    ("Tab  ⇧Tab  1 2 3", "Status · Servers · History"),
    ("r", "refresh now"),
    ("Esc", "close this overlay, else cancel the running action"),
    ("c", "connect the default target"),
    ("d  D", "disconnect · force disconnect (tears down proton0)"),
    ("R  n", "reconnect · reconnect to the next server"),
    ("S", "open a sign-in terminal"),
    ("i", "rebuild the server index"),
    ("/", "Servers: filter (Esc leaves the box)"),
    ("f  t  o", "Servers: cycle feature · tier · sort"),
    ("↑↓ PgUp PgDn Home End", "Servers: move"),
    (
        "Enter",
        "Servers: connect · History: reconnect to that target",
    ),
];

const SPINNER: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
/// One sample per `status` poll; §7a asks for a 60-sample rx sparkline.
const RX_SAMPLES: usize = 60;

pub enum Msg {
    Raw(Event),
    /// 1 s: re-read `status` (dropped when one is already in flight).
    Poll,
    /// 120 ms: spinner, filter debounce, note expiry.
    Tick,
    Status(Box<Result<Status, String>>),
    Servers(u64, Box<Result<Vec<ServerRow>, String>>),
    History(Box<Result<Vec<HistoryEntry>, String>>),
    Recents(Vec<String>),
    ActionDone(Action, Box<Result<String, ActionErr>>),
}

impl From<Event> for Msg {
    fn from(e: Event) -> Self {
        Msg::Raw(e)
    }
}

pub struct Tui {
    ctx: Context,
    exit: Arc<AtomicI32>,
    theme: Theme,
    theme_mtime: Option<SystemTime>,
    screen: Screen,
    tabs: RefCell<TabsState>,
    show_help: bool,
    tick: usize,

    status: Option<Status>,
    status_err: Option<String>,
    poll_inflight: bool,
    rx: VecDeque<f64>,

    servers: Vec<ServerRow>,
    servers_err: Option<String>,
    servers_loaded: bool,
    query_seq: u64,
    filter: TextInput,
    filter_open: bool,
    filter_dirty: Option<Instant>,
    feature: Option<Feature>,
    tier: Option<Tier>,
    sort: Sort,
    list: RefCell<VirtualizedListState>,

    history: Vec<HistoryEntry>,
    history_err: Option<String>,
    table: RefCell<TableState>,
    recents: Vec<String>,

    action: Option<(SubId, Action, Instant)>,
    sub_seq: u64,
    quit_armed: Option<(Instant, i32)>,
    note: String,
    note_err: bool,
    note_at: Instant,
}

/// Run the TUI. Returns the process exit code: 0 on a clean quit, 130 on Ctrl+C, and
/// otherwise the §4a code of the session's last failed action (§7a). Never invents one.
pub fn run(args: TuiArgs, ctx: Context) -> anyhow::Result<i32> {
    let exit = Arc::new(AtomicI32::new(0));
    let app = Tui::new(args.screen(), ctx, exit.clone());
    App::fullscreen(app).run()?;
    Ok(exit.load(Ordering::SeqCst))
}

impl Tui {
    fn new(screen: Screen, ctx: Context, exit: Arc<AtomicI32>) -> Tui {
        let theme = Theme::load();
        let filter = TextInput::new()
            .with_placeholder("filter by name, country or city")
            .with_style(theme.text())
            .with_cursor_style(Style::new().bg(theme.accent).fg(theme.selection));
        Tui {
            ctx,
            exit,
            theme_mtime: std::fs::metadata(theme::colors_path())
                .and_then(|m| m.modified())
                .ok(),
            theme,
            screen,
            tabs: RefCell::new(TabsState {
                active: screen.index(),
                offset: 0,
            }),
            show_help: false,
            tick: 0,
            status: None,
            status_err: None,
            poll_inflight: false,
            rx: VecDeque::with_capacity(RX_SAMPLES),
            servers: Vec::new(),
            servers_err: None,
            servers_loaded: false,
            query_seq: 0,
            filter,
            filter_open: false,
            filter_dirty: None,
            feature: None,
            tier: None,
            sort: Sort::default(),
            list: RefCell::new(VirtualizedListState::new()),
            history: Vec::new(),
            history_err: None,
            table: RefCell::new(TableState::default()),
            recents: Vec::new(),
            action: None,
            sub_seq: 0,
            quit_armed: None,
            note: String::new(),
            note_err: false,
            note_at: Instant::now(),
        }
    }

    // ------------------------------------------------------------ short reads

    fn poll(&mut self) -> Cmd<Msg> {
        if self.poll_inflight {
            return Cmd::none();
        }
        self.poll_inflight = true;
        let ctx = self.ctx.clone();
        Cmd::task(move || Msg::Status(Box::new(status::status(&ctx).map_err(|e| e.to_string()))))
    }

    fn query(&self) -> Query {
        let text = self.filter.value().trim().to_string();
        Query {
            country: None,
            city: None,
            feature: self.feature,
            tier: self.tier,
            search: (!text.is_empty()).then_some(text),
            sort: self.sort,
            limit: None,
        }
    }

    fn load_servers(&mut self) -> Cmd<Msg> {
        self.query_seq += 1;
        self.filter_dirty = None;
        let seq = self.query_seq;
        let ctx = self.ctx.clone();
        let q = self.query();
        Cmd::task(move || {
            Msg::Servers(
                seq,
                Box::new(index::servers(&ctx, &q).map_err(|e| e.to_string())),
            )
        })
    }

    fn load_history(&self) -> Cmd<Msg> {
        let ctx = self.ctx.clone();
        Cmd::task(move || {
            Msg::History(Box::new(
                state::history(&ctx, 200).map_err(|e| e.to_string()),
            ))
        })
    }

    fn load_recents(&self) -> Cmd<Msg> {
        let ctx = self.ctx.clone();
        Cmd::task(move || {
            Msg::Recents(
                state::recents(&ctx)
                    .unwrap_or_default()
                    .iter()
                    .map(|t| t.to_string())
                    .collect(),
            )
        })
    }

    // ------------------------------------------------------------ long actions

    /// Start a long action, unless one is already running or the Proton CLI is in its
    /// hang backoff (§2) and the action needs it.
    fn start(&mut self, action: Action) -> Cmd<Msg> {
        if let Some((_, running, _)) = &self.action {
            self.note(
                format!("{} already — Esc cancels it", running.label()),
                true,
            );
            return Cmd::none();
        }
        if let Some(text) = self.cli_refusal(&action) {
            self.note(text, true);
            return Cmd::none();
        }
        self.sub_seq += 1;
        self.note(action.label(), false);
        self.action = Some((0x4650_0000 + self.sub_seq, action, Instant::now()));
        Cmd::none()
    }

    /// The Proton CLI is wedged on this machine (§1.2); while its 60 s backoff is live
    /// every CLI-backed action would exit 5 immediately. Refuse with that text instead
    /// of spawning something that cannot work. nmcli-only actions are unaffected.
    fn cli_refusal(&self, action: &Action) -> Option<String> {
        let needs_cli = matches!(
            action,
            Action::Connect(_) | Action::Reconnect(_) | Action::Disconnect(false)
        );
        if !needs_cli {
            return None;
        }
        let cli = &self.status.as_ref()?.cli;
        let waiting = cli
            .backoff_until
            .as_deref()
            .and_then(seconds_until)
            .unwrap_or(0);
        if waiting > 0 {
            Some(format!(
                "the Proton CLI is in its hang backoff for another {waiting}s — run fproton doctor"
            ))
        } else if cli.hang_suspected {
            Some("the Proton CLI is not answering (hang suspected) — run fproton doctor".into())
        } else {
            None
        }
    }

    /// `Esc`, `q` twice: drop the subscription (the runtime stops it, which trips the
    /// CancelToken), then tear a half-built tunnel down.
    fn cancel_action(&mut self) -> Cmd<Msg> {
        let Some((_, action, _)) = self.action.take() else {
            return Cmd::none();
        };
        self.note(format!("cancelled: {}", action.label()), true);
        match action.cancel_cleanup() {
            Some(cleanup) => self.start(cleanup),
            None => Cmd::none(),
        }
    }

    fn default_target(&self) -> Result<Target, Error> {
        self.ctx.settings.default_target.parse::<Target>()
    }

    fn note(&mut self, text: impl Into<String>, err: bool) {
        self.note = text.into();
        self.note_err = err;
        self.note_at = Instant::now();
    }

    fn maybe_reload_theme(&mut self) {
        let m = std::fs::metadata(theme::colors_path())
            .and_then(|m| m.modified())
            .ok();
        if m != self.theme_mtime {
            self.theme_mtime = m;
            self.theme = Theme::load();
            let value = self.filter.value().to_string();
            let focused = self.filter.focused();
            let mut input = TextInput::new()
                .with_placeholder("filter by name, country or city")
                .with_style(self.theme.text())
                .with_cursor_style(Style::new().bg(self.theme.accent).fg(self.theme.selection))
                .with_value(value);
            input.set_focused(focused);
            self.filter = input;
        }
    }

    fn set_screen(&mut self, s: Screen) -> Cmd<Msg> {
        self.screen = s;
        self.tabs.borrow_mut().active = s.index();
        match s {
            Screen::Servers if !self.servers_loaded => self.load_servers(),
            Screen::History if self.history.is_empty() && self.history_err.is_none() => {
                self.load_history()
            }
            _ => Cmd::none(),
        }
    }

    fn selected_server(&self) -> Option<&ServerRow> {
        let i = self.list.borrow().selected?;
        self.servers.get(i)
    }

    /// Keep the highlighted row inside the viewport without snapping it to the top.
    fn reveal(&self) {
        let total = self.servers.len();
        let mut st = self.list.borrow_mut();
        let Some(sel) = st.selected else { return };
        let vis = st.visible_count().max(1);
        let off = st.scroll_offset_clamped(total);
        if sel < off {
            st.scroll_to(sel, total);
        } else if sel + 1 > off + vis {
            st.scroll_to(sel + 1 - vis, total);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let total = self.servers.len();
        if total == 0 {
            return;
        }
        {
            let mut st = self.list.borrow_mut();
            let cur = st.selected.unwrap_or(0) as isize;
            let next = (cur.saturating_add(delta)).clamp(0, total as isize - 1) as usize;
            st.selected = Some(next);
        }
        self.reveal();
    }

    // ------------------------------------------------------------ keys

    fn on_key(&mut self, k: KeyEvent) -> Cmd<Msg> {
        if k.kind == KeyEventKind::Release {
            return Cmd::none();
        }
        let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(Modifiers::CTRL);
        if self.show_help && !ctrl_c {
            self.show_help = false;
            return Cmd::none();
        }
        if ctrl_c {
            return self.quit(130);
        }

        // The filter box eats printable keys while it is open.
        if self.screen == Screen::Servers && self.filter_open {
            match k.code {
                KeyCode::Escape | KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                    self.filter_open = false;
                    self.filter.set_focused(false);
                }
                _ => {
                    if self.filter.handle_event(&Event::Key(k)) {
                        self.filter_dirty = Some(Instant::now());
                    }
                }
            }
            return Cmd::none();
        }

        match k.code {
            KeyCode::Char('q') => return self.quit(self.exit.load(Ordering::SeqCst)),
            KeyCode::Char('?') => {
                self.show_help = true;
                return Cmd::none();
            }
            KeyCode::Escape => return self.cancel_action(),
            KeyCode::Char('1') => return self.set_screen(Screen::Status),
            KeyCode::Char('2') => return self.set_screen(Screen::Servers),
            KeyCode::Char('3') => return self.set_screen(Screen::History),
            KeyCode::Tab => return self.set_screen(Screen::from_index(self.screen.index() + 1)),
            KeyCode::BackTab => {
                return self.set_screen(Screen::from_index(self.screen.index() + 2))
            }
            KeyCode::Char('r') => {
                let mut cmds = vec![self.poll(), self.load_recents()];
                match self.screen {
                    Screen::Servers => cmds.push(self.load_servers()),
                    Screen::History => cmds.push(self.load_history()),
                    Screen::Status => {}
                }
                return Cmd::batch(cmds);
            }
            KeyCode::Char('i') => return self.start(Action::Rebuild),
            KeyCode::Char('c') => {
                return match self.default_target() {
                    Ok(t) => self.start(Action::Connect(t)),
                    Err(e) => {
                        self.note(format!("default_target: {e}"), true);
                        Cmd::none()
                    }
                }
            }
            KeyCode::Char('d') => return self.start(Action::Disconnect(false)),
            KeyCode::Char('D') => return self.start(Action::Disconnect(true)),
            KeyCode::Char('R') => return self.start(Action::Reconnect(false)),
            KeyCode::Char('n') => return self.start(Action::Reconnect(true)),
            KeyCode::Char('S') => {
                self.sign_in();
                return Cmd::none();
            }
            _ => {}
        }

        match self.screen {
            Screen::Servers => self.on_servers_key(k),
            Screen::History => self.on_history_key(k),
            Screen::Status => Cmd::none(),
        }
    }

    fn on_servers_key(&mut self, k: KeyEvent) -> Cmd<Msg> {
        let page = self.list.borrow().visible_count().max(1) as isize;
        match k.code {
            KeyCode::Char('/') => {
                self.filter_open = true;
                self.filter.set_focused(true);
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(page),
            KeyCode::PageUp => self.move_selection(-page),
            KeyCode::Home => self.move_selection(isize::MIN / 2),
            KeyCode::End => self.move_selection(isize::MAX / 2),
            KeyCode::Char('f') => {
                self.feature = match self.feature {
                    None => Some(Feature::P2p),
                    Some(Feature::P2p) => Some(Feature::SecureCore),
                    Some(Feature::SecureCore) => Some(Feature::Tor),
                    Some(Feature::Tor) => Some(Feature::Streaming),
                    Some(Feature::Streaming) => None,
                };
                return self.load_servers();
            }
            KeyCode::Char('t') => {
                self.tier = match self.tier {
                    None => Some(Tier::Free),
                    Some(Tier::Free) => Some(Tier::Plus),
                    Some(Tier::Plus) => None,
                };
                return self.load_servers();
            }
            KeyCode::Char('o') => {
                self.sort = match self.sort {
                    Sort::Score => Sort::Load,
                    Sort::Load => Sort::Name,
                    Sort::Name => Sort::Score,
                };
                return self.load_servers();
            }
            KeyCode::Enter => {
                let name = match self.selected_server() {
                    Some(row) => row.name.clone(),
                    None => return Cmd::none(),
                };
                return match engine::target_for_server(&name) {
                    Ok(t) => self.start(Action::Connect(t)),
                    Err(e) => {
                        self.note(e.to_string(), true);
                        Cmd::none()
                    }
                };
            }
            _ => {}
        }
        Cmd::none()
    }

    fn on_history_key(&mut self, k: KeyEvent) -> Cmd<Msg> {
        let total = self.history.len();
        match k.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let mut st = self.table.borrow_mut();
                let next = st
                    .selected
                    .map_or(0, |i| (i + 1).min(total.saturating_sub(1)));
                st.selected = (total > 0).then_some(next);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let mut st = self.table.borrow_mut();
                let next = st.selected.map_or(0, |i| i.saturating_sub(1));
                st.selected = (total > 0).then_some(next);
            }
            KeyCode::Home => self.table.borrow_mut().selected = (total > 0).then_some(0),
            KeyCode::End => {
                self.table.borrow_mut().selected = (total > 0).then_some(total - 1);
            }
            KeyCode::Enter => {
                let target = self
                    .table
                    .borrow()
                    .selected
                    .and_then(|i| self.history.get(i))
                    .and_then(|h| h.target.clone());
                let Some(target) = target else {
                    self.note("that row has no target to reconnect to", true);
                    return Cmd::none();
                };
                return match target.parse::<Target>() {
                    Ok(t) => self.start(Action::Connect(t)),
                    Err(e) => {
                        self.note(format!("{target}: {e}"), true);
                        Cmd::none()
                    }
                };
            }
            _ => {}
        }
        Cmd::none()
    }

    /// `S`: hand sign-in to a terminal of its own — it is interactive (password + TOTP)
    /// and has no deadline. Constant literal argv only (§1.5).
    fn sign_in(&mut self) {
        let spawned =
            std::process::Command::new("omarchy-launch-floating-terminal-with-presentation")
                .arg("fproton signin")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        match spawned {
            Ok(_) => self.note("opened a sign-in terminal", false),
            Err(e) => self.note(format!("could not open a terminal: {e}"), true),
        }
    }

    /// `q` / `Ctrl+C`. While an action is in flight the first press only warns; a
    /// second press within 2 s cancels the action and quits (§7a).
    fn quit(&mut self, code: i32) -> Cmd<Msg> {
        if self.action.is_some() {
            let armed = self
                .quit_armed
                .is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(2));
            if !armed {
                self.quit_armed = Some((Instant::now(), code));
                self.note(
                    "an action is running — Esc cancels it, or press quit again to cancel and quit",
                    true,
                );
                return Cmd::none();
            }
            let _ = self.cancel_action();
            self.action = None;
        }
        self.exit.store(code, Ordering::SeqCst);
        Cmd::quit()
    }
}

/// Seconds from now until an RFC 3339 instant, `None` when it is unparsable or past.
fn seconds_until(text: &str) -> Option<u64> {
    let then = state::unix_from_rfc3339(text)?;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    (then > now).then(|| (then - now) as u64)
}

impl Model for Tui {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Msg> {
        let mut cmds = vec![self.poll(), self.load_history(), self.load_recents()];
        if self.screen == Screen::Servers {
            cmds.push(self.load_servers());
        }
        Cmd::batch(cmds)
    }

    fn subscriptions(&self) -> Vec<Box<dyn Subscription<Msg>>> {
        let mut v: Vec<Box<dyn Subscription<Msg>>> = vec![
            Box::new(Every::with_id(1, Duration::from_millis(1000), || Msg::Poll)),
            Box::new(Every::with_id(2, Duration::from_millis(120), || Msg::Tick)),
        ];
        if let Some((id, action, _)) = &self.action {
            v.push(Box::new(ActionSub {
                id: *id,
                ctx: self.ctx.clone(),
                action: action.clone(),
            }));
        }
        v
    }

    fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::Raw(Event::Key(k)) => self.on_key(k),
            Msg::Raw(_) => Cmd::none(),
            Msg::Poll => {
                self.maybe_reload_theme();
                self.poll()
            }
            Msg::Tick => {
                self.tick += 1;
                if !self.note.is_empty() && self.note_at.elapsed() > Duration::from_secs(10) {
                    self.note.clear();
                }
                if self
                    .filter_dirty
                    .is_some_and(|at| at.elapsed() >= Duration::from_millis(80))
                {
                    return self.load_servers();
                }
                Cmd::none()
            }
            Msg::Status(r) => {
                self.poll_inflight = false;
                match *r {
                    Ok(s) => {
                        match s.traffic.as_ref() {
                            Some(t) => {
                                if self.rx.len() == RX_SAMPLES {
                                    self.rx.pop_front();
                                }
                                self.rx.push_back(t.rx_bps.unwrap_or(0.0));
                            }
                            None => self.rx.clear(),
                        }
                        self.status_err = None;
                        self.status = Some(s);
                    }
                    Err(e) => self.status_err = Some(e),
                }
                Cmd::none()
            }
            Msg::Servers(seq, r) => {
                if seq != self.query_seq {
                    return Cmd::none();
                }
                self.servers_loaded = true;
                match *r {
                    Ok(rows) => {
                        self.servers_err = None;
                        self.servers = rows;
                        {
                            let mut st = self.list.borrow_mut();
                            let last = self.servers.len().saturating_sub(1);
                            st.selected = (!self.servers.is_empty())
                                .then(|| st.selected.unwrap_or(0).min(last));
                        }
                        self.reveal();
                    }
                    Err(e) => {
                        self.servers.clear();
                        self.list.borrow_mut().selected = None;
                        self.servers_err = Some(e);
                    }
                }
                Cmd::none()
            }
            Msg::History(r) => {
                match *r {
                    Ok(rows) => {
                        self.history_err = None;
                        self.history = rows;
                        let mut st = self.table.borrow_mut();
                        st.selected = match st.selected {
                            _ if self.history.is_empty() => None,
                            Some(i) => Some(i.min(self.history.len() - 1)),
                            None => Some(0),
                        };
                    }
                    Err(e) => self.history_err = Some(e),
                }
                Cmd::none()
            }
            Msg::Recents(r) => {
                self.recents = r;
                Cmd::none()
            }
            Msg::ActionDone(action, r) => {
                if self
                    .action
                    .as_ref()
                    .is_some_and(|(_, running, _)| *running == action)
                {
                    self.action = None;
                }
                self.quit_armed = None;
                match *r {
                    Ok(text) => {
                        self.exit.store(0, Ordering::SeqCst);
                        self.note(text, false);
                    }
                    Err(e) => {
                        self.exit.store(e.code, Ordering::SeqCst);
                        if e.interrupted {
                            self.note(format!("{} was cancelled", action.label()), true);
                        } else {
                            let hint = if e.hint.is_empty() {
                                String::new()
                            } else {
                                format!(" — {}", e.hint)
                            };
                            self.note(format!("{} ({}){hint}", e.text, e.code_name), true);
                        }
                    }
                }
                let mut cmds = vec![self.poll(), self.load_history(), self.load_recents()];
                if matches!(action, Action::Rebuild) {
                    cmds.push(self.load_servers());
                }
                Cmd::batch(cmds)
            }
        }
    }

    fn view(&self, frame: &mut ftui::Frame) {
        screens::view(self, frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftui::{Frame, GraphemePool};

    fn model() -> Tui {
        let dir = std::env::temp_dir().join(format!("fproton-tui-test-{}", std::process::id()));
        let ctx = Context::for_test(&dir).expect("test context");
        Tui::new(Screen::Status, ctx, Arc::new(AtomicI32::new(0)))
    }

    /// Render into a real `Buffer` and read the text back, row by row.
    fn render(m: &Tui, w: u16, h: u16) -> Vec<String> {
        let mut pool = GraphemePool::new();
        let mut frame = Frame::new(w, h, &mut pool);
        m.view(&mut frame);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| {
                        frame
                            .buffer
                            .get(x, y)
                            .and_then(|c| c.content.as_char())
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn screen_text(m: &Tui, w: u16, h: u16) -> String {
        render(m, w, h).join("\n")
    }

    #[test]
    fn the_wide_tier_renders_tabs_a_badge_and_a_status_line() {
        let m = model();
        let text = screen_text(&m, 120, 40);
        assert!(text.contains("Status"), "{text}");
        assert!(text.contains("Servers"), "{text}");
        assert!(text.contains('●'), "state badge missing:\n{text}");
        assert!(text.contains("help"), "{text}");
    }

    #[test]
    fn the_medium_tier_drops_the_sparkline_but_keeps_the_badge() {
        let m = model();
        let text = screen_text(&m, 80, 24);
        assert!(text.contains('●'), "{text}");
        assert!(text.contains("Status"), "{text}");
    }

    #[test]
    fn the_narrow_tier_collapses_the_tabs_and_still_shows_the_badge() {
        let m = model();
        let text = screen_text(&m, 50, 14);
        assert!(text.contains('●'), "{text}");
        assert!(
            text.contains("1·2·3"),
            "collapsed tab strip missing:\n{text}"
        );
    }

    #[test]
    fn below_thirty_by_eight_it_is_one_honest_line() {
        let m = model();
        let text = screen_text(&m, 28, 6);
        assert!(text.contains("too small"), "{text}");
        assert!(text.contains("30x8"), "{text}");
    }

    #[test]
    fn a_zero_area_frame_renders_nothing_and_does_not_panic() {
        let m = model();
        let rows = render(&m, 0, 0);
        assert!(rows.is_empty());
    }

    #[test]
    fn every_screen_renders_with_no_index_and_no_status() {
        for screen in [Screen::Status, Screen::Servers, Screen::History] {
            let mut m = model();
            m.screen = screen;
            m.tabs.borrow_mut().active = screen.index();
            m.servers_loaded = true;
            m.servers_err = Some("no server index".into());
            let text = screen_text(&m, 100, 30);
            assert!(!text.trim().is_empty(), "{screen:?} rendered blank");
            assert!(text.contains('●'), "{screen:?} lost its badge:\n{text}");
        }
    }

    #[test]
    fn the_help_overlay_lists_the_bindings_the_model_implements() {
        let mut m = model();
        m.show_help = true;
        let text = screen_text(&m, 110, 34);
        for (key, _) in KEYS.iter().take(6) {
            let first = key.split_whitespace().next().unwrap();
            assert!(text.contains(first), "help is missing {first}:\n{text}");
        }
    }

    #[test]
    fn quitting_while_an_action_runs_takes_two_presses() {
        let mut m = model();
        m.action = Some((7, Action::Disconnect(true), Instant::now()));
        let _ = m.quit(0);
        assert!(m.action.is_some(), "the first press must not quit");
        assert!(m.note.contains("press quit again"), "{}", m.note);
        let _ = m.quit(0);
        assert!(m.action.is_none(), "the second press cancels");
    }

    #[test]
    fn a_second_action_is_refused_while_one_is_in_flight() {
        let mut m = model();
        let _ = m.start(Action::Rebuild);
        assert!(m.action.is_some());
        let _ = m.start(Action::Disconnect(true));
        assert!(m.note.contains("already"), "{}", m.note);
    }

    #[test]
    fn a_cancelled_connect_is_followed_by_a_force_disconnect() {
        let mut m = model();
        let t: Target = "fastest".parse().expect("target");
        let _ = m.start(Action::Connect(t));
        let _ = m.cancel_action();
        assert_eq!(
            m.action.as_ref().map(|(_, a, _)| a.clone()),
            Some(Action::Disconnect(true))
        );
    }

    #[test]
    fn the_keymap_table_is_the_help_overlay() {
        assert!(KEYS.iter().any(|(k, _)| k.starts_with('?')));
        assert!(KEYS.iter().all(|(k, a)| !k.is_empty() && !a.is_empty()));
    }
}
