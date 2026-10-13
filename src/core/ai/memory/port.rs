//! The memory port: the commands the app sends to the engram worker and the
//! replies that come back. Plain values, no IO.

use super::{
    context::ContextBlock, health::Health, kind::MemKind, offer::Offer, reply::ProjectError,
};
use crate::core::layout::PaneId;

/// A note ready to be saved. `title` and `content` are NOT yet redacted: the
/// call builder redacts them (and cleans the title and caps the content), so
/// no other path can send raw text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Note {
    pub kind: MemKind,
    pub title: String,
    pub content: String,
}

/// What the worker is asked to do. Plain values: the app emits them, the
/// runtime executes them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MemCmd {
    /// Resolve the engram project of a pane's origin directory.
    Resolve { pane: PaneId, cwd: String },
    /// Fetch the context block of a project.
    Context { pane: PaneId, project: String },
    /// Save a note. `session` is `None` until the conversation has one. `cwd`
    /// is the directory of the chat: the session registers with it.
    Save {
        pane: PaneId,
        session: Option<String>,
        project: String,
        cwd: String,
        note: Note,
    },
    /// End a registered session with its summary.
    End {
        session: String,
        project: String,
        summary: String,
    },
    /// Delete notes, each `(project, observation id)`, with the user's token.
    Delete {
        pane: PaneId,
        notes: Vec<(String, i64)>,
    },
    /// Start `engram serve` after the user confirmed it.
    StartServer,
    /// Flush pending work and stop.
    Quit,
}

impl MemCmd {
    /// The pane a command answers to; global commands have none.
    pub fn pane(&self) -> Option<PaneId> {
        match self {
            MemCmd::Resolve { pane, .. }
            | MemCmd::Context { pane, .. }
            | MemCmd::Save { pane, .. }
            | MemCmd::Delete { pane, .. } => Some(*pane),
            MemCmd::End { .. } | MemCmd::StartServer | MemCmd::Quit => None,
        }
    }
}

/// Why memory cannot be used right now.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reason {
    /// Nothing answers on the port.
    NotRunning,
    /// An engram 1.x answers on the port.
    OldServer { port: u16 },
    /// Something else answers on the port.
    Foreign { port: u16 },
    /// A call timed out or broke.
    NoAnswer,
    /// No `engram` on the PATH.
    NoBinary,
    /// The `engram` on the PATH is 1.x.
    OldBinary,
    /// The command queue is full.
    Busy,
    /// The directory is not a git project.
    NotGit,
    /// `engram serve` was started and nothing answered; holds the last line
    /// of its log, already sanitized and capped.
    StartFailed(Option<String>),
}

impl Reason {
    /// The reason text, as in the `memory unavailable: <reason>` line and in
    /// `Memory is not ready: <reason>`.
    pub fn text(&self) -> String {
        match self {
            Reason::NotRunning => "engram is not running".into(),
            Reason::OldServer { port } => {
                format!("old engram server on :{port}, restart it with 3.x")
            }
            Reason::Foreign { port } => format!("port {port} is not engram"),
            Reason::NoAnswer => "engram did not answer".into(),
            Reason::NoBinary => "engram not found on PATH".into(),
            Reason::OldBinary => "engram on PATH is 1.x; upgrade it".into(),
            Reason::Busy => "engram is busy".into(),
            Reason::NotGit => "memory needs a git project".into(),
            Reason::StartFailed(None) => "could not start engram".into(),
            Reason::StartFailed(Some(line)) => format!("could not start engram: {line}"),
        }
    }

    /// The system line shown once in the transcript. A non-git project has
    /// its own wording, without the `memory unavailable:` prefix.
    pub fn line(&self) -> String {
        match self {
            Reason::NotGit => self.text(),
            other => format!("memory unavailable: {}", other.text()),
        }
    }
}

/// How an `engram serve` start ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ServerOutcome {
    /// A server was spawned and answered.
    Started { pid: u32 },
    /// A ready server was already there; nothing was spawned.
    Adopted,
    /// The start was not possible or did not work.
    Failed(Reason),
}

impl ServerOutcome {
    /// The line shown when a server was started. Adopting a server that was
    /// already there says nothing, and a failure has its own reason line.
    pub fn notice(&self) -> Option<String> {
        match self {
            ServerOutcome::Started { pid } => Some(format!(
                "started engram serve (pid {pid}); it keeps running after lazarobox quits"
            )),
            ServerOutcome::Adopted | ServerOutcome::Failed(_) => None,
        }
    }
}

/// What the worker answers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MemReply {
    /// The health state changed; `offer` is true when nothing runs on the port
    /// (the real offer, with the binary's path, follows as `ServerOffer`).
    Health { state: Health, offer: bool },
    /// The project of a pane, or why there is none.
    Project {
        pane: PaneId,
        result: Result<String, ProjectError>,
    },
    /// The context block of a pane, `None` when there is no memory yet.
    Context {
        pane: PaneId,
        block: Option<ContextBlock>,
    },
    /// A note was saved (or updated: same id).
    Saved {
        pane: PaneId,
        session: String,
        observation_id: i64,
        topic_key: String,
        title: String,
    },
    /// A save failed; `reason` is short and sanitized.
    SaveFailed { pane: PaneId, reason: String },
    /// Notes were deleted one by one.
    Deleted {
        pane: PaneId,
        ok: usize,
        failed: usize,
        refused: bool,
    },
    /// The result of `StartServer`.
    Server(ServerOutcome),
    /// Nothing runs on the port and a 3.x `engram` can start one: the app
    /// asks the user (see `offer::ServerOffer`). At most once per run.
    ServerOffer(Offer),
    /// Memory cannot be used; `pane` is `None` for a global state.
    Unavailable {
        pane: Option<PaneId>,
        reason: Reason,
    },
}

