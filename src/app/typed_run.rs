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

/// Why an output of a finished command did not reach the model.
const NOT_SENT: &str =
    "the command finished but its output was not sent; send a message to continue";

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
    /// The pane's number when it was typed: the label the model was told, kept
    /// whatever the layout or the active tab does later.
    number: usize,
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
        let number = self.pane_number(target).unwrap_or(0);
        let pty = self.panes.get_mut(&target).and_then(PaneState::pty_mut);
        let Some(pty) = pty.filter(|pty| pty.proc_cwd.is_some()) else {
            return;
        };
        let line = pty.emu.live_cursor_line();
        pty.typed = Some(Typed {
            chat,
            turn,
            call,
            command: command.to_owned(),
            number,
            anchor: Anchor {
                line: line.unwrap_or(0),
                col: pty.emu.live_cursor().map_or(0, |(_, col)| col),
                generation: pty.emu.line_generation(),
                cwd: pty.proc_cwd.clone().unwrap_or_default(),
            },
            anchored: line.is_some(),
            floor: 0,
            clean: pty.line_pending.is_none() && !pty.user_dirty,
            edited: false,
            stale: false,
        });
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.note_followed(number);
        }
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
            let number = typed.number;
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
            End::PaneGone => closed_line(n),
            end => {
                let why = match end {
                    End::TimedOut => {
                        format!("the command did not finish in {} s", TIMEOUT.as_secs())
                    }
                    End::Unconfirmed => "could not confirm the command finished".to_owned(),
                    End::UserTyped => "you typed while the command ran".to_owned(),
                    _ => "the command opened a full-screen program".to_owned(),
                };
                format!("pane {n}: {why}; send a message to go on")
            }
        };
        self.say_in_chat(chat, line);
        if report.end == End::Settled {
            self.held_resumes.push((chat, n, None));
            return vec![Effect::ResumeNote {
                chat,
                command,
                report: Box::new(report),
            }];
        }
        // The last command of the chat to end may free outputs held for it.
        self.flush_resumes(chat)
    }

    /// The runtime worded the report of the command the user ran in pane
    /// `number`: it waits for the other commands of its chat, and then one
    /// note carries every output. A report nobody waits for any more (a new
    /// message or Esc dropped it) is ignored.
    pub(super) fn resume_turn(&mut self, chat: PaneId, number: usize, text: &str) -> Vec<Effect> {
        let Some(held) =
            (self.held_resumes.iter_mut()).find(|h| h.0 == chat && h.1 == number && h.2.is_none())
        else {
            return Vec::new();
        };
        held.2 = Some(text.to_owned());
        self.flush_resumes(chat)
    }

    /// The outputs of panes `numbers` will not reach the model.
    fn say_not_sent(&mut self, chat: PaneId, numbers: impl Iterator<Item = usize>) {
        for n in numbers {
            self.say_in_chat(chat, format!("pane {n}: {NOT_SENT}"));
        }
    }

    /// Sends the held outputs of `chat` when it follows nothing else: the
    /// chat must be idle with no draft (a send would clear it).
    pub(super) fn flush_resumes(&mut self, chat: PaneId) -> Vec<Effect> {
        // Another command of the chat is followed, or its report is on its way.
        let waiting = (self.panes.values().filter_map(PaneState::pty))
            .any(|pty| pty.run.as_ref().is_some_and(|s| s.resume && s.chat == chat))
            || (self.held_resumes.iter()).any(|(c, _, text)| *c == chat && text.is_none());
        if waiting {
            return Vec::new();
        }
        let mine: Vec<_> = self.held_resumes.extract_if(.., |h| h.0 == chat).collect();
        let numbers: Vec<usize> = mine.iter().map(|h| h.1).collect();
        let note = (mine.iter())
            .filter_map(|(_, n, text)| Some((n, text.as_ref()?)))
            .map(|(n, text)| {
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
        if ready && availability != Availability::Checking && !numbers.is_empty() {
            self.resume_sending = true;
            effects = self.send_with(chat, &note, &availability, None);
            self.resume_sending = false;
        }
        if effects.is_empty() {
            self.say_not_sent(chat, numbers.into_iter());
        } else {
            let shown: Vec<String> = numbers.iter().map(usize::to_string).collect();
            let shown = shown.join(", ");
            self.say_in_chat(chat, format!("going on with the output of pane {shown}"));
        }
        effects
    }

    /// Chat `chat` sent a message or was cancelled: the command it waits for
    /// is not followed any more, and said. With `typed`, a line not yet run
    /// is forgotten too (the message changed what the model knows).
    pub(super) fn stop_following(&mut self, chat: PaneId, typed: bool) {
        for id in self.panes.keys().copied().collect::<Vec<_>>() {
            let Some(pty) = self.panes.get_mut(&id).and_then(PaneState::pty_mut) else {
                continue;
            };
            if let Some(t) = pty.typed.as_mut().filter(|t| typed && t.chat == chat) {
                t.stale = true;
            }
            if let Some(slot) = pty.run.take_if(|s| s.resume && s.chat == chat) {
                let number = slot.number();
                self.say_in_chat(
                    chat,
                    format!("stopped waiting for the command in pane {number}"),
                );
            }
        }
        // What was held for the commands it stopped following is not sent.
        let held: Vec<_> = self.held_resumes.extract_if(.., |h| h.0 == chat).collect();
        self.say_not_sent(chat, held.into_iter().map(|h| h.1));
    }

    pub(super) fn say_in_chat(&mut self, chat: PaneId, line: String) {
        if let Some(chat) = self.panes.get_mut(&chat).and_then(PaneState::chat_mut) {
            chat.push_system_line(line);
            self.dirty = true;
        }
    }
}
