use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::{
    copy::{CopyCommand, CopyState},
    keys::{encode_key, encode_paste},
    pane::{CursorKind, CursorShape, Pane, PaneSize},
    prefix::{self, PREFIX_KEY, PrefixAction},
    pty::PtyEvent,
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
    /// Waiting for `y` to quit.
    ConfirmQuit,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    Pty(PtyEvent),
}

/// Side effects requested by `App::update`, executed by the runtime.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    WritePty(Vec<u8>),
    ResizePty(PaneSize),
    Quit,
}

/// Pane size for a terminal of `cols` x `rows`: the bottom row is the
/// statusline, and the pane is never smaller than 1x1.
pub fn pane_size(cols: u16, rows: u16) -> PaneSize {
    PaneSize {
        rows: rows.saturating_sub(1).max(1),
        cols: cols.max(1),
    }
}

/// Owns the emulator and the input state. Pure: it never touches IO.
pub struct App {
    pub pane: Pane,
    pub input: InputMode,
    pub dirty: bool,
    size: PaneSize,
}

impl App {
    pub fn new(size: PaneSize) -> Self {
        Self {
            pane: Pane::new(size, SCROLLBACK_LINES),
            input: InputMode::default(),
            dirty: true,
            size,
        }
    }

    pub fn update(&mut self, event: AppEvent) -> Vec<Effect> {
        match event {
            AppEvent::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Key(_) => Vec::new(),
            AppEvent::Paste(text) => self.on_paste(&text),
            AppEvent::Resize { cols, rows } => self.on_resize(cols, rows),
            AppEvent::Pty(event) => self.on_pty(event),
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

    fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        self.dirty = true;
        match self.input {
            InputMode::Terminal => self.on_terminal_key(key),
            InputMode::Prefix => self.on_prefix_key(&key),
            InputMode::Copy(state) => self.on_copy_key(state, &key),
            InputMode::ConfirmQuit => self.on_confirm_key(&key),
        }
    }

    fn on_terminal_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if PREFIX_KEY.matches(&key) {
            self.input = InputMode::Prefix;
            return Vec::new();
        }
        encode_key(key, self.pane.modes())
            .map(|bytes| vec![Effect::WritePty(bytes)])
            .unwrap_or_default()
    }

