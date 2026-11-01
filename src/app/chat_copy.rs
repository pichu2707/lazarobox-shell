//! COPY on a chat pane: a cursor and a selection over the transcript rows,
//! as in a shell pane, copied as clean text. The view stays in
//! `ChatState::scroll`; the mode carries the cursor, the selection and the
//! offset (`ChatCopy`), synced with the view before every use.

use super::*;
use crate::core::{
    ai::chat::{
        copy::{ChatCopyCommand, ChatMarks, geometry},
        transcript::{self, Scroll},
        wrap::text_width,
    },
    copy::{CopyMotion, Geometry, join},
    mouse::WheelDir,
};

/// Shown when COPY is asked on a chat pane that has no transcript to scroll.
const NOTHING_TO_SCROLL: &str = "nothing to scroll here";

impl App {
    /// `Prefix [` on a chat pane: chat COPY over a conversation, with the
    /// view where it is and the cursor on its last row; otherwise TERMINAL
    /// with a notice (the list, the editors, the summary draft and guidance
    /// with no transcript have nothing to scroll).
    pub(super) fn enter_chat_copy(&mut self) -> InputMode {
        let focus = self.tab().focus;
        match self.chat_copy_view(focus) {
            Some((geo, scroll)) if self.chat_scrollable(focus) => {
                InputMode::ChatCopy(ChatCopy::enter(geo, scroll))
            }
            _ => {
                self.notice = Some(NOTHING_TO_SCROLL.to_string());
                InputMode::Terminal
            }
        }
    }

    /// The chat COPY cursor and selection of the focused chat, synced with
    /// its transcript; `None` outside chat COPY.
    pub fn chat_copy_marks(&self) -> Option<ChatMarks> {
        let InputMode::ChatCopy(mut state) = self.input else {
            return None;
        };
        let (geo, scroll) = self.chat_copy_view(self.tab().focus)?;
        state.sync(geo, scroll);
        Some(ChatMarks {
            cursor: state.state.cursor,
            span: state.state.span(geo.cols),
        })
    }

    /// Whether chat `id` shows a transcript view.
    pub(super) fn chat_scrollable(&self, id: PaneId) -> bool {
        if self.list_open(id)
            || self.cost_open(id)
            || self.tag_editor_open(id)
            || self.memory_view_open(id)
        {
            return false;
        }
        let Some(state) = self.panes.get(&id) else {
            return false;
        };
        let Some(chat) = state.chat_state() else {
            return false;
        };
        !chat.lines(usize::from(state.size.cols)).is_empty() || self.chat_guidance(id).is_none()
    }

    /// The transcript of chat `id` as COPY sees it, and where its view is.
    fn chat_copy_view(&self, id: PaneId) -> Option<(Geometry, Scroll)> {
        let state = self.panes.get(&id)?;
        let chat = state.chat_state()?;
        let cols = state.size.cols;
        let total = chat.lines(usize::from(cols)).len();
        Some((geometry(total, self.chat_viewport(id), cols), chat.scroll()))
    }

    /// Syncs chat COPY with the focused transcript after a relayout: a new
    /// width drops the selection.
    pub(super) fn sync_chat_copy(&mut self) {
        let InputMode::ChatCopy(mut state) = self.input else {
            return;
        };
        if let Some((geo, scroll)) = self.chat_copy_view(self.tab().focus) {
            state.sync(geo, scroll);
            self.input = InputMode::ChatCopy(state);
        }
    }

    /// A key in chat COPY. Nothing here reaches the composer or a PTY.
    pub(super) fn on_chat_copy_key(
        &mut self,
        mut state: ChatCopy,
        key: &KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        let focus = self.tab().focus;
        if let Some((geo, scroll)) = self.chat_copy_view(focus) {
            state.sync(geo, scroll);
        }
        let command = state.on_key(key, repeat);
        self.input = InputMode::ChatCopy(state);
        match command {
            ChatCopyCommand::Ignore => {}
            ChatCopyCommand::Prefix => self.input = InputMode::Prefix,
            ChatCopyCommand::Exit => {
                self.leave_chat_copy(focus);
                self.hold_exit(key);
            }
            ChatCopyCommand::Move(motion) => self.move_chat_copy(state, motion),
            ChatCopyCommand::Yank => {
                let effects = self.yank_chat_copy(state);
                if self.input == InputMode::Terminal {
                    self.hold_exit(key);
                }
                return effects;
            }
            ChatCopyCommand::Answer => return self.chat_yank(false),
            ChatCopyCommand::CodeBlock => return self.chat_yank(true),
        }
        Vec::new()
    }

