//! The receipt-driven review of one staged change (agent-orchestrator S7b,
//! D16): consent first, the lenses the orchestrator picked, their findings,
//! the refuter's verdicts and at most one correction. Pure: it holds the
//! state of one review and decides what may come next; the runtime starts the
//! workers and the host asks the user.

use super::{
    changes::{Change, ChangeLedger},
    review::{Assessment, one_line},
};
use crate::core::ai::tools::label;

/// The findings kept per lens; the rest are only counted.
pub const MAX_FINDINGS: usize = 12;

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
        let Some(rest) = line.strip_prefix("FINDING ") else {
            continue;
        };
        match finding(lens, rest) {
            None => report.malformed += 1,
            Some(_) if report.findings.len() >= MAX_FINDINGS => report.omitted += 1,
            Some(found) => report.findings.push(found),
        }
    }
    report
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
/// refuted>: <evidence>` at the start of a line. The first answer to a claim
/// counts; a claim with none (or a malformed one) is `None`, and it is not
/// upheld.
pub fn parse_verdicts(text: &str, count: usize) -> Vec<Option<Refutation>> {
    let mut verdicts = vec![None; count];
    for line in text.lines() {
        let Some((head, _)) = line.strip_prefix("CLAIM ").and_then(|r| r.split_once(':')) else {
            continue;
        };
        let words: Vec<&str> = head.split_whitespace().collect();
        let [number, verdict] = words[..] else {
            continue;
        };
        let verdict = match verdict {
            "upheld" => Refutation::Upheld,
            "refuted" => Refutation::Refuted,
            _ => continue,
        };
        let slot = number.parse::<usize>().ok().and_then(|n| n.checked_sub(1));
        if let Some(slot) = slot.and_then(|at| verdicts.get_mut(at))
            && slot.is_none()
        {
            *slot = Some(verdict);
        }
    }
    verdicts
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
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Correction {
    Unused,
    /// Running, aimed at these findings.
    Running(Vec<usize>),
    /// Done; these findings are still blockers.
    Done {
        open: Vec<usize>,
    },
}

/// The state of one review: the consent, the lenses, what they found, what
/// the refuter said and the one correction.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Review {
    assessment: Assessment,
    consent: Option<Consent>,
    lenses: Vec<(Lens, String)>,
    reported: Vec<Lens>,
    findings: Vec<Finding>,
    verdicts: Vec<Option<Refutation>>,
    correction: Correction,
}

