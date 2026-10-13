//! What a chat knows about engram: whether its project is known, the note
//! editor, the session of the conversation and the notes it saved. Pure.
//!
//! The app asks the memory worker and the store thread; the answers are
//! recorded here so that every decision (what to link, what to end) is a
//! plain function of this state.

use super::{
    ChatState, Entry,
    note::{NOTHING_TO_SAVE, NoteEditor},
    yank,
};
use crate::core::ai::memory::{context::ContextBlock, port::SaveId, reply::ProjectError};

/// The longest end-of-session summary, in characters.
pub const END_SUMMARY_CAP: usize = 1000;
/// Text of `Memory is not ready` when the project reply was not understood.
const UNEXPECTED_REPLY: &str = "engram sent an unexpected reply";

/// Whether the chat's engram project is known.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Readiness {
    /// Nothing asked yet.
    Unknown,
    /// The project was asked; `want_note` opens the note editor when it comes.
    Resolving { want_note: bool },
    /// The project of the chat's directory.
    Ready(String),
    /// Memory cannot be used; the reason is already shown text.
    Unavailable(String),
}

/// The engram session of a conversation and the project it was registered
/// with (engram rejects a save under another project).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SessionRef {
    pub id: String,
    pub project: String,
}

/// A note this conversation saved.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SavedNote {
    pub project: String,
    pub topic_key: String,
    pub observation_id: i64,
    pub title: String,
    /// Whether its link row was handed to the store.
    linked: bool,
}

/// A save sent to the worker whose answer has not come. A reply names its
/// save by `id`: matching by queue order would hand the answer of one save to
/// another when a later save is refused at once (queue full) while an earlier
/// one is still queued.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PendingSave {
    /// The token the save carries and its reply echoes.
    pub id: SaveId,
    /// The project the save was sent under: the note is recorded and linked
    /// under exactly this one.
    pub project: String,
    /// Which editor sent it (see `ChatMemory::editors`).
    editor: u64,
}

/// The memory slot of a chat.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatMemory {
    readiness: Readiness,
    session: Option<SessionRef>,
    /// Whether the session link was handed to the store (or came from it).
    session_linked: bool,
    notes: Vec<SavedNote>,
    /// The database id of the conversation, once the store created it.
    conversation: Option<i64>,
    link_warned: bool,
    ended: bool,
    pending_saves: Vec<PendingSave>,
    /// The last save token handed out.
    saves_sent: u64,
    /// How many note editors this chat opened: the number of the open one.
    editors: u64,
    /// The block the next turns carry, once the context reply arrived.
    injected: Option<ContextBlock>,
}

impl ChatMemory {
    pub(super) fn new() -> Self {
        Self {
            readiness: Readiness::Unknown,
            session: None,
            session_linked: false,
            notes: Vec::new(),
            conversation: None,
            link_warned: false,
            ended: false,
            pending_saves: Vec::new(),
            saves_sent: 0,
            editors: 0,
            injected: None,
        }
    }

    pub(super) fn injected(&self) -> Option<&ContextBlock> {
        self.injected.as_ref()
    }
}

impl ChatState {
    pub fn readiness(&self) -> &Readiness {
        &self.memory.readiness
    }

    /// The engram project, once it is known.
    pub fn project(&self) -> Option<&str> {
        match &self.memory.readiness {
            Readiness::Ready(project) => Some(project),
            _ => None,
        }
    }

    /// The project was asked for; `want_note` opens the editor on the answer.
    pub fn start_resolving(&mut self, want_note: bool) {
        self.memory.readiness = Readiness::Resolving { want_note };
    }

    /// Memory cannot be used for this chat, whatever it was waiting for.
    pub fn set_unavailable(&mut self, reason: &str) {
        self.memory.readiness = Readiness::Unavailable(reason.to_owned());
    }

