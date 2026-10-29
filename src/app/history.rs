//! Chat history in the app (ai-chat C9): the conversation list a chat opens
//! on, what is saved and when, resume and delete. The store thread does the
//! IO; this side only builds its commands and applies its replies.

use super::*;
use crate::core::ai::chat::{
    list::{
        DELETE_FAILED, LIST_FAILED, ListOutcome, NotesStay, OPEN_ELSEWHERE, RESUME_FAILED,
        notes_result_line, notes_stay_line,
    },
    store::{ConvRow, NoteRow, SAVE_FAILED_LINE, UNAVAILABLE_PREFIX},
    tags::Tags,
};

/// What a confirmed delete does with the notes of its conversation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NoteChoice {
    /// The prompt had no choice about notes: they stay in engram.
    Plain,
    /// `y` in the three-way prompt: the user keeps them.
    Keep,
    /// `a`: delete them in engram once the conversation is gone.
    Delete,
}

impl App {
    /// A chat for a pane opened from `origin`: the current profile and, with
    /// history on, the list as its first view. The project tag comes with the
    /// store's answer to the list: its walk is IO.
    pub(super) fn new_chat_state(&self, origin: Option<PathBuf>) -> ChatState {
        let mut chat = ChatState::new(origin);
        chat.set_profile(self.ai.profile());
        chat.set_run_hint(self.ai.tools_enabled());
        chat.set_approval_mode(self.ai.approval());
        if self.history {
            chat.open_list();
        }
        chat
    }

