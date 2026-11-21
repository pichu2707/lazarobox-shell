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
/// Said when the line was typed before the user's last message.
const STALE: &str = "typed command is from an earlier turn; send a message to go on";
/// Said when the Enter cannot be followed for another reason (a run of the
/// pane is still in flight).
const NOT_FOLLOWED: &str = "could not follow the command: send a message to go on";

/// Said to the chat when the pane of the command it waits for closes.
pub(super) fn closed_line(number: usize) -> String {
    format!("pane {number} closed; send a message to go on")
}

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
    /// The cursor row at the Enter (see `Settle::with_floor`).
    pub(super) floor: usize,
    /// The line was empty and untouched before the typing.
    clean: bool,
    /// A key other than Enter reached the pane since.
    pub(super) edited: bool,
    /// A new message made the line stale: the Enter says so.
    stale: bool,
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
            floor: 0,
            clean: pty.line_pending.is_none() && !pty.user_dirty,
            edited: false,
            stale: false,
        });
    }

    /// The user pressed Enter in `target` with `typed` on its line; `exact`
    /// says no key, paste or wheel touched the line since the typing.
    pub(super) fn follow_enter(&mut self, target: PaneId, mut typed: Typed, exact: bool) {
        if !self.panes.contains_key(&typed.chat) {
            return;
        }
        let busy =
            (self.panes.get(&target).and_then(PaneState::pty)).is_none_or(|p| p.run.is_some());
        let why = if typed.stale {
            STALE
        } else if !(exact && typed.clean && !typed.edited) {
            CHANGED
        } else if busy {
            NOT_FOLLOWED
        } else {
            let number = self.pane_number(target).unwrap_or(0);
            // The wait is measured from where the Enter finds the cursor; a
            // line that was reflowed since the typing is no longer comparable.
            if let Some(pty) = self.panes.get(&target).and_then(PaneState::pty) {
                let (line, generation) = (pty.emu.live_cursor_line(), pty.emu.line_generation());
                if generation != typed.anchor.generation {
                    typed.anchor.line = line.unwrap_or(0);
                    typed.anchor.generation = generation;
                    typed.anchored = line.is_some();
                }
                typed.floor = line.unwrap_or(0);
            }
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
            End::Settled => format!("pane {n}: the command finished"),
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
            End::PaneGone => closed_line(n),
        };
        self.say_in_chat(chat, line);
        if report.end == End::Settled {
            return vec![Effect::ResumeNote {
                chat,
                command,
                report: Box::new(report),
            }];
        }
        // The last command of the chat to end may free outputs held for it.
        self.flush_resumes(chat)
    }

    /// Whether another command the user ran is still followed for `chat`.
    fn still_following(&self, chat: PaneId) -> bool {
        (self.panes.values().filter_map(PaneState::pty))
            .any(|pty| pty.run.as_ref().is_some_and(|s| s.resume && s.chat == chat))
    }

    /// The runtime worded the report of the command the user ran in pane
    /// `number`: it waits for the other commands of its chat, and then one
    /// note carries every output.
    pub(super) fn resume_turn(&mut self, chat: PaneId, number: usize, text: &str) -> Vec<Effect> {
        self.held_resumes.push((chat, number, text.to_owned()));
        self.flush_resumes(chat)
    }

    /// Sends the held outputs of `chat` when it follows nothing else: the
    /// chat must be idle with no draft (a send would clear it).
    fn flush_resumes(&mut self, chat: PaneId) -> Vec<Effect> {
        if self.still_following(chat) {
            return Vec::new();
        }
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held_resumes)
            .into_iter()
            .partition(|(c, ..)| *c == chat);
        self.held_resumes = rest;
        if mine.is_empty() {
            return Vec::new();
        }
        let numbers: Vec<String> = mine.iter().map(|(_, n, _)| n.to_string()).collect();
        let note = (mine.iter())
            .map(|(_, n, text)| {
                format!(
                    "lazarobox: the user ran the command you typed in pane {n}, and it finished.\n{text}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let ready = (self.panes.get(&chat).and_then(PaneState::chat_state))
            .is_some_and(|c| c.phase() == Phase::Idle && c.composer().is_empty());
        let availability = self.turn_availability(chat);
        let mut effects = Vec::new();
        if ready && availability != Availability::Checking {
            self.say_in_chat(
                chat,
                format!("going on with the output of pane {}", numbers.join(", ")),
            );
            self.resume_sending = true;
            effects = self.send_with(chat, &note, &availability, None);
            self.resume_sending = false;
        }
        if effects.is_empty() {
            for number in numbers {
                self.say_in_chat(
                    chat,
                    format!(
                        "pane {number}: the command finished but its output was not sent; send a message to continue"
                    ),
                );
            }
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
            if let Some(t) = pty.typed.as_mut().filter(|t| typed && t.chat == chat) {
                t.stale = true;
            }
            if pty.run.take_if(|s| s.resume && s.chat == chat).is_some() {
                lines.push(format!("stopped waiting for the command in pane {number}"));
            }
        }
        for line in lines {
            self.say_in_chat(chat, line);
        }
    }

    pub(super) fn say_in_chat(&mut self, chat: PaneId, line: String) {
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.push_system_line(line);
            self.dirty = true;
        }
    }
}
