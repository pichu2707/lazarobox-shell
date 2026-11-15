//! Acceptance judges (acceptance-judges S1a): how many judges a staged
//! change gets, the standard they rule against, the judge prompt and the
//! verdict parser. Pure: no process, no file.

use super::changes::Change;
use super::feature::Spec;
use super::ledger::{NO_HANDOFF, NONE, one_line as plain_line};
use super::rdd::{is_mark, one_line};
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

/// What a judge decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

/// Why a judge's text is no verdict. It never counts as a pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoVerdict {
    Empty,
    Unparseable,
    Inconsistent,
    Unseen,
}

/// A criterion as a judge ruled it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Met,
    NotMet,
    Unknown,
}

/// A fault a judge named: where it is and what is wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fault {
    /// The criterion's number: `C<n>`.
    pub criterion: usize,
    /// `<file>:<line>`, `<file>:<start>-<end>` or a function; `None` is
    /// shown as `at: unknown`.
    pub at: Option<String>,
    pub text: String,
}

/// What a judge's text held.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ruling {
    pub verdict: Result<Verdict, NoVerdict>,
    /// One per criterion: its state and evidence, `None` with no line.
    pub criteria: Vec<Option<(State, String)>>,
    pub faults: Vec<Fault>,
    /// Lines that looked like a verdict, criterion or finding but did not fit.
    pub malformed: usize,
}

impl Ruling {
    /// The criteria a judge could not verify: number and reason.
    pub fn unverified(&self) -> Vec<(usize, &str)> {
        let rows = self.criteria.iter().enumerate();
        rows.filter_map(|(at, row)| match row {
            Some((State::Unknown, why)) => Some((at + 1, why.as_str())),
            _ => None,
        })
        .collect()
    }
}

/// The longest evidence, location or fault text, in characters.
pub const EVIDENCE_CHARS: usize = 120;

/// Reads a judge's text (D5). One line each, at the start of a line, the
/// keyword in any case and marks before it ignored, fields split on ` | `:
/// `VERDICT pass|fail`, `CRITERION C<n> met|not met|unknown | <evidence>`
/// and `FINDING C<n> | at: <where> | <what>`. The first line of a criterion
/// counts; a line that does not fit is counted, a finding with no place keeps
/// `at: None`. Anything short of a consistent verdict is no verdict.
pub fn parse_ruling(text: &str, count: usize) -> Ruling {
    let mut ruling = Ruling {
        verdict: Err(NoVerdict::Empty),
        criteria: vec![None; count],
        faults: Vec::new(),
        malformed: 0,
    };
    let (mut said, mut clash, mut unseen) = (None, false, false);
    for line in text.lines().map(|line| line.trim_start_matches(is_mark)) {
        if let Some(rest) = word(line, "verdict") {
            match answer(rest) {
                None => ruling.malformed += 1,
                Some(now) => {
                    clash |= said.is_some_and(|before| before != now);
                    said = said.or(Some(now));
                }
            }
        } else if let Some(rest) = word(line, "criterion") {
            ruling.malformed += usize::from(!criterion(&mut ruling.criteria, rest));
        } else if let Some(rest) = word(line, "finding") {
            match fault(rest, count) {
                Some(found) => ruling.faults.push(found),
                None => ruling.malformed += 1,
            }
        } else if let Some(rest) = word(line, "unseen") {
            unseen |= rest.trim_start_matches(['*', '_', '`']).starts_with(':');
        }
    }
    for (at, row) in ruling.criteria.iter().enumerate() {
        let named = ruling.faults.iter().any(|f| f.criterion == at + 1);
        if let (Some((State::NotMet, why)), false) = (row, named) {
            let (criterion, text) = (at + 1, why.clone());
            ruling.faults.push(Fault {
                criterion,
                at: None,
                text,
            });
        }
    }
    ruling.verdict = if text.trim().is_empty() {
        Err(NoVerdict::Empty)
    } else if unseen {
        Err(NoVerdict::Unseen)
    } else {
        match said {
            None => Err(NoVerdict::Unparseable),
            Some(_) if clash => Err(NoVerdict::Inconsistent),
            Some(said) => consistent(said, &ruling.criteria),
        }
    };
    ruling
}

