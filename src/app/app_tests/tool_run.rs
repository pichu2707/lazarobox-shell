//! The `run` mode (pane-execution, design D21): a model's safe command is
//! routed to a check, and every other case is the prompt, where `y` types
//! and never presses Enter.

use super::tool_approval::{announce, announce_now, live, pending, result, run};
use super::*;
use crate::app::RunEvent;
use crate::core::ai::{
    chat::{
        approval::MAX_AUTO_PER_TURN,
        engine::{CallId, Decision, ToolStatus},
    },
    config::ApprovalMode,
};

/// `live()` with the chat in `run` mode: shell pane 1 (unfocused, its cwd
/// known), the focused chat is pane 2.
fn run_live() -> (App, u64) {
    let (mut a, turn) = live();
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(ApprovalMode::Run);
    (a, turn)
}

fn check(turn: u64, call: u64, command: &str) -> Effect {
    Effect::CheckRun {
        chat: id2(),
        target: PaneId::FIRST,
        turn,
        call: CallId(call),
        command: command.to_owned(),
    }
}

fn chat_of(a: &App) -> &ChatState {
    super::run_pane::chat(a, id2())
}

fn slot(a: &App) -> Option<(PaneId, u64, CallId, String)> {
    let pty = a.panes.get(&PaneId::FIRST).and_then(PaneState::pty)?;
    let slot = pty.run.as_ref()?;
    Some((slot.chat, slot.turn, slot.call, slot.command.clone()))
}

fn why(a: &App) -> Option<String> {
    let approval = chat_of(a).approvals().pending()?;
    approval.why().map(str::to_owned)
}

fn checked(a: &mut App, call: u64) -> Vec<Effect> {
    a.update(AppEvent::Run(
        PaneId::FIRST,
        RunEvent::Checked {
            call: CallId(call),
            cwd: LAUNCH.into(),
            cd_target: None,
        },
    ))
}

fn refused(a: &mut App, call: u64, reason: &'static str) -> Vec<Effect> {
    a.update(AppEvent::Run(
        PaneId::FIRST,
        RunEvent::Refused {
            call: CallId(call),
            reason,
        },
    ))
}

// S3.3: the route reserves the slot and asks the runtime to check.
#[test]
fn a_safe_command_reserves_the_slot_counts_and_asks_for_the_check() {
    let (mut a, turn) = run_live();
    assert_eq!(
        announce(&mut a, turn, 1, run("cd ~/dev")),
        vec![check(turn, 1, "cd ~/dev")]
    );
    assert_eq!(pending(&a), None, "no prompt opens");
    assert_eq!(
        slot(&a),
        Some((id2(), turn, CallId(1), "cd ~/dev".to_owned()))
    );
    assert_eq!(chat_of(&a).auto_typed(), 1);
}

#[test]
fn a_second_call_to_the_same_pane_in_a_batch_prompts() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("ls"));
    assert_eq!(announce(&mut a, turn, 2, run("pwd")), vec![]);
    assert_eq!(pending(&a).map(|(ask, _)| ask), Some(run("pwd")));
    assert_eq!(chat_of(&a).auto_typed(), 1, "a prompt takes no slot");
    assert_eq!(slot(&a).map(|s| s.2), Some(CallId(1)), "the first stays");
}

// `y` types, and an off-list command never presses Enter.
#[test]
fn an_off_list_command_prompts_and_y_types_without_enter() {
    let (mut a, turn) = run_live();
    assert_eq!(announce(&mut a, turn, 1, run("git status")), vec![]);
    assert_eq!(why(&a).as_deref(), Some("not a safe command"));
    assert_eq!(slot(&a), None);
    assert_eq!(chat_of(&a).auto_typed(), 0);
    let effects = a.update(key('y'));
    assert_eq!(
        effects,
        vec![
            Effect::WritePty(PaneId::FIRST, b"git status".to_vec()),
            super::tool_approval::decision(turn, 1, Decision::Sent)
        ]
    );
    for effect in &effects {
        if let Effect::WritePty(_, bytes) = effect {
            assert!(!bytes.contains(&b'\r') && !bytes.ends_with(b"\n"));
        }
    }
}

