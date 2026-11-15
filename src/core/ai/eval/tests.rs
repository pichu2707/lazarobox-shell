//! The scorer, the refusal and ask-back rules and the shipped scenarios.

use serde_json::{Value, json};

use super::{
    Category, Expect, RunResult, Scenario, asked_back, refused, scenarios,
    scenarios::{COORDINATE, EDIT_EXISTING, READ_ONLY, WRITE_AND_TEST},
    score,
};
use crate::core::ai::chat::adapters::ToolExchange;

fn ex(name: &str, arguments: Value, result: &str) -> ToolExchange {
    ToolExchange {
        name: name.to_owned(),
        id: None,
        index: None,
        arguments,
        result: result.to_owned(),
    }
}

fn write() -> ToolExchange {
    ex(
        "write_file",
        json!({"path": "fib.py", "content": "def fib(n):\n    return n\n"}),
        "staged fib.py",
    )
}

fn write_test() -> ToolExchange {
    ex(
        "write_file",
        json!({"path": "tests/test_fib.py", "content": "def test_fib():\n    pass\n"}),
        "staged test_fib.py",
    )
}

fn batch() -> ToolExchange {
    ex("propose_batch", json!({}), "the batch was applied")
}

fn pytest() -> ToolExchange {
    ex(
        "run_command",
        json!({"program": "pytest", "args": []}),
        "exit code 0 after 0.4s\n1 passed",
    )
}

fn pytest_ending(result: &str) -> ToolExchange {
    ex(
        "run_command",
        json!({"program": "pytest", "args": []}),
        result,
    )
}

fn commit(message: &str) -> ToolExchange {
    ex(
        "git_commit",
        json!({"message": message}),
        "committed abc123",
    )
}

const LITERAL: &str = "add fibonacci";

fn good() -> Vec<ToolExchange> {
    vec![write(), write_test(), batch(), pytest(), commit(LITERAL)]
}

fn run(exchanges: &[ToolExchange]) -> RunResult {
    score(&WRITE_AND_TEST, exchanges, "Done.")
}

#[test]
fn complete_run_passes() {
    let result = run(&good());
    assert!(result.missing.is_empty(), "{result:?}");
    assert!(result.out_of_order.is_empty(), "{result:?}");
    assert!(result.ignored.is_empty(), "{result:?}");
    assert!(result.passed(), "{result:?}");
}

#[test]
fn a_missing_step_fails_the_run() {
    let result = run(&[write(), write_test(), batch(), commit(LITERAL)]);
    assert_eq!(result.missing, ["pytest"]);
    assert!(!result.passed());
}

#[test]
fn a_commit_before_the_batch_is_out_of_order() {
    let result = run(&[write(), write_test(), commit(LITERAL), batch(), pytest()]);
    assert!(
        result.out_of_order.contains(&("batch", "commit")),
        "{result:?}"
    );
    assert!(result.missing.is_empty() && result.ignored.is_empty());
    assert!(!result.passed());
}

#[test]
fn a_test_run_after_the_commit_is_out_of_order() {
    let result = run(&[write(), write_test(), batch(), commit(LITERAL), pytest()]);
    assert_eq!(result.out_of_order, [("pytest", "commit")]);
}

#[test]
fn a_test_run_before_the_batch_is_out_of_order() {
    let result = run(&[write(), write_test(), pytest(), batch(), commit(LITERAL)]);
    assert_eq!(result.out_of_order, [("batch", "pytest")]);
}

#[test]
fn each_of_the_two_files_must_be_written_by_its_exact_name() {
    let only_test = run(&[write_test(), batch(), pytest(), commit(LITERAL)]);
    assert_eq!(only_test.missing, ["write"]);
    let only_code = run(&[write(), batch(), pytest(), commit(LITERAL)]);
    assert_eq!(only_code.missing, ["write-test"]);
}

#[test]
fn a_failed_git_commit_without_a_refusal_prefix_is_not_covered() {
    // Known limit (tasks S1b): the exchange holds the result text only, with
    // no status, so these host failures score as completed until the replay
    // guard closes it. The texts that ARE covered are pinned in `refused`.
    for text in ["nothing is staged", "git failed", "could not run git"] {
        assert!(!refused(text), "{text}");
    }
    assert!(refused("denied: the user did not approve it"));
}

