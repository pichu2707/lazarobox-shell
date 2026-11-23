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
use crate::core::pty::PtyEvent;

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
            echo_off: None,
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
        withheld: false,
        limits: crate::core::ai::tools::settle::Limits::Follow,
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
    assert!(says(&a, "pane 1: the command finished"));
    assert!(!says(&a, "pane 1: the command finished, going on"));
}

#[test]
fn the_resume_event_starts_a_turn_with_the_fixed_note_and_the_text() {
    let mut a = typed_live();
    a.held_resumes.push((id2(), 1, None));
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
    a.held_resumes.push((id2(), 1, None));
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
    assert!(!says(&a, "going on with the output of pane 1"));
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
    a.held_resumes.push((id2(), number, None));
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
fn closing_a_tab_with_a_waited_pane_asks_first_and_y_closes_its_pane() {
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

#[test]
fn a_resume_that_goes_says_so_once() {
    let mut a = typed_live();
    assert!(!resume(&mut a, 1).is_empty());
    assert!(says(&a, "going on with the output of pane 1"));
}

/// A chat, an `auto` turn that typed `echo one` in pane 1 and `echo two` in a
/// second shell, both followed after the user's Enter in each.
fn two_followed() -> (App, PaneId) {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Auto);
    a.update(AppEvent::Cwd(SHELL, LAUNCH.into()));
    announce(&mut a, turn, 1, run("echo one"));
    result(&mut a, turn, 1, ToolStatus::Ok);
    a.tab_mut().focus = SHELL;
    press(&mut a, "wv");
    let second = a.focused();
    a.update(AppEvent::Cwd(second, LAUNCH.into()));
    a.tab_mut().focus = id2();
    a.tab_mut().last_shell = Some(second);
    announce(&mut a, turn, 2, run("echo two"));
    result(&mut a, turn, 2, ToolStatus::Ok);
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    for pane in [SHELL, second] {
        a.tab_mut().focus = pane;
        a.update(enter());
    }
    a.tab_mut().focus = id2();
    (a, second)
}

fn settle_pane(a: &mut App, pane: PaneId) -> Vec<Effect> {
    a.update(AppEvent::Pty(
        pane,
        PtyEvent::Output(b"\r\nout\r\n$ ".to_vec()),
    ));
    at(now() + 1_000);
    a.update(AppEvent::Run(
        pane,
        RunEvent::Probe {
            foreground: Some(true),
            cwd: Some(LAUNCH.into()),
            echo_off: None,
        },
    ))
}

#[test]
fn two_followed_commands_reach_the_model_in_one_combined_note() {
    let (mut a, second) = two_followed();
    assert_eq!(settle_pane(&mut a, SHELL).len(), 1);
    let held = a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number: 1,
            text: "OUT-ONE".to_owned(),
        },
    ));
    assert!(held.is_empty(), "the other command is still running");
    assert_eq!(settle_pane(&mut a, second).len(), 1);
    let sent = a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number: 2,
            text: "OUT-TWO".to_owned(),
        },
    ));
    assert!(matches!(sent[..], [Effect::ChatSend { .. }]), "{sent:?}");
    let note = chat_of(&a).messages().last().unwrap().text.clone();
    assert!(
        note.contains("OUT-ONE") && note.contains("OUT-TWO"),
        "{note}"
    );
}

