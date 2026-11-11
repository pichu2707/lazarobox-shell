//! The limit ask of a `simple` worker (W9b, tasks 22.1, 22.4, 22.6, 22.7):
//! what the run lists as over the limit, the split it records and the ask
//! the user reads.

use std::time::Instant;

use super::WorkerKey;
use super::clarify_tests::{done, run, simple};
use super::limit::{Count, count, limit_ask};
use super::worker::Closing;
use crate::core::ai::chat::approval::ApprovalAsk;
use crate::core::ai::config::Task;

fn counted(path: &str, n: usize) -> (String, Count) {
    let text = "let x = 1;\n".repeat(n);
    (path.to_owned(), count(path, None, &text))
}

fn listed(over: Vec<(String, Count)>) -> Vec<(String, usize)> {
    over.into_iter().map(|(p, c)| (p, c.lines)).collect()
}

#[test]
fn a_simple_workers_file_over_400_is_listed_and_400_is_not() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    let over = run.over_limit(id, &[counted("src/a.rs", 401), counted("src/b.rs", 400)]);
    assert_eq!(listed(over), [("src/a.rs".to_owned(), 401)]);
}

#[test]
fn a_code_worker_is_never_listed() {
    let mut run = run();
    let id = run.spawn(Instant::now(), Task::Code, "p").unwrap();
    assert!(run.over_limit(id, &[counted("src/a.rs", 900)]).is_empty());
}

#[test]
fn a_test_file_has_no_cap() {
    let mut run = run();
    let id = simple(&mut run, &["tests/a.rs"]);
    assert!(run.over_limit(id, &[counted("tests/a.rs", 900)]).is_empty());
}

#[test]
fn applied_batches_sum_and_a_batch_not_applied_adds_nothing() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    assert!(run.over_limit(id, &[counted("src/a.rs", 300)]).is_empty());
    // Proposed and never applied: the next batch starts from nothing.
    assert!(run.over_limit(id, &[counted("src/a.rs", 101)]).is_empty());
    assert!(run.over_limit(id, &[counted("src/a.rs", 300)]).is_empty());
    run.applied(id, &["src/a.rs".to_owned()]);
    let over = run.over_limit(id, &[counted("src/a.rs", 101)]);
    assert_eq!(listed(over), [("src/a.rs".to_owned(), 401)]);
}

#[test]
fn another_worker_starts_from_zero() {
    let mut run = run();
    let first = simple(&mut run, &["src/a.rs"]);
    assert!(
        run.over_limit(first, &[counted("src/a.rs", 300)])
            .is_empty()
    );
    run.applied(first, &["src/a.rs".to_owned()]);
    let second = simple(&mut run, &["src/b.rs"]);
    assert!(
        run.over_limit(second, &[counted("src/a.rs", 300)])
            .is_empty()
    );
}

#[test]
fn a_split_is_recorded_once_and_ends_the_workers_writes() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    let files = vec!["src/a.rs".to_owned()];
    assert!(run.split(id, &files));
    assert_eq!(run.closing(id), Some(&Closing::Split(files.clone())));
    assert!(run.clarifying(id));
    assert!(!run.split(id, &files), "the first closing stands");
}

// 22.7: a retry drops the split of the failed attempt, and what its brief
// already applied is still counted: lines on the disk do not vanish.
#[test]
fn a_retry_drops_the_split_and_keeps_the_applied_lines() {
    use super::run::{UserChoice, WorkerEvent};
    use super::worker::ErrorClass;
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    assert!(run.over_limit(id, &[counted("src/a.rs", 300)]).is_empty());
    run.applied(id, &["src/a.rs".to_owned()]);
    assert!(run.split(id, &["src/a.rs".to_owned()]));
    let failed = WorkerEvent::Failed {
        class: ErrorClass::Other,
        text: "boom".into(),
    };
    run.on_event(Instant::now(), id, failed);
    run.decide(Instant::now(), UserChoice::Retry).unwrap();
    assert!(!run.split_chosen(id));
    let over = run.over_limit(id, &[counted("src/a.rs", 101)]);
    assert_eq!(listed(over), [("src/a.rs".to_owned(), 401)]);
}

// 22.5 (C): the staged files the brief does not list, for a `simple` only;
// an output may be a folder; another kind has no brief to leave.
#[test]
fn staged_files_the_brief_does_not_list_are_outside_it() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs", "docs"]);
    let staged = ["src/a.rs", "docs/x.md", "src/b.rs", "Cargo.toml"].map(str::to_owned);
    assert_eq!(run.outside_brief(id, &staged), ["src/b.rs", "Cargo.toml"]);
    let code = run.spawn(Instant::now(), Task::Code, "p").unwrap();
    assert!(run.outside_brief(code, &staged).is_empty());
}

// 22.7: only a split closing is a split; a clarification is not.
#[test]
fn only_a_split_closing_is_split_chosen() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    assert!(!run.split_chosen(id));
    assert!(run.split(id, &["src/a.rs".to_owned()]));
    assert!(run.split_chosen(id));
    let other = simple(&mut run, &["src/z.rs"]);
    assert!(run.clarify(other, "what?"));
    assert!(!run.split_chosen(other));
}

#[test]
fn the_coordinator_is_told_of_the_split() {
    let mut run = run();
    let id = simple(&mut run, &["src/a.rs"]);
    assert!(run.split(id, &["src/a.rs".to_owned()]));
    done(&mut run, id, "stopped");
    let text = run.collect_wrapped([1; 8], id).unwrap();
    assert!(text.contains("split chosen: src/a.rs"), "{text}");
}

fn counts(lines: usize, every_line: bool) -> Count {
    Count {
        lines,
        every_line,
        test: false,
    }
}

// 22.6: the ask is a worker ask with clean text, aimed at the worker.
#[test]
fn the_ask_is_a_worker_ask_with_clean_text() {
    let key = WorkerKey::for_test(1, 2);
    let over = [
        ("src/a.rs".to_owned(), counts(450, false)),
        ("notes\n.xyz".to_owned(), counts(500, true)),
    ];
    let ask = limit_ask(key, "1/2 simple x>y\n", &over, &["src/b.rs".to_owned()]);
    assert!(ask.is_worker());
    assert_eq!(ask.worker(), Some(key));
    let ApprovalAsk::Limit {
        label,
        over,
        outside,
        ..
    } = ask
    else {
        panic!("{ask:?}");
    };
    assert_eq!(label, "1/2 simple x>y");
    assert_eq!((over[0].path.as_str(), over[0].lines), ("src/a.rs", 450));
    assert!(!over[0].every_line && over[1].every_line);
    assert!(!over[1].path.contains('\n'));
    assert_eq!(outside, ["src/b.rs"]);
}