    /// Whether chat `id` shows its conversation list.
    pub(super) fn list_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.list().is_some())
    }

    /// The database ids of the conversations open in any pane, which
    /// retention must not evict.
    fn open_conversations(&self) -> Vec<i64> {
        let mut open: Vec<i64> = self
            .panes
            .values()
            .filter_map(PaneState::chat_state)
            .filter_map(ChatState::conversation)
            .collect();
        open.sort_unstable();
        open
    }

    /// Whether a pane other than `id` has conversation `conversation` open.
    fn open_elsewhere(&self, conversation: i64, id: PaneId) -> bool {
        self.panes.iter().any(|(pane, state)| {
            *pane != id
                && state
                    .chat_state()
                    .is_some_and(|chat| chat.conversation() == Some(conversation))
        })
    }

    /// The commands that save what chat `id` sent and answered since the last
    /// call. With history off nothing is saved and nothing is kept.
    pub(super) fn store_effects(&mut self, id: PaneId) -> Vec<Effect> {
        let open = self.open_conversations();
        let history = self.history;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if !history {
            chat.drop_saves();
            return Vec::new();
        }
        chat.store_cmds(id, &open)
            .into_iter()
            .map(Effect::Store)
            .collect()
    }

    /// Ends the streams of the chats among `ids` that are closing, so the
    /// text written so far is saved, and returns the saves. The caller emits
    /// them before the pane or the app goes away.
    pub(super) fn close_store_effects(&mut self, ids: &[PaneId]) -> Vec<Effect> {
        let mut effects = Vec::new();
        for id in ids {
            if let Some(chat) = self.panes.get_mut(id).and_then(PaneState::chat_mut) {
                chat.close_stream();
            }
            effects.extend(self.store_effects(*id));
        }
        effects
    }

    /// The effect that prepares chat `id`'s system prompt for its profile.
    pub(super) fn prepare_prompt(&self, id: PaneId) -> Option<Effect> {
        let chat = self.panes.get(&id)?.chat_state()?;
        Some(Effect::PreparePrompt {
            pane: id,
            profile: chat.profile(),
            origin: chat.origin_cwd().map(Path::to_path_buf),
            context: self.ai.context(),
        })
    }

    pub(super) fn set_list_line(&mut self, id: PaneId, line: &str) {
        if let Some(list) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::list_mut)
        {
            list.set_line(line);
            self.dirty = true;
        }
    }

    /// A key while the conversation list of chat `id` is shown.
    pub(super) fn on_list_key(&mut self, id: PaneId, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        // Whether notes can be deleted is looked up when the prompt opens, and
        // only when it matters: memory on and the row under the cursor has
        // notes (so a run without memory never reads the environment).
        let asks = key.code == KeyCode::Char('d') && key.modifiers == KeyModifiers::NONE;
        let deletable = asks && !repeat && self.notes_deletable(id);
        let outcome = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::list_mut)
            .map(|list| {
                list.set_notes_deletable(deletable);
                list.on_key(key, repeat)
            });
        let Some(outcome) = outcome else {
            return Vec::new();
        };
        self.dirty = true;
        match outcome {
            ListOutcome::Handled => Vec::new(),
            ListOutcome::New => {
                let mode = self.ai.approval();
                if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                    chat.close_list();
                    // A mode chosen with the toggle lasts for one conversation.
                    chat.set_approval_mode(mode);
                }
                Vec::new()
            }
            ListOutcome::Resume(conversation) => {
                if self.open_elsewhere(conversation, id) {
                    self.set_list_line(id, OPEN_ELSEWHERE);
                    return Vec::new();
                }
                vec![Effect::Store(StoreCmd::Resume {
                    pane: id,
                    conversation,
                })]
            }
            ListOutcome::EditTags(conversation) => {
                self.edit_row_tags(id, conversation);
                Vec::new()
            }
            ListOutcome::Delete(conversation) => self.delete_conversation(id, conversation, None),
            ListOutcome::DeleteKeepNotes(conversation) => {
                self.delete_conversation(id, conversation, Some(NoteChoice::Keep))
            }
            ListOutcome::DeleteWithNotes(conversation) => {
                self.delete_conversation(id, conversation, Some(NoteChoice::Delete))
            }
        }
    }

    /// Whether the delete prompt of the row under the cursor of chat `id` can
    /// offer to delete its notes: memory is on, the row has notes and a token
    /// is present.
    fn notes_deletable(&self, id: PaneId) -> bool {
        if !self.ai.memory.active() {
            return false;
        }
        let has_notes = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(ChatState::list)
            .and_then(|list| list.selected())
            .is_some_and(|row| row.notes > 0);
        has_notes && (self.token_check)()
    }

    /// The store command for a confirmed delete, remembering what to do with
    /// the notes when its answer arrives. `choice` is `None` for the plain
    /// prompt.
    fn delete_conversation(
        &mut self,
        id: PaneId,
        conversation: i64,
        choice: Option<NoteChoice>,
    ) -> Vec<Effect> {
        self.note_choices
            .retain(|(pane, conv, _)| (*pane, *conv) != (id, conversation));
        self.note_choices
            .push((id, conversation, choice.unwrap_or(NoteChoice::Plain)));
        vec![Effect::Store(StoreCmd::Delete {
            pane: id,
            conversation,
        })]
    }

    /// Shows `line` to the user of chat `id`: on its list while it is open,
    /// else in the transcript.
    fn say_in(&mut self, id: PaneId, line: &str) {
        if self.list_open(id) {
            self.set_list_line(id, line);
        } else if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system(line);
            self.dirty = true;
        }
    }

    /// What follows a delete that removed the conversation and `notes` with
    /// it: nothing for a kept choice, the request to delete them for `a`,
    /// else the line that says why they stay.
    fn after_delete(&mut self, id: PaneId, conversation: i64, notes: &[NoteRow]) -> Vec<Effect> {
        let at = self
            .note_choices
            .iter()
            .position(|(pane, conv, _)| (*pane, *conv) == (id, conversation));
        let choice = at
            .map(|at| self.note_choices.remove(at).2)
            .unwrap_or(NoteChoice::Plain);
        if notes.is_empty() || choice == NoteChoice::Keep {
            return Vec::new();
        }
        // The state may have changed since the prompt: memory turned off, and
        // `a` only ever deletes with memory on and a token present.
        let why = if !self.ai.memory.active() {
            Some(NotesStay::MemoryOff)
        } else if !(self.token_check)() {
            Some(NotesStay::NoToken)
        } else if choice == NoteChoice::Plain {
            // The list had no notes for it, so the prompt had no `a`.
            Some(NotesStay::SavedLater)
        } else {
            None
        };
        if let Some(why) = why {
            self.say_in(id, &notes_stay_line(notes.len(), why));
            return Vec::new();
        }
        vec![Effect::Memory(MemCmd::Delete {
            pane: id,
            notes: notes
                .iter()
                .map(|note| (note.project.clone(), note.observation_id))
                .collect(),
        })]
    }

    /// The memory worker deleted (some of) the notes of a deleted
    /// conversation.
    pub(super) fn on_notes_deleted(&mut self, id: PaneId, ok: usize, failed: usize, refused: bool) {
        self.say_in(id, &notes_result_line(ok, failed, refused));
    }

    /// The memory worker could not try the notes delete at all.
    pub(super) fn on_notes_unavailable(&mut self, id: PaneId, reason: &str) {
        self.say_in(id, &format!("could not delete notes: {reason}"));
    }

    /// Pane `id` is gone: what it chose for the notes of a delete in flight
    /// is not needed any more.
    pub(super) fn drop_note_choices(&mut self, id: PaneId) {
        self.note_choices.retain(|(pane, _, _)| *pane != id);
    }

    /// The store thread's answer, as far as history is concerned: the list,
    /// resume, delete, the write failures and the unusable store. What the
    /// memory links need from `Created` is `on_store_reply`'s.
    pub(super) fn on_history_reply(&mut self, reply: &StoreReply) -> Vec<Effect> {
        match reply {
            StoreReply::Created { pane, .. } | StoreReply::Failed { pane, .. } => {
                self.on_write_reply(*pane, reply)
            }
            StoreReply::Unavailable { pane, reason } => {
                let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                // The first answer that finds the store unusable says so; the
                // list of such a store is empty.
                if !chat.store_detached() {
                    chat.push_system(format!("{UNAVAILABLE_PREFIX}{reason}"));
                }
                chat.detach_store();
                if let Some(list) = chat.list_mut()
                    && !list.loaded()
                {
                    list.set_rows(Vec::new());
                }
                self.dirty = true;
                Vec::new()
            }
            StoreReply::Listed { pane, tag, rows } => {
                self.on_listed(*pane, tag, rows.clone());
                Vec::new()
            }
            StoreReply::ListFailed { pane, tag } => {
                self.on_listed(*pane, tag, Vec::new());
                self.set_list_line(*pane, LIST_FAILED);
                Vec::new()
            }
            StoreReply::Resumed {
                pane,
                conversation,
                profile,
                tag,
                extras,
                messages,
                session,
                folder,
            } => {
                let tags = Tags {
                    project: tag.clone(),
                    extras: extras.clone(),
                };
                self.on_resumed(
                    *pane,
                    *conversation,
                    profile,
                    tags,
                    messages,
                    (session, folder),
                )
            }
            StoreReply::ResumeFailed { pane } => {
                self.set_list_line(*pane, RESUME_FAILED);
                Vec::new()
            }
            StoreReply::Deleted {
                pane,
                conversation,
                notes,
            } => {
                // The row goes from every open list, not only this one's.
                for state in self.panes.values_mut() {
                    if let Some(list) = state.chat_mut().and_then(ChatState::list_mut) {
                        list.remove(*conversation);
                    }
                }
                self.close_tag_editors_of(*conversation);
                // Another pane that has it open keeps working but stops saving.
                for (id, state) in &mut self.panes {
                    if let Some(chat) = state.chat_mut()
                        && id != pane
                        && chat.conversation() == Some(*conversation)
                    {
                        chat.detach_store_deleted();
                    }
                }
                self.dirty = true;
                self.after_delete(*pane, *conversation, notes)
            }
            StoreReply::DeleteFailed { pane } => {
                self.note_choices.retain(|(p, _, _)| p != pane);
                self.set_list_line(*pane, DELETE_FAILED);
                Vec::new()
            }
            StoreReply::TagsSaved { .. } | StoreReply::TagsFailed { .. } => {
                self.on_tags_reply(reply);
                Vec::new()
            }
            StoreReply::SessionLinked { .. } | StoreReply::LinkFailed { .. } => Vec::new(),
        }
    }

    /// The list arrived (or could not be read): the pane learns its project
    /// tag and the list shows its rows.
    fn on_listed(&mut self, id: PaneId, tag: &Option<String>, rows: Vec<ConvRow>) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        // A conversation that began without waiting keeps the tag it was
        // created with (the store thread resolved it from the origin).
        if tag.is_some() && chat.store_fresh() && !chat.tags_edited() {
            chat.set_tag(tag.clone());
        }
        if let Some(list) = chat.list_mut() {
            list.set_project(tag.clone());
            list.set_rows(rows);
        }
        self.dirty = true;
    }

    /// The answer to a `Create` or an `Append`: the line for a first failed
    /// write.
    fn on_write_reply(&mut self, id: PaneId, reply: &StoreReply) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // A write nobody sent is not news, and neither is one that was in
        // flight when the pane stopped saving (its conversation was deleted).
        if chat.store_fresh() {
            return Vec::new();
        }
        let detached = chat.store_detached();
        chat.on_store_reply(id, reply);
        if matches!(reply, StoreReply::Failed { notify: true, .. }) && !detached {
            chat.push_system(SAVE_FAILED_LINE);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The stored conversation arrived: it takes the place of the list. A pane
    /// that no longer shows the list, or a conversation another pane opened
    /// in the meantime, is left alone.
    fn on_resumed(
        &mut self,
        id: PaneId,
        conversation: i64,
        profile: &str,
        tags: Tags,
        messages: &[crate::core::ai::chat::Msg],
        (session, folder): (&Option<(String, String)>, &Option<PathBuf>),
    ) -> Vec<Effect> {
        if !self.list_open(id) {
            return Vec::new();
        }
        if self.open_elsewhere(conversation, id) {
            self.set_list_line(id, OPEN_ELSEWHERE);
            return Vec::new();
        }
        let mode = self.ai.approval();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.set_approval_mode(mode);
        let profile = Profile::from_id(profile).unwrap_or_else(|| chat.profile());
        let reprepare = chat.adopt_profile(profile);
        chat.resume_into(conversation, profile, tags, messages.to_vec());
        // The pair its notes were written under is reused before the first
        // write: engram rejects a session under another project, and the
        // store already holds the link.
        if let Some((session_id, project)) = session {
            chat.replace_session(session_id, project);
        }
        self.dirty = true;
        let mut effects: Vec<Effect> = if reprepare {
            self.prepare_prompt(id).into_iter().collect()
        } else {
            Vec::new()
        };
        effects.extend(self.probe_stored_folder(id, conversation, folder.as_deref()));
        effects
    }

    /// The runtime prepared a system prompt for chat `id`. When the profile
    /// changed while it was being prepared (a resume) it is dropped and
    /// prepared again for the stored profile.
    pub(super) fn on_prompt_for(&mut self, id: PaneId, resolved: Resolved) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if chat.set_prompt_or_reprepare(resolved) {
            return self.prepare_prompt(id).into_iter().collect();
        }
        self.dirty = true;
        self.run_waiting_sends()
    }
}