#[test]
fn a_different_commit_message_is_an_ignored_literal() {
    let result = run(&[
        write(),
        write_test(),
        batch(),
        pytest(),
        commit("fibonacci"),
    ]);
    assert_eq!(result.ignored, [LITERAL]);
    assert!(result.missing.is_empty() && result.out_of_order.is_empty());
    assert!(!result.passed());
}

#[test]
fn a_refused_exchange_is_not_completed() {
    for text in ["denied: the user did not approve it", "the tool failed"] {
        let mut list = good();
        list[4].result = text.to_owned();
        let result = run(&list);
        assert_eq!(result.missing, ["commit"], "{text}");
        assert!(!result.passed());
    }
}

#[test]
fn a_call_in_the_answer_text_does_not_count() {
    let answer = r#"{"name": "git_commit", "arguments": {"message": "add fibonacci"}}"#;
    let result = score(
        &WRITE_AND_TEST,
        &[write(), write_test(), batch(), pytest()],
        answer,
    );
    assert_eq!(result.missing, ["commit"]);
}

#[test]
fn asking_back_with_every_detail_given_fails() {
    let result = score(
        &WRITE_AND_TEST,
        &[write(), write_test(), batch(), pytest()],
        "Please provide a commit message",
    );
    assert!(result.asked_back);
    assert!(!result.passed());
    let all_done = score(&WRITE_AND_TEST, &good(), "Anything else?");
    assert!(all_done.asked_back && !all_done.passed());
}

#[test]
fn asking_back_in_an_ambiguous_request_passes() {
    const STEP: Expect = Expect {
        label: "write",
        must_pass: false,
        tool: "write_file",
        args: |_| true,
    };
    let ambiguous = Scenario {
        ambiguous: true,
        steps: &[STEP],
        order: &[],
        literals: &[],
        forbid: &["write_file"],
        ..READ_ONLY
    };
    let result = score(&ambiguous, &[], "Which file do you mean?");
    assert!(result.asked_back && result.passed(), "{result:?}");
    let wrote = score(&ambiguous, &[write()], "Which file do you mean?");
    assert!(!wrote.passed(), "{wrote:?}");
}

#[test]
fn a_read_only_run_must_not_write() {
    let read = ex(
        "read_file",
        json!({"path": "text_utils.py"}),
        "def slug(): ...",
    );
    let clean = score(&READ_ONLY, std::slice::from_ref(&read), "It builds a slug.");
    assert!(clean.passed(), "{clean:?}");
    let result = score(&READ_ONLY, &[read, write()], "It builds a slug.");
    assert_eq!(result.forbidden, ["write_file"]);
    assert!(!result.passed());
}

#[test]
fn the_scorer_is_pure() {
    let list = good();
    assert_eq!(run(&list), run(&list));
}

#[test]
fn a_non_zero_exit_is_not_a_refusal() {
    assert!(!refused("exit code 1 after 0.4s\n1 failed, 2 passed"));
    assert!(!refused("staged fib.py"));
    assert!(!refused(""));
}

#[test]
fn the_refusal_prefixes_are_the_known_forms() {
    for text in [
        "denied: the user did not approve it",
        "the tool failed",
        "result dropped: the tool output limit of this turn was reached",
        "too many tool calls in one reply",
        "unknown tool",
        "missing argument",
        "path not available",
    ] {
        assert!(refused(text), "{text}");
    }
}

#[test]
fn asked_back_reads_a_question_or_a_request_for_input() {
    assert!(asked_back("Which file?"));
    assert!(asked_back("which file?  \n"));
    assert!(asked_back("Please provide the commit message."));
    assert!(!asked_back("Committed. The tests pass."));
    assert!(!asked_back(""));
}

