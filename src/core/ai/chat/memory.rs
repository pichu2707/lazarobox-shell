//! What a chat knows about engram: whether its project is known, the note
//! editor, the session of the conversation and the notes it saved. Pure.
//!
//! The app asks the memory worker and the store thread; the answers are
//! recorded here so that every decision (what to link, what to end) is a
//! plain function of this state.

use std::time::{Duration, Instant};

use super::{
    ChatState, Entry,
    note::{NOTHING_TO_SAVE, NoteEditor},
    yank,
};
use crate::core::ai::memory::{
    context::ContextBlock,
    port::{Reason, SaveId},
    reply::ProjectError,
};

/// The longest end-of-session summary, in characters.
pub const END_SUMMARY_CAP: usize = 1000;
/// How long a view waits for a memory reply (project, context or a save)
/// before it gives up. The worker can lose a reply when its reply channel is
/// full, so no wait is unbounded: a health probe (300 ms) and a read (800 ms)
/// behind a few 2 s writes fit in it.
pub const MEMORY_WAIT: Duration = Duration::from_secs(5);
/// Text of `Memory is not ready` when the project reply was not understood.
const UNEXPECTED_REPLY: &str = "engram sent an unexpected reply";

/// The memory line at the top of the transcript: how many notes the model was
/// given, and the text it was given (shown when the line is unfolded).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MemoryLine {
    text: String,
    notes: usize,
    expanded: bool,
}

impl MemoryLine {
    /// The injected block, as sent.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The notes the block holds.
    pub fn notes(&self) -> usize {
        self.notes
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    /// The folded or unfolded head: `▸ memory (3 notes)` / `▾ memory (1 note)`.
    pub fn head(&self) -> String {
        let glyph = if self.expanded { '▾' } else { '▸' };
        let noun = if self.notes == 1 { "note" } else { "notes" };
        format!("{glyph} memory ({} {noun})", self.notes)
    }
}

/// The system line for a project that cannot be used. A directory that is not
/// a git project has its own wording; the rest say why memory is unavailable.
pub fn project_error_line(error: &ProjectError) -> String {
    match error {
        ProjectError::NotGit => Reason::NotGit.line(),
        ProjectError::Invalid => format!("memory unavailable: {UNEXPECTED_REPLY}"),
    }
}

/// What the statusline says about memory over a focused chat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemBadge {
    /// The project is known.
    Ready,
    /// The project is being asked for, or not asked yet.
    Checking,
    /// Memory cannot be used for this chat.
    Unavailable,
}

/// Where the one context request of a view is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fetch {
    /// Not asked yet.
    Idle,
    /// Asked, no answer yet.
    Pending,
    /// Answered, or given up on.
    Done,
}

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
    /// When it was sent: the answer is awaited for `MEMORY_WAIT`.
    since: Instant,
    /// Sent by the app, not by the user (the rolling summary): its answer
    /// adds no transcript line and a failure says nothing.
    pub quiet: bool,
}

/// The waits that ran out (see `ChatState::expire_waits`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Expired {
    /// The project request, with whether a note editor was waiting on it.
    pub resolve: Option<bool>,
    /// The context request.
    pub context: bool,
    /// The saves that got no answer.
    pub saves: Vec<PendingSave>,
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
    /// When the store created that conversation in this run. `None` for one
    /// resumed: it names no lifetime this run knows from message 0.
    born: Option<i64>,
    link_warned: bool,
    ended: bool,
    pending_saves: Vec<PendingSave>,
    /// The last save token handed out.
    saves_sent: u64,
    /// How many note editors this chat opened: the number of the open one.
    editors: u64,
    /// The block the next turns carry, once the context reply arrived.
    injected: Option<ContextBlock>,
    context: Fetch,
    /// When the project request was sent, while it awaits its answer.
    resolving_since: Option<Instant>,
    /// When the context request was sent, while it awaits its answer.
    context_since: Option<Instant>,
    /// The `memory unavailable` line was shown for this view.
    degraded: bool,
    /// Bumped when the chat folder changes: a project or context reply from
    /// an older epoch belongs to the old folder and is ignored.
    epoch: u32,
    /// The next session link replaces the stored one: the project changed.
    relink: bool,
}

