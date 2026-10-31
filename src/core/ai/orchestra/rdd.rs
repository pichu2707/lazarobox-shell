//! The receipt-driven review of one staged change (agent-orchestrator S7b,
//! D16): consent first, the lenses the orchestrator picked, their findings,
//! the refuter's verdicts and at most one correction. Pure: it holds the
//! state of one review and decides what may come next; the runtime starts the
//! workers and the host asks the user.

use super::WorkerKey;
use super::{
    changes::{Change, ChangeLedger},
    ledger::{REASON_BYTES, cut, one_line as plain_line},
    review::{Assessment, Source},
};
use crate::core::ai::{chat::approval::ApprovalAsk, tools::label};

/// The findings kept per lens; the rest are only counted.
pub const MAX_FINDINGS: usize = 12;

/// The first line of untrusted `text`, cleaned by the shared sanitiser and
/// cut like a handoff reason. A line cut short ends in `…`, so a reader
/// knows it is not whole.
fn one_line(text: &str) -> String {
    let clean = plain_line(text);
    if clean.len() <= REASON_BYTES {
        return clean;
    }
    let kept = cut(&clean, REASON_BYTES - '…'.len_utf8()).trim_end();
    format!("{kept}…")
}

/// A way of looking at a change. Each lens is one read-only `review` worker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lens {
    Risk,
    Resilience,
    Readability,
    Reliability,
}

impl Lens {
    pub const ALL: [Self; 4] = [
        Self::Risk,
        Self::Resilience,
        Self::Readability,
        Self::Reliability,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Risk => "risk",
            Self::Resilience => "resilience",
            Self::Readability => "readability",
            Self::Reliability => "reliability",
        }
    }

    /// The lens named `id` (any case), if there is one.
    pub fn parse(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|lens| lens.id() == id)
    }
}

/// The user's answer to the consent ask. The orchestrator never gives it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consent {
    Granted,
    Declined,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Severe,
    Minor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Basis {
    Observed,
    Inferred,
}

/// One finding of a lens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Finding {
    pub lens: Lens,
    pub severity: Severity,
    /// The change caused it; a finding that already held is a follow-up.
    pub caused: bool,
    pub basis: Basis,
    /// One clean line.
    pub text: String,
}

impl Finding {
    /// Only a severe finding the change caused blocks.
    pub fn blocks(&self) -> bool {
        self.severity == Severity::Severe && self.caused
    }
}

/// What a lens worker's text held.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// `FINDING` lines that did not have the fixed shape.
    pub malformed: usize,
    /// Well-formed findings past [`MAX_FINDINGS`].
    pub omitted: usize,
}

/// Reads a lens worker's text. The shape is one line per finding, at the
/// start of the line: `FINDING <severe|minor> <change|existing>
/// <observed|inferred>: <text>`. Any other line is ignored, and a `FINDING`
/// line that does not fit the shape is counted, never dropped silently.
pub fn parse_report(lens: Lens, text: &str) -> Report {
    let mut report = Report::default();
    for line in text.lines() {
        match line.strip_prefix("FINDING ") {
            Some(rest) => match finding(lens, rest) {
                None => report.malformed += 1,
                Some(found) => report.findings.push(found),
            },
            None if looks_like(line, "finding") => report.malformed += 1,
            None => {}
        }
    }
    cap(&mut report);
    report
}

/// Whether `line` reads like a line of `word` (`finding` or `claim`) that is
/// not in the exact shape: after leading blanks and bullet, quote, emphasis
/// or list-number marks, it starts with `word` in any case.
fn looks_like(line: &str, word: &str) -> bool {
    let marks = |c: char| c.is_whitespace() || matches!(c, '-' | '*' | '>' | '#' | '_');
    let mut rest = line.trim_start_matches(marks);
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with('.') {
        rest = rest[digits + 1..].trim_start_matches(marks);
    }
    rest.get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
}

