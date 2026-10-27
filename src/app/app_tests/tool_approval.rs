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
use crate::core::ai::{
    config::ToolToggle,
    tools::registry::{Family, FamilySet},
};

pub(super) fn chat(a: &App, id: PaneId) -> &ChatState {
    super::run_pane::chat(a, id)
}

pub(super) fn run(command: &str) -> ApprovalAsk {
    ApprovalAsk::Run {
        command: command.to_owned(),
    }
}

pub(super) fn read(path: &str) -> ApprovalAsk {
    ApprovalAsk::Read {
        path: path.to_owned(),
    }
}

/// A shell (pane 1), a focused chat (pane 2) and a turn in flight.
pub(super) fn live() -> (App, u64) {
    NOW_MS.with(|n| n.set(0));
    let mut a = one_shell_and_a_chat("hello").with_clock(mock_now);
    // The tests model a platform that tells the pane's cwd (`/proc`); the
    // default is false elsewhere, so macOS would route no run at all.
    a.cwd_readable = true;
    a.update(key('q'));
    let turn = enter_turn(&mut a);
    (a, turn)
}

/// The ask arrives and the prompt's delay passes (see `guard` for the delay).
pub(super) fn announce(a: &mut App, turn: u64, call: u64, ask: ApprovalAsk) -> Vec<Effect> {
    let effects = announce_now(a, turn, call, ask);
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    effects
}

pub(super) fn announce_now(a: &mut App, turn: u64, call: u64, ask: ApprovalAsk) -> Vec<Effect> {
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

pub(super) fn result(a: &mut App, turn: u64, call: u64, status: ToolStatus) -> Vec<Effect> {
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

pub(super) fn decision(turn: u64, call: u64, decision: Decision) -> Effect {
    Effect::ToolDecision {
        pane: id2(),
        turn,
        call: CallId(call),
        decision,
    }
}

pub(super) fn pending(a: &App) -> Option<(ApprovalAsk, Option<(u64, CallId)>)> {
    chat(a, id2())
        .approvals()
        .pending()
        .map(|p| (p.ask().clone(), p.call()))
}

pub(super) fn shell_output(a: &mut App, bytes: &[u8]) {
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

fn external(server: &str, tool: &str, args: &str) -> ApprovalAsk {
    ApprovalAsk::External {
        server: server.into(),
        tool: tool.into(),
        args: args.into(),
    }
}

#[test]
fn a_user_tool_ask_shows_the_server_the_tool_and_the_arguments() {
    let (mut a, turn) = live();
    assert_eq!(
        announce(&mut a, turn, 1, external("fs", "write", "path=~/x")),
        Vec::new()
    );
    let prompt = a.approval_prompt(id2()).unwrap();
    assert!(prompt.target.contains("fs"), "{prompt:?}");
    assert_eq!(prompt.command, "fs write path=~/x");
    assert!(prompt.keys.contains("y call it"), "{prompt:?}");
    // A second server and tool give a different prompt.
    let (mut b, turn) = live();
    announce(&mut b, turn, 1, external("docs", "search", "q=cats"));
    assert_eq!(
        b.approval_prompt(id2()).unwrap().command,
        "docs search q=cats"
    );
}

#[test]
fn y_on_a_user_tool_answers_sent_and_n_denies_without_a_pty() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, external("fs", "write", "{}"));
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    announce(&mut a, turn, 2, external("fs", "write", "{}"));
    assert_eq!(
        a.update(key('n')),
        vec![decision(turn, 2, Decision::Denied("the user declined"))]
    );
}

fn search(pattern: &str, dir: &str) -> ApprovalAsk {
    ApprovalAsk::Search {
        pattern: pattern.into(),
        dir: dir.into(),
    }
}

// Spec (code-search, grep): the prompt shows the pattern and where it looks.
#[test]
fn a_search_ask_shows_the_pattern_and_the_path() {
    let (mut a, turn) = live();
    assert_eq!(
        announce(&mut a, turn, 1, search("fn main", ".")),
        Vec::new()
    );
    let prompt = a.approval_prompt(id2()).unwrap();
    assert_eq!(prompt.command, "search fn main in .");
    assert!(prompt.keys.contains("y search"), "{prompt:?}");
    let (mut b, turn) = live();
    announce(&mut b, turn, 1, search("TODO", "src/core"));
    assert_eq!(
        b.approval_prompt(id2()).unwrap().command,
        "search TODO in src/core"
    );
}

#[test]
fn y_on_a_search_answers_sent_and_n_denies_and_auto_mode_still_asks() {
    let (mut a, turn) = live();
    super::tool_run::chat_mut(&mut a)
        .set_approval_mode(crate::core::ai::config::ApprovalMode::Auto);
    assert_eq!(announce(&mut a, turn, 1, search("x", ".")), Vec::new());
    assert_eq!(pending(&a).map(|(ask, _)| ask), Some(search("x", ".")));
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    announce(&mut a, turn, 2, search("y", "."));
    assert_eq!(
        a.update(key('n')),
        vec![decision(turn, 2, Decision::Denied("the user declined"))]
    );
}

// D18: the approval mode is for `run_in_pane` only; a user tool never types
// or approves by itself.
#[test]
fn a_user_tool_asks_for_y_even_in_auto_mode() {
    let (mut a, turn) = live();
    super::tool_run::chat_mut(&mut a)
        .set_approval_mode(crate::core::ai::config::ApprovalMode::Auto);
    assert_eq!(
        announce(&mut a, turn, 1, external("fs", "write", "{}")),
        Vec::new()
    );
    assert_eq!(
        pending(&a).map(|(ask, _)| ask),
        Some(external("fs", "write", "{}"))
    );
    assert_eq!(
        a.update(super::chat_live::ctrl('c')).first(),
        Some(&decision(turn, 1, Decision::Denied("cancelled")))
    );
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
    NOW_MS.with(|n| n.set(0));
    let mut a = one_shell_and_a_chat("hello")
        .with_clock(mock_now)
        .with_tool_engines(&["ollama"]);
    a.cwd_readable = true;
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
            origin: chat(&a, id2()).origin_cwd().map(Path::to_path_buf),
            external: Default::default(),
            mcp: None,
        }
    );
}

