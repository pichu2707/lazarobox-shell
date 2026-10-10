use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::{
    copy::{CopyCommand, CopyState},
    keys::{encode_key, encode_paste},
    layout::{PaneId, Rect},
    pane::{CursorKind, CursorShape, Pane, PaneSize},
    prefix::{self, Binding, Group, PREFIX_KEY, PREFIX_TREE, PrefixAction, Step},
    pty::{PtyEvent, SpawnSpec},
};

/// Rows of history kept by the emulator.
const SCROLLBACK_LINES: usize = 10_000;

/// Byte sent to the child when the prefix is pressed twice (NUL, Ctrl+Space).
const PREFIX_LITERAL: u8 = 0x00;

/// Where the user's keys go. Independent from `AppMode`, which is what is viewed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputMode {
    /// Keys are encoded and sent to the child.
    #[default]
    Terminal,
    /// The prefix was pressed; the next key picks an action.
    Prefix,
    /// Read-only scrollback navigation.
    Copy(CopyState),
    /// A group is open (WINDOW, TAB, GO, BUFFER); the next key picks its binding.
    Group(&'static Group),
    /// Pane resize mode; its keys arrive with the resize slice.
    Resize,
    /// Waiting for a yes/no answer.
    Confirm(Confirm),
}

/// What a pending confirmation asks about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirm {
    /// Waiting for `y` to quit.
    Quit,
}

impl InputMode {
    /// Text of the statusline mode block.
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "TERMINAL",
            Self::Prefix => "PREFIX",
            Self::Copy(_) => "COPY",
            Self::Group(group) => group.label,
            Self::Resize => "RESIZE",
            Self::Confirm(Confirm::Quit) => "Quit? (y/n)",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Something a pane's child did.
    Pty(PaneId, PtyEvent),
    /// The child's working directory, as last read by the runtime.
    Cwd(PathBuf),
}

/// Side effects requested by `App::update`, executed by the runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    WritePty(PaneId, Vec<u8>),
    ResizePty(PaneId, PaneSize),
    Quit,
}

/// Pane size for a terminal of `cols` x `rows`: the bottom row is the
/// statusline, and the pane is never smaller than 1x1.
fn pane_size(cols: u16, rows: u16) -> PaneSize {
    PaneSize {
        rows: rows.saturating_sub(1).max(1),
        cols: cols.max(1),
    }
}

/// Name shown in the statusline for the shell at `path` (`sh` if unknown).
pub fn shell_basename(path: Option<&str>) -> &str {
    path.and_then(|p| Path::new(p).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("sh")
}

/// Where the parts of the screen go, computed once from the terminal size so
/// the app and the UI cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenLayout {
    pub body: Rect,
    pub status: Rect,
}

impl ScreenLayout {
    fn new(cols: u16, rows: u16) -> Self {
        let size = pane_size(cols, rows);
        Self {
            body: Rect {
                x: 0,
                y: 0,
                width: size.cols,
                height: size.rows,
            },
            status: Rect {
                x: 0,
                y: rows.saturating_sub(1),
                width: size.cols,
                height: 1,
            },
        }
    }
}

/// Owns the emulator and the input state. Pure: it never touches IO.
pub struct App {
    id: PaneId,
    pane: Pane,
    input: InputMode,
    dirty: bool,
    screen: ScreenLayout,
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    shell: Option<String>,
}

impl App {
    /// An app for a terminal of `cols` x `rows`, with one pane in the body.
    pub fn new(cols: u16, rows: u16) -> Self {
        let screen = ScreenLayout::new(cols, rows);
        Self {
            id: PaneId::FIRST,
            pane: Pane::new(body_size(screen.body), SCROLLBACK_LINES),
            input: InputMode::default(),
            dirty: true,
            screen,
            cwd: None,
            home: None,
            shell: None,
        }
    }

    /// Sets the facts the statusline shows. They are inputs, not lookups, so
    /// `update` stays free of IO.
    pub fn with_env(mut self, shell: Option<&str>, home: Option<PathBuf>, cwd: PathBuf) -> Self {
        self.shell = shell.map(str::to_owned);
        self.home = home;
        self.cwd = Some(cwd);
        self
    }

    pub fn shell_name(&self) -> &str {
        shell_basename(self.shell.as_deref())
    }

