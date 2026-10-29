//! The chat folder in the app (chat-folder): when it may change, and the
//! change itself.

use super::*;

impl App {
    /// Why chat `id` cannot change folder now (see `ChatState::folder_busy`),
    /// with the run slots the app owns. `None` for a chat that is gone.
    pub(super) fn folder_busy(&self, id: PaneId) -> Option<&'static str> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        let running = self
            .panes
            .values()
            .filter_map(PaneState::pty)
            .any(|pty| pty.run.as_ref().is_some_and(|slot| slot.chat == id));
        chat.folder_busy(running)
    }
}