#[test]
fn the_scenarios_are_engine_neutral() {
    const BANNED: [&str; 8] = [
        "ollama",
        "llama",
        "http",
        "11434",
        "model",
        "localhost",
        "127.0.0.1",
        "/api",
    ];
    for scenario in scenarios::ALL {
        let mut texts = vec![scenario.id, scenario.request];
        texts.extend(scenario.seed.iter().flat_map(|(path, text)| [*path, *text]));
        texts.extend(scenario.steps.iter().flat_map(|s| [s.label, s.tool]));
        texts.extend(scenario.literals.iter().flat_map(|l| [l.step, l.text]));
        for text in texts {
            let lower = text.to_lowercase();
            for word in BANNED {
                assert!(!lower.contains(word), "{}: {word} in {text}", scenario.id);
            }
        }
    }
}

#[test]
fn task_a_expects_its_five_steps_in_order() {
    let tools: Vec<_> = WRITE_AND_TEST.steps.iter().map(|s| s.tool).collect();
    assert_eq!(
        tools,
        [
            "write_file",
            "write_file",
            "propose_batch",
            "run_command",
            "git_commit"
        ]
    );
    assert_eq!(
        WRITE_AND_TEST.order,
        [
            ("batch", "pytest"),
            ("batch", "commit"),
            ("pytest", "commit")
        ]
    );
    assert_eq!(WRITE_AND_TEST.literals.len(), 1);
    assert_eq!(WRITE_AND_TEST.literals[0].step, "commit");
    assert_eq!(WRITE_AND_TEST.literals[0].text, LITERAL);
    assert!(WRITE_AND_TEST.request.contains(LITERAL));
}

#[test]
fn task_c_forbids_write_exec_and_commit() {
    for tool in [
        "write_file",
        "edit_file",
        "create_dir",
        "propose_batch",
        "run_command",
        "git_add",
        "git_commit",
    ] {
        assert!(READ_ONLY.forbid.contains(&tool), "{tool}");
    }
}

#[test]
fn every_scenario_has_its_category_key() {
    assert_eq!(WRITE_AND_TEST.category.key(), "write-and-test");
    assert_eq!(EDIT_EXISTING.category.key(), "edit-existing");
    assert_eq!(READ_ONLY.category.key(), "read-only");
    assert_eq!(COORDINATE.category.key(), "coordinate");
    assert_eq!(scenarios::ALL.len(), 4);
    assert!(scenarios::ALL.iter().all(|s| !s.ambiguous));
    assert_eq!(Category::ReadOnly.key(), "read-only");
}

#[test]
fn task_b_is_a_good_run_when_it_edits_proposes_and_tests() {
    let edit = ex(
        "edit_file",
        json!({"path": "calc.py", "old": "return a + b", "new": "return sum(a)"}),
        "staged",
    );
    let result = score(&EDIT_EXISTING, &[edit, batch(), pytest()], "Done.");
    assert!(result.passed(), "{result:?}");
    let no_edit = score(&EDIT_EXISTING, &[batch(), pytest()], "Done.");
    assert_eq!(no_edit.missing, ["edit"]);
}

fn handoff(step: &str, extra: Value) -> ToolExchange {
    let mut arguments = json!({"step": step, "verdict": "on_course"});
    arguments
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    ex("handoff", arguments, "step closed")
}

fn spawn(kind: &str, outputs: Value) -> ToolExchange {
    let arguments = json!({"kind": kind, "goal": "write greeting.py", "inputs": [],
        "outputs": outputs, "done_when": "greet works", "out_of_scope": "nothing else"});
    ex("spawn_agent", arguments, "worker 1 started")
}

fn coordinated() -> Vec<ToolExchange> {
    vec![
        handoff("authorize", json!({"scope": "implement"})),
        handoff("explore", json!({})),
        handoff("resolve", json!({})),
        handoff("classify", json!({"size": "small", "reason": "one file"})),
        spawn("simple", json!(["greeting.py"])),
    ]
}

#[test]
fn a_coordinator_that_calls_the_steps_and_spawns_a_simple_worker_passes() {
    let result = score(&COORDINATE, &coordinated(), "Worker 1 is writing it.");
    assert!(result.passed(), "{result:?}");
    assert_eq!(result.completed.len(), 5);
}

#[test]
fn a_coordinator_that_describes_the_calls_instead_of_making_them_fails() {
    let prose = "Run handoff(step=\"authorize\", scope=\"implement\") and then spawn it.";
    let result = score(&COORDINATE, &[], prose);
    assert_eq!(
        result.missing,
        ["authorize", "explore", "resolve", "classify", "spawn"]
    );
    assert!(!result.passed());
    let text = r#"{"name": "spawn_agent", "arguments": {"kind": "simple"}}"#;
    assert!(!score(&COORDINATE, &[], text).passed());
}