/// Keeps [`MAX_FINDINGS`] findings, the blockers before the rest, in the
/// order found; the others are counted as omitted.
fn cap(report: &mut Report) {
    if report.findings.len() <= MAX_FINDINGS {
        return;
    }
    let total = report.findings.len();
    let blockers = report.findings.iter().filter(|f| f.blocks()).count();
    let mut room_for_rest = MAX_FINDINGS.saturating_sub(blockers);
    let mut room_for_blockers = MAX_FINDINGS;
    let kept: Vec<Finding> = std::mem::take(&mut report.findings)
        .into_iter()
        .filter(|found| {
            let room = if found.blocks() {
                &mut room_for_blockers
            } else {
                &mut room_for_rest
            };
            let keep = *room > 0;
            *room = room.saturating_sub(1);
            keep
        })
        .collect();
    report.omitted += total - kept.len();
    report.findings = kept;
}

fn finding(lens: Lens, rest: &str) -> Option<Finding> {
    let (head, text) = rest.split_once(':')?;
    let words: Vec<&str> = head.split_whitespace().collect();
    let [severity, origin, basis] = words[..] else {
        return None;
    };
    let severity = match severity {
        "severe" => Severity::Severe,
        "minor" => Severity::Minor,
        _ => return None,
    };
    let caused = match origin {
        "change" => true,
        "existing" => false,
        _ => return None,
    };
    let basis = match basis {
        "observed" => Basis::Observed,
        "inferred" => Basis::Inferred,
        _ => return None,
    };
    let text = one_line(text);
    (!text.is_empty()).then_some(Finding {
        lens,
        severity,
        caused,
        basis,
        text,
    })
}

/// What the refuter decided about one inferred blocker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refutation {
    Upheld,
    Refuted,
}

/// Reads the refuter's text for `count` claims: `CLAIM <number> <upheld|
/// refuted|unsettled>: <evidence>` at the start of a line. The first line for
/// a claim counts; `unsettled`, a claim with none and a malformed one are all
/// `None`: unanswered, never upheld and never cleared.
pub fn parse_verdicts(text: &str, count: usize) -> Verdicts {
    let mut answers = vec![None; count];
    let mut seen = vec![false; count];
    let (mut malformed, mut unsettled) = (0, 0);
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("CLAIM ") else {
            malformed += usize::from(looks_like(line, "claim"));
            continue;
        };
        match claim(rest, count) {
            None => malformed += 1,
            Some((at, _)) if seen[at] => {}
            Some((at, verdict)) => {
                seen[at] = true;
                unsettled += usize::from(verdict.is_none());
                answers[at] = verdict;
            }
        }
    }
    Verdicts {
        answers,
        malformed,
        unsettled,
    }
}

/// `<number> <upheld|refuted|unsettled>: <evidence>` as the claim index and
/// verdict (`None` for `unsettled`).
fn claim(rest: &str, count: usize) -> Option<(usize, Option<Refutation>)> {
    let (head, _) = rest.split_once(':')?;
    let words: Vec<&str> = head.split_whitespace().collect();
    let [number, verdict] = words[..] else {
        return None;
    };
    let verdict = match verdict {
        "upheld" => Some(Refutation::Upheld),
        "refuted" => Some(Refutation::Refuted),
        "unsettled" => None,
        _ => return None,
    };
    let at = number.parse::<usize>().ok()?.checked_sub(1)?;
    (at < count).then_some((at, verdict))
}

/// What the refuter's text answered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdicts {
    /// One per claim; `None` when it got no valid answer.
    pub answers: Vec<Option<Refutation>>,
    /// `CLAIM` lines that did not fit the shape or named no claim.
    pub malformed: usize,
    /// Claims the refuter said it could not settle.
    pub unsettled: usize,
}

