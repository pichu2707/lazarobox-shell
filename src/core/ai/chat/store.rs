//! What the chat pane and the store thread say to each other. Pure: the thread
//! that owns the database lives in `runtime::chat_store`.
//!
//! The pane sends `StoreCmd`s and gets `StoreReply`s back, in order. A pane
//! does not know its conversation's database id until `Created` arrives, so
//! `PaneStore` holds the appends that are produced before then.

use super::{MsgStatus, engine::Role};
use crate::core::{ai::sanitize::clean, layout::PaneId};

/// The most conversations the store keeps.
pub const RETENTION_LIMIT: usize = 200;
/// System line for a new conversation that made the store evict the oldest.
pub const EVICTED_LINE: &str = "oldest conversation removed (limit 200)";
/// System line for a write that failed in the middle of a session.
pub const SAVE_FAILED_LINE: &str = "could not save conversation";
/// Start of the system line shown when the store cannot be used.
pub const UNAVAILABLE_PREFIX: &str = "history unavailable: ";
/// The longest error text shown to the user, in characters.
pub const ERROR_MAX: usize = 120;

/// One message to save.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StoredMessage {
    pub role: Role,
    pub text: String,
    pub status: MsgStatus,
    pub engine: String,
    pub model: Option<String>,
}

/// A request to the store thread.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StoreCmd {
    /// Create a conversation holding `first`. Conversations in `open` are
    /// never evicted by retention.
    Create {
        pane: PaneId,
        profile: String,
        tag: Option<String>,
        first: StoredMessage,
        open: Vec<i64>,
    },
    Append {
        pane: PaneId,
        conversation: i64,
        message: StoredMessage,
    },
    /// Reads or sets the engram session of a conversation: the stored id is
    /// answered with its project when there is one (a resume), else
    /// `session_id` and `project` are stored.
    /// Answered with `SessionLinked`, or `LinkFailed`.
    LinkSession {
        pane: PaneId,
        conversation: i64,
        session_id: String,
        project: String,
    },
    /// Records that `conversation` wrote the note `(project, topic_key)`. The
    /// row is upserted, so the last writer owns it. Only ids and the title are
    /// kept, never the note's content. Silent on success.
    LinkNote {
        pane: PaneId,
        conversation: i64,
        project: String,
        topic_key: String,
        observation_id: i64,
        title: String,
    },
    /// Finish what is queued and stop.
    Quit,
}

/// Which link a [`StoreReply::LinkFailed`] is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    Session,
    Note,
}

/// What the store thread answers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StoreReply {
    Created {
        pane: PaneId,
        conversation: i64,
        /// Retention removed at least one conversation to make room.
        evicted: bool,
        /// Note rows of the conversations retention removed. Their notes stay
        /// in engram; only the link is gone.
        evicted_notes: u32,
    },
    /// The store cannot be used at all; `reason` is already bounded.
    Unavailable { pane: PaneId, reason: String },
    /// The session of a conversation: the stored id and the project it was
    /// registered with, which are the ones just proposed unless the
    /// conversation already had a pair. The caller reuses both, because engram
    /// rejects a session under another project (400 `session_project_mismatch`).
    SessionLinked {
        pane: PaneId,
        conversation: i64,
        session_id: String,
        project: String,
    },
    /// A link could not be written. Nothing else was undone; the caller says
    /// so once if it wants to.
    LinkFailed { pane: PaneId, kind: LinkKind },
    /// A write failed. `notify` is true for the first failure of a run, the
    /// only one that earns a system line.
    Failed { pane: PaneId, notify: bool },
}

/// The system lines a new conversation shows when retention removed some: the
/// eviction line, then, when notes stayed behind in engram, how many. Nothing
/// when nothing was evicted. Shown whether or not memory is on; it needs no
/// engram request and asks nothing.
pub fn eviction_lines(evicted: bool, notes: u32) -> Vec<String> {
    if !evicted {
        return Vec::new();
    }
    let mut lines = vec![EVICTED_LINE.to_owned()];
    match notes {
        0 => {}
        1 => lines.push("1 note of a removed conversation remains in engram".to_owned()),
        n => lines.push(format!(
            "{n} notes of removed conversations remain in engram"
        )),
    }
    lines
}

/// [`eviction_lines`] for a store reply; empty for any reply but `Created`.
pub fn eviction_lines_of(reply: &StoreReply) -> Vec<String> {
    match reply {
        StoreReply::Created {
            evicted,
            evicted_notes,
            ..
        } => eviction_lines(*evicted, *evicted_notes),
        _ => Vec::new(),
    }
}

/// An error text safe to show: sanitized, on one line, at most `ERROR_MAX`
/// characters.
pub fn bounded_error(raw: &str) -> String {
    let flat = clean(raw).split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = flat.chars().take(ERROR_MAX).collect();
    capped.trim_end().to_owned()
}

/// Where a pane's conversation is in the store.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
enum Link {
    /// Nothing sent yet: the next save creates the conversation.
    #[default]
    Fresh,
    /// `Create` is in flight; later saves wait here.
    Creating(Vec<StoredMessage>),
    Open(i64),
    /// The store is unusable for this conversation: saves are dropped.
    Detached,
}

/// The pane's side of its conversation's storage.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PaneStore {
    link: Link,
}

/// How a new conversation is created.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NewConversation {
    pub profile: String,
    pub tag: Option<String>,
    /// Conversations open in any pane, which retention must skip.
    pub open: Vec<i64>,
}

impl PaneStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The database id once the conversation exists.
    pub fn conversation(&self) -> Option<i64> {
        match self.link {
            Link::Open(id) => Some(id),
            _ => None,
        }
    }

    /// Saves `message`: the command to send now, if any. Errors are never
    /// saved.
    pub fn save(
        &mut self,
        pane: PaneId,
        message: StoredMessage,
        new: &NewConversation,
    ) -> Option<StoreCmd> {
        if message.status == MsgStatus::Error {
            return None;
        }
        match &mut self.link {
            Link::Fresh => {
                self.link = Link::Creating(Vec::new());
                Some(StoreCmd::Create {
                    pane,
                    profile: new.profile.clone(),
                    tag: new.tag.clone(),
                    first: message,
                    open: new.open.clone(),
                })
            }
            Link::Creating(queue) => {
                queue.push(message);
                None
            }
            Link::Open(conversation) => Some(StoreCmd::Append {
                pane,
                conversation: *conversation,
                message,
            }),
            Link::Detached => None,
        }
    }

    /// Applies a reply for this pane and returns the commands it releases.
    pub fn on_reply(&mut self, pane: PaneId, reply: &StoreReply) -> Vec<StoreCmd> {
        match reply {
            StoreReply::Created {
                pane: from,
                conversation,
                ..
            } if *from == pane => {
                match std::mem::replace(&mut self.link, Link::Open(*conversation)) {
                    Link::Creating(queue) => queue
                        .into_iter()
                        .map(|message| StoreCmd::Append {
                            pane,
                            conversation: *conversation,
                            message,
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            }
            StoreReply::Unavailable { pane: from, .. } | StoreReply::Failed { pane: from, .. }
                if *from == pane =>
            {
                // Only a conversation that is still being created is lost;
                // one that exists keeps trying on its next append.
                if matches!(self.link, Link::Creating(_))
                    || matches!(reply, StoreReply::Unavailable { .. })
                {
                    self.link = Link::Detached;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
