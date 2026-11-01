//! The runs of orchestrating panes in the local store (agent-orchestrator S9):
//! each run is saved as its workers change and closed when it ends, so a
//! restart can find the ones that did not and show them as interrupted. The
//! first time a pane goes into ORCH after a start asks the store; a session
//! that never orchestrates never touches the run tables. Never silent: a
//! failed save or read is said once.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

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
    /// A save of this run failed: the next one carries every worker.
    resend: bool,
    /// The chat folder the run started in.
    folder: Option<PathBuf>,
    /// The interrupted run this one resumes, named by the first save (and a
    /// resend after a failed one) so the store closes that row.
    supersedes: Option<u64>,
}

/// What the app keeps to save runs.
#[derive(Default)]
pub(super) struct RunKeep {
    /// The goal of the run a pane is about to start, until its first worker.
    goals: HashMap<PaneId, String>,
    runs: HashMap<RunId, Kept>,
    /// The runs a restart found interrupted and nobody answered yet, newest
    /// first. A run leaves when it is discarded, or while its resume starts.
    pub(super) interrupted: Vec<RunRecord>,
    /// The interrupted run each pane is resuming, until the new run has its
    /// first worker (then the store closes it) or fails to start (then it is
    /// offered again).
    pub(super) resuming: HashMap<PaneId, RunRecord>,
    /// The run a pane's abandoned resume named, kept after it went back on the
    /// offer list: the worker rows travel apart from the stream's `Done`, so a
    /// first worker may still arrive afterwards and then closes that run. It
    /// goes at the pane's next turn.
    pub(super) resume_of: HashMap<PaneId, u64>,
    /// The goal the next fresh turn of a pane records and shows instead of
    /// the message it sends (a resume's seed).
    pub(super) resume_goal: Option<String>,
    /// The store was asked for interrupted runs in this process.
    asked: bool,
    save_failure_said: bool,
    read_failure_said: bool,
}

impl RunKeep {
    /// Puts a run back among the ones offered, newest first.
    pub(super) fn offer_again(&mut self, run: RunRecord) {
        self.interrupted.push(run);
        self.interrupted.sort_by_key(|r| std::cmp::Reverse(r.id));
    }
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
            folder: self.folder.clone(),
            supersedes: None,
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
        self.run_keep.resume_of.remove(&id);
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
        let folder = (self.panes.get(&id).and_then(PaneState::chat_state))
            .and_then(ChatState::origin_cwd)
            .map(Path::to_path_buf);
        let keep = &mut self.run_keep;
        let kept = keep.runs.entry(row.run).or_insert_with(|| Kept {
            pane: id,
            goal: keep.goals.remove(&id).unwrap_or_default(),
            workers: BTreeMap::new(),
            resend: false,
            folder,
            supersedes: keep.resuming.remove(&id).map(|run| run.id).or_else(|| {
                let late = keep.resume_of.remove(&id)?;
                keep.interrupted.retain(|run| run.id != late);
                Some(late)
            }),
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
        let first = kept.workers.is_empty();
        let unchanged = kept.workers.get(&worker.id) == Some(&worker);
        if unchanged && !kept.resend {
            return Vec::new();
        }
        kept.workers.insert(worker.id, worker.clone());
        // After a failed save the store may lack earlier workers: send all.
        let resent = std::mem::take(&mut kept.resend);
        let changed = if resent {
            kept.workers.values().cloned().collect()
        } else {
            vec![worker]
        };
        let mut run = kept.record(row.run, RunPhase::Live, changed);
        // The first save closes the run this one resumes (a no-op if another
        // instance did already); so does a resend, in case that save failed.
        if first || resent {
            run.supersedes = kept.supersedes;
        }
        let pane = kept.pane;
        vec![Effect::Store(StoreCmd::SaveRun { pane, run })]
    }

    /// Run `run` is over: its record is closed and nothing more is saved.
    pub(super) fn keep_end(&mut self, run: RunId) -> Vec<Effect> {
        let Some(kept) = self.run_keep.runs.remove(&run) else {
            return Vec::new();
        };
        // The close carries every known worker: it repairs a lost save.
        let all = kept.workers.values().cloned().collect();
        let (pane, record) = (kept.pane, kept.record(run, RunPhase::Ended, all));
        vec![Effect::Store(StoreCmd::SaveRun { pane, run: record })]
    }

    /// Pane `id` is gone: the runs it had are closed in the store now, so the
    /// next start does not report them as interrupted by a restart, and its
    /// waiting goal goes. The run manager's own `Ended` saves nothing more.
    pub(super) fn keep_pane_closed(&mut self, id: PaneId) -> Vec<Effect> {
        self.run_keep.goals.remove(&id);
        self.run_keep.resume_goal = None;
        // A resume that never started leaves its run offered (to the other
        // panes of this folder).
        if let Some(run) = self.run_keep.resuming.remove(&id) {
            self.run_keep.offer_again(run);
        }
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
        self.resume_not_started(id);
    }

    /// The store's answer about runs: the interrupted ones are named in the
    /// pane, a failure is said once.
    pub(super) fn on_runs_reply(&mut self, reply: &StoreReply) {
        let (pane, lines) = match reply {
            StoreReply::RunsRecovered { pane, runs } => {
                self.run_keep.interrupted.clone_from(runs);
                // Only the runs of this chat's folder are named, goal and all;
                // the others are counted, their text stays out of the pane.
                let here = self.chat_folder(*pane);
                let (mine, other): (Vec<_>, Vec<_>) = runs
                    .iter()
                    .partition(|run| super::run_resume::same_folder(run, here.as_deref()));
                let mut lines: Vec<_> = (mine.iter().take(SHOWN_MAX))
                    .map(|run| run.interrupted_line())
                    .collect();
                if mine.len() > SHOWN_MAX {
                    lines.push(format!(
                        "{} more runs were interrupted by a restart",
                        mine.len() - SHOWN_MAX
                    ));
                }
                if !mine.is_empty() {
                    lines.push(super::run_resume::HOW_TO.to_owned());
                }
                if !other.is_empty() {
                    lines.push(format!(
                        "{} interrupted in other folders; open a chat there to resume or discard {}",
                        runs_noun(other.len()),
                        if other.len() == 1 { "it" } else { "them" }
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
                // Which run failed is not known: every run of the pane is
                // resent in full by its next save.
                if *saving {
                    for kept in self.run_keep.runs.values_mut().filter(|k| k.pane == *pane) {
                        kept.resend = true;
                    }
                }
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

/// `1 run`, `2 runs`.
fn runs_noun(count: usize) -> String {
    format!("{count} run{}", if count == 1 { "" } else { "s" })
}