/// How a review ended. None of them approves the batch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The user declined: no review worker ran.
    NotReviewed,
    Clean,
    /// The one correction cleared every blocker it targeted.
    Corrected,
    /// Blockers remain for the user to decide.
    BlockersOpen,
    /// Nothing blocks, but the review could not read everything: findings
    /// were omitted or malformed, or the refuter left claims unanswered.
    Incomplete,
}

/// Why a step of the review was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewError {
    ConsentFirst,
    Declined,
    AlreadyAnswered,
    UnknownLens,
    Reason,
    LensTaken,
    NotPicked,
    NotABlocker,
    NoBlockers,
    RefuterPending,
    CorrectionUsed,
    NoCorrection,
    NotTargeted,
    Pending,
    AlreadyJudged,
    LensesPicked,
    Waived,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Correction {
    Unused,
    /// Running, aimed at these findings.
    Running(Vec<usize>),
    /// Done; `open` of its `targets` are still blockers.
    Done {
        targets: Vec<usize>,
        open: Vec<usize>,
    },
}

/// What the refuter said about one inferred blocker, as the review keeps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    Said(Refutation),
    /// Settled without an answer: not upheld.
    Unanswered,
}

/// What one lens's report left out, for the receipt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LensCount {
    pub lens: Lens,
    pub malformed: usize,
    pub omitted: usize,
}

/// The state of one review: the consent, the lenses, what they found, what
/// the refuter said and the one correction.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Review {
    assessment: Assessment,
    consent: Option<Consent>,
    lenses: Vec<(Lens, String)>,
    /// The review needs no lens: said by the orchestrator after consent.
    waived: bool,
    counts: Vec<LensCount>,
    findings: Vec<Finding>,
    verdicts: Vec<Option<Verdict>>,
    correction: Correction,
}

impl Review {
    pub fn new(assessment: Assessment) -> Self {
        Self {
            assessment,
            consent: None,
            lenses: Vec::new(),
            waived: false,
            counts: Vec::new(),
            findings: Vec::new(),
            verdicts: Vec::new(),
            correction: Correction::Unused,
        }
    }

    pub fn assessment(&self) -> &Assessment {
        &self.assessment
    }

    /// The user's answer, given once.
    pub fn consent(&mut self, answer: Consent) -> Result<(), ReviewError> {
        if self.consent.is_some() {
            return Err(ReviewError::AlreadyAnswered);
        }
        self.consent = Some(answer);
        Ok(())
    }

    /// Consent given, or why the review cannot go on.
    fn consented(&self) -> Result<(), ReviewError> {
        match self.consent {
            None => Err(ReviewError::ConsentFirst),
            Some(Consent::Declined) => Err(ReviewError::Declined),
            Some(Consent::Granted) => Ok(()),
        }
    }

    /// Consent given and no correction begun: the lenses may still change.
    fn open_for_lenses(&self) -> Result<(), ReviewError> {
        self.consented()?;
        match self.correction {
            Correction::Unused => Ok(()),
            _ => Err(ReviewError::CorrectionUsed),
        }
    }

    /// The orchestrator picks lens `id` with a one-line `reason`.
    pub fn pick(&mut self, id: &str, reason: &str) -> Result<Lens, ReviewError> {
        self.open_for_lenses()?;
        if self.waived {
            return Err(ReviewError::Waived);
        }
        let lens = Lens::parse(id).ok_or(ReviewError::UnknownLens)?;
        let reason = one_line(reason);
        if reason.is_empty() {
            return Err(ReviewError::Reason);
        }
        if self.lenses.iter().any(|(known, _)| *known == lens) {
            return Err(ReviewError::LensTaken);
        }
        self.lenses.push((lens, reason));
        Ok(lens)
    }

    /// The consented review needs no lens (the change calls for none): it
    /// can end `Clean` with none picked. Refused once a lens is picked.
    pub fn no_lenses_needed(&mut self) -> Result<(), ReviewError> {
        self.open_for_lenses()?;
        if !self.lenses.is_empty() {
            return Err(ReviewError::LensesPicked);
        }
        self.waived = true;
        Ok(())
    }

