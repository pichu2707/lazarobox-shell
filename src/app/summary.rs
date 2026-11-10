//! The end-of-conversation summary (engram-memory M8): the summary key, the
//! request it sends and the keys of the draft the user reviews.

use super::*;
use crate::core::ai::chat::{
    guidance::guidance,
    memory::Readiness,
    note::{MEMORY_CHECKING, MEMORY_OFF, not_ready},
    summary::{KEPT, NOTHING_TO_SUMMARIZE, SUMMARY_BUSY, SummaryOutcome, cannot_summarize},
    yank,
};
use crate::core::ai::memory::port::Reason;

impl App {
    /// Whether chat `id` has a memory view (note editor or summary draft) open.
    pub(super) fn memory_view_open(&self, id: PaneId) -> bool {
        self.note_editor_open(id) || self.summary_draft_open(id)
    }

    /// Whether chat `id` has its summary draft open.
    pub(super) fn summary_draft_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.summary_draft().is_some())
    }

    /// `Prefix s`: asks the engine of the `chat` task for the summary of the
    /// conversation. It needs memory on and ready, an answer, an idle pane and
    /// a ready engine; otherwise it says why and sends nothing. The request
    /// takes the pane's stream slot; its text fills the draft when it ends.
    pub(super) fn chat_summary_key(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.ai.memory.active() {
            self.notice = Some(MEMORY_OFF.to_owned());
            return Vec::new();
        }
        let now = (self.clock)();
        // A memory key brings the held `engram serve` offer in front of the user.
        self.show_server_offer();
        let availability = self.chat_availability();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // One memory view at a time.
        if chat.note_editor().is_some()
            || chat.summary_draft().is_some()
            || chat.tag_editor().is_some()
        {
            return Vec::new();
        }
        // Busy first: while the first answer streams, there is no finished
        // answer yet, but "nothing to summarize" would be the wrong reason.
        if chat.phase() != Phase::Idle {
            self.notice = Some(SUMMARY_BUSY.to_owned());
            return Vec::new();
        }
        if yank::last_answer(chat).is_none() {
            self.notice = Some(NOTHING_TO_SUMMARIZE.to_owned());
            return Vec::new();
        }
        let cwd = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned());
        match chat.readiness().clone() {
            Readiness::Ready(_) => {}
            state => {
                // Without a directory there is no project to look for.
                let Some(cwd) = cwd else {
                    let reason = Reason::NotGit.text();
                    chat.set_unavailable(&reason);
                    self.notice = Some(not_ready(&reason));
                    return Vec::new();
                };
                self.notice = Some(match &state {
                    Readiness::Unavailable(reason) => not_ready(reason),
                    _ => MEMORY_CHECKING.to_owned(),
                });
                // Still looking: nothing to add. Otherwise look (again): the
                // user may have started engram. Press the key once more.
                if matches!(state, Readiness::Resolving { .. }) {
                    return Vec::new();
                }
                chat.start_resolving_at(false, now);
                let epoch = chat.memory_epoch();
                return vec![Effect::Memory(MemCmd::Resolve {
                    pane: id,
                    cwd,
                    epoch,
                })];
            }
        }
        if let Some(reason) = guidance(&availability) {
            self.notice = Some(cannot_summarize(&reason));
            return Vec::new();
        }
        let Some((turn, request)) = chat.begin_summary_at(&availability, now) else {
            return Vec::new();
        };
        self.dirty = true;
        vec![Effect::ChatSend {
            pane: id,
            turn,
            request,
        }]
    }

    /// A key while the summary draft of chat `id` is open.
    pub(super) fn on_summary_key(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        let memory_on = self.ai.memory.active();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(draft) = chat.summary_draft_mut() else {
            return Vec::new();
        };
        self.dirty = true;
        match draft.on_key(key, repeat) {
            SummaryOutcome::Handled => {}
            SummaryOutcome::Discard => chat.discard_summary(),
            SummaryOutcome::Approve(_) if !memory_on => {
                // Memory was turned off while the draft was open: nothing is
                // kept, and the text stays.
                draft.refused(MEMORY_OFF);
                self.notice = Some(MEMORY_OFF.to_owned());
            }
            SummaryOutcome::Approve(text) => {
                // Kept in the chat state; nothing goes to engram until the
                // session ends.
                chat.approve_summary(&text);
                self.notice = Some(KEPT.to_owned());
            }
        }
        Vec::new()
    }

    /// A paste into the summary draft of chat `id`.
    pub(super) fn summary_paste(&mut self, id: PaneId, text: &str) {
        if let Some(draft) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::summary_draft_mut)
        {
            draft.paste(text);
            self.dirty = true;
        }
    }
}