/// Sends one turn from a chat whose engine offers tools, with `ai`.
fn families_of_a_turn(ai: AiConfig) -> FamilySet {
    families_of_a_turn_on(ai, &["ollama"])
}

/// Sends one turn from a chat, with `ai` and the engines verified for tools.
fn families_of_a_turn_on(ai: AiConfig, engines: &'static [&'static str]) -> FamilySet {
    let mut a = launched(80, 25).with_ai(ai).with_tool_engines(engines);
    press(&mut a, "c");
    super::run_pane::answer(&mut a, id2(), "hi");
    a.update(key('q'));
    let effects = a.update(plain(KeyCode::Enter));
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    request.families
}

#[test]
fn a_turn_offers_the_families_of_the_toggles() {
    assert_eq!(families_of_a_turn(ollama()), FamilySet::LEGACY);
    let mut ai = ollama();
    ai.set_toggle(ToolToggle::Search, true);
    ai.set_toggle(ToolToggle::GitWrite, true);
    assert_eq!(
        families_of_a_turn(ai),
        FamilySet::LEGACY
            .with(Family::Search)
            .with(Family::GitWrite)
    );
}

#[test]
fn a_turn_without_tools_offers_no_coding_family() {
    let mut ai = ollama();
    ai.set_toggle(ToolToggle::Exec, true);
    assert_eq!(families_of_a_turn_on(ai, &[]), FamilySet::LEGACY);
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
        NOW_MS.with(|n| n.set(1_000));
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

// -- the prompt must show everything `y` would approve ------------------------

const TOO_SMALL: &str = "Pane too small to show it all: enlarge it or press n";

fn resize(a: &mut App, cols: u16, rows: u16) {
    a.update(AppEvent::Resize { cols, rows });
}

/// About 190 characters: six rows in the chat pane at 80 columns split in two,
/// so the prompt needs eight rows and a short pane cannot show it.
fn long_path() -> String {
    format!("/home/u/{}", "d".repeat(180))
}

#[test]
fn y_is_ignored_when_the_path_cannot_be_shown_in_full_and_n_still_denies() {
    for deny in [key('n'), plain(KeyCode::Esc), super::chat_live::ctrl('c')] {
        let (mut a, turn) = live();
        // More than six rows of text: no pane is tall enough.
        announce(&mut a, turn, 1, read(&format!("/{}", "d".repeat(500))));
        assert_eq!(a.update(key('y')), vec![], "y approves nothing");
        assert_eq!(a.notice(), Some(TOO_SMALL));
        assert!(pending(&a).is_some(), "the prompt stays");
        let effects = a.update(deny);
        assert!(
            matches!(
                effects.first(),
                Some(Effect::ToolDecision {
                    decision: Decision::Denied(_),
                    ..
                })
            ),
            "{effects:?}"
        );
        assert_eq!(pending(&a), None);
    }
}

#[test]
fn a_short_pane_ignores_y_and_enlarging_it_enables_y_again() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, read(&long_path()));
    resize(&mut a, 80, 8);
    assert_eq!(a.update(key('y')), vec![]);
    assert!(pending(&a).is_some());
    resize(&mut a, 80, 30);
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
}