#[test]
fn a_held_output_goes_when_the_other_command_ends_without_resuming() {
    let (mut a, second) = two_followed();
    settle_pane(&mut a, SHELL);
    a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number: 1,
            text: "OUT-ONE".to_owned(),
        },
    ));
    in_shell_of(&mut a, second, key('q'));
    let effects = settle_pane(&mut a, second);
    assert!(
        matches!(effects[..], [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
}

fn in_shell_of(a: &mut App, pane: PaneId, event: AppEvent) {
    a.tab_mut().focus = pane;
    a.update(event);
    a.tab_mut().focus = id2();
}

#[test]
fn closing_the_chat_drops_the_commands_it_follows() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    a.tab_mut().focus = id2();
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(slot(&a).is_none());
}

#[test]
fn a_denial_of_another_turns_call_keeps_the_typed_line() {
    let mut a = typed_live();
    a.clear_line_pending(id2(), 999, CallId(1));
    in_shell(&mut a, enter());
    assert!(slot(&a).is_some(), "the line of turn 1 call 1 stays");
}

const WAITS_TRACKED: &str =
    "waiting for you: press Enter in pane 1 to run it; the chat goes on when it finishes";
const WAITS_UNTRACKED: &str =
    "waiting for you: press Enter in pane 1 to run it, then send a message to go on";

#[test]
fn a_tracked_typed_line_promises_the_chat_goes_on() {
    let a = typed_live();
    assert!(says(&a, WAITS_TRACKED), "{:?}", system_lines(&a));
    assert!(!says(&a, WAITS_UNTRACKED));
}

#[test]
fn an_untracked_typed_line_keeps_the_old_waiting_line() {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Auto);
    a.panes
        .get_mut(&SHELL)
        .and_then(PaneState::pty_mut)
        .unwrap()
        .proc_cwd = None;
    announce(&mut a, turn, 1, run("echo hi"));
    result(&mut a, turn, 1, ToolStatus::Ok);
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert!(says(&a, WAITS_UNTRACKED), "{:?}", system_lines(&a));
    assert!(!says(&a, WAITS_TRACKED));
}

fn resume_event(a: &mut App, number: usize, text: &str) -> Vec<Effect> {
    a.update(AppEvent::Run(
        id2(),
        RunEvent::Resume {
            number,
            text: text.to_owned(),
        },
    ))
}

// Final review, 1: an output held for another command is never stranded.
#[test]
fn a_new_message_drops_the_held_output_and_says_so() {
    let (mut a, _) = two_followed();
    settle_pane(&mut a, SHELL);
    assert!(resume_event(&mut a, 1, "OUT-ONE").is_empty());
    let availability = a.turn_availability(id2());
    a.send_text(id2(), "something else", &availability);
    assert!(says(&a, &format!("pane 1: {NOT_SENT}")));
    assert!(a.held_resumes.is_empty(), "nothing is sent stale later");
}

#[test]
fn esc_drops_the_held_output_and_says_so() {
    let (mut a, _) = two_followed();
    settle_pane(&mut a, SHELL);
    resume_event(&mut a, 1, "OUT-ONE");
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    assert!(says(&a, &format!("pane 1: {NOT_SENT}")));
    assert!(a.held_resumes.is_empty());
}

#[test]
fn closing_the_other_pane_sends_the_held_output_now() {
    let (mut a, second) = two_followed();
    settle_pane(&mut a, SHELL);
    assert!(resume_event(&mut a, 1, "OUT-ONE").is_empty());
    let effects = a.deny_closing_runs(&[second]);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })),
        "{effects:?}"
    );
    assert!(a.held_resumes.is_empty());
}

#[test]
fn closing_the_chat_forgets_what_it_held() {
    let (mut a, _) = two_followed();
    settle_pane(&mut a, SHELL);
    resume_event(&mut a, 1, "OUT-ONE");
    a.drop_chat_slots(id2());
    assert!(a.held_resumes.is_empty());
}

// Final review, 2: a report still on its way counts as a command followed.
#[test]
fn both_commands_settling_before_the_first_report_returns_send_one_note() {
    let (mut a, second) = two_followed();
    assert_eq!(settle_pane(&mut a, SHELL).len(), 1);
    assert_eq!(settle_pane(&mut a, second).len(), 1);
    assert!(
        resume_event(&mut a, 1, "OUT-ONE").is_empty(),
        "pane 2's report is due"
    );
    let sent = resume_event(&mut a, 2, "OUT-TWO");
    assert!(matches!(sent[..], [Effect::ChatSend { .. }]), "{sent:?}");
    let note = chat_of(&a).messages().last().unwrap().text.clone();
    assert!(
        note.contains("OUT-ONE") && note.contains("OUT-TWO"),
        "{note}"
    );
}

