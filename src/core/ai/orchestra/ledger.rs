//! The handoff of a worker, the orchestrator's verdict on it and the run
//! ledger a continuation turn is built from (agent-orchestrator S4, D8,
//! D17). Pure: the run fills the metadata, the orchestrator only writes the
//! judgement.

use super::steps::Step;
use super::worker::WorkerStatus;
use super::{RunId, WorkerId};
use crate::core::ai::config::Task;

/// What an empty `done`, `left` or `decisions` reads.
pub const NONE: &str = "none";

/// The reason of a handoff lazarobox built itself.
pub const NO_HANDOFF: &str = "no handoff from the orchestrator";

/// The longest `reason`, in bytes: one line.
pub const REASON_BYTES: usize = 200;

/// The longest `done`, `left` or `decisions`, in bytes.
pub const FIELD_BYTES: usize = 2 * 1024;

/// The most a rendered ledger takes, in bytes, the goal included. The goal
/// is the user's own message and is never cut, so only a goal of that size
/// alone can pass it.
pub const LEDGER_BYTES: usize = 8 * 1024;

/// The longest summary of a failed worker's row.
const FAILED_BYTES: usize = 200;

/// What a summary is shortened to when the ledger is over its bound.
const SHORT_BYTES: usize = 24;

/// The orchestrator's judgement of a handoff against the run goal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    OnCourse,
    OffCourse,
    Blocked,
    /// Absent or not one of the three values.
    Unknown,
}

impl Verdict {
    /// The spec's id of the verdict.
    pub fn label(self) -> &'static str {
        match self {
            Self::OnCourse => "on_course",
            Self::OffCourse => "off_course",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    /// The verdict a model's text names; anything else is `Unknown`.
    pub fn parse(text: &str) -> Self {
        [Self::OnCourse, Self::OffCourse, Self::Blocked]
            .into_iter()
            .find(|verdict| verdict.label() == text)
            .unwrap_or(Self::Unknown)
    }
}

/// What a handoff is about. Receipts (S7b) join the worker and the step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Worker(WorkerId),
    /// An ODD step (S5, D15).
    Step(Step),
}

/// The part of a handoff the orchestrator writes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Judgement {
    pub verdict: Verdict,
    pub reason: String,
    pub done: String,
    pub left: String,
    pub decisions: String,
}

impl Judgement {
    /// The judgement as it is stored: a one-line reason, blank fields as
    /// `none`, everything capped.
    fn normalised(self) -> Self {
        let first = self.reason.trim().lines().next().unwrap_or("");
        Self {
            verdict: self.verdict,
            reason: field(&plain(first), REASON_BYTES),
            done: field(&self.done, FIELD_BYTES),
            left: field(&self.left, FIELD_BYTES),
            decisions: field(&self.decisions, FIELD_BYTES),
        }
    }

    /// What lazarobox writes when the orchestrator did not: no worker text.
    fn missing(verdict: Verdict) -> Self {
        Self {
            verdict,
            reason: NO_HANDOFF.to_owned(),
            done: NONE.to_owned(),
            left: NONE.to_owned(),
            decisions: NONE.to_owned(),
        }
    }
}

/// One handoff: the run's facts and the orchestrator's judgement.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Handoff {
    pub run: RunId,
    pub unit: Unit,
    pub kind: Task,
    pub engine: String,
    pub model: Option<String>,
    pub status: WorkerStatus,
    pub verdict: Verdict,
    pub reason: String,
    pub done: String,
    pub left: String,
    pub decisions: String,
}

impl Handoff {
    /// The handoff `facts` describes. `blocked` forces the verdict.
    pub(super) fn of(facts: Facts, judgement: Judgement, blocked: bool) -> Self {
        let judgement = judgement.normalised();
        Self {
            run: facts.run,
            unit: facts.unit,
            kind: facts.kind,
            engine: facts.engine,
            model: facts.model,
            status: facts.status,
            verdict: if blocked {
                Verdict::Blocked
            } else {
                judgement.verdict
            },
            reason: judgement.reason,
            done: judgement.done,
            left: judgement.left,
            decisions: judgement.decisions,
        }
    }

    /// The handoff lazarobox builds for a worker whose handoff is missing.
    pub(super) fn fallback(facts: Facts, blocked: bool) -> Self {
        Self::of(facts, Judgement::missing(Verdict::Unknown), blocked)
    }

    /// `engine>model`, or the engine alone.
    pub fn engine_model(&self) -> String {
        match &self.model {
            Some(model) => format!("{}>{model}", self.engine),
            None => self.engine.clone(),
        }
    }

    /// The first line of `done`: what the ledger row says.
    fn summary(&self) -> String {
        plain(self.done.lines().next().unwrap_or(NONE))
    }
}

/// What the run knows about a worker, for its handoff.
pub(super) struct Facts {
    pub run: RunId,
    pub unit: Unit,
    pub kind: Task,
    pub engine: String,
    pub model: Option<String>,
    pub status: WorkerStatus,
}

/// Why a handoff was not stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoffError {
    Unknown,
    NotFinished,
    /// The run is over: its record is closed.
    Ended,
}

/// Why `finish` did not end the run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FinishError {
    Ended,
    /// A failure waits for the user.
    Paused,
    /// These workers are still queued or running.
    Live(Vec<WorkerId>),
    /// These ended with no handoff; a second `finish` builds them.
    Missing(Vec<WorkerId>),
}

