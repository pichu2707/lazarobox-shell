//! The commands a worker called that reached the user's ask (W6): the record
//! `collect` hands the coordinator, after the change ledger and inside the
//! untrusted-data wrapper, so a test counts as run only when a result shows it.

use std::collections::VecDeque;
use std::fmt::Write as _;

/// Most commands kept per worker: the newest; the older ones are counted.
pub const MAX_RAN: usize = 4;
/// Most bytes of an output tail kept: its end.
pub const TAIL_MAX: usize = 2 * 1024;
/// Most bytes of a command line kept: its start.
const COMMAND_MAX: usize = 200;
/// The head of the record `collect` shows before the worker's wrapper:
/// lazarobox's own word, which the worker's text cannot pass for.
pub const COMMANDS_HEAD: &str = "lazarobox: commands this worker called, by program; their arguments and output are in its data below:";

/// How a command the user was asked about ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RanEnd {
    /// Not approved (the user's `n`, a cancel, an approval mode): nothing ran.
    Denied,
    /// Approved, but it could not run or the sandbox failed: why, no output.
    Error(String),
    /// It ran: lazarobox's facts line (exit code, time) and the end of the
    /// cleaned, capped output.
    Ended { facts: String, tail: String },
}

/// One command of a worker: its program, its whole line and how it ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ran {
    /// The program, as the allow-list let it through.
    pub program: String,
    /// The program and its arguments, as the worker gave them.
    pub command: String,
    pub end: RanEnd,
}

/// The commands of one worker, newest last, capped at [`MAX_RAN`].
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Commands {
    kept: VecDeque<Ran>,
    dropped: usize,
}

impl Commands {
    /// Records `ran`, cut to its caps; the oldest goes past [`MAX_RAN`]. A
    /// program that is not a plain name is not shown as lazarobox's word.
    pub fn ran(&mut self, mut ran: Ran) {
        let plain = |c: char| c.is_ascii_alphanumeric() || "._+-".contains(c);
        if ran.program.is_empty() || ran.program.len() > 32 || !ran.program.chars().all(plain) {
            ran.program = "(program)".to_owned();
        }
        ran.command = head(&ran.command, COMMAND_MAX);
        match &mut ran.end {
            RanEnd::Ended { tail, .. } => *tail = end(tail, TAIL_MAX),
            RanEnd::Error(why) => *why = head(&why.replace('\n', " "), COMMAND_MAX),
            RanEnd::Denied => {}
        }
        self.kept.push_back(ran);
        if self.kept.len() > MAX_RAN {
            self.kept.pop_front();
            self.dropped += 1;
        }
    }

    /// The kept commands with their number among all the worker called.
    fn numbered(&self) -> impl Iterator<Item = (usize, &Ran)> {
        (self.dropped + 1..).zip(&self.kept)
    }

    /// lazarobox's own record, shown before the worker's wrapper: how each
    /// command ended, by program, never its arguments or output (the
    /// worker's). `None` when no command was asked.
    pub fn attested(&self) -> Option<String> {
        if self.kept.is_empty() {
            return None;
        }
        let mut out = String::from(COMMANDS_HEAD);
        if self.dropped > 0 {
            let _ = write!(out, " ({} earlier commands not shown)", self.dropped);
        }
        for (n, ran) in self.numbered() {
            let ended = match &ran.end {
                RanEnd::Denied => "denied, it did not run",
                RanEnd::Error(why) => why,
                RanEnd::Ended { facts, .. } => facts,
            };
            let _ = write!(out, "\n- {n} {}: {ended}", ran.program);
        }
        Some(out)
    }

    /// The data part, inside the worker's wrapper: each command line and the
    /// end of its output. `None` when no command was asked.
    pub fn render(&self) -> Option<String> {
        if self.kept.is_empty() {
            return None;
        }
        let mut out = String::from("commands run:");
        for (n, ran) in self.numbered() {
            let tail = match &ran.end {
                RanEnd::Ended { tail, .. } if !tail.trim().is_empty() => tail,
                _ => {
                    let _ = write!(out, "\n- {n} {}: no output", ran.command);
                    continue;
                }
            };
            let tail = tail.trim_end().replace('\n', "\n  ");
            let _ = write!(out, "\n- {n} {}:\n  {tail}", ran.command);
        }
        Some(out)
    }
}

/// The first `max` bytes of `text`, cut on a character boundary.
fn head(text: &str, max: usize) -> String {
    let mut at = text.len().min(max);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text[..at].to_owned()
}

/// The last `max` bytes of `text`, cut on a character boundary.
fn end(text: &str, max: usize) -> String {
    let mut at = text.len().saturating_sub(max);
    while !text.is_char_boundary(at) {
        at += 1;
    }
    text[at..].to_owned()
}
