//! The acceptance judges inside the review (acceptance-judges S2a): the level
//! of a batch, the judge workers, the consent once per run and the lines.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::core::ai::{chat::prompts::judge_builtin, orchestra::judge::JudgeLevel};
use crate::runtime::tools::Judges;

fn is_judge(request: &ChatRequest) -> bool {
    user_of(request).starts_with("Acceptance judge:")
}

/// How many criteria the judge's prompt names.
fn criteria_of(request: &ChatRequest) -> usize {
    let text = user_of(request);
    let word = text.split("against these ").nth(1).unwrap_or("1");
    word.split(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

/// A judge text over `count` criteria: every one `met`, or `C1` not met at
/// `src/a.rs:4-6`, or the unverified one.
fn text_of(count: usize, how: &str) -> &'static str {
    let rows: String = (1..=count)
        .map(|n| match (how, n) {
            ("fail", 1) => "CRITERION C1 not met | the pin is missing\n".to_owned(),
            ("unverified", 1) => "CRITERION C1 unknown | tests not seen\n".to_owned(),
            _ => format!("CRITERION C{n} met | seen in the diff\n"),
        })
        .collect();
    let tail = match how {
        "fail" => "FINDING C1 | at: src/a.rs:4-6 | the pin is missing\nVERDICT fail",
        _ => "VERDICT pass",
    };
    Box::leak(format!("{rows}{tail}").into_boxed_str())
}

fn passes(request: &ChatRequest) -> Option<&'static str> {
    if is_judge(request) {
        Some(text_of(criteria_of(request), "pass"))
    } else {
        quiet(request)
    }
}

fn fails(request: &ChatRequest) -> Option<&'static str> {
    if is_judge(request) {
        Some(text_of(criteria_of(request), "fail"))
    } else if is_correction(request) {
        Some("staged the fix")
    } else if is_refuter(request) {
        Some("CLAIM 1 refuted: fixed")
    } else {
        quiet(request)
    }
}

fn unverified(request: &ChatRequest) -> Option<&'static str> {
    if is_judge(request) {
        Some(text_of(criteria_of(request), "unverified"))
    } else {
        quiet(request)
    }
}

static SPLIT: AtomicUsize = AtomicUsize::new(0);

/// The first judge to ask passes, the second fails.
fn split(request: &ChatRequest) -> Option<&'static str> {
    if is_judge(request) {
        let how = if SPLIT.fetch_add(1, Ordering::SeqCst) == 0 {
            "pass"
        } else {
            "fail"
        };
        Some(text_of(criteria_of(request), how))
    } else {
        quiet(request)
    }
}

fn small_change() -> Vec<Change> {
    vec![Change {
        path: "src/a.rs".to_owned(),
        size: Some(1),
        hash: "h".to_owned(),
        applied: false,
    }]
}

/// What a judged batch runs with: the changes, the lens answer, the run's
/// judge consent (`None`: not answered) and what is staged after a correction.
struct Batch {
    changes: Vec<Change>,
    answered: Answered,
    consent: Option<bool>,
    restage: Option<Restage>,
    roster: Roster,
    diff: fn() -> StagedDiff,
    /// Corrections the worker already spent.
    spent: u32,
}

impl Batch {
    fn one(consent: Option<bool>) -> Self {
        Self {
            changes: small_change(),
            answered: Answered::Judges,
            consent,
            restage: None,
            roster: codex_both(),
            diff: staged_text,
            spent: 0,
        }
    }

    fn high(answered: Answered, consent: Option<bool>) -> Self {
        Self {
            changes: lock_change(),
            answered,
            consent,
            restage: Some(restaged_differently),
            roster: codex_both(),
            diff: staged_text,
            spent: 0,
        }
    }

    async fn run(self, by: fn(&ChatRequest) -> Option<&'static str>) -> (Lab, RunId, OrchHandle) {
        let lab = lab_with(Script::By(by), |m, home| RunManager {
            scratch: |_| false,
            restage: self.restage,
            diff: Some(self.diff),
            ..tooled(m, home)
        });
        let (run, handle) = lab.begin(lab.scope(self.roster, &[])).unwrap();
        handle.call(spawn(Task::Code, "write a")).await;
        lab.seen.first().await;
        if let Some(granted) = self.consent {
            handle.judges_answered(run, granted);
        }
        let worker = WorkerId(1);
        lab.manager.on_run(pane(), run, |live| {
            live.review.corrections.insert(worker, self.spent);
        });
        let assessment = handle.assess(run, &self.changes);
        let done = handle.review(run, WorkerId(1), self.changes, assessment, self.answered);
        let ended = timeout(Duration::from_secs(10), done).await;
        assert!(ended.expect("the review ended").is_ok());
        (lab, run, handle)
    }
}

