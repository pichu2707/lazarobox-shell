//! The choice about an interrupted run (agent-orchestrator S9 9.1c): `Ctrl+Space
//! r r` starts a NEW run in the focused ORCH pane from the newest one a restart
//! interrupted, `Ctrl+Space r d` discards it. Nothing runs without that choice.
//! The new run is seeded with the stored goal (wrapped as data) and the names of
//! the unfinished workers, none of which relaunches until the user says so, and
//! it asks its read scope like any run. Either way the old row is closed in the
//! store, so it is not offered again. Every refusal says why.

use super::*;
use crate::core::ai::{
    chat::engine::Availability,
    orchestra::record::{RunPhase, RunRecord},
};

/// Said after the interrupted runs are named.
pub(super) const HOW_TO: &str = "resume the newest with Ctrl+Space r r (it asks the read scope again \
     and relaunches nothing without your word), or discard it with Ctrl+Space r d";
/// Said when nothing is waiting for an answer.
const NONE_LEFT: &str = "No interrupted run to resume or discard";
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
/// Said when the turn did not go and no other reason is known.
const NOT_SENT: &str = "The run was not resumed; try again";

impl App {
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
            }),
        })
    }

    /// What stops the choice about an interrupted run in the focused pane.
    fn choice_refusal(&self) -> Option<&'static str> {
        if !self.orch_gate() {
            return Some(GATE_OFF);
        }
        if !self.focused_is_chat() {
            return Some(CHAT_ONLY);
        }
        self.run_keep.interrupted.is_empty().then_some(NONE_LEFT)
    }

    /// `Prefix r r`: a new run from the newest interrupted one.
    pub(super) fn resume_run(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        let said = self.choice_refusal().or_else(|| self.resume_blocked(id));
        if let Some(text) = said {
            self.notice = Some(text.to_owned());
            return Vec::new();
        }
        let availability = self.turn_availability(id);
        let run = self.run_keep.interrupted[0].clone();
        let mut effects = self.send_with(id, &run.resume_message(), &availability, None);
        if effects.is_empty() {
            // `orch_refusal` shows its own reason in the pane.
            if self.orch_refusal(id).is_none() {
                self.notice = Some(NOT_SENT.to_owned());
            }
            return effects;
        }
        self.run_keep.interrupted.remove(0);
        // The new run keeps the goal the user gave, not the seed around it.
        self.keep_goal(id, &run.goal);
        effects.push(self.closing(&run));
        effects
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

    /// `Prefix r d`: the newest interrupted run is closed and not offered again.
    pub(super) fn discard_run(&mut self) -> Vec<Effect> {
        if let Some(text) = self.choice_refusal() {
            self.notice = Some(text.to_owned());
            return Vec::new();
        }
        let run = self.run_keep.interrupted.remove(0);
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