    /// The lenses picked, each with its reason, in the order picked.
    pub fn lenses(&self) -> Vec<(Lens, &str)> {
        let lenses = self.lenses.iter();
        lenses.map(|(lens, why)| (*lens, why.as_str())).collect()
    }

    /// A picked lens reported `text`: the findings are kept. Returns how many
    /// `FINDING` lines did not fit the shape. Refused once the correction
    /// began: it was aimed at what the lenses had reported by then.
    pub fn report(&mut self, lens: Lens, text: &str) -> Result<usize, ReviewError> {
        self.open_for_lenses()?;
        if !self.lenses.iter().any(|(known, _)| *known == lens) {
            return Err(ReviewError::NotPicked);
        }
        if self.counts.iter().any(|count| count.lens == lens) {
            return Err(ReviewError::LensTaken);
        }
        let report = parse_report(lens, text);
        self.counts.push(LensCount {
            lens,
            malformed: report.malformed,
            omitted: report.omitted,
        });
        self.verdicts.extend(report.findings.iter().map(|_| None));
        self.findings.extend(report.findings);
        Ok(report.malformed)
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Per lens that reported: the lines that did not fit the shape and the
    /// findings past the cap. Both are shown in the receipt.
    pub fn counts(&self) -> &[LensCount] {
        &self.counts
    }

    pub fn omitted(&self) -> usize {
        self.counts.iter().map(|count| count.omitted).sum()
    }

    pub fn malformed(&self) -> usize {
        self.counts.iter().map(|count| count.malformed).sum()
    }

    /// The claims settled without an answer from the refuter.
    pub fn unanswered(&self) -> usize {
        let settled = self.verdicts.iter();
        settled.filter(|v| **v == Some(Verdict::Unanswered)).count()
    }

    /// The inferred blockers no verdict covers yet: the refuter's batch.
    pub fn to_refute(&self) -> Vec<usize> {
        (0..self.findings.len())
            .filter(|&at| self.inferred_blocker(at) && self.verdicts[at].is_none())
            .collect()
    }

    fn inferred_blocker(&self, at: usize) -> bool {
        self.findings
            .get(at)
            .is_some_and(|f| f.blocks() && f.basis == Basis::Inferred)
    }

    /// The refuter's verdict on finding `at`, which must be an inferred
    /// blocker. A verdict is final, and none comes after the correction began.
    pub fn refute(&mut self, at: usize, verdict: Refutation) -> Result<(), ReviewError> {
        self.open_for_lenses()?;
        if !self.inferred_blocker(at) {
            return Err(ReviewError::NotABlocker);
        }
        if self.verdicts[at].is_some() {
            return Err(ReviewError::AlreadyJudged);
        }
        self.verdicts[at] = Some(Verdict::Said(verdict));
        Ok(())
    }

    /// The refuter ended without answering some claims: they are not upheld.
    /// Returns how many were settled; `unanswered` keeps the count.
    pub fn settle_unanswered(&mut self) -> Result<usize, ReviewError> {
        self.open_for_lenses()?;
        let pending = self.to_refute();
        for &at in &pending {
            self.verdicts[at] = Some(Verdict::Unanswered);
        }
        Ok(pending.len())
    }

    /// The findings that count as blockers: severe, caused by the change, and
    /// either observed or upheld by the refuter.
    pub fn blockers(&self) -> Vec<usize> {
        (0..self.findings.len())
            .filter(|&at| {
                let found = &self.findings[at];
                found.blocks()
                    && (found.basis == Basis::Observed
                        || self.verdicts[at] == Some(Verdict::Said(Refutation::Upheld)))
            })
            .collect()
    }

    /// Every picked lens reported (or none was needed).
    fn all_reported(&self) -> bool {
        if self.lenses.is_empty() {
            return self.waived;
        }
        self.counts.len() >= self.lenses.len()
    }

    /// Starts the one correction, aimed at the blockers that survived, and
    /// returns them. Refused a second time, and while a lens has not
    /// reported or the refuter has not answered.
    pub fn begin_correction(&mut self) -> Result<Vec<usize>, ReviewError> {
        self.consented()?;
        if self.correction != Correction::Unused {
            return Err(ReviewError::CorrectionUsed);
        }
        if !self.all_reported() {
            return Err(ReviewError::Pending);
        }
        if !self.to_refute().is_empty() {
            return Err(ReviewError::RefuterPending);
        }
        let targets = self.blockers();
        if targets.is_empty() {
            return Err(ReviewError::NoBlockers);
        }
        self.correction = Correction::Running(targets.clone());
        Ok(targets)
    }

    /// The correction ended and its result was checked against the blockers
    /// it targeted only: `open` are those that remain.
    pub fn finish_correction(&mut self, open: &[usize]) -> Result<(), ReviewError> {
        let Correction::Running(targets) = &self.correction else {
            return Err(ReviewError::NoCorrection);
        };
        if open.iter().any(|at| !targets.contains(at)) {
            return Err(ReviewError::NotTargeted);
        }
        let targets = targets.clone();
        let open = targets
            .iter()
            .copied()
            .filter(|at| open.contains(at))
            .collect();
        self.correction = Correction::Done { targets, open };
        Ok(())
    }

    /// How the review ends, once nothing is pending. A blocker open, in or
    /// out of the correction's targets, is `BlockersOpen`; a review that could
    /// not read everything is never `Clean` or `Corrected`.
    pub fn conclude(&self) -> Result<Outcome, ReviewError> {
        match self.consent {
            None => return Err(ReviewError::ConsentFirst),
            Some(Consent::Declined) => return Ok(Outcome::NotReviewed),
            Some(Consent::Granted) => {}
        }
        if !self.all_reported() {
            return Err(ReviewError::Pending);
        }
        if !self.to_refute().is_empty() {
            return Err(ReviewError::RefuterPending);
        }
        if matches!(self.correction, Correction::Running(_)) {
            return Err(ReviewError::Pending);
        }
        if !self.open().is_empty() {
            return Ok(Outcome::BlockersOpen);
        }
        if self.omitted() + self.malformed() + self.unanswered() > 0 {
            return Ok(Outcome::Incomplete);
        }
        Ok(match self.correction {
            Correction::Done { .. } => Outcome::Corrected,
            _ => Outcome::Clean,
        })
    }

    /// The blockers still open for the user to decide: those the correction
    /// did not clear, and any the correction was not aimed at.
    pub fn open(&self) -> Vec<usize> {
        match &self.correction {
            Correction::Unused => self.blockers(),
            Correction::Running(targets) => targets.clone(),
            Correction::Done { targets, open } => {
                let mut open = open.clone();
                open.extend(
                    self.blockers()
                        .into_iter()
                        .filter(|at| !targets.contains(at)),
                );
                open
            }
        }
    }
}

/// What a worker is told: `lead`, then the staged change, all wrapped as
/// untrusted data under `nonce`. The wrapper cuts a long text at its end, so
/// what must survive goes in `lead`, before the listing.
fn candidate(nonce: [u8; 8], lead: &str, changes: &[Change]) -> String {
    let mut staged = ChangeLedger::default();
    for change in changes {
        staged.staged(change.clone());
    }
    let listing = staged
        .render()
        .unwrap_or_else(|| "files changed: none".to_owned());
    label::wrap(&label::nonce_from(nonce), &format!("{lead}{listing}"))
}

/// The prompt of the worker of `lens`: the lens, the orchestrator's one-line
/// reason and the staged change, wrapped. The worker reads the content
/// through the staged view of its read tools.
pub fn lens_prompt(nonce: [u8; 8], lens: Lens, why: &str, changes: &[Change]) -> String {
    format!(
        "Review lens: {}\nWhy this lens: {}\nThe staged change to review, as data:\n{}",
        lens.id(),
        one_line(why),
        candidate(nonce, "", changes)
    )
}

/// The prompt of the refuter: the claims to examine, then the staged change,
/// wrapped together because the claims are a worker's text.
pub fn refuter_prompt(nonce: [u8; 8], changes: &[Change], claims: &[&Finding]) -> String {
    let lead = claims_lead(claims);
    format!(
        "Claims to refute: {}\nThe claims and the staged change, as data:\n{}",
        claims.len(),
        candidate(nonce, &lead, changes)
    )
}

/// The prompt of the check of a correction: the same claims as the refuter's,
/// but a neutral header and a lead that does not lean toward refuting. A claim
/// is refuted only on clear evidence in the staged text that the problem is
/// gone; anything else is unsettled and stays open.
pub fn validation_prompt(nonce: [u8; 8], changes: &[Change], claims: &[&Finding]) -> String {
    let lead = claims_lead(claims);
    format!(
        "Claims to check: {}\nThese claims were reported before a correction was staged. \
         Refute a claim only if the staged text clearly shows the problem is gone; \
         anything else is unsettled and stays open.\nThe claims and the staged change, as data:\n{}",
        claims.len(),
        candidate(nonce, &lead, changes)
    )
}

fn claims_lead(claims: &[&Finding]) -> String {
    let mut lead = String::from("claims to examine:");
    for (at, claim) in claims.iter().enumerate() {
        lead.push_str(&format!(
            "\n{}. [{}] {}",
            at + 1,
            claim.lens.id(),
            claim.text
        ));
    }
    lead.push_str("\n\n");
    lead
}

/// The prompt of the one correction worker: the blockers that survived, as a
/// worker's text and so wrapped, then the staged change. It stages fixes with
/// its write tools and does not propose the batch.
pub fn correction_prompt(nonce: [u8; 8], changes: &[Change], blockers: &[&Finding]) -> String {
    let mut lead = String::from("blockers to fix:");
    for (at, blocker) in blockers.iter().enumerate() {
        lead.push_str(&format!(
            "\n{}. [{}] {}",
            at + 1,
            blocker.lens.id(),
            blocker.text
        ));
    }
    lead.push_str("\n\n");
    format!(
        "Correction: fix only these {} blockers by staging changes with your write tools. \
         Change nothing else and do not call propose_batch: the worker whose change this is \
         proposes it.\nThe blockers and the staged change, as data:\n{}",
        blockers.len(),
        candidate(nonce, &lead, changes)
    )
}

/// The paths named in a consent ask before it says `+N more`.
const SHOWN_PATHS: usize = 5;

/// The consent ask of a worker's `high` change, for the user's chat pane:
/// the worker's label, why the change is `high` and its first paths, all one
/// clean line. The ask is aimed at the worker's session.
pub fn consent_ask(
    worker: WorkerKey,
    label: &str,
    assessment: &Assessment,
    changes: &[Change],
) -> ApprovalAsk {
    let why = match &assessment.source {
        Source::Rules => "build, CI, dependency or config files".to_owned(),
        Source::Raised(reason) => format!("raised by the orchestrator ({})", one_line(reason)),
        Source::Failed(why) => format!("could not assess ({})", one_line(why)),
    };
    // The ledger's paths are clean lines already; cleaned again, as this
    // line goes on the user's screen.
    let mut paths: Vec<String> = changes
        .iter()
        .take(SHOWN_PATHS)
        .map(|change| one_line(&change.path))
        .collect();
    let more = changes.len().saturating_sub(SHOWN_PATHS);
    if more > 0 {
        paths.push(format!("+{more} more"));
    }
    ApprovalAsk::Consent {
        worker,
        label: one_line(label),
        change: format!("{why}: {}", paths.join(", ")),
    }
}
