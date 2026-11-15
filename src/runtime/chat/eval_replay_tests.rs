//! The replay guard of the local-model evaluation (openspec `local-multistep`):
//! scripted transcripts go through the real Ollama tool loop and the real
//! dispatch for files and batches, with fixed answers for commands and git, and
//! the scorer judges the exchanges. It checks the loop and the scorer, not the
//! quality of any model, and it is never reported as a score.

use std::time::Duration;

use serde_json::Value;

use super::{
    eval_harness::{Fixture, Folder, Rig, leaks, proxy, throwaway_root},
    ollama_tests::fake,
};
use crate::{
    core::ai::eval::{RunResult, table::Run},
    runtime::loopback::Timeouts,
};

const GOOD: &str = include_str!("../../../tests/fixtures/ollama/eval/fib-good.json");
const NO_PYTEST: &str = include_str!("../../../tests/fixtures/ollama/eval/fib-no-pytest.json");
const WRONG_MESSAGE: &str =
    include_str!("../../../tests/fixtures/ollama/eval/fib-wrong-message.json");
const COMMIT_FIRST: &str =
    include_str!("../../../tests/fixtures/ollama/eval/fib-commit-first.json");
const READ_ONLY_BATCH: &str =
    include_str!("../../../tests/fixtures/ollama/eval/read-only-batch.json");
const COORDINATE_GOOD: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-good.json");
const COORDINATE_PROSE: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-prose.json");
const COORDINATE_TEXT: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-text-call.json");

struct Replayed {
    run: Run,
    folder: Folder,
    last: Value,
}

async fn replay(raw: &str, fail: Option<&'static str>) -> Replayed {
    let fixture = Fixture::parse(raw).unwrap();
    let scenario = fixture.scenario();
    let fake = fake(fixture.steps()).await;
    let folder = Folder::new(&throwaway_root(), scenario);
    let rig = Rig::new(scenario, &folder, fail);
    let judge = rig.judge.clone();
    let answer = rig.drive(fake.port, &Timeouts::SERVER).await;
    let last = fake.bodies().last().cloned().expect("a request");
    Replayed {
        run: judge.run(&last, &answer, Duration::ZERO),
        folder,
        last,
    }
}

/// Nothing failed but what the test names.
fn only(result: &RunResult, missing: &[&str], order: &[(&str, &str)], ignored: &[&str]) {
    assert_eq!(result.missing, missing, "{result:?}");
    assert_eq!(result.out_of_order, order, "{result:?}");
    assert_eq!(result.ignored, ignored, "{result:?}");
    assert!(
        !result.asked_back && result.forbidden.is_empty(),
        "{result:?}"
    );
}

#[tokio::test]
async fn the_good_transcript_passes_and_its_files_reach_the_lab() {
    let replayed = replay(GOOD, None).await;
    assert!(replayed.run.passed(), "{:?}", replayed.run.result);
    assert_eq!(replayed.run.result.completed.len(), 5);
    assert!(!replayed.run.unexpected);
    // The files and the batch went through the real dispatch.
    let fib = std::fs::read_to_string(replayed.folder.repo.join("fib.py")).unwrap();
    assert!(fib.contains("def fib"), "{fib}");
    assert!(replayed.folder.repo.join("test_fib.py").is_file());
    let repo = replayed.folder.repo.clone();
    drop(replayed);
    assert!(!repo.exists(), "the lab is removed");
}

#[tokio::test]
async fn a_run_without_pytest_fails_for_the_missing_step_only() {
    let replayed = replay(NO_PYTEST, None).await;
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &["pytest"], &[], &[]);
}

#[tokio::test]
async fn a_commit_with_another_message_fails_for_the_literal_only() {
    let replayed = replay(WRONG_MESSAGE, None).await;
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &[], &[], &["add fibonacci"]);
}

// A commit made before the batch is also before the test run that follows
// the batch, so two pairs hold the commit: no transcript breaks one alone.
#[tokio::test]
async fn a_commit_before_the_batch_fails_for_the_order_only() {
    let replayed = replay(COMMIT_FIRST, None).await;
    assert!(!replayed.run.passed());
    let pairs = [("batch", "commit"), ("pytest", "commit")];
    only(&replayed.run.result, &[], &pairs, &[]);
    assert!(!replayed.run.unexpected);
}

// Task 3.0: a git failure with no refusal prefix is not a completed step.
#[tokio::test]
async fn a_failed_commit_is_not_a_completed_step() {
    let replayed = replay(GOOD, Some("git_commit")).await;
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &["commit"], &[], &[]);
}

// Spec (Unexpected approval denied): the batch of a read-only task is denied,
// nothing reaches the disk and the run is reported as unexpected.
#[tokio::test]
async fn a_batch_the_scenario_does_not_expect_is_denied_and_reported() {
    let replayed = replay(READ_ONLY_BATCH, None).await;
    assert!(replayed.run.unexpected);
    assert!(!replayed.run.passed());
    assert!(!replayed.folder.repo.join("extra.py").exists());
    let tool = replayed.last["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert!(
        tool["content"].as_str().unwrap().starts_with("denied: "),
        "{tool}"
    );
    assert_eq!(
        replayed.run.result.forbidden,
        ["write_file", "propose_batch"]
    );
}

