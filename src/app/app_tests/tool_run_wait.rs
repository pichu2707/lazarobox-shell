//! A model's run that waits for input (a `sudo` password), runs long or hears
//! the user typing (chat-runs-commands, design D7, D9 and D10): the settle
//! machine runs with `Limits::Run`, the probe carries the pty's echo flag, and
//! what the user types in the pane never reaches the report.

use super::run_pane_keys::system_lines;
use super::tool_approval::{announce, run, shell_output};
use super::tool_run::{in_shell, run_live, slot};
use super::*;
use crate::app::RunEvent;
use crate::core::ai::tools::{
    report::RunReport,
    settle::{End, Limits},
};

const SHELL: PaneId = PaneId::FIRST;
const SECRET: &str = "hunter2";
const WAITING: &str =
    "pane 1: waiting for your input in the pane; type it there (it is never sent to the chat)";

fn at(ms: u64) {
    NOW_MS.with(|n| n.set(ms));
}

fn now() -> u64 {
    NOW_MS.with(|n| n.get())
}

/// A `run` chat whose off-list `command` the user approved with `y`: it is
/// written at once. Returns the app and the write time.
fn written(command: &str) -> (App, u64) {
    let (mut a, turn) = run_live();
    assert_eq!(announce(&mut a, turn, 1, run(command)), vec![]);
    at(now() + 1_000);
    let effects = a.update(key('y'));
    assert!(
        matches!(effects[..], [Effect::AutoRun { .. }]),
        "{effects:?}"
    );
    (a, now())
}

fn probe(a: &mut App, t: u64, foreground: bool, echo_off: bool) -> Vec<Effect> {
    at(t);
    a.update(AppEvent::Run(
        SHELL,
        RunEvent::Probe {
            foreground: Some(foreground),
            cwd: Some(LAUNCH.into()),
            echo_off: Some(echo_off),
        },
    ))
}

fn type_in_shell(a: &mut App, text: &str) {
    for c in text.chars() {
        in_shell(a, key(c));
    }
    in_shell(a, chat_live::plain(KeyCode::Enter));
}

fn report_of(effects: &[Effect]) -> &RunReport {
    match effects {
        [Effect::RunDone { report, .. }] => report,
        other => panic!("expected one RunDone, got {other:?}"),
    }
}

/// `sudo` asks, the user types the password with echo off, and the command ends.
#[test]
fn a_password_typed_in_the_pane_does_not_end_the_run_nor_reach_the_report() {
    let (mut a, t0) = written("sudo true");
    shell_output(&mut a, b"sudo true\r\n[sudo] password for u: ");
    assert_eq!(probe(&mut a, t0 + 1_100, false, true), vec![]);
    assert_eq!(system_lines(&a), vec![WAITING], "shown once");
    assert_eq!(probe(&mut a, t0 + 1_200, false, true), vec![]);
    assert_eq!(system_lines(&a), vec![WAITING], "not again");
    type_in_shell(&mut a, SECRET);
    assert_eq!(
        probe(&mut a, t0 + 2_000, false, true),
        vec![],
        "typing is not UserTyped"
    );
    shell_output(&mut a, b"\r\nall done\r\n$ ");
    let effects = probe(&mut a, t0 + 2_400, true, false);
    let report = report_of(&effects);
    assert_eq!(report.end, End::Settled);
    assert_eq!(report.limits, Limits::Run);
    assert!(!report.withheld);
    let output = report.output.as_deref().unwrap_or_default();
    assert!(output.contains("all done"), "{output:?}");
    assert!(!format!("{effects:?}").contains(SECRET));
    assert!(!format!("{:?}", system_lines(&a)).contains(SECRET));
    assert_eq!(slot(&a), None);
}

/// Input that is echoed (a `[Y/n]` answer) sits on rows the report hides.
#[test]
fn an_echoed_answer_is_hidden_from_the_report() {
    let (mut a, t0) = written("apt upgrade");
    shell_output(&mut a, b"apt upgrade\r\nProceed? [Y/n] ");
    assert_eq!(probe(&mut a, t0 + 100, false, false), vec![]);
    in_shell(&mut a, key('y'));
    shell_output(&mut a, b"y");
    in_shell(&mut a, chat_live::plain(KeyCode::Enter));
    assert_eq!(probe(&mut a, t0 + 200, false, false), vec![]);
    shell_output(&mut a, b"\r\ndone\r\n$ ");
    let effects = probe(&mut a, t0 + 800, true, false);
    let output = report_of(&effects).output.clone().unwrap_or_default();
    assert!(output.contains("done"), "{output:?}");
    assert!(!output.contains("Proceed"), "{output:?}");
}

