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
use crate::core::ai::{
    chat::approval::{ApprovalAsk, NOT_ALLOWED, Route},
    tools::{
        report::{cwd_result, system_line},
        settle::{Anchor, End, Probe, Settle},
    },
};

/// Why a call is denied when its target pane is closed or has no process.
pub const PANE_GONE: &str = "the pane is gone";

/// Why nothing was written: the call is over, or has no chat to answer to.
/// Like `PANE_GONE`, it denies the call and opens no prompt.
pub const CALL_OVER: &str = "the call is over";

/// A routed run, from its routing until it settles or its call ends. It lives
/// on the target pane, so closing the pane drops it, and its presence is the
/// pane's `executing` mark.
pub(super) struct RunSlot {
    pub(super) chat: PaneId,
    pub(super) turn: u64,
    pub(super) call: CallId,
    pub(super) command: String,
    stage: Stage,
}

/// Where a run is: waiting for its check, or written and being settled.
enum Stage {
    Checking,
    Running(Running),
}

/// A written run. The settle machine owns the anchor taken at the write.
struct Running {
    settle: Settle,
    /// The real directory a `cd` was checked against.
    cd_target: Option<PathBuf>,
    /// The pane's number when it was written, for a pane that closes.
    number: usize,
}

impl RunSlot {
    /// Pty output reached the pane: the quiet starts again.
    pub(super) fn output(&mut self, now: Instant) {
        if let Stage::Running(run) = &mut self.stage {
            run.settle.output(now);
        }
    }

    /// The user wrote to the pane: the run is no longer the only thing in it.
    pub(super) fn user_key(&mut self) {
        if let Stage::Running(run) = &mut self.stage {
            run.settle.user_key();
        }
    }
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
    /// The runtime read the pane on its tick while a run settles (design
    /// D22): whether the shell owns the terminal and the shell's real
    /// directory, each `None` when the system cannot say.
    Probe {
        foreground: Option<bool>,
        cwd: Option<PathBuf>,
    },
}

impl RunEvent {
    /// The call an answer is about; a probe is about the pane.
    fn call(&self) -> Option<CallId> {
        match self {
            Self::Checked { call, .. } | Self::Refused { call, .. } | Self::NotRun { call, .. } => {
                Some(*call)
            }
            Self::Probe { .. } => None,
        }
    }
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
                stage: Stage::Checking,
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
    /// write is emitted, and stays until the run settles or the call ends.
    pub(super) fn on_run(&mut self, target: PaneId, event: RunEvent) -> Vec<Effect> {
        let Some(call) = event.call() else {
            let RunEvent::Probe { foreground, cwd } = event else {
                return Vec::new();
            };
            return self.on_probe(target, foreground, cwd);
        };
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        // A check's answer is for a run still checking: a written one is
        // never written twice.
        let any_stage = matches!(event, RunEvent::NotRun { .. });
        let waiting = |slot: &RunSlot| any_stage || matches!(slot.stage, Stage::Checking);
        let Some(slot) = pty.run.take_if(|slot| slot.call == call && waiting(slot)) else {
            return Vec::new();
        };
        // The write step found no process: nothing to ask the user about.
        if matches!(
            event,
            RunEvent::NotRun {
                reason: PANE_GONE | CALL_OVER,
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
            RunEvent::Probe { .. } => return Vec::new(),
            RunEvent::Checked { cwd, cd_target, .. } => {
                match self.auto_route(slot.chat, target, &ask, true) {
                    Route::AutoRun(safe) => {
                        return match self.frame_bytes(target, safe.as_str()) {
                            Ok(bytes) => self.auto_run(slot, target, bytes, (cwd, cd_target)),
                            Err(refused) => Self::deny(slot.chat, slot.turn, call, refused.reason),
                        };
                    }
                    Route::Prompt(fallback) => self.fallback_why(slot.chat, fallback),
                    Route::AutoType => None,
                }
            }
        };
        self.open_prompt(slot.chat, (slot.turn, call), Some(target), ask, why)
    }

    /// The run is decided: its one write, the command and one `\r`, goes to
    /// the runtime together with the directory it was checked in. The slot
    /// stays (the pane is executing) until the run settles or the call ends;
    /// it starts settling from where the cursor is now. The fallbacks before
    /// it are forgiven only when the run ends.
    fn auto_run(
        &mut self,
        mut slot: RunSlot,
        target: PaneId,
        mut bytes: Vec<u8>,
        (cwd, cd_target): (PathBuf, Option<PathBuf>),
    ) -> Vec<Effect> {
        bytes.push(b'\r');
        let effect = Effect::AutoRun {
            chat: slot.chat,
            target,
            bytes,
            turn: slot.turn,
            call: slot.call,
            cwd: cwd.clone(),
        };
        let (number, written) = (self.pane_number(target).unwrap_or(0), (self.clock)());
        if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
            // A hidden cursor gives no line to move past: the first probe that
            // sees one anchors the run (see `Settle::unanchored`).
            let line = pty.emu.live_cursor_line();
            let anchor = Anchor {
                line: line.unwrap_or(0),
                col: pty.emu.live_cursor().map_or(0, |(_, col)| col),
                generation: pty.emu.line_generation(),
                cwd,
            };
            let settle = Settle::new(anchor, written);
            slot.stage = Stage::Running(Running {
                settle: if line.is_some() {
                    settle
                } else {
                    settle.unanchored()
                },
                cd_target,
                number,
            });
            pty.run = Some(slot);
        }
        self.dirty = true;
        vec![effect]
    }

