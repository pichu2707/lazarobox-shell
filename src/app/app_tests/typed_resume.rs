//! #819: a command the model typed and the user ran goes on by itself. The app
//! follows the Enter of an untouched typed line, waits for the command with the
//! settle machine of `run` mode, and starts the next turn with a fixed note
//! plus the output the `run` mode already caps and redacts.

use super::run_pane_keys::system_lines;
use super::tool_approval::{announce, live, result, run, shell_output};
use super::tool_run::{chat_of, in_shell, slot};
use super::*;
use crate::app::RunEvent;
use crate::core::ai::{
    chat::engine::{CallId, Finish, StreamEvent, ToolStatus},
    config::ApprovalMode,
    tools::{
        report::{CwdSource, RunReport},
        settle::End,
    },
};

const SHELL: PaneId = PaneId::FIRST;
const CHANGED: &str = "typed command changed: send a message to go on";

fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn at(ms: u64) {
    NOW_MS.with(|n| n.set(ms));
}

fn now() -> u64 {
    NOW_MS.with(|n| n.get())
}

/// An `auto` chat whose model typed `echo hi` into the shell and whose turn
/// ended with the waiting line.
fn typed_live() -> App {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Auto);
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    announce(&mut a, turn, 1, run("echo hi"));
    result(&mut a, turn, 1, ToolStatus::Ok);
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    a
}

fn probe(a: &mut App, after: u64, t0: u64) -> Vec<Effect> {
    at(t0 + after);
    a.update(AppEvent::Run(
        SHELL,
        RunEvent::Probe {
            foreground: Some(true),
            cwd: Some(LAUNCH.into()),
        },
    ))
}

fn report(end: End) -> RunReport {
    RunReport {
        number: 1,
        end,
        cwd: Some((LAUNCH.into(), CwdSource::Proc)),
        cd_target: None,
        output: None,
    }
}

fn resume_note(end: End) -> Effect {
    Effect::ResumeNote {
        chat: id2(),
        command: "echo hi".to_owned(),
        report: Box::new(report(end)),
    }
}

fn says(a: &App, line: &str) -> bool {
    system_lines(a).iter().any(|l| l == line)
}

#[test]
fn enter_on_the_untouched_typed_line_follows_the_command() {
    let mut a = typed_live();
    assert!(slot(&a).is_none(), "typing alone follows nothing yet");
    in_shell(&mut a, enter());
    let (chat, _, call, command) = slot(&a).expect("the Enter is followed");
    assert_eq!(
        (chat, call, command.as_str()),
        (id2(), CallId(1), "echo hi")
    );
    assert_eq!(a.run_probes(), vec![SHELL]);
}

#[test]
fn a_settled_command_resumes_the_chat_with_its_report() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let t0 = now();
    shell_output(&mut a, b"\r\nhi\r\n$ ");
    assert_eq!(probe(&mut a, 400, t0), vec![resume_note(End::Settled)]);
    assert!(slot(&a).is_none(), "the executing mark is released");
    assert!(says(&a, "pane 1: the command finished, going on"));
}

#[test]
fn the_resume_event_starts_a_turn_with_the_fixed_note_and_the_text() {
    let mut a = typed_live();
    let effects = a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number: 1,
            text: "Ran in pane 1. cwd: /launch.".to_owned(),
        },
    ));
    assert!(
        matches!(effects[..], [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
    let last = chat_of(&a).messages().last().unwrap().text.clone();
    assert!(last.starts_with("lazarobox: the user ran the command you typed in pane 1"));
    assert!(last.contains("Ran in pane 1. cwd: /launch."), "{last}");
}

#[test]
fn a_resume_the_chat_cannot_take_is_said() {
    let mut a = typed_live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .composer_mut()
        .insert('x');
    let effects = a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number: 1,
            text: "Ran.".to_owned(),
        },
    ));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(says(
        &a,
        "pane 1: the command finished but the chat could not go on; send a message to continue"
    ));
}

#[test]
fn an_edited_line_is_not_followed_and_says_so() {
    for edit in [key('x'), ctrl('c'), ctrl('u')] {
        let mut a = typed_live();
        in_shell(&mut a, edit);
        in_shell(&mut a, enter());
        assert!(slot(&a).is_none());
        assert!(says(&a, CHANGED), "{:?}", system_lines(&a));
        assert_eq!(a.notice.as_deref(), Some(CHANGED));
    }
}

#[test]
fn a_cursor_key_before_the_enter_counts_as_an_edit() {
    let mut a = typed_live();
    let left = AppEvent::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    in_shell(&mut a, left);
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    assert!(says(&a, CHANGED));
}

fn ctrl(c: char) -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

#[test]
fn a_line_the_user_had_started_before_the_typing_is_not_followed() {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Auto);
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    in_shell(&mut a, key('x'));
    announce(&mut a, turn, 1, run("echo hi"));
    a.update(key('y'));
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    assert!(says(&a, CHANGED));
}

#[test]
fn a_typing_the_runtime_refused_follows_nothing() {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Auto);
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    announce(&mut a, turn, 1, run("echo hi"));
    result(&mut a, turn, 1, ToolStatus::Denied);
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
}

#[test]
fn a_command_that_does_not_finish_in_time_ends_the_wait_with_a_line() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let t0 = now();
    assert_eq!(probe(&mut a, 9_000, t0), vec![]);
    assert_eq!(probe(&mut a, 10_000, t0), vec![]);
    assert!(slot(&a).is_none());
    assert!(says(
        &a,
        "pane 1: the command did not finish in 10 s; send a message to go on"
    ));
}

#[test]
fn typing_while_the_command_runs_ends_the_wait_with_a_line() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let t0 = now();
    in_shell(&mut a, key('q'));
    assert_eq!(probe(&mut a, 100, t0), vec![]);
    assert!(says(
        &a,
        "pane 1: you typed while the command ran; send a message to go on"
    ));
}

#[test]
fn esc_in_the_chat_ends_the_wait_with_a_line() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    assert!(slot(&a).is_none());
    assert!(says(&a, "stopped waiting for the command in pane 1"));
}

#[test]
fn a_new_message_ends_the_wait_with_a_line() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let availability = a.turn_availability(id2());
    a.send_text(id2(), "something else", &availability);
    assert!(slot(&a).is_none());
    assert!(says(&a, "stopped waiting for the command in pane 1"));
}

#[test]
fn a_new_message_before_the_enter_drops_the_typed_line() {
    let mut a = typed_live();
    let availability = a.turn_availability(id2());
    a.send_text(id2(), "something else", &availability);
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
}

#[test]
fn a_command_typed_with_y_in_ask_mode_is_followed_too() {
    let (mut a, turn) = live();
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    announce(&mut a, turn, 1, run("echo hi"));
    a.update(key('y'));
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    in_shell(&mut a, enter());
    assert!(slot(&a).is_some());
}
