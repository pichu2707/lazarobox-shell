//! The AI chat core. Pure: no IO lives here.
//!
//! A `ChatState` is the conversation of one chat pane: its transcript, the
//! turn in flight and the cwd the pane was opened from.

pub mod adapters;
pub mod composer;
pub mod copy;
pub mod engine;
pub mod guidance;
pub mod history;
pub mod list;
pub mod memory;
pub mod note;
pub mod notify;
pub mod prompts;
pub mod store;
pub mod stream;
pub mod streams;
pub mod summary;
pub mod tag_editor;
pub mod tags;
pub mod transcript;
pub mod workspace;
pub mod wrap;
pub mod yank;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use crate::core::ai::config::Profile;
use composer::Composer;
use engine::{
    Availability, ChatError, ChatMessage, ChatRequest, Role, StreamEvent, build_context,
    history_chars,
};
use prompts::Resolved;
use transcript::{LineCache, Scroll};

/// The longest answer kept, in bytes. Past it the answer is cut.
pub const ANSWER_CAP: usize = 1 << 20;
/// The most messages one conversation holds.
pub const MAX_MESSAGES: usize = 400;
/// Shown until the first byte of an answer arrives.
pub const WAITING: &str = "waiting for the model…";
/// Shown when the first byte is late (a cold model load).
pub const LOADING: &str = "loading model…";
/// System line for a stream that ended without a final event.
pub const INTERRUPTED: &str = "stream interrupted";
/// System line shown once, before the oldest message the model still sees.
pub const TRUNCATION_MARKER: &str = "earlier messages are not sent to the model";
/// System line for a send while an answer is still streaming.
pub const BUSY: &str = "still answering: wait for it or press Esc to cancel";
/// System line for a send into a full conversation.
pub const FULL: &str = "this conversation is full: start a new one";
/// System line for an answer that ended with no text at all.
pub const NO_ANSWER: &str = "the model sent no answer";
/// System line for an answer cut at `ANSWER_CAP`.
pub const TOO_LONG: &str = "answer too long: cut at 1 MiB";

/// How a message ended. Anything but `Complete` is a partial message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgStatus {
    Complete,
    Cancelled,
    Interrupted,
    Truncated,
    Error,
}

impl MsgStatus {
    pub fn is_partial(self) -> bool {
        self != Self::Complete
    }

    /// The status a stored name stands for.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Complete,
            Self::Cancelled,
            Self::Interrupted,
            Self::Truncated,
            Self::Error,
        ]
        .into_iter()
        .find(|status| status.as_str() == name)
    }

    /// The name stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Truncated => "truncated",
            Self::Error => "error",
        }
    }
}

/// One message of the conversation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Msg {
    pub role: Role,
    pub text: String,
    pub status: MsgStatus,
}

/// One line of the transcript. System lines are never sent or stored.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Entry {
    Message(Msg),
    System(String),
    /// The reasoning of one turn: shown, never stored, never sent back.
    Reasoning(Reasoning),
    /// The memory the conversation was given, first in the transcript: shown,
    /// never stored, never sent as history.
    Memory(memory::MemoryLine),
}

/// The most reasoning text kept per turn, in bytes. Past it the rest is
/// dropped without an error.
pub const REASONING_CAP: usize = 256 * 1024;

/// Where a reasoning block is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReasoningState {
    /// Still growing: shown dimmed, line by line.
    Streaming,
    /// Folded into one line (the default once the turn moved on).
    Collapsed,
    /// Unfolded by the user.
    Expanded,
}

/// The reasoning of one turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reasoning {
    text: String,
    state: ReasoningState,
    started: Instant,
    /// Tenths of a second from the first thinking delta to the collapse.
    tenths: u32,
}

impl Reasoning {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn state(&self) -> ReasoningState {
        self.state
    }

    /// The time the reasoning took, like `2.3s`. Meaningful once collapsed.
    pub fn seconds_label(&self) -> String {
        format!("{}.{}s", self.tenths / 10, self.tenths % 10)
    }
}

/// Where the turn in flight is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    /// Sent, no byte yet; `slow` once the wait passed 3 seconds.
    Waiting {
        slow: bool,
    },
    /// The last message entry is the assistant message being written. System
    /// lines (a refused send's BUSY) may follow it, so look it up among the
    /// messages, not the entries.
    Streaming,
}