#[tokio::test]
async fn a_coordinator_that_calls_the_tools_passes() {
    let replayed = replay(COORDINATE_GOOD, None).await;
    assert!(replayed.run.passed(), "{:?}", replayed.run.result);
    assert!(!replayed.run.unexpected);
}

#[tokio::test]
async fn a_coordinator_that_only_describes_the_calls_completes_nothing() {
    let all = ["authorize", "explore", "resolve", "classify", "spawn"];
    let replayed = replay(COORDINATE_PROSE, None).await;
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &all, &[], &[]);
}

#[tokio::test]
async fn a_coordinator_that_writes_the_calls_as_text_completes_nothing() {
    let all = ["authorize", "explore", "resolve", "classify", "spawn"];
    let replayed = replay(COORDINATE_TEXT, None).await;
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &all, &[], &[]);
}

// Spec (Fixture curation): the repository is public, so a fixture holds
// messages and tool calls only and nothing of the machine that made it.
fn shipped() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ollama/eval");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(path).unwrap())
        })
        .collect();
    files.sort();
    files
}

#[test]
fn fixtures_are_clean() {
    let files = shipped();
    assert!(files.len() >= 8, "{} fixtures", files.len());
    // The running machine's values, as the spec asks.
    let home = std::env::var("HOME").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    for (name, text) in &files {
        // The kind of leak only: never the matched value.
        let kinds = leaks(text, &home, &user);
        assert!(kinds.is_empty(), "{name}: {kinds:?}");
    }
}

#[test]
fn the_scan_names_the_kind_of_leak_and_never_the_value() {
    let good = r#"{"scenario": "read-only", "replies": [{"text": "see src/lib.rs"}]}"#;
    assert!(leaks(good, "/home/someone", "someone").is_empty());
    let planted = |text: &str| {
        let body = format!(r#"{{"scenario": "read-only", "replies": [{{"text": "{text}"}}]}}"#);
        leaks(&body, "/home/someone", "someone")
    };
    assert_eq!(planted("open /home/x/y.py now"), ["absolute path"]);
    assert_eq!(planted("open ~/notes.txt"), ["home path"]);
    assert_eq!(planted(r"open C:\\Users\\x"), ["windows path"]);
    assert_eq!(
        planted("the folder /home/someone"),
        ["absolute path", "home", "user"]
    );
    assert_eq!(planted("hello someone"), ["user"]);
    assert_eq!(planted("mail ana@example.com"), ["email"]);
    assert_eq!(planted("key sk-abcdefgh12345678"), ["token"]);
    assert_eq!(planted("key ghp_abcdefgh12345678"), ["token"]);
    assert_eq!(planted(&"a1".repeat(20)), ["token"]);
    let extra = r#"{"scenario": "read-only", "replies": [], "cwd": "."}"#;
    assert_eq!(leaks(extra, "/home/someone", "someone"), ["field"]);
    let big = format!(
        r#"{{"scenario": "read-only", "replies": [{{"text": "{}"}}]}}"#,
        "word ".repeat(4000)
    );
    assert_eq!(leaks(&big, "/home/someone", "someone"), ["size"]);
}

#[test]
fn a_fixture_with_a_field_outside_the_format_does_not_parse() {
    assert!(Fixture::parse(r#"{"scenario": "x", "replies": [], "model": "m"}"#).is_err());
    assert!(Fixture::parse(r#"{"scenario": "x", "replies": [{"text": "", "id": 1}]}"#).is_err());
    assert!(
        Fixture::parse(r#"{"scenario": "x", "replies": [{"text": "", "calls": [{"name": "n"}]}]}"#)
            .is_err()
    );
}

// The live harness reads its exchanges through this proxy: it must record the
// bodies and pass the replies through byte for byte.
#[tokio::test]
async fn a_run_through_the_recording_proxy_scores_like_a_direct_run() {
    let fixture = Fixture::parse(GOOD).unwrap();
    let scenario = fixture.scenario();
    let server = fake(fixture.steps()).await;
    let upstream: &'static str = Box::leak(format!("127.0.0.1:{}", server.port).into_boxed_str());
    let (port, bodies) = proxy(upstream).await;
    let folder = Folder::new(&throwaway_root(), scenario);
    let rig = Rig::new(scenario, &folder, None);
    let judge = rig.judge.clone();
    let answer = rig.drive(port, &Timeouts::SERVER).await;
    let last = bodies
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("a recorded body");
    assert_eq!(bodies.lock().unwrap().len(), server.bodies().len());
    assert_eq!(last, *server.bodies().last().unwrap());
    let run = judge.run(&last, &answer, Duration::ZERO);
    assert!(run.passed(), "{:?}", run.result);
    assert!(answer.starts_with("Done"), "{answer}");
}
