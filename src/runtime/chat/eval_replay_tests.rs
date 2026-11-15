//! The replay guard of the local-model evaluation (openspec `local-multistep`):
//! scripted transcripts go through the real Ollama tool loop and the real
//! dispatch for files and batches, with fixed answers for commands and git, and
//! the scorer judges the exchanges. It checks the loop and the scorer, not the
//! quality of any model, and it is never reported as a score.

use std::time::Duration;

use serde_json::Value;

use super::{
    eval_harness::{
        EXIT_NONZERO, Fixture, Folder, Mode, Rig, helper_age_warning, leaks, proxy, pytest_probe,
        sandbox_probe, throwaway_root,
    },
    ollama_tests::fake,
};
use crate::{
    core::ai::{
        eval::{RunResult, Scenario, scenarios, table::Run},
        tools::Call,
    },
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
const PYTEST_FAILS: &str =
    include_str!("../../../tests/fixtures/ollama/eval/fib-pytest-fails.json");
const COORDINATE_GOOD: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-good.json");
const COORDINATE_PROSE: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-prose.json");
const COORDINATE_TEXT: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-text-call.json");
const COORDINATE_TOOL_KEY: &str =
    include_str!("../../../tests/fixtures/ollama/eval/coordinate-tool-key.json");

struct Replayed {
    /// How many requests the model server got.
    requests: usize,
    run: Run,
    folder: Folder,
    last: Value,
}

async fn replay(raw: &str, fail: Option<&'static str>) -> Replayed {
    let fixture = Fixture::parse(raw).unwrap();
    let scenario = fixture.scenario();
    let fake = fake(fixture.steps()).await;
    let folder = Folder::new(&throwaway_root(), scenario);
    let rig = Rig::new(scenario, &folder, fail, Mode::Replay);
    let judge = rig.judge.clone();
    let answer = rig.drive(fake.port, &Timeouts::SERVER).await;
    let last = fake.bodies().last().cloned().expect("a request");
    Replayed {
        requests: fake.bodies().len(),
        run: judge.run(&last, &answer, Duration::ZERO),
        folder,
        last,
    }
}

/// Nothing failed but what the test names.
fn only(result: &RunResult, missing: &[&str], order: &[(&str, &str)], ignored: &[&str]) {
    assert_eq!(result.missing, missing, "{result:?}");
    assert!(result.failed.is_empty(), "{result:?}");
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

// Hardening: the same transcript as the good run, but pytest exits 1: the run
// fails because the step did not pass (`failed`), not because it was skipped.
#[tokio::test]
async fn a_test_run_that_exits_nonzero_fails_for_that_step_only() {
    let replayed = replay(PYTEST_FAILS, Some(EXIT_NONZERO)).await;
    assert!(!replayed.run.passed());
    let result = &replayed.run.result;
    assert_eq!(result.failed, ["pytest"], "{result:?}");
    assert!(result.missing.is_empty() && result.out_of_order.is_empty());
    assert!(result.ignored.is_empty() && result.forbidden.is_empty());
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

// Spec (chat-engines, "The `tool` key gets the reminder"): a call written as
// `{"tool": ..}` is reminded once, like the `name` form, and the model's
// reply to the reminder is the second request. Nothing in the text is run.
#[tokio::test]
async fn a_coordinator_that_writes_a_tool_key_call_is_reminded_once() {
    let all = ["authorize", "explore", "resolve", "classify", "spawn"];
    let replayed = replay(COORDINATE_TOOL_KEY, None).await;
    assert_eq!(replayed.requests, 2, "one reminder");
    let reminder = crate::core::ai::chat::text_call::reminder("spawn_agent");
    assert!(
        replayed.last.to_string().contains(&reminder),
        "{}",
        replayed.last
    );
    assert!(!replayed.run.passed());
    only(&replayed.run.result, &all, &[], &[]);
    // The `name` form of the same call gets the same single reminder.
    assert_eq!(replay(COORDINATE_TEXT, None).await.requests, 2);
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
    let rig = Rig::new(scenario, &folder, None, Mode::Replay);
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

// Verify W1: the live rig runs commands and git for real in the lab; only the
// replay rig answers them with fixed text. The helper is a script that does
// not sandbox, so the test needs git and a shell, not Landlock.
const STAND_IN: &str =
    "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n";

fn live_rig(folder: &Folder) -> Rig {
    live_rig_for(&crate::core::ai::eval::scenarios::WRITE_AND_TEST, folder)
}

/// A helper script `body` in the lab, ready to run.
fn script(folder: &Folder, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let helper = folder.home.parent().unwrap().join("helper");
    std::fs::write(&helper, body).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    helper
}

fn live_rig_for(scenario: &'static Scenario, folder: &Folder) -> Rig {
    let helper = script(folder, STAND_IN);
    Rig::new(scenario, folder, None, Mode::StandIn(helper))
}

#[tokio::test]
async fn the_live_rig_runs_git_in_the_lab_and_the_replay_rig_does_not() {
    let scenario = &crate::core::ai::eval::scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), scenario);
    folder.init_git();
    let log = || Call::GitLog {
        max_count: None,
        path: None,
    };
    let replay = Rig::new(scenario, &folder, None, Mode::Replay);
    assert_eq!(
        replay.dispatch().call(log()).await.text,
        "nothing to report"
    );
    let real = live_rig(&folder).dispatch().call(log()).await.text;
    assert!(real.contains("eval seed"), "{real}");
}

#[tokio::test]
async fn a_real_failure_in_the_live_rig_is_recorded_and_fails_the_step() {
    let scenario = &crate::core::ai::eval::scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), scenario);
    folder.init_git();
    let rig = live_rig(&folder);
    // Nothing is staged: the real commit fails.
    let call = Call::GitCommit {
        message: "add fibonacci".into(),
    };
    let outcome = rig.dispatch().call(call).await;
    assert!(outcome.is_error, "{outcome:?}");
    let body = serde_json::json!({"messages": [
        {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "git_commit", "arguments": {"message": "add fibonacci"}}}]},
        {"role": "tool", "content": outcome.text, "tool_name": "git_commit"}]});
    let run = rig.judge.run(&body, "Done.", Duration::ZERO);
    assert!(run.result.missing.contains(&"commit"), "{:?}", run.result);
}

