//! The local project memory in a chat view (chat-memory-local): what a view
//! asks the `.lzr/` store when it opens, and how each outcome becomes a
//! visible, display-only line.

use super::*;
use crate::core::ai::memory::local::port::{LocalCmd, LocalReply, NO_GIT_LINE};

impl App {
    /// A conversation view opened (or changed folder): asks for the notes of
    /// its project, once. Reads only, and never when `[ai.memory] local` is
    /// off. A view without a directory has no project, and says so.
    pub(super) fn begin_local_memory(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.ai.memory.local {
            return Vec::new();
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(cwd) = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned()) else {
            chat.push_system(NO_GIT_LINE);
            self.dirty = true;
            return Vec::new();
        };
        let epoch = chat.memory_epoch();
        vec![Effect::LocalMemory(LocalCmd::Load {
            pane: id,
            epoch,
            cwd,
        })]
    }

    /// An answer of the local memory thread. Every outcome that needs a word
    /// is a system line: shown, never stored and never sent to the model. An
    /// answer for a closed pane or for the folder the chat left is dropped.
    pub(super) fn on_local_reply(&mut self, reply: &LocalReply) -> Vec<Effect> {
        let id = reply.pane();
        if let LocalReply::Loaded { epoch, .. } = reply
            && !self.memory_epoch_is(id, *epoch)
        {
            return Vec::new();
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let lines = reply.lines();
        self.dirty |= !lines.is_empty();
        for line in lines {
            chat.push_system(line);
        }
        Vec::new()
    }
}
