//! The port between the app and the local memory thread: plain values, like
//! the engram `MemCmd`. Every outcome is typed and has its visible line.

use std::path::PathBuf;

use super::{
    gitignore::GitignoreOutcome,
    ledger::LedgerError,
    note::{CUT_LINE, LocalNote, NewNote},
};
use crate::core::layout::PaneId;

/// The most note files read in one load; the rest are counted and left out.
pub const MAX_NOTES: usize = 5000;

/// The line for a folder outside any git repository.
pub const NO_GIT_LINE: &str =
    "memory: no git repository here; nothing is remembered for this folder";

/// Where a chat's memory is: the store root (the main checkout for a linked
/// worktree) and the area of the chat folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Root {
    pub store: PathBuf,
    pub area: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LocalCmd {
    /// Read the notes of the project of `cwd`, once, at project entry. `epoch`
    /// is the memory epoch of the chat, echoed back.
    Load {
        pane: PaneId,
        epoch: u32,
        cwd: String,
    },
    /// Write one note. The `.lzr/` folder is created here, never at load.
    Save {
        pane: PaneId,
        root: Root,
        note: NewNote,
    },
}

/// Why a note was not written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Failure {
    /// The folder (shown) cannot be written.
    ReadOnly(String),
    Io(String),
    /// The memory thread could not start or has stopped: nothing is read or
    /// saved.
    Unavailable(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LocalReply {
    Loaded {
        pane: PaneId,
        epoch: u32,
        /// `None` outside a git repository: nothing is remembered.
        root: Option<Root>,
        /// Notes this machine made and nobody changed.
        notes: Vec<LocalNote>,
        /// Notes made elsewhere or changed outside lazarobox: never used.
        foreign: usize,
        unreadable: usize,
        newer: usize,
        /// Note files past [`MAX_NOTES`]: not read.
        capped: usize,
        /// The private ledger could not be used: nothing is trusted.
        ledger: Option<LedgerError>,
    },
    Saved {
        pane: PaneId,
        id: String,
        cut: bool,
        /// Set only when this save created `.lzr/`.
        gitignore: Option<GitignoreOutcome>,
        /// Something the save did on its own that the user should know.
        notice: Option<String>,
    },
    Failed {
        pane: PaneId,
        failure: Failure,
    },
}

impl LocalCmd {
    pub fn pane(&self) -> PaneId {
        match self {
            Self::Load { pane, .. } | Self::Save { pane, .. } => *pane,
        }
    }
}

impl LocalReply {
    pub fn pane(&self) -> PaneId {
        match self {
            Self::Loaded { pane, .. } | Self::Saved { pane, .. } | Self::Failed { pane, .. } => {
                *pane
            }
        }
    }

    /// The visible lines of this outcome, in order. Empty when there is
    /// nothing to say.
    pub fn lines(&self) -> Vec<String> {
        let notes = |n: usize| format!("{n} note{}", if n == 1 { "" } else { "s" });
        match self {
            Self::Loaded { root: None, .. } => vec![NO_GIT_LINE.to_owned()],
            Self::Loaded {
                foreign,
                unreadable,
                newer,
                capped,
                ledger,
                ..
            } => [
                (
                    usize::from(ledger.is_some()),
                    match ledger {
                        Some(LedgerError::Newer) => "memory: the private ledger was made by a newer lazarobox; no note is trusted".to_owned(),
                        _ => "memory: the private ledger is damaged; no note is trusted until it is repaired".to_owned(),
                    },
                ),
                (
                    *foreign,
                    format!(
                        "memory: {} from others ignored (not created on this machine)",
                        notes(*foreign)
                    ),
                ),
                (
                    *unreadable,
                    format!(
                        "memory: skipped {} in .lzr/",
                        notes(*unreadable).replace("note", "unreadable note")
                    ),
                ),
                (
                    *newer,
                    format!("{} made by a newer lazarobox were skipped", notes(*newer)),
                ),
                (
                    *capped,
                    format!(
                        "memory: only the first {MAX_NOTES} notes in .lzr/ were read; {capped} more were left out"
                    ),
                ),
            ]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(_, line)| line)
            .collect(),
            Self::Saved {
                cut,
                gitignore,
                notice,
                ..
            } => cut
                .then(|| CUT_LINE.to_owned())
                .into_iter()
                .chain(gitignore.iter().filter_map(GitignoreOutcome::line))
                .chain(notice.clone())
                .collect(),
            Self::Failed {
                failure: Failure::ReadOnly(path),
                ..
            } => {
                vec![format!("memory not saved: {path} is read-only")]
            }
            Self::Failed {
                failure: Failure::Unavailable(reason),
                ..
            } => vec![format!(
                "memory: not available ({reason}); notes are not read or saved"
            )],
            Self::Failed {
                failure: Failure::Io(reason),
                ..
            } => vec![format!(
                "memory: cannot write .lzr/ ({reason}); notes from this chat are not saved"
            )],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(root: bool, foreign: usize, unreadable: usize, newer: usize) -> LocalReply {
        LocalReply::Loaded {
            pane: PaneId::FIRST,
            epoch: 0,
            root: root.then(|| Root {
                store: "/r".into(),
                area: ".".into(),
            }),
            notes: Vec::new(),
            foreign,
            unreadable,
            newer,
            capped: 0,
            ledger: None,
        }
    }

    #[test]
    fn a_damaged_or_newer_ledger_is_said_before_the_notes_it_makes_foreign() {
        let with = |ledger| match loaded(true, 3, 0, 0) {
            LocalReply::Loaded {
                pane,
                epoch,
                root,
                notes,
                foreign,
                unreadable,
                newer,
                capped,
                ..
            } => LocalReply::Loaded {
                pane,
                epoch,
                root,
                notes,
                foreign,
                unreadable,
                newer,
                capped,
                ledger: Some(ledger),
            },
            other => other,
        };
        assert_eq!(
            with(LedgerError::Damaged).lines()[0],
            "memory: the private ledger is damaged; no note is trusted until it is repaired"
        );
        assert_eq!(
            with(LedgerError::Newer).lines()[0],
            "memory: the private ledger was made by a newer lazarobox; no note is trusted"
        );
        assert_eq!(with(LedgerError::Newer).lines().len(), 2);
    }

    #[test]
    fn a_clean_load_says_nothing() {
        assert!(loaded(true, 0, 0, 0).lines().is_empty());
    }

    #[test]
    fn no_git_repository_says_so() {
        assert_eq!(
            loaded(false, 0, 0, 0).lines(),
            ["memory: no git repository here; nothing is remembered for this folder"]
        );
    }

    #[test]
    fn foreign_unreadable_and_newer_notes_each_get_a_line() {
        assert_eq!(
            loaded(true, 5, 2, 3).lines(),
            [
                "memory: 5 notes from others ignored (not created on this machine)",
                "memory: skipped 2 unreadable notes in .lzr/",
                "3 notes made by a newer lazarobox were skipped",
            ]
        );
        assert_eq!(
            loaded(true, 1, 1, 0).lines(),
            [
                "memory: 1 note from others ignored (not created on this machine)",
                "memory: skipped 1 unreadable note in .lzr/",
            ]
        );
    }

    #[test]
    fn a_save_shows_the_cut_and_the_gitignore_outcome() {
        let saved = LocalReply::Saved {
            pane: PaneId::FIRST,
            id: "n-1".into(),
            cut: true,
            gitignore: Some(GitignoreOutcome::Added),
            notice: None,
        };
        assert_eq!(
            saved.lines(),
            [
                CUT_LINE.to_owned(),
                "added .lzr/ to .gitignore (remove the entry to share memory through git)"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn an_unavailable_memory_says_so() {
        let reply = LocalReply::Failed {
            pane: PaneId::FIRST,
            failure: Failure::Unavailable("out of threads".into()),
        };
        assert_eq!(
            reply.lines(),
            ["memory: not available (out of threads); notes are not read or saved"]
        );
    }

    #[test]
    fn a_failed_save_says_why() {
        let failed = |failure| LocalReply::Failed {
            pane: PaneId::FIRST,
            failure,
        };
        assert_eq!(
            failed(Failure::ReadOnly("/r".into())).lines(),
            ["memory not saved: /r is read-only"]
        );
        assert_eq!(
            failed(Failure::Io("disk full".into())).lines(),
            ["memory: cannot write .lzr/ (disk full); notes from this chat are not saved"]
        );
    }
}
