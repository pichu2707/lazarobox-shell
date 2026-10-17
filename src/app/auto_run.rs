//! The `run` half of a chat's tool calls (design D21): `approval::route`
//! sends a safe command to a check that the runtime does off the UI thread,
//! and the answer is routed again, through the same function, before anything
//! is done with it. Every case that is not a clean pass ends in the prompt,
//! where `y` types the command and never presses Enter.
//!
//! One-write invariant: a run is exactly one `Effect::AutoRun`: the checked
//! command, framed like a `y` write, plus one `\r`. It is built only here, from
//! the second route, so `y` and `auto` cannot press Enter.
//!
//! Residual: the cwd is compared at the write, but a symlink swapped between
//! the check's `canonicalize` and the shell's `chdir` is not seen (it needs a
//! local attacker who can write a path component).

use super::*;
use crate::core::ai::chat::approval::{ApprovalAsk, NOT_ALLOWED, Route};

/// Why a call is denied when its target pane is closed or has no process.
pub const PANE_GONE: &str = "the pane is gone";

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
    /// The write step found the pane moved or busy, and wrote nothing: the
    /// prompt opens with `reason`.
    NotRun { call: CallId, reason: &'static str },
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
    /// see the run itself as one already in flight; it goes back only when the
    /// write is emitted, and stays until the call ends.
    pub(super) fn on_run(&mut self, target: PaneId, event: RunEvent) -> Vec<Effect> {
        let (RunEvent::Checked { call, .. }
        | RunEvent::Refused { call, .. }
        | RunEvent::NotRun { call, .. }) = event;
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        let Some(slot) = pty.run.take_if(|slot| slot.call == call) else {
            return Vec::new();
        };
        // The write step found no process: nothing to ask the user about.
        if matches!(
            event,
            RunEvent::NotRun {
                reason: PANE_GONE,
                ..
            }
        ) {
            return Self::deny(slot.chat, slot.turn, call, PANE_GONE);
        }
        let ask = ApprovalAsk::Run {
            command: slot.command.clone(),
        };
        let why = match event {
            RunEvent::Refused { reason, .. } => {
                if reason == NOT_ALLOWED {
                    self.note_policy_fallback(slot.chat);
                }
                Some(reason.to_owned())
            }
            RunEvent::NotRun { reason, .. } => Some(reason.to_owned()),
            RunEvent::Checked { cwd, .. } => match self.auto_route(slot.chat, target, &ask, true) {
                Route::AutoRun(safe) => {
                    return match self.frame_bytes(target, safe.as_str()) {
                        Ok(bytes) => self.auto_run(slot, target, bytes, cwd),
                        Err(refused) => Self::deny(slot.chat, slot.turn, call, refused.reason),
                    };
                }
                Route::Prompt(fallback) => self.fallback_why(slot.chat, fallback),
                Route::AutoType => None,
            },
        };
        self.open_prompt(slot.chat, (slot.turn, call), Some(target), ask, why)
    }

    /// The run is decided: its one write, the command and one `\r`, goes to
    /// the runtime together with the directory it was checked in. The slot
    /// stays (the pane is executing) until the call ends; the fallbacks before
    /// it are forgiven only when the call ends `Ok`.
    fn auto_run(
        &mut self,
        slot: RunSlot,
        target: PaneId,
        mut bytes: Vec<u8>,
        cwd: PathBuf,
    ) -> Vec<Effect> {
        bytes.push(b'\r');
        let effect = Effect::AutoRun {
            chat: slot.chat,
            target,
            bytes,
            number: self.pane_number(target).unwrap_or(0),
            turn: slot.turn,
            call: slot.call,
            cwd,
        };
        if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
            pty.run = Some(slot);
        }
        self.dirty = true;
        vec![effect]
    }

    /// A policy refusal counts toward the pause like a grammar one.
    fn note_policy_fallback(&mut self, chat: PaneId) {
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.note_run_fallback();
        }
    }

    /// Denies the calls whose run is in flight in the panes `ids`, about to
    /// close (a chat closing with them is answered by its own end).
    pub(super) fn deny_closing_runs(&mut self, ids: &[PaneId]) -> Vec<Effect> {
        let mut slots = Vec::new();
        for id in ids {
            if let Some(pty) = self.panes.get_mut(id).and_then(PaneState::pty_mut) {
                slots.extend(pty.run.take());
            }
        }
        (slots.into_iter())
            .filter(|slot| !ids.contains(&slot.chat))
            .flat_map(|slot| Self::deny(slot.chat, slot.turn, slot.call, PANE_GONE))
            .collect()
    }

    /// Drops every run slot that `stale` names, which frees the pane's
    /// `executing` mark, and says whether there was one. A late answer for a
    /// dropped slot is ignored.
    pub(super) fn drop_run_slots(&mut self, stale: impl Fn(&RunSlot) -> bool) -> bool {
        let mut dropped = false;
        for pty in self.panes.values_mut().filter_map(PaneState::pty_mut) {
            dropped |= pty.run.take_if(|slot| stale(slot)).is_some();
        }
        dropped
    }
}
