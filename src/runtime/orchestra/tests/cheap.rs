//! The mandatory review of a cheap author (agent-orchestrator "Mandatory
//! review for a cheap author, optional for a strong one" [W7], design D20,
//! D21, D6): a local author's staged change is reviewed by the Review row
//! with no consent ask, its clock stops for the review, and a reviewer that
//! cannot run stops the run visibly.

use super::*;

/// `code` on Ollama (a cheap author), `review` on `review`, `orchestrate` on
/// claude; claude, codex and Ollama are found.
fn local_code(review: (&str, &str)) -> Roster {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Code, "ollama", "llama3:8b"),
        (Task::Review, review.0, review.1),
    ];
    let engines = vec![
        found("claude", &[]),
        found("codex", &[]),
        found("ollama", &["llama3:8b"]),
    ];
    roster(&assigned, engines, JAIL)
}

// Spec "Menu change after spawn" (17.3): the run manager tells a cheap
// author by the spec it was spawned with. A menu change later does not make
// it strong; an unknown worker or another run is never cheap.
#[tokio::test]
async fn the_run_manager_tells_a_cheap_author_by_its_spec_as_spawned() {
    let lab = lab(Script::Hang);
    let limits = RunLimits {
        parallel: 2,
        ..RunLimits::default()
    };
    let scope = OrchScope {
        limits,
        ..lab.scope(local_code(("codex", "default")), &[])
    };
    let (run, handle) = lab.begin(scope).unwrap();
    handle.call(spawn(Task::Code, "write a")).await;
    handle.call(spawn(Task::Review, "look")).await;
    assert!(handle.cheap(run, WorkerId(1)));
    assert!(!handle.cheap(run, WorkerId(2)));
    lab.manager
        .on_run(pane(), run, |live| {
            live.run.set_roster(codex_both());
        })
        .unwrap();
    assert!(handle.cheap(run, WorkerId(1)), "the menu now is not read");
    assert!(!handle.cheap(run, WorkerId(9)));
    assert!(!handle.cheap(RunId(run.0 + 1), WorkerId(1)));
}

// Spec "Cheap author, no consent" (17.2, D21): a mandatory review runs its
// lens workers with no consent recorded as the user's, and its receipt and
// batch-review line say `mandatory`.
#[tokio::test]
async fn a_mandatory_review_runs_and_its_receipt_says_mandatory() {
    let (mut lab, run, handle) = reviewed(quiet, Answered::Mandatory, RunLimits::default()).await;
    let lenses = lab.seen.requests().iter().filter(|r| is_lens(r)).count();
    assert_eq!(lenses, 2);
    let kept = lab.manager.reviews(pane(), run);
    assert_eq!(kept[0].outcome, Some(Ended::Clean));
    let made = receipts(&mut lab);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        made[0].done.contains("consent: mandatory"),
        "{}",
        made[0].done
    );
    let line = handle
        .receipt_line(run, WorkerId(1), &lock_change())
        .expect("a line");
    assert!(line.contains("consent mandatory"), "{line}");
    assert!(line.contains("clean"), "{line}");
}
