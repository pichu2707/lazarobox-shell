//! The AI chat core. Pure: no IO lives here.
//!
//! A `ChatState` is the conversation of one chat pane: its transcript, the
//! turn in flight and the cwd the pane was opened from.

pub mod engine;

use std::path::{Path, PathBuf};

use engine::{
    Availability, ChatError, ChatMessage, ChatRequest, Role, StreamEvent, build_context,
    history_chars,
};

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

/// The state of one chat pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatState {
    origin_cwd: Option<PathBuf>,
    entries: Vec<Entry>,
    phase: Phase,
    turn: u64,
}

impl ChatState {
    /// A chat opened from a pane whose cwd was `origin_cwd`.
    pub fn new(origin_cwd: Option<PathBuf>) -> Self {
        Self {
            origin_cwd,
            entries: Vec::new(),
            phase: Phase::Idle,
            turn: 0,
        }
    }

    /// The cwd of the pane the chat was opened from.
    pub fn origin_cwd(&self) -> Option<&Path> {
        self.origin_cwd.as_deref()
    }

    /// The transcript, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The messages of the transcript, without system lines.
    pub fn messages(&self) -> impl Iterator<Item = &Msg> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Message(m) => Some(m),
            Entry::System(_) => None,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The id events of the turn in flight must carry.
    pub fn turn(&self) -> u64 {
        self.turn
    }

    /// The waiting text, while no byte of the answer has arrived.
    pub fn waiting_text(&self) -> Option<&'static str> {
        match self.phase {
            Phase::Waiting { slow: false } => Some(WAITING),
            Phase::Waiting { slow: true } => Some(LOADING),
            Phase::Idle | Phase::Streaming => None,
        }
    }

    /// Sends `text` to the engine `availability` names, with `system` as the
    /// pinned system prompt.
    pub fn send(&mut self, text: &str, availability: &Availability, system: &str) -> SendOutcome {
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
        self.entries.push(Entry::Message(Msg {
            role: Role::User,
            text: text.trim_end().to_owned(),
            status: MsgStatus::Complete,
        }));
        // A user message nobody answered (cancelled while waiting, or an
        // error before any text) stays in the transcript but is not sent:
        // the engine never gets two user messages in a row.
        let all: Vec<&Msg> = self.messages().collect();
        let sendable: Vec<usize> = (0..all.len())
            .filter(|&i| {
                all[i].role != Role::User
                    || all
                        .get(i + 1)
                        .is_none_or(|next| next.role == Role::Assistant)
            })
            .collect();
        let history: Vec<ChatMessage> = sendable
            .iter()
            .map(|&i| ChatMessage {
                role: all[i].role,
                content: all[i].text.clone(),
            })
            .collect();
        let context = build_context(&history, history_chars(engine));
        let oldest = sendable[history.len() - context.messages.len()];
        self.place_marker(oldest, context.omitted);
        self.turn += 1;
        self.phase = Phase::Waiting { slow: false };
        SendOutcome::Sent {
            turn: self.turn,
            request: ChatRequest {
                engine: engine.clone(),
                model: model.clone(),
                system: system.to_owned(),
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
        if turn != self.turn || self.phase == Phase::Idle {
            return Follow::Nothing;
        }
        match event {
            StreamEvent::Slow => {
                if let Phase::Waiting { slow } = &mut self.phase {
                    *slow = true;
                }
                Follow::Nothing
            }
            StreamEvent::Delta(text) => self.push_delta(&text),
            StreamEvent::Done(_) => {
                let line = (self.phase != Phase::Streaming).then(|| NO_ANSWER.to_owned());
                self.finish(MsgStatus::Complete, line);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Interrupted) => {
                self.finish(MsgStatus::Interrupted, Some(INTERRUPTED.to_owned()));
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Truncated) => {
                self.finish(MsgStatus::Truncated, Some(TOO_LONG.to_owned()));
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Failed(message)) => {
                self.finish(MsgStatus::Error, Some(message));
                Follow::Nothing
            }
        }
    }

    /// Appends a delta to the answer, creating it on the first one.
    fn push_delta(&mut self, text: &str) -> Follow {
        if text.is_empty() {
            return Follow::Nothing;
        }
        if self.phase != Phase::Streaming {
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
        self.finish(MsgStatus::Truncated, Some(TOO_LONG.to_owned()));
        // Anything the engine still sends belongs to a turn nobody awaits.
        self.turn += 1;
        Follow::CancelEngine
    }

    /// The assistant message being written.
    fn answer_mut(&mut self) -> Option<&mut Msg> {
        match self.entries.last_mut() {
            Some(Entry::Message(m)) if self.phase == Phase::Streaming => Some(m),
            _ => None,
        }
    }

    /// Ends the turn: the partial answer, if any, gets `status`, and an
    /// optional system line is added.
    fn finish(&mut self, status: MsgStatus, line: Option<String>) {
        if let Some(answer) = self.answer_mut() {
            answer.status = status;
        }
        if let Some(line) = line {
            self.entries.push(Entry::System(line));
        }
        self.phase = Phase::Idle;
    }

    /// Stops the turn in flight, keeping the partial answer.
    pub fn cancel(&mut self) -> Follow {
        if self.phase == Phase::Idle {
            return Follow::Nothing;
        }
        self.finish(MsgStatus::Cancelled, None);
        self.turn += 1;
        Follow::CancelEngine
    }
}

#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod stream_tests;
