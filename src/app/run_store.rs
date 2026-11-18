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
use crate::core::ai::chat::engine::StreamEvent;
use crate::core::ai::orchestra::{
    RunId,
    event::WorkerRow,
    record::{RunPhase, RunRecord, SHOWN_MAX, WorkerRecord},
    trail::{SHOWN, Trail},
};

/// Said once when a run could not be stored.
const SAVE_FAILED: &str = "could not save the run: it will not be listed after a restart, and its step trail may be missing";
/// Said once when the stored runs could not be read.
const READ_FAILED: &str = "could not read the stored runs";

/// A key that waits for the store's answer about interrupted runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Waiting {
    Resume,
    Discard,
}

/// How long the store has to answer before the read counts as failed.
pub(super) const RUNS_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Said when the store did not answer in time.
pub(super) const READ_TIMED_OUT: &str = "interrupted runs could not be read";

/// What the app knows of the interrupted runs: "none" is only ever said in
/// `Known`.
#[derive(Default)]
pub(super) enum RunsRead {
    /// The store was not asked in this process.
    #[default]
    Unknown,
    /// The store was asked; `pending` is the key pressed meanwhile, with the
    /// pane it was pressed in.
    Recovering {
        since: Instant,
        pending: Option<(PaneId, Waiting)>,
    },
    /// The store answered.
    Known,
    /// The read failed. `told`: a press already said so.
    Failed { told: bool, text: &'static str },
}

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
    /// The steps of each run (#771), until its end.
    trails: HashMap<RunId, Trail>,
    /// Runs whose stop report already listed the trail: their end says no more.
    reported: HashSet<RunId>,
    /// The steps taken before the pane's run is known, until it is.
    pending: HashMap<PaneId, Vec<Held>>,
    /// The latest coordinator turn of each pane and the run it belongs to.
    turns: HashMap<PaneId, TurnRun>,
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
    /// The run whose resume turn ended with the coordinator waiting for the
    /// user (no worker started), per pane: off the offer list, because the
    /// pane is handling it. The run the user's reply starts closes it; the
    /// link lasts that one reply, and any other use of the pane gives the run
    /// back to the list.
    pub(super) awaiting: HashMap<PaneId, Awaiting>,
    /// The goal the next fresh turn of a pane records and shows instead of
    /// the message it sends (a resume's seed).
    pub(super) resume_goal: Option<String>,
    /// What the app knows of the stored interrupted runs.
    pub(super) read: RunsRead,
    save_failure_said: bool,
    read_failure_said: bool,
}

/// The steps of a coordinator turn that has not named its run yet.
struct Held {
    turn: u64,
    trail: Trail,
    /// The turn's goal, once a later goal replaced it.
    goal: Option<String>,
}

/// The coordinator turn whose steps feed a trail.
struct TurnRun {
    turn: u64,
    /// The run it belongs to, once known.
    run: Option<RunId>,
    /// The run the pane had when the turn began: a fresh goal's is not it.
    prev: Option<RunId>,
}

