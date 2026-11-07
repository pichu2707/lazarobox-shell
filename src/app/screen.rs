//! `read_pane` in the app: the pane is read when the ask arrives, the text is
//! frozen in a review the user reads, and only that frozen text is ever sent.
//! A pane that is gone, the chat's own, or a blank screen is denied with a
//! visible line and no prompt.

use std::sync::Arc;

use super::*;
use crate::core::ai::chat::{approval::ApprovalAsk, review::ScreenReview};
use crate::core::ai::tools::screen::build;

const NOT_OPEN: &str = "the pane is not open";
const EMPTY: &str = "the pane's screen is empty";

impl App {
    /// A `read_pane` ask from chat `id`: reads pane number `pane` (never the
    /// chat's own: a chat has no terminal) and opens its review.
    pub(super) fn on_screen_ask(
        &mut self,
        id: PaneId,
        (turn, call): (u64, CallId),
        pane: u32,
        scrollback: Option<u32>,
    ) -> Vec<Effect> {
        let target = (self.panes.iter())
            .find(|(key, state)| key.number() == pane && state.pty().is_some())
            .map(|(key, _)| *key);
        let Some(pty) = target.and_then(|key| self.panes.get_mut(&key)?.pty_mut()) else {
            return self.refuse_screen(
                id,
                (turn, call),
                format!("pane {pane} is not open"),
                NOT_OPEN,
            );
        };
        let alt = pty.emu.mouse_facts().alt_screen;
        let span = pty.emu.screen_span(scrollback.map_or(0, |n| n as usize));
        let rows = pty.emu.read_lines(span);
        let Some(screen) = build(&rows, alt, scrollback.is_some()) else {
            let notice = format!("pane {pane}: the screen is empty");
            return self.refuse_screen(id, (turn, call), notice, EMPTY);
        };
        let ask = ApprovalAsk::Screen {
            pane,
            scrollback,
            review: Some(Arc::new(ScreenReview::new(pane, &screen))),
        };
        self.open_prompt(id, (turn, call), None, ask, None)
    }

    /// Denies the call without a prompt and leaves `notice` in the chat.
    fn refuse_screen(
        &mut self,
        id: PaneId,
        (turn, call): (u64, CallId),
        notice: String,
        reason: &'static str,
    ) -> Vec<Effect> {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(notice);
        }
        self.dirty = true;
        Self::deny(id, turn, call, reason)
    }

    /// Says in the chat that the user did not release the screen of `ask`.
    pub(super) fn note_screen_denied(&mut self, id: PaneId, ask: &ApprovalAsk) {
        if let (ApprovalAsk::Screen { pane, .. }, Some(chat)) =
            (ask, self.panes.get_mut(&id).and_then(PaneState::chat_mut))
        {
            chat.push_system_line(format!("screen read denied: pane {pane}"));
        }
    }
}
