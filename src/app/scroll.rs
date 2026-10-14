//! Where a pane's view is scrolled: the badge of every pane and the position
//! in the statusline.

use super::*;
use crate::core::mouse::{PaneFacts, WHEEL_STEP, WheelAction, WheelDir, route_wheel};
use ratatui::crossterm::event::MouseEventKind;

/// The COPY key hint of a shell pane.
const SHELL_COPY_HINT: [&str; 4] = ["j/k scroll", "v select", "y copy", "Esc exit"];
/// The COPY key hint of a chat pane: no selection or yank.
const CHAT_COPY_HINT: [&str; 2] = ["j/k scroll", "Esc exit"];

/// Whether the cell `(x, y)` is inside `rect`.
fn rect_has(rect: &Rect, x: u16, y: u16) -> bool {
    (rect.x..rect.x.saturating_add(rect.width)).contains(&x)
        && (rect.y..rect.y.saturating_add(rect.height)).contains(&y)
}

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

    /// A mouse event: only a vertical wheel tick over a pane does anything.
    pub(super) fn on_mouse(&mut self, mouse: MouseEvent) -> Vec<Effect> {
        let dir = match mouse.kind {
            MouseEventKind::ScrollUp => WheelDir::Up,
            MouseEventKind::ScrollDown => WheelDir::Down,
            _ => return Vec::new(),
        };
        if !self.config.mouse
            || !matches!(
                self.input,
                InputMode::Terminal | InputMode::Copy(_) | InputMode::ChatCopy(_)
            )
        {
            return Vec::new();
        }
        let tiling = self.tiling();
        let Some((id, rect)) = tiling
            .panes
            .iter()
            .find(|(_, rect)| rect_has(rect, mouse.column, mouse.row))
            .copied()
        else {
            return Vec::new();
        };
        let (col, row) = (mouse.column - rect.x, mouse.row - rect.y);
        let Some(facts) = self.wheel_facts(id) else {
            return Vec::new();
        };
        match route_wheel(facts, dir, col, row, mouse.modifiers) {
            WheelAction::Write(bytes) => vec![Effect::WritePty(id, bytes)],
            WheelAction::Scrollback(dir) => {
                self.scroll_shell_view(id, dir);
                Vec::new()
            }
            WheelAction::CopyScroll(dir) => {
                self.scroll_copy_view(dir);
                Vec::new()
            }
            WheelAction::ChatLines(dir) => {
                self.scroll_chat_by_wheel(id, dir);
                Vec::new()
            }
            WheelAction::Ignore => Vec::new(),
        }
    }

    /// What routing needs to know about pane `id`; `None` for a chat that
    /// shows no transcript (list, editors, summary draft, guidance).
    fn wheel_facts(&self, id: PaneId) -> Option<PaneFacts> {
        match &self.panes.get(&id)?.content {
            PaneContent::Pty(pty) => Some(PaneFacts::Shell {
                mouse: pty.emu.mouse_facts(),
                modes: pty.emu.modes(),
                copy: matches!(self.input, InputMode::Copy(_)) && id == self.tab().focus,
            }),
            PaneContent::Chat(_) => self.chat_scrollable(id).then_some(PaneFacts::Chat),
        }
    }

    /// One wheel tick on a main-screen pane that is not in COPY.
    fn scroll_shell_view(&mut self, id: PaneId, dir: WheelDir) {
        let Some(pty) = self.panes.get_mut(&id).and_then(PaneState::pty_mut) else {
            return;
        };
        let now = pty.emu.scrollback_offset();
        let next = match dir {
            WheelDir::Up => now.saturating_add(WHEEL_STEP),
            WheelDir::Down => now.saturating_sub(WHEEL_STEP),
        };
        pty.emu.set_scrollback(next);
        self.dirty |= pty.emu.scrollback_offset() != now;
    }

    /// One wheel tick on the pane in COPY: the viewport moves, COPY stays.
    fn scroll_copy_view(&mut self, dir: WheelDir) {
        let InputMode::Copy(mut state) = self.input else {
            return;
        };
        let motion = match dir {
            WheelDir::Up => CopyMotion::WheelUp,
            WheelDir::Down => CopyMotion::WheelDown,
        };
        let geo = self.copy_geometry();
        let before = state;
        state.apply_motion(motion, geo, 0);
        self.copy_pane_mut().set_scrollback(state.offset);
        self.input = InputMode::Copy(state);
        self.dirty |= state != before;
    }

    /// One wheel tick on chat `id`: the transcript moves by whole lines.
    fn scroll_chat_by_wheel(&mut self, id: PaneId, dir: WheelDir) {
        let rows = self.chat_viewport(id);
        let Some(state) = self.panes.get_mut(&id) else {
            return;
        };
        let cols = usize::from(state.size.cols);
        let Some(chat) = state.chat_mut() else {
            return;
        };
        let total = chat.lines(cols).len();
        let scroll = chat.scroll();
        let next = match dir {
            WheelDir::Up => scroll.lines_up(WHEEL_STEP, total, rows),
            WheelDir::Down => scroll.lines_down(WHEEL_STEP, total, rows),
        };
        chat.set_scroll(next);
        // Compare where the view is, not the enum: `Pinned` at the bottom
        // and `Follow` show the same lines.
        self.dirty |= scroll.top(total, rows) != next.top(total, rows);
    }

    /// Back to the live screen: the focused shell pane after a key or a paste
    /// was written to its PTY.
    pub(super) fn return_to_live(&mut self) {
        let focus = self.tab().focus;
        if let Some(pty) = self.panes.get_mut(&focus).and_then(PaneState::pty_mut)
            && pty.emu.scrollback_offset() > 0
        {
            pty.emu.set_scrollback(0);
            self.dirty = true;
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
