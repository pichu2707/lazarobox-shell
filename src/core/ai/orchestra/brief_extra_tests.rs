//! The brief's last checks (W8b, tasks 19.5 and 19.7): two `simple` workers
//! never write the same file at once (D10), and the extra fields of a brief
//! are flagged in lazarobox's own words in `collect` and in the handoff.

use std::path::Path;
use std::time::{Duration, Instant};

use super::brief::Brief;
use super::ledger::{Judgement, Verdict};
use super::persist::Item;
use super::run::{Refusal, Roster, Run, RunLimits, WorkerEvent};
use super::worker::WorkerStatus;
use super::{RunId, WorkerId};
use crate::core::ai::chat::engine::Availability;
use crate::core::ai::config::Task;
use crate::core::ai::tools::registry::FamilySet;

fn run() -> Run {
    let ready = Availability::Ready {
        engine: "ollama".to_owned(),
        model: Some("llama3:8b".to_owned()),
    };
    let roster = Roster::new(Task::ALL.into_iter().map(|t| (t, ready.clone())).collect());
    let mut run = Run::new(RunId(1), roster, Duration::from_secs(600)).with_ceiling(FamilySet::ALL);
    run.close_steps_for_tests();
    run.with_limits(RunLimits {
        parallel: 1,
        ..RunLimits::default()
    })
}

fn brief(outputs: &[&str]) -> Brief {
    Brief {
        goal: "add a flag".to_owned(),
        inputs: vec!["src/a.rs".to_owned()],
        outputs: Some(outputs.iter().map(|p| (*p).to_owned()).collect()),
        done_when: "it parses".to_owned(),
        out_of_scope: "docs".to_owned(),
        ..Brief::default()
    }
}

fn spawn(run: &mut Run, brief: Brief) -> Result<WorkerId, Refusal> {
    run.spawn_simple(Instant::now(), brief, Path::new("/work/app"))
}

fn with_priority() -> Brief {
    Brief {
        extra: vec![("priority".to_owned(), "high".to_owned())],
        ..brief(&["src/b.rs"])
    }
}

fn note() -> Judgement {
    Judgement {
        verdict: Verdict::OnCourse,
        reason: "fits".to_owned(),
        done: "added it".to_owned(),
        left: "none".to_owned(),
        decisions: "none".to_owned(),
    }
}

fn done(run: &mut Run, id: WorkerId) {
    run.on_event(Instant::now(), id, WorkerEvent::Done { text: "ok".into() });
}

// 19.5 (D10): a live worker's outputs block a second `simple`, by name.
#[test]
fn outputs_that_overlap_a_live_worker_are_refused_naming_it() {
    let mut run = run();
    let first = spawn(&mut run, brief(&["src/b.rs", "src/c.rs"])).unwrap();
    // The first runs and the second queues: both are live.
    let queued = spawn(&mut run, brief(&["src/z.rs"])).unwrap();
    assert_eq!(run.status(queued), Some(WorkerStatus::Queued));
    let refusal = spawn(&mut run, brief(&["docs/x.md", "src/c.rs"])).unwrap_err();
    assert_eq!(
        refusal,
        Refusal::Overlap {
            worker: first,
            path: "src/c.rs".to_owned(),
            held: "src/c.rs".to_owned()
        }
    );
    let refusal = spawn(&mut run, brief(&["./src/z.rs"])).unwrap_err();
    assert_eq!(
        refusal,
        Refusal::Overlap {
            worker: queued,
            path: "./src/z.rs".to_owned(),
            held: "src/z.rs".to_owned()
        }
    );
    assert_eq!(run.worker_count(), 2);
}

// A directory output covers the files under it.
#[test]
fn a_directory_output_overlaps_the_files_under_it() {
    let mut run = run();
    let first = spawn(&mut run, brief(&["src/"])).unwrap();
    assert!(matches!(
        spawn(&mut run, brief(&["src/b.rs"])),
        Err(Refusal::Overlap { worker, .. }) if worker == first
    ));
    // A sibling whose name only starts the same does not overlap.
    assert!(spawn(&mut run, brief(&["src2/b.rs"])).is_ok());
}