/// Like [`STAND_IN`], but answers `pytest` itself  with a text only a real helper gives.
const STAND_IN_PYTEST: &str = "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\ncase \"$1\" in *cargo|*rustup) echo real-helper-ran; exit 0;; esac\nexec \"$@\"\n";

fn pytest() -> Call {
    Call::RunCommand {
        args: serde_json::json!({"program": "cargo", "args": ["fmt", "--check"]}),
    }
}

fn pytest_rig(scenario: &'static Scenario, folder: &Folder) -> Rig {
    let helper = script(folder, STAND_IN_PYTEST);
    Rig::new(scenario, folder, None, Mode::StandIn(helper))
}

// Hardening: in a live-style rig `run_command` and `git_add` are approved by
// the Judge (a scenario without them is denied and flagged), and the
// orchestration tools stay `recorded`.
#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "run_command through the stand-in helper needs the Linux sandbox, like the live harness"
)]
async fn in_the_live_rig_commands_and_git_add_go_through_the_judge() {
    let write = &scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), write);
    folder.init_git();
    let rig = pytest_rig(write, &folder);
    let out = rig.dispatch().call(pytest()).await;
    assert!(
        !out.is_error && out.text.contains("real-helper-ran"),
        "{out:?}"
    );
    assert!(!out.text.contains("1 passed"), "{out:?}");
    std::fs::write(folder.repo.join("fib.py"), "x = 1\n").unwrap();
    let add = Call::GitAdd {
        paths: vec!["fib.py".into()],
    };
    let out = rig.dispatch().call(add).await;
    assert!(!out.is_error && out.text != "staged 2 paths", "{out:?}");
    let body = serde_json::json!({});
    assert!(!rig.judge.run(&body, "", Duration::ZERO).unexpected);

    // A scenario with neither step: the same calls reach the Judge and lose.
    let read = &scenarios::READ_ONLY;
    let folder = Folder::new(&throwaway_root(), read);
    folder.init_git();
    let rig = pytest_rig(read, &folder);
    assert!(rig.dispatch().call(pytest()).await.is_error);
    let add = Call::GitAdd {
        paths: vec!["README.md".into()],
    };
    assert!(rig.dispatch().call(add).await.is_error);
    assert!(rig.judge.run(&body, "", Duration::ZERO).unexpected);

    // Orchestration tools never reach the host, live or not.
    let folder = Folder::new(&throwaway_root(), &scenarios::COORDINATE);
    let rig = live_rig_for(&scenarios::COORDINATE, &folder);
    let cancel = Call::Orchestra(crate::core::ai::orchestra::tools::OrchCall::Cancel {
        id: crate::core::ai::orchestra::WorkerId(1),
    });
    let out = rig.dispatch().call(cancel).await;
    assert_eq!((out.text.as_str(), out.is_error), ("recorded", false));
}

// Hardening: a sandbox that cannot run a command (no Landlock) must fail the
// pre-check naming the cause, not score every run_command as a failed step.
#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "run_command through the stand-in helper needs the Linux sandbox, like the live harness"
)]
async fn the_probe_passes_when_the_helper_runs_a_command() {
    let scenario = &scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), scenario);
    let rig = pytest_rig(scenario, &folder);
    assert_eq!(
        sandbox_probe(&*rig.dispatch(), "cargo", &["fmt", "--check"]).await,
        Ok(())
    );
}

