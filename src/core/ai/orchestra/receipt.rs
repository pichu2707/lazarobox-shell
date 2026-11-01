//! The receipt of a receipt-driven review (agent-orchestrator S7b, D16, D17):
//! one handoff per review, built by lazarobox from the review it kept and
//! never by the model, and the short line the user's batch review shows
//! about it. Pure. A receipt reports; it never approves: nothing in it can
//! answer the batch review, whose keys and default stay the user's.

use super::WorkerId;
use super::ledger::{FIELD_BYTES, Judgement, NONE, Verdict, cut, one_line};
use super::rdd::{Basis, Consent, Outcome, Review, Severity};
use super::review::Source;

/// The longest list of paths in one line, in bytes.
const LIST_BYTES: usize = 400;

/// The longest text of one finding or lens reason in the receipt, in bytes.
const TEXT_BYTES: usize = 240;

/// What a finished review left behind, as the receipt reads it.
pub struct Evidence<'a> {
    pub worker: WorkerId,
    /// The review's number among this worker's, from 1.
    pub n: u32,
    /// The staged paths the review was about.
    pub paths: &'a [String],
    pub review: &'a Review,
    /// `None` when the review stopped before it could conclude.
    pub outcome: Option<Outcome>,
    /// Staged after the set that was assessed or changed after its review.
    pub unreviewed: &'a [String],
    /// Nobody answered the consent, and why.
    pub unanswered: Option<&'a str>,
    /// Staged paths a review worker could not read.
    pub unread: &'a [String],
}

/// The counts a run keeps per receipt for its ledger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// Lens workers looked at the change.
    pub reviewed: bool,
    /// Blockers still open for the user to decide.
    pub open: usize,
}

pub fn summary(ev: &Evidence) -> Summary {
    let granted = ev.review.consent_answer() == Some(Consent::Granted);
    let concluded = !matches!(ev.outcome, None | Some(Outcome::NotReviewed));
    Summary {
        reviewed: granted && concluded,
        open: if granted { ev.review.open().len() } else { 0 },
    }
}

/// The word for how the review ended. A review that stopped early, or never
/// began, is `not reviewed`. A clean or corrected review that left paths
/// unseen says it covers the reviewed paths only.
fn outcome_word(ev: &Evidence) -> String {
    let word = match ev.outcome {
        Some(Outcome::Clean) => "clean",
        Some(Outcome::Corrected) => "corrected",
        Some(Outcome::BlockersOpen) => "blockers open",
        Some(Outcome::Incomplete) => "incomplete",
        Some(Outcome::NotReviewed) | None => "not reviewed",
    };
    let partial = matches!(ev.outcome, Some(Outcome::Clean | Outcome::Corrected))
        && !(ev.unreviewed.is_empty() && ev.unread.is_empty());
    if partial {
        format!("{word} (reviewed paths only)")
    } else {
        word.to_owned()
    }
}

/// The word for the consent answer.
fn consent_word(ev: &Evidence) -> String {
    match (ev.review.consent_answer(), ev.unanswered) {
        (_, Some(why)) => format!("not answered ({})", clip(why)),
        (Some(Consent::Granted), None) => "granted".to_owned(),
        (Some(Consent::Declined), None) => "declined".to_owned(),
        (None, None) => "not asked".to_owned(),
    }
}

/// `text` as one clean line, cut at [`TEXT_BYTES`] with a mark.
fn clip(text: &str) -> String {
    let line = one_line(text);
    if line.len() <= TEXT_BYTES {
        return line;
    }
    format!(
        "{}\u{2026}",
        cut(&line, TEXT_BYTES - '\u{2026}'.len_utf8()).trim_end()
    )
}

/// `items` as one line, as many as [`LIST_BYTES`] allow, then how many more.
fn list(items: &[String]) -> String {
    let mut out = String::new();
    for (shown, item) in items.iter().enumerate() {
        let item = clip(item);
        if !out.is_empty() && out.len() + item.len() + 2 > LIST_BYTES {
            out.push_str(&format!(", +{} more", items.len() - shown));
            return out;
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&item);
    }
    out
}