/// A key typed with the cursor hidden withholds the whole output.
#[test]
fn a_key_typed_with_the_cursor_hidden_withholds_the_output() {
    let (mut a, t0) = written("sudo true");
    shell_output(&mut a, b"sudo true\r\nPassword: \x1b[?25l");
    assert_eq!(probe(&mut a, t0 + 100, false, true), vec![]);
    in_shell(&mut a, key('x'));
    shell_output(&mut a, b"\x1b[?25h\r\n$ ");
    let effects = probe(&mut a, t0 + 800, true, false);
    assert!(report_of(&effects).withheld);
}

#[test]
fn typing_at_the_shell_still_ends_the_run_as_typed() {
    let (mut a, t0) = written("make");
    assert_eq!(probe(&mut a, t0 + 50, true, false), vec![]);
    in_shell(&mut a, key('x'));
    let effects = probe(&mut a, t0 + 100, true, false);
    assert_eq!(report_of(&effects).end, End::UserTyped);
}

#[test]
fn a_run_still_going_after_30_s_is_cut_with_its_line() {
    let (mut a, t0) = written("npm run dev");
    shell_output(&mut a, b"npm run dev\r\nready on :3000\r\n");
    assert_eq!(probe(&mut a, t0 + 29_000, false, false), vec![]);
    let effects = probe(&mut a, t0 + 30_000, false, false);
    let report = report_of(&effects);
    assert_eq!(report.end, End::StillRunning);
    assert!(
        report
            .output
            .as_deref()
            .unwrap_or_default()
            .contains("ready on :3000")
    );
    assert_eq!(
        system_lines(&a),
        vec![
            "pane 1: npm run dev is still running after 30 s; the chat goes on with the output so far"
        ]
    );
    let (_, _, _, command) = slot(&a).expect("the cut command goes on being followed");
    assert_eq!(
        command, "npm run dev",
        "a continuation follow, no call to answer"
    );
}

#[test]
fn a_wait_for_input_ends_as_no_input_after_5_min() {
    let (mut a, t0) = written("sudo true");
    shell_output(&mut a, b"sudo true\r\nPassword: ");
    assert_eq!(probe(&mut a, t0 + 1_100, false, true), vec![]);
    assert_eq!(
        probe(&mut a, t0 + 200_000, false, true),
        vec![],
        "the cut-off is paused"
    );
    let effects = probe(&mut a, t0 + 301_200, false, true);
    assert_eq!(report_of(&effects).end, End::NoInput);
    assert_eq!(
        system_lines(&a),
        vec![
            WAITING,
            "pane 1: no input after 5 min; the chat goes on (the command still waits in the pane)"
        ]
    );
}

#[test]
fn cancelling_the_turn_while_waiting_clears_the_mark() {
    let (mut a, t0) = written("sudo true");
    shell_output(&mut a, b"sudo true\r\nPassword: ");
    probe(&mut a, t0 + 1_100, false, true);
    assert!(slot(&a).is_some());
    a.cancel_chat(id2());
    assert_eq!(slot(&a), None);
    assert_eq!(a.run_probes(), vec![]);
}

/// The follow of a typed command words the new ends, not the full-screen one.
#[test]
fn a_followed_command_words_the_cut_off_and_the_missing_input() {
    for (end, said) in [
        (
            End::StillRunning,
            "pane 1: the command is still running; send a message to go on",
        ),
        (
            End::NoInput,
            "pane 1: the command waits for input; send a message to go on",
        ),
    ] {
        let (mut a, _) = run_live();
        let report = RunReport {
            number: 1,
            end,
            cwd: None,
            cd_target: None,
            output: None,
            withheld: false,
            limits: Limits::Continue,
        };
        a.end_follow(id2(), "make".into(), report);
        assert_eq!(system_lines(&a), vec![said], "{end:?}");
    }
}
