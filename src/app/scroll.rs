//! Where a pane's view is scrolled: the badge of every pane and the position
//! in the statusline.

use super::*;

impl App {
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
