//! What the chat pane and the store thread say to each other. Pure: the thread
//! that owns the database lives in `runtime::chat_store`.
//!
//! The pane sends `StoreCmd`s and gets `StoreReply`s back, in order. A pane
//! does not know its conversation's database id until `Created` arrives, so
//! its appends before then name the pane only and the store thread resolves
//! the conversation it created for it.

use std::path::PathBuf;

use super::{Msg, MsgStatus, engine::Role, tags::Tags};
use crate::core::{ai::sanitize::clean, layout::PaneId};

/// The most conversations the store keeps.
pub const RETENTION_LIMIT: usize = 200;
/// System line for a new conversation that made the store evict the oldest.
pub const EVICTED_LINE: &str = "oldest conversation removed (limit 200)";
/// System line for a write that failed in the middle of a session.
pub const SAVE_FAILED_LINE: &str = "could not save conversation";
/// System line for a conversation another pane deleted while this one has it
/// open.
pub const DELETED_LINE: &str = "conversation deleted; new messages are not saved";
/// System line when tags are edited on a conversation that is not saving.
pub const TAGS_NOT_SAVED_LINE: &str = "tags kept in this pane only: history is not saving";
/// Start of the system line shown when the store cannot be used.
pub const UNAVAILABLE_PREFIX: &str = "history unavailable: ";
/// The longest error text shown to the user, in characters.
pub const ERROR_MAX: usize = 120;

/// The most characters of a conversation's first message the list carries.
pub const FIRST_MAX: usize = 200;

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
        /// The extra tags the pane set before its first message.
        extras: Vec<String>,
        first: StoredMessage,
        /// The cwd the chat was opened from: when `tag` is not known yet (the
        /// list has not answered), the store thread resolves it from here.
        origin: Option<PathBuf>,
        open: Vec<i64>,
    },
    /// Adds a message. `conversation` is `None` while the pane does not know
    /// its id yet: the store thread, which created the conversation for the
    /// pane, resolves it. Commands run in order, so the `Create` is done.
    Append {
        pane: PaneId,
        conversation: Option<i64>,
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
    /// Replaces the project tag and the extra tags of a conversation, in one
    /// transaction that also bumps its last activity. `conversation` is
    /// `None` while the pane does not know its id yet (as for `Append`).
    /// Answered with `TagsSaved`, or `TagsFailed`.
    SetTags {
        pane: PaneId,
        conversation: Option<i64>,
        tags: Tags,
    },
    /// The conversations of the store, newest last activity first, and the
    /// project tag of `origin` (resolved on the store thread: it walks the
    /// filesystem). Answered with `Listed`, `ListFailed`, or `Unavailable`.
    List {
        pane: PaneId,
        origin: Option<PathBuf>,
    },
    /// Reads a whole conversation. Answered with `Resumed`, or `ResumeFailed`.
    Resume { pane: PaneId, conversation: i64 },
    /// Removes a conversation with everything that hangs from it. Answered
    /// with `Deleted` (carrying the note rows the delete took with it), or
    /// `DeleteFailed`.
    Delete { pane: PaneId, conversation: i64 },
    /// Finish what is queued and stop.
    Quit,
}

/// One row of the conversation list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConvRow {
    pub id: i64,
    /// Last activity, in seconds since the Unix epoch.
    pub updated_at: i64,
    pub profile: String,
    /// The project tag, when the conversation has one.
    pub tag: Option<String>,
    /// The extra tags, by name.
    pub extras: Vec<String>,
    /// The first user message, cut to `FIRST_MAX` characters.
    pub first: String,
}

