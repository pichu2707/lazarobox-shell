//! Clarification rounds at the run manager (agent-orchestrator [W8c], design
//! D4): a spawn that skips `answers` is refused with the same text in the
//! pane, the third round and the turn cap stop the run with a visible line.

use super::*;

fn open(lab: &Lab, turns: usize) -> (RunId, OrchHandle) {
    let scope = OrchScope {
        origin: lab.app(),
        ceiling: FamilySet::ALL,
        limits: RunLimits {
            turns,
            parallel: 4,
            ..RunLimits::default()
        },
        ..lab.scope(simple_roster(), &["~/dev/app"])
    };
    lab.begin(scope).unwrap()
}

/// Worker `id` calls `needs_specification` and ends, as the host would.
fn ask(lab: &Lab, run: RunId, id: u32) {
    lab.manager.on_run(pane(), run, |live| {
        assert!(live.run.clarify(WorkerId(id), "which file?"));
        let done = WorkerEvent::Done { text: "stopped".into() };
        live.run.on_event(std::time::Instant::now(), WorkerId(id), done);
    });
}

fn answering(output: &str, answers: u32) -> OrchCall {
    let mut brief = writing_brief(output);
    brief.answers = Some(answers);
    OrchCall::SpawnSimple { brief }
}

fn stops(lab: &mut Lab) -> Vec<String> {
    let events = lab.sent();
    events
        .into_iter()
        .filter_map(|event| match event {
            OrchestraEvent::Stopped { reason, .. } => Some(reason),
            _ => None,
        })
        .collect()
}

// Spec ("Two rounds allowed", D4): a spawn that skips an open clarification
// is refused naming the worker, the pane hears the same text, and the spawn
// that answers goes through.
#[tokio::test]
async fn a_spawn_that_skips_an_open_clarification_is_refused_and_shown() {
    let mut lab = writing_lab(Script::Hang);
    let (run, handle) = open(&lab, 8);
    let first = OrchCall::SpawnSimple {
        brief: writing_brief("src/a.rs"),
    };
    assert_eq!(handle.call(first).await, fine("worker 1: running"));
    ask(&lab, run, 1);
    let skipped = OrchCall::SpawnSimple {
        brief: writing_brief("src/a.rs"),
    };
    let refused = handle.call(skipped).await;
    assert!(refused.is_error, "{refused:?}");
    assert!(refused.text.contains("answers: 1"), "{}", refused.text);
    assert_eq!(
        refusal(&mut lab).await,
        OrchestraEvent::SpawnRefused {
            run,
            kind: Task::Simple,
            reason: refused.text,
        }
    );
    let answered = handle.call(answering("src/a.rs", 1)).await;
    assert_eq!(answered, fine("worker 2: running"));
}

// Spec ("Third round stops the run"): the third `needs_specification` of a
// brief ends the run with a line naming the brief and the limit.
#[tokio::test]
async fn the_third_round_stops_the_run_naming_the_brief_and_the_limit() {
    let mut lab = writing_lab(Script::Hang);
    let (run, handle) = open(&lab, 8);
    let first = OrchCall::SpawnSimple {
        brief: writing_brief("src/a.rs"),
    };
    handle.call(first).await;
    ask(&lab, run, 1);
    handle.call(answering("src/a.rs", 1)).await;
    ask(&lab, run, 2);
    handle.call(answering("src/a.rs", 2)).await;
    ask(&lab, run, 3);
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let reasons = stops(&mut lab);
    let [reason] = reasons.as_slice() else {
        panic!("{reasons:?}");
    };
    assert!(reason.contains("brief 1"), "{reason}");
    assert!(reason.contains("2 rounds"), "{reason}");
    assert!(reason.is_ascii(), "{reason}");
}

// Open point "turn-cap interaction": a run stuck in an open clarification
// still stops at `max_turns`, and the line names the clarification.
#[tokio::test]
async fn the_turn_cap_names_an_open_clarification() {
    let mut lab = writing_lab(Script::Hang);
    let (run, handle) = open(&lab, 1);
    let first = OrchCall::SpawnSimple {
        brief: writing_brief("src/a.rs"),
    };
    handle.call(first).await;
    ask(&lab, run, 1);
    lab.sent();
    lab.manager.turn_ended(pane(), run);
    let reasons = stops(&mut lab);
    let [reason] = reasons.as_slice() else {
        panic!("{reasons:?}");
    };
    assert!(reason.contains("max_turns (1)"), "{reason}");
    assert!(reason.contains("brief 1 still waits"), "{reason}");
    assert!(reason.contains("round 1 of 2"), "{reason}");
}
