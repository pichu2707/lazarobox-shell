//! The full-pane transcript of a worker (agent-orchestrator S9, modal-input
//! "Worker transcript key"): `Ctrl+Space W` opens it for the selected worker
//! of the focused chat's run, Esc or `q` closes it. While it is open the keys
//! scroll it and nothing reaches the composer; Ctrl+C keeps cancelling, and
//! an approval prompt keeps its keys (it is routed before this view).

use super::*;
use crate::core::ai::chat::run_trace::Scroll;

/// Said when no worker is selected.
const NO_SELECTION: &str = "Select a worker with Alt+Up/Down";
/// Said when the orchestrator gate is off.
const GATE_OFF: &str = "The orchestrator is off; enable it in the menu (Ctrl+Space m)";
/// Said on a pane that is not a chat.
const CHAT_ONLY: &str = "The worker transcript is for chat panes; open one with Ctrl+Space c";
/// Said when another view of the chat is open.
const VIEW_OPEN: &str = "Close this view first";
/// Said by a prefix key that would replace the transcript.
const CLOSE_FIRST: &str = "Close the worker transcript first (Esc)";

impl App {
    /// Whether the transcript of a worker is open in chat `id`.
    pub(super) fn run_view_open(&self, id: PaneId) -> bool {
        (self.panes.get(&id).and_then(PaneState::chat_state))
            .is_some_and(|c| c.run_view().is_some())
    }

    /// `Prefix W`: opens the transcript of the selected worker. Every way it
    /// does nothing says why.
    pub(super) fn view_worker(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        let gate = self.orch_gate();
        let said = match self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            None => Some(CHAT_ONLY),
            Some(_) if !gate => Some(GATE_OFF),
            Some(chat) => match chat.selected_worker() {
                None => Some(NO_SELECTION),
                Some(_) if !chat.conversation_shown() => Some(VIEW_OPEN),
                Some(worker) => (!chat.open_run_view(worker)).then_some(NO_SELECTION),
            },
        };
        match said {
            Some(text) => self.notice = Some(text.to_owned()),
            None => self.dirty = true,
        }
        Vec::new()
    }

    /// `Prefix T`, `n`, `s` and `e` would replace the transcript under the
    /// user, so they refuse.
    pub(super) fn refuse_over_run_view(&mut self) -> bool {
        let open = self.focused_is_chat() && self.run_view_open(self.tab().focus);
        if open {
            self.notice = Some(CLOSE_FIRST.to_owned());
        }
        open
    }

    /// A key in a chat whose transcript view is open (after an approval, which
    /// has its keys first).
    pub(super) fn on_run_view_key(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        let plain = key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE;
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            return if repeat {
                Vec::new()
            } else {
                self.chat_ctrl_c(id)
            };
        }
        if !plain {
            return Vec::new();
        }
        let size = self.focused_state().size;
        // The title and the key line take two rows.
        let rows = usize::from(size.rows).saturating_sub(2).max(1);
        let scroll = match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if !repeat && let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
                {
                    chat.close_run_view();
                    self.dirty = true;
                }
                return Vec::new();
            }
            KeyCode::Up | KeyCode::Char('k') => Scroll::Up(1),
            KeyCode::Down | KeyCode::Char('j') => Scroll::Down(1),
            KeyCode::PageUp => Scroll::Up(rows),
            KeyCode::PageDown => Scroll::Down(rows),
            KeyCode::Home | KeyCode::Char('g') => Scroll::Top,
            KeyCode::End | KeyCode::Char('G') => Scroll::Follow,
            _ => return Vec::new(),
        };
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            let total = chat.run_view_rows(usize::from(size.cols)).len();
            chat.run_view_scroll(scroll, total, rows);
            self.dirty = true;
        }
        Vec::new()
    }
}