impl ChatMemory {
    pub(super) fn new() -> Self {
        Self {
            readiness: Readiness::Unknown,
            session: None,
            session_linked: false,
            notes: Vec::new(),
            conversation: None,
            born: None,
            link_warned: false,
            ended: false,
            pending_saves: Vec::new(),
            saves_sent: 0,
            editors: 0,
            injected: None,
            context: Fetch::Idle,
            resolving_since: None,
            context_since: None,
            degraded: false,
            epoch: 0,
            relink: false,
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

    /// The statusline badge for this chat's memory.
    pub fn mem_badge(&self) -> MemBadge {
        match self.memory.readiness {
            Readiness::Ready(_) => MemBadge::Ready,
            Readiness::Unknown | Readiness::Resolving { .. } => MemBadge::Checking,
            Readiness::Unavailable(_) => MemBadge::Unavailable,
        }
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
        self.start_resolving_at(want_note, Instant::now());
    }

    /// `start_resolving` with the clock injected: `now` starts the wait. Asking
    /// again while a request is out keeps the first start, so the wait never
    /// grows.
    pub fn start_resolving_at(&mut self, want_note: bool, now: Instant) {
        if !matches!(self.memory.readiness, Readiness::Resolving { .. }) {
            self.memory.resolving_since = Some(now);
        }
        self.memory.readiness = Readiness::Resolving { want_note };
    }

    /// Memory cannot be used for this chat, whatever it was waiting for.
    pub fn set_unavailable(&mut self, reason: &str) {
        self.memory.resolving_since = None;
        self.memory.readiness = Readiness::Unavailable(reason.to_owned());
    }

    /// The project reply. Returns whether the note editor was wanted.
    pub fn project_resolved(&mut self, result: Result<String, ProjectError>) -> bool {
        let want = self.take_want_note();
        self.memory.resolving_since = None;
        self.memory.readiness = match result {
            Ok(project) => Readiness::Ready(project),
            Err(ProjectError::NotGit) => Readiness::Unavailable(Reason::NotGit.text()),
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
        self.memory.resolving_since = None;
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
        self.entries.retain(|e| !matches!(e, Entry::Memory(_)));
        // A block with no note (sessions only) is still sent, but there is
        // nothing to announce.
        if block.notes > 0 {
            self.entries.insert(
                0,
                Entry::Memory(MemoryLine {
                    text: block.text.clone(),
                    notes: block.notes,
                    expanded: false,
                }),
            );
        }
        self.memory.injected = Some(block);
        self.generation += 1;
    }

    /// `inject_memory` that keeps a scrolled view on the same content: the
    /// line appears above everything, and `width` and `height` are the
    /// transcript's.
    pub fn inject_memory_at(&mut self, block: ContextBlock, width: usize, height: usize) {
        let had = self.has_memory_line();
        let anchor = self.anchor(width, height);
        self.inject_memory(block);
        let has = self.has_memory_line();
        let anchor =
            anchor.map(|(entry, row)| ((entry + usize::from(has)) - usize::from(had), row));
        self.restore(anchor, width, height);
    }

    /// Memory was turned off: the chat forgets its block and drops the line,
    /// and a context request still out is given up on (its late reply finds
    /// nobody waiting). `width` and `height` are the transcript's, to keep a
    /// scrolled view on the same content.
    pub fn clear_memory_at(&mut self, width: usize, height: usize) {
        // Memory off: what the user approved for engram goes too.
        self.approved_summary = None;
        if self.memory.injected.is_none()
            && !self.has_memory_line()
            && self.memory.context != Fetch::Pending
        {
            return;
        }
        let had = self.has_memory_line();
        let anchor = self.anchor(width, height);
        self.entries.retain(|e| !matches!(e, Entry::Memory(_)));
        self.memory.injected = None;
        if self.memory.context == Fetch::Pending {
            self.memory.context = Fetch::Done;
            self.memory.context_since = None;
        }
        self.generation += 1;
        let anchor = anchor.map(|(entry, row)| (entry.saturating_sub(usize::from(had)), row));
        self.restore(anchor, width, height);
    }

    /// Shows or hides the memory line for the block the chat holds, keeping a
    /// scrolled view on the same content: the line is shown only while the
    /// engine can carry the block. Nothing changes without a block or when the
    /// line is already as asked.
    pub fn show_memory_line_at(&mut self, show: bool, width: usize, height: usize) {
        let had = self.has_memory_line();
        if had == show {
            return;
        }
        let anchor = self.anchor(width, height);
        if show {
            let Some(block) = self.memory.injected.clone().filter(|b| b.notes > 0) else {
                return;
            };
            self.entries.insert(
                0,
                Entry::Memory(MemoryLine {
                    text: block.text,
                    notes: block.notes,
                    expanded: false,
                }),
            );
        } else {
            self.entries.retain(|e| !matches!(e, Entry::Memory(_)));
        }
        self.generation += 1;
        let has = self.has_memory_line();
        let anchor = anchor.map(|(entry, row)| {
            (
                (entry + usize::from(has)).saturating_sub(usize::from(had)),
                row,
            )
        });
        self.restore(anchor, width, height);
    }

    fn has_memory_line(&self) -> bool {
        self.entries.iter().any(|e| matches!(e, Entry::Memory(_)))
    }

    /// Unfolds or folds the memory line. False when there is none.
    pub fn toggle_memory(&mut self) -> bool {
        let line = self.entries.iter_mut().find_map(|e| match e {
            Entry::Memory(line) => Some(line),
            _ => None,
        });
        let Some(line) = line else {
            return false;
        };
        line.expanded = !line.expanded;
        self.generation += 1;
        true
    }

    /// `toggle_memory` that keeps a scrolled view on the same content.
    pub fn toggle_memory_at(&mut self, width: usize, height: usize) -> bool {
        let anchor = self.anchor(width, height);
        let changed = self.toggle_memory();
        if changed {
            self.restore(anchor, width, height);
        }
        changed
    }

    /// The view is about to ask for its context. True the first time only: a
    /// view makes one context request.
    pub fn start_context(&mut self) -> bool {
        self.start_context_at(Instant::now())
    }

    /// `start_context` with the clock injected: `now` starts the wait.
    pub fn start_context_at(&mut self, now: Instant) -> bool {
        if self.memory.context != Fetch::Idle {
            return false;
        }
        self.memory.context = Fetch::Pending;
        self.memory.context_since = Some(now);
        true
    }

    /// The context request was answered, or given up on. False when none was
    /// pending: a reply nobody awaits is to be ignored.
    pub fn finish_context(&mut self) -> bool {
        if self.memory.context != Fetch::Pending {
            return false;
        }
        self.memory.context = Fetch::Done;
        self.memory.context_since = None;
        true
    }

    /// Whether the context request has no answer yet.
    pub fn context_pending(&self) -> bool {
        self.memory.context == Fetch::Pending
    }

    /// Shows the `memory unavailable` system line, once per view. Returns
    /// whether it was added.
    pub fn degrade_once(&mut self, line: String) -> bool {
        if std::mem::replace(&mut self.memory.degraded, true) {
            return false;
        }
        self.push_system(line);
        true
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
        // One memory view at a time.
        if self.editor.is_some()
            || self.draft.is_some()
            || self.summarizing.is_some()
            || self.tag_editor.is_some()
        {
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
    /// The flag says the stored link is replaced (the folder, so the project,
    /// changed since the conversation had one).
    pub fn take_session_link(&mut self) -> Option<(SessionRef, bool)> {
        if self.memory.session_linked {
            return None;
        }
        let session = self.memory.session.clone()?;
        self.memory.session_linked = true;
        Some((session, std::mem::take(&mut self.memory.relink)))
    }

    /// The epoch the memory requests of this view carry.
    pub fn memory_epoch(&self) -> u32 {
        self.memory.epoch
    }

    /// The folder changed, so the project did: forget what memory knew (the
    /// project, the block and its line, the degraded mark, the session and
    /// its notes) and make every reply still on its way stale. The caller
    /// takes the end of the old session first (`take_end`).
    pub fn reset_memory_for_folder(&mut self) {
        let memory = &mut self.memory;
        memory.epoch += 1;
        memory.readiness = Readiness::Unknown;
        memory.resolving_since = None;
        memory.injected = None;
        memory.context = Fetch::Idle;
        memory.context_since = None;
        memory.degraded = false;
        memory.session = None;
        memory.session_linked = false;
        memory.relink = true;
        memory.notes.clear();
        memory.ended = false;
        self.approved_summary = None;
        self.entries.retain(|e| !matches!(e, Entry::Memory(_)));
        self.generation += 1;
    }

    /// A save is about to be sent under `project`: its token, to put on the
    /// command. Its answer is awaited.
    pub fn save_sent(&mut self, project: &str) -> SaveId {
        self.save_sent_at(project, Instant::now())
    }

    /// `save_sent` with the clock injected: `now` starts the wait for its
    /// answer.
    pub fn save_sent_at(&mut self, project: &str, now: Instant) -> SaveId {
        self.memory.saves_sent += 1;
        let id = SaveId(self.memory.saves_sent);
        self.memory.pending_saves.push(PendingSave {
            id,
            project: project.to_owned(),
            editor: self.memory.editors,
            since: now,
            quiet: false,
        });
        id
    }

    /// `save_sent_at` for a save the app sends on its own. It belongs to no
    /// note editor, so its answer never closes or fails one.
    pub fn quiet_save_sent_at(&mut self, project: &str, now: Instant) -> SaveId {
        let id = self.save_sent_at(project, now);
        if let Some(save) = self.memory.pending_saves.last_mut() {
            save.editor = 0;
            save.quiet = true;
        }
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

    /// Whether any memory reply is awaited: the project, the context or a
    /// save. The runtime keeps a timer armed exactly while this holds.
    pub fn memory_waiting(&self) -> bool {
        self.memory.resolving_since.is_some()
            || self.memory.context_since.is_some()
            || !self.memory.pending_saves.is_empty()
    }

    /// Gives up on every wait older than `MEMORY_WAIT` at `now`. A project
    /// request that runs out leaves memory unavailable (`engram did not
    /// answer`); a context request is done; a save is no longer pending, so
    /// its late answer is not tied to an editor. What it returns says what
    /// was given up on, for the lines and notices.
    pub fn expire_waits(&mut self, now: Instant) -> Expired {
        let ran_out = |since: Instant| now.saturating_duration_since(since) >= MEMORY_WAIT;
        let mut expired = Expired::default();
        if let (Readiness::Resolving { want_note }, Some(since)) =
            (&self.memory.readiness, self.memory.resolving_since)
            && ran_out(since)
        {
            expired.resolve = Some(*want_note);
            self.set_unavailable(&Reason::NoAnswer.text());
        }
        if self.memory.context == Fetch::Pending && self.memory.context_since.is_some_and(ran_out) {
            expired.context = true;
            self.memory.context = Fetch::Done;
            self.memory.context_since = None;
        }
        let (late, waiting) = std::mem::take(&mut self.memory.pending_saves)
            .into_iter()
            .partition(|save| ran_out(save.since));
        self.memory.pending_saves = waiting;
        expired.saves = late;
        expired
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

    /// A conversation the store has not created in this run (a resume).
    pub fn set_conversation(&mut self, conversation: i64) {
        self.memory.conversation = Some(conversation);
        self.memory.born = None;
    }

    /// The store just created the conversation, at `created_at` seconds.
    pub fn set_created(&mut self, conversation: i64, created_at: i64) {
        self.memory.conversation = Some(conversation);
        self.memory.born = Some(created_at);
    }

    /// The engram key of the rolling summary of this conversation, unique for
    /// its lifetime: the id alone is reused once the conversation is deleted,
    /// so the creation time joins it. `None` for a resumed conversation: its
    /// rolling summary is not persisted, so after a resume it covers only the
    /// later messages and must not replace the fuller one already saved.
    pub fn summary_key(&self) -> Option<String> {
        let id = self.memory.conversation?;
        let born = self.memory.born?;
        Some(format!("chat/{id}-{born}/summary"))
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
        // A session adopted from the store (a resume) that this view never
        // wrote to has nothing to end.
        if self.memory.notes.is_empty() && self.approved_summary.is_none() {
            return None;
        }
        let session = self.memory.session.clone()?;
        self.memory.ended = true;
        let titles: Vec<&str> = self.memory.notes.iter().map(|n| n.title.as_str()).collect();
        // The approved summary wins over the note titles.
        let summary: String = match &self.approved_summary {
            Some(approved) => approved.clone(),
            None => format!("Notes: {}", titles.join("; "))
                .chars()
                .take(END_SUMMARY_CAP)
                .collect(),
        };
        Some((session.id, session.project, summary))
    }
}
