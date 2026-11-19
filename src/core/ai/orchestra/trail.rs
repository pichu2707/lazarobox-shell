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
/// The most worker lines kept, apart from [`MAX_STEPS`]: a busy worker never
/// pushes a coordinator step out.
pub const WORKER_STEPS: usize = 20;
/// The most worker lines a report shows, besides the last [`SHOWN`] steps of
/// the coordinator's share.
const WORKER_SHOWN: usize = 4;
/// What the stored line of a worker starts with, before its number.
const WORKER_KEY: &str = "worker ";
/// The first line of the stored text: how many steps were dropped.
const CUT_KEY: &str = "cut ";

/// Whose step it is: the coordinator's share (calls, handoffs, separators) or
/// a worker's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Coord,
    Worker(u32),
}

/// How a refusal reads in the trail. The key is the result summary after an
/// optional `denied: ` prefix; the two must be equal. Core cannot import the
/// app or runtime constants, so a drift test there checks each key. Any other
/// reason is stored as plain `refused`: it can carry file or stderr text.
const REASONS: &[(&str, &str)] = &[
    ("the user declined", "declined"),
    ("cancelled", "cancelled"),
    ("the user did not approve it", "not approved"),
    ("that call is not a command", "not a command"),
    ("tools are off", "tools off"),
    ("the worker already ended", "worker ended"),
    (
        "the batch was too large to review: split the change into smaller batches",
        "too large to review",
    ),
    ("not reviewed", "not reviewed"),
];

#[derive(Clone, PartialEq, Eq, Debug)]
struct Step {
    kind: Kind,
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
    worker_cut: usize,
}

impl Step {
    fn plain(text: String) -> Self {
        Self {
            kind: Kind::Coord,
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
            kind: Kind::Coord,
            open: Some(id),
            text,
            outcome: String::new(),
        });
    }

    /// The call `id` ended; a result for no open call is ignored.
    pub fn result(&mut self, id: CallId, status: ToolStatus, detail: &str) {
        let Some(step) =
            (self.steps.iter_mut().rev()).find(|s| s.kind == Kind::Coord && s.open == Some(id))
        else {
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

    /// A tool call of worker `worker`: the tool and the summary lazarobox
    /// built, cleaned like a coordinator call.
    pub fn worker_call(&mut self, worker: u32, id: CallId, name: &str, summary: &str) {
        let call = line(&scrub(&flat(&format!("{name} {summary}"))), CALL_BYTES);
        self.push(Step {
            kind: Kind::Worker(worker),
            open: Some(id),
            text: format!("{WORKER_KEY}{worker}: {call}"),
            outcome: String::new(),
        });
    }

    /// The call `id` of worker `worker` ended. Only the status and a fixed
    /// table are stored: `summary` can carry file, issue or stderr text, so it
    /// is an equality key and is never copied.
    pub fn worker_result(&mut self, worker: u32, id: CallId, status: ToolStatus, summary: &str) {
        let Some(step) = (self.steps.iter_mut().rev())
            .find(|s| s.kind == Kind::Worker(worker) && s.open == Some(id))
        else {
            return;
        };
        step.open = None;
        step.outcome = match status {
            ToolStatus::Ok => "ok".to_owned(),
            ToolStatus::Error => "error".to_owned(),
            ToolStatus::Denied => reason(summary)
                .map_or_else(|| "refused".to_owned(), |why| format!("refused: {why}")),
        };
    }

    fn push(&mut self, step: Step) {
        self.steps.push_back(step);
        self.fit();
    }

    /// Each share keeps its own most recent steps and counts what it drops.
    fn fit(&mut self) {
        for (is_worker, max) in [(false, MAX_STEPS), (true, WORKER_STEPS)] {
            let in_share = |s: &Step| matches!(s.kind, Kind::Worker(_)) == is_worker;
            let mut over = self.steps.iter().filter(|s| in_share(s)).count();
            while over > max {
                if let Some(at) = self.steps.iter().position(in_share) {
                    self.steps.remove(at);
                }
                if is_worker {
                    self.worker_cut += 1;
                } else {
                    self.cut += 1;
                }
                over -= 1;
            }
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
        self.worker_cut += early.worker_cut;
        for step in early.steps.into_iter().rev() {
            self.steps.push_front(step);
        }
        self.fit();
    }

    /// A resumed run starts from `earlier`, the trail of run `from` that it
    /// resumes: its lines, then a separator, then this trail's own steps. The
    /// separator is a plain line, so it never reaches a model (display only).
    pub fn carry(&mut self, mut earlier: Self, from: u64) {
        if !earlier.is_empty() {
            earlier.push(Step::plain(format!("resumed from run {from}")));
        }
        self.absorb(earlier);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// How many steps the run took, the dropped ones included.
    pub fn taken(&self) -> usize {
        self.cut + self.worker_cut + self.steps.len()
    }

    /// How many older coordinator-share steps were dropped at the cap.
    pub fn cut(&self) -> usize {
        self.cut
    }

    /// How many older worker steps were dropped at their cap.
    pub fn worker_cut(&self) -> usize {
        self.worker_cut
    }

    /// What the store keeps: the cut count of both shares, then a line per
    /// step.
    pub fn to_text(&self) -> String {
        let mut out = format!("{CUT_KEY}{}", self.cut + self.worker_cut);
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
            let mut step = Step::plain(line(l, LINE_BYTES));
            step.kind = worker_of(&step.text).map_or(Kind::Coord, Kind::Worker);
            trail.push(step);
        }
        trail
    }

    /// The last `last` steps of the coordinator's share and the last
    /// [`WORKER_SHOWN`] of the workers' for a report, in order, indented, after
    /// a line that counts the ones not shown; empty when there is nothing.
    pub fn shown(&self, last: usize) -> Vec<String> {
        if self.steps.is_empty() {
            return Vec::new();
        }
        let (mut coord, mut work) = (last, WORKER_SHOWN);
        let mut listed = Vec::new();
        for (step, text) in self.steps.iter().zip(self.lines()).rev() {
            let room = if step.kind == Kind::Coord {
                &mut coord
            } else {
                &mut work
            };
            if *room > 0 {
                *room -= 1;
                listed.push(text);
            }
        }
        listed.reverse();
        let hidden = self.taken() - listed.len();
        let mut out = Vec::new();
        if hidden > 0 {
            let noun = if hidden == 1 { "step" } else { "steps" };
            out.push(format!("step trail: {hidden} earlier {noun} not shown"));
        } else {
            out.push("step trail:".to_owned());
        }
        out.extend(listed.into_iter().map(|l| format!("  {l}")));
        out
    }
}

/// The worker a stored line belongs to: `worker <digits>: ` starts it.
fn worker_of(text: &str) -> Option<u32> {
    let (n, _) = text.strip_prefix(WORKER_KEY)?.split_once(": ")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// The label of a refusal in the fixed table, if `summary` is one of its keys.
fn reason(summary: &str) -> Option<&'static str> {
    let key = summary.strip_prefix("denied: ").unwrap_or(summary);
    REASONS.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
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
