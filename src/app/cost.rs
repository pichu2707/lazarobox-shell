//! `/cost` in the app (ai-cost 1c): the user-only command, the overlay it
//! opens over the conversation and the store's answer. The store thread does
//! the reading; nothing here waits for it.

use super::*;
use crate::core::ai::chat::{
    cost_view::{CostView, NO_ARGUMENTS},
    usage_view::{UNAVAILABLE_LINE, UsageRow, report_lines},
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

    /// `/cost`: clears the composer and opens the overlay. The store is asked
    /// (after the rows still queued, so the view is current).
    pub(super) fn cost_submit(&mut self, id: PaneId) -> Vec<Effect> {
        // The report must see the usage of the turn that just ended.
        let mut effects = self.store_effects(id);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return effects;
        };
        chat.composer_mut().clear();
        self.dirty = true;
        // A detached store answers `None` itself: the reply decides.
        let conversation = chat.conversation();
        chat.open_cost(CostView::reading());
        effects.push(Effect::Store(StoreCmd::UsageReport {
            pane: id,
            conversation,
        }));
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
        // The model the status bar names for the chat engine, for the rows
        // whose engine reported none (the bar and `/cost` must agree).
        let (assignment, _) = self.ai.resolve(Task::Chat);
        let configured = assignment.model.filter(|m| !m.is_empty());
        let lines = match rows {
            Some(rows) => {
                let named: Vec<UsageRow> = rows
                    .iter()
                    .map(|r| match &configured {
                        Some(model)
                            if r.model.is_none()
                                && assignment.engine.as_deref() == Some(r.engine.as_str()) =>
                        {
                            UsageRow {
                                model: Some(model.clone()),
                                ..r.clone()
                            }
                        }
                        _ => r.clone(),
                    })
                    .collect();
                report_lines(&named, chat.session_usage(), &|e| {
                    self.ai.usage().billing(e)
                })
            }
            None => vec![UNAVAILABLE_LINE.to_owned()],
        };
        if let Some(view) = chat.cost_mut() {
            view.set_lines(lines);
            self.dirty = true;
        }
    }
}