/// A resumed run waiting for the user's reply.
pub(super) struct Awaiting {
    pub(super) run: RunRecord,
    /// The reply was sent: the turn it started is the one that may close
    /// the run.
    pub(super) replied: bool,
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
    fn record(
        &self,
        run: RunId,
        phase: RunPhase,
        changed: Vec<WorkerRecord>,
        trail: Option<String>,
    ) -> Box<RunRecord> {
        Box::new(RunRecord {
            id: run.0,
            goal: self.goal.clone(),
            phase,
            workers: changed,
            folder: self.folder.clone(),
            supersedes: None,
            trail,
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

    /// Coordinator turn `turn` of pane `id` starts: a continuation belongs to
    /// the run it attaches to, a fresh goal to a run that is not yet named
    /// (and is not the one the pane has now).
    pub(super) fn begin_turn(&mut self, id: PaneId, turn: u64, attach: Option<RunId>) {
        let prev =
            (self.panes.get(&id).and_then(PaneState::chat_state)).and_then(ChatState::orch_run);
        // The steps of the turn this one follows wait for their run, which
        // may be named after this turn began (a run with no worker ends after
        // the next goal was typed): they keep that turn's goal. Older ones
        // are dropped, and said.
        let before = self.run_keep.turns.get(&id).map(|t| t.turn);
        let goal = self.run_keep.goals.get(&id).cloned();
        let held = self.run_keep.pending.entry(id).or_default();
        let mut lost = 0;
        held.retain(|h| {
            let keep = Some(h.turn) == before;
            lost += if keep { 0 } else { h.trail.lines().len() };
            keep
        });
        if let Some(h) = held.first_mut() {
            h.goal = goal;
        }
        if lost > 0
            && let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut)
        {
            let noun = if lost == 1 { "step" } else { "steps" };
            chat.push_system_line(format!(
                "step trail of the unstarted run discarded ({lost} {noun})"
            ));
        }
        self.run_keep.turns.insert(
            id,
            TurnRun {
                turn,
                run: attach,
                prev,
            },
        );
    }

    /// Whether the store may hold `run` of pane `id`: it has a row, it is the
    /// pane's run, or it is the run the latest coordinator turn started.
    fn owns(&self, id: PaneId, run: RunId) -> bool {
        let current =
            (self.panes.get(&id).and_then(PaneState::chat_state)).and_then(ChatState::orch_run);
        self.run_keep.runs.contains_key(&run)
            || current == Some(run)
            || self
                .run_keep
                .turns
                .get(&id)
                .is_some_and(|t| t.run == Some(run))
    }

    /// A step of coordinator turn `turn` in pane `id`: a tool call, or its
    /// result. Only the turn that `begin_turn` named feeds a trail. Steps taken
    /// before the run is named wait; the trail is saved when a result arrives.
    pub(super) fn keep_step(&mut self, id: PaneId, turn: u64, step: &StreamEvent) -> Vec<Effect> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state);
        let named = self.run_keep.turns.get(&id).filter(|t| t.turn == turn);
        if !chat.is_some_and(ChatState::orch_turn_in_flight) {
            return Vec::new();
        }
        let Some(run) = named.map(|t| t.run) else {
            return Vec::new();
        };
        let trail = match run {
            Some(run) if self.ended_runs.contains(&run) => return Vec::new(),
            Some(run) => {
                self.ensure_kept(id, run);
                self.run_keep.trails.entry(run).or_default()
            }
            None => {
                let held = self.run_keep.pending.entry(id).or_default();
                let at = held.iter().position(|h| h.turn == turn).unwrap_or_else(|| {
                    held.push(Held {
                        turn,
                        trail: Trail::default(),
                        goal: None,
                    });
                    held.len() - 1
                });
                &mut held[at].trail
            }
        };
        match step {
            // A worker's ask shows in the pane's chat too: not the coordinator's.
            StreamEvent::ToolCall {
                approval: Some(ask),
                ..
            } if ask.worker().is_some() => {}
            StreamEvent::ToolCall {
                id, name, summary, ..
            } => trail.call(*id, name, summary),
            StreamEvent::ToolResult {
                id: call,
                status,
                summary,
            } => {
                trail.result(*call, *status, summary);
                return run.map_or_else(Vec::new, |run| self.save_trail(id, run));
            }
            _ => {}
        }
        Vec::new()
    }

    /// A handoff of `run`: its unit, verdict and reason.
    pub(super) fn keep_handoff(
        &mut self,
        id: PaneId,
        run: RunId,
        (who, verdict, reason): (&str, &str, &str),
    ) -> Vec<Effect> {
        if self.ended_runs.contains(&run) || !self.owns(id, run) {
            return Vec::new();
        }
        self.run_keep
            .trails
            .entry(run)
            .or_default()
            .handoff(who, verdict, reason);
        self.save_trail(id, run)
    }

    /// An event names `run` in pane `id`. If the latest coordinator turn has
    /// no run yet and this is not the run the pane had before it, it is that
    /// turn's run: the steps taken so far join it, and it gets its row in the
    /// store even with no worker.
    pub(super) fn adopt_trail(&mut self, id: PaneId, run: RunId, starts: bool) -> Vec<Effect> {
        let known =
            self.run_keep.runs.contains_key(&run) || self.run_keep.trails.contains_key(&run);
        let Some(turn) = self.run_keep.turns.get_mut(&id) else {
            return Vec::new();
        };
        if self.ended_runs.contains(&run) || known {
            return Vec::new();
        }
        let held = self.run_keep.pending.entry(id).or_default();
        let unnamed = turn.run.is_none() && turn.prev != Some(run);
        let current = held.iter().position(|h| h.turn == turn.turn);
        // `starts`: the event shows a run starting (a worker, a handoff, a
        // gate), which is the latest turn's. An end or a stop may be the end
        // of an earlier turn's run that no event named before: that turn's
        // steps are older than the latest one's, so they come first.
        let early = if starts {
            if unnamed {
                turn.run = Some(run);
            }
            current.filter(|_| unnamed).map(|at| held.remove(at))
        } else if let Some(at) = held.iter().position(|h| h.turn != turn.turn) {
            Some(held.remove(at))
        } else if let (true, Some(at)) = (unnamed, current) {
            turn.run = Some(run);
            Some(held.remove(at))
        } else {
            None
        };
        let Some(early) = early else {
            return Vec::new();
        };
        self.run_keep
            .trails
            .entry(run)
            .or_default()
            .absorb(early.trail);
        if let Some(goal) = early.goal {
            self.make_kept(id, run, goal, None);
        }
        self.save_trail(id, run)
    }