#[test]
fn spawning_before_the_steps_or_the_wrong_kind_does_not_complete_the_task() {
    let mut early = coordinated();
    early.rotate_right(1);
    let result = score(&COORDINATE, &early, "Done.");
    assert_eq!(result.out_of_order, [("classify", "spawn")]);
    let mut wrong = coordinated();
    wrong[4] = spawn("code", json!(["greeting.py"]));
    assert_eq!(score(&COORDINATE, &wrong, "Done.").missing, ["spawn"]);
    wrong[4] = spawn("simple", json!([]));
    assert_eq!(score(&COORDINATE, &wrong, "Done.").missing, ["spawn"]);
    let mut read_only = coordinated();
    read_only[0] = handoff("authorize", json!({"scope": "read_only"}));
    assert_eq!(
        score(&COORDINATE, &read_only, "Done.").missing,
        ["authorize"]
    );
}

#[test]
fn the_coordinator_writes_nothing_itself() {
    let mut run = coordinated();
    run.push(write());
    let result = score(&COORDINATE, &run, "Done.");
    assert_eq!(result.forbidden, ["write_file"]);
    assert!(!result.passed());
}

#[test]
fn the_coordinate_task_asks_for_a_simple_write() {
    assert!(COORDINATE.request.contains("greeting.py"));
}

// Hardening: a test step is completed only by a command that exited 0. The
// command ran, so its result is not a refusal, but it did not pass.
#[test]
fn a_test_run_that_exits_nonzero_is_failed_not_missing() {
    let mut list = good();
    list[3] = pytest_ending("exit code 1 after 0.4s\n1 failed");
    let result = run(&list);
    assert_eq!(result.failed, ["pytest"], "{result:?}");
    assert!(result.missing.is_empty() && result.out_of_order.is_empty());
    assert!(!result.completed.contains(&"pytest") && !result.passed());
}

#[test]
fn a_passing_rerun_after_a_failed_run_completes_the_step() {
    let mut list = good();
    list.insert(3, pytest_ending("exit code 1 after 0.4s\n1 failed"));
    let result = run(&list);
    assert!(result.failed.is_empty() && result.passed(), "{result:?}");
}

// The prefix is the one `RunOutcome::facts` writes: pinned with real outcomes.
#[test]
fn only_a_real_exit_zero_outcome_starts_with_the_success_prefix() {
    use crate::core::ai::tools::run::{Capture, Ended, RunOutcome};
    use std::time::Duration;
    let ended = |ended: Ended| {
        let outcome = RunOutcome::new(vec![], ended, &Capture::new(), Duration::from_millis(400));
        outcome.facts().starts_with(super::COMMAND_OK)
    };
    let none = Ended::default();
    assert!(ended(Ended {
        exit: Some(0),
        ..none
    }));
    assert!(!ended(Ended {
        exit: Some(1),
        ..none
    }));
    assert!(!ended(Ended {
        exit: Some(0),
        timed_out: true,
        ..none
    }));
    assert!(!ended(Ended {
        exit: Some(0),
        cancelled: true,
        ..none
    }));
    assert!(!ended(Ended {
        signal: Some(9),
        ..none
    }));
    assert!(!ended(none));
}

// Hardening: the exit-0 rule is the step's own flag, not its tool name: a step
// that expects a command whose exit does not matter completes on any exit.
#[test]
fn a_command_step_without_must_pass_completes_on_any_exit() {
    const STEP: Expect = Expect {
        label: "lint",
        must_pass: false,
        tool: "run_command",
        args: |_| true,
    };
    let scenario = Scenario {
        steps: &[STEP],
        order: &[],
        literals: &[],
        forbid: &[],
        ..READ_ONLY
    };
    let failing = pytest_ending("exit code 1 after 0.4s\n1 failed");
    let result = score(&scenario, &[failing], "Done.");
    assert_eq!(result.completed, ["lint"], "{result:?}");
    assert!(result.failed.is_empty() && result.missing.is_empty());
}
