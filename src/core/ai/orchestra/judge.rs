//! Acceptance judges (acceptance-judges S1a): how many judges a staged
//! change gets, the standard they rule against, the judge prompt and the
//! verdict parser. Pure: no process, no file.

use super::changes::Change;
use super::changes::StagedDiff;
use super::feature::Spec;
use super::ledger::{NO_HANDOFF, NONE, one_line as plain_line};
use super::rdd::{Prompt, candidate, is_mark, one_line};
use super::review::Tier;
use super::steps::{Size, Step, Steps};
use crate::core::ai::{memory::redact::redact, sanitize::clean};

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

/// The bytes read past [`NORM_BYTES`] so a secret on the cut is masked whole.
pub const NORM_SLACK: usize = 256;

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
    /// Secrets masked in the text before it went anywhere.
    pub redacted: usize,
}

/// What a judge rules against.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standard {
    pub criteria: Vec<Criterion>,
    pub norms: Vec<Norm>,
    pub thin: bool,
    /// Criteria past [`MAX_CRITERIA`] that were left out.
    pub dropped: usize,
}

/// The repo files a judge reads as norms, in order.
pub const NORM_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The criterion of a run that gave no goal text: a standard is never empty.
const FALLBACK_CRITERION: &str = "the change does what the run asked";

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
    if texts.is_empty() {
        texts.push(FALLBACK_CRITERION.to_owned());
    }
    let thin = texts.len() <= 1;
    let room = MAX_CRITERIA - usize::from(!norms.is_empty());
    let dropped = texts.len().saturating_sub(room);
    texts.truncate(room);
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
        dropped,
    }
}

/// A norm file's start (D3): the first [`NORM_BYTES`] of `bytes`, cut on a
/// character boundary, read lossily, without a line that only imports a norm
/// file. `total` is the file's size, so a head read with `take` still says it
/// was cut. `None` when nothing is left.
pub fn norm(name: &str, bytes: &[u8], total: u64) -> Option<Norm> {
    let kept = bytes.len().min(NORM_BYTES);
    // A window past the cut is masked first, so a secret that starts before
    // byte 8 KiB is never left half shown by the cut.
    let window = &bytes[..bytes.len().min(NORM_BYTES + NORM_SLACK)];
    let window = match std::str::from_utf8(window) {
        Err(cut) if cut.error_len().is_none() => &window[..cut.valid_up_to()],
        _ => window,
    };
    let mut masked = redact(&clean(&String::from_utf8_lossy(window)));
    let mut end = masked.text.len().min(NORM_BYTES);
    while !masked.text.is_char_boundary(end) {
        end -= 1;
    }
    masked.text.truncate(end);
    let text: String = masked
        .text
        .split_inclusive('\n')
        .filter(|line| !is_import(line))
        .collect();
    (!text.trim().is_empty()).then(|| Norm {
        name: name.to_owned(),
        text,
        cut_from: (total > kept as u64).then_some(total.max(bytes.len() as u64)),
        redacted: masked.hits,
    })
}

/// Said once per run for a norm file that had secrets masked.
pub fn norms_redacted_line(norm: &Norm) -> Option<String> {
    (norm.redacted > 0).then(|| {
        let name = plain_line(&norm.name);
        format!("judge: {name} had {} secret(s) redacted", norm.redacted)
    })
}

/// Said once per run for a norm file that was cut (acceptance-judges J7):
/// `None` when the whole file fits.
pub fn norms_cut_line(norm: &Norm) -> Option<String> {
    let total = norm.cut_from?;
    Some(format!(
        "judge: norms cut at {} KB \u{b7} {} is {} KB",
        NORM_BYTES / 1024,
        plain_line(&norm.name),
        total.div_ceil(1024)
    ))
}

