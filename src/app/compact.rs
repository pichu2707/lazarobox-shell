//! Rolling compaction (chat-memory S3, S4): after a turn ends, the history
//! that left the window is folded into a summary by the `summarize` task's
//! engine. The run takes no stream slot: the pane stays idle, so it never
//! delays the user, and the next send supersedes it.

use super::*;
use crate::core::ai::{
    chat::engine::availability_for,
    config::{CompactionMode, Task},
};

impl App {
    /// A turn of chat `id` ended well: asks for a compaction run when the
    /// conversation is due for one and its engine is ready. Any
    /// other state starts nothing; the truncation marker stands.
    pub(super) fn start_compaction(&mut self, id: PaneId) -> Vec<Effect> {
        if self.ai.compaction() == CompactionMode::Off {
            return Vec::new();
        }
        // Chat content only goes to the engine the user picked for it: the
        // `summarize` task when set explicitly, the chat engine otherwise
        // (never the `general` fallback of an unset `summarize`).
        let task = if self.ai.get(Task::Summarize).engine.is_some() {
            Task::Summarize
        } else {
            Task::Chat
        };
        let availability = availability_for(task, &self.ai, self.engines.as_ref(), self.jail);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        match chat.begin_compaction(&availability) {
            Some((turn, request)) => vec![Effect::ChatCompact {
                pane: id,
                turn,
                request,
            }],
            None => Vec::new(),
        }
    }

    /// One event of a compaction run of chat `id`. A redraw only when the
    /// transcript changed; a run that outgrows its cap stops its engine.
    pub(super) fn on_compaction(&mut self, id: PaneId, event: StreamEvent) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let before = chat.generation();
        let follow = chat.apply_compaction(event);
        self.dirty |= chat.generation() != before;
        if follow == Follow::CancelEngine {
            vec![Effect::ChatCompactCancel(id)]
        } else {
            Vec::new()
        }
    }
}