    /// The project reply. Returns whether the note editor was wanted.
    pub fn project_resolved(&mut self, result: Result<String, ProjectError>) -> bool {
        let want = self.take_want_note();
        self.memory.readiness = match result {
            Ok(project) => Readiness::Ready(project),
            Err(ProjectError::NotGit) => {
                Readiness::Unavailable(crate::core::ai::memory::port::Reason::NotGit.text())
            }
            Err(ProjectError::Invalid) => Readiness::Unavailable(UNEXPECTED_REPLY.to_owned()),
        };
        want
    }

    /// Memory said it is unavailable while the project was asked. Returns
    /// whether the note editor was wanted. Ignored in any other state.
    pub fn memory_unavailable(&mut self, reason: &str) -> bool {
        if !matches!(self.memory.readiness, Readiness::Resolving { .. }) {
            return false;
        }
        let want = self.take_want_note();
        self.memory.readiness = Readiness::Unavailable(reason.to_owned());
        want
    }

    fn take_want_note(&mut self) -> bool {
        matches!(
            self.memory.readiness,
            Readiness::Resolving { want_note: true }
        )
    }

    /// The context reply of the view: the block the turns sent from now on
    /// carry in their request. A turn already in flight keeps what it was
    /// sent with, and the block is never part of the history.
    pub fn inject_memory(&mut self, block: ContextBlock) {
        self.memory.injected = Some(block);
    }

    /// The note editor, while it is open.
    pub fn note_editor(&self) -> Option<&NoteEditor> {
        self.editor.as_ref()
    }

    pub fn note_editor_mut(&mut self) -> Option<&mut NoteEditor> {
        self.editor.as_mut()
    }

    /// Opens the note editor on the last finished or cancelled answer (never
    /// the reasoning). An editor already open stays as it is. `Err` holds the
    /// notice when there is no answer.
    pub fn open_note_editor(&mut self) -> Result<(), String> {
        if self.editor.is_some() {
            return Ok(());
        }
        let answer = yank::last_answer(self).ok_or_else(|| NOTHING_TO_SAVE.to_owned())?;
        self.editor = Some(NoteEditor::seeded(&answer));
        self.memory.editors += 1;
        Ok(())
    }

    pub fn close_note_editor(&mut self) {
        self.editor = None;
    }

    /// Adds a system line to the transcript.
    pub fn push_system(&mut self, line: impl Into<String>) {
        self.entries.push(Entry::System(line.into()));
        self.generation += 1;
    }

    /// The engram session of the conversation, once a note was written.
    pub fn session(&self) -> Option<&SessionRef> {
        self.memory.session.as_ref()
    }

    /// Records the session a first save created. Returns whether it was set
    /// (a conversation keeps the first session it had).
    pub fn adopt_session(&mut self, id: &str, project: &str) -> bool {
        if self.memory.session.is_some() {
            return false;
        }
        self.memory.session = Some(SessionRef {
            id: id.to_owned(),
            project: project.to_owned(),
        });
        true
    }

    /// The store knew another pair for this conversation (a resume): it wins,
    /// and it is already linked.
    pub fn replace_session(&mut self, id: &str, project: &str) {
        self.memory.session = Some(SessionRef {
            id: id.to_owned(),
            project: project.to_owned(),
        });
        self.memory.session_linked = true;
    }

    /// The session that still has to be linked, once. `None` when there is
    /// none or it was handed to the store already.
    pub fn take_session_link(&mut self) -> Option<SessionRef> {
        if self.memory.session_linked {
            return None;
        }
        let session = self.memory.session.clone()?;
        self.memory.session_linked = true;
        Some(session)
    }

    /// A save is about to be sent under `project`: its token, to put on the
    /// command. Its answer is awaited.
    pub fn save_sent(&mut self, project: &str) -> SaveId {
        self.memory.saves_sent += 1;
        let id = SaveId(self.memory.saves_sent);
        self.memory.pending_saves.push(PendingSave {
            id,
            project: project.to_owned(),
            editor: self.memory.editors,
        });
        id
    }