#[test]
fn a_narrow_pane_with_a_path_of_100_characters_needs_height_for_it() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, read(&format!("/{}", "d".repeat(99))));
    resize(&mut a, 40, 6);
    assert_eq!(a.update(key('y')), vec![]);
    resize(&mut a, 40, 25);
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
}

#[test]
fn a_very_short_pane_never_approves() {
    for rows in [3, 4, 5] {
        let (mut a, turn) = live();
        announce(&mut a, turn, 1, run("echo a\necho b\necho c"));
        resize(&mut a, 80, rows);
        let effects = a.update(key('y'));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::WritePty(..))),
            "{rows}: {effects:?}"
        );
        assert!(pending(&a).is_some(), "{rows}");
    }
}

#[test]
fn a_model_command_over_the_row_budget_is_never_typed() {
    let (mut a, turn) = live();
    let command = (1..=8)
        .map(|n| format!("echo {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    announce(&mut a, turn, 1, run(&command));
    shell_output(&mut a, b"\x1b[?2004h");
    assert_eq!(a.update(key('y')), vec![]);
    assert_eq!(a.notice(), Some(TOO_SMALL));
    assert!(pending(&a).is_some());
}

// Turning tools off mid-turn stops the tools at once: the session closes, so
// free tools stop too, not only the call that waits.
#[test]
fn turning_tools_off_ends_the_tool_session_once() {
    let (mut a, _turn) = live_with_session();
    a.ai.set_tools_enabled(false);
    assert_eq!(a.sync_tools(), vec![Effect::EndToolSession(id2())]);
    assert!(!chat(&a, id2()).tool_session());
    assert_eq!(a.sync_tools(), vec![], "nothing left to end");
}

#[test]
fn turning_tools_off_ends_the_session_after_denying_the_waiting_call() {
    let (mut a, turn) = live_with_session();
    announce(&mut a, turn, 1, run("cd x"));
    a.ai.set_tools_enabled(false);
    assert_eq!(
        a.sync_tools(),
        vec![
            decision(turn, 1, Decision::Denied("tools are off")),
            Effect::EndToolSession(id2())
        ]
    );
}

#[test]
fn turning_tools_on_or_a_chat_without_a_session_ends_nothing() {
    let (mut a, _) = live_with_session();
    assert_eq!(a.sync_tools(), vec![]);
    let (mut b, _) = live();
    b.ai.set_tools_enabled(false);
    assert_eq!(b.sync_tools(), vec![]);
}

// -- a key typed before the prompt appeared cannot answer it -------------------

mod guard {
    use super::*;
    use crate::core::ai::chat::approval::ASK_ARM_DELAY as ARM_DELAY;

    fn ms(d: std::time::Duration) -> u64 {
        u64::try_from(d.as_millis()).unwrap()
    }

    /// `live` with the injected clock at t = 0; the ask arrives at t = 0.
    fn asked(ask: ApprovalAsk) -> (App, u64) {
        NOW_MS.with(|n| n.set(0));
        let mut a = one_shell_and_a_chat("hello").with_clock(mock_now);
        a.update(key('q'));
        let turn = enter_turn(&mut a);
        announce_now(&mut a, turn, 1, ask);
        (a, turn)
    }

    #[test]
    fn keys_that_answer_do_nothing_until_the_prompt_is_armed() {
        for answer in [key('y'), key('n'), plain(KeyCode::Esc)] {
            let (mut a, _) = asked(run("cd x"));
            assert_eq!(at(&mut a, 0, answer.clone()), vec![]);
            assert_eq!(at(&mut a, ms(ARM_DELAY) - 1, answer), vec![]);
            assert!(pending(&a).is_some(), "still waiting");
        }
    }

    #[test]
    fn a_y_after_the_delay_approves() {
        let (mut a, turn) = asked(run("cd x"));
        assert_eq!(at(&mut a, 10, key('y')), vec![]);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), key('y')),
            vec![
                Effect::WritePty(PaneId::FIRST, b"cd x".to_vec()),
                decision(turn, 1, Decision::Sent)
            ]
        );
    }

    #[test]
    fn a_read_is_guarded_the_same_way() {
        let (mut a, turn) = asked(read("/home/u/a.txt"));
        assert_eq!(at(&mut a, 5, key('y')), vec![]);
        assert_eq!(
            at(&mut a, ms(ARM_DELAY), key('y')),
            vec![decision(turn, 1, Decision::Sent)]
        );
    }

    #[test]
    fn ctrl_c_cancels_at_once() {
        let (mut a, turn) = asked(run("cd x"));
        let effects = at(&mut a, 0, super::super::chat_live::ctrl('c'));
        assert_eq!(
            effects,
            vec![
                decision(turn, 1, Decision::Denied("cancelled")),
                Effect::ChatCancel(id2())
            ]
        );
    }

    #[test]
    fn the_keys_line_is_dots_until_armed() {
        let (a, _) = asked(run("cd x"));
        assert!(!a.approval_prompt(id2()).unwrap().armed);
        NOW_MS.with(|n| n.set(ms(ARM_DELAY)));
        assert!(a.approval_prompt(id2()).unwrap().armed);
    }

    // The confirm the user opens with `Prefix e` is theirs: no delay.
    #[test]
    fn the_users_own_confirm_answers_at_once() {
        NOW_MS.with(|n| n.set(0));
        let mut a = one_shell_and_a_chat(super::super::run_pane::BLOCK).with_clock(mock_now);
        at(
            &mut a,
            0,
            AppEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)),
        );
        at(&mut a, 0, key('e'));
        assert!(pending(&a).is_some());
        assert!(a.approval_prompt(id2()).unwrap().armed);
        let effects = at(&mut a, 0, key('y'));
        assert!(
            matches!(effects.as_slice(), [Effect::WritePty(..)]),
            "{effects:?}"
        );
    }
}