    /// Pane ids whose run is written and not settled: the runtime probes them
    /// on its tick, and no other pane.
    pub fn run_probes(&self) -> Vec<PaneId> {
        (self.panes.iter())
            .filter(|(_, state)| {
                (state.pty()).is_some_and(|pty| {
                    (pty.run.as_ref()).is_some_and(|slot| matches!(slot.stage, Stage::Running(_)))
                })
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// The user wrote to pane `id`: a run being settled ends as `UserTyped`
    /// at the next probe.
    pub(super) fn note_typed(&mut self, id: PaneId) {
        if let Some(slot) = (self.panes.get_mut(&id))
            .and_then(PaneState::pty_mut)
            .and_then(|pty| pty.run.as_mut())
        {
            slot.user_key();
        }
    }

    /// One reading of the pane while its run settles. The run ends, with one
    /// report, as soon as the settle machine says so.
    fn on_probe(
        &mut self,
        target: PaneId,
        foreground: Option<bool>,
        cwd: Option<PathBuf>,
    ) -> Vec<Effect> {
        let now = (self.clock)();
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        let Some(Stage::Running(run)) = pty.run.as_mut().map(|slot| &mut slot.stage) else {
            return Vec::new();
        };
        let probe = Probe {
            now,
            foreground,
            cwd: cwd.clone(),
            cursor_line: pty.emu.live_cursor_line(),
            generation: pty.emu.line_generation(),
            alt_screen: pty.emu.mouse_facts().alt_screen,
        };
        match run.settle.step(&probe) {
            Some(end) => self.end_run(target, end, cwd),
            None => Vec::new(),
        }
    }

    /// The run in `target` ended: its slot is freed (the `executing` mark with
    /// it), and the report goes out once. The real cwd comes first; the one
    /// the shell announced (OSC 7) is a labelled fallback. A `cd` leaves its
    /// system line. Output is not read back yet, so it is `None`.
    fn end_run(&mut self, target: PaneId, end: End, real_cwd: Option<PathBuf>) -> Vec<Effect> {
        let number = self.pane_number(target);
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Vec::new();
        };
        let osc7 = pty.emu.cwd().map(Path::to_path_buf);
        let Some(RunSlot {
            chat,
            turn,
            call,
            command,
            stage: Stage::Running(run),
        }) = pty.run.take()
        else {
            return Vec::new();
        };
        let report = RunReport {
            number: number.unwrap_or(run.number),
            end,
            cwd: cwd_result(real_cwd, osc7),
            cd_target: run.cd_target,
            output: None,
        };
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.clear_run_fallbacks();
            if let Some(line) = system_line(&command, &report) {
                chat.push_system_line(line);
            }
        }
        self.dirty = true;
        vec![Effect::RunDone {
            chat,
            turn,
            call,
            report: Box::new(report),
        }]
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
        // A run that was written is parked: it ends with a report, not a denial.
        (slots.into_iter())
            .filter(|slot| !ids.contains(&slot.chat))
            .flat_map(|slot| match slot.stage {
                Stage::Checking => Self::deny(slot.chat, slot.turn, slot.call, PANE_GONE),
                Stage::Running(run) => vec![Effect::RunDone {
                    chat: slot.chat,
                    turn: slot.turn,
                    call: slot.call,
                    report: Box::new(RunReport {
                        number: run.number,
                        end: End::PaneGone,
                        cwd: None,
                        cd_target: None,
                        output: None,
                    }),
                }],
            })
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