    /// The save with token `id`, now answered. `None` for a token this chat
    /// does not await (already answered, or never sent).
    pub fn save_answered(&mut self, id: SaveId) -> Option<PendingSave> {
        let at = self.memory.pending_saves.iter().position(|s| s.id == id)?;
        Some(self.memory.pending_saves.remove(at))
    }

    /// The save with token `id`, while it awaits its answer.
    pub fn pending_save(&self, id: SaveId) -> Option<PendingSave> {
        self.memory
            .pending_saves
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    /// The tokens of the saves still awaiting their answer, oldest first.
    pub fn pending_save_ids(&self) -> Vec<SaveId> {
        self.memory.pending_saves.iter().map(|s| s.id).collect()
    }

    /// The editor that sent `save`, while it is open and waiting. Without a
    /// record of the save, none: an answer nobody awaits touches no editor.
    fn editor_waiting_for(&mut self, save: Option<&PendingSave>) -> Option<&mut NoteEditor> {
        let current = self.memory.editors;
        self.editor
            .as_mut()
            .filter(|e| e.is_saving() && save.is_some_and(|s| s.editor == current))
    }

    /// The save failed: the editor that sent it shows `reason` and keeps its
    /// text. Returns false when that editor is gone (or is another one).
    pub fn fail_save(&mut self, save: Option<&PendingSave>, reason: &str) -> bool {
        match self.editor_waiting_for(save) {
            Some(editor) => {
                editor.save_failed(reason);
                true
            }
            None => false,
        }
    }

    /// The save went through: the editor that sent it closes; one the user
    /// opened since stays.
    pub fn close_saved_editor(&mut self, save: Option<&PendingSave>) {
        if self.editor_waiting_for(save).is_some() {
            self.editor = None;
        }
    }

    /// The database id of the conversation, if the store created it.
    pub fn conversation(&self) -> Option<i64> {
        self.memory.conversation
    }

    pub fn set_conversation(&mut self, conversation: i64) {
        self.memory.conversation = Some(conversation);
    }

    /// Records a note this conversation saved. The same `topic_key` is an
    /// upsert: one entry, the latest title, its link written again.
    pub fn record_note(
        &mut self,
        project: &str,
        topic_key: &str,
        observation_id: i64,
        title: &str,
    ) {
        let note = SavedNote {
            project: project.to_owned(),
            topic_key: topic_key.to_owned(),
            observation_id,
            title: title.to_owned(),
            linked: false,
        };
        match self
            .memory
            .notes
            .iter_mut()
            .find(|n| n.project == project && n.topic_key == topic_key)
        {
            Some(known) => *known = note,
            None => self.memory.notes.push(note),
        }
    }

    /// The notes whose link was not handed to the store yet.
    pub fn unlinked_notes(&self) -> Vec<&SavedNote> {
        self.memory.notes.iter().filter(|n| !n.linked).collect()
    }

    /// The notes whose link was not handed to the store yet, marking them
    /// handed.
    pub fn take_unlinked_notes(&mut self) -> Vec<SavedNote> {
        self.memory
            .notes
            .iter_mut()
            .filter(|n| !n.linked)
            .map(|n| {
                n.linked = true;
                n.clone()
            })
            .collect()
    }

    /// True the first time only: the line `note saved but not linked` shows
    /// once per conversation.
    pub fn warn_not_linked_once(&mut self) -> bool {
        !std::mem::replace(&mut self.memory.link_warned, true)
    }

    /// What ends the engram session: its id, project and the summary
    /// `Notes: A; B` (at most 1,000 characters). Once; `None` when the
    /// conversation never wrote a note, or the end was taken already.
    pub fn take_end(&mut self) -> Option<(String, String, String)> {
        if self.memory.ended {
            return None;
        }
        let session = self.memory.session.clone()?;
        self.memory.ended = true;
        let titles: Vec<&str> = self.memory.notes.iter().map(|n| n.title.as_str()).collect();
        let summary: String = format!("Notes: {}", titles.join("; "))
            .chars()
            .take(END_SUMMARY_CAP)
            .collect();
        Some((session.id, session.project, summary))
    }
}
