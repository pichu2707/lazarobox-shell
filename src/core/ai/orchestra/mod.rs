//! The pure core of the orchestrator: the run and worker state machines, the
//! orchestration tool calls and the read scope. No IO beyond the path checks
//! of `scope`; time comes in as explicit `Instant`s.

pub mod run;
pub mod scope;
pub mod tools;
pub mod worker;

use super::config::Task;

/// One orchestrated job.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunId(pub u64);

/// One worker of a run, numbered from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorkerId(pub u32);

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
    /// The toolset of a worker kind. Read-only for every kind until write
    /// tools exist.
    pub fn for_kind(_task: Task) -> Self {
        Self::ReadOnly
    }
}

#[cfg(test)]
mod tests;
