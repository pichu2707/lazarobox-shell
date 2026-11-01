//! The choice about an interrupted run (agent-orchestrator S9 9.1c): `Ctrl+Space
//! r r` starts a NEW run in the focused ORCH pane from the newest one a restart
//! interrupted in this chat's folder, `Ctrl+Space r d` discards it. Nothing runs
//! without that choice. The new run is seeded with the stored goal (wrapped as
//! data) and the names of the unfinished workers; it sets its read scope like
//! any run. A run of another folder, or one stored with no folder, is never
//! offered here: its goal would leave for this pane's engine. A discard closes
//! the old row at once; a resume closes it with the new run's first worker (the
//! store does it, only while the row is still interrupted), and until then the
//! row is not offered twice here and comes back if the turn never gets a worker.
//! Two instances can each resume the same row: the second close changes nothing.
//! Every refusal says why.

use std::path::{Path, PathBuf};

use super::run_store::Awaiting;
use super::*;
use crate::core::ai::{
    chat::engine::Availability,
    orchestra::record::{RunPhase, RunRecord},
};

/// Said after the interrupted runs are named.
pub(super) const HOW_TO: &str = "resume the newest with Ctrl+Space r r (the new run sets its read scope \
     again from the chat folder, and the orchestrator is told to relaunch no worker until you \
     say so), or discard it with Ctrl+Space r d";
/// Said when nothing is waiting for an answer.
const NONE_LEFT: &str = "No interrupted run to resume or discard";
/// Said when the interrupted runs belong to other folders.
const OTHER_FOLDER: &str =
    "The interrupted runs belong to other folders; open a chat there to resume or discard them";
/// Said when the orchestrator gate is off.
const GATE_OFF: &str = "The orchestrator is off; enable it in the menu (Ctrl+Space m)";
/// Said on a pane that is not a chat.
const CHAT_ONLY: &str = "Resuming a run is for chat panes; open one with Ctrl+Space c";
/// Said in a chat with ORCH off.
const ORCH_OFF: &str = "Turn ORCH on in this pane first (Ctrl+O), then resume the run";
/// Said while a run of the pane is active.
const RUN_ACTIVE: &str = "This pane has a run; cancel it first (Ctrl+C), then resume the run";
/// Said while the pane is busy or its draft would be lost.
const NOT_IDLE: &str =
    "Finish the current answer and clear the composer first, then resume the run";
/// Said before the engine or the conversation's prompt is ready.
const NOT_READY: &str = "The engine is not ready yet; try again in a moment";
/// Said when the pane's resumed run waits for the user's reply.
const WAITS: &str =
    "This pane's resumed run waits for your reply; send it, or toggle ORCH to get the run back";
/// Said when the turn did not go and no other reason is known.
const NOT_SENT: &str = "The run was not resumed; try again";

/// Whether `run` started in `here`: both known and equal. A run stored with no
/// folder (before folders were kept) is never the same as any folder.
pub(super) fn same_folder(run: &RunRecord, here: Option<&Path>) -> bool {
    matches!((run.folder.as_deref(), here), (Some(a), Some(b)) if a == b)
}

impl App {
    /// The folder of chat `id`, as the chat store keeps it.
    pub(super) fn chat_folder(&self, id: PaneId) -> Option<PathBuf> {
        let chat = self.panes.get(&id)?.chat_state()?;
        chat.origin_cwd().map(Path::to_path_buf)
    }

    /// The record that closes run `run` in the store: ended, with no workers
    /// (the store keeps the ones it has).
    fn closing(&self, run: &RunRecord) -> Effect {
        Effect::Store(StoreCmd::SaveRun {
            pane: self.tab().focus,
            run: Box::new(RunRecord {
                id: run.id,
                goal: run.goal.clone(),
                phase: RunPhase::Ended,
                workers: Vec::new(),
                folder: None,
                supersedes: None,
            }),
        })
    }

