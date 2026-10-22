//! The pure core of the orchestrator: the run and worker state machines, the
//! orchestration tool calls and the read scope. No IO beyond the path checks
//! of `scope`; time comes in as explicit `Instant`s.

pub mod run;
pub mod scope;
pub mod tools;
pub mod worker;

use super::config::Task;
use super::tools::registry::{Family, FamilySet};

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

/// The tools a session is offered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toolset {
    /// `find_dir`, `list_dir` and `read_file`: what every worker gets.
    ReadOnly,
    /// Reserved for the write-tools SDD (S7): not offered yet.
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
            // `Code` adds nothing until the write tools exist (S7).
            Self::ReadOnly | Self::Code => read,
            Self::Orchestrator => read.with(Family::Orchestra),
        }
    }

    /// The toolset of a worker kind. Read-only for every kind until write
    /// tools exist.
    pub fn for_kind(_task: Task) -> Self {
        Self::ReadOnly
    }
}

#[cfg(test)]
mod tests;