/// One worker of the ledger.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub id: WorkerId,
    pub kind: Task,
    pub status: WorkerStatus,
    summary: String,
    /// The worker ended and no handoff was written for it.
    pending: bool,
}

impl Row {
    pub(super) fn new(
        id: WorkerId,
        kind: Task,
        status: WorkerStatus,
        handoff: Option<&Handoff>,
    ) -> Self {
        let terminal = !matches!(status, WorkerStatus::Queued | WorkerStatus::Running);
        let summary = match (handoff, terminal) {
            (Some(handoff), _) => handoff.summary(),
            (None, true) => "handoff pending".to_owned(),
            (None, false) => "in progress".to_owned(),
        };
        Self {
            id,
            kind,
            status,
            summary,
            pending: terminal && handoff.is_none(),
        }
    }

    /// A failure: the row is never hidden, its summary is capped.
    fn failed(&self) -> bool {
        matches!(
            self.status,
            WorkerStatus::Failed | WorkerStatus::TimedOut | WorkerStatus::Refused
        )
    }

    fn line(&self, summary_max: usize) -> String {
        let summary = cut(&self.summary, summary_max);
        let ellipsis = if summary.len() < self.summary.len() {
            "\u{2026}"
        } else {
            ""
        };
        format!(
            "{} {} {}: {summary}{ellipsis}",
            self.id.0,
            self.kind.id(),
            self.status.label()
        )
    }
}

/// The compact record a continuation turn is built from: the goal, as the
/// user wrote it, and a row per worker. Never a worker's transcript.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ledger {
    goal: String,
    rows: Vec<Row>,
    /// The closed ODD steps, one line (S5); empty before the first.
    steps: String,
    /// The same line without the detail, for a ledger at its bound.
    steps_short: String,
}

impl Ledger {
    pub(super) fn new(goal: &str, rows: Vec<Row>) -> Self {
        Self {
            goal: goal.to_owned(),
            rows,
            steps: String::new(),
            steps_short: String::new(),
        }
    }

    /// The ledger with the line of the steps the run closed.
    pub(super) fn with_steps(mut self, line: String, short: String) -> Self {
        self.steps = line;
        self.steps_short = short;
        self
    }

    /// The workers that ended with no handoff yet.
    pub fn pending(&self) -> Vec<WorkerId> {
        self.rows
            .iter()
            .filter(|row| row.pending)
            .map(|row| row.id)
            .collect()
    }

    pub fn render(&self) -> String {
        self.render_within(LEDGER_BYTES)
    }

    /// The ledger within `budget` bytes. The goal and the id and status of
    /// every failure are kept; a failure's summary is capped, the oldest
    /// summaries are shortened first, then the oldest resolved rows collapse
    /// into a count.
    pub(super) fn render_within(&self, budget: usize) -> String {
        // The steps line counts inside the bound: when the rows cannot make
        // room, it shrinks to a count.
        let full = self.fit(budget, &self.steps);
        if full.len() <= budget || self.steps_short.is_empty() {
            return full;
        }
        self.fit(budget, &self.steps_short)
    }

    fn fit(&self, budget: usize, steps: &str) -> String {
        let mut lens: Vec<usize> = self
            .rows
            .iter()
            .map(|row| {
                if row.failed() {
                    FAILED_BYTES
                } else {
                    usize::MAX
                }
            })
            .collect();
        let mut hidden = vec![false; self.rows.len()];
        // Shorten, oldest first.
        for at in 0..self.rows.len() {
            if self.draw(&lens, &hidden, steps).len() <= budget {
                break;
            }
            lens[at] = SHORT_BYTES;
        }
        // Then drop the oldest resolved rows into a count, passing over the
        // failures.
        while self.draw(&lens, &hidden, steps).len() > budget {
            let Some(at) = (0..self.rows.len()).find(|&at| !hidden[at] && !self.rows[at].failed())
            else {
                break;
            };
            hidden[at] = true;
        }
        self.draw(&lens, &hidden, steps)
    }

    fn draw(&self, lens: &[usize], hidden: &[bool], steps: &str) -> String {
        let mut out = format!("Goal: {}\n", self.goal);
        if !steps.is_empty() {
            out.push_str(steps);
            out.push('\n');
        }
        out.push_str("Workers:");
        let count = hidden.iter().filter(|hidden| **hidden).count();
        if count > 0 {
            out.push_str(&format!("\n{count} earlier workers, resolved"));
        }
        let shown = self.rows.iter().zip(lens).zip(hidden);
        for ((row, max), _) in shown.filter(|(_, hidden)| !**hidden) {
            out.push('\n');
            out.push_str(&row.line(*max));
        }
        let pending = self.pending();
        if !pending.is_empty() {
            let ids: Vec<_> = pending.iter().map(|id| id.0.to_string()).collect();
            out.push_str(&format!("\nHandoffs pending: {}", ids.join(", ")));
        }
        out
    }
}

/// `text` without its control characters (ESC, a lone `\r`...): it may go to
/// the transcript and to a model.
fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// `text` trimmed, as `none` when blank, cut to `max` bytes.
fn field(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.is_empty() {
        NONE.to_owned()
    } else {
        cut(text, max).to_owned()
    }
}

/// The longest prefix of `text` of at most `max` bytes, on a char boundary.
fn cut(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
