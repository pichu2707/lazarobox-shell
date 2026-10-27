//! Rolling compaction (chat-memory S3, S4): after a turn ends, the history
//! that left the window is folded into a summary by the `summarize` task's
//! engine. The run takes no stream slot: the pane stays idle, so it never
//! delays the user, and the next send supersedes it.

use super::*;
use crate::core::ai::{
    chat::{compact::SUMMARY_NOTICE, engine::availability_for},
    config::{CompactionMode, Task},
    memory::{
        kind::MemKind,
        port::{MemCmd, Note},
    },
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
            return vec![Effect::ChatCompactCancel(id)];
        }
        match chat.take_fresh_summary() {
            Some(summary) => self.save_summary(id, summary),
            None => Vec::new(),
        }
    }

    /// A run kept `summary` for chat `id` (chat-memory S5, S6). It goes to
    /// engram only when memory is on and ready, the conversation has a stable
    /// id and the user opted in (`[ai.memory] save_summaries`); the save
    /// upserts under `chat/<conversation>/summary`. Without the opt-in the
    /// user is told once, in this run, what would be stored. Memory off or
    /// unavailable writes nothing and says nothing (S8).
    fn save_summary(&mut self, id: PaneId, summary: String) -> Vec<Effect> {
        if !self.ai.memory.active() {
            return Vec::new();
        }
        let opted_in = self.ai.memory.save_summaries;
        let now = (self.clock)();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // The pair the session was registered with, as a note save does.
        let project = chat
            .session()
            .map(|s| s.project.clone())
            .or_else(|| chat.project().map(str::to_owned));
        let (Some(project), Some(conversation)) = (project, chat.conversation()) else {
            return Vec::new();
        };
        if !opted_in {
            if !self.summary_notice_shown {
                self.summary_notice_shown = true;
                self.notice = Some(SUMMARY_NOTICE.to_owned());
            }
            return Vec::new();
        }
        let token = chat.quiet_save_sent_at(&project, now);
        vec![Effect::Memory(MemCmd::Save {
            pane: id,
            id: token,
            session: chat.session().map(|s| s.id.clone()),
            project,
            cwd: chat
                .origin_cwd()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            note: Note {
                kind: MemKind::Discovery,
                title: "Conversation summary".to_owned(),
                content: summary,
            },
            topic_key: Some(format!("chat/{conversation}/summary")),
        })]
    }
}