    fn on_prefix_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        let action = prefix::lookup(key);
        self.input = match action {
            Some(PrefixAction::RequestQuit) => InputMode::ConfirmQuit,
            Some(PrefixAction::EnterCopy) => InputMode::Copy(CopyState::default()),
            Some(PrefixAction::SendPrefixLiteral) | None => InputMode::Terminal,
        };
        match action {
            Some(PrefixAction::SendPrefixLiteral) => vec![Effect::WritePty(vec![PREFIX_LITERAL])],
            _ => Vec::new(),
        }
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
                state.apply(motion, self.pane.scrollback_len(), self.size.rows);
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
            InputMode::Terminal => vec![Effect::WritePty(encode_paste(text, self.pane.modes()))],
            _ => Vec::new(),
        }
    }

    fn on_resize(&mut self, cols: u16, rows: u16) -> Vec<Effect> {
        self.size = pane_size(cols, rows);
        self.pane.resize(self.size);
        if let InputMode::Copy(state) = &mut self.input {
            state.offset = state.offset.min(self.pane.scrollback_len());
            self.pane.set_scrollback(state.offset);
        }
        self.sync_copy_offset();
        self.dirty = true;
        vec![Effect::ResizePty(self.size)]
    }

    /// The emulator moves its own offset when output arrives while scrolled
    /// (it pins the viewport) and clamps it at the history cap. In COPY, the
    /// pane is the source of truth, so the state follows it.
    fn sync_copy_offset(&mut self) {
        if let InputMode::Copy(state) = &mut self.input {
            state.offset = self.pane.scrollback_offset();
        }
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
                    vec![Effect::WritePty(reply)]
                }
            }
            PtyEvent::Exited => vec![Effect::Quit],
        }
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
        pane::{CursorKind, CursorShape, PaneSize},
        pty::PtyEvent,
    };

    fn app() -> App {
        App::new(PaneSize { rows: 24, cols: 80 })
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
            a.update(AppEvent::Pty(PtyEvent::Output(
                format!("line{i}\r\n").into_bytes(),
            )));
        }
    }

    fn is_copy(a: &App) -> bool {
        matches!(a.input, InputMode::Copy(_))
    }

    #[test]
    fn initial_mode_is_terminal() {
        assert_eq!(app().input, InputMode::Terminal);
    }

    #[test]
    fn terminal_passes_keys_to_the_pty() {
        let mut a = app();
        assert_eq!(a.update(key('l')), vec![Effect::WritePty(b"l".to_vec())]);
        assert_eq!(a.input, InputMode::Terminal);
        let enter = AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.update(enter), vec![Effect::WritePty(b"\r".to_vec())]);
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
            vec![Effect::WritePty(b"\x1b[A".to_vec())]
        );
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[?1h".to_vec())));
        assert_eq!(a.update(up), vec![Effect::WritePty(b"\x1bOA".to_vec())]);
    }

    #[test]
    fn prefix_key_enters_prefix_without_writing() {
        let mut a = app();
        assert_eq!(a.update(ctrl_space()), vec![]);
        assert_eq!(a.input, InputMode::Prefix);
    }

    #[test]
    fn prefix_twice_sends_a_literal_nul() {
        let mut a = in_prefix();
        assert_eq!(a.update(ctrl_space()), vec![Effect::WritePty(vec![0x00])]);
        assert_eq!(a.input, InputMode::Terminal);
    }

    #[test]
    fn prefix_cancel_and_unmapped_keys_are_swallowed() {
        for ev in [esc(), key('z')] {
            let mut a = in_prefix();
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input, InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_q_asks_for_confirmation() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('q')), vec![]);
        assert_eq!(a.input, InputMode::ConfirmQuit);
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
            assert_eq!(a.input, InputMode::Terminal, "{ev:?}");
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
            assert_eq!(a.input, InputMode::Terminal, "{ev:?}");
        }
    }

    #[test]
    fn prefix_open_bracket_enters_copy_without_writing() {
        let mut a = in_prefix();
        assert_eq!(a.update(key('[')), vec![]);
        assert_eq!(a.input, InputMode::Copy(CopyState::default()));
    }

    #[test]
    fn paste_goes_to_the_pty_only_in_terminal() {
        let mut a = app();
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(b"hi".to_vec())]
        );
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[?2004h".to_vec())));
        assert_eq!(
            a.update(AppEvent::Paste("hi".into())),
            vec![Effect::WritePty(b"\x1b[200~hi\x1b[201~".to_vec())]
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
                vec![Effect::ResizePty(PaneSize {
                    rows: 39,
                    cols: 100
                })]
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
        assert!(a.pane.cell(38, 99).is_some());
        assert!(a.pane.cell(39, 0).is_none());
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
            a.update(AppEvent::Pty(PtyEvent::Exited)),
            vec![Effect::Quit]
        );
    }

    #[test]
    fn pty_output_is_parsed_and_marks_dirty() {
        let mut a = app();
        a.dirty = false;
        assert_eq!(
            a.update(AppEvent::Pty(PtyEvent::Output(b"hi".to_vec()))),
            vec![]
        );
        assert_eq!(a.pane.cell(0, 0).unwrap().text, "h");
        assert!(a.dirty);
    }

    #[test]
    fn pty_output_with_a_query_writes_the_reply_back() {
        let mut a = app();
        let effects = a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[5n".to_vec())));
        assert_eq!(effects, vec![Effect::WritePty(b"\x1b[0n".to_vec())]);
    }

    #[test]
    fn non_press_key_events_are_ignored() {
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut a = app();
            let ev = AppEvent::Key(KeyEvent::new_with_kind(
                KeyCode::Char('l'),
                KeyModifiers::NONE,
                kind,
            ));
            assert_eq!(a.update(ev), vec![], "{kind:?}");
        }
        let mut a = app();
        let release = KeyEvent::new_with_kind(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        );
        a.update(AppEvent::Key(release));
        assert_eq!(
            a.input,
            InputMode::Terminal,
            "release must not enter PREFIX"
        );
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
        let history = a.pane.scrollback_len();
        assert!(history > 30);
        a.update(key('k'));
        assert_eq!(a.pane.scrollback_offset(), 1);
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(a.pane.scrollback_offset(), 13, "half of 24 rows");
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.pane.scrollback_offset(), history);
        a.update(key('G'));
        assert_eq!(a.pane.scrollback_offset(), 0);
        a.update(key('j'));
        assert_eq!(a.pane.scrollback_offset(), 0, "clamped at the bottom");
    }

    #[test]
    fn copy_exit_returns_to_terminal_at_the_bottom() {
        for ev in [key('q'), key('i'), esc()] {
            let mut a = in_copy();
            with_history(&mut a, 60);
            a.update(key('k'));
            a.update(key('k'));
            assert_eq!(a.update(ev.clone()), vec![], "{ev:?}");
            assert_eq!(a.input, InputMode::Terminal, "{ev:?}");
            assert_eq!(a.pane.scrollback_offset(), 0, "{ev:?}");
        }
    }

    #[test]
    fn copy_on_the_alternate_screen_stays_at_zero() {
        let mut a = app();
        with_history(&mut a, 60);
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[?1049h".to_vec())));
        a.update(ctrl_space());
        a.update(key('['));
        assert!(is_copy(&a));
        for ev in [key('k'), key('j'), key('G')] {
            a.update(ev);
            assert_eq!(a.pane.scrollback_offset(), 0);
        }
        a.update(key('g'));
        a.update(key('g'));
        assert_eq!(a.pane.scrollback_offset(), 0);
    }

    fn copy_offset(a: &App) -> usize {
        match a.input {
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
        assert!(after <= a.pane.scrollback_len());
        assert_eq!(after, a.pane.scrollback_offset());
        a.update(key('k'));
        assert!(copy_offset(&a) <= a.pane.scrollback_len());
        assert_eq!(copy_offset(&a), a.pane.scrollback_offset());
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
        assert_eq!(copy_offset(&a), a.pane.scrollback_offset());
        let anchored = a.pane.scrollback_offset();
        assert!(anchored > 10, "the emulator pins the viewport");
        a.update(key('j'));
        assert_eq!(a.pane.scrollback_offset(), anchored - 1);
        assert_eq!(copy_offset(&a), anchored - 1);
        a.update(key('k'));
        a.update(key('k'));
        assert_eq!(a.pane.scrollback_offset(), anchored + 1);
        assert_eq!(copy_offset(&a), anchored + 1);
    }

    #[test]
    fn copy_output_past_the_scrollback_cap_does_not_panic_and_stays_in_sync() {
        let mut a = in_copy();
        with_history(&mut a, SCROLLBACK_LINES + 100);
        a.update(key('g'));
        a.update(key('g'));
        with_history(&mut a, 50);
        assert_eq!(copy_offset(&a), a.pane.scrollback_offset());
        a.update(key('k'));
        a.update(key('j'));
        assert_eq!(copy_offset(&a), a.pane.scrollback_offset());
        assert!(a.pane.scrollback_offset() <= a.pane.scrollback_len());
    }

    fn shape(kind: CursorKind, blinking: bool) -> CursorShape {
        CursorShape { kind, blinking }
    }

    #[test]
    fn cursor_shape_follows_the_child_in_terminal_and_prefix() {
        let mut a = app();
        assert_eq!(a.cursor_shape(), CursorShape::default());
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[6 q".to_vec())));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
        a.update(ctrl_space());
        assert_eq!(a.input, InputMode::Prefix);
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit() {
        let mut a = app();
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[6 q".to_vec())));
        a.update(ctrl_space());
        a.update(key('['));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Bar, false));
    }

    #[test]
    fn child_shape_requests_during_copy_do_not_leak_until_exit() {
        let mut a = in_copy();
        a.update(AppEvent::Pty(PtyEvent::Output(b"\x1b[3 q".to_vec())));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Block, false));
        a.update(key('q'));
        assert_eq!(a.cursor_shape(), shape(CursorKind::Underline, true));
    }
}