/// `lines` joined within [`FIELD_BYTES`]: the lines that do not fit are
/// counted, never dropped without a word.
pub(super) fn fit(lines: Vec<String>) -> String {
    let room = FIELD_BYTES - 32;
    let mut out = String::new();
    for (shown, line) in lines.iter().enumerate() {
        if out.len() + line.len() + 1 > room {
            if out.is_empty() {
                // One line over the cap on its own: clipped, not dropped.
                let kept = cut(line, room - '\u{2026}'.len_utf8()).trim_end();
                out = format!("{kept}\u{2026}");
                if shown + 1 == lines.len() {
                    return out;
                }
                continue;
            }
            out.push_str(&format!("\n(+{} more lines)", lines.len() - shown));
            return out;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn tier_line(ev: &Evidence) -> String {
    let assessment = ev.review.assessment();
    let source = match &assessment.source {
        Source::Rules => "rules".to_owned(),
        Source::Raised(why) => format!("raised by the orchestrator: {}", clip(why)),
        Source::Failed(why) => format!("assessment failed: {}", clip(why)),
    };
    format!("tier: {} ({source})", assessment.tier.as_str())
}

fn lenses_line(ev: &Evidence) -> String {
    let picked: Vec<String> = ev
        .review
        .lenses()
        .into_iter()
        .map(|(lens, why)| format!("{} ({})", lens.id(), clip(why)))
        .collect();
    if picked.is_empty() {
        "lenses: none".to_owned()
    } else {
        format!("lenses: {}", picked.join("; "))
    }
}

fn correction_line(ev: &Evidence) -> String {
    match ev.review.correction_targets() {
        None => "correction: none".to_owned(),
        Some((targets, open)) => {
            format!("correction: {targets} blocker(s) targeted, {open} still open")
        }
    }
}

fn done(ev: &Evidence) -> String {
    fit(vec![
        format!("change: worker {} staged {}", ev.worker.0, list(ev.paths)),
        tier_line(ev),
        format!("consent: {}", consent_word(ev)),
        lenses_line(ev),
        format!("outcome: {}", outcome_word(ev)),
        correction_line(ev),
    ])
}

fn left(ev: &Evidence) -> String {
    let review = ev.review;
    let mut lines: Vec<String> = Vec::new();
    if summary(ev).open > 0 {
        for at in review.open() {
            if let Some(found) = review.findings().get(at) {
                lines.push(format!("open blocker: {}", clip(&found.text)));
            }
        }
    }
    if !ev.unreviewed.is_empty() {
        lines.push(format!("not reviewed: {}", list(ev.unreviewed)));
    }
    if !ev.unread.is_empty() {
        lines.push(format!("could not read: {}", list(ev.unread)));
    }
    for (what, n) in [
        ("omitted", review.omitted()),
        ("malformed", review.malformed()),
        ("unanswered", review.unanswered()),
    ] {
        if n > 0 {
            lines.push(format!("{what}: {n}"));
        }
    }
    if lines.is_empty() {
        NONE.to_owned()
    } else {
        fit(lines)
    }
}

fn decisions(ev: &Evidence) -> String {
    let review = ev.review;
    let blockers = review.blockers();
    let lines: Vec<String> = review
        .findings()
        .iter()
        .enumerate()
        .map(|(at, found)| {
            let severity = match found.severity {
                Severity::Severe => "severe",
                Severity::Minor => "minor",
            };
            let cause = if found.caused { "change" } else { "existing" };
            let basis = match found.basis {
                Basis::Observed => "observed",
                Basis::Inferred => "inferred",
            };
            let blocked = if blockers.contains(&at) {
                "blocked"
            } else {
                "did not block"
            };
            let refuter = if found.blocks() && found.basis == Basis::Inferred {
                format!("; refuter: {}", review.refuter_word(at))
            } else {
                String::new()
            };
            format!(
                "{} {severity} {cause} {basis}: {} ({blocked}{refuter})",
                found.lens.id(),
                clip(&found.text)
            )
        })
        .collect();
    if lines.is_empty() {
        NONE.to_owned()
    } else {
        fit(lines)
    }
}

/// The judgement of the receipt handoff. The verdict is `blocked` while a
/// blocker is open and `unknown` otherwise: a receipt never says `on_course`,
/// which is the orchestrator's word and the user's decision.
pub fn judgement(ev: &Evidence) -> Judgement {
    let open = summary(ev).open;
    Judgement {
        verdict: if open > 0 {
            Verdict::Blocked
        } else {
            Verdict::Unknown
        },
        reason: format!(
            "review {} of worker {}: {}, {open} blocker(s) open",
            ev.n,
            ev.worker.0,
            outcome_word(ev)
        ),
        done: done(ev),
        left: left(ev),
        decisions: decisions(ev),
    }
}

/// The line the batch review shows about the receipt: lazarobox's words and
/// counts only, never a worker's text, and no word of an approval.
pub fn line(ev: &Evidence) -> String {
    let mut out = format!(
        "review: {} \u{b7} consent {}",
        ev.review.assessment().tier.as_str(),
        consent_word(ev).split(" (").next().unwrap_or_default()
    );
    let open = summary(ev).open;
    if open > 0 {
        out.push_str(&format!(" \u{b7} blockers open: {open}"));
    } else {
        out.push_str(&format!(" \u{b7} {}", outcome_word(ev)));
    }
    let unseen = ev.unreviewed.len() + ev.unread.len();
    if unseen > 0 {
        let s = if unseen == 1 { "" } else { "s" };
        out.push_str(&format!(" \u{b7} not reviewed: {unseen} path{s}"));
    }
    out
}
