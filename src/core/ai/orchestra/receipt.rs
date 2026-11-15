//! The receipt of a receipt-driven review (agent-orchestrator S7b, D16, D17):
//! one handoff per review, built by lazarobox from the review it kept and
//! never by the model, and the short line the user's batch review shows
//! about it. Pure. A receipt reports; it never approves: nothing in it can
//! answer the batch review, whose keys and default stay the user's.

use super::WorkerId;
use super::changes::GAP_CUT;
use super::judge::{Agreement, State, Verdict as Said};
use super::ledger::{FIELD_BYTES, Judgement, NONE, Verdict, cut, one_line};
use super::rdd::{Basis, Consent, Lens, Outcome, Review, Severity};
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
    let granted = matches!(
        ev.review.consent_answer(),
        Some(Consent::Granted | Consent::Mandatory)
    );
    let concluded = !matches!(ev.outcome, None | Some(Outcome::NotReviewed));
    Summary {
        reviewed: granted && concluded,
        open: if granted { ev.review.open().len() } else { 0 },
    }
}

/// The word for how the review ended. A review that stopped early, or never
/// began, is `not reviewed`, with why when it stopped. A clean or corrected review that left paths
/// unseen says it covers the reviewed paths only.
fn outcome_word(ev: &Evidence) -> String {
    let word = match ev.outcome {
        Some(Outcome::Clean) => "clean".to_owned(),
        Some(Outcome::Corrected) => "corrected".to_owned(),
        Some(Outcome::BlockersOpen) => "blockers open".to_owned(),
        Some(Outcome::Incomplete) if !ev.review.reasons().is_empty() => {
            format!("incomplete ({})", clip(&ev.review.reasons().join("; ")))
        }
        Some(Outcome::Incomplete) => "incomplete".to_owned(),
        Some(Outcome::NotReviewed) | None => match ev.review.stopped() {
            Some(why) => format!("not reviewed ({})", clip(why)),
            None => "not reviewed".to_owned(),
        },
    };
    let partial = matches!(ev.outcome, Some(Outcome::Clean | Outcome::Corrected))
        && !(ev.unreviewed.is_empty() && ev.unread.is_empty());
    if partial {
        format!("{word} (reviewed paths only)")
    } else {
        word
    }
}

/// The word for the consent answer.
fn consent_word(ev: &Evidence) -> String {
    match (ev.review.consent_answer(), ev.unanswered) {
        (_, Some(why)) => format!("not answered ({})", clip(why)),
        (Some(Consent::Granted), None) => "granted".to_owned(),
        (Some(Consent::Declined), None) => "declined".to_owned(),
        (Some(Consent::Mandatory), None) => "mandatory".to_owned(),
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
        None => match ev.review.limit() {
            Some(rounds) => format!("correction: limit reached ({rounds} rounds)"),
            None => "correction: none".to_owned(),
        },
        Some((targets, open)) => {
            format!("correction: {targets} blocker(s) targeted, {open} still open")
        }
    }
}