fn judge_requests(lab: &Lab) -> Vec<ChatRequest> {
    let all = lab.seen.requests();
    all.into_iter().filter(is_judge).collect()
}

fn said(lab: &mut Lab) -> Vec<String> {
    review_lines(lab)
}

// Spec (The gate is wider than before; Read-only goal gets none): the level
// of a low-tier one-file batch is one judge, and a read-only goal gets none.
#[tokio::test]
async fn the_run_manager_gives_a_low_tier_batch_one_judge_and_a_read_only_goal_none() {
    let lab = lab(Script::Hang);
    let (run, handle) = lab.begin(lab.scope(codex_both(), &[])).unwrap();
    let changes = small_change();
    let assessment = handle.assess(run, &changes);
    assert_eq!(assessment.tier, Tier::Passive);
    assert_eq!(
        handle.judge_level(run, &changes, &assessment),
        JudgeLevel::One
    );
    let two = lock_change();
    let high = handle.assess(run, &two);
    assert_eq!(handle.judge_level(run, &two, &high), JudgeLevel::Two);
    lab.manager
        .on_run(pane(), run, |live| live.run.read_only_for_tests())
        .unwrap();
    assert_eq!(
        handle.judge_level(run, &changes, &assessment),
        JudgeLevel::None
    );
}

// Spec (Consent once per run): the answer is kept for the run; an unanswered
// run says what to ask with; a decline is said once.
#[tokio::test]
async fn the_run_keeps_the_judge_consent_and_says_the_decline_once() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.begin(lab.scope(codex_both(), &[])).unwrap();
    let Judges::Ask(text) = handle.judges(run) else {
        panic!("nothing was answered yet");
    };
    assert!(
        text.contains("codex") && text.contains("one judge"),
        "{text}"
    );
    handle.judges_answered(run, true);
    assert_eq!(handle.judges(run), Judges::Granted);
    handle.judges_answered(run, false);
    assert_eq!(handle.judges(run), Judges::Declined);
    let declined: Vec<_> = said(&mut lab)
        .into_iter()
        .filter(|line| line.contains("declined"))
        .collect();
    assert_eq!(declined.len(), 1, "{declined:?}");
}

// Spec (Judge workers are read-only; Which model judges): one judge on the
// review assignment, no lens, the level line before its verdict, and the pass
// line.
#[tokio::test]
async fn level_one_runs_one_read_only_judge_and_says_what_it_did() {
    let (mut lab, run, _h) = Batch::one(Some(true)).run(passes).await;
    let judges = judge_requests(&lab);
    assert_eq!(judges.len(), 1);
    assert_eq!(lab.seen.requests().iter().filter(|r| is_lens(r)).count(), 0);
    assert_eq!(judges[0].families, Toolset::ReadOnly.families());
    assert_eq!(judges[0].system, judge_builtin(Profile::Programmer));
    assert!(user_of(&judges[0]).contains("<untrusted"), "wrapped");
    let lines = said(&mut lab);
    let at = |needle: &str| lines.iter().position(|l| l.contains(needle));
    let level = at("judge: one judge");
    let verdict = at("judge 1/1: pass \u{b7} 5 criteria met");
    assert!(level < verdict && verdict.is_some(), "{lines:?}");
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::Clean));
    assert_eq!(kept[0].review.rounds().len(), 1);
    assert!(kept[0].review.judging().is_some_and(|j| j.granted));
}

// J14: a pass that could not verify a criterion says so, and still waits.
#[tokio::test]
async fn a_pass_with_an_unverified_criterion_names_it_in_its_line() {
    let (mut lab, _run, _h) = Batch::one(Some(true)).run(unverified).await;
    let lines = said(&mut lab);
    assert!(
        lines
            .iter()
            .any(|l| l == "judge 1/1: pass \u{b7} 4 met, 1 unverified (C1: tests not seen)"),
        "{lines:?}"
    );
}

// Spec (High tier runs both, one correction): lenses and judges start
// together, one correction covers a failed criterion, two judges on one model
// say they are not independent.
#[tokio::test]
async fn on_a_high_change_lenses_and_two_judges_run_and_one_correction_covers_both() {
    let (mut lab, run, _h) = Batch::high(Answered::Granted, Some(true)).run(fails).await;
    let requests = lab.seen.requests();
    assert_eq!(requests.iter().filter(|r| is_lens(r)).count(), 2);
    // Both failed, so both rule again over the corrected change.
    assert_eq!(judge_requests(&lab).len(), 4);
    assert_eq!(requests.iter().filter(|r| is_correction(r)).count(), 1);
    let lines = said(&mut lab);
    for want in [
        "judge: judges are not independent \u{b7} both on codex \u{203a} default",
        "judge 1/2: fail \u{b7} 1 of 5 criteria not met",
        "judge 1/2 \u{b7} C1 not met \u{b7} at: src/a.rs:4-6 \u{b7} the pin is missing",
        "judges agree: fail \u{b7} 1 criteria not met",
    ] {
        assert!(lines.iter().any(|l| l.contains(want)), "{want}: {lines:?}");
    }
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].review.rounds().len(), 2);
}

