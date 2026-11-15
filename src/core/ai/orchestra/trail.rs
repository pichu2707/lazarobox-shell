//! The trail of a run's steps (issue #771): each tool call of the coordinator
//! with a one-line argument summary and its outcome, and each handoff with its
//! step and verdict. Chat history does not store tool calls, so the trail is
//! what a stopped run leaves to diagnose it. Capped, one clean line per step,
//! redacted like the transcript. Pure: the store keeps it as text.

use std::collections::VecDeque;

use super::scrub::scrub;
use crate::core::ai::chat::engine::{CallId, ToolStatus};
use crate::core::ai::sanitize::{clean, redact_tokens};

/// The most steps kept; the oldest go first and are counted.
pub const MAX_STEPS: usize = 60;
/// The most bytes of one step's line.
pub const LINE_BYTES: usize = 180;
/// The most bytes of a call's text; its outcome is cut apart from it, so a
/// long call never hides how it ended.
const CALL_BYTES: usize = 110;
/// The most bytes of an outcome's detail.
const DETAIL_BYTES: usize = 50;
/// The most bytes of one text that are read at all (a defence against
/// scanning a huge argument).
const INPUT_BYTES: usize = 1024;
/// The most steps a stop report shows.
pub const SHOWN: usize = 12;
/// The first line of the stored text: how many steps were dropped.
const CUT_KEY: &str = "cut ";

#[derive(Clone, PartialEq, Eq, Debug)]
struct Step {
    /// The call it records, until its result arrives.
    open: Option<CallId>,
    text: String,
    /// How the call ended; empty for a handoff or a stored line.
    outcome: String,
}

/// The steps of one run.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Trail {
    steps: VecDeque<Step>,
    cut: usize,
}

impl Step {
    fn plain(text: String) -> Self {
        Self {
            open: None,
            text,
            outcome: String::new(),
        }
    }
}

impl Trail {
    /// A tool call was announced.
    pub fn call(&mut self, id: CallId, name: &str, summary: &str) {
        let text = line(&scrub(&flat(&format!("{name} {summary}"))), CALL_BYTES);
        self.push(Step {
            open: Some(id),
            text,
            outcome: String::new(),
        });
    }

    /// The call `id` ended; a result for no open call is ignored.
    pub fn result(&mut self, id: CallId, status: ToolStatus, detail: &str) {
        let Some(step) = self.steps.iter_mut().rev().find(|s| s.open == Some(id)) else {
            return;
        };
        step.open = None;
        let word = match status {
            ToolStatus::Ok => "ok",
            ToolStatus::Error => "error",
            ToolStatus::Denied => "refused",
        };
        let detail = line(&scrub(&flat(detail)), DETAIL_BYTES);
        step.outcome = if detail.is_empty() {
            word.to_owned()
        } else {
            format!("{word}: {detail}")
        };
    }

    /// A handoff: `who` is `step explore`, `worker 2`...
    pub fn handoff(&mut self, who: &str, verdict: &str, reason: &str) {
        let reason = scrub(&flat(reason));
        let text = line(&format!("handoff {who} {verdict}: {reason}"), LINE_BYTES);
        self.push(Step::plain(text));
    }

    fn push(&mut self, step: Step) {
        self.steps.push_back(step);
        while self.steps.len() > MAX_STEPS {
            self.steps.pop_front();
            self.cut += 1;
        }
    }

    /// The lines, oldest first; a call with no result says so.
    pub fn lines(&self) -> Vec<String> {
        let render = |s: &Step| match (&s.open, s.outcome.as_str()) {
            (Some(_), _) => format!("{} -> no result", s.text),
            (None, "") => s.text.clone(),
            (None, outcome) => format!("{} -> {outcome}", s.text),
        };
        self.steps.iter().map(render).collect()
    }

    /// Steps taken before this trail's run was known come first.
    pub fn absorb(&mut self, early: Self) {
        self.cut += early.cut;
        for step in early.steps.into_iter().rev() {
            self.steps.push_front(step);
        }
        while self.steps.len() > MAX_STEPS {
            self.steps.pop_front();
            self.cut += 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// How many older steps were dropped at the cap.
    pub fn cut(&self) -> usize {
        self.cut
    }

    /// What the store keeps: the cut count, then a line per step.
    pub fn to_text(&self) -> String {
        let mut out = format!("{CUT_KEY}{}", self.cut);
        for l in self.lines() {
            out.push('\n');
            out.push_str(&l);
        }
        out
    }

    /// The trail a stored text holds. The first line is always the cut
    /// count (a step line is never read as one); text that lacks it, as a
    /// newer build may write, is kept as plain lines.
    pub fn from_text(text: &str) -> Self {
        let mut lines = text.lines().peekable();
        let mut trail = Self::default();
        let header = lines
            .peek()
            .and_then(|l| l.strip_prefix(CUT_KEY))
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(cut) = header {
            trail.cut = cut;
            lines.next();
        }
        for l in lines {
            trail.push(Step::plain(line(l, LINE_BYTES)));
        }
        trail
    }

    /// The last `last` steps for a report, indented, after a line that counts
    /// the ones not shown; empty when there is nothing.
    pub fn shown(&self, last: usize) -> Vec<String> {
        let lines = self.lines();
        if lines.is_empty() {
            return Vec::new();
        }
        let skip = lines.len().saturating_sub(last);
        let hidden = skip + self.cut;
        let mut out = Vec::new();
        if hidden > 0 {
            let noun = if hidden == 1 { "step" } else { "steps" };
            out.push(format!("step trail: {hidden} earlier {noun} not shown"));
        } else {
            out.push("step trail:".to_owned());
        }
        out.extend(lines.into_iter().skip(skip).map(|l| format!("  {l}")));
        out
    }
}

/// The first [`INPUT_BYTES`] of `text`, before any scan of it.
fn cap(text: &str) -> &str {
    let mut end = text.len().min(INPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `text` on one clean line, tokens redacted.
fn flat(text: &str) -> String {
    let text = redact_tokens(&clean(cap(text)));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` as one clean, redacted line of at most `max` bytes.
fn line(text: &str, max: usize) -> String {
    let flat = flat(text);
    if flat.len() <= max {
        return flat;
    }
    let mut end = max.saturating_sub(3);
    while !flat.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &flat[..end])
}

#[cfg(test)]
#[path = "trail_tests.rs"]
mod tests;