/// Where a running stream is, for the statusline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamPhase {
    /// Sent, and no answer text yet (waiting or reasoning).
    Thinking,
    /// Answer text is arriving.
    Answering,
}

/// What a send did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SendOutcome {
    /// The message was appended; the app emits `Effect::ChatSend`.
    Sent { turn: u64, request: ChatRequest },
    /// Nothing was sent; the caller keeps the composer text.
    Refused,
}

/// What the app must do after an event or a cancel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Follow {
    Nothing,
    /// The turn is over here but the engine may still run: emit
    /// `Effect::ChatCancel`.
    CancelEngine,
}

/// The indices of the messages a request may carry: a user message nobody
/// answered (cancelled while waiting, or an error before any text) stays in
/// the transcript but is not sent, so the engine never gets two user messages
/// in a row.
pub(crate) fn sendable_indices(all: &[&Msg]) -> Vec<usize> {
    (0..all.len())
        .filter(|&i| {
            all[i].role != Role::User
                || all
                    .get(i + 1)
                    .is_none_or(|next| next.role == Role::Assistant)
        })
        .collect()
}

/// The state of one chat pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatState {
    origin_cwd: Option<PathBuf>,
    entries: Vec<Entry>,
    phase: Phase,
    turn: u64,
    composer: Composer,
    /// When the stream in flight was sent.
    started: Option<Instant>,
    /// The model of the request in flight, as sent.
    request_model: String,
    /// Moves with every change of the transcript.
    generation: u64,
    scroll: Scroll,
    cache: LineCache,
    /// The system prompt of the conversation, resolved once when it started.
    /// `None` until the runtime has prepared it.
    prompt: Option<String>,
    /// What the chat knows about engram: project, session, saved notes.
    memory: memory::ChatMemory,
    /// The note editor, while it is open over the conversation.
    editor: Option<note::NoteEditor>,
    /// The text a summary request has collected so far; `Some` exactly while
    /// that request holds the stream slot.
    summarizing: Option<String>,
    /// The summary draft, while it is open over the conversation.
    draft: Option<summary::SummaryDraft>,
    /// The summary the user approved: sent once, with the session end.
    approved_summary: Option<String>,
    /// The project tag proposed for the conversation (see `history`).
    tag: Option<String>,
    /// The extra tags of the conversation.
    extras: Vec<String>,
    /// The user edited the tags: the project tag is theirs, not the origin's.
    tags_edited: bool,
    /// The tag editor, while it is open over the conversation or the list.
    tag_editor: Option<tag_editor::TagEditor>,
    /// The conversation list, while it is shown instead of the conversation.
    list: Option<list::ListView>,
    /// The profile of the conversation: the current one when it is new, the
    /// stored one when it was resumed.
    profile: Profile,
    /// The pane's side of its conversation's storage.
    store: store::PaneStore,
    /// Messages to save, in order, until the app takes them.
    outbox: Vec<store::StoredMessage>,
    /// The engine of the request in flight, as sent.
    request_engine: String,
    /// `close_stream` already queued the answer in progress.
    closing: bool,
    /// The profile changed while the first system prompt was still being
    /// prepared: that answer is dropped and the prompt prepared again.
    reprepare_prompt: bool,
}

impl ChatState {
    /// A chat opened from a pane whose cwd was `origin_cwd`.
    pub fn new(origin_cwd: Option<PathBuf>) -> Self {
        Self {
            origin_cwd,
            entries: Vec::new(),
            phase: Phase::Idle,
            turn: 0,
            composer: Composer::new(),
            started: None,
            request_model: String::new(),
            generation: 0,
            scroll: Scroll::Follow,
            cache: LineCache::default(),
            prompt: None,
            memory: memory::ChatMemory::new(),
            editor: None,
            summarizing: None,
            draft: None,
            approved_summary: None,
            tag: None,
            extras: Vec::new(),
            tags_edited: false,
            tag_editor: None,
            list: None,
            profile: Profile::default(),
            store: store::PaneStore::new(),
            outbox: Vec::new(),
            request_engine: String::new(),
            closing: false,
            reprepare_prompt: false,
        }
    }

    /// The message box.
    pub fn composer(&self) -> &Composer {
        &self.composer
    }

    pub fn composer_mut(&mut self) -> &mut Composer {
        &mut self.composer
    }