fn exec(program: &str, args: &[&str], cwd: &str) -> ApprovalAsk {
    ApprovalAsk::Exec {
        program: program.into(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: cwd.into(),
        script: None,
        timeout_secs: 120,
        cloud: false,
        earlier: None,
    }
}

// Spec (command-runner, Approval on every command): a command asks for `y` in
// every mode, shows what will run and where, and nothing is typed.
#[test]
fn a_command_asks_for_y_in_auto_mode_and_shows_the_program_the_arguments_and_the_folder() {
    let (mut a, turn) = live();
    super::tool_run::chat_mut(&mut a)
        .set_approval_mode(crate::core::ai::config::ApprovalMode::Auto);
    let ask = exec("cargo", &["test", "-p", "x"], "crates/x");
    assert_eq!(announce(&mut a, turn, 1, ask.clone()), Vec::new());
    assert_eq!(pending(&a).map(|(ask, _)| ask), Some(ask));
    let prompt = a.approval_prompt(id2()).unwrap();
    assert_eq!(prompt.command, "cargo test -p x\nin crates/x");
    assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    announce(&mut a, turn, 2, exec("go", &["test"], "."));
    assert_eq!(
        a.update(key('n')),
        vec![decision(turn, 2, Decision::Denied("the user declined"))]
    );
}

fn shown(ask: ApprovalAsk) -> crate::core::ai::chat::approval::ApprovalPrompt {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, ask);
    a.approval_prompt(id2()).unwrap()
}

