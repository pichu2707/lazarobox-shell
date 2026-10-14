//! COPY on a chat pane: line scrolling of the transcript. The view stays in
//! `ChatState::scroll`; the mode only carries the half-typed `gg`.

use super::*;
use crate::core::ai::chat::{copy::ChatCopyCommand, transcript::Scroll};

/// Shown when COPY is asked on a chat pane that has no transcript to scroll.
const NOTHING_TO_SCROLL: &str = "nothing to scroll here";

impl App {
    /// `Prefix [` on a chat pane: chat COPY over a conversation, otherwise
    /// TERMINAL with a notice (the list, the editors, the summary draft and
    /// guidance with no transcript have nothing to scroll).
    pub(super) fn enter_chat_copy(&mut self) -> InputMode {
        if self.chat_scrollable(self.tab().focus) {
            InputMode::ChatCopy(ChatCopy::default())
        } else {
            self.notice = Some(NOTHING_TO_SCROLL.to_string());
            InputMode::Terminal
        }
    }

    /// Whether chat `id` shows a transcript view.
    pub(super) fn chat_scrollable(&self, id: PaneId) -> bool {
        if self.list_open(id) || self.tag_editor_open(id) || self.memory_view_open(id) {
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

    /// A key in chat COPY. Nothing here reaches the composer, the clipboard
    /// or a PTY.
    pub(super) fn on_chat_copy_key(
        &mut self,
        mut state: ChatCopy,
        key: &KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        let command = state.on_key(key, repeat);
        self.input = InputMode::ChatCopy(state);
        match command {
            ChatCopyCommand::Ignore => {}
            ChatCopyCommand::Prefix => self.input = InputMode::Prefix,
            ChatCopyCommand::Exit => {
                self.leave_chat_copy(self.tab().focus);
                self.hold_exit(key);
            }
            motion => self.scroll_chat(motion),
        }
        Vec::new()
    }

    /// Ends chat COPY (Esc or `q`): the mode is TERMINAL and `pane`'s
    /// transcript follows the bottom again. Focus and tab changes need the
    /// prefix, which already ends COPY, so they never run through here.
    fn leave_chat_copy(&mut self, pane: PaneId) {
        self.input = InputMode::Terminal;
        if let Some(chat) = self.panes.get_mut(&pane).and_then(PaneState::chat_mut) {
            chat.set_scroll(Scroll::Follow);
        }
    }

    /// Applies a scroll command to the focused chat's transcript.
    fn scroll_chat(&mut self, command: ChatCopyCommand) {
        let rows = self.chat_viewport(self.tab().focus);
        let cols = usize::from(self.focused_state().size.cols);
        let half = (rows / 2).max(1);
        let Some(chat) = self.focused_state_mut().chat_mut() else {
            return;
        };
        let total = chat.lines(cols).len();
        let scroll = chat.scroll();
        let next = match command {
            ChatCopyCommand::Up => scroll.lines_up(1, total, rows),
            ChatCopyCommand::Down => scroll.lines_down(1, total, rows),
            ChatCopyCommand::HalfUp => scroll.lines_up(half, total, rows),
            ChatCopyCommand::HalfDown => scroll.lines_down(half, total, rows),
            ChatCopyCommand::Top => scroll.lines_up(total, total, rows),
            ChatCopyCommand::Bottom => Scroll::Follow,
            _ => scroll,
        };
        chat.set_scroll(next);
    }
}