    /// The trail of `run` is saved alone; its row is made if no worker did.
    fn save_trail(&mut self, id: PaneId, run: RunId) -> Vec<Effect> {
        if self.ended_runs.contains(&run) {
            return Vec::new();
        }
        let text = self.run_keep.trails.get(&run).map(Trail::to_text);
        let kept = self.ensure_kept(id, run);
        let mut record = kept.record(run, RunPhase::Live, Vec::new(), text);
        // A resumed run's first save closes the run it resumes (a no-op
        // when that is done), like the first worker save does.
        record.supersedes = kept.supersedes;
        vec![Effect::Store(StoreCmd::SaveRun {
            pane: id,
            run: record,
        })]
    }

    /// The lines that report the trail of `run`: empty when it has no step.
    pub(super) fn trail_report(&self, run: RunId) -> Vec<String> {
        (self.run_keep.trails.get(&run)).map_or_else(Vec::new, |trail| trail.shown(SHOWN))
    }

    /// The trail for a run that stops (turn cap, timeout): its end follows
    /// and does not list it again.
    pub(super) fn report_closing(&mut self, run: RunId) -> Vec<String> {
        let lines = self.trail_report(run);
        if !lines.is_empty() {
            self.run_keep.reported.insert(run);
        }
        lines
    }

    /// The trail at the end of `run`: after a cancel, and after a clean
    /// finish while the run is still the pane's own (a replaced run's end
    /// says nothing in the newer one). Once only.
    pub(super) fn report_end(&mut self, id: PaneId, run: RunId) -> Vec<String> {
        if self.run_keep.reported.remove(&run) {
            return Vec::new();
        }
        let chat = self.panes.get(&id).and_then(PaneState::chat_state);
        let own =
            chat.is_some_and(|c| c.run_cancelled(run) || c.orch_run().is_none_or(|r| r == run));
        if own {
            self.trail_report(run)
        } else {
            Vec::new()
        }
    }

    /// The coordinator turn of `id` failed with its run still open: the
    /// trail says what the run did before it. Nothing when the failure ends
    /// the run (a continuation was waiting: its end lists the trail).
    pub(super) fn report_failed_turn(&self, id: PaneId) -> Vec<String> {
        let chat = self.panes.get(&id).and_then(PaneState::chat_state);
        match chat
            .filter(|c| !c.continue_pending())
            .and_then(ChatState::orch_run)
        {
            Some(run) => self.trail_report(run),
            None => Vec::new(),
        }
    }

    /// Asks the store for interrupted runs, once per process.
    pub(super) fn recover_runs(&mut self, id: PaneId) -> Vec<Effect> {
        if !matches!(self.run_keep.read, RunsRead::Unknown) {
            return Vec::new();
        }
        self.run_keep.read = RunsRead::Recovering {
            since: self.now(),
            pending: None,
        };
        vec![Effect::Store(StoreCmd::RecoverRuns { pane: id })]
    }

    /// A worker of run `row.run` changed: the run is saved with it.
    pub(super) fn keep_worker(&mut self, id: PaneId, row: &WorkerRow) -> Vec<Effect> {
        if self.ended_runs.contains(&row.run) {
            return Vec::new();
        }
        let trail = self.run_keep.trails.get(&row.run).map(Trail::to_text);
        let kept = self.ensure_kept(id, row.run);
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
        let mut run = kept.record(row.run, RunPhase::Live, changed, trail);
        // The first save closes the run this one resumes (a no-op if another
        // instance did already); so does a resend, in case that save failed.
        if first || resent {
            run.supersedes = kept.supersedes;
        }
        let pane = kept.pane;
        vec![Effect::Store(StoreCmd::SaveRun { pane, run })]
    }

    /// The store's view of `run` of pane `id`, made on first use.
    fn ensure_kept(&mut self, id: PaneId, run: RunId) -> &mut Kept {
        if !self.run_keep.runs.contains_key(&run) {
            let keep = &mut self.run_keep;
            let goal = keep.goals.remove(&id).unwrap_or_default();
            let supersedes = keep
                .resuming
                .remove(&id)
                .map(|run| run.id)
                .or_else(|| keep.awaiting.remove(&id).map(|waiting| waiting.run.id))
                .or_else(|| {
                    let late = keep.resume_of.remove(&id)?;
                    keep.interrupted.retain(|run| run.id != late);
                    Some(late)
                });
            self.make_kept(id, run, goal, supersedes);
        }
        self.run_keep
            .runs
            .entry(run)
            .or_insert_with(|| unreachable!())
    }