    /// The pane to start before the first event, and how to start it.
    pub fn initial_spawn(&self) -> (PaneId, SpawnSpec) {
        let cwd = self.cwd.clone().unwrap_or_else(|| "/".into());
        (
            self.id,
            SpawnSpec::new(self.shell.as_deref(), cwd, self.focused_size()),
        )
    }

    pub fn input(&self) -> InputMode {
        self.input
    }

    /// Whether a redraw is due. Reading it clears it.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn screen(&self) -> ScreenLayout {
        self.screen
    }

    pub fn focused(&self) -> PaneId {
        self.id
    }

    pub fn focused_pane(&self) -> &Pane {
        &self.pane
    }

    pub fn focused_size(&self) -> PaneSize {
        body_size(self.screen.body)
    }

    /// The child's cwd for the statusline, with `$HOME` shown as `~`. Empty
    /// until the first cwd is known.
    pub fn cwd_label(&self) -> String {
        let Some(cwd) = &self.cwd else {
            return String::new();
        };
        let home = self.home.as_deref().filter(|home| home.parent().is_some());
        match home.and_then(|home| cwd.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".into(),
            Some(rest) => format!("~/{}", rest.display()),
            None => cwd.display().to_string(),
        }
    }

    pub fn update(&mut self, event: AppEvent) -> Vec<Effect> {
        match event {
            AppEvent::Key(key) => self.on_key(key),
            AppEvent::Paste(text) => self.on_paste(&text),
            AppEvent::Resize { cols, rows } => self.on_resize(cols, rows),
            AppEvent::Pty(id, event) => {
                // One pane for now; S4/S5a will route by id and drop stale ids.
                debug_assert_eq!(id, self.id);
                self.on_pty(event)
            }
            AppEvent::Cwd(cwd) => self.on_cwd(cwd),
        }
    }

    /// Cursor shape the outer terminal should show: a steady block in COPY
    /// (the viewport may be away from the child's cursor), otherwise whatever
    /// the child last requested.
    pub fn cursor_shape(&self) -> CursorShape {
        match self.input {
            InputMode::Copy(_) => CursorShape {
                kind: CursorKind::Block,
                blinking: false,
            },
            _ => self.pane.cursor_shape(),
        }
    }

    /// Release is always ignored. Repeat acts like Press where holding a key
    /// is meaningful (TERMINAL typing, COPY scrolling) and is dropped where a
    /// held key could trigger a one-shot decision (PREFIX, GROUP, CONFIRM).
    fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let repeat = match key.kind {
            KeyEventKind::Press => false,
            KeyEventKind::Repeat => true,
            KeyEventKind::Release => return Vec::new(),
        };
        if repeat
            && matches!(
                self.input,
                InputMode::Prefix | InputMode::Group(_) | InputMode::Confirm(_)
            )
        {
            return Vec::new();
        }
        self.dirty = true;
        match self.input {
            InputMode::Terminal => self.on_terminal_key(key),
            InputMode::Prefix => self.on_pending_key(PREFIX_TREE, &key),
            InputMode::Group(group) => self.on_pending_key(group.bindings, &key),
            InputMode::Copy(state) => self.on_copy_key(state, &key),
            InputMode::Resize => self.on_resize_key(),
            InputMode::Confirm(Confirm::Quit) => self.on_confirm_key(&key),
        }
    }

    fn on_terminal_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if PREFIX_KEY.matches(&key) {
            self.input = InputMode::Prefix;
            return Vec::new();
        }
        encode_key(key, self.pane.modes())
            .map(|bytes| vec![Effect::WritePty(self.id, bytes)])
            .unwrap_or_default()
    }

    /// Resolve a key against the pending table (the root or an open group).
    /// Actions beyond quit, copy and the literal prefix are no-ops until the
    /// slices that implement them land.
    fn on_pending_key(&mut self, table: &'static [Binding], key: &KeyEvent) -> Vec<Effect> {
        let step = prefix::lookup(table, key);
        self.input = match step {
            Step::Enter(group) => InputMode::Group(group),
            Step::Run(PrefixAction::RequestQuit) => InputMode::Confirm(Confirm::Quit),
            Step::Run(PrefixAction::EnterCopy) => InputMode::Copy(CopyState::default()),
            _ => InputMode::Terminal,
        };
        match step {
            Step::Run(PrefixAction::SendPrefixLiteral) => {
                vec![Effect::WritePty(self.id, vec![PREFIX_LITERAL])]
            }
            _ => Vec::new(),
        }
    }

    /// RESIZE has no keys yet and nothing enters it; leave it safely.
    fn on_resize_key(&mut self) -> Vec<Effect> {
        self.input = InputMode::Terminal;
        Vec::new()
    }

    fn on_confirm_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Char('y') && key.modifiers == KeyModifiers::NONE {
            return vec![Effect::Quit];
        }
        self.input = InputMode::Terminal;
        Vec::new()
    }

    fn on_copy_key(&mut self, mut state: CopyState, key: &KeyEvent) -> Vec<Effect> {
        match state.on_key(key) {
            CopyCommand::Move(motion) => {
                state.apply(motion, self.pane.scrollback_len(), self.focused_size().rows);
                self.pane.set_scrollback(state.offset);
                self.input = InputMode::Copy(state);
            }
            CopyCommand::Exit => {
                self.pane.set_scrollback(0);
                self.input = InputMode::Terminal;
            }
            CopyCommand::Ignore => self.input = InputMode::Copy(state),
        }
        Vec::new()
    }

    fn on_paste(&mut self, text: &str) -> Vec<Effect> {
        match self.input {
            InputMode::Terminal => vec![Effect::WritePty(
                self.id,
                encode_paste(text, self.pane.modes()),
            )],
            _ => Vec::new(),
        }
    }

    fn on_resize(&mut self, cols: u16, rows: u16) -> Vec<Effect> {
        self.screen = ScreenLayout::new(cols, rows);
        let size = self.focused_size();
        self.pane.resize(size);
        self.sync_copy_offset();
        self.dirty = true;
        vec![Effect::ResizePty(self.id, size)]
    }

    /// The emulator moves its own offset when output arrives while scrolled
    /// (it pins the viewport) and clamps it at the history cap. In COPY, the
    /// pane is the source of truth, so the state follows it.
    fn sync_copy_offset(&mut self) {
        if let InputMode::Copy(state) = &mut self.input {
            state.offset = self.pane.scrollback_offset();
        }
    }

    fn on_cwd(&mut self, cwd: PathBuf) -> Vec<Effect> {
        if self.cwd.as_ref() != Some(&cwd) {
            self.cwd = Some(cwd);
            self.dirty = true;
        }
        Vec::new()
    }

    fn on_pty(&mut self, event: PtyEvent) -> Vec<Effect> {
        match event {
            PtyEvent::Output(bytes) => {
                self.dirty = true;
                let reply = self.pane.feed(&bytes);
                self.sync_copy_offset();
                if reply.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::WritePty(self.id, reply)]
                }
            }
            PtyEvent::Exited => vec![Effect::Quit],
        }
    }
}

