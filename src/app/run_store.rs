//! The runs of orchestrating panes in the local store (agent-orchestrator S9):
//! each run is saved as its workers change and closed when it ends, so a
//! restart can find the ones that did not and show them as interrupted. The
//! first time a pane goes into ORCH after a start asks the store; a session
//! that never orchestrates never touches the run tables. Never silent: a
//! failed save or read is said once.

use std::collections::{BTreeMap, HashMap};

use super::*;
use crate::core::ai::orchestra::{
    RunId,
    event::WorkerRow,
    record::{RunPhase, RunRecord, SHOWN_MAX, WorkerRecord},
};

/// Said once when a run could not be stored.
const SAVE_FAILED: &str = "could not save the run: it will not be listed after a restart";
/// Said once when the stored runs could not be read.
const READ_FAILED: &str = "could not read the stored runs";

/// A run as the app last told the store.
struct Kept {
    pane: PaneId,
    goal: String,
    workers: BTreeMap<u32, WorkerRecord>,
}

/// What the app keeps to save runs.
#[derive(Default)]
pub(super) struct RunKeep {
    /// The goal of the run a pane is about to start, until its first worker.
    goals: HashMap<PaneId, String>,
    runs: HashMap<RunId, Kept>,
    /// The store was asked for interrupted runs in this process.
    asked: bool,
    save_failure_said: bool,
    read_failure_said: bool,
}

impl Kept {
    /// The run's record carrying only the workers that changed: the store
    /// upserts them and keeps the others.
    fn record(&self, run: RunId, phase: RunPhase, changed: Vec<WorkerRecord>) -> Box<RunRecord> {
        Box::new(RunRecord {
            id: run.0,
            goal: self.goal.clone(),
            phase,
            workers: changed,
        })
    }
}

impl App {
    /// How many goals wait for their run's first worker.
    #[cfg(test)]
    pub(super) fn kept_goals(&self) -> usize {
        self.run_keep.goals.len()
    }

    /// A fresh run of pane `id` starts from `goal`: the user's words, which
    /// the store keeps sanitized and cut.
    pub(super) fn keep_goal(&mut self, id: PaneId, goal: &str) {
        self.run_keep.goals.insert(id, RunRecord::stored_goal(goal));
    }

    /// Asks the store for interrupted runs, once per process.
    pub(super) fn recover_runs(&mut self, id: PaneId) -> Vec<Effect> {
        if std::mem::replace(&mut self.run_keep.asked, true) {
            return Vec::new();
        }
        vec![Effect::Store(StoreCmd::RecoverRuns { pane: id })]
    }

    /// A worker of run `row.run` changed: the run is saved with it.
    pub(super) fn keep_worker(&mut self, id: PaneId, row: &WorkerRow) -> Vec<Effect> {
        if self.ended_runs.contains(&row.run) {
            return Vec::new();
        }
        let keep = &mut self.run_keep;
        let kept = keep.runs.entry(row.run).or_insert_with(|| Kept {
            pane: id,
            goal: keep.goals.remove(&id).unwrap_or_default(),
            workers: BTreeMap::new(),
        });
        let worker = WorkerRecord {
            id: row.id.0,
            kind: row.kind.id().to_owned(),
            engine: row.engine.clone(),
            model: row.model.clone(),
            status: row.status.label().to_owned(),
        };
        // Usage, elapsed time and the like are not stored: a row that changes
        // none of what is stored writes nothing.
        if kept.workers.get(&worker.id) == Some(&worker) {
            return Vec::new();
        }
        kept.workers.insert(worker.id, worker.clone());
        let (pane, run) = (
            kept.pane,
            kept.record(row.run, RunPhase::Live, vec![worker]),
        );
        vec![Effect::Store(StoreCmd::SaveRun { pane, run })]
    }

    /// Run `run` is over: its record is closed and nothing more is saved.
    pub(super) fn keep_end(&mut self, run: RunId) -> Vec<Effect> {
        let Some(kept) = self.run_keep.runs.remove(&run) else {
            return Vec::new();
        };
        let (pane, record) = (kept.pane, kept.record(run, RunPhase::Ended, Vec::new()));
        vec![Effect::Store(StoreCmd::SaveRun { pane, run: record })]
    }

    /// Pane `id` is gone: the runs it had are closed in the store now, so the
    /// next start does not report them as interrupted by a restart, and its
    /// waiting goal goes. The run manager's own `Ended` saves nothing more.
    pub(super) fn keep_pane_closed(&mut self, id: PaneId) -> Vec<Effect> {
        self.run_keep.goals.remove(&id);
        let runs: Vec<RunId> = (self.run_keep.runs.iter())
            .filter(|(_, kept)| kept.pane == id)
            .map(|(run, _)| *run)
            .collect();
        let mut effects = Vec::new();
        for run in runs {
            self.ended_runs.insert(run);
            effects.extend(self.keep_end(run));
        }
        effects
    }

    /// The turn of pane `id` failed before any run began: its goal is not
    /// kept for a later one.
    pub(super) fn forget_goal(&mut self, id: PaneId) {
        self.run_keep.goals.remove(&id);
    }

    /// The store's answer about runs: the interrupted ones are named in the
    /// pane, a failure is said once.
    pub(super) fn on_runs_reply(&mut self, reply: &StoreReply) {
        let (pane, lines) = match reply {
            StoreReply::RunsRecovered { pane, runs } => {
                let mut lines: Vec<_> = (runs.iter().take(SHOWN_MAX))
                    .map(RunRecord::interrupted_line)
                    .collect();
                if runs.len() > SHOWN_MAX {
                    lines.push(format!(
                        "{} more runs were interrupted by a restart",
                        runs.len() - SHOWN_MAX
                    ));
                }
                (*pane, lines)
            }
            StoreReply::RunsFailed { pane, saving } => {
                let said = if *saving {
                    &mut self.run_keep.save_failure_said
                } else {
                    &mut self.run_keep.read_failure_said
                };
                if std::mem::replace(said, true) {
                    return;
                }
                let text = if *saving { SAVE_FAILED } else { READ_FAILED };
                (*pane, vec![text.to_owned()])
            }
            _ => return,
        };
        if let Some(chat) = self.panes.get_mut(&pane).and_then(PaneState::chat_mut) {
            for line in lines {
                chat.push_system_line(line);
            }
            self.dirty = true;
        }
    }
}
