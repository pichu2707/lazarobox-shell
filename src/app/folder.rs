//! The chat folder in the app (chat-folder): when it may change, and the
//! change itself.

use super::*;

impl App {
    /// Why chat `id` cannot change folder now (see `ChatState::folder_busy`),
    /// with the run slots the app owns. `None` for a chat that is gone.
    #[allow(dead_code)] // used by the `/cd` flow (chat-folder A2)
    pub(super) fn folder_busy(&self, id: PaneId) -> Option<&'static str> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        let running = self
            .panes
            .values()
            .filter_map(PaneState::pty)
            .any(|pty| pty.run.as_ref().is_some_and(|slot| slot.chat == id));
        chat.folder_busy(running)
    }

    /// Makes `new` (probed, resolved) the folder of chat `id` and asks for
    /// what the old one pinned: the system prompt, the memory project and the
    /// tool session (whose approvals belong to the old folder). Every other
    /// consumer reads the folder when it needs it (design D1).
    #[allow(dead_code)] // used by the `/cd` flow (chat-folder A2)
    pub(super) fn apply_folder(&mut self, id: PaneId, new: PathBuf) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let change = chat.set_folder(new);
        let session = chat.take_tool_session();
        let mut effects = Vec::new();
        if change.prompt_now {
            effects.extend(self.prepare_prompt(id));
        }
        if session {
            effects.push(Effect::EndToolSession(id));
        }
        self.dirty = true;
        effects
    }
}