/// A note row a delete removed: the note itself stays in engram. What a later
/// "also delete its notes" needs to find it there.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NoteRow {
    pub project: String,
    pub observation_id: i64,
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
    Unavailable {
        pane: PaneId,
        reason: String,
    },
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
    LinkFailed {
        pane: PaneId,
        kind: LinkKind,
    },
    /// A write failed. `notify` is true for the first failure of a run, the
    /// only one that earns a system line.
    Failed {
        pane: PaneId,
        notify: bool,
    },
    /// The answer to `List`: the project tag of the origin (when it has one)
    /// and the conversations, newest last activity first.
    Listed {
        pane: PaneId,
        tag: Option<String>,
        rows: Vec<ConvRow>,
    },
    /// The conversations could not be read. The store itself is not declared
    /// unusable: the pane keeps saving.
    ListFailed {
        pane: PaneId,
        tag: Option<String>,
    },
    /// The answer to `Resume`: the profile and project tag the conversation
    /// was stored with and its messages in order (no reasoning, no system
    /// lines: none is ever stored).
    Resumed {
        pane: PaneId,
        conversation: i64,
        profile: String,
        tag: Option<String>,
        extras: Vec<String>,
        messages: Vec<Msg>,
    },
    ResumeFailed {
        pane: PaneId,
    },
    /// The conversation is gone. `notes` are the note rows it had, read in the
    /// same transaction that removed it.
    Deleted {
        pane: PaneId,
        conversation: i64,
        notes: Vec<NoteRow>,
    },
    DeleteFailed {
        pane: PaneId,
    },
    /// The tags of `conversation` are stored, as the store keeps them
    /// (cleaned, without repeats, at most 8 extras).
    TagsSaved {
        pane: PaneId,
        conversation: i64,
        tags: Tags,
        /// The last activity the edit set, in seconds since the Unix epoch.
        updated_at: i64,
    },
    TagsFailed {
        pane: PaneId,
    },
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
    /// `Create` is in flight; later saves append by pane.
    Creating,
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
    pub extras: Vec<String>,
    pub origin: Option<PathBuf>,
    /// Conversations open in any pane, which retention must skip.
    pub open: Vec<i64>,
}

impl PaneStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store for a conversation that already exists (a resume): saves
    /// append to it.
    pub fn open(conversation: i64) -> Self {
        Self {
            link: Link::Open(conversation),
        }
    }

    /// Stops saving: the conversation is gone or the store is unusable.
    pub fn detach(&mut self) {
        self.link = Link::Detached;
    }

    pub fn is_detached(&self) -> bool {
        self.link == Link::Detached
    }

    /// Nothing was saved yet, so no write can be in flight.
    pub fn is_fresh(&self) -> bool {
        self.link == Link::Fresh
    }

    /// The database id once the conversation exists.
    pub fn conversation(&self) -> Option<i64> {
        match self.link {
            Link::Open(id) => Some(id),
            _ => None,
        }
    }

    /// Saves `message`: the command to send now, if any. An answer that failed
    /// after partial text is saved with status `error`; the error line is a
    /// system line and never reaches a save.
    pub fn save(
        &mut self,
        pane: PaneId,
        message: StoredMessage,
        new: &NewConversation,
    ) -> Option<StoreCmd> {
        match self.link {
            Link::Fresh => {
                self.link = Link::Creating;
                Some(StoreCmd::Create {
                    pane,
                    profile: new.profile.clone(),
                    tag: new.tag.clone(),
                    extras: new.extras.clone(),
                    first: message,
                    origin: new.origin.clone(),
                    open: new.open.clone(),
                })
            }
            Link::Creating => Some(StoreCmd::Append {
                pane,
                conversation: None,
                message,
            }),
            Link::Open(conversation) => Some(StoreCmd::Append {
                pane,
                conversation: Some(conversation),
                message,
            }),
            Link::Detached => None,
        }
    }

    /// The command that stores `tags` now, if the conversation can take them.
    /// A conversation not created yet keeps them for its `Create` (they are in
    /// `NewConversation`); one that is being created is named by its pane; a
    /// detached one stores nothing.
    pub fn set_tags(&self, pane: PaneId, tags: Tags) -> Option<StoreCmd> {
        let conversation = match self.link {
            Link::Fresh | Link::Detached => return None,
            Link::Creating => None,
            Link::Open(id) => Some(id),
        };
        Some(StoreCmd::SetTags {
            pane,
            conversation,
            tags,
        })
    }

    /// Applies a reply for this pane.
    pub fn on_reply(&mut self, pane: PaneId, reply: &StoreReply) {
        match reply {
            StoreReply::Created {
                pane: from,
                conversation,
                ..
            } if *from == pane => self.link = Link::Open(*conversation),
            StoreReply::Unavailable { pane: from, .. } if *from == pane => {
                self.link = Link::Detached;
            }
            // Only a conversation that is still being created is lost; one
            // that exists keeps trying on its next append.
            StoreReply::Failed { pane: from, .. }
                if *from == pane && self.link == Link::Creating =>
            {
                self.link = Link::Detached;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