/// The judge section (acceptance-judges), in two parts so the cap of one
/// field never reduces a fault to a count: the lines for `done` (consent,
/// level, flags, the tests line, each round's agreement and judges, then the
/// criteria that were not met or unknown, and the `met` ones last) and one
/// line per fault with its place for `decisions`. Both empty when no judge
/// was set up for the review.
fn judge_section(review: &Review) -> (Vec<String>, Vec<String>) {
    let Some(judging) = review.judging() else {
        return (Vec::new(), Vec::new());
    };
    let granted = judging.granted;
    let word = if granted {
        "granted"
    } else {
        "declined (not judged)"
    };
    let mut lines = vec![format!("judge consent: {word}")];
    if !granted {
        return (lines, Vec::new());
    }
    let standard = &judging.standard;
    let criteria: Vec<String> = (standard.criteria.iter())
        .map(|c| format!("{} {}", c.id, c.text))
        .collect();
    lines.push(clip(&judging.level));
    lines.push(format!("criteria: {}", list(&criteria)));
    let cut = standard.norms.iter().any(|norm| norm.cut_from.is_some());
    for (on, text) in [
        (standard.thin, "criteria: thin".to_owned()),
        (
            standard.dropped > 0,
            format!("criteria: {} not judged (cap 12)", standard.dropped),
        ),
        (cut, "norms cut at 8 KB".to_owned()),
        (!judging.independent, "judges not independent".to_owned()),
    ] {
        if on {
            lines.push(text);
        }
    }
    lines.push("tests: not seen before apply".to_owned());
    let (mut faults, mut open, mut met) = (Vec::new(), Vec::new(), Vec::new());
    for (at, round) in review.rounds().iter().enumerate() {
        let r = at + 1;
        lines.push(match &round.agreement {
            Agreement::Pass { clean: true, .. } => format!("round {r} agree: pass"),
            Agreement::Pass { .. } => format!("round {r} agree: pass, unverified"),
            Agreement::Fail(_) => format!("round {r} agree: fail"),
            Agreement::Disagree { missing } => {
                format!("round {r} disagree, missing: {}", clip(missing))
            }
        });
        if let Agreement::Pass { unverified, .. } = &round.agreement {
            lines.extend(
                unverified
                    .iter()
                    .map(|(n, why)| format!("unverified: C{n} {}", clip(why))),
            );
        }
        for (i, ruling) in round.rulings.iter().enumerate() {
            let who = format!("round {r} judge {}", i + 1);
            let model = judging
                .models
                .get(i)
                .map_or("unknown model", String::as_str);
            let said = match ruling.verdict {
                Ok(Said::Pass) => "pass".to_owned(),
                Ok(Said::Fail) => "fail".to_owned(),
                Err(why) => format!("no verdict ({})", why.word()),
            };
            let count = |want: State| {
                ruling
                    .criteria
                    .iter()
                    .flatten()
                    .filter(|(s, _)| *s == want)
                    .count()
            };
            lines.push(format!(
                "{who} ({}): {said} \u{b7} {} met, {} not met, {} unknown",
                clip(model),
                count(State::Met),
                count(State::NotMet),
                count(State::Unknown)
            ));
            for (n, row) in ruling.criteria.iter().enumerate() {
                let Some((state, why)) = row else { continue };
                let (name, rows) = match state {
                    State::Met => ("met", &mut met),
                    State::NotMet => ("not met", &mut open),
                    State::Unknown => ("unknown", &mut open),
                };
                rows.push(format!("{who} C{} {name}: {}", n + 1, clip(why)));
            }
            for f in &ruling.faults {
                let at = f.at.as_deref().unwrap_or("unknown");
                faults.push(format!(
                    "{who} C{} at: {}: {}",
                    f.criterion,
                    clip(at),
                    clip(&f.text)
                ));
            }
        }
    }
    lines.extend(open);
    lines.extend(met);
    (lines, faults)
}

fn done(ev: &Evidence) -> String {
    let mut lines = vec![
        format!("change: worker {} staged {}", ev.worker.0, list(ev.paths)),
        tier_line(ev),
        format!("consent: {}", consent_word(ev)),
        lenses_line(ev),
        format!("outcome: {}", outcome_word(ev)),
        correction_line(ev),
    ];
    lines.extend(judge_section(ev.review).0);
    fit(lines)
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
    for gap in review.gaps() {
        let bytes = review.cut_bytes();
        if gap == GAP_CUT && bytes > 0 {
            lines.push(format!("not whole: {gap} ({bytes} byte(s) missing)"));
        } else {
            lines.push(format!("not whole: {}", clip(gap)));
        }
    }
    for (what, n) in [
        ("omitted", review.omitted()),
        ("malformed", review.malformed()),
        ("reviewers that could not see the change", review.unseen()),
        ("reviewers that gave no answer", review.mute()),
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
    // The judges' faults come first: they keep their places, with the cap.
    let mut lines = judge_section(review).1;
    let others = review.findings().iter().enumerate();
    let others = others.filter(|(_, found)| found.lens != Lens::Judge);
    lines.extend(others.map(|(at, found)| {
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
    }));
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
