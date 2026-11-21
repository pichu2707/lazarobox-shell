//! A command the model typed and the user ran goes on by itself (#819).
//!
//! The model's command sits unsent on a pane's line (`ask` after `y`, or
//! `auto`) and the turn ends with the waiting line. Here the app remembers that
//! line and the anchor of its row. The first key the user sends to the pane
//! decides: a bare Enter with no key, paste or wheel since the typing (and a
//! line that was empty before it) means the shell runs exactly the typed text,
//! so the Enter is followed with the settle machine of `run` mode and its
//! timeout. Any other key marks the line edited; the Enter then follows
//! nothing and says so. When the command settles, the runtime words the report
//! with the output capped and redacted as `run` mode does, and the chat goes on
//! with a fixed note and that text. Every other end of the wait (timeout, a
//! key typed meanwhile, Esc, a new message) is a visible line and no turn.
//! Never in an orchestrating chat, and only where the shell's cwd is known.

use super::*;
use crate::core::ai::{
    chat::engine::Availability,
    tools::{
        report::RunReport,
        settle::{Anchor, End, TIMEOUT},
    },
};

/// Said when the Enter ran something other than the typed line.
pub(super) const CHANGED: &str = "typed command changed: send a message to go on";
/// Said when the Enter cannot be followed for another reason.
const NOT_FOLLOWED: &str = "could not follow the command: send a message to go on";

/// A model's command typed into a pane and not yet run.
pub(super) struct Typed {
    pub(super) chat: PaneId,
    pub(super) turn: u64,
    pub(super) call: CallId,
    pub(super) command: String,
    /// The cursor when it was typed: the row the echo starts on.
    pub(super) anchor: Anchor,
    /// Whether the cursor was visible then (see `Settle::unanchored`).
    pub(super) anchored: bool,
    /// The line was empty and untouched before the typing.
    clean: bool,
    /// A key other than Enter reached the pane since.
    pub(super) edited: bool,
}

impl App {
    /// `command` of call `call` was typed into `target` for chat `chat`: it is
    /// remembered until the user's Enter or another key. Call it before the
    /// line-pending mark is set, which is what tells a clean line.
    pub(super) fn track_typed(
        &mut self,
        chat: PaneId,
        target: PaneId,
        (turn, call): (u64, CallId),
        command: &str,
    ) {
        if self.orchestrating(chat) {
            return;
        }
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return;
        };
        let Some(cwd) = pty.proc_cwd.clone() else {
            return;
        };
        let line = pty.emu.live_cursor_line();
        pty.typed = Some(Typed {
            chat,
            turn,
            call,
            command: command.to_owned(),
            anchor: Anchor {
                line: line.unwrap_or(0),
                col: pty.emu.live_cursor().map_or(0, |(_, col)| col),
                generation: pty.emu.line_generation(),
                cwd,
            },
            anchored: line.is_some(),
            clean: pty.line_pending.is_none() && !pty.user_dirty,
            edited: false,
        });
    }

    /// The user pressed Enter in `target` with `typed` on its line; `exact`
    /// says no key, paste or wheel touched the line since the typing.
    pub(super) fn follow_enter(&mut self, target: PaneId, typed: Typed, exact: bool) {
        let busy =
            (self.panes.get(&target).and_then(PaneState::pty)).is_none_or(|p| p.run.is_some());
        let why = if !(exact && typed.clean && !typed.edited) {
            CHANGED
        } else if busy {
            NOT_FOLLOWED
        } else {
            let number = self.pane_number(target).unwrap_or(0);
            let slot = RunSlot::following(typed, number, (self.clock)());
            if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
                pty.run = Some(slot);
            }
            self.dirty = true;
            return;
        };
        self.notice = Some(why.to_owned());
        self.say_in_chat(typed.chat, why.to_owned());
    }

    /// The command the user ran ended with `report`: one line says how, and
    /// only a settled one resumes the chat.
    pub(super) fn end_follow(
        &mut self,
        chat: PaneId,
        command: String,
        report: RunReport,
    ) -> Vec<Effect> {
        let n = report.number;
        let line = match report.end {
            End::Settled => format!("pane {n}: the command finished, going on"),
            End::TimedOut => format!(
                "pane {n}: the command did not finish in {} s; send a message to go on",
                TIMEOUT.as_secs()
            ),
            End::Unconfirmed => {
                format!("pane {n}: could not confirm the command finished; send a message to go on")
            }
            End::UserTyped => {
                format!("pane {n}: you typed while the command ran; send a message to go on")
            }
            End::FullScreen => {
                format!(
                    "pane {n}: the command opened a full-screen program; send a message to go on"
                )
            }
            End::PaneGone => format!("pane {n} closed; send a message to go on"),
        };
        self.say_in_chat(chat, line);
        if report.end != End::Settled {
            return Vec::new();
        }
        vec![Effect::ResumeNote {
            chat,
            command,
            report: Box::new(report),
        }]
    }

    /// The runtime worded the report of the command the user ran in pane
    /// `number`: the chat goes on with it when it is idle and its composer
    /// holds no draft (a send would clear it).
    pub(super) fn resume_turn(&mut self, chat: PaneId, number: usize, text: &str) -> Vec<Effect> {
        let ready = (self.panes.get(&chat).and_then(PaneState::chat_state))
            .is_some_and(|c| c.phase() == Phase::Idle && c.composer().is_empty());
        let note = format!(
            "lazarobox: the user ran the command you typed in pane {number}, and it finished.\n{text}"
        );
        let availability = self.turn_availability(chat);
        let effects = if ready && availability != Availability::Checking {
            self.send_with(chat, &note, &availability, None)
        } else {
            Vec::new()
        };
        if effects.is_empty() {
            self.say_in_chat(
                chat,
                format!(
                    "pane {number}: the command finished but the chat could not go on; send a message to continue"
                ),
            );
        }
        effects
    }

    /// Chat `chat` sent a message or was cancelled: the command it waits for
    /// is not followed any more, and said. With `typed`, a line not yet run
    /// is forgotten too (the message changed what the model knows).
    pub(super) fn stop_following(&mut self, chat: PaneId, typed: bool) {
        let mut lines = Vec::new();
        for id in self.panes.keys().copied().collect::<Vec<_>>() {
            let number = self.pane_number(id).unwrap_or(0);
            let Some(pty) = self.panes.get_mut(&id).and_then(PaneState::pty_mut) else {
                continue;
            };
            if typed {
                pty.typed.take_if(|t| t.chat == chat);
            }
            if pty.run.take_if(|s| s.resume && s.chat == chat).is_some() {
                lines.push(format!("stopped waiting for the command in pane {number}"));
            }
        }
        for line in lines {
            self.say_in_chat(chat, line);
        }
    }

    fn say_in_chat(&mut self, chat: PaneId, line: String) {
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.push_system_line(line);
            self.dirty = true;
        }
    }
}
