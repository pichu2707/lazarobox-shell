//! Where a pane's view is scrolled: the badge of every pane and the position
//! in the statusline.

use super::*;

/// The COPY key hint of a shell pane.
const SHELL_COPY_HINT: [&str; 4] = ["j/k scroll", "v select", "y copy", "Esc exit"];
/// The COPY key hint of a chat pane: no selection or yank.
const CHAT_COPY_HINT: [&str; 2] = ["j/k scroll", "Esc exit"];

impl App {
    /// The static COPY key hint of the active COPY kind, clipped by whole
    /// entries to `max_cols`.
    pub(super) fn copy_hint(&self, max_cols: u16) -> String {
        match self.input {
            InputMode::ChatCopy(_) => prefix::clip_hint(&CHAT_COPY_HINT, max_cols),
            _ => prefix::clip_hint(&SHELL_COPY_HINT, max_cols),
        }
    }

    /// How far pane `id` is scrolled back: the shell's offset in rows, or the
    /// transcript lines below a chat's view. `None` when it is live, on the
    /// alternate screen, or `id` is not a pane.
    pub fn scrolled_by(&self, id: PaneId) -> Option<usize> {
        let state = self.panes.get(&id)?;
        let offset = match &state.content {
            PaneContent::Pty(pty) => pty.emu.scrollback_offset(),
            PaneContent::Chat(_) => self.chat_extent(id)?.0,
        };
        (offset > 0).then_some(offset)
    }

    /// The `(offset, total)` the statusline mode block shows: the COPY
    /// position in shell COPY, always one in chat COPY (an explicit 0), and
    /// otherwise the focused pane's while TERMINAL and scrolled back.
    pub fn scroll_position(&self) -> Option<(usize, usize)> {
        let focus = self.tab().focus;
        match self.input {
            InputMode::Copy(_) => self.copy_position(),
            InputMode::ChatCopy(_) => self.chat_extent(focus),
            InputMode::Terminal => {
                let offset = self.scrolled_by(focus)?;
                let total = match self.panes.get(&focus)?.pty() {
                    Some(pty) => pty.emu.scrollback_len(),
                    None => self.chat_extent(focus)?.1,
                };
                Some((offset, total))
            }
            _ => None,
        }
    }

    /// `(lines below the view, scrollable lines)` of chat `id`: the lines
    /// above the last `viewport` rows are what can be scrolled through.
    fn chat_extent(&self, id: PaneId) -> Option<(usize, usize)> {
        let state = self.panes.get(&id)?;
        let chat = state.chat_state()?;
        let rows = self.chat_viewport(id);
        let total = chat.lines(usize::from(state.size.cols)).len();
        let top = chat.scroll().top(total, rows);
        let scrollable = total.saturating_sub(rows);
        Some((scrollable - top, scrollable))
    }
}

#[cfg(test)]
impl App {
    /// Test-only: scrolls shell pane `id` back by `offset` rows, as the wheel
    /// will.
    pub(crate) fn scroll_shell_for_test(&mut self, id: PaneId, offset: usize) {
        if let Some(pty) = self.panes.get_mut(&id).and_then(PaneState::pty_mut) {
            pty.emu.set_scrollback(offset);
        }
    }
}