/// `said` holds only if every criterion has a line, a pass has none not met
/// and a fail has one.
fn consistent(said: Verdict, rows: &[Option<(State, String)>]) -> Result<Verdict, NoVerdict> {
    let failed = rows
        .iter()
        .any(|row| matches!(row, Some((State::NotMet, _))));
    let whole = rows.iter().all(Option::is_some);
    match (said, failed) {
        _ if !whole => Err(NoVerdict::Inconsistent),
        (Verdict::Pass, false) | (Verdict::Fail, true) => Ok(said),
        _ => Err(NoVerdict::Inconsistent),
    }
}

/// What follows `word` at the start of `line`, in any case, if it is the
/// whole word.
fn word<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let head = line.get(..word.len())?;
    let rest = &line[word.len()..];
    (head.eq_ignore_ascii_case(word) && !rest.starts_with(char::is_alphanumeric)).then_some(rest)
}

fn answer(rest: &str) -> Option<Verdict> {
    let rest = rest.trim_start_matches(['*', '_', '`', ':', ' ']);
    let first = rest.split(|c: char| !c.is_alphabetic()).next()?;
    match first.to_ascii_lowercase().as_str() {
        "pass" => Some(Verdict::Pass),
        "fail" => Some(Verdict::Fail),
        _ => None,
    }
}

/// The words of a field head, lowercase and without emphasis marks.
fn head_words(head: &str) -> Vec<String> {
    let clean = head.replace(['*', '_', '`', ':'], " ").to_lowercase();
    clean.split_whitespace().map(str::to_owned).collect()
}

/// The 0-based criterion `C<n>` names, if it is one of `count`.
fn criterion_at(word: &str, count: usize) -> Option<usize> {
    let at = word
        .strip_prefix('c')?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    (at < count).then_some(at)
}

/// Records `<id> <state> | <evidence>`; false when it does not fit.
fn criterion(rows: &mut [Option<(State, String)>], rest: &str) -> bool {
    let (head, evidence) = rest.split_once('|').unwrap_or((rest, ""));
    let words = head_words(head);
    let Some((id, state)) = words.split_first() else {
        return false;
    };
    let state = match state.join(" ").as_str() {
        "met" => State::Met,
        "not met" => State::NotMet,
        "unknown" => State::Unknown,
        _ => return false,
    };
    let Some(row) = criterion_at(id, rows.len()).and_then(|at| rows.get_mut(at)) else {
        return false;
    };
    row.get_or_insert((state, tidy(evidence)));
    true
}

/// `<id> | at: <where> | <what>`; the place may be missing.
fn fault(rest: &str, count: usize) -> Option<Fault> {
    let mut parts = rest.split('|');
    let id = head_words(parts.next()?).into_iter().next()?;
    let criterion = criterion_at(&id, count)? + 1;
    let first = parts.next()?.trim();
    let (at, first) = match first.get(..3) {
        Some(tag) if tag.eq_ignore_ascii_case("at:") => (Some(&first[3..]), None),
        _ => (None, Some(first)),
    };
    let text = tidy(&first.into_iter().chain(parts).collect::<Vec<_>>().join("|"));
    let at = at.map(tidy).filter(|at| !at.is_empty());
    (!text.is_empty()).then_some(Fault {
        criterion,
        at,
        text,
    })
}

/// One clean line of at most [`EVIDENCE_CHARS`] characters.
fn tidy(text: &str) -> String {
    let line = plain_line(text);
    let line = line.trim_matches(['*', '`', ' ']);
    if line.chars().count() <= EVIDENCE_CHARS {
        return line.to_owned();
    }
    let kept: String = line.chars().take(EVIDENCE_CHARS - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

#[cfg(test)]
mod tests;