    /// The rows the composer takes in a pane `width` x `height`: what its text
    /// needs, up to 5, and never the last row left to the transcript (unless
    /// the pane has one row only).
    pub fn composer_rows(&self, width: u16, height: u16) -> u16 {
        if width == 0 || height == 0 {
            return 0;
        }
        Self::clamp_composer_rows(self.composer.height(usize::from(width)), height)
    }

    /// `wanted` composer rows clamped to a pane `height`: the last row stays
    /// with the transcript unless the pane has one row only. Takes the row
    /// count so that a caller holding a `ComposerPlan` need not wrap the text
    /// again.
    pub fn clamp_composer_rows(wanted: usize, height: u16) -> u16 {
        let wanted = u16::try_from(wanted).unwrap_or(u16::MAX);
        wanted.min(height.saturating_sub(1).max(1)).min(height)
    }

    /// The rows left to the transcript in a pane `width` x `height`.
    pub fn transcript_rows(&self, width: u16, height: u16) -> u16 {
        height - self.composer_rows(width, height)
    }

    /// The rows of the transcript that are really drawn in a pane `width` x
    /// `height`: what the composer leaves, less the rows `guidance` takes.
    pub fn viewport_rows(&self, width: u16, height: u16, guidance: Option<&str>) -> u16 {
        let transcript = self.transcript_rows(width, height);
        let taken = guidance.map_or(0, |text| guidance::guidance_rows(text, width, transcript));
        transcript - taken
    }

    /// The cwd of the pane the chat was opened from.
    pub fn origin_cwd(&self) -> Option<&Path> {
        self.origin_cwd.as_deref()
    }

