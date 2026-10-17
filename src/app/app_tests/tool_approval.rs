//! A model's tool call that needs the user reaches the same approval overlay
//! as `Prefix e` (agent-tools, pane-commands): the keys answer the call under
//! its pane, turn and call, a refused write is a denial and never "typed", and
//! a prompt whose call is over goes away.

use super::chat_live::{ollama, plain};
use super::run_pane::{enter_turn, one_shell_and_a_chat};
use super::*;
use crate::core::ai::chat::{
    approval::{ANOTHER_PENDING, ApprovalAsk, Origin},
    engine::{CallId, Decision, Finish, StreamEvent, ToolStatus, Tools},
};

fn chat(a: &App, id: PaneId) -> &ChatState {
    super::run_pane::chat(a, id)
}

fn run(command: &str) -> ApprovalAsk {
    ApprovalAsk::Run {
        command: command.to_owned(),
    }
}

fn read(path: &str) -> ApprovalAsk {
    ApprovalAsk::Read {
        path: path.to_owned(),
    }
}

/// A shell (pane 1), a focused chat (pane 2) and a turn in flight.
fn live() -> (App, u64) {
    let mut a = one_shell_and_a_chat("hello");
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    (a, turn)
}

fn announce(a: &mut App, turn: u64, call: u64, ask: ApprovalAsk) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::ToolCall {
            id: CallId(call),
            name: "tool".into(),
            summary: "args".into(),
            approval: Some(Box::new(ask)),
        },
    ))
}

fn result(a: &mut App, turn: u64, call: u64, status: ToolStatus) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::ToolResult {
            id: CallId(call),
            status,
            summary: "done".into(),
        },
    ))
}

fn decision(turn: u64, call: u64, decision: Decision) -> Effect {
    Effect::ToolDecision {
        pane: id2(),
        turn,
        call: CallId(call),
        decision,
    }
}

fn pending(a: &App) -> Option<(ApprovalAsk, Option<(u64, CallId)>)> {
    chat(a, id2())
        .approvals()
        .pending()
        .map(|p| (p.ask().clone(), p.call()))
}

fn shell_output(a: &mut App, bytes: &[u8]) {
    a.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(bytes.to_vec()),
    ));
}

// 2e.3 / 2e.4: the ask opens the overlay -------------------------------------

#[test]
fn a_run_ask_opens_the_overlay_aimed_at_the_last_shell() {
    let (mut a, turn) = live();
    assert_eq!(announce(&mut a, turn, 1, run("cd x")), vec![]);
    assert_eq!(pending(&a), Some((run("cd x"), Some((turn, CallId(1))))));
    let approval = chat(&a, id2()).approvals().pending().unwrap();
    assert_eq!(approval.origin(), Origin::Tool);
    assert_eq!(chat(&a, id2()).run_target(), Some(PaneId::FIRST));
    let prompt = a.approval_prompt(id2()).unwrap();
    assert!(prompt.target.starts_with("run in pane 1"), "{prompt:?}");
    assert_eq!(prompt.command, "cd x");
}

// A model that asks for something destructive gets no shortcut: the prompt
// shows the exact command, and nothing is written until `y`.
#[test]
fn a_destructive_command_is_shown_exactly_and_waits_for_y() {
    let (mut a, turn) = live();
    assert_eq!(announce(&mut a, turn, 1, run("rm -rf ~")), vec![]);
    assert_eq!(a.approval_prompt(id2()).unwrap().command, "rm -rf ~");
    for other in [key('Y'), plain(KeyCode::Enter), key(' '), key('a')] {
        assert_eq!(a.update(other), vec![], "only y approves");
        assert!(pending(&a).is_some());
    }
    assert_eq!(
        a.update(key('y')),
        vec![
            Effect::WritePty(PaneId::FIRST, b"rm -rf ~".to_vec()),
            decision(turn, 1, Decision::Sent)
        ]
    );
}

#[test]
fn a_read_ask_shows_read_and_the_path() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, read("/home/u/note.txt"));
    let prompt = a.approval_prompt(id2()).unwrap();
    assert_eq!(prompt.command, "read /home/u/note.txt");
    assert!(prompt.keys.contains("y read it"), "{prompt:?}");
}

