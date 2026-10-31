//! The pure core of the orchestrator: the run and worker state machines, the
//! orchestration tool calls and the read scope. No IO beyond the path checks
//! of `scope`; time comes in as explicit `Instant`s.

pub mod changes;
pub mod errors;
pub mod event;
pub mod ledger;
pub mod overrides;
pub mod run;
pub mod scope;
pub mod tools;
pub mod worker;

use super::config::Task;
use super::tools::registry::{Family, FamilySet};
use crate::core::layout::PaneId;

/// One orchestrated job.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RunId(pub u64);

/// One worker of a run, numbered from 1.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WorkerId(pub u32);

/// A worker of a run: the key of its tool session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WorkerKey {
    pub run: RunId,
    pub worker: WorkerId,
}

impl WorkerKey {
    #[cfg(test)]
    pub fn for_test(run: u64, worker: u32) -> Self {
        Self {
            run: RunId(run),
            worker: WorkerId(worker),
        }
    }
}

/// The owner of a tool session and of the approvals it waits on: a chat pane,
/// or one worker of a run (which belongs to the pane that started the run).
/// Two sessions never share a wait, a token or an ask.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SessionKey {
    Pane(PaneId),
    Worker(PaneId, WorkerKey),
}

impl SessionKey {
    /// The chat pane the session belongs to.
    pub fn pane(self) -> PaneId {
        match self {
            Self::Pane(pane) | Self::Worker(pane, _) => pane,
        }
    }
}

impl From<PaneId> for SessionKey {
    fn from(pane: PaneId) -> Self {
        Self::Pane(pane)
    }
}

/// The tools a session is offered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toolset {
    /// `find_dir`, `list_dir` and `read_file`: what every worker but a
    /// `code` one gets.
    ReadOnly,
    /// The read tools and the coding-tools write tools (S7): `create_dir`,
    /// `write_file`, `edit_file` and `propose_batch`. Writes are staged in
    /// the pane's batch and reach the disk only through the user's review.
    Code,
    /// The four orchestration tools, for the orchestrator turn only.
    Orchestrator,
}

impl Toolset {
    /// The tool families this toolset is offered and may call. The single
    /// gate for both the list and the parse. No toolset gets `Pane`: only a
    /// chat pane may type into the user's session.
    pub fn families(self) -> FamilySet {
        let read = FamilySet::EMPTY.with(Family::Read);
        match self {
            Self::ReadOnly => read,
            Self::Code => read.with(Family::Write),
            Self::Orchestrator => read.with(Family::Orchestra),
        }
    }

    /// The toolset of a worker kind: `Code` for a `code` worker, read-only
    /// for every other kind.
    pub fn for_kind(task: Task) -> Self {
        match task {
            Task::Code => Self::Code,
            _ => Self::ReadOnly,
        }
    }
}

#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod tests;