    /// Which interrupted run the focused chat may act on: the newest of its
    /// folder, or why there is none.
    fn offered_here(&self) -> Result<usize, &'static str> {
        if !self.orch_gate() {
            return Err(GATE_OFF);
        }
        if !self.focused_is_chat() {
            return Err(CHAT_ONLY);
        }
        if self.run_keep.awaiting.contains_key(&self.tab().focus) {
            return Err(WAITS);
        }
        let here = self.chat_folder(self.tab().focus);
        let runs = &self.run_keep.interrupted;
        (runs.iter())
            .position(|run| same_folder(run, here.as_deref()))
            .ok_or(if runs.is_empty() {
                NONE_LEFT
            } else {
                OTHER_FOLDER
            })
    }

    /// `Prefix r r`: a new run from the newest interrupted one of this folder.
    pub(super) fn resume_run(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        let at = match self.offered_here() {
            Ok(at) => at,
            Err(text) => {
                self.notice = Some(text.to_owned());
                return Vec::new();
            }
        };
        if let Some(text) = self.resume_blocked(id) {
            self.notice = Some(text.to_owned());
            return Vec::new();
        }
        let availability = self.turn_availability(id);
        let run = self.run_keep.interrupted[at].clone();
        // The new run records and shows the goal the user gave, not the seed.
        self.run_keep.resume_goal = Some(run.goal.clone());
        let effects = self.send_with(id, &run.resume_message(), &availability, None);
        self.run_keep.resume_goal = None;
        if effects.is_empty() {
            // `orch_refusal` shows its own reason in the pane.
            if self.orch_refusal(id).is_none() {
                self.notice = Some(NOT_SENT.to_owned());
            }
            return effects;
        }
        // Off the list while it starts; the first worker closes it in the store.
        self.run_keep.interrupted.remove(at);
        self.run_keep.resuming.insert(id, run);
        effects
    }

    /// The resume of pane `id` got no worker (its turn failed, was cancelled
    /// or ended): the run is offered again and the pane says so.
    pub(super) fn resume_not_started(&mut self, id: PaneId) {
        let Some(run) = self.run_keep.resuming.remove(&id) else {
            return;
        };
        self.run_keep.resume_of.insert(id, run.id);
        self.say_offered_again(id, run);
    }

    /// The resume turn of pane `id` ended normally with no worker: the
    /// coordinator waits for the user, whose reply may relaunch the worker.
    /// The run leaves the list while the pane waits, and the pane says so.
    pub(super) fn resume_awaits_reply(&mut self, id: PaneId) {
        let Some(run) = self.run_keep.resuming.remove(&id) else {
            return;
        };
        let line = format!(
            "run {} waits for your reply; it closes when a worker starts",
            run.id
        );
        self.run_keep.awaiting.insert(
            id,
            Awaiting {
                run,
                replied: false,
            },
        );
        self.say_in_pane(id, line);
    }

    /// The user's turn in pane `id` starts while a resumed run waits: it is
    /// the reply.
    pub(super) fn reply_started(&mut self, id: PaneId) {
        if let Some(waiting) = self.run_keep.awaiting.get_mut(&id) {
            waiting.replied = true;
        }
    }

    /// A turn of pane `id` ended. If it was the reply to a resumed run: when
    /// it ended normally the wait is over and the run goes back on the list,
    /// still linked so a worker that arrives after the turn's end closes it;
    /// when it failed or was cancelled the pane keeps waiting for a retry.
    pub(super) fn reply_ended(&mut self, id: PaneId, done: bool) {
        match self.run_keep.awaiting.get_mut(&id) {
            Some(waiting) if waiting.replied && done => self.release_wait(id, true),
            Some(waiting) if waiting.replied => waiting.replied = false,
            _ => {}
        }
    }

    /// Pane `id` stops waiting for a reply (it sent something else, ORCH was
    /// toggled, its reply ended, it closed): the run is still interrupted and
    /// goes back on the list, and the pane says so. With `link`, a worker that
    /// arrives later still closes it, as after an abandoned resume.
    pub(super) fn release_wait(&mut self, id: PaneId, link: bool) {
        if let Some(waiting) = self.run_keep.awaiting.remove(&id) {
            if link {
                self.run_keep.resume_of.insert(id, waiting.run.id);
            }
            self.say_offered_again(id, waiting.run);
        }
    }

    fn say_offered_again(&mut self, id: PaneId, run: RunRecord) {
        let line = format!(
            "run {} is still interrupted; resume it again with Ctrl+Space r r",
            run.id
        );
        self.run_keep.offer_again(run);
        self.say_in_pane(id, line);
    }

    fn say_in_pane(&mut self, id: PaneId, line: String) {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(line);
            self.dirty = true;
        }
    }

    /// Why pane `id` cannot start the resumed run right now.
    fn resume_blocked(&self, id: PaneId) -> Option<&'static str> {
        if !self.orchestrating(id) {
            return Some(ORCH_OFF);
        }
        if self.orch_run_active(id) {
            return Some(RUN_ACTIVE);
        }
        let chat = self.panes.get(&id)?.chat_state()?;
        if chat.phase() != Phase::Idle || !chat.composer().is_empty() {
            return Some(NOT_IDLE);
        }
        let ready = !matches!(self.turn_availability(id), Availability::Checking);
        (!ready || chat.prompt().is_none()).then_some(NOT_READY)
    }

    /// `Prefix r d`: the newest interrupted run of this folder is closed and
    /// not offered again.
    pub(super) fn discard_run(&mut self) -> Vec<Effect> {
        let at = match self.offered_here() {
            Ok(at) => at,
            Err(text) => {
                self.notice = Some(text.to_owned());
                return Vec::new();
            }
        };
        let run = self.run_keep.interrupted.remove(at);
        let id = self.tab().focus;
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(format!(
                "run {} discarded; it will not be offered again",
                run.id
            ));
        }
        self.dirty = true;
        vec![self.closing(&run)]
    }
}
