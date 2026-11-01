//! `/cost` in the app (ai-cost 1c): the user-only command, the overlay it
//! opens over the conversation and the store's answer. The store thread does
//! the reading; nothing here waits for it.

use super::*;
use crate::core::ai::chat::{
    cost_view::{CostView, NO_ARGUMENTS},
    usage_view::{UNAVAILABLE_LINE, UsageRow, default_billing, report_lines, session_lines},
};

impl App {
    /// Whether chat `id` shows its `/cost` overlay.
    pub(super) fn cost_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.cost().is_some())
    }

    /// `/cost` followed by text: says so and keeps the composer text.
    pub(super) fn cost_refused(&mut self, id: PaneId) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(NO_ARGUMENTS.to_owned());
            self.dirty = true;
        }
    }

    /// `/cost`: clears the composer and opens the overlay. With history on the
    /// store is asked (after the rows still queued, so the view is current);
    /// with it off the session is told from memory.
    pub(super) fn cost_submit(&mut self, id: PaneId) -> Vec<Effect> {
        let history = self.history;
        // The report must see the usage of the turn that just ended.
        let mut effects = self.store_effects(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return effects;
        };
        chat.composer_mut().clear();
        self.dirty = true;
        if !history {
            let lines = session_lines(chat.session_usage(), &default_billing);
            chat.open_cost(CostView::with(lines));
        } else {
            // A detached store answers `None` itself: the reply decides.
            let conversation = chat.conversation();
            chat.open_cost(CostView::reading());
            effects.push(Effect::Store(StoreCmd::UsageReport {
                pane: id,
                conversation,
            }));
        }
        effects
    }

    /// A key while the overlay of chat `id` is shown: Esc closes it.
    pub(super) fn on_cost_key(&mut self, id: PaneId, key: &KeyEvent) -> Vec<Effect> {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            let closes = chat.cost_mut().is_some_and(|view| view.on_key(key));
            if closes {
                chat.close_cost();
            }
            self.dirty = true;
        }
        Vec::new()
    }

    /// The store's answer to the report: the lines replace the reading line.
    /// An overlay closed in the meantime ignores it.
    pub(super) fn on_usage_report(&mut self, id: PaneId, rows: Option<&[UsageRow]>) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let lines = match rows {
            Some(rows) => report_lines(rows, chat.session_usage(), &default_billing),
            None => vec![UNAVAILABLE_LINE.to_owned()],
        };
        if let Some(view) = chat.cost_mut() {
            view.set_lines(lines);
            self.dirty = true;
        }
    }
}