/// Said once per run for a norm file that is there but holds nothing of its
/// own (empty, or only an import of the other file).
pub fn norms_empty_line(name: &str) -> String {
    format!("judge: {} has no norms of its own", plain_line(name))
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
    /// The judge's engine could not start it.
    Refused,
    /// The judge ended without a text (failed, timed out or cancelled).
    Failed,
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
    /// A judge that gave no verdict, for `why`, over `count` criteria.
    pub fn none(why: NoVerdict, count: usize) -> Self {
        Self {
            verdict: Err(why),
            criteria: vec![None; count],
            faults: Vec::new(),
            malformed: 0,
        }
    }

    /// How the run step trail records this ruling: the verdict word and a
    /// reason built from counts and fixed words only (never judge text).
    pub fn step(&self) -> (&'static str, String) {
        let count = |want: State| {
            let rows = self.criteria.iter().flatten();
            rows.filter(|(state, _)| *state == want).count()
        };
        match self.verdict {
            Ok(Verdict::Pass) => ("pass", format!("{} criteria met", count(State::Met))),
            Ok(Verdict::Fail) => (
                "fail",
                format!(
                    "{} of {} criteria not met",
                    count(State::NotMet),
                    self.criteria.len()
                ),
            ),
            Err(why) => ("no verdict", why.word().to_owned()),
        }
    }

    /// The criteria a judge could not verify: number and reason.
    pub fn unverified(&self) -> Vec<(usize, &str)> {
        let rows = self.criteria.iter().enumerate();
        rows.filter_map(|(at, row)| match row {
            Some((State::Unknown, why)) => Some((at + 1, why.as_str())),
            _ => None,
        })
        .collect()
    }

    /// A pass that settles little: it carries a fault, or it verified no
    /// criterion at all. Kept, but never read as clean (J14, S1a verify).
    pub fn hollow(&self) -> bool {
        let mut rows = self.criteria.iter().flatten();
        self.verdict == Ok(Verdict::Pass)
            && (!self.faults.is_empty() || rows.all(|(state, _)| *state == State::Unknown))
    }
}

impl NoVerdict {
    pub fn word(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Unparseable => "unparseable",
            Self::Inconsistent => "inconsistent",
            Self::Unseen => "unseen",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }
}

/// How the judges of one change ended together (D7).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Agreement {
    /// Every judge passed. `unverified` is the union of the criteria they
    /// could not verify (number, reason); `clean` is false when a pass
    /// settles little (see [`Ruling::hollow`]).
    Pass {
        unverified: Vec<(usize, String)>,
        clean: bool,
    },
    /// Every judge failed: the union of their faults, every place kept.
    Fail(Vec<Fault>),
    /// A pass against a fail, or a judge with no verdict: nothing is
    /// corrected. `missing` says who and what.
    Disagree { missing: String },
}

/// The judges' agreement over one batch: one judge's verdict decides, two
/// must say the same, and anything else is a disagreement.
pub fn agree(rulings: &[Ruling]) -> Agreement {
    let all = |want: Verdict| rulings.iter().all(|r| r.verdict == Ok(want));
    if !rulings.is_empty() && all(Verdict::Pass) {
        let mut unverified: Vec<(usize, String)> = Vec::new();
        for (at, why) in rulings.iter().flat_map(Ruling::unverified) {
            if !unverified.iter().any(|(known, _)| *known == at) {
                unverified.push((at, why.to_owned()));
            }
        }
        let clean = !rulings.iter().any(Ruling::hollow);
        return Agreement::Pass { unverified, clean };
    }
    if !rulings.is_empty() && all(Verdict::Fail) {
        let mut faults: Vec<Fault> = Vec::new();
        for fault in rulings.iter().flat_map(|r| &r.faults) {
            if !faults.contains(fault) {
                faults.push(fault.clone());
            }
        }
        return Agreement::Fail(faults);
    }
    let mut said = Vec::new();
    for (at, ruling) in rulings.iter().enumerate() {
        match ruling.verdict {
            Err(why) => said.push(format!("judge {} gave no verdict ({})", at + 1, why.word())),
            Ok(Verdict::Fail) => {
                let ids: Vec<String> = ruling
                    .faults
                    .iter()
                    .map(|f| format!("C{}", f.criterion))
                    .collect();
                said.push(format!("judge {} fails {}", at + 1, ids.join(", ")));
            }
            Ok(Verdict::Pass) => {}
        }
    }
    let missing = if said.is_empty() {
        "no judge ruled".to_owned()
    } else {
        said.join("; ")
    };
    Agreement::Disagree { missing }
}

/// The most criterion lines one judge shows; the receipt names the rest.
const SHOWN_CRITERIA: usize = 8;

