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
        "pane 1: the command finished but its output was not sent; send a message to continue"
    ));
}

#[test]
fn an_edited_line_is_not_followed_and_says_so() {
    for edit in [key('x'), ctrl('u')] {
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

fn resume(a: &mut App, number: usize) -> Vec<Effect> {
    a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number,
            text: "Ran.".to_owned(),
        },
    ))
}

const NOT_SENT: &str =
    "the command finished but its output was not sent; send a message to continue";

// Review of #824, 1: a pane that closes during the wait is said.
#[test]
fn a_pane_closing_during_the_wait_says_so() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let effects = a.deny_closing_runs(&[SHELL]);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(says(&a, "pane 1 closed; send a message to go on"));
}

// 2: a line forgotten because of a new message is said at the next Enter.
#[test]
fn enter_on_a_line_from_an_earlier_turn_says_so() {
    let mut a = typed_live();
    let availability = a.turn_availability(id2());
    a.send_text(id2(), "something else", &availability);
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    let line = "typed command is from an earlier turn; send a message to go on";
    assert!(says(&a, line), "{:?}", system_lines(&a));
    in_shell(&mut a, enter());
    let n = system_lines(&a).iter().filter(|l| *l == line).count();
    assert_eq!(n, 1, "said once");
}

// 3: Ctrl+C forgets the line; a closed chat is not told anything.
#[test]
fn ctrl_c_forgets_the_typed_line_so_a_later_enter_says_nothing() {
    let mut a = typed_live();
    in_shell(&mut a, ctrl('c'));
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    assert!(!says(&a, CHANGED));
    assert_eq!(a.notice, None);
}

#[test]
fn a_closed_chat_leaves_no_notice_at_the_enter() {
    let mut a = typed_live();
    a.panes.remove(&id2());
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    assert_eq!(a.notice, None);
}

// 4: the progress is measured from where the Enter found the cursor.
#[test]
fn a_wrapped_command_does_not_settle_before_it_prints_anything() {
    let mut a = typed_live();
    shell_output(&mut a, "$ ".repeat(90).as_bytes());
    in_shell(&mut a, enter());
    let t0 = now();
    assert_eq!(probe(&mut a, 400, t0), vec![], "nothing moved yet");
    shell_output(&mut a, b"\r\nout\r\n$ ");
    assert_eq!(probe(&mut a, 800, t0).len(), 1);
}

// 5 and 6: a second resume while the chat is busy says whose output is lost.
#[test]
fn a_resume_while_the_chat_is_busy_says_the_output_was_not_sent() {
    let mut a = typed_live();
    assert!(!resume(&mut a, 1).is_empty(), "the first one goes");
    assert!(resume(&mut a, 2).is_empty());
    assert!(says(&a, &format!("pane 2: {NOT_SENT}")));
}

// 6: the orchestrator guard, paste and wheel, a menu key.
#[test]
fn an_orchestrating_chat_tracks_no_typed_line() {
    let (mut a, turn) = live();
    a.ai.orchestrator.enabled = true;
    let chat = a
        .panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap();
    chat.set_approval_mode(ApprovalMode::Auto);
    chat.set_orch(true);
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    announce(&mut a, turn, 1, run("echo hi"));
    assert!(a.orchestrating(id2()));
    in_shell(&mut a, enter());
    assert!(slot(&a).is_none());
    assert!(!says(&a, CHANGED));
}

#[test]
fn a_paste_wheel_or_menu_key_before_the_enter_counts_as_an_edit() {
    for touch in ["paste", "wheel", "tab"] {
        let mut a = typed_live();
        match touch {
            "paste" => drop(in_shell(&mut a, AppEvent::Paste("x".into()))),
            "wheel" => a.mark_dirty(SHELL),
            _ => drop(in_shell(
                &mut a,
                AppEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            )),
        }
        in_shell(&mut a, enter());
        assert!(slot(&a).is_none(), "{touch}");
        assert!(says(&a, CHANGED), "{touch}");
    }
}

// Owner: closing a pane whose command the chat waits for asks first.
fn close_prompt(a: &mut App) {
    a.tab_mut().focus = SHELL;
    press(a, "wq");
}

#[test]
fn closing_a_pane_the_chat_waits_on_asks_and_y_closes_it_with_a_line() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    close_prompt(&mut a);
    assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePaneWaited));
    assert!(a.input().label().contains("the chat waits for"));
    let effects = a.update(key('y'));
    assert!(effects.contains(&Effect::ClosePane(SHELL)), "{effects:?}");
    assert!(says(&a, "pane 1 closed; send a message to go on"));
}

#[test]
fn declining_the_close_changes_nothing() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    close_prompt(&mut a);
    let effects = a.update(key('n'));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(slot(&a).is_some(), "still followed");
    assert!(!says(&a, "pane 1 closed; send a message to go on"));
}

#[test]
fn a_pane_nobody_waits_on_keeps_the_plain_close_prompt() {
    let mut a = typed_live();
    close_prompt(&mut a);
    assert_eq!(a.input(), InputMode::Confirm(Confirm::ClosePane));
}

// Owner: a tab with such a pane, and the quit prompt, warn too.
fn two_tabs_waiting() -> App {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    // A second tab, built by hand: the fixture's pane ids are not the allocator's.
    let other = PaneId::for_test(9);
    a.tabs.push(Tab {
        tree: Node::Leaf(other),
        focus: other,
        zoom: false,
        last_shell: None,
    });
    a.tab_mut().focus = SHELL;
    a
}

#[test]
fn closing_a_tab_with_a_waited_pane_asks_and_y_tells_the_chat() {
    let mut a = two_tabs_waiting();
    press(&mut a, "tc");
    assert_eq!(a.input(), InputMode::Confirm(Confirm::CloseTabWaited));
    assert_eq!(
        a.input().label(),
        "Close tab? a command the chat waits for is running (y/n)"
    );
    let effects = a.update(key('y'));
    assert!(effects.contains(&Effect::ClosePane(SHELL)), "{effects:?}");
}

#[test]
fn declining_the_tab_close_changes_nothing() {
    let mut a = two_tabs_waiting();
    press(&mut a, "tc");
    assert!(a.update(key('n')).is_empty());
    assert!(slot(&a).is_some());
    assert_eq!((a.tabs.len(), a.input()), (2, InputMode::Terminal));
}

#[test]
fn the_quit_prompt_says_a_command_the_chat_waits_for_is_running() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    a.input = InputMode::Confirm(Confirm::Quit);
    assert_eq!(
        a.status_label(80).as_deref(),
        Some("Quit? a command the chat waits for is running (y/n)")
    );
}