fn body_size(rect: Rect) -> PaneSize {
    PaneSize {
        rows: rect.height,
        cols: rect.width,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    #[default]
    Normal,
    AiChat,
    Metrics,
    Settings,
}

impl AppMode {
    pub const ALL: [AppMode; 4] = [Self::Normal, Self::AiChat, Self::Metrics, Self::Settings];

    /// Returns the next mode, wrapping from the last back to the first.
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::AiChat,
            Self::AiChat => Self::Metrics,
            Self::Metrics => Self::Settings,
            Self::Settings => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::AiChat => "AI CHAT",
            Self::Metrics => "METRICS",
            Self::Settings => "SETTINGS",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_cycles_through_all_modes_and_wraps() {
        assert_eq!(AppMode::Normal.next(), AppMode::AiChat);
        assert_eq!(AppMode::AiChat.next(), AppMode::Metrics);
        assert_eq!(AppMode::Metrics.next(), AppMode::Settings);
        assert_eq!(AppMode::Settings.next(), AppMode::Normal);
    }

    #[test]
    fn labels_are_uppercase_names() {
        let labels: Vec<_> = AppMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels, ["NORMAL", "AI CHAT", "METRICS", "SETTINGS"]);
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(AppMode::default(), AppMode::Normal);
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use crate::core::{
        copy::CopyState,
        layout::PaneIds,
        pane::{CursorKind, CursorShape, PaneSize},
        pty::PtyEvent,
    };

    fn app() -> App {
        App::new(80, 25)
    }

    fn key(c: char) -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn ctrl_space() -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL))
    }

    fn esc() -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    }

    fn in_prefix() -> App {
        let mut a = app();
        a.update(ctrl_space());
        a
    }

    fn in_copy() -> App {
        let mut a = in_prefix();
        a.update(key('['));
        a
    }

    fn with_history(a: &mut App, lines: usize) {
        for i in 0..lines {
            a.update(AppEvent::Pty(
                PaneId::FIRST,
                PtyEvent::Output(format!("line{i}\r\n").into_bytes()),
            ));
        }
    }

    fn is_copy(a: &App) -> bool {
        matches!(a.input(), InputMode::Copy(_))
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn an_event_for_a_foreign_pane_id_trips_the_single_pane_assertion() {
        let mut ids = PaneIds::default();
        let (_first, foreign) = (ids.alloc(), ids.alloc());
        app().update(AppEvent::Pty(foreign, PtyEvent::Output(b"x".to_vec())));
    }

    #[test]
    fn a_new_app_has_a_24_row_body_and_a_statusline_row() {
        let a = app();
        assert_eq!(
            a.screen(),
            ScreenLayout {
                body: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24
                },
                status: Rect {
                    x: 0,
                    y: 24,
                    width: 80,
                    height: 1
                },
            }
        );
        assert_eq!(a.focused_size(), PaneSize { rows: 24, cols: 80 });
    }

    #[test]
    fn resize_moves_the_screen_layout() {
        let mut a = app();
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        let screen = a.screen();
        assert_eq!((screen.body.width, screen.body.height), (100, 39));
        assert_eq!((screen.status.y, screen.status.width), (39, 100));
    }

    #[test]
    fn a_tiny_terminal_keeps_a_1x1_body() {
        let a = App::new(0, 0);
        assert_eq!((a.screen().body.width, a.screen().body.height), (1, 1));
        assert_eq!(a.focused_size(), PaneSize { rows: 1, cols: 1 });
    }

    #[test]
    fn the_initial_spawn_describes_the_shell_in_the_focused_pane() {
        let a = app().with_env(Some("/bin/zsh"), None, "/work".into());
        let (id, spec) = a.initial_spawn();
        assert_eq!(id, a.focused());
        assert_eq!(spec.program, PathBuf::from("/bin/zsh"));
        assert_eq!(spec.cwd, PathBuf::from("/work"));
        assert_eq!(spec.size, PaneSize { rows: 24, cols: 80 });
    }

    #[test]
    fn the_initial_spawn_falls_back_to_sh_in_the_root() {
        let (_, spec) = app().initial_spawn();
        assert_eq!(spec.program, PathBuf::from("/bin/sh"));
        assert_eq!(spec.cwd, PathBuf::from("/"));
    }

    #[test]
    fn take_dirty_reports_once_until_something_changes() {
        let mut a = app();
        assert!(a.take_dirty(), "a new app needs a first draw");
        assert!(!a.take_dirty());
        a.update(key('l'));
        assert!(a.take_dirty());
        assert!(!a.take_dirty());
    }

    #[test]
    fn effects_name_the_focused_pane() {
        let mut a = app();
        let id = a.focused();
        assert_eq!(
            a.update(key('l')),
            vec![Effect::WritePty(id, b"l".to_vec())]
        );
    }

    #[test]
    fn initial_mode_is_terminal() {
        assert_eq!(app().input(), InputMode::Terminal);
    }

    #[test]
    fn terminal_passes_keys_to_the_pty() {
        let mut a = app();
        assert_eq!(
            a.update(key('l')),
            vec![Effect::WritePty(PaneId::FIRST, b"l".to_vec())]
        );
        assert_eq!(a.input(), InputMode::Terminal);
        let enter = AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            a.update(enter),
            vec![Effect::WritePty(PaneId::FIRST, b"\r".to_vec())]
        );
    }

    #[test]
    fn unencodable_keys_write_nothing() {
        let mut a = app();
        let f20 = AppEvent::Key(KeyEvent::new(KeyCode::F(20), KeyModifiers::NONE));
        assert_eq!(a.update(f20), vec![]);
    }

    #[test]
    fn terminal_encodes_with_the_pane_modes() {
        let mut a = app();
        let up = AppEvent::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            a.update(up.clone()),
            vec![Effect::WritePty(PaneId::FIRST, b"\x1b[A".to_vec())]
        );
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1h".to_vec()),
        ));
        assert_eq!(
            a.update(up),
            vec![Effect::WritePty(PaneId::FIRST, b"\x1bOA".to_vec())]
        );
    }

    #[test]
    fn prefix_key_enters_prefix_without_writing() {
        let mut a = app();
        assert_eq!(a.update(ctrl_space()), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn prefix_twice_sends_a_literal_nul() {
        let mut a = in_prefix();
        assert_eq!(
            a.update(ctrl_space()),
            vec![Effect::WritePty(PaneId::FIRST, vec![0x00])]
        );
        assert_eq!(a.input(), InputMode::Terminal);
    }

    #[test]
    fn prefix_cancel_and_unmapped_keys_are_swallowed() {
        for ev in [esc(), key('z')] {
            let mut a = in_prefix();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_q_asks_for_confirmation() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('q')), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    fn group_label(a: &App) -> &'static str {
        match a.input() {
            InputMode::Group(group) => group.label,
            other => panic!("expected a group, got {other:?}"),
        }
    }

    fn in_group(c: char) -> App {
        let mut a = in_prefix();
        a.update(key(c));
        a
    }

    #[test]
    fn group_keys_open_their_group_without_effects() {
        for (c, label) in [('w', "WINDOW"), ('t', "TAB"), ('g', "GO"), ('b', "BUFFER")] {
            let mut a = in_prefix();
            assert_eq!(a.update(key(c)), vec![], "{c}");
            assert_eq!(group_label(&a), label, "{c}");
        }
    }

    #[test]
    fn a_binding_in_a_group_returns_to_terminal_with_no_effect_yet() {
        for (g, c) in [('w', 'v'), ('w', 'z'), ('t', 'n'), ('g', 'b'), ('b', '3')] {
            let mut a = in_group(g);
            assert_eq!(a.update(key(c)), vec![], "{g} {c}");
            assert_eq!(a.input(), InputMode::Terminal, "{g} {c}");
        }
    }

    #[test]
    fn group_esc_unmapped_and_prefix_key_cancel_and_are_swallowed() {
        for ev in [esc(), key('x'), ctrl_space()] {
            let mut a = in_group('w');
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn question_mark_returns_to_terminal_with_no_effect() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('?')), vec![]);
        assert_eq!(a.input(), InputMode::Terminal);
    }

    #[test]
    fn root_focus_keys_return_to_terminal_with_no_effect_yet() {
        for c in ['h', 'j', 'k', 'l'] {
            let mut a = in_prefix();
            assert_eq!(a.update(key(c)), vec![], "{c}");
            assert_eq!(a.input(), InputMode::Terminal, "{c}");
        }
    }

    #[test]
    fn repeat_is_ignored_in_group_and_keeps_the_group_pending() {
        let mut a = in_group('w');
        assert_eq!(a.update(repeat('v')), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
    }

    #[test]
    fn release_is_ignored_in_group() {
        let mut a = in_group('w');
        let up = kinded(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(a.update(up), vec![]);
        assert_eq!(group_label(&a), "WINDOW");
    }

    #[test]
    fn input_mode_labels_cover_group_resize_and_confirm() {
        assert_eq!(in_group('t').input().label(), "TAB");
        assert_eq!(InputMode::Resize.label(), "RESIZE");
        assert_eq!(InputMode::Confirm(Confirm::Quit).label(), "Quit? (y/n)");
    }

    // Deliberate (design ADR 12, spec "SHIFT MUST be ignored for character
    // keys"): terminals report a char with or without SHIFT.
    #[test]
    fn shift_is_ignored_for_character_keys_in_the_prefix_flow() {
        let mut a = app();
        let ctrl_shift_space = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char(' '),
            ctrl_shift_space,
        )));
        assert_eq!(a.input(), InputMode::Prefix);

        let shift_q = AppEvent::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT));
        assert_eq!(a.update(shift_q), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    #[test]
    fn confirm_quit_y_quits() {
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.update(key('y')), vec![Effect::Quit]);
    }

    #[test]
    fn confirm_quit_declined_by_n_esc_or_other_keys() {
        for ev in [key('n'), esc(), key('x')] {
            let mut a = in_prefix();
            a.update(key('q'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn confirm_quit_uppercase_y_declines() {
        let shift_y = AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT));
        let caps_y = AppEvent::Key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE));
        for ev in [shift_y, caps_y] {
            let mut a = in_prefix();
            a.update(key('q'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_open_bracket_enters_copy_without_writing() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('[')), vec![]);
        assert_eq!(a.input(), InputMode::Copy(CopyState::default()));
    }

    #[test]
    fn paste_goes_to_the_pty_only_in_terminal() {
        let mut a = app();
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(PaneId::FIRST, b"hi".to_vec())]
        );
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?2004h".to_vec()),
        ));
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(
                PaneId::FIRST,
                b"\x1b[200~hi\x1b[201~".to_vec()
            )]
        );
        for mut a in [in_prefix(), in_copy()] {
            assert_eq!(a.update(AppEvent::Paste("hi".into())), vec![]);
        }
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.update(AppEvent::Paste("hi".into())), vec![]);
    }

    #[test]
    fn resize_emits_resize_pty_in_every_mode() {
        let mut confirm = in_prefix();
        confirm.update(key('q'));
        for mut a in [app(), in_prefix(), in_copy(), confirm] {
            let effects = a.update(AppEvent::Resize {
                cols: 100,
                rows: 40,
            });
            assert_eq!(
                effects,
                vec![Effect::ResizePty(
                    PaneId::FIRST,
                    PaneSize {
                        rows: 39,
                        cols: 100
                    }
                )]
            );
        }
    }

    #[test]
    fn resize_resizes_the_emulator_too() {
        let mut a = app();
        a.update(AppEvent::Resize {
            cols: 100,
            rows: 40,
        });
        assert!(a.focused_pane().cell(38, 99).is_some());
        assert!(a.focused_pane().cell(39, 0).is_none());
    }

    #[test]
    fn pane_size_reserves_the_statusline_row_with_a_1x1_minimum() {
        assert_eq!(
            pane_size(100, 40),
            PaneSize {
                rows: 39,
                cols: 100
            }
        );
        assert_eq!(pane_size(80, 2), PaneSize { rows: 1, cols: 80 });
        assert_eq!(pane_size(1, 1), PaneSize { rows: 1, cols: 1 });
        assert_eq!(pane_size(0, 0), PaneSize { rows: 1, cols: 1 });
    }

    #[test]
    fn pty_exit_quits() {
        let mut a = app();
        assert_eq!(
            a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited)),
            vec![Effect::Quit]
        );
    }

    #[test]
    fn pty_event_for_the_current_pane_is_handled() {
        let mut a = app();
        let id = a.focused();
        assert_eq!(
            a.update(AppEvent::Pty(id, PtyEvent::Output(b"x".to_vec()))),
            vec![]
        );
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "x");
    }

    #[test]
    fn pty_output_is_parsed_and_marks_dirty() {
        let mut a = app();
        a.take_dirty();
        assert_eq!(
            a.update(AppEvent::Pty(
                PaneId::FIRST,
                PtyEvent::Output(b"hi".to_vec())
            )),
            vec![]
        );
        assert_eq!(a.focused_pane().cell(0, 0).unwrap().text, "h");
        assert!(a.take_dirty());
    }

    #[test]
    fn pty_output_with_a_query_writes_the_reply_back() {
        let mut a = app();
        let effects = a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[5n".to_vec()),
        ));
        assert_eq!(
            effects,
            vec![Effect::WritePty(PaneId::FIRST, b"\x1b[0n".to_vec())]
        );
    }

    fn kinded(code: KeyCode, mods: KeyModifiers, kind: KeyEventKind) -> AppEvent {
        AppEvent::Key(KeyEvent::new_with_kind(code, mods, kind))
    }

    fn repeat(c: char) -> AppEvent {
        kinded(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Repeat)
    }

    #[test]
    fn release_key_events_are_ignored_in_every_mode() {
        let release = |c| kinded(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Release);
        let mut a = app();
        assert_eq!(a.update(release('l')), vec![]);
        let mut a = in_prefix();
        assert_eq!(a.update(release('[')), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
        let mut a = app();
        a.update(kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        ));
        assert_eq!(
            a.input(),
            InputMode::Terminal,
            "release must not enter PREFIX"
        );
    }

    #[test]
    fn repeat_in_terminal_writes_the_encoded_bytes() {
        let mut a = app();
        assert_eq!(
            a.update(repeat('l')),
            vec![Effect::WritePty(PaneId::FIRST, b"l".to_vec())]
        );
    }

    #[test]
    fn repeat_of_k_in_copy_moves_the_view() {
        let mut a = in_copy();
        with_history(&mut a, 40);
        a.update(repeat('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
        a.update(repeat('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 2);
    }

    #[test]
    fn repeat_in_prefix_does_nothing_and_stays_in_prefix() {
        let mut a = in_prefix();
        assert_eq!(a.update(repeat('[')), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn held_prefix_key_enters_prefix_once_and_repeats_are_dropped() {
        let mut a = app();
        a.update(ctrl_space());
        let held = kinded(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        );
        assert_eq!(a.update(held), vec![]);
        assert_eq!(a.input(), InputMode::Prefix);
    }

    #[test]
    fn repeat_of_y_in_confirm_quit_does_not_quit() {
        let mut a = in_prefix();
        a.update(key('q'));
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
        assert_eq!(a.update(repeat('y')), vec![]);
        assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
    }

    #[test]
    fn copy_swallows_keys() {
        for ev in [
            key('j'),
            key('x'),
            AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ] {
            let mut a = in_copy();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert!(is_copy(&a), "{ev:?}");
        }
    }

    #[test]
    fn copy_motions_scroll_the_pane() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        let history = a.focused_pane().scrollback_len();
        assert!(history > 30);
        a.update(key('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), 1);
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(a.focused_pane().scrollback_offset(), 13, "half of 24 rows");
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.focused_pane().scrollback_offset(), history);
        a.update(key('G'));
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
        a.update(key('j'));
        assert_eq!(
            a.focused_pane().scrollback_offset(),
            0,
            "clamped at the bottom"
        );
    }

    #[test]
    fn copy_exit_returns_to_terminal_at_the_bottom() {
        for ev in [key('q'), key('i'), esc()] {
            let mut a = in_copy();
            with_history(&mut a, 60);
            a.update(key('k'));
            a.update(key('k'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input(), InputMode::Terminal, "{ev:?}");
            assert_eq!(a.focused_pane().scrollback_offset(), 0, "{ev:?}");
        }
    }

    #[test]
    fn copy_on_the_alternate_screen_stays_at_zero() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        for ev in [key('k'), key('j'), key('G')] {
            a.update(ev);
            assert_eq!(a.focused_pane().scrollback_offset(), 0);
        }
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.focused_pane().scrollback_offset(), 0);
    }

    fn copy_offset(a: &App) -> usize {
        match a.input() {
            InputMode::Copy(state) => state.offset,
            other => panic!("not in COPY: {other:?}"),
        }
    }

    #[test]
    fn copy_resize_clamps_the_offset_and_keeps_it_in_sync_with_the_pane() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        a.update(key('g'));
        a.update(key('g'));
        let before = copy_offset(&a);
        a.update(AppEvent::Resize { cols: 80, rows: 60 });
        let after = copy_offset(&a);
        assert!(after <= a.focused_pane().scrollback_len());
        assert_eq!(after, a.focused_pane().scrollback_offset());
        a.update(key('k'));
        assert!(copy_offset(&a) <= a.focused_pane().scrollback_len());
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        assert!(before > 0);
    }

    #[test]
    fn copy_offset_follows_the_pane_when_output_arrives_while_scrolled() {
        let mut a = in_copy();
        with_history(&mut a, 40);
        for _ in 0..10 {
            a.update(key('k'));
        }
        assert_eq!(copy_offset(&a), 10);
        with_history(&mut a, 5);
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        let anchored = a.focused_pane().scrollback_offset();
        assert!(anchored > 10, "the emulator pins the viewport");
        a.update(key('j'));
        assert_eq!(a.focused_pane().scrollback_offset(), anchored - 1);
        assert_eq!(copy_offset(&a), anchored - 1);
        a.update(key('k'));
        a.update(key('k'));
        assert_eq!(a.focused_pane().scrollback_offset(), anchored + 1);
        assert_eq!(copy_offset(&a), anchored + 1);
    }

    #[test]
    fn copy_output_past_the_scrollback_cap_does_not_panic_and_stays_in_sync() {
        let mut a = in_copy();
        with_history(&mut a, SCROLLBACK_LINES + 100);
        a.update(key('g'));
        a.update(key('g'));
        with_history(&mut a, 50);
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        a.update(key('k'));
        a.update(key('j'));
        assert_eq!(copy_offset(&a), a.focused_pane().scrollback_offset());
        assert!(a.focused_pane().scrollback_offset() <= a.focused_pane().scrollback_len());
    }

    fn visible_text(a: &App) -> Vec<String> {
        (0..a.focused_size().rows)
            .map(|row| {
                (0..a.focused_size().cols)
                    .filter_map(|col| a.focused_pane().cell(row, col))
                    .map(|cell| cell.text)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    // Spec: output during COPY keeps the same content visible. The anchoring
    // comes from the emulator pinning the viewport plus `sync_copy_offset`.
    #[test]
    fn output_during_copy_keeps_the_same_content_visible() {
        let mut a = in_copy();
        with_history(&mut a, 60);
        for _ in 0..10 {
            a.update(key('k'));
        }
        let before = visible_text(&a);
        assert_eq!(before.first().map(String::as_str), Some("line27"));
        with_history(&mut a, 5);
        assert_eq!(visible_text(&a), before);
        assert!(is_copy(&a));
        a.update(key('j'));
        assert_eq!(visible_text(&a)[0], "line28");
    }

    // Spec: entering COPY on the alternate screen shows the current screen.
    #[test]
    fn entering_copy_on_the_alternate_screen_shows_the_current_screen() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[?1049h\x1b[HALT SCREEN".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        assert_eq!(visible_text(&a)[0], "ALT SCREEN");
    }

    fn cwd_event(path: &str) -> AppEvent {
        AppEvent::Cwd(PathBuf::from(path))
    }

    #[test]
    fn cwd_event_updates_the_cwd_and_marks_dirty() {
        let mut a = app();
        a.take_dirty();
        assert_eq!(a.update(cwd_event("/tmp")), vec![]);
        assert_eq!(a.cwd_label(), "/tmp");
        assert!(a.take_dirty());
    }

    #[test]
    fn unchanged_cwd_does_not_mark_dirty() {
        let mut a = app();
        a.update(cwd_event("/tmp"));
        a.take_dirty();
        a.update(cwd_event("/tmp"));
        assert!(!a.take_dirty());
    }

    #[test]
    fn cwd_label_is_empty_until_the_first_cwd() {
        assert_eq!(app().cwd_label(), "");
    }

    // The runtime only sends `Cwd` when the lookup succeeds, so a failed
    // lookup is the absence of an event: the previous value stays.
    #[test]
    fn cwd_is_retained_when_no_new_cwd_arrives() {
        let mut a = app();
        a.update(cwd_event("/tmp"));
        a.update(key('l'));
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"x".to_vec()),
        ));
        assert_eq!(a.cwd_label(), "/tmp");
    }

    #[test]
    fn home_is_shown_as_a_tilde() {
        let mut a = app().with_env(
            Some("/bin/zsh"),
            Some("/home/ana".into()),
            "/home/ana".into(),
        );
        assert_eq!(a.cwd_label(), "~");
        a.update(cwd_event("/home/ana/src/app"));
        assert_eq!(a.cwd_label(), "~/src/app");
        a.update(cwd_event("/home/anabel"));
        assert_eq!(
            a.cwd_label(),
            "/home/anabel",
            "prefix must be a path prefix"
        );
        a.update(cwd_event("/tmp"));
        assert_eq!(a.cwd_label(), "/tmp");
    }

    #[test]
    fn a_root_or_missing_home_never_produces_a_tilde() {
        for home in [None, Some(PathBuf::from("/")), Some(PathBuf::new())] {
            let a = app().with_env(None, home, "/usr/bin".into());
            assert_eq!(a.cwd_label(), "/usr/bin");
        }
    }

    #[test]
    fn shell_name_is_the_basename_of_the_shell_with_an_sh_fallback() {
        assert_eq!(shell_basename(Some("/bin/zsh")), "zsh");
        assert_eq!(shell_basename(Some("fish")), "fish");
        assert_eq!(shell_basename(Some("/usr/bin/")), "bin");
        for shell in [None, Some(""), Some("/")] {
            assert_eq!(shell_basename(shell), "sh", "{shell:?}");
        }
        assert_eq!(app().shell_name(), "sh");
        let a = app().with_env(Some("/bin/zsh"), None, "/".into());
        assert_eq!(a.shell_name(), "zsh");
    }

    fn shape(kind: CursorKind, blinking: bool) -> CursorShape {
        CursorShape { kind, blinking }
    }

    #[test]
    fn cursor_shape_follows_the_child_in_terminal_and_prefix() {
        let mut a = app();
        assert_eq!(a.cursor_shape(), CursorShape::default());
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[6 q".to_vec()),
        ));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
        a.update(ctrl_space());
        assert_eq!(a.input(), InputMode::Prefix);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit() {
        let mut a = app();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[6 q".to_vec()),
        ));
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn child_shape_requests_during_copy_do_not_leak_until_exit() {
        let mut a = in_copy();
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(b"\x1b[3 q".to_vec()),
        ));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Underline, true));
    }
}
