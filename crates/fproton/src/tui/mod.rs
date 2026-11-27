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
use ftui::{
    App, Cmd, Event, KeyCode, KeyEvent, KeyEventKind, Model, Modifiers, MouseButton, MouseEventKind,
};

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

/// Why the Servers screen has no rows.
///
/// The §7a index empty state is chosen by `code_name`, never by the error's prose:
/// gating on the substring "index" meant the real message — "Proton's server list is
/// missing at …" — never matched, so the required "Press i to build it" line never
/// appeared on a real install, and the error's own runnable hint was dropped too.
#[derive(Clone, Debug)]
pub struct ServersError {
    pub code_name: String,
    pub text: String,
    pub hint: String,
}

pub enum Msg {
    Raw(Event),
    /// 1 s: re-read `status` (dropped when one is already in flight).
    Poll,
    /// 120 ms: spinner, filter debounce, note expiry.
    Tick,
    Status(Box<Result<Status, String>>),
    Servers(u64, Box<Result<Vec<ServerRow>, ServersError>>),
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
    /// Set when a `q q` quit cancelled a connect: `run()` runs `disconnect --force`
    /// after the runtime has stopped, so §7a's cleanup is not lost (see `run`).
    force_disconnect_on_exit: Arc<std::sync::atomic::AtomicBool>,
    theme: Theme,
    theme_mtime: Option<SystemTime>,
    screen: Screen,
    help_rect: std::cell::Cell<ftui::layout::Rect>,
    tab_rects: std::cell::Cell<[ftui::layout::Rect; 3]>,
    show_help: bool,
    tick: usize,

    status: Option<Status>,
    status_err: Option<String>,
    poll_inflight: bool,
    rx: VecDeque<f64>,

    servers: Vec<ServerRow>,
    servers_err: Option<ServersError>,
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
    // §7a: "a cancelled connect is followed by `disconnect --force`". The Esc path
    // queues that as the next action; the quit path cannot — the runtime stops the
    // moment `Cmd::quit()` is returned, so a queued action would never run and a
    // half-built tunnel outlived the TUI. The flag is honoured HERE, after the
    // runtime has stopped the subscription (which trips the CancelToken and kills the
    // connect child group), so the teardown cannot race the connect it is cleaning up.
    let owed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = Tui::new(args.screen(), ctx.clone(), exit.clone(), owed.clone());
    App::fullscreen(app).with_mouse().run()?;
    if owed.load(Ordering::SeqCst) {
        // nmcli only, and bounded by the disconnect deadline like every other spawn.
        let _ = engine::disconnect(&ctx, true, &fproton_core::CancelToken::new());
    }
    Ok(exit.load(Ordering::SeqCst))
}

impl Tui {
    fn new(
        screen: Screen,
        ctx: Context,
        exit: Arc<AtomicI32>,
        force_disconnect_on_exit: Arc<std::sync::atomic::AtomicBool>,
    ) -> Tui {
        let theme = Theme::load();
        let filter = TextInput::new()
            .with_placeholder("filter by name, country or city")
            .with_style(theme.text())
            .with_cursor_style(theme.cursor());
        Tui {
            ctx,
            exit,
            force_disconnect_on_exit,
            theme_mtime: std::fs::metadata(theme::colors_path())
                .and_then(|m| m.modified())
                .ok(),
            theme,
            screen,
            help_rect: Default::default(),
            tab_rects: Default::default(),
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
                Box::new(index::servers(&ctx, &q).map_err(|e| ServersError {
                    code_name: e.code_name().to_string(),
                    text: e.to_string(),
                    hint: e.hint(),
                })),
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
        self.cli_breaker_note()
    }

    /// The breaker's own words, independent of any action.
    ///
    /// §7a wants `cli.hang_suspected`/backoff to produce "a persistent yellow row
    /// **and** `c/R/n` refused with the same text". Only the refusal existed, so a
    /// user who never pressed a key was never told the CLI was wedged. The Status
    /// screen renders this; `cli_refusal` returns it — one source, so the row and the
    /// refusal cannot drift apart.
    pub(super) fn cli_breaker_note(&self) -> Option<String> {
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
            // The help overlay documents Esc as "close this overlay, else cancel the
            // running action"; with nothing running it used to be a silent no-op, so
            // the key read as dead. Every other refusal explains itself.
            self.note("nothing running to cancel", false);
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
                .with_cursor_style(self.theme.cursor())
                .with_value(value);
            input.set_focused(focused);
            self.filter = input;
        }
    }