// y on a run: one write, then the answer that says it was typed.
#[test]
fn y_types_the_command_and_answers_sent() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    assert_eq!(
        a.update(key('y')),
        vec![
            Effect::WritePty(PaneId::FIRST, b"cd x".to_vec()),
            decision(turn, 1, Decision::Sent)
        ]
    );
    assert_eq!(pending(&a), None);
    // A second y has nothing to answer.
    assert_eq!(a.update(key('y')), vec![]);
}

// The required follow-up: `Sent` means typed. A check that fails when `y`
// comes is a denial with the reason, and nothing is written.
#[test]
fn a_full_screen_app_denies_the_call_with_the_reason_and_writes_nothing() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    shell_output(&mut a, b"\x1b[?1049h");
    assert_eq!(
        a.update(key('y')),
        vec![decision(
            turn,
            1,
            Decision::Denied("the pane runs a full-screen app")
        )]
    );
    assert_eq!(a.notice(), Some("Pane 1 runs a full-screen app"));
    assert_eq!(pending(&a), None);
}

#[test]
fn a_multi_line_command_without_paste_mode_is_denied_with_the_reason() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("echo a\necho b"));
    assert_eq!(
        a.update(key('y')),
        vec![decision(
            turn,
            1,
            Decision::Denied("the shell cannot take a multi-line command safely")
        )]
    );
}

#[test]
fn with_paste_mode_a_multi_line_command_is_typed_framed_and_sent() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("echo a\necho b"));
    shell_output(&mut a, b"\x1b[?2004h");
    let effects = a.update(key('y'));
    let [Effect::WritePty(PaneId::FIRST, bytes), sent] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert!(bytes.starts_with(b"\x1b[200~") && bytes.ends_with(b"\x1b[201~"));
    assert!(!bytes.contains(&b'\r'));
    assert_eq!(sent, &decision(turn, 1, Decision::Sent));
}

// The target is checked when y comes: a pane that is gone keeps the prompt so
// `p` can pick another, and no answer is given meanwhile.
#[test]
fn a_gone_target_keeps_the_prompt_and_answers_nothing() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
    assert_eq!(a.update(key('y')), vec![]);
    assert!(pending(&a).is_some());
}

#[test]
fn n_and_esc_deny_the_call_and_write_nothing() {
    for decline in [key('n'), plain(KeyCode::Esc)] {
        let (mut a, turn) = live();
        announce(&mut a, turn, 1, run("cd x"));
        assert_eq!(
            a.update(decline),
            vec![decision(turn, 1, Decision::Denied("the user declined"))]
        );
        assert_eq!(pending(&a), None);
    }
}

#[test]
fn ctrl_c_denies_the_call_and_cancels_the_turn_never_quitting() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    let effects = a.update(super::chat_live::ctrl('c'));
    assert_eq!(
        effects,
        vec![
            decision(turn, 1, Decision::Denied("cancelled")),
            Effect::ChatCancel(id2())
        ]
    );
    assert_eq!(pending(&a), None);
}

#[test]
fn y_on_a_read_allows_it_without_touching_a_pty() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, read("/home/u/note.txt"));
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    announce(&mut a, turn, 2, read("/home/u/other.txt"));
    assert_eq!(
        a.update(key('n')),
        vec![decision(turn, 2, Decision::Denied("the user declined"))]
    );
}

// One pending approval per chat: the second is denied at once, and the first
// is still the one on screen.
#[test]
fn a_second_ask_is_denied_at_once_and_the_first_stays() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("one"));
    assert_eq!(
        announce(&mut a, turn, 2, read("/home/u/x")),
        vec![decision(turn, 2, Decision::Denied(ANOTHER_PENDING))]
    );
    assert_eq!(pending(&a), Some((run("one"), Some((turn, CallId(1))))));
}

#[test]
fn a_confirm_the_user_opened_also_blocks_a_tool_ask() {
    let (mut a, turn) = live();
    let chat = a.focused_state_mut().chat_mut().unwrap();
    chat.approvals_mut()
        .request(run("mine"), Origin::Block)
        .unwrap();
    assert_eq!(
        announce(&mut a, turn, 1, run("x")),
        vec![decision(turn, 1, Decision::Denied(ANOTHER_PENDING))]
    );
    assert_eq!(pending(&a), Some((run("mine"), None)));
}

// Free tools run around a pending one: their results are rows, they never
// touch the overlay.
#[test]
fn rows_of_free_calls_do_not_disturb_a_pending_prompt() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    for (call, status) in [(2, ToolStatus::Ok), (3, ToolStatus::Error)] {
        a.update(AppEvent::Chat(
            id2(),
            turn,
            StreamEvent::ToolCall {
                id: CallId(call),
                name: "find_dir".into(),
                summary: "q".into(),
                approval: None,
            },
        ));
        result(&mut a, turn, call, status);
    }
    assert_eq!(pending(&a), Some((run("cd x"), Some((turn, CallId(1))))));
}