// 19.5: an ended worker's outputs do not block; disjoint outputs both start.
#[test]
fn an_ended_worker_does_not_block_and_disjoint_outputs_both_start() {
    let mut run = run();
    let first = spawn(&mut run, brief(&["src/b.rs"])).unwrap();
    assert!(spawn(&mut run, brief(&["src/other.rs"])).is_ok());
    done(&mut run, first);
    assert!(spawn(&mut run, brief(&["src/b.rs"])).is_ok());
}

// 19.5: a worker with no outputs never overlaps.
#[test]
fn a_brief_with_no_outputs_never_overlaps() {
    let mut run = run();
    spawn(&mut run, brief(&["src/b.rs"])).unwrap();
    assert!(spawn(&mut run, brief(&[])).is_ok());
}

// 19.5: a `code` worker has no brief: apply-time drift protects it.
#[test]
fn a_code_worker_is_not_checked_for_overlap() {
    let mut run = run();
    spawn(&mut run, brief(&["src/b.rs"])).unwrap();
    assert!(
        run.spawn(Instant::now(), Task::Code, "edit src/b.rs")
            .is_ok()
    );
}

// 19.7: `collect` says which fields were extra, outside the worker's wrapper.
#[test]
fn collect_flags_the_extra_fields_outside_the_wrapper() {
    let mut run = run();
    let id = spawn(&mut run, with_priority()).unwrap();
    done(&mut run, id);
    let text = run.collect_wrapped([7; 8], id).unwrap();
    let flag = "lazarobox: brief extra fields: priority";
    assert!(text.starts_with(flag), "{text}");
    assert!(text.contains("\n\n"), "{text}");
    // A brief with no extra field adds nothing.
    let plain = spawn(&mut run, brief(&["src/z.rs"])).unwrap();
    done(&mut run, plain);
    let text = run.collect_wrapped([7; 8], plain).unwrap();
    assert!(!text.contains("extra fields"), "{text}");
}

// 19.7: the handoff records them, in the run and in the stored note.
#[test]
fn the_handoff_records_the_extra_fields() {
    let mut run = run();
    let id = spawn(&mut run, with_priority()).unwrap();
    done(&mut run, id);
    let handoff = run.handoff(id, note()).unwrap();
    assert_eq!(handoff.extra, "priority");
    let content = Item::of_handoff(&handoff).note.content;
    assert!(
        content.contains("\nbrief extra fields: priority"),
        "{content}"
    );
    // Without one, the note has no such line.
    let plain = spawn(&mut run, brief(&["src/z.rs"])).unwrap();
    done(&mut run, plain);
    let handoff = run.handoff(plain, note()).unwrap();
    assert!(handoff.extra.is_empty());
    assert!(
        !Item::of_handoff(&handoff)
            .note
            .content
            .contains("extra fields")
    );
}

// Review: `path_ok` accepts an absolute path under the chat origin, so the
// same file written both ways is the same output. Directory against file too.
#[test]
fn an_absolute_path_under_the_origin_overlaps_its_relative_form() {
    let mut run = run();
    let first = spawn(&mut run, brief(&["src/b.rs", "lib/"])).unwrap();
    for path in ["/work/app/src/b.rs", "/work/app/lib/x.rs"] {
        assert!(
            matches!(
                spawn(&mut run, brief(&[path])),
                Err(Refusal::Overlap { worker, .. }) if worker == first
            ),
            "{path}"
        );
    }
    // A directory given as absolute covers a relative file.
    let mut run = self::run();
    let first = spawn(&mut run, brief(&["/work/app/pkg"])).unwrap();
    assert!(matches!(
        spawn(&mut run, brief(&["pkg/m.rs"])),
        Err(Refusal::Overlap { worker, .. }) if worker == first
    ));
    assert!(spawn(&mut run, brief(&["/work/app/other.rs"])).is_ok());
}

// Review: the same file on a case-insensitive filesystem. Over-refusing two
// names that differ only in case on Linux is accepted.
#[test]
fn outputs_that_differ_only_in_case_overlap() {
    let mut run = run();
    let first = spawn(&mut run, brief(&["src/Readme.md"])).unwrap();
    assert!(matches!(
        spawn(&mut run, brief(&["SRC/README.md"])),
        Err(Refusal::Overlap { worker, .. }) if worker == first
    ));
}