/// The transcript lines of judge `at` of `of` (spec: every verdict is a
/// visible line): its verdict, and for a fail one line per fault with its
/// place. `note` is the runtime's text for a judge that was refused.
pub fn judge_lines(at: usize, of: usize, ruling: &Ruling, note: Option<&str>) -> Vec<String> {
    let who = format!("judge {at}/{of}");
    let rows = ruling.criteria.iter().flatten();
    let met = rows.filter(|(state, _)| *state == State::Met).count();
    match ruling.verdict {
        Err(why) => {
            let note = note.map_or_else(String::new, |text| format!(" ({})", plain_line(text)));
            vec![format!("{who}: no verdict \u{b7} {}{note}", why.word())]
        }
        Ok(Verdict::Pass) => {
            let head = match (ruling.hollow(), ruling.faults.is_empty()) {
                (false, _) => "pass",
                (true, true) => "pass, nothing verified",
                (true, false) => "pass, but it contradicts itself",
            };
            let unverified = ruling.unverified();
            let detail = if unverified.is_empty() {
                format!("{met} criteria met")
            } else {
                format!("{met} met, {}", unverified_text(&unverified))
            };
            vec![format!("{who}: {head} \u{b7} {detail}")]
        }
        Ok(Verdict::Fail) => {
            let rows = ruling.criteria.iter().flatten();
            let failed = rows.filter(|(state, _)| *state == State::NotMet).count();
            let mut lines = vec![format!(
                "{who}: fail \u{b7} {failed} of {} criteria not met",
                ruling.criteria.len()
            )];
            for fault in ruling.faults.iter().take(SHOWN_CRITERIA) {
                let place = fault.at.as_deref().unwrap_or("unknown");
                lines.push(format!(
                    "{who} \u{b7} C{} not met \u{b7} at: {place} \u{b7} {}",
                    fault.criterion, fault.text
                ));
            }
            if let Some(more) = ruling
                .faults
                .len()
                .checked_sub(SHOWN_CRITERIA)
                .filter(|n| *n > 0)
            {
                lines.push(format!("{who}: +{more} more in the receipt"));
            }
            lines
        }
    }
}

/// The ruling of each judge, in order: `refusals` holds why a judge did not
/// start (`None`: it runs), `texts` the answers of the judges that run, in
/// their order. A refused judge takes no text.
pub fn rulings(
    refusals: &[Option<String>],
    texts: Vec<Option<String>>,
    count: usize,
) -> Vec<(Ruling, Option<String>)> {
    let mut texts = texts.into_iter();
    let each = |why: &Option<String>| match why {
        Some(why) => (Ruling::none(NoVerdict::Refused, count), Some(why.clone())),
        None => match texts.next().flatten() {
            Some(text) => (parse_ruling(&text, count), None),
            None => (Ruling::none(NoVerdict::Failed, count), None),
        },
    };
    refusals.iter().map(each).collect()
}

/// `<u> unverified (C3: tests not seen, ...)`.
fn unverified_text<T: AsRef<str>>(unverified: &[(usize, T)]) -> String {
    let each: Vec<String> = unverified
        .iter()
        .map(|(at, why)| format!("C{at}: {}", why.as_ref()))
        .collect();
    format!("{} unverified ({})", unverified.len(), each.join(", "))
}

/// The line that says how two judges ended together.
pub fn agreement_line(agreement: &Agreement, rulings: &[Ruling]) -> String {
    match agreement {
        Agreement::Pass { unverified, clean } => {
            let mut line = "judges agree: pass".to_owned();
            if !unverified.is_empty() {
                line.push_str(&format!(" \u{b7} {}", unverified_text(unverified)));
            }
            if !clean {
                line.push_str(" \u{b7} not clean, see the judge lines");
            }
            line
        }
        Agreement::Fail(faults) => {
            let mut ids: Vec<usize> = faults.iter().map(|f| f.criterion).collect();
            ids.sort_unstable();
            ids.dedup();
            format!("judges agree: fail \u{b7} {} criteria not met", ids.len())
        }
        Agreement::Disagree { missing } => {
            let said: Vec<String> = rulings
                .iter()
                .enumerate()
                .map(|(at, ruling)| {
                    let word = match ruling.verdict {
                        Ok(Verdict::Pass) => "pass",
                        Ok(Verdict::Fail) => "fail",
                        Err(_) => "no verdict",
                    };
                    format!("judge {} {word}", at + 1)
                })
                .collect();
            format!(
                "judges disagree \u{b7} {} \u{b7} missing: {missing}",
                said.join(", ")
            )
        }
    }
}

/// Said before the verdicts when the standard holds only the goal.
pub fn thin_line() -> String {
    "judge: criteria are thin \u{b7} judged against the goal and the resolve/classify reasons"
        .to_owned()
}

/// Said when criteria past the cap were left out.
pub fn dropped_line(count: usize) -> String {
    format!("judge: {count} criteria not judged (cap {MAX_CRITERIA})")
}

/// Said when both judges run on one model.
pub fn independence_line(model: &str) -> String {
    format!(
        "judge: judges are not independent \u{b7} both on {}",
        plain_line(model)
    )
}

/// One time the judges ruled over a change: the first, or after a correction.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Round {
    pub agreement: Agreement,
    pub rulings: Vec<Ruling>,
}

