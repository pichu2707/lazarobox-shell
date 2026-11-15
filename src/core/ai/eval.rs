//! How lazarobox measures whether a local model finishes a multi-step coding
//! task (openspec `local-multistep`). Pure: a scenario, the tool exchanges of
//! one run and its final answer go in; a [`RunResult`] comes out. No engine,
//! wire, clock or file type appears here, so the same scenario scores any
//! engine. A developer tool: nothing in this module is shown to the user and
//! nothing in it changes how a chat turn behaves.

use serde_json::Value;

use super::chat::adapters::ToolExchange;

pub mod scenarios;

#[cfg(test)]
mod tests;

/// The kind of work a task stands for; the score table summarizes by it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    WriteAndTest,
    EditExisting,
    ReadOnly,
}

impl Category {
    /// The name the score table prints.
    pub fn key(self) -> &'static str {
        match self {
            Self::WriteAndTest => "write-and-test",
            Self::EditExisting => "edit-existing",
            Self::ReadOnly => "read-only",
        }
    }
}

/// One step a run must take: a tool and a test of its arguments.
#[derive(Clone, Copy)]
pub struct Expect {
    pub label: &'static str,
    pub tool: &'static str,
    pub args: fn(&Value) -> bool,
}

/// A string the user gave in the request that a step must carry verbatim.
#[derive(Clone, Copy, Debug)]
pub struct Literal {
    /// The label of the step whose arguments must hold it.
    pub step: &'static str,
    pub text: &'static str,
}

/// A task as plain data.
#[derive(Clone, Copy)]
pub struct Scenario {
    pub id: &'static str,
    pub category: Category,
    /// What the user types.
    pub request: &'static str,
    /// The generated project: relative path and text of each file.
    pub seed: &'static [(&'static str, &'static str)],
    pub steps: &'static [Expect],
    /// Pairs of step labels: the first must precede the second.
    pub order: &'static [(&'static str, &'static str)],
    pub literals: &'static [Literal],
    /// Tools no run of this task may call.
    pub forbid: &'static [&'static str],
    /// A clarifying question is a correct answer.
    pub ambiguous: bool,
}

/// What one run did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunResult {
    pub completed: Vec<&'static str>,
    pub missing: Vec<&'static str>,
    pub out_of_order: Vec<(&'static str, &'static str)>,
    pub ignored: Vec<&'static str>,
    pub asked_back: bool,
    /// The forbidden tools the run called.
    pub forbidden: Vec<String>,
    ambiguous: bool,
}

impl RunResult {
    /// Whether the run did the task. An ambiguous task is also done by asking.
    pub fn passed(&self) -> bool {
        if self.ambiguous && self.asked_back {
            return self.forbidden.is_empty();
        }
        !self.asked_back
            && self.missing.is_empty()
            && self.out_of_order.is_empty()
            && self.ignored.is_empty()
            && self.forbidden.is_empty()
    }
}

/// The start of every text the host or the loop returns for a call that did
/// not run (pinned by a test against the real constants).
const REFUSALS: [&str; 17] = [
    "denied: ",
    "the tool failed",
    "result dropped:",
    "too many tool calls in one reply",
    "path not available",
    "unknown tool",
    "arguments must be an object",
    "missing argument",
    "argument must be a string",
    "unexpected argument",
    "argument too long",
    "pane must be",
    "scrollback must be",
    "kind must be",
    "title and description",
    "title must be",
    "description must be",
];

/// Whether a tool result says the call was refused, denied or failed. A
/// non-zero exit of a command is a normal result: the tool ran.
pub fn refused(result: &str) -> bool {
    REFUSALS.iter().any(|prefix| result.starts_with(prefix))
}

/// Whether the answer ends in a question or asks the user for an input.
pub fn asked_back(answer: &str) -> bool {
    const ASKS: [&str; 4] = [
        "please provide",
        "please specify",
        "could you provide",
        "can you provide",
    ];
    let lower = answer.trim().to_lowercase();
    lower.ends_with('?') || ASKS.iter().any(|ask| lower.contains(ask))
}

/// Whether some string of `arguments` holds `text`.
fn carries(arguments: &Value, text: &str) -> bool {
    let quoted = Value::from(text).to_string();
    arguments.to_string().contains(&quoted[1..quoted.len() - 1])
}

/// Scores one run: the tool exchanges of its turn and its final answer.
pub fn score(scenario: &Scenario, exchanges: &[ToolExchange], answer: &str) -> RunResult {
    let first = |step: &Expect| {
        exchanges
            .iter()
            .position(|e| e.name == step.tool && (step.args)(&e.arguments) && !refused(&e.result))
    };
    let at: Vec<_> = scenario.steps.iter().map(|s| (s.label, first(s))).collect();
    let index = |label: &str| at.iter().find(|(l, _)| *l == label).and_then(|(_, i)| *i);
    let completed = at.iter().filter(|(_, i)| i.is_some()).map(|(l, _)| *l);
    let ignored = scenario.literals.iter().filter(|lit| {
        let step = scenario.steps.iter().find(|s| s.label == lit.step);
        let done =
            |e: &&ToolExchange| step.is_some_and(|s| e.name == s.tool && !refused(&e.result));
        index(lit.step).is_some()
            && !exchanges
                .iter()
                .filter(done)
                .any(|e| carries(&e.arguments, lit.text))
    });
    RunResult {
        completed: completed.clone().collect(),
        missing: at
            .iter()
            .filter(|(_, i)| i.is_none())
            .map(|(l, _)| *l)
            .collect(),
        out_of_order: (scenario.order.iter())
            .filter(|(a, b)| matches!((index(a), index(b)), (Some(a), Some(b)) if b < a))
            .copied()
            .collect(),
        ignored: ignored.map(|lit| lit.text).collect(),
        asked_back: asked_back(answer),
        forbidden: (exchanges.iter())
            .filter(|e| scenario.forbid.contains(&e.name.as_str()))
            .map(|e| e.name.clone())
            .collect(),
        ambiguous: scenario.ambiguous,
    }
}