// Spec (Disagreement is visible and does not correct).
#[tokio::test]
async fn judges_that_disagree_start_no_correction_and_say_what_is_missing() {
    SPLIT.store(0, Ordering::SeqCst);
    let (mut lab, run, _h) = Batch::high(Answered::Granted, Some(true)).run(split).await;
    let requests = lab.seen.requests();
    assert_eq!(requests.iter().filter(|r| is_correction(r)).count(), 0);
    let lines = said(&mut lab);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("judges disagree \u{b7} judge 1") && l.contains("missing:")),
        "{lines:?}"
    );
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::BlockersOpen));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("blocker(s) open") && l.contains("at: src/a.rs:4-6")),
        "{lines:?}"
    );
}

// Spec (Unavailable judge engine is visible): a refused judge is no verdict
// with its reason, and the review still ends.
#[tokio::test]
async fn a_refused_judge_is_no_verdict_with_its_reason_and_does_not_halt_the_review() {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Code, "codex", "default"),
        (Task::Review, "gemini", "default"),
    ];
    let mut batch = Batch::one(Some(true));
    batch.roster = roster(
        &assigned,
        vec![found("claude", &[]), found("codex", &[])],
        JAIL,
    );
    let (mut lab, run, _h) = batch.run(passes).await;
    assert!(judge_requests(&lab).is_empty(), "nothing ran");
    let lines = said(&mut lab);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("judge 1/1: no verdict \u{b7} refused (")),
        "{lines:?}"
    );
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::Incomplete));
}

// Spec (Declined leaves changes not judged): no judge runs, the batch is
// `not judged` and the receipt holds the decline.
#[tokio::test]
async fn a_declined_judge_consent_runs_no_judge_and_marks_the_batch_not_judged() {
    let (mut lab, run, _h) = Batch::one(Some(false)).run(passes).await;
    assert_eq!(lab.seen.requests().len(), 1, "only the code worker");
    let lines = said(&mut lab);
    assert!(lines.iter().any(|l| l.contains("not judged")), "{lines:?}");
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::NotReviewed));
    assert!(kept[0].review.judging().is_some_and(|j| !j.granted));
}

// Spec (Declining the lens consent does not cancel the judges).
#[tokio::test]
async fn a_declined_lens_consent_still_runs_the_judges() {
    let (mut lab, _run, _h) = Batch::high(Answered::Declined, Some(true))
        .run(passes)
        .await;
    let requests = lab.seen.requests();
    assert_eq!(requests.iter().filter(|r| is_lens(r)).count(), 0);
    assert_eq!(judge_requests(&lab).len(), 2);
    let lines = said(&mut lab);
    assert!(
        lines.iter().any(|l| l.contains("judges agree: pass")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("lens review was declined")),
        "{lines:?}"
    );
}

// Spec (Mode inside the S7b review): a review with no judge consent and no
// lens consent runs nothing, as before this change.
#[tokio::test]
async fn a_lens_review_with_no_judge_answer_runs_no_judge() {
    let (mut lab, _run, _h) = Batch::high(Answered::Granted, None).run(passes).await;
    assert!(judge_requests(&lab).is_empty());
    let lines = said(&mut lab);
    assert!(!lines.iter().any(|l| l.contains("judge")), "{lines:?}");
}

// Spec (Judge mode): the orchestrator cannot spawn a judge by kind.
#[test]
fn the_orchestrator_cannot_spawn_a_judge_by_kind() {
    use crate::core::ai::orchestra::tools::{OrchError, parse};
    let args = serde_json::json!({"kind": "judge", "prompt": "rule it"});
    assert_eq!(parse("spawn_agent", &args), Err(OrchError::BadKind));
}

// Only `a_correction_says_what_it_fixes...` uses this flag, so no other test
// reads it while it changes.
static FIXED: AtomicBool = AtomicBool::new(false);

/// The judge fails until the correction ran, then passes.
fn fails_until_fixed(request: &ChatRequest) -> Option<&'static str> {
    if is_correction(request) {
        FIXED.store(true, Ordering::SeqCst);
        return Some("staged the fix in src/b.rs");
    }
    if is_judge(request) && FIXED.load(Ordering::SeqCst) {
        return Some(text_of(criteria_of(request), "pass"));
    }
    fails(request)
}