    /// Ends chat COPY (Esc, `q` or a copy): the mode is TERMINAL and
    /// `pane`'s transcript follows the bottom again. Focus and tab changes
    /// need the prefix, which already ends COPY, so they never run through
    /// here.
    fn leave_chat_copy(&mut self, pane: PaneId) {
        self.input = InputMode::Terminal;
        if let Some(chat) = self.panes.get_mut(&pane).and_then(PaneState::chat_mut) {
            chat.set_scroll(Scroll::Follow);
        }
    }

    /// Moves the cursor, and the focused transcript's view only as far as
    /// the cursor needs.
    fn move_chat_copy(&mut self, mut state: ChatCopy, motion: CopyMotion) {
        let focus = self.tab().focus;
        let Some((geo, _)) = self.chat_copy_view(focus) else {
            return;
        };
        let line_end = match motion {
            CopyMotion::LineEnd => self.chat_line_end(focus, state.state.cursor.line),
            _ => 0,
        };
        state.state.apply_motion(motion, geo, line_end);
        self.set_chat_copy(focus, state, geo);
    }

    /// One wheel tick on the focused chat in COPY: the view moves and the
    /// cursor only as far as needed to stay on a visible row. Whether the
    /// view moved.
    pub(super) fn wheel_chat_copy(&mut self, dir: WheelDir) -> bool {
        let InputMode::ChatCopy(mut state) = self.input else {
            return false;
        };
        let focus = self.tab().focus;
        let Some((geo, scroll)) = self.chat_copy_view(focus) else {
            return false;
        };
        state.sync(geo, scroll);
        let before = state;
        let motion = match dir {
            WheelDir::Up => CopyMotion::WheelUp,
            WheelDir::Down => CopyMotion::WheelDown,
        };
        state.state.apply_motion(motion, geo, 0);
        self.set_chat_copy(focus, state, geo);
        state != before
    }

    /// Stores `state` and maps its offset onto the view of chat `id`.
    fn set_chat_copy(&mut self, id: PaneId, state: ChatCopy, geo: Geometry) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_scroll(state.scroll(geo));
        }
        self.input = InputMode::ChatCopy(state);
    }

    /// The last column with text on transcript row `line` of chat `id`.
    fn chat_line_end(&self, id: PaneId, line: usize) -> u16 {
        let Some(state) = self.panes.get(&id) else {
            return 0;
        };
        let Some(chat) = state.chat_state() else {
            return 0;
        };
        let lines = chat.lines(usize::from(state.size.cols));
        let width = lines.get(line).map_or(0, |l| text_width(l.text.trim_end()));
        u16::try_from(width.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    /// `y`: copies the clean text of the selection (OSC 52) and leaves to
    /// TERMINAL, following. A selection with no text, or one over the cap,
    /// sends nothing and keeps chat COPY.
    fn yank_chat_copy(&mut self, state: ChatCopy) -> Vec<Effect> {
        let focus = self.tab().focus;
        let cols = self.panes.get(&focus).map_or(0, |pane| pane.size.cols);
        let text = state.state.span(cols).and_then(|span| {
            let chat = self.panes.get(&focus)?.chat_state()?;
            Some(join(&transcript::read_rows(
                &chat.lines(usize::from(cols)),
                span,
            )))
        });
        let Some(text) = text else {
            return Vec::new();
        };
        if text.is_empty() {
            // An empty OSC 52 would clobber the clipboard.
            self.notice = Some("Nothing to copy".to_string());
            return Vec::new();
        }
        let effects = self.copy_to_clipboard(&text);
        if !effects.is_empty() {
            self.leave_chat_copy(focus);
        }
        effects
    }
}
