//! Acceptance judges (acceptance-judges S1a): how many judges a staged
//! change gets, the standard they rule against, the judge prompt and the
//! verdict parser. Pure: no process, no file.

use super::changes::Change;
use super::feature::Spec;
use super::ledger::{NO_HANDOFF, NONE};
use super::rdd::one_line;
use super::review::Tier;
use super::steps::{Size, Step, Steps};

/// Staged text at or above this many bytes counts as a big change.
pub const BIG_BYTES: u64 = 16 * 1024;

/// How many judges a change gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JudgeLevel {
    None,
    One,
    Two,
}

/// The judge level (D1): none for a read-only goal; two for a Large goal,
/// more than one file, a tier of medium or more, or a big staged text; one
/// otherwise. An unknown size reads as small.
pub fn level(read_only: bool, size: Option<Size>, changes: &[Change], tier: Tier) -> JudgeLevel {
    if read_only {
        return JudgeLevel::None;
    }
    if size == Some(Size::Large)
        || changes.len() > 1
        || tier >= Tier::Medium
        || staged_bytes(changes) >= BIG_BYTES
    {
        return JudgeLevel::Two;
    }
    JudgeLevel::One
}

fn staged_bytes(changes: &[Change]) -> u64 {
    changes.iter().filter_map(|change| change.size).sum()
}

/// The transcript line that says the level and its inputs; none for `None`.
pub fn level_line(
    level: JudgeLevel,
    size: Option<Size>,
    changes: &[Change],
    tier: Tier,
) -> Option<String> {
    let judges = match level {
        JudgeLevel::None => return None,
        JudgeLevel::One => "one judge",
        JudgeLevel::Two => "two judges",
    };
    let size = size.map_or("unknown", Size::label);
    Some(format!(
        "judge: {judges} \u{b7} {} file(s), {} B \u{b7} tier {} \u{b7} size {size}",
        changes.len(),
        staged_bytes(changes),
        tier.as_str(),
    ))
}

/// The most criteria a standard holds.
pub const MAX_CRITERIA: usize = 12;

/// The most of each norm file a judge reads.
pub const NORM_BYTES: usize = 8 * 1024;

/// One thing the change is ruled against, with a stable id (`C1`, `C2`...).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Criterion {
    pub id: String,
    pub text: String,
}

/// The start of a repo norm file, as a judge reads it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Norm {
    pub name: String,
    pub text: String,
    pub cut_from: Option<u64>,
}

/// What a judge rules against.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standard {
    pub criteria: Vec<Criterion>,
    pub norms: Vec<Norm>,
    pub thin: bool,
}

/// The repo files a judge reads as norms, in order.
pub const NORM_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

const NORMS_CRITERION: &str = "the change follows the repo norms below";

/// The reason, done and left of the closed `resolve`, `classify` and `track`
/// handoffs that say something: the run's own criteria for a small goal.
pub fn handoff_notes(steps: &Steps) -> Vec<String> {
    let mut notes = Vec::new();
    for step in [Step::Resolve, Step::Classify, Step::Track] {
        let Some(handoff) = steps.handoff(step) else {
            continue;
        };
        for (what, text) in [
            ("reason", &handoff.reason),
            ("done", &handoff.done),
            ("left", &handoff.left),
        ] {
            if !matches!(text.as_str(), NONE | NO_HANDOFF) && !text.trim().is_empty() {
                notes.push(format!("{} {what}: {text}", step.label()));
            }
        }
    }
    notes
}

/// The standard (D2): the goal, the handoff `notes`, the `specs` of a Large
/// goal's feature document and, when a norm was read, one criterion for the
/// norms. Numbered `C1..`, at most [`MAX_CRITERIA`], each one clean line.
/// `thin` when nothing but the goal was given.
pub fn standard(goal: &str, notes: &[String], specs: &[Spec], norms: Vec<Norm>) -> Standard {
    let spec_lines = specs.iter().map(|s| format!("{}: {}", s.id, s.text));
    let mut texts: Vec<String> = std::iter::once(goal.to_owned())
        .chain(notes.iter().cloned())
        .chain(spec_lines)
        .map(|text| one_line(&text))
        .filter(|text| !text.is_empty())
        .collect();
    let thin = texts.len() <= 1;
    texts.truncate(MAX_CRITERIA - usize::from(!norms.is_empty()));
    if !norms.is_empty() {
        texts.push(NORMS_CRITERION.to_owned());
    }
    let criteria = (1..)
        .zip(texts)
        .map(|(n, text)| Criterion {
            id: format!("C{n}"),
            text,
        })
        .collect();
    Standard {
        criteria,
        norms,
        thin,
    }
}

/// A norm file's start (D3): the first [`NORM_BYTES`] of `bytes`, cut on a
/// character boundary, read lossily, without a line that only imports a norm
/// file. `total` is the file's size, so a head read with `take` still says it
/// was cut. `None` when nothing is left.
pub fn norm(name: &str, bytes: &[u8], total: u64) -> Option<Norm> {
    let head = &bytes[..bytes.len().min(NORM_BYTES)];
    let head = match std::str::from_utf8(head) {
        Err(cut) if cut.error_len().is_none() => &head[..cut.valid_up_to()],
        _ => head,
    };
    let text: String = String::from_utf8_lossy(head)
        .split_inclusive('\n')
        .filter(|line| !is_import(line))
        .collect();
    (!text.trim().is_empty()).then(|| Norm {
        name: name.to_owned(),
        text,
        cut_from: (total > head.len() as u64).then_some(total.max(bytes.len() as u64)),
    })
}

/// A line that is only `@AGENTS.md` or `@CLAUDE.md`.
fn is_import(line: &str) -> bool {
    let file = line.trim().strip_prefix('@');
    file.is_some_and(|file| NORM_FILES.contains(&file))
}

#[cfg(test)]
mod tests;