/// What is staged: one line before the correction, another after it.
fn staged_after_fix() -> StagedDiff {
    let mut diff = staged_text();
    if FIXED.load(Ordering::SeqCst) {
        diff.files[0].text = "yanked = 2\n".to_owned();
    }
    diff
}

fn at_of(lines: &[String], needle: &str) -> Option<usize> {
    lines.iter().position(|l| l.contains(needle))
}

// Spec (Corrections say what they change; Worker claim is not trusted): the
// target before, lazarobox's own `changes:` after (never the worker's `src/b.rs`),
// and the judge that failed rules again with its own lines.
#[tokio::test]
async fn a_correction_says_what_it_fixes_what_it_changed_and_the_judge_rules_again() {
    FIXED.store(false, Ordering::SeqCst);
    let mut batch = Batch::one(Some(true));
    batch.restage = Some(restaged_differently);
    batch.diff = staged_after_fix;
    let (mut lab, run, _h) = batch.run(fails_until_fixed).await;
    let lines = said(&mut lab);
    let before = at_of(
        &lines,
        "correction 1/3 \u{b7} fixes C1 \u{b7} at: src/a.rs:4-6",
    );
    let after = at_of(&lines, "correction 1/3 done \u{b7} changes: Cargo.lock 1-1");
    let again = at_of(&lines, "judge 1/1: pass");
    assert!(
        before < after && after < again && before.is_some(),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("src/b.rs")), "{lines:?}");
    assert_eq!(judge_requests(&lab).len(), 2);
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].review.rounds().len(), 2);
    assert_eq!(kept[0].outcome, Some(Ended::Corrected));
}

// Spec (a correction that changed nothing says so): and the failing judge
// still rules again; its fail keeps the criterion open.
#[tokio::test]
async fn a_correction_that_changed_nothing_says_none_and_a_failing_rejudge_keeps_it_open() {
    let mut batch = Batch::one(Some(true));
    batch.restage = Some(restaged_differently);
    let (mut lab, run, _h) = batch.run(fails).await;
    let lines = said(&mut lab);
    assert!(
        lines
            .iter()
            .any(|l| l == "correction 1/3 done \u{b7} changes: none"),
        "{lines:?}"
    );
    assert_eq!(judge_requests(&lab).len(), 2);
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::BlockersOpen));
}

// Spec (Exhausted rounds say what is missing): no correction starts, the
// line names the open criterion and what is missing.
#[tokio::test]
async fn exhausted_rounds_name_the_open_criterion_and_the_batch_reaches_the_user() {
    let mut batch = Batch::one(Some(true));
    batch.restage = Some(restaged_differently);
    batch.spent = 3;
    let (mut lab, run, _h) = batch.run(fails).await;
    assert!(lab.seen.requests().iter().all(|r| !is_correction(r)));
    let lines = said(&mut lab);
    let want = "judge: rounds exhausted (3) \u{b7} open: C1 \u{b7} missing: the pin is missing";
    assert!(lines.iter().any(|l| l == want), "{lines:?}");
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::BlockersOpen));
    assert_eq!(kept[0].review.limit(), Some(3));
}

// Run step trail (7.8c): a judge's start and verdict are events for the trail
// with fixed text only, before and after its lines.
#[tokio::test]
async fn each_judge_start_and_verdict_is_sent_for_the_step_trail() {
    let (mut lab, _run, _h) = Batch::one(Some(true)).run(fails).await;
    let steps: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|event| match event {
            OrchestraEvent::Judge {
                who,
                verdict,
                reason,
                ..
            } => Some((who, verdict, reason)),
            _ => None,
        })
        .collect();
    let want = |who: &str, verdict: &str, reason: &str| {
        (who.to_owned(), verdict.to_owned(), reason.to_owned())
    };
    assert_eq!(
        steps,
        [
            want("judge 1/1", "started", "read-only, over the staged change"),
            want("judge 1/1", "fail", "1 of 5 criteria not met"),
        ]
    );
}

// Review fix: the round that uses the last correction says the rounds are
// exhausted at once; no later review has to run for the user to see it.
#[tokio::test]
async fn the_last_correction_round_says_the_rounds_are_exhausted_when_it_fails() {
    let mut batch = Batch::one(Some(true));
    batch.restage = Some(restaged_differently);
    batch.spent = 2;
    let (mut lab, _run, _h) = batch.run(fails).await;
    let lines = said(&mut lab);
    let said_it: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("judge: rounds exhausted (3)"))
        .collect();
    assert_eq!(said_it.len(), 1, "{lines:?}");
    assert!(said_it[0].contains("open: C1"), "{said_it:?}");
}
