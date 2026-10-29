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

    /// Whether `epoch` is the memory epoch chat `id` asks under now.
    pub(super) fn memory_epoch_is(&self, id: PaneId, epoch: u32) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.memory_epoch() == epoch)
    }

    /// Whether save `token` of chat `id` was sent before its folder changed.
    pub(super) fn save_is_stale(
        &self,
        id: PaneId,
        token: crate::core::ai::memory::port::SaveId,
    ) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.save_is_stale(token))
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
        // The session of the old project ends before the chat forgets it.
        let end = chat.take_end().map(|(session, project, summary)| {
            Effect::Memory(MemCmd::End {
                session,
                project,
                summary,
                cwd: None,
            })
        });
        let change = chat.set_folder(new);
        let session = chat.take_tool_session();
        let mut effects: Vec<Effect> = end.into_iter().collect();
        if change.prompt_now {
            effects.extend(self.prepare_prompt(id));
        }
        if session {
            effects.push(Effect::EndToolSession(id));
        }
        effects.extend(self.begin_memory(id));
        self.dirty = true;
        effects
    }
}
