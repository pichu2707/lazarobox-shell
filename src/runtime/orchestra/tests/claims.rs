//! The claim check of a handoff at the run manager (agent-orchestrator [W4],
//! design D28): the ack, a later collect, the close report and the event the
//! stored note is made from.

use super::*;

fn claiming(id: u32, verdict: Verdict, done: &str) -> OrchCall {
    OrchCall::Handoff {
        id: WorkerId(id),
        note: Judgement {
            verdict,
            reason: "fits".to_owned(),
            done: done.to_owned(),
            left: "none".to_owned(),
            decisions: "none".to_owned(),
        },
    }
}

/// A `code` worker that ended without staging anything.
async fn empty_code_worker(lab: &Lab) -> OrchHandle {
    let (_, handle) = lab.start_open(codex_code());
    let spawned = handle.call(spawn(Task::Code, "write it")).await;
    assert!(!spawned.is_error, "{spawned:?}");
    assert_eq!(handle.call(wait(1)).await, fine("1: done"));
    handle
}

// Spec (W4, Claimed file not in the ledger): the ack names the flag, and the
// verdict stays the orchestrator's beside it.
#[tokio::test]
async fn the_handoff_ack_carries_the_flag_and_the_verdict() {
    let lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    let ack = handle
        .call(claiming(1, Verdict::OffCourse, "wrote src/b.rs"))
        .await;
    assert_eq!(
        ack,
        fine("handoff 1: off_course; named in handoff, not in ledger: src/b.rs")
    );
}

// Spec (W4, The orchestrator cannot clear a flag): a second save that says the
// flag is wrong keeps it, and the neutral second flag shows for on_course.
#[tokio::test]
async fn a_second_save_cannot_clear_the_flag() {
    let lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    handle
        .call(claiming(1, Verdict::OnCourse, "wrote src/b.rs"))
        .await;
    let ack = handle
        .call(claiming(1, Verdict::OnCourse, "the flag is wrong"))
        .await;
    assert_eq!(
        ack,
        fine(
            "handoff 1: on_course; named in handoff, not in ledger: src/b.rs; \
             on_course, nothing staged"
        )
    );
}

// Spec (W4, Nothing to check says so): never `ok`.
#[tokio::test]
async fn a_handoff_naming_no_file_is_not_checked() {
    let lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    let ack = handle
        .call(claiming(1, Verdict::OffCourse, "did the work"))
        .await;
    assert_eq!(
        ack,
        fine("handoff 1: off_course; not checked: no file named in the handoff")
    );
}

// Spec (W4, shown in `collect`): the first collect precedes the handoff and
// carries no check; a later one carries it, outside the wrapper.
#[tokio::test]
async fn a_later_collect_shows_the_check_outside_the_wrapper() {
    let lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    let before = handle.call(collect(1)).await;
    assert!(!before.text.contains("handoff check"), "{before:?}");
    handle
        .call(claiming(1, Verdict::OffCourse, "wrote src/b.rs"))
        .await;
    let after = handle.call(collect(1)).await;
    assert!(
        after
            .text
            .starts_with("lazarobox: handoff check: named in handoff, not in ledger: src/b.rs\n\n"),
        "{after:?}"
    );
    assert!(after.text.contains("all done"), "{after:?}");
}

// Bypass: a worker degraded to plain text (no tools, its reply holds a call as
// text) is checked like any other; only the calls-as-text line stays away.
#[tokio::test]
async fn a_degraded_worker_is_checked_too() {
    let call = r#"{"name": "write_file", "arguments": {"path": "a", "content": "x"}}"#;
    let lab = lab(Script::Answer(call));
    let handle = empty_code_worker(&lab).await;
    handle
        .call(claiming(1, Verdict::OffCourse, "created dir/a.txt"))
        .await;
    let got = handle.call(collect(1)).await.text;
    let open = got.find("<untrusted-tool-output").unwrap();
    assert!(
        got.find("named in handoff, not in ledger: dir/a.txt")
            .unwrap()
            < open,
        "{got}"
    );
    assert!(!got.contains("tool calls written as text"), "{got}");
}

// Spec (W4, shown in the close report): finishing sends the ledger with the
// flag under the worker row.
#[tokio::test]
async fn the_close_report_shows_the_flag_under_the_worker() {
    let mut lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    handle
        .call(claiming(1, Verdict::OffCourse, "wrote src/b.rs"))
        .await;
    let finish = OrchCall::Finish {
        summary: "s".to_owned(),
    };
    handle.call(finish).await;
    let ledgers: Vec<_> = lab
        .sent()
        .into_iter()
        .filter_map(|event| match event {
            OrchestraEvent::Closed { ledger, .. } => Some(ledger),
            _ => None,
        })
        .collect();
    let [ledger] = ledgers.as_slice() else {
        panic!("{ledgers:?}");
    };
    assert!(
        ledger.contains("\n  named in handoff, not in ledger: src/b.rs"),
        "{ledger}"
    );
    assert!(ledger.contains("Flags: 1"), "{ledger}");
}

// The event the stored note is made from holds the check.
#[tokio::test]
async fn the_handoff_event_holds_the_check_for_the_stored_note() {
    let lab = lab(Script::Answer("all done"));
    let handle = empty_code_worker(&lab).await;
    handle
        .call(claiming(1, Verdict::OffCourse, "wrote src/b.rs"))
        .await;
    let mut lab = lab;
    let sent = handoffs(&lab.sent());
    assert_eq!(sent.len(), 1);
    let audit = sent[0].audit.as_ref().expect("a worker's handoff");
    assert_eq!(audit.flags(), ["named in handoff, not in ledger: src/b.rs"]);
}

// Bypass: a worker that failed has its forced handoff checked like any other.
#[tokio::test]
async fn a_failed_workers_forced_handoff_is_checked_too() {
    let lab = lab(Script::FailOnce("boom"));
    let (_, handle) = lab.start_open(codex_code());
    handle.call(spawn(Task::Code, "write it")).await;
    handle.call(wait(1)).await;
    let ack = handle
        .call(claiming(1, Verdict::OnCourse, "wrote src/b.rs"))
        .await;
    assert!(!ack.is_error, "{ack:?}");
    assert!(
        ack.text
            .starts_with("handoff 1: blocked (forced: worker 1 failed); named in handoff"),
        "{ack:?}"
    );
}
