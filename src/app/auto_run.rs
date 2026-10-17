//! The `run` half of a chat's tool calls (design D21): `approval::route`
//! sends a safe command to a check that the runtime does off the UI thread,
//! and the answer is routed again, through the same function, before anything
//! is done with it. Every case that is not a clean pass ends in the prompt,
//! where `y` types the command and never presses Enter.

use super::*;
use crate::core::ai::chat::approval::{ApprovalAsk, Route};

/// A routed run, from its routing until its call ends. It lives on the target
/// pane, so closing the pane drops it, and its presence is the pane's
/// `executing` mark.
pub(super) struct RunSlot {
    pub(super) chat: PaneId,
    pub(super) turn: u64,
    pub(super) call: CallId,
    pub(super) command: String,
}

/// What the runtime says about the check of a run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RunEvent {
    /// The paths of the command passed in `cwd`, the shell's real directory
    /// when the runtime read it; `cd_target` is the real directory a `cd`
    /// goes to.
    Checked {
        call: CallId,
        cwd: PathBuf,
        cd_target: Option<PathBuf>,
    },
    /// The check refused the command or could not finish: the prompt opens
    /// with `reason`.
    Refused { call: CallId, reason: &'static str },
}

impl App {
    /// Reserves the slot of a routed run and asks the runtime to check it.
    /// The count is taken now and never refunded, so a run that falls back
    /// still used one of the turn's slots (fail closed).
    pub(super) fn check_run(
        &mut self,
        id: PaneId,
        (turn, call): (u64, CallId),
        target: PaneId,
        command: &str,
    ) -> Vec<Effect> {
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.note_auto_typed();
        }
        if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
            pty.run = Some(RunSlot {
                chat: id,
                turn,
                call,
                command: command.to_owned(),
            });
        }
        vec![Effect::CheckRun {
            chat: id,
            target,
            turn,
            call,
            command: command.to_owned(),
        }]
    }

    /// The runtime's answer for pane `target`. An answer for a call that has
    /// no slot there (it ended, or the pane or the chat closed) changes
    /// nothing. The slot is taken first, so the route that follows does not
    /// see the run itself as one already in flight.
    pub(super) fn on_run(&mut self, target: PaneId, event: RunEvent) -> Vec<Effect> {
        let (RunEvent::Checked { call, .. } | RunEvent::Refused { call, .. }) = event;
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        let Some(slot) = pty.run.take_if(|slot| slot.call == call) else {
            return Vec::new();
        };
        let ask = ApprovalAsk::Run {
            command: slot.command,
        };
        let why = match event {
            RunEvent::Refused { reason, .. } => Some(reason.to_owned()),
            RunEvent::Checked { .. } => match self.auto_route(slot.chat, target, &ask, true) {
                Route::Prompt(fallback) => fallback.map(|f| f.why.to_owned()),
                // Until the write exists, a checked run only prompts.
                Route::AutoRun(_) | Route::AutoType => Some("run is not enabled yet".to_owned()),
            },
        };
        self.open_prompt(slot.chat, (slot.turn, call), Some(target), ask, why)
    }

    /// Drops every run slot that `stale` names, which frees the pane's
    /// `executing` mark. A late answer for a dropped slot is ignored.
    pub(super) fn drop_run_slots(&mut self, stale: impl Fn(&RunSlot) -> bool) {
        for pty in self.panes.values_mut().filter_map(PaneState::pty_mut) {
            pty.run.take_if(|slot| stale(slot));
        }
    }
}