// 2e.4: a stale prompt goes away ---------------------------------------------

#[test]
fn the_result_of_the_call_withdraws_its_prompt() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    // The client cancelled the call, or the engine gave up on it.
    assert_eq!(result(&mut a, turn, 1, ToolStatus::Denied), vec![]);
    assert_eq!(pending(&a), None);
    assert_eq!(chat(&a, id2()).run_target(), None);
    assert_eq!(a.update(key('y')), vec![], "a late y answers nothing");
}

#[test]
fn the_result_of_another_call_leaves_the_prompt() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    result(&mut a, turn, 9, ToolStatus::Ok);
    assert!(pending(&a).is_some());
}

#[test]
fn a_turn_that_ends_withdraws_the_prompt() {
    for end in [
        StreamEvent::Done(Finish::Stop),
        StreamEvent::Error(crate::core::ai::chat::engine::ChatError::Interrupted),
    ] {
        let (mut a, turn) = live();
        announce(&mut a, turn, 1, run("cd x"));
        a.update(AppEvent::Chat(id2(), turn, end));
        assert_eq!(pending(&a), None);
        assert_eq!(chat(&a, id2()).run_target(), None);
    }
}

#[test]
fn an_ask_from_a_turn_that_is_over_is_denied_not_shown() {
    let (mut a, turn) = live();
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert_eq!(
        announce(&mut a, turn, 1, run("x")),
        vec![decision(turn, 1, Decision::Denied("the turn is over"))]
    );
    assert_eq!(pending(&a), None);
}

// 2e.4: tools off, no shell, user tools ---------------------------------------

#[test]
fn an_ask_with_tools_off_is_denied() {
    let mut ai = ollama();
    ai.set_tools_enabled(false);
    let mut a = launched(80, 25).with_ai(ai);
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hello");
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    assert_eq!(
        announce(&mut a, turn, 1, run("x")),
        vec![decision(turn, 1, Decision::Denied("tools are off"))]
    );
}

#[test]
fn turning_tools_off_denies_the_call_that_waits() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    a.ai.set_tools_enabled(false);
    let effects = a.sync_tools();
    assert!(
        effects.contains(&decision(turn, 1, Decision::Denied("tools are off"))),
        "{effects:?}"
    );
    assert_eq!(pending(&a), None);
}

#[test]
fn a_user_tool_ask_is_denied_until_it_is_supported() {
    let (mut a, turn) = live();
    let ask = ApprovalAsk::External {
        server: "s".into(),
        tool: "t".into(),
        args: "{}".into(),
    };
    assert_eq!(
        announce(&mut a, turn, 1, ask),
        vec![decision(
            turn,
            1,
            Decision::Denied("user tools are not available")
        )]
    );
    assert_eq!(pending(&a), None);
}

// The chat's own tab decides the target, not the tab on screen.
#[test]
fn the_target_is_a_shell_of_the_chats_tab_not_the_active_one() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "tn");
    let second_shell = a.focused();
    press(&mut a, "c");
    let chat_pane = a.focused();
    super::run_pane::answer(&mut a, chat_pane, "hello");
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    press(&mut a, "gB");
    assert_ne!(a.focused(), chat_pane, "the other tab is on screen");
    a.update(AppEvent::Chat(
        chat_pane,
        turn,
        StreamEvent::ToolCall {
            id: CallId(1),
            name: "run_in_pane".into(),
            summary: "cd x".into(),
            approval: Some(Box::new(run("cd x"))),
        },
    ));
    let state = super::run_pane::chat(&a, chat_pane);
    assert!(state.approvals().pending().is_some());
    assert_eq!(state.run_target(), Some(second_shell));
}

#[test]
fn a_chat_whose_tab_has_no_shell_denies_a_run() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.open_chat_tab(None);
    let chat_pane = a.focused();
    super::run_pane::answer(&mut a, chat_pane, "hello");
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    let effects = a.update(AppEvent::Chat(
        chat_pane,
        turn,
        StreamEvent::ToolCall {
            id: CallId(1),
            name: "run_in_pane".into(),
            summary: "x".into(),
            approval: Some(Box::new(run("x"))),
        },
    ));
    assert_eq!(
        effects,
        vec![Effect::ToolDecision {
            pane: chat_pane,
            turn,
            call: CallId(1),
            decision: Decision::Denied("there is no shell pane in this tab"),
        }]
    );
}