impl Review {
    pub fn new(assessment: Assessment) -> Self {
        Self {
            assessment,
            consent: None,
            lenses: Vec::new(),
            reported: Vec::new(),
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

    /// The orchestrator picks lens `id` with a one-line `reason`.
    pub fn pick(&mut self, id: &str, reason: &str) -> Result<Lens, ReviewError> {
        self.consented()?;
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

    /// The lenses picked, each with its reason, in the order picked.
    pub fn lenses(&self) -> Vec<(Lens, &str)> {
        let lenses = self.lenses.iter();
        lenses.map(|(lens, why)| (*lens, why.as_str())).collect()
    }

    /// A picked lens reported `text`: the findings are kept. Returns how many
    /// `FINDING` lines did not fit the shape.
    pub fn report(&mut self, lens: Lens, text: &str) -> Result<usize, ReviewError> {
        self.consented()?;
        if !self.lenses.iter().any(|(known, _)| *known == lens) {
            return Err(ReviewError::NotPicked);
        }
        if self.reported.contains(&lens) {
            return Err(ReviewError::LensTaken);
        }
        self.reported.push(lens);
        let report = parse_report(lens, text);
        self.verdicts.extend(report.findings.iter().map(|_| None));
        self.findings.extend(report.findings);
        Ok(report.malformed)
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
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
    /// blocker.
    pub fn refute(&mut self, at: usize, verdict: Refutation) -> Result<(), ReviewError> {
        if !self.inferred_blocker(at) {
            return Err(ReviewError::NotABlocker);
        }
        self.verdicts[at] = Some(verdict);
        Ok(())
    }

    /// The findings that count as blockers: severe, caused by the change, and
    /// either observed or upheld by the refuter.
    pub fn blockers(&self) -> Vec<usize> {
        (0..self.findings.len())
            .filter(|&at| {
                let found = &self.findings[at];
                found.blocks()
                    && (found.basis == Basis::Observed
                        || self.verdicts[at] == Some(Refutation::Upheld))
            })
            .collect()
    }

    /// Starts the one correction, aimed at the blockers that survived, and
    /// returns them. Refused a second time.
    pub fn begin_correction(&mut self) -> Result<Vec<usize>, ReviewError> {
        self.consented()?;
        if self.correction != Correction::Unused {
            return Err(ReviewError::CorrectionUsed);
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
        let open = targets
            .iter()
            .copied()
            .filter(|at| open.contains(at))
            .collect();
        self.correction = Correction::Done { open };
        Ok(())
    }

    /// How the review ends, once nothing is pending.
    pub fn conclude(&self) -> Result<Outcome, ReviewError> {
        match self.consent {
            None => return Err(ReviewError::ConsentFirst),
            Some(Consent::Declined) => return Ok(Outcome::NotReviewed),
            Some(Consent::Granted) => {}
        }
        if self.lenses.is_empty() || self.reported.len() < self.lenses.len() {
            return Err(ReviewError::Pending);
        }
        if !self.to_refute().is_empty() {
            return Err(ReviewError::RefuterPending);
        }
        match &self.correction {
            Correction::Running(_) => Err(ReviewError::Pending),
            Correction::Done { open } if open.is_empty() => Ok(Outcome::Corrected),
            Correction::Done { .. } => Ok(Outcome::BlockersOpen),
            Correction::Unused if self.blockers().is_empty() => Ok(Outcome::Clean),
            Correction::Unused => Ok(Outcome::BlockersOpen),
        }
    }

    /// The blockers still open for the user to decide.
    pub fn open(&self) -> Vec<usize> {
        match &self.correction {
            Correction::Done { open } => open.clone(),
            Correction::Running(targets) => targets.clone(),
            Correction::Unused => self.blockers(),
        }
    }
}

/// The staged change as a worker is told of it, with `extra` lines after it,
/// all wrapped as untrusted data under `nonce`.
fn candidate(nonce: [u8; 8], changes: &[Change], extra: &str) -> String {
    let mut staged = ChangeLedger::default();
    for change in changes {
        staged.staged(change.clone());
    }
    let listing = staged
        .render()
        .unwrap_or_else(|| "files changed: none".to_owned());
    label::wrap(&label::nonce_from(nonce), &format!("{listing}{extra}"))
}

/// The prompt of the worker of `lens`: the lens, the orchestrator's one-line
/// reason and the staged change, wrapped. The worker reads the content
/// through the staged view of its read tools.
pub fn lens_prompt(nonce: [u8; 8], lens: Lens, why: &str, changes: &[Change]) -> String {
    format!(
        "Review lens: {}\nWhy this lens: {}\nThe staged change to review, as data:\n{}",
        lens.id(),
        one_line(why),
        candidate(nonce, changes, "")
    )
}

/// The prompt of the refuter: the staged change and the inferred blockers to
/// examine, wrapped together because the claims are a worker's text.
pub fn refuter_prompt(nonce: [u8; 8], changes: &[Change], claims: &[&Finding]) -> String {
    let mut extra = String::from("\n\nclaims to examine:");
    for (at, claim) in claims.iter().enumerate() {
        extra.push_str(&format!(
            "\n{}. [{}] {}",
            at + 1,
            claim.lens.id(),
            claim.text
        ));
    }
    format!(
        "Claims to refute: {}\nThe staged change and the claims, as data:\n{}",
        claims.len(),
        candidate(nonce, changes, &extra)
    )
}
