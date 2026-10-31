//! Save as note (engram-memory M7b): the save-note key, the note editor's
//! keys and the memory worker's replies that concern a chat.

use super::*;
use crate::core::ai::chat::{
    memory::{PendingSave, Readiness},
    note::{self, MEMORY_CHECKING, MEMORY_OFF, NOT_LINKED, NOTHING_TO_SAVE, NoteOutcome},
    store::eviction_lines_of,
    summary::SUMMARY_BUSY,
    yank,
};
use crate::core::ai::memory::{port::Reason, reply::ProjectError};

impl App {
    /// Whether chat `id` has its note editor open.
    pub(super) fn note_editor_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.note_editor().is_some())
    }

    /// `Prefix n`: opens the note editor on the last answer. It needs memory on,
    /// an answer and a known project; when the project is not known it is
    /// asked for, and the editor opens when the reply comes.
    pub(super) fn chat_note_key(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.ai.memory.active() {
            self.notice = Some(MEMORY_OFF.to_owned());
            return Vec::new();
        }
        let now = (self.clock)();
        // A memory key brings the held `engram serve` offer in front of the user.
        self.show_server_offer();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // One memory view at a time: a summary running or open holds the pane.
        if chat.summary_draft().is_some() || chat.tag_editor().is_some() {
            return Vec::new();
        }
        if chat.summarizing() {
            self.notice = Some(SUMMARY_BUSY.to_owned());
            return Vec::new();
        }
        if yank::last_answer(chat).is_none() {
            self.notice = Some(NOTHING_TO_SAVE.to_owned());
            return Vec::new();
        }
        let cwd = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned());
        // Without a directory there is no project to look for, and nothing to
        // wait for: say so now instead of staying in `Resolving` for ever.
        if cwd.is_none() && !matches!(chat.readiness(), Readiness::Ready(_)) {
            let reason = Reason::NotGit.text();
            chat.set_unavailable(&reason);
            self.notice = Some(note::not_ready(&reason));
            return Vec::new();
        }
        let probe = |chat: &mut ChatState, want_note: bool| {
            chat.start_resolving_at(want_note, now);
            let epoch = chat.memory_epoch();
            cwd.clone().map(|cwd| {
                Effect::Memory(MemCmd::Resolve {
                    pane: id,
                    cwd,
                    epoch,
                })
            })
        };
        match chat.readiness().clone() {
            Readiness::Ready(_) => {
                if let Err(notice) = chat.open_note_editor() {
                    self.notice = Some(notice);
                }
                self.dirty = true;
                Vec::new()
            }
            Readiness::Resolving { .. } => {
                chat.start_resolving_at(true, now);
                self.notice = Some(MEMORY_CHECKING.to_owned());
                Vec::new()
            }
            Readiness::Unknown => {
                self.notice = Some(MEMORY_CHECKING.to_owned());
                probe(chat, true).into_iter().collect()
            }
            Readiness::Unavailable(reason) => {
                // Show why, and look again: the user may have started engram.
                self.notice = Some(note::not_ready(&reason));
                probe(chat, false).into_iter().collect()
            }
        }
    }

    /// A key while the note editor of chat `id` is open.
    pub(super) fn on_note_key(&mut self, id: PaneId, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        let now = (self.clock)();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(editor) = chat.note_editor_mut() else {
            return Vec::new();
        };
        self.dirty = true;
        let memory_on = self.ai.memory.active();
        match editor.on_key(key, repeat) {
            NoteOutcome::Handled => Vec::new(),
            NoteOutcome::Discard => {
                chat.close_note_editor();
                Vec::new()
            }
            NoteOutcome::Save(_) if !memory_on => {
                // Memory was turned off in the menu while the editor was
                // open: nothing is sent, and the text stays.
                if let Some(editor) = chat.note_editor_mut() {
                    editor.save_failed(MEMORY_OFF);
                }
                self.notice = Some(MEMORY_OFF.to_owned());
                Vec::new()
            }
            NoteOutcome::Save(note) => {
                // A resumed conversation keeps the pair its session was
                // registered with: engram rejects the session under another
                // project.
                let project = chat
                    .session()
                    .map(|s| s.project.clone())
                    .or_else(|| chat.project().map(str::to_owned));
                let Some(project) = project else {
                    if let Some(editor) = chat.note_editor_mut() {
                        editor.save_failed("memory is not ready");
                    }
                    return Vec::new();
                };
                let token = chat.save_sent_at(&project, now);
                vec![Effect::Memory(MemCmd::Save {
                    pane: id,
                    id: token,
                    session: chat.session().map(|s| s.id.clone()),
                    project,
                    cwd: chat
                        .origin_cwd()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    note,
                    topic_key: None,
                })]
            }
        }
    }

    /// A paste into the note editor of chat `id`.
    pub(super) fn note_paste(&mut self, id: PaneId, text: &str) {
        if let Some(editor) = self
            .panes
            .get_mut(&id)
            .and_then(PaneState::chat_mut)
            .and_then(ChatState::note_editor_mut)
        {
            editor.paste(text);
            self.dirty = true;
        }
    }

    /// The memory worker's answer. A reply for a pane that is gone changes
    /// nothing.
    pub(super) fn on_memory_reply(&mut self, reply: &MemReply) -> Vec<Effect> {
        let Some(id) = reply.pane() else {
            return self.on_chat_memory_reply(reply);
        };
        let (consumed, mut effects) = self.on_run_reply(id, reply);
        if !consumed {
            effects.extend(self.on_chat_memory_reply(reply));
        }
        effects
    }

    /// `on_memory_reply` for what the chat itself asked.
    fn on_chat_memory_reply(&mut self, reply: &MemReply) -> Vec<Effect> {
        match reply {
            MemReply::ServerOffer(offer) => return self.on_server_offer(offer),
            MemReply::Server(outcome) => return self.on_server_outcome(outcome),
            // The worker says once why it cannot offer a server; the pane's
            // own answer follows and takes it as its reason.
            MemReply::Unavailable {
                pane: None,
                epoch: None,
                reason: reason @ (Reason::NoBinary | Reason::OldBinary),
            } => {
                if self.ai.memory.active() {
                    self.no_server = Some(reason.clone());
                }
                return Vec::new();
            }
            _ => {}
        }
        let Some(id) = reply.pane() else {
            return Vec::new();
        };
        if self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_none()
        {
            return Vec::new();
        }
        self.dirty = true;
        match reply {
            // An answer asked under the folder the chat left is not for it.
            MemReply::Project { epoch, .. } | MemReply::Context { epoch, .. }
                if !self.memory_epoch_is(id, *epoch) => {}
            MemReply::Project { result, .. } => return self.on_project_reply(id, result),
            MemReply::Context { block, .. } => self.on_context(id, block),
            // A save asked under the folder the chat left is not for it.
            MemReply::Saved { id: token, .. }
            | MemReply::SaveFailed { id: token, .. }
            | MemReply::SaveUnavailable { id: token, .. }
                if self.save_is_stale(id, *token) => {}
            MemReply::SaveUnavailable {
                id: token, reason, ..
            } => {
                let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                let save = chat.save_answered(*token);
                save_not_done(chat, save.as_ref(), &reason.text());
            }
            // The worker could not even try the notes delete.
            MemReply::NotesUnavailable { reason, .. } => {
                self.on_notes_unavailable(id, &reason.text());
            }
            MemReply::Deleted {
                ok,
                failed,
                refused,
                ..
            } => self.on_notes_deleted(id, *ok, *failed, *refused),
            MemReply::Unavailable {
                epoch: Some(epoch), ..
            } if !self.memory_epoch_is(id, *epoch) => {}
            MemReply::Unavailable { reason, .. } => {
                let reason = match (&self.no_server, reason) {
                    (Some(cause), Reason::NotRunning) => cause.clone(),
                    _ => reason.clone(),
                };
                self.on_unavailable(id, &reason);
            }
            MemReply::SaveFailed {
                id: token, reason, ..
            } => {
                let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                let save = chat.save_answered(*token);
                // The opted-in summary save failing every time must not be
                // silent forever: say so once per run.
                if save.as_ref().is_some_and(|s| s.quiet) && !self.summary_failure_shown {
                    self.summary_failure_shown = true;
                    chat.push_system(format!(
                        "could not save the conversation summary to engram: {reason}"
                    ));
                }
                save_not_done(chat, save.as_ref(), reason);
            }
            MemReply::Saved {
                id: token,
                project,
                session,
                observation_id,
                topic_key,
                title,
                ..
            } => {
                let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                // The editor bookkeeping comes from the pending save with this
                // token (none: no editor is touched). The note is recorded
                // under the project the worker saved it under, which it echoes,
                // so a reply whose token is unknown still lands in the right
                // project instead of being lost.
                let save = chat.save_answered(*token);
                chat.adopt_session(session, project);
                chat.record_note(project, topic_key, *observation_id, title);
                // The editor that sent this save closes; one the user opened
                // since stays.
                chat.close_saved_editor(save.as_ref());
                if !save.as_ref().is_some_and(|s| s.quiet) {
                    let kind = topic_key.split('/').nth(1).unwrap_or("note");
                    chat.push_system(format!("saved note {kind}: {title}"));
                }
                let links = self.link_effects(id);
                return self.with_waiting_sends(links);
            }
            MemReply::Health { .. } | MemReply::Server(_) | MemReply::ServerOffer(_) => {}
        }
        Vec::new()
    }

    /// The project reply: the note editor when it was wanted and the project
    /// is usable, the notice when it is not, and the context request of the
    /// view (one per view).
    fn on_project_reply(
        &mut self,
        id: PaneId,
        result: &Result<String, ProjectError>,
    ) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // A project nobody awaits (the wait ran out, or it came twice) changes
        // nothing.
        if !matches!(chat.readiness(), Readiness::Resolving { .. }) {
            return Vec::new();
        }
        let wanted = chat.project_resolved(result.clone());
        if wanted {
            match chat.readiness().clone() {
                Readiness::Ready(_) => {
                    if let Err(notice) = chat.open_note_editor() {
                        self.notice = Some(notice);
                    }
                }
                Readiness::Unavailable(reason) => {
                    self.notice = Some(note::not_ready(&reason));
                }
                Readiness::Unknown | Readiness::Resolving { .. } => {}
            }
        }
        self.on_project(id, result)
    }

    /// Memory cannot be used: a pane waiting for its project or its context
    /// stops waiting and says why, once per view. In any other state the
    /// reply changes nothing.
    fn on_unavailable(&mut self, id: PaneId, reason: &Reason) {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        let text = reason.text();
        let resolving = matches!(chat.readiness(), Readiness::Resolving { .. });
        if chat.memory_unavailable(&text) {
            self.notice = Some(note::not_ready(&text));
        }
        let fetching = chat.finish_context();
        if resolving || fetching {
            chat.degrade_once(reason.line());
        }
    }

    /// The link rows still to write for chat `id`: its session and its notes,
    /// once the conversation has a database id. Each is handed out once.
    pub(super) fn link_effects(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(conversation) = chat.conversation() else {
            return Vec::new();
        };
        let session = chat.take_session_link().map(|(session, replace)| {
            Effect::Store(StoreCmd::LinkSession {
                pane: id,
                conversation,
                session_id: session.id,
                project: session.project,
                replace,
            })
        });
        let notes = chat.take_unlinked_notes().into_iter().map(|note| {
            Effect::Store(StoreCmd::LinkNote {
                pane: id,
                conversation,
                project: note.project,
                topic_key: note.topic_key,
                observation_id: note.observation_id,
                title: note.title,
            })
        });
        session.into_iter().chain(notes).collect()
    }

    /// The store thread's answer. Only what concerns memory is consumed here:
    /// the conversation id with the eviction lines, and the session and note
    /// links. The rest belongs to the history wiring.
    pub(super) fn on_store_reply(&mut self, reply: &StoreReply) -> Vec<Effect> {
        match reply {
            StoreReply::Created {
                pane,
                conversation,
                created_at,
                ..
            } => {
                let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                chat.set_created(*conversation, *created_at);
                for line in eviction_lines_of(reply) {
                    chat.push_system(line);
                }
                self.dirty = true;
                self.link_effects(*pane)
            }
            StoreReply::SessionLinked {
                pane,
                session_id,
                project,
                ..
            } => {
                if let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut) {
                    // The pair the store holds is the one engram registered.
                    if chat
                        .session()
                        .is_none_or(|s| s.id != *session_id || s.project != *project)
                    {
                        chat.replace_session(session_id, project);
                    }
                }
                Vec::new()
            }
            StoreReply::LinkFailed { pane, .. } => {
                if let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut)
                    && chat.warn_not_linked_once()
                {
                    chat.push_system(NOT_LINKED);
                    self.dirty = true;
                }
                Vec::new()
            }
            StoreReply::Unavailable { .. }
            | StoreReply::Failed { .. }
            | StoreReply::Listed { .. }
            | StoreReply::ListFailed { .. }
            | StoreReply::Resumed { .. }
            | StoreReply::ResumeFailed { .. }
            | StoreReply::Deleted { .. }
            | StoreReply::DeleteFailed { .. }
            | StoreReply::TagsSaved { .. }
            | StoreReply::TagsFailed { .. }
            | StoreReply::RecentFolders { .. }
            | StoreReply::RunsRecovered { .. }
            | StoreReply::RunsFailed { .. } => Vec::new(),
        }
    }

    /// The `End` commands of the chats among `ids` that wrote a note or kept
    /// an approved summary, each once. The caller emits them before the pane
    /// or the app goes away. A summary alone takes a new session here, which
    /// the worker registers with the end (and is linked to the conversation).
    pub(super) fn end_effects(&mut self, ids: &[PaneId]) -> Vec<Effect> {
        let memory_on = self.ai.memory.active();
        let new_id = self.session_id;
        // The text a closing chat has written so far is saved first.
        let mut effects = self.close_store_effects(ids);
        for id in ids {
            let Some(chat) = self.panes.get_mut(id).and_then(PaneState::chat_mut) else {
                continue;
            };
            // Memory was found off at close: what the user approved for
            // engram does not go (turning it off clears it already).
            if !memory_on {
                chat.drop_approved_summary();
            }
            let with_summary = chat.approved_summary().is_some();
            let fresh = chat.adopt_summary_session(new_id);
            let cwd = chat
                .origin_cwd()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Some((session, project, summary)) = chat.take_end() else {
                continue;
            };
            if fresh {
                effects.extend(self.link_effects(*id));
            }
            effects.push(Effect::Memory(MemCmd::End {
                session,
                project,
                summary,
                cwd: with_summary.then_some(cwd),
            }));
        }
        effects
    }

    /// Every open chat's `End`, the discard of each chat's batch (the runtime
    /// counts the entries for the notice at exit), then the quit. No badge is
    /// asked for: one staged since the last poll has none yet, and the discard
    /// of an absent or empty batch is a no-op.
    pub(super) fn quit_effects(&mut self) -> Vec<Effect> {
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort();
        let mut effects = self.end_effects(&ids);
        let holds = |id: &&PaneId| {
            self.panes
                .get(*id)
                .and_then(PaneState::chat_state)
                .is_some()
        };
        effects.extend(ids.iter().filter(holds).map(|id| Effect::BatchClose(*id)));
        effects.push(Effect::Quit);
        effects
    }
}

/// A save that did not go through: its editor shows why and keeps the text;
/// when that editor is gone (Esc, or another one is open) the transcript says
/// so instead.
pub(super) fn save_not_done(chat: &mut ChatState, save: Option<&PendingSave>, reason: &str) {
    // A save the app sent on its own (the rolling summary) fails silently.
    if save.is_some_and(|s| s.quiet) {
        return;
    }
    if !chat.fail_save(save, reason) {
        chat.push_system(format!("note not saved: {reason}"));
    }
}