// 2e.4: the end of the session ------------------------------------------------

/// Like `live` for an app whose engine offers tools.
fn live_with_session() -> (App, u64) {
    let mut a = one_shell_and_a_chat("hello").with_tool_engines(&["ollama"]);
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    (a, turn)
}

#[test]
fn a_turn_of_a_verified_engine_carries_tools_and_the_origin() {
    let mut a = launched(80, 25)
        .with_ai(ollama())
        .with_tool_engines(&["ollama"]);
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hi");
    a.update(key('q'));
    let effects = a.update(plain(KeyCode::Enter));
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(
        request.tools,
        Tools::On {
            origin: chat(&a, id2()).origin_cwd().map(Path::to_path_buf)
        }
    );
}

#[test]
fn an_unverified_engine_or_tools_off_send_tools_off() {
    let (mut a, _) = live();
    a.update(AppEvent::Chat(id2(), 2, StreamEvent::Done(Finish::Stop)));
    a.update(key('q'));
    let effects = a.update(plain(KeyCode::Enter));
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(request.tools, Tools::Off);
}

#[test]
fn cancelling_a_turn_with_a_session_ends_the_session_too() {
    let (mut a, _turn) = live_with_session();
    assert_eq!(
        a.update(plain(KeyCode::Esc)),
        vec![Effect::ChatCancel(id2()), Effect::EndToolSession(id2())]
    );
    // Once ended, a second cancel has no session left to end.
    assert_eq!(a.update(plain(KeyCode::Esc)), vec![]);
}

#[test]
fn closing_a_chat_with_a_session_ends_it_with_the_cancel() {
    let (mut a, _turn) = live_with_session();
    press(&mut a, "wq");
    // The close is confirmed with `y`.
    let effects = a.update(key('y'));
    assert!(
        effects.contains(&Effect::ChatCancel(id2()))
            && effects.contains(&Effect::EndToolSession(id2())),
        "{effects:?}"
    );
}

#[test]
fn a_chat_that_never_had_a_session_emits_no_end() {
    let (mut a, _turn) = live();
    assert_eq!(
        a.update(plain(KeyCode::Esc)),
        vec![Effect::ChatCancel(id2())]
    );
}

// The two halves together: the host announces a call, the app shows it and
// answers on `y`, the answer reaches the call. What the model is told follows
// what really happened to the pane.
#[tokio::test]
async fn what_the_model_is_told_follows_what_happened_to_the_pane() {
    use crate::{
        core::ai::tools::{Call, Locality},
        runtime::tools::{Limits, Pending, ToolHost, TurnScope, Waits},
    };
    for full_screen in [false, true] {
        let (mut a, turn) = live();
        if full_screen {
            shell_output(&mut a, b"\x1b[?1049h");
        }
        let waits = Waits::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut scope = TurnScope::new(tx, Locality::Cloud, PathBuf::from("/"));
        scope.approver = waits.approver(id2(), turn, Pending::new());
        let host = ToolHost::new(None, None, None, Limits::default());
        let call = tokio::spawn(host.dispatch(scope).call(Call::RunInPane {
            command: "cd x".into(),
        }));
        a.update(AppEvent::Chat(id2(), turn, rx.recv().await.unwrap()));
        let effects = a.update(key('y'));
        let wrote = effects.iter().any(|e| matches!(e, Effect::WritePty(..)));
        for effect in effects {
            if let Effect::ToolDecision {
                pane,
                turn,
                call,
                decision,
            } = effect
            {
                assert!(waits.decide(pane, turn, call, decision));
            }
        }
        let out = call.await.unwrap();
        let StreamEvent::ToolResult {
            status, summary, ..
        } = rx.recv().await.unwrap()
        else {
            panic!("no result row")
        };
        if full_screen {
            assert!(!wrote && out.is_error);
            assert_eq!(out.text, "denied: the pane runs a full-screen app");
            assert_eq!(
                (status, summary.as_str()),
                (ToolStatus::Denied, "the pane runs a full-screen app")
            );
        } else {
            assert!(wrote && !out.is_error && out.text.contains("typed"));
            assert_eq!(status, ToolStatus::Ok);
        }
    }
}
