//! The history side of a chat pane: the profile and project tag of the
//! conversation, the conversation list, what is saved and when, and resume.
//! Pure: the store thread lives in `runtime::chat_store`.

use super::{
    ChatState, Entry, Msg, MsgStatus, Phase,
    engine::Role,
    list::ListView,
    store::{DELETED_LINE, NewConversation, StoreCmd, StoreReply, StoredMessage},
};
use crate::core::{ai::config::Profile, layout::PaneId};

impl ChatState {
    /// The project tag proposed for this conversation.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn set_tag(&mut self, tag: Option<String>) {
        self.tag = tag;
    }

    /// The profile of the conversation.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    /// The conversation list, while it is shown.
    pub fn list(&self) -> Option<&ListView> {
        self.list.as_ref()
    }

    pub fn list_mut(&mut self) -> Option<&mut ListView> {
        self.list.as_mut()
    }

    /// Shows the list, filtered by the project tag. The rows arrive with the
    /// store's answer.
    pub fn open_list(&mut self) {
        self.list = Some(ListView::new(self.tag.clone()));
    }

    pub fn close_list(&mut self) {
        self.list = None;
    }

    /// The commands that save what was sent and answered since the last call,
    /// in order. `open` holds the conversations open in any pane, which
    /// retention must not evict. A message sent before the conversation exists
    /// waits for the store's `Created` (see `on_store_reply`).
    pub fn store_cmds(&mut self, pane: PaneId, open: &[i64]) -> Vec<StoreCmd> {
        let new = NewConversation {
            profile: self.profile.id().to_owned(),
            tag: self.tag.clone(),
            open: open.to_vec(),
        };
        std::mem::take(&mut self.outbox)
            .into_iter()
            .filter_map(|message| self.store.save(pane, message, &new))
            .collect()
    }

    /// Forgets what is waiting to be saved (history is off).
    pub fn drop_saves(&mut self) {
        self.outbox.clear();
    }

    /// The store's answer to a write of this pane.
    pub fn on_store_reply(&mut self, pane: PaneId, reply: &StoreReply) {
        self.store.on_reply(pane, reply);
    }

    /// Whether nothing was ever saved for this pane: no write can be in
    /// flight.
    pub fn store_fresh(&self) -> bool {
        self.store.is_fresh()
    }

    /// Whether this conversation no longer saves (deleted, or the store is
    /// unusable).
    pub fn store_detached(&self) -> bool {
        self.store.is_detached()
    }

    /// Another pane deleted this conversation: nothing more is saved and the
    /// transcript says so, once.
    pub fn detach_store_deleted(&mut self) {
        if self.store.conversation().is_some() {
            self.store.detach();
            self.push_system(DELETED_LINE);
        }
    }

    /// The store cannot be used: nothing more is saved.
    pub fn detach_store(&mut self) {
        self.store.detach();
    }

    /// The pane is closing: the answer a stream has written so far is saved
    /// as a cancelled one. The state itself is left as it is (the runtime
    /// stops the turn). A summary request is not part of the conversation and
    /// saves nothing, and neither does a turn that has not answered yet.
    pub fn close_stream(&mut self) {
        if self.phase != Phase::Streaming || self.summarizing.is_some() || self.closing {
            return;
        }
        let Some(text) = self.messages().last().map(|m| m.text.clone()) else {
            return;
        };
        self.closing = true;
        self.outbox.push(StoredMessage {
            role: Role::Assistant,
            text,
            status: MsgStatus::Cancelled,
            engine: self.request_engine.clone(),
            model: (!self.request_model.is_empty()).then(|| self.request_model.clone()),
        });
    }

    /// Opens a stored conversation in this pane: its messages (partial ones
    /// keep their status) replace the transcript, the list closes, and
    /// further messages are appended to the same conversation. The memory
    /// line and the system lines already shown stay; nothing stored holds
    /// reasoning, so none is shown.
    pub fn resume_into(
        &mut self,
        conversation: i64,
        profile: Profile,
        tag: Option<String>,
        messages: Vec<Msg>,
    ) {
        let stored = Self::resumed(self.origin_cwd.clone(), messages).entries;
        let (memory, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries)
            .into_iter()
            .filter(|entry| matches!(entry, Entry::Memory(_) | Entry::System(_)))
            .partition(|entry| matches!(entry, Entry::Memory(_)));
        self.entries = memory.into_iter().chain(stored).chain(others).collect();
        self.list = None;
        self.profile = profile;
        self.tag = tag;
        self.store = super::store::PaneStore::open(conversation);
        self.set_conversation(conversation);
        self.scroll = super::transcript::Scroll::Follow;
        self.generation += 1;
    }

    /// The profile changed to `profile` (a resume): returns whether the
    /// system prompt must be prepared again. A prompt already prepared is
    /// dropped; one still being prepared is dropped when it arrives (see
    /// `set_prompt_or_reprepare`).
    pub fn adopt_profile(&mut self, profile: Profile) -> bool {
        if profile == self.profile {
            return false;
        }
        self.profile = profile;
        if self.prompt.take().is_some() {
            return true;
        }
        self.reprepare_prompt = true;
        false
    }

    /// The prepared prompt arrived. When the profile changed while it was
    /// being prepared it is dropped and `true` says to prepare it again;
    /// otherwise it is pinned like `set_prompt` does.
    pub fn set_prompt_or_reprepare(&mut self, resolved: super::prompts::Resolved) -> bool {
        if std::mem::take(&mut self.reprepare_prompt) {
            return true;
        }
        self.set_prompt(resolved);
        false
    }
}