#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "run_command through the stand-in helper needs the Linux sandbox, like the live harness"
)]
async fn the_probe_names_the_cause_when_the_helper_cannot_run_a_command() {
    let scenario = &scenarios::WRITE_AND_TEST;
    let folder = Folder::new(&throwaway_root(), scenario);
    let broken = "#!/bin/sh\necho 'landlock: not supported by this kernel' >&2\nexit 1\n";
    let helper = script(&folder, broken);
    let rig = Rig::new(scenario, &folder, None, Mode::StandIn(helper));
    let why = sandbox_probe(&*rig.dispatch(), "cargo", &["fmt", "--check"])
        .await
        .unwrap_err();
    assert!(why.contains("sandbox"), "{why}");
    assert!(why.contains("landlock: not supported"), "{why}");
}

#[tokio::test]
async fn the_probe_fails_on_any_failing_runner() {
    struct Failing;
    impl crate::runtime::mcp::Dispatch for Failing {
        fn call(
            &self,
            _: Call,
        ) -> futures::future::BoxFuture<'static, crate::runtime::mcp::Outcome> {
            Box::pin(async {
                crate::runtime::mcp::Outcome {
                    text: "sandbox unavailable on this kernel".into(),
                    is_error: true,
                }
            })
        }
    }
    let why = sandbox_probe(&Failing, "cargo", &["fmt", "--check"])
        .await
        .unwrap_err();
    assert!(why.contains("sandbox unavailable on this kernel"), "{why}");
}

// Hardening: the helper is stale when a source file is newer than it (`cargo
// test --lib` does not rebuild it), whatever order cargo linked the binaries.
#[test]
fn a_helper_older_than_a_source_file_is_warned_about() {
    use std::{
        fs::File,
        time::{Duration, SystemTime},
    };
    let folder = Folder::new(&throwaway_root(), &scenarios::READ_ONLY);
    let root = folder.home.parent().unwrap().join("crate");
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    let helper = root.join("lazarobox");
    let now = SystemTime::now();
    let touch = |path: std::path::PathBuf, age: u64| {
        File::create(&path).unwrap();
        let file = File::options().write(true).open(&path).unwrap();
        file.set_modified(now - Duration::from_secs(age)).unwrap();
    };
    touch(root.join("Cargo.toml"), 300);
    touch(root.join("Cargo.lock"), 300);
    touch(root.join("src/deep/a.rs"), 200);
    touch(helper.clone(), 100);
    assert_eq!(helper_age_warning(&helper, &root), None, "fresh helper");
    touch(root.join("src/deep/b.rs"), 10);
    let warning = helper_age_warning(&helper, &root).expect("a warning");
    assert!(warning.contains("cargo build"), "{warning}");
    touch(root.join("src/deep/b.rs"), 200);
    touch(root.join("Cargo.lock"), 10);
    assert!(helper_age_warning(&helper, &root).is_some(), "Cargo.lock");
    assert_eq!(helper_age_warning(&root.join("missing"), &root), None);
}

/// Answers by program name: `ok` programs exit 0, any other fails.
struct Programs(&'static [&'static str]);

impl crate::runtime::mcp::Dispatch for Programs {
    fn call(
        &self,
        call: Call,
    ) -> futures::future::BoxFuture<'static, crate::runtime::mcp::Outcome> {
        let Call::RunCommand { args } = call else {
            unreachable!()
        };
        let ok = self.0.iter().any(|p| args["program"] == *p);
        Box::pin(async move {
            crate::runtime::mcp::Outcome {
                text: if ok {
                    "exit code 0 after 0.1s".into()
                } else {
                    "the program was not found on the PATH".into()
                },
                is_error: !ok,
            }
        })
    }
}

const FORMS: [(&str, &[&str]); 2] = [
    ("pytest", &["--version"]),
    ("python3", &["-m", "pytest", "--version"]),
];

// Hardening: the live probe tries every way a model may run pytest and only
// fails when none works, saying which one does.
#[tokio::test]
async fn the_pytest_probe_reports_which_form_works_and_fails_when_none_does() {
    let both = pytest_probe(&Programs(&["pytest", "python3"]), &FORMS).await;
    assert_eq!(both.unwrap(), "pytest --version");
    let only_module = pytest_probe(&Programs(&["python3"]), &FORMS).await.unwrap();
    assert!(
        only_module.contains("python3 -m pytest --version"),
        "{only_module}"
    );
    assert!(
        only_module.contains("pytest not on the sandboxed PATH"),
        "{only_module}"
    );
    let why = pytest_probe(&Programs(&[]), &FORMS).await.unwrap_err();
    assert!(
        why.contains("pytest --version") && why.contains("python3 -m pytest"),
        "{why}"
    );
}