impl MemReply {
    /// The pane a reply is for; global replies have none.
    pub fn pane(&self) -> Option<PaneId> {
        match self {
            MemReply::Project { pane, .. }
            | MemReply::Context { pane, .. }
            | MemReply::Saved { pane, .. }
            | MemReply::SaveFailed { pane, .. }
            | MemReply::Deleted { pane, .. } => Some(*pane),
            MemReply::Unavailable { pane, .. } => *pane,
            MemReply::Health { .. } | MemReply::Server(_) | MemReply::ServerOffer(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_replies_are_plain_values() {
        let pane = PaneId::FIRST;
        let note = Note {
            kind: MemKind::Bugfix,
            title: "t".into(),
            content: "c".into(),
        };
        let cmds = [
            MemCmd::Resolve {
                pane,
                cwd: "/x".into(),
            },
            MemCmd::Context {
                pane,
                project: "p".into(),
            },
            MemCmd::Save {
                pane,
                session: None,
                project: "p".into(),
                cwd: "/x".into(),
                note,
            },
            MemCmd::End {
                session: "s".into(),
                project: "p".into(),
                summary: "x".into(),
            },
            MemCmd::Delete {
                pane,
                notes: vec![("p".into(), 7)],
            },
            MemCmd::StartServer,
            MemCmd::Quit,
        ];
        assert_eq!(cmds.to_vec(), cmds);
        let replies = [
            MemReply::Health {
                state: Health::Down,
                offer: true,
            },
            MemReply::Project {
                pane,
                result: Err(ProjectError::NotGit),
            },
            MemReply::Context { pane, block: None },
            MemReply::Saved {
                pane,
                session: "s".into(),
                observation_id: 1,
                topic_key: "chat/bugfix/t".into(),
                title: "t".into(),
            },
            MemReply::SaveFailed {
                pane,
                reason: "r".into(),
            },
            MemReply::Deleted {
                pane,
                ok: 1,
                failed: 0,
                refused: false,
            },
            MemReply::Server(ServerOutcome::Adopted),
            MemReply::ServerOffer(Offer {
                path: "/usr/bin/engram".into(),
                port: 7437,
            }),
            MemReply::Unavailable {
                pane: None,
                reason: Reason::Busy,
            },
        ];
        assert_eq!(replies.to_vec(), replies);
    }

    #[test]
    fn each_reason_renders_the_exact_degradation_text() {
        let table = [
            (Reason::NotRunning, "engram is not running"),
            (
                Reason::OldServer { port: 7437 },
                "old engram server on :7437, restart it with 3.x",
            ),
            (Reason::Foreign { port: 9000 }, "port 9000 is not engram"),
            (Reason::NoAnswer, "engram did not answer"),
            (Reason::NoBinary, "engram not found on PATH"),
            (Reason::OldBinary, "engram on PATH is 1.x; upgrade it"),
            (Reason::Busy, "engram is busy"),
        ];
        for (reason, text) in table {
            assert_eq!(reason.text(), text);
            assert_eq!(reason.line(), format!("memory unavailable: {text}"));
        }
    }

    #[test]
    fn a_non_git_project_has_its_own_line_without_the_prefix() {
        assert_eq!(Reason::NotGit.text(), "memory needs a git project");
        assert_eq!(Reason::NotGit.line(), "memory needs a git project");
    }

    #[test]
    fn commands_and_replies_name_the_pane_they_belong_to() {
        let pane = PaneId::FIRST;
        let resolve = MemCmd::Resolve {
            pane,
            cwd: "/x".into(),
        };
        assert_eq!(resolve.pane(), Some(pane));
        assert_eq!(MemCmd::Quit.pane(), None);
        assert_eq!(MemCmd::StartServer.pane(), None);
        let end = MemCmd::End {
            session: "s".into(),
            project: "p".into(),
            summary: "x".into(),
        };
        assert_eq!(end.pane(), None);
        let busy = |pane| MemReply::Unavailable {
            pane,
            reason: Reason::Busy,
        };
        assert_eq!(busy(Some(pane)).pane(), Some(pane));
        assert_eq!(busy(None).pane(), None);
        let health = MemReply::Health {
            state: Health::Down,
            offer: false,
        };
        assert_eq!(health.pane(), None);
        assert_eq!(MemReply::Context { pane, block: None }.pane(), Some(pane));
        let offer = Offer {
            path: "/usr/bin/engram".into(),
            port: 7437,
        };
        assert_eq!(MemReply::ServerOffer(offer).pane(), None);
    }

    #[test]
    fn a_failed_start_says_why_without_echoing_more_than_the_log_line() {
        assert_eq!(Reason::StartFailed(None).text(), "could not start engram");
        assert_eq!(
            Reason::StartFailed(Some("bind: address in use".into())).text(),
            "could not start engram: bind: address in use"
        );
        assert_eq!(
            Reason::StartFailed(None).line(),
            "memory unavailable: could not start engram"
        );
    }

    #[test]
    fn only_a_started_server_has_a_notice_and_it_names_the_pid() {
        assert_eq!(
            ServerOutcome::Started { pid: 4242 }.notice().as_deref(),
            Some("started engram serve (pid 4242); it keeps running after lazarobox quits")
        );
        assert_eq!(ServerOutcome::Adopted.notice(), None);
        assert_eq!(ServerOutcome::Failed(Reason::NoBinary).notice(), None);
    }
}