// S3.7: no real cwd, no run.
#[test]
fn a_pane_whose_cwd_is_unknown_prompts_with_the_reason() {
    let (mut a, turn) = run_live();
    a.panes
        .get_mut(&PaneId::FIRST)
        .and_then(PaneState::pty_mut)
        .unwrap()
        .proc_cwd = None;
    assert_eq!(announce(&mut a, turn, 1, run("ls src")), vec![]);
    assert_eq!(
        why(&a).as_deref(),
        Some("the pane's directory cannot be read on this system")
    );
    assert_eq!(slot(&a), None);
}

#[test]
fn a_focused_target_prompts_and_takes_no_slot() {
    let (mut a, turn) = run_live();
    a.tab_mut().focus = PaneId::FIRST;
    assert_eq!(announce(&mut a, turn, 1, run("pwd")), vec![]);
    assert!(pending(&a).is_some());
    assert_eq!((slot(&a), chat_of(&a).auto_typed()), (None, 0));
}

// S3.3: the count is reserved at the route and never refunded.
#[test]
fn the_slot_counts_toward_the_cap_and_a_refusal_does_not_refund_it() {
    let (mut a, turn) = run_live();
    for _ in 1..MAX_AUTO_PER_TURN {
        chat_mut(&mut a).note_auto_typed();
    }
    assert_eq!(
        announce(&mut a, turn, 1, run("pwd")),
        vec![check(turn, 1, "pwd")]
    );
    assert_eq!(chat_of(&a).auto_typed(), MAX_AUTO_PER_TURN);
    refused(&mut a, 1, "the path is not allowed");
    assert_eq!(chat_of(&a).auto_typed(), MAX_AUTO_PER_TURN, "no refund");
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(announce(&mut a, turn, 2, run("pwd")), vec![]);
    assert_eq!(why(&a), None, "the cap is a limit, not a fallback");
}

fn chat_mut(a: &mut App) -> &mut ChatState {
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
}

// S3.4: a refused check and a stale answer.
#[test]
fn a_refused_check_opens_the_prompt_with_its_reason_and_frees_the_slot() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/.ssh"));
    assert_eq!(refused(&mut a, 1, "the path is not allowed"), vec![]);
    assert_eq!(
        pending(&a),
        Some((run("cd ~/.ssh"), Some((turn, CallId(1)))))
    );
    assert_eq!(why(&a).as_deref(), Some("the path is not allowed"));
    assert_eq!(slot(&a), None);
    assert_eq!(chat_of(&a).run_target(), Some(PaneId::FIRST));
}

#[test]
fn an_answer_for_a_call_with_no_slot_is_ignored() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    assert_eq!(refused(&mut a, 7, "late"), vec![]);
    assert_eq!(checked(&mut a, 7), vec![]);
    assert_eq!(pending(&a), None);
    assert_eq!(slot(&a).map(|s| s.2), Some(CallId(1)), "the slot stays");
}

// The route runs again after the check: a pane that took the focus meanwhile
// prompts.
#[test]
fn the_route_runs_again_after_the_check() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    a.tab_mut().focus = PaneId::FIRST;
    assert_eq!(checked(&mut a, 1), vec![]);
    assert_eq!(pending(&a).map(|(ask, _)| ask), Some(run("pwd")));
    assert_eq!(why(&a), None);
    assert_eq!(slot(&a), None);
}

// Until the write exists a checked run still prompts (S3b replaces this).
#[test]
fn a_checked_run_never_types_or_presses_enter_yet() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    assert_eq!(checked(&mut a, 1), vec![]);
    assert!(pending(&a).is_some());
}

// S3.10: cleanup.
#[test]
fn the_end_of_the_turn_the_cancel_and_the_result_free_the_slot() {
    use crate::core::ai::chat::engine::Finish;
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    result(&mut a, turn, 1, ToolStatus::Ok);
    assert_eq!(slot(&a), None, "the call's result");

    announce(&mut a, turn, 2, run("pwd"));
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert_eq!(slot(&a), None, "the turn ended");
}

#[test]
fn a_cancel_and_a_closed_chat_free_the_slot() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    a.cancel_chat(id2());
    assert_eq!(slot(&a), None, "cancelled");

    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    a.remove_pane(id2());
    assert_eq!(slot(&a), None, "the chat closed");
    assert_eq!(checked(&mut a, 1), vec![], "and a late answer is ignored");
}

#[test]
fn a_freed_slot_lets_the_next_safe_command_through() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("pwd"));
    result(&mut a, turn, 1, ToolStatus::Ok);
    assert_eq!(
        announce_now(&mut a, turn, 2, run("ls")),
        vec![check(turn, 2, "ls")]
    );
}