// What is shown is what the program receives: an argument that is empty or has
// a space or a quote shows quoted, so two arguments never read as one.
#[test]
fn an_argument_with_a_space_a_quote_or_nothing_is_shown_quoted() {
    let ask = exec(
        "cargo",
        &["test", "--", "x y", "", "it's", "a\"b", "plain-1"],
        ".",
    );
    assert_eq!(
        shown(ask).command,
        "cargo test -- 'x y' '' 'it'\\''s' 'a\"b' plain-1\nin ."
    );
}

#[test]
fn a_folder_with_a_space_is_shown_quoted_too() {
    assert_eq!(
        shown(exec("go", &["test"], "my dir")).command,
        "go test\nin 'my dir'"
    );
}

#[test]
fn the_script_a_package_manager_will_run_is_shown_under_the_command() {
    let ask = ApprovalAsk::Exec {
        program: "npm".into(),
        args: vec!["run".into(), "build".into()],
        cwd: ".".into(),
        script: Some("prebuild: rm -rf dist\nbuild: tsc".into()),
        timeout_secs: 120,
        cloud: false,
        earlier: None,
    };
    assert_eq!(
        shown(ask).command,
        "npm run build\nin .\nscript prebuild: rm -rf dist\nscript build: tsc"
    );
}

// The folder is part of the text `fits` checks, so a pane too small to show it
// all does not let `y` through (it used to sit in a clipped head row).
#[test]
fn a_long_folder_is_checked_by_fits_and_not_clipped() {
    use crate::core::ai::chat::approval_rows::{Part, fits, parts};
    let cwd = format!("{}/deep", "d".repeat(150));
    let prompt = shown(exec("cargo", &["test"], &cwd));
    assert!(!fits(&prompt, 20, 12));
    assert!(fits(&prompt, 80, 12));
    let rows = parts(&prompt, 80, 12);
    assert!(
        rows.iter()
            .any(|row| matches!(row, Part::Command(t) if t.contains("deep"))),
        "{rows:?}"
    );
}

// Spec (command-runner, Approval on every command): the prompt shows the
// network and `.env` notice, whatever the approval mode.
#[test]
fn the_prompt_carries_the_notice_and_nothing_hides_it_from_fits() {
    use crate::core::ai::chat::approval_rows::{Part, fits, parts};
    let prompt = shown(exec("cargo", &["test"], "."));
    let notice = prompt
        .notice
        .clone()
        .expect("a command prompt has a notice");
    assert!(
        notice.contains(".env") && notice.contains("network is on"),
        "{notice}"
    );
    assert!(fits(&prompt, 100, 20));
    // A pane too short for the command AND the notice does not let `y` through.
    assert!(!fits(&prompt, 100, 5));
    let rows = parts(&prompt, 100, 20);
    assert!(
        rows.iter()
            .any(|row| matches!(row, Part::Notice(t) if t.contains(".env"))),
        "{rows:?}"
    );
}