    fn set_screen(&mut self, s: Screen) -> Cmd<Msg> {
        self.screen = s;
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
            // What Esc would have queued next. `cancel_action` turns it into a
            // pending action, which quitting immediately discards, so record the debt
            // for `run()` to settle once the runtime has torn the child group down.
            if self
                .action
                .as_ref()
                .and_then(|(_, a, _)| a.cancel_cleanup())
                .is_some()
            {
                self.force_disconnect_on_exit.store(true, Ordering::SeqCst);
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
            Msg::Raw(Event::Mouse(ev)) if ev.kind == MouseEventKind::Down(MouseButton::Left) => {
                if self.show_help {
                    self.show_help = false;
                } else if self.help_rect.get().contains(ev.x, ev.y) {
                    self.show_help = true;
                } else if let Some(index) = self
                    .tab_rects
                    .get()
                    .iter()
                    .position(|r| r.contains(ev.x, ev.y))
                {
                    return self.set_screen(Screen::from_index(index));
                }
                Cmd::none()
            }
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
        Tui::new(
            Screen::Status,
            ctx,
            Arc::new(AtomicI32::new(0)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
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
        assert!(text.contains("?  Keys"), "{text}");
    }

    #[test]
    fn clicking_navigation_after_resize_follows_the_drawn_labels() {
        let mut m = model();
        let click = |x, y| {
            Msg::Raw(Event::Mouse(ftui::MouseEvent::new(
                MouseEventKind::Down(MouseButton::Left),
                x,
                y,
            )))
        };
        for (w, h, word, screen) in [
            (120, 34, "History", Screen::History),
            (40, 16, "2", Screen::Servers),
        ] {
            let rows = render(&m, w, h);
            let (y, (line, byte)) = rows
                .iter()
                .enumerate()
                .find_map(|(y, line)| line.find(word).map(|byte| (y, (line, byte))))
                .unwrap();
            let x = unicode_width::UnicodeWidthStr::width(&line[..byte]) as u16;
            m.update(click(x, y as u16));
            assert_eq!(m.screen, screen);
            let header = &rows[y];
            let key = if w >= 45 { "?  Keys" } else { "?" };
            let byte = header.find(key).unwrap();
            m.update(click(
                unicode_width::UnicodeWidthStr::width(&header[..byte]) as u16,
                y as u16,
            ));
            assert!(m.show_help);
            m.update(click(x, y as u16));
            assert!(!m.show_help);
        }
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
            m.servers_loaded = true;
            m.servers_err = Some(ServersError {
                code_name: "index".into(),
                // Deliberately NOT containing the word "index": the §7a empty state
                // used to be gated on that substring, while the core's real message
                // is "Proton's server list is missing at <path>".
                text: "Proton's server list is missing at /nowhere/serverlist.json".into(),
                hint: "protonvpn connect".into(),
            });
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
    fn quitting_during_a_connect_still_owes_a_force_disconnect() {
        // §7a: "a cancelled connect is followed by `disconnect --force`". The quit
        // path discards the queued cleanup (the runtime stops immediately), so the
        // debt is recorded for `run()` to settle after the child group is torn down.
        let mut m = model();
        let owed = m.force_disconnect_on_exit.clone();
        let t: Target = "fastest".parse().expect("target");
        let _ = m.start(Action::Connect(t));
        let _ = m.quit(0); // first press only warns
        let _ = m.quit(0); // second press cancels and quits
        assert!(
            owed.load(Ordering::SeqCst),
            "a half-built tunnel would have outlived the TUI"
        );
    }

    #[test]
    fn quitting_with_nothing_running_owes_no_teardown() {
        let mut m = model();
        let owed = m.force_disconnect_on_exit.clone();
        let _ = m.quit(0);
        assert!(!owed.load(Ordering::SeqCst));
    }

    #[test]
    fn escape_with_nothing_running_says_so_instead_of_doing_nothing() {
        let mut m = model();
        let _ = m.cancel_action();
        assert_eq!(m.note, "nothing running to cancel");
    }

    #[test]
    fn the_medium_tier_still_shows_the_warnings_panel() {
        // §7a's Resize rule drops only the sparkline at 60-99 columns; the Warnings
        // panel is required on the Status screen at every tier that has one.
        let mut m = model();
        let mut status = Status::empty("cli", "2026-09-15T00:00:00+02:00".into());
        status.warnings.push(fproton_core::Warning {
            id: "conflicting_tunnel".into(),
            severity: fproton_core::Severity::Warn,
            text: "tailscale0 is up and will contest the default route".into(),
            hint: "sudo tailscale down".into(),
        });
        m.status = Some(status);
        for (w, h) in [(64u16, 16u16), (80, 24), (99, 30), (120, 40)] {
            let text = screen_text(&m, w, h);
            assert!(
                text.contains("Warnings"),
                "no Warnings panel at {w}x{h}:\n{text}"
            );
            assert!(
                text.contains("tailscale0"),
                "the warning itself is invisible at {w}x{h}:\n{text}"
            );
        }
    }

    #[test]
    fn a_wedged_cli_is_a_persistent_row_not_only_a_refusal() {
        // §7a: hang_suspected/backoff gets "a persistent yellow row AND c/R/n refused
        // with the same text". Only the refusal existed.
        let mut m = model();
        let mut status = Status::empty("cli", "2026-09-15T00:00:00+02:00".into());
        status.cli.hang_suspected = true;
        m.status = Some(status);
        let row = m.cli_breaker_note().expect("a breaker note");
        for (w, h) in [(80u16, 24u16), (120, 40)] {
            let text = screen_text(&m, w, h);
            assert!(
                text.contains("not answering"),
                "the wedged CLI is invisible at {w}x{h}:\n{text}"
            );
        }
        // The refusal uses the same string, so help and behaviour cannot drift.
        let t: Target = "fastest".parse().expect("target");
        let _ = m.start(Action::Connect(t));
        assert_eq!(m.note, row);
        assert!(
            m.action.is_none(),
            "the action must be refused, not started"
        );
    }

    #[test]
    fn a_missing_server_list_offers_the_rebuild_key() {
        // §7a's index empty state was gated on the substring "index"; the core's real
        // message is "Proton's server list is missing at <path>", so on a real install
        // the required line never appeared.
        let mut m = model();
        m.screen = Screen::Servers;
        m.servers_loaded = true;
        m.servers_err = Some(ServersError {
            code_name: "index".into(),
            text: "Proton's server list is missing at /nowhere/serverlist.json".into(),
            hint: "protonvpn connect".into(),
        });
        let text = screen_text(&m, 110, 24);
        assert!(text.contains("Press i to build it"), "{text}");
        assert!(
            text.contains("protonvpn connect"),
            "the hint is dropped:\n{text}"
        );
    }

    #[test]
    fn the_keymap_table_is_the_help_overlay() {
        assert!(KEYS.iter().any(|(k, _)| k.starts_with('?')));
        assert!(KEYS.iter().all(|(k, a)| !k.is_empty() && !a.is_empty()));
    }
}