// #826, 1: "going on" is said only after a send that really happened.
#[test]
fn a_refused_send_does_not_say_it_goes_on() {
    let mut a = typed_live();
    a.ai.orchestrator.enabled = true;
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_orch(true);
    assert!(!matches!(
        a.turn_availability(id2()),
        crate::core::ai::chat::engine::Availability::Checking
    ));
    let effects = resume(&mut a, 1);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(says(&a, &format!("pane 1: {NOT_SENT}")));
    assert!(!says(&a, "going on with the output of pane 1"));
}

// #826, 2: the pane label is the one the model was told, whatever happens
// to the layout or the active tab meanwhile.
#[test]
fn stopping_the_wait_from_another_tab_names_the_pane() {
    let mut a = typed_live();
    in_shell(&mut a, enter());
    let other = PaneId::for_test(9);
    a.tabs.push(Tab {
        tree: Node::Leaf(other),
        focus: other,
        zoom: false,
        last_shell: None,
    });
    a.active = 1;
    a.stop_following(id2(), false);
    assert!(says(&a, "stopped waiting for the command in pane 1"));
    assert!(!says(&a, "pane 0"));
}

#[test]
fn a_lower_pane_closing_does_not_renumber_the_wait() {
    let (mut a, second) = two_followed();
    let before = a.pane_number(second).unwrap();
    assert!(before > 1, "the shell comes before it");
    // The pane before it closes: its position changes, its label must not.
    a.tab_mut().tree.remove(SHELL);
    assert_ne!(a.pane_number(second), Some(before));
    a.stop_following(id2(), false);
    assert!(says(
        &a,
        &format!("stopped waiting for the command in pane {before}")
    ));
}

// #826, 4: a report still on its way (the `None` placeholder) when the chat
// moves on is dropped, and its late Resume starts nothing.
fn placeholder_waiting() -> App {
    let (mut a, _) = two_followed();
    settle_pane(&mut a, SHELL);
    assert!(a.held_resumes.iter().any(|h| h.2.is_none()));
    a
}

fn late_resume_is_ignored(a: &mut App) {
    let lines = system_lines(a).len();
    let effects = resume_event(a, 1, "LATE");
    assert!(effects.is_empty(), "{effects:?}");
    assert!(a.held_resumes.is_empty());
    assert_eq!(system_lines(a).len(), lines, "a late report says nothing");
    assert!(!says(a, "going on with the output of pane 1"));
}

#[test]
fn a_new_message_before_the_report_returns_ignores_the_late_resume() {
    let mut a = placeholder_waiting();
    let availability = a.turn_availability(id2());
    a.send_text(id2(), "something else", &availability);
    assert!(says(&a, &format!("pane 1: {NOT_SENT}")));
    late_resume_is_ignored(&mut a);
}

#[test]
fn esc_before_the_report_returns_ignores_the_late_resume() {
    let mut a = placeholder_waiting();
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    assert!(says(&a, &format!("pane 1: {NOT_SENT}")));
    late_resume_is_ignored(&mut a);
}

#[test]
fn closing_the_chat_before_the_report_returns_ignores_the_late_resume() {
    let mut a = placeholder_waiting();
    a.drop_chat_slots(id2());
    assert!(a.held_resumes.is_empty());
    let effects = resume_event(&mut a, 1, "LATE");
    assert!(effects.is_empty(), "{effects:?}");
}

// #826, 3: Esc with nothing followed or held cancels nothing and says nothing.
#[test]
fn esc_in_an_idle_chat_with_nothing_to_stop_says_nothing() {
    let mut a = typed_live();
    a.tab_mut().focus = id2();
    let lines = system_lines(&a).len();
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
    assert_eq!(system_lines(&a).len(), lines);
}