    /// The store's view of `run`, made if there is none, with the goal it
    /// started from and the interrupted run it resumes.
    fn make_kept(&mut self, id: PaneId, run: RunId, goal: String, supersedes: Option<u64>) {
        let folder = (self.panes.get(&id).and_then(PaneState::chat_state))
            .and_then(ChatState::origin_cwd)
            .map(Path::to_path_buf);
        self.run_keep.runs.entry(run).or_insert_with(|| Kept {
            pane: id,
            goal,
            workers: BTreeMap::new(),
            resend: false,
            folder,
            supersedes,
        });
    }

    /// Run `run` is over: its record is closed and nothing more is saved.
    pub(super) fn keep_end(&mut self, run: RunId) -> Vec<Effect> {
        let trail = self.run_keep.trails.remove(&run).map(|t| t.to_text());
        let Some(kept) = self.run_keep.runs.remove(&run) else {
            return Vec::new();
        };
        // The close carries every known worker: it repairs a lost save.
        let all = kept.workers.values().cloned().collect();
        let (pane, record) = (kept.pane, kept.record(run, RunPhase::Ended, all, trail));
        vec![Effect::Store(StoreCmd::SaveRun { pane, run: record })]
    }

    /// Pane `id` is gone: the runs it had are closed in the store now, so the
    /// next start does not report them as interrupted by a restart, and its
    /// waiting goal goes. The run manager's own `Ended` saves nothing more.
    pub(super) fn keep_pane_closed(&mut self, id: PaneId) -> Vec<Effect> {
        self.run_keep.goals.remove(&id);
        self.run_keep.pending.remove(&id);
        self.run_keep.turns.remove(&id);
        self.release_wait(id, false);
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
        // The steps of a start that failed are no run's.
        let turn = self.run_keep.turns.get(&id).map(|t| t.turn);
        if let Some(held) = self.run_keep.pending.get_mut(&id) {
            held.retain(|h| Some(h.turn) != turn);
        }
        self.resume_not_started(id);
    }

    /// The store's answer about runs: the interrupted ones are named in the
    /// pane, a failure is said once.
    pub(super) fn on_runs_reply(&mut self, reply: &StoreReply) -> Vec<Effect> {
        let read_reply = matches!(
            reply,
            StoreReply::RunsRecovered { .. } | StoreReply::RunsFailed { saving: false, .. }
        );
        let waiting = match read_reply.then(|| std::mem::take(&mut self.run_keep.read)) {
            Some(RunsRead::Recovering { pending, .. }) => pending,
            _ => None,
        };
        let looking = self.notice.as_deref() == Some(super::run_resume::LOOKING);
        let (pane, lines) = match reply {
            StoreReply::RunsRecovered { pane, runs } => {
                self.run_keep.read = RunsRead::Known;
                self.run_keep.interrupted.clone_from(runs);
                // Only the runs of this chat's folder are named, goal and all;
                // the others are counted, their text stays out of the pane.
                let here = self.chat_folder(*pane);
                let (mine, other): (Vec<_>, Vec<_>) = runs
                    .iter()
                    .partition(|run| super::run_resume::same_folder(run, here.as_deref()));
                let mut lines: Vec<_> = (mine.iter().take(SHOWN_MAX))
                    .flat_map(|run| {
                        let steps = (run.trail.as_deref()).map(Trail::from_text);
                        let shown = steps.map_or_else(Vec::new, |t| t.shown(SHOWN));
                        std::iter::once(run.interrupted_line()).chain(shown)
                    })
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
                if !*saving {
                    // Never "none": the next press says why and retries.
                    let told = waiting.is_some();
                    self.run_keep.read = RunsRead::Failed {
                        told,
                        text: READ_FAILED,
                    };
                    if told || looking {
                        self.notice = Some(READ_FAILED.to_owned());
                        self.dirty = true;
                    }
                }
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
                    return Vec::new();
                }
                let text = if *saving { SAVE_FAILED } else { READ_FAILED };
                (*pane, vec![text.to_owned()])
            }
            _ => return Vec::new(),
        };
        if let Some(chat) = self.panes.get_mut(&pane).and_then(PaneState::chat_mut) {
            for line in lines {
                chat.push_system_line(line);
            }
            self.dirty = true;
        }
        if !matches!(reply, StoreReply::RunsRecovered { .. }) {
            return Vec::new();
        }
        // The notice that said the store was being asked does not outlive the
        // answer; the key that waited gets its answer now.
        if looking {
            self.notice = None;
            self.dirty = true;
        }
        match waiting {
            Some((at, key)) if self.tab().focus == at && self.panes.contains_key(&at) => {
                match key {
                    Waiting::Resume => self.resume_run(),
                    Waiting::Discard => self.discard_run(),
                }
            }
            _ => Vec::new(),
        }
    }
}

/// `1 run`, `2 runs`.
fn runs_noun(count: usize) -> String {
    format!("{count} run{}", if count == 1 { "" } else { "s" })
}