// The notice is on screen whatever the approval mode: every mode asks.
#[test]
fn a_command_prompts_with_the_notice_in_every_approval_mode() {
    use crate::core::ai::config::ApprovalMode;
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        let (mut a, turn) = live();
        super::tool_run::chat_mut(&mut a).set_approval_mode(mode);
        assert_eq!(
            announce(&mut a, turn, 1, exec("cargo", &["test"], ".")),
            Vec::new()
        );
        let prompt = a.approval_prompt(id2()).unwrap();
        let notice = prompt.notice.expect("a notice");
        assert!(notice.contains("network is on"), "{mode:?}: {notice}");
        assert_eq!(a.update(key('y')), vec![decision(turn, 1, Decision::Sent)]);
    }
}

// The host caps the earlier-run line at 240 columns whatever the paths are
// (see `exec_tests`); a line of that size must leave the prompt answerable in
// a normal pane.
#[test]
fn a_prompt_with_a_capped_earlier_run_line_still_fits_a_normal_pane() {
    use crate::core::ai::chat::approval_rows::fits;
    let mut ask = exec("npm", &["install"], ".");
    if let ApprovalAsk::Exec { earlier, .. } = &mut ask {
        *earlier = Some("p".repeat(240));
    }
    let prompt = shown(ask);
    assert!(fits(&prompt, 100, 30), "{:?}", prompt.notice);
    assert!(fits(&prompt, 80, 30), "{:?}", prompt.notice);
}

#[test]
fn a_cloud_command_prompt_names_the_provider() {
    let mut ask = exec("cargo", &["test"], ".");
    if let ApprovalAsk::Exec { cloud, .. } = &mut ask {
        *cloud = true;
    }
    let notice = shown(ask).notice.unwrap();
    assert!(notice.contains("provider"), "{notice}");
}

#[test]
fn the_prompt_shows_the_timeout_of_the_run() {
    let mut ask = exec("go", &["test"], ".");
    if let ApprovalAsk::Exec { timeout_secs, .. } = &mut ask {
        *timeout_secs = 600;
    }
    assert!(shown(ask).notice.unwrap().contains("timeout 600 s"));
}

#[test]
fn an_argument_a_shell_would_read_is_shown_quoted() {
    for arg in [
        ";", "a|b", "a&b", "<x", "(x)", "*", "a?", "#c", "~", "{a,b}", "!x", "[a]", "a]", "^x",
    ] {
        let shown = shown(exec("cargo", &["test", arg], ".")).command;
        assert!(
            shown.starts_with(&format!("cargo test '{arg}'")),
            "{arg}: {shown}"
        );
    }
}

fn exec_row(a: &App) -> Option<String> {
    use crate::core::menu::Row;
    a.menu
        .as_ref()?
        .all_rows(&a.config)
        .into_iter()
        .find_map(|row| match row {
            Row::Item {
                label: "Exec",
                value: Some(value),
                ..
            } => Some(value.into_owned()),
            _ => None,
        })
}

fn menu_with_exec_on(jail: crate::core::ai::jail::JailSupport) -> App {
    use crate::core::ai::config::ToolToggle;
    let mut ai = crate::core::ai::config::AiConfig::default();
    ai.set_toggle(ToolToggle::Exec, true);
    let mut a = App::new(80, 25).with_ai(ai).with_jail(jail);
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::CONTROL,
    )));
    a.update(key('m'));
    a
}

// Design D18: the menu row says what the refusal will say.
#[test]
fn the_exec_row_says_why_when_the_system_has_no_landlock_jail() {
    use crate::core::ai::jail::{Backend, JailOff, JailSupport};
    let landlock = JailSupport::Available {
        backend: Backend::Landlock,
        abi: 4,
    };
    assert_eq!(
        exec_row(&menu_with_exec_on(landlock)).as_deref(),
        Some("on")
    );
    for off in [
        JailSupport::Unavailable(JailOff::NoLandlock(2)),
        JailSupport::Unavailable(JailOff::NoBackend),
        JailSupport::Unavailable(JailOff::NotProbed),
    ] {
        assert_eq!(
            exec_row(&menu_with_exec_on(off)).as_deref(),
            Some("on · needs Linux sandbox"),
            "{off:?}"
        );
    }
}
