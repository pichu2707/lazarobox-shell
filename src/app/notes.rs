//! Save as note (engram-memory M7b): the save-note key, the note editor's
//! keys and the memory worker's replies that concern a chat.

use super::*;
use crate::core::ai::chat::{
    memory::Readiness,
    note::{self, MEMORY_CHECKING, MEMORY_OFF, NOT_LINKED, NOTHING_TO_SAVE, NoteOutcome},
    store::eviction_lines_of,
    yank,
};

impl App {
    /// Whether chat `id` has its note editor open.
    pub(super) fn note_editor_open(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.note_editor().is_some())
    }

    /// Ctrl+N: opens the note editor on the last answer. It needs memory on,
    /// an answer and a known project; when the project is not known it is
    /// asked for, and the editor opens when the reply comes.
    pub(super) fn chat_note_key(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.ai.memory.active() {
            self.notice = Some(MEMORY_OFF.to_owned());
            return Vec::new();
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if yank::last_answer(chat).is_none() {
            self.notice = Some(NOTHING_TO_SAVE.to_owned());
            return Vec::new();
        }
        let cwd = chat.origin_cwd().map(|p| p.to_string_lossy().into_owned());
        let probe = |chat: &mut ChatState, want_note: bool| {
            chat.start_resolving(want_note);
            cwd.clone()
                .map(|cwd| Effect::Memory(MemCmd::Resolve { pane: id, cwd }))
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
                chat.start_resolving(true);
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
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(editor) = chat.note_editor_mut() else {
            return Vec::new();
        };
        self.dirty = true;
        match editor.on_key(key, repeat) {
            NoteOutcome::Handled => Vec::new(),
            NoteOutcome::Discard => {
                chat.close_note_editor();
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
                chat.save_sent(&project);
                vec![Effect::Memory(MemCmd::Save {
                    pane: id,
                    session: chat.session().map(|s| s.id.clone()),
                    project,
                    cwd: chat
                        .origin_cwd()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    note,
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
            return Vec::new();
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let saving = chat.note_editor().is_some_and(|e| e.is_saving());
        self.dirty = true;
        match reply {
            MemReply::Project { result, .. } => {
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
            }
            MemReply::Unavailable { reason, .. } if saving => {
                chat.save_answered();
                if let Some(editor) = chat.note_editor_mut() {
                    editor.save_failed(&reason.text());
                }
            }
            MemReply::Unavailable { reason, .. } => {
                let text = reason.text();
                if chat.memory_unavailable(&text) {
                    self.notice = Some(note::not_ready(&text));
                }
            }
            MemReply::SaveFailed { reason, .. } => {
                chat.save_answered();
                if let Some(editor) = chat.note_editor_mut() {
                    editor.save_failed(reason);
                }
            }
            MemReply::Saved {
                session,
                observation_id,
                topic_key,
                title,
                ..
            } => {
                // The project the save was sent under, as `Save` chose it.
                let project = chat.save_answered().map(|p| p.project).or_else(|| {
                    chat.session()
                        .map(|s| s.project.clone())
                        .or_else(|| chat.project().map(str::to_owned))
                });
                let Some(project) = project else {
                    return Vec::new();
                };
                chat.adopt_session(session, &project);
                chat.record_note(&project, topic_key, *observation_id, title);
                // The editor that sent this save closes; one the user opened
                // since stays.
                if saving {
                    chat.close_note_editor();
                }
                let kind = topic_key.split('/').nth(1).unwrap_or("note");
                chat.push_system(format!("saved note {kind}: {title}"));
                return self.link_effects(id);
            }
            MemReply::Health { .. }
            | MemReply::Context { .. }
            | MemReply::Deleted { .. }
            | MemReply::Server(_)
            | MemReply::ServerOffer(_) => {}
        }
        Vec::new()
    }

    /// The link rows still to write for chat `id`: its session and its notes,
    /// once the conversation has a database id. Each is handed out once.
    fn link_effects(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(conversation) = chat.conversation() else {
            return Vec::new();
        };
        let session = chat.take_session_link().map(|session| {
            Effect::Store(StoreCmd::LinkSession {
                pane: id,
                conversation,
                session_id: session.id,
                project: session.project,
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
                pane, conversation, ..
            } => {
                let Some(chat) = self.panes.get_mut(pane).and_then(PaneState::chat_mut) else {
                    return Vec::new();
                };
                chat.set_conversation(*conversation);
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
            StoreReply::Unavailable { .. } | StoreReply::Failed { .. } => Vec::new(),
        }
    }

    /// The `End` commands of the chats among `ids` that wrote a note, each
    /// once. The caller emits them before the pane or the app goes away.
    pub(super) fn end_effects(&mut self, ids: &[PaneId]) -> Vec<Effect> {
        ids.iter()
            .filter_map(|id| {
                let (session, project, summary) = self
                    .panes
                    .get_mut(id)
                    .and_then(PaneState::chat_mut)?
                    .take_end()?;
                Some(Effect::Memory(MemCmd::End {
                    session,
                    project,
                    summary,
                }))
            })
            .collect()
    }

    /// Every open chat's `End`, then the quit.
    pub(super) fn quit_effects(&mut self) -> Vec<Effect> {
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort();
        let mut effects = self.end_effects(&ids);
        effects.push(Effect::Quit);
        effects
    }
}