/// What the judges of a change were set up with, for the receipt: the
/// runtime fills it in; nothing here is a model's text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Judging {
    /// The user's judge consent (S2a): declined means no judge ran.
    pub granted: bool,
    /// The level line, with its inputs.
    pub level: String,
    /// `engine > model` of each judge, in order.
    pub models: Vec<String>,
    pub standard: Standard,
    /// The judges ran on different models (false: both on one model).
    pub independent: bool,
}

impl Judging {
    /// What the transcript says before any judge speaks: the level line, the
    /// thin or dropped criteria and two judges on one model.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = vec![self.level.clone()];
        if self.standard.thin {
            notes.push(thin_line());
        }
        if self.standard.dropped > 0 {
            notes.push(dropped_line(self.standard.dropped));
        }
        if !self.independent {
            notes.extend(self.models.first().map(|model| independence_line(model)));
        }
        notes
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
    // No criteria means nothing was ruled: neither verdict can stand.
    let whole = !rows.is_empty() && rows.iter().all(Option::is_some);
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

/// Where a correction changed a file's staged text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Span {
    /// Lines `start..=end` of the new text, from 1.
    Lines(usize, usize),
    /// The file is new in the batch.
    Whole,
    /// It changed, but no range of the new text names it (lines removed).
    None,
}

/// What a correction changed, from the staged batch before and after it (D12):
/// one `(path, span)` per file whose text differs, in the order of `after`,
/// then the files it dropped. Never the worker's own account.
pub fn changed(before: &StagedDiff, after: &StagedDiff) -> Vec<(String, Span)> {
    let mut out = Vec::new();
    for file in &after.files {
        let Some(old) = before.files.iter().find(|old| old.path == file.path) else {
            out.push((file.path.clone(), Span::Whole));
            continue;
        };
        if old.text == file.text {
            continue;
        }
        let (old, new): (Vec<&str>, Vec<&str>) =
            (old.text.lines().collect(), file.text.lines().collect());
        let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - head;
        let tail = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        let end = new.len() - tail;
        let span = if end > head {
            Span::Lines(head + 1, end)
        } else {
            Span::None
        };
        out.push((file.path.clone(), span));
    }
    for old in &before.files {
        if !after.files.iter().any(|file| file.path == old.path) {
            out.push((old.path.clone(), Span::None));
        }
    }
    out
}

/// `correction <r>/<of> done · changes: ...` (D12), from [`changed`] only.
pub fn changes_line(r: u32, of: u32, changes: &[(String, Span)]) -> String {
    let each: Vec<String> = changes
        .iter()
        .map(|(path, span)| match span {
            Span::Lines(start, end) => format!("{path} {start}-{end}"),
            Span::Whole | Span::None => path.clone(),
        })
        .collect();
    let what = if each.is_empty() {
        "none".to_owned()
    } else {
        each.join(", ")
    };
    format!("correction {r}/{of} done \u{b7} changes: {what}")
}

/// The text of the run's judge consent (D10): the level rule and the models
/// that would judge, as one clean line.
pub fn consent_text(models: &[String]) -> String {
    let rule = format!(
        "one judge for a small change, two for several files, a large goal, {} KB or more, \
         or a medium or high tier",
        BIG_BYTES / 1024
    );
    let models: Vec<String> = models.iter().map(|model| plain_line(model)).collect();
    if models.is_empty() {
        return plain_line(&rule);
    }
    plain_line(&format!("{} \u{b7} {rule}", models.join(", ")))
}

/// The prompt of a judge: the criteria and the norms lead the staged change
/// and its diff, all wrapped as untrusted data together, so a criterion
/// cannot gain the rank of an instruction and a cut diff stays a gap.
pub fn judge_prompt(
    nonce: [u8; 8],
    standard: &Standard,
    changes: &[Change],
    diff: &StagedDiff,
) -> Prompt {
    let mut lead = String::from("criteria:");
    for criterion in &standard.criteria {
        lead.push_str(&format!("\n{}: {}", criterion.id, criterion.text));
    }
    for norm in &standard.norms {
        let cut = norm.cut_from.map_or_else(String::new, |total| {
            format!(" (cut at 8 KB, file is {} KB)", total.div_ceil(1024))
        });
        lead.push_str(&format!(
            "\n\nnorms from {}{cut}:\n{}",
            norm.name, norm.text
        ));
    }
    lead.push_str("\n\n");
    let (body, gaps, cut_bytes) = candidate(nonce, &lead, changes, diff);
    Prompt {
        text: format!(
            "Acceptance judge: rule the staged change against these {} criteria.\n\
             The criteria, the norms and the staged change, as data:\n{body}",
            standard.criteria.len(),
        ),
        gaps,
        cut_bytes,
    }
}

#[cfg(test)]
mod tests;
