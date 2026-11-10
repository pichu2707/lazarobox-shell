//! Deleting a downloaded model from the picker (config-menu, Delete a
//! model): the picker asks, the runtime deletes, and the reply comes back
//! here as an event. The notice names the result and the engines are
//! detected again, so the list matches what the server now holds.

use super::{App, Effect};
use crate::core::ai::chat::Phase;
use crate::core::ai::picker::{DeleteAsk, DeleteTarget};

impl App {
    /// Who has a chat turn running now, `(engine, model)`: the delete check
    /// reads it on every key the menu gets.
    pub(super) fn running_turns(&self) -> Vec<(String, String)> {
        self.panes
            .values()
            .filter_map(|pane| pane.chat_state())
            .filter(|chat| chat.phase() != Phase::Idle)
            .map(|chat| {
                (
                    chat.stream_engine().to_owned(),
                    chat.stream_model().to_owned(),
                )
            })
            .collect()
    }

    /// `y` on the Delete screen. A file goes through the runtime's own
    /// checks; without an Ollama port an Ollama model fails at once, so the
    /// picker does not wait for a reply that never comes.
    pub(super) fn start_delete(&mut self, ask: DeleteAsk) -> Vec<Effect> {
        if matches!(ask.target, DeleteTarget::Gguf(_)) {
            return vec![Effect::DeleteFiles(ask)];
        }
        let Some(port) = self.ai.servers().port("ollama") else {
            return self.on_delete_reply(ask, Err("no Ollama port".to_owned()));
        };
        vec![Effect::DeleteModel { ask, port }]
    }

    /// The runtime's answer. The picker drops one that is not the delete in
    /// flight, but the disk changed either way, so the notice and the new
    /// detection do not depend on who still waits.
    pub(super) fn on_delete_reply(
        &mut self,
        ask: DeleteAsk,
        answer: Result<(), String>,
    ) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.delete_reply(ask.id);
        }
        self.notice = Some(match &answer {
            Ok(()) => ask.target.deleted(),
            Err(why) => ask.target.failed(why),
        });
        self.dirty = true;
        let effects = self.request_forced_detection();
        self.sync_checking();
        effects
    }

    /// The runtime's measure of a file for the Delete screen. Nothing was
    /// removed, so nothing is detected again.
    pub(super) fn on_delete_measured(
        &mut self,
        ask: DeleteAsk,
        answer: Result<(), String>,
    ) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.delete_measured(ask, answer);
        }
        self.dirty = true;
        Vec::new()
    }
}