    /// The system prompt of the conversation, once it is prepared.
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }

    /// Pins the system prompt of the conversation and shows the notices of
    /// the override files that were skipped as system lines. Done once: a
    /// later call is ignored, so the prompt never changes mid-conversation.
    pub fn set_prompt(&mut self, resolved: Resolved) {
        if self.prompt.is_some() {
            return;
        }
        self.prompt = Some(resolved.text);
        for notice in resolved.notices {
            self.entries.push(Entry::System(notice));
        }
        self.generation += 1;
    }

    /// The transcript, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The messages of the transcript, without system lines.
    pub fn messages(&self) -> impl Iterator<Item = &Msg> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Message(m) => Some(m),
            Entry::System(_) | Entry::Reasoning(_) | Entry::Memory(_) => None,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The wrapped transcript at `width` columns.
    pub fn lines(&self, width: usize) -> std::sync::Arc<Vec<transcript::Line>> {
        self.cache.get(self, width)
    }

    pub fn scroll(&self) -> Scroll {
        self.scroll
    }

    pub fn set_scroll(&mut self, scroll: Scroll) {
        self.scroll = scroll;
    }

    /// Moves with every change of the transcript; the line cache keys on it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The id events of the turn in flight must carry.
    pub fn turn(&self) -> u64 {
        self.turn
    }

    /// The waiting text, while no byte of the answer has arrived.
    pub fn waiting_text(&self) -> Option<&'static str> {
        match self.phase {
            // Reasoning counts as the first byte: its block replaces the text.
            Phase::Waiting { .. } if self.live_reasoning().is_some() => None,
            Phase::Waiting { slow: false } => Some(WAITING),
            Phase::Waiting { slow: true } => Some(LOADING),
            Phase::Idle | Phase::Streaming => None,
        }
    }

    /// Where the stream is: `Thinking` from the send until the first
    /// non-empty text delta, `Answering` after, `None` when idle.
    pub fn stream_phase(&self) -> Option<StreamPhase> {
        match self.phase {
            Phase::Idle => None,
            Phase::Waiting { .. } => Some(StreamPhase::Thinking),
            Phase::Streaming => Some(StreamPhase::Answering),
        }
    }

    /// When the send of the stream in flight happened.
    pub fn stream_started(&self) -> Option<Instant> {
        self.started
    }

    /// The model of the request as sent; empty when idle or when the engine
    /// picked its own.
    pub fn stream_model(&self) -> &str {
        &self.request_model
    }

    /// Expands or collapses the most recent reasoning block. Returns whether
    /// anything changed: a block that still streams, or no block, is left
    /// alone.
    pub fn toggle_reasoning(&mut self) -> bool {
        let last = self.entries.iter_mut().rev().find_map(|e| match e {
            Entry::Reasoning(r) => Some(r),
            _ => None,
        });
        let Some(block) = last else {
            return false;
        };
        block.state = match block.state {
            ReasoningState::Streaming => return false,
            ReasoningState::Collapsed => ReasoningState::Expanded,
            ReasoningState::Expanded => ReasoningState::Collapsed,
        };
        self.generation += 1;
        true
    }

    /// `toggle_reasoning` that keeps the same content in view: a pinned view
    /// is anchored on (entry, row inside it), so rows added or removed above
    /// the viewport do not shift it. `width` and `height` are the transcript's.
    pub fn toggle_reasoning_at(&mut self, width: usize, height: usize) -> bool {
        let anchor = self.anchor(width, height);
        let changed = self.toggle_reasoning();
        if changed {
            self.restore(anchor, width, height);
        }
        changed
    }

    /// Keeps the content in view when the transcript was `old_width` wide and
    /// is now `new_width` wide (and `height` rows high).
    pub fn rewrap(&mut self, old_width: usize, new_width: usize, height: usize) {
        let anchor = self.anchor(old_width, height);
        self.restore(anchor, new_width, height);
    }

    /// The (entry, row inside it) the top of a pinned view sits on.
    fn anchor(&self, width: usize, height: usize) -> Option<(usize, usize)> {
        let Scroll::Pinned(_) = self.scroll else {
            return None;
        };
        let (lines, starts) = transcript::layout(self, width);
        let top = self.scroll.top(lines.len(), height);
        let entry = starts.iter().rposition(|&s| s <= top)?;
        Some((entry, top - starts[entry]))
    }

    /// Pins the view on `anchor` again, at the current wrapping.
    fn restore(&mut self, anchor: Option<(usize, usize)>, width: usize, height: usize) {
        let Some((entry, offset)) = anchor else {
            return;
        };
        let (lines, starts) = transcript::layout(self, width);
        let Some(&start) = starts.get(entry) else {
            return;
        };
        let end = starts.get(entry + 1).copied().unwrap_or(lines.len());
        // The entry may have lost rows (a collapsed block): stay inside it.
        let top = (start + offset).min(end.saturating_sub(1).max(start));
        let last = lines.len().saturating_sub(height);
        self.scroll = if top >= last {
            Scroll::Follow
        } else {
            Scroll::Pinned(top)
        };
    }

    /// A chat reopened from storage: the stored messages and no reasoning.
    pub fn resumed(origin_cwd: Option<PathBuf>, messages: Vec<Msg>) -> Self {
        let mut state = Self::new(origin_cwd);
        state.entries = messages.into_iter().map(Entry::Message).collect();
        state
    }

    /// Sends `text` to the engine `availability` names, with `system` as the
    /// pinned system prompt.
    pub fn send(&mut self, text: &str, availability: &Availability, system: &str) -> SendOutcome {
        self.send_at(text, availability, system, Instant::now())
    }

    /// `send` with the clock injected: `now` is when the stream starts.
    pub fn send_at(
        &mut self,
        text: &str,
        availability: &Availability,
        system: &str,
        now: Instant,
    ) -> SendOutcome {
        let Availability::Ready { engine, model } = availability else {
            return SendOutcome::Refused;
        };
        if text.trim().is_empty() {
            return SendOutcome::Refused;
        }
        if self.phase != Phase::Idle {
            self.push_system_once(BUSY);
            return SendOutcome::Refused;
        }
        // Room for this message and its answer.
        if self.messages().count() + 2 > MAX_MESSAGES {
            self.push_system_once(FULL);
            return SendOutcome::Refused;
        }
        let text = text.trim_end();
        self.entries.push(Entry::Message(Msg {
            role: Role::User,
            text: text.to_owned(),
            status: MsgStatus::Complete,
        }));
        self.outbox.push(store::StoredMessage {
            role: Role::User,
            text: text.to_owned(),
            status: MsgStatus::Complete,
            engine: engine.clone(),
            model: model.clone(),
        });
        // A user message nobody answered (cancelled while waiting, or an
        // error before any text) stays in the transcript but is not sent:
        // the engine never gets two user messages in a row.
        let all: Vec<&Msg> = self.messages().collect();
        let sendable = sendable_indices(&all);
        let history: Vec<ChatMessage> = sendable
            .iter()
            .map(|&i| ChatMessage {
                role: all[i].role,
                content: all[i].text.clone(),
            })
            .collect();
        // The memory block shares the history budget: its characters come off
        // the cap, and the newest message is kept whole whatever is left.
        // Only an adapter that carries it gets the block, and only then does it
        // take budget.
        let memory = adapters::carries_memory(engine)
            .then(|| self.memory.injected().map(|block| block.text.clone()))
            .flatten();
        let memory_chars = memory.as_ref().map_or(0, |text| text.chars().count());
        let context = build_context(&history, history_chars(engine).saturating_sub(memory_chars));
        let oldest = sendable[history.len() - context.messages.len()];
        self.place_marker(oldest, context.omitted);
        self.turn += 1;
        self.phase = Phase::Waiting { slow: false };
        self.started = Some(now);
        self.request_model = model.clone().unwrap_or_default();
        self.request_engine = engine.clone();
        self.generation += 1;
        self.scroll = Scroll::Follow;
        SendOutcome::Sent {
            turn: self.turn,
            request: ChatRequest {
                engine: engine.clone(),
                model: model.clone(),
                system: system.to_owned(),
                memory,
                messages: context.messages,
            },
        }
    }

    /// Adds a system line unless the transcript already ends with it.
    fn push_system_once(&mut self, line: &str) {
        let entry = Entry::System(line.to_owned());
        if self.entries.last() != Some(&entry) {
            self.entries.push(entry);
        }
    }

    /// Keeps the truncation marker once, before the message at `oldest`
    /// (counting messages only), or removes it when nothing is omitted.
    fn place_marker(&mut self, oldest: usize, omitted: bool) {
        let marker = Entry::System(TRUNCATION_MARKER.to_owned());
        self.entries.retain(|e| *e != marker);
        if !omitted {
            return;
        }
        let at = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Entry::Message(_)))
            .nth(oldest)
            .map_or(self.entries.len(), |(i, _)| i);
        self.entries.insert(at, marker);
    }

    /// Applies one event of turn `turn`. Events of any other turn, and events
    /// while idle, are dropped.
    pub fn apply(&mut self, turn: u64, event: StreamEvent) -> Follow {
        self.apply_at(turn, event, Instant::now())
    }

    /// `apply` with the clock injected.
    pub fn apply_at(&mut self, turn: u64, event: StreamEvent, now: Instant) -> Follow {
        if turn != self.turn || self.phase == Phase::Idle {
            return Follow::Nothing;
        }
        if self.summarizing.is_some() {
            return self.apply_summary(event);
        }
        if !matches!(event, StreamEvent::ThinkingDelta(_)) {
            self.generation += 1;
        }
        match event {
            StreamEvent::Slow => {
                if let Phase::Waiting { slow } = &mut self.phase {
                    *slow = true;
                }
                Follow::Nothing
            }
            StreamEvent::Delta(text) => self.push_delta(&text, now),
            StreamEvent::ThinkingDelta(text) => {
                if self.push_thinking(&text, now) {
                    self.generation += 1;
                }
                Follow::Nothing
            }
            StreamEvent::Done(_) => {
                let line = (self.phase != Phase::Streaming).then(|| NO_ANSWER.to_owned());
                self.finish(MsgStatus::Complete, line, now);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Interrupted) => {
                self.finish(MsgStatus::Interrupted, Some(INTERRUPTED.to_owned()), now);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Truncated) => {
                self.finish(MsgStatus::Truncated, Some(TOO_LONG.to_owned()), now);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Failed(message)) => {
                self.finish(MsgStatus::Error, Some(message), now);
                Follow::Nothing
            }
        }
    }

    /// The reasoning block of the turn in flight, if it has started one.
    fn live_reasoning(&self) -> Option<&Reasoning> {
        if self.phase == Phase::Idle {
            return None;
        }
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Reasoning(r) if r.state == ReasoningState::Streaming => Some(r),
            _ => None,
        })
    }

    fn live_reasoning_mut(&mut self) -> Option<&mut Reasoning> {
        if self.phase == Phase::Idle {
            return None;
        }
        self.entries.iter_mut().rev().find_map(|e| match e {
            Entry::Reasoning(r) if r.state == ReasoningState::Streaming => Some(r),
            _ => None,
        })
    }

    /// Adds reasoning text to the turn's block, starting it on the first
    /// piece. Dropped once the answer has started, and past the cap. Returns
    /// whether the transcript changed.
    fn push_thinking(&mut self, text: &str, now: Instant) -> bool {
        if text.is_empty() || self.phase == Phase::Streaming {
            return false;
        }
        if self.live_reasoning().is_none() {
            self.entries.push(Entry::Reasoning(Reasoning {
                text: String::new(),
                state: ReasoningState::Streaming,
                started: now,
                tenths: 0,
            }));
        }
        let Some(block) = self.live_reasoning_mut() else {
            return false;
        };
        let room = REASONING_CAP - block.text.len();
        let mut cut = text.len().min(room);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        block.text.push_str(&text[..cut]);
        true
    }

    /// Folds the live reasoning block into its one line, with the seconds it
    /// took.
    fn collapse_reasoning(&mut self, now: Instant) {
        if let Some(block) = self.live_reasoning_mut() {
            let ms = now.saturating_duration_since(block.started).as_millis();
            block.tenths = u32::try_from((ms + 50) / 100).unwrap_or(u32::MAX);
            block.state = ReasoningState::Collapsed;
        }
    }

    /// Appends a delta to the answer, creating it on the first one.
    fn push_delta(&mut self, text: &str, now: Instant) -> Follow {
        if text.is_empty() {
            return Follow::Nothing;
        }
        if self.phase != Phase::Streaming {
            self.collapse_reasoning(now);
            self.entries.push(Entry::Message(Msg {
                role: Role::Assistant,
                text: String::new(),
                status: MsgStatus::Complete,
            }));
            self.phase = Phase::Streaming;
        }
        let Some(answer) = self.answer_mut() else {
            return Follow::Nothing;
        };
        let room = ANSWER_CAP - answer.text.len();
        if text.len() <= room {
            answer.text.push_str(text);
            return Follow::Nothing;
        }
        let mut cut = room;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        answer.text.push_str(&text[..cut]);
        self.finish(MsgStatus::Truncated, Some(TOO_LONG.to_owned()), now);
        // Anything the engine still sends belongs to a turn nobody awaits.
        self.turn += 1;
        Follow::CancelEngine
    }

    /// The assistant message being written: the last message entry. System
    /// lines can follow it (a refused send adds BUSY), and no user message is
    /// appended while a turn is in flight.
    fn answer_mut(&mut self) -> Option<&mut Msg> {
        if self.phase != Phase::Streaming {
            return None;
        }
        self.entries.iter_mut().rev().find_map(|e| match e {
            Entry::Message(m) => Some(m),
            Entry::System(_) | Entry::Reasoning(_) | Entry::Memory(_) => None,
        })
    }

    /// Ends the turn: the partial answer, if any, gets `status`, and an
    /// optional system line is added.
    fn finish(&mut self, status: MsgStatus, line: Option<String>, now: Instant) {
        self.collapse_reasoning(now);
        // Only the answer text is saved: reasoning is not a message, and a
        // turn that never answered leaves no assistant message to save.
        let answer = self.answer_mut().map(|answer| {
            answer.status = status;
            answer.text.clone()
        });
        if let Some(text) = answer {
            self.outbox.push(store::StoredMessage {
                role: Role::Assistant,
                text,
                status,
                engine: std::mem::take(&mut self.request_engine),
                model: (!self.request_model.is_empty()).then(|| self.request_model.clone()),
            });
        }
        if let Some(line) = line {
            self.entries.push(Entry::System(line));
        }
        self.phase = Phase::Idle;
        self.started = None;
        self.request_model.clear();
        self.request_engine.clear();
    }

    /// Stops the turn in flight, keeping the partial answer.
    pub fn cancel(&mut self) -> Follow {
        self.cancel_at(Instant::now())
    }

    /// `cancel` with the clock injected.
    pub fn cancel_at(&mut self, now: Instant) -> Follow {
        if self.phase == Phase::Idle {
            return Follow::Nothing;
        }
        if self.summarizing.is_some() {
            return self.cancel_summary();
        }
        self.finish(MsgStatus::Cancelled, None, now);
        self.turn += 1;
        self.generation += 1;
        Follow::CancelEngine
    }
}

#[cfg(test)]
mod anchor_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod memory_line_tests;
#[cfg(test)]
mod memory_tests;
#[cfg(test)]
mod note_tests;
#[cfg(test)]
mod reasoning_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod tag_editor_tests;
#[cfg(test)]
mod wait_tests;
