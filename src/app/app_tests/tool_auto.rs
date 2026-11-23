//! Chat approval mode (agent-tools, D18): where the mode of a chat comes from
//! and when it resets.

use super::chat_history::{chat_of, history_app, listed, opened, prompt_for, resumed, rows};
use super::*;
use crate::core::ai::config::ApprovalMode;

fn auto_config() -> AiConfig {
    let mut ai = chat_live::ollama();
    ai.set_approval(ApprovalMode::Auto);
    ai
}

// 2e-3.3: a chat takes the live config value when its pane opens.
#[test]
fn a_chat_pane_opens_in_the_configured_mode() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    press(&mut a, "c");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);

    let mut b = launched(80, 25).with_ai(auto_config());
    press(&mut b, "c");
    assert_eq!(chat_of_focus(&b), ApprovalMode::Auto);
}

fn chat_of_focus(a: &App) -> ApprovalMode {
    run_pane::chat(a, a.focused()).approval_mode()
}

fn set_mode(a: &mut App, mode: ApprovalMode) {
    let id = a.focused();
    a.panes
        .get_mut(&id)
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_approval_mode(mode);
}

// A config change reaches later opens and switches, never the open
// conversation.
#[test]
fn a_config_change_does_not_touch_an_open_chat() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    press(&mut a, "c");
    a.ai.set_approval(ApprovalMode::Auto);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
}

// The mode chosen in a chat lasts for its conversation: starting a new one
// from the list goes back to the config value.
#[test]
fn starting_a_new_conversation_resets_the_mode_to_the_config() {
    let mut a = history_app();
    a.ai.set_approval(ApprovalMode::Auto);
    press(&mut a, "c");
    let id = a.focused();
    a.update(prompt_for(id));
    listed(&mut a, id, rows());
    set_mode(&mut a, ApprovalMode::Ask);
    a.update(key('n'));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Auto);
}

#[test]
fn resuming_a_conversation_resets_the_mode_to_the_config() {
    let (mut a, _) = opened();
    let id = a.focused();
    a.update(prompt_for(id));
    listed(&mut a, id, rows());
    a.ai.set_approval(ApprovalMode::Auto);
    a.update_settled(resumed(id, 30, "programmer"));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Auto);

    // And the other way: the toggled chat goes back to ask.
    a.ai.set_approval(ApprovalMode::Ask);
    set_mode(&mut a, ApprovalMode::Auto);
    a.focused_state_mut().chat_mut().unwrap().open_list();
    listed(&mut a, id, rows());
    a.update_settled(resumed(id, 10, "analyst"));
    assert_eq!(chat_of(&a).approval_mode(), ApprovalMode::Ask);
}

// 2e-3.4: `auto` no longer types on its own ---------------------------------

use super::tool_approval::{announce, decision, live, pending, run, shell_output};
use crate::core::ai::chat::{
    approval::ApprovalAsk,
    engine::{CallId, Decision, StreamEvent},
};

fn auto_typed(a: &App) -> u8 {
    run_pane::chat(a, id2()).auto_typed()
}

// A model's command prompts in every mode, `auto` included: `y` runs it.
#[test]
fn a_model_command_prompts_in_ask_and_auto() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto] {
        let (mut a, turn) = live();
        set_mode(&mut a, mode);
        assert_eq!(announce(&mut a, turn, 1, run("cd x")), vec![], "{mode:?}");
        assert!(pending(&a).is_some(), "{mode:?}");
        assert_eq!(auto_typed(&a), 0, "{mode:?}");
    }
}

// A `cargo install` update pane is not a shell: nothing is typed into its
// stdin, and a tab with only that pane has no shell.
#[test]
fn an_update_pane_is_never_a_target() {
    use crate::app::{RunState, UpdateRun};
    let (mut a, turn) = live();
    a.panes
        .get_mut(&PaneId::FIRST)
        .and_then(PaneState::pty_mut)
        .unwrap()
        .update = Some(UpdateRun {
        version: "9.9.9".into(),
        state: RunState::Running,
    });
    let no_shell = Decision::Denied("there is no shell pane in this tab");
    assert_eq!(
        announce(&mut a, turn, 1, run("cd x")),
        vec![decision(turn, 1, no_shell)]
    );
}

// 2e-3.6: Prefix a ------------------------------------------------------------

fn lines(a: &App) -> Vec<String> {
    run_pane::chat(a, id2())
        .entries()
        .iter()
        .filter_map(|e| match e {
            crate::core::ai::chat::Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

const RUN_LINE: &str = "approval: run (cd, pwd, ls, tree, find, grep, rg and reads inside the chat's folder run at once and their output goes to the model; a cloud chat whose folder is your home or / asks for them; paths outside the folder and other commands, git and mkdir included, ask)";

// Spec (modal-input): `a` cycles ask, auto, run, ask, with one system line per
// mode and nothing written to a PTY.
#[test]
fn prefix_a_cycles_the_mode_of_the_focused_chat_and_says_so() {
    let (mut a, _) = live();
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Auto);
    assert!(
        lines(&a).contains(&"approval: auto (commands are typed, never run)".to_owned()),
        "{:?}",
        lines(&a)
    );
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Run);
    assert_eq!(lines(&a).last().map(String::as_str), Some(RUN_LINE));
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
    assert_eq!(lines(&a).last().map(String::as_str), Some("approval: ask"));
}

#[test]
fn entering_run_needs_no_confirmation() {
    let (mut a, _) = live();
    set_mode(&mut a, ApprovalMode::Auto);
    assert_eq!(press(&mut a, "a"), vec![]);
    assert_eq!(chat_of_focus(&a), ApprovalMode::Run);
    assert!(a.menu().is_none());
    assert_eq!(pending(&a), None);
}

// The toggle flips only the focused chat.
#[test]
fn the_toggle_leaves_the_other_chats_alone() {
    let (mut a, _) = live();
    press(&mut a, "c");
    let second = a.focused();
    assert_ne!(second, id2());
    press(&mut a, "a");
    assert_eq!(
        run_pane::chat(&a, second).approval_mode(),
        ApprovalMode::Auto
    );
    assert_eq!(run_pane::chat(&a, id2()).approval_mode(), ApprovalMode::Ask);
}

// A pending approval is never answered by the mode.
#[test]
fn cycling_the_mode_leaves_a_pending_confirm_pending() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    for want in [ApprovalMode::Auto, ApprovalMode::Run, ApprovalMode::Ask] {
        assert_eq!(press(&mut a, "a"), vec![]);
        assert_eq!(chat_of_focus(&a), want);
        assert_eq!(pending(&a), Some((run("cd x"), Some((turn, CallId(1))))));
    }
}

#[test]
fn a_shell_pane_or_tools_off_only_shows_a_notice() {
    let mut a = launched(80, 25).with_ai(chat_live::ollama());
    assert_eq!(press(&mut a, "a"), vec![]);
    assert!(
        a.notice().is_some_and(|n| n.contains("chat")),
        "{:?}",
        a.notice()
    );

    let mut ai = chat_live::ollama();
    ai.set_tools_enabled(false);
    let mut a = launched(80, 25).with_ai(ai);
    press(&mut a, "c");
    assert_eq!(press(&mut a, "a"), vec![]);
    assert!(
        a.notice().is_some_and(|n| n.contains("Tools")),
        "{:?}",
        a.notice()
    );
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
    assert!(lines(&a).is_empty() || !lines(&a).iter().any(|l| l.starts_with("approval")));
}

#[test]
fn a_inside_a_group_toggles_nothing() {
    let (mut a, _) = live();
    press(&mut a, "wa");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
}

// Spec (agent-tools, config menu): saving `auto` from the menu is no decision
// on a pending approval: it stays pending, nothing is typed or written, and
// the open chat keeps its mode.
#[test]
fn saving_auto_from_the_menu_leaves_a_pending_approval_alone() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("cd x"));
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask);
    let before = pending(&a);
    assert_eq!(before, Some((run("cd x"), Some((turn, CallId(1))))));

    let mut effects = a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Char(' '),
        KeyModifiers::CONTROL,
    )));
    effects.extend(a.update(key('m')));
    assert!(a.menu().is_some(), "the menu opened over the chat");
    goto_row(&mut a, "Approval");
    effects.extend(a.update(key('l')));
    effects.extend(a.update(key('s')));
    effects.extend(a.update(AppEvent::ConfigSaved));

    assert_eq!(
        a.ai().approval(),
        ApprovalMode::Auto,
        "the save went through"
    );
    assert_eq!(pending(&a), before, "the approval is still pending");
    assert_eq!(chat_of_focus(&a), ApprovalMode::Ask, "the chat keeps ask");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::WritePty(..))),
        "{effects:?}"
    );
}

// agent-orchestrator / Outside read follows the mode as a chat read: in
// `ask`, `auto` and `run`, a worker's read ask gets exactly the answer the
// same chat read gets (the same effects, a prompt or none), and only its
// prompt carries the worker label.
#[test]
fn a_worker_read_is_answered_by_the_mode_as_the_same_chat_read() {
    use crate::core::ai::chat::approval::Reach;
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto, ApprovalMode::Run] {
        for reach in [
            Reach::Root,
            Reach::Broad,
            Reach::Refused,
            Reach::Other,
            Reach::Unshowable,
        ] {
            for cloud in [true, false] {
                let path = if reach == Reach::Unshowable {
                    ""
                } else {
                    "~/x/a.rs"
                };
                let chat = ApprovalAsk::Read {
                    path: path.into(),
                    reach,
                    cloud,
                };
                let worker = ApprovalAsk::WorkerRead {
                    worker: crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
                    path: path.into(),
                    label: "review claude>opus".into(),
                    reach,
                    cloud,
                };
                let (mut a, turn) = live();
                set_mode(&mut a, mode);
                let by_chat = announce(&mut a, turn, 1, chat);
                let chat_prompts = pending(&a).is_some();
                let (mut b, turn) = live();
                set_mode(&mut b, mode);
                let by_worker = announce(&mut b, turn, 1, worker.clone());
                let case = format!("{mode:?} {reach:?} cloud={cloud}");
                // The worker's answer is addressed to its session, the chat's
                // is not; nothing else differs.
                let unaddressed: Vec<_> = by_worker
                    .iter()
                    .cloned()
                    .map(|mut effect| {
                        if let Effect::ToolDecision { worker, .. } = &mut effect {
                            assert!(worker.take().is_some(), "{case}");
                        }
                        effect
                    })
                    .collect();
                assert_eq!(unaddressed, by_chat, "{case}");
                let shown = pending(&b).map(|(ask, _)| ask);
                assert_eq!(shown, chat_prompts.then_some(worker), "{case}");
            }
        }
    }
}

// S7 guard: a chat read keeps its answers in every mode (chat-auto-permissions).
#[test]
fn a_chat_read_keeps_its_answers_in_every_mode() {
    use crate::core::ai::chat::approval::{Reach, UNSHOWABLE};
    for (mode, reach, cloud, prompts, decided) in [
        (
            ApprovalMode::Run,
            Reach::Root,
            true,
            false,
            Some(Decision::Unprompted),
        ),
        (ApprovalMode::Ask, Reach::Root, true, true, None),
        (
            ApprovalMode::Auto,
            Reach::Other,
            false,
            false,
            Some(Decision::Unprompted),
        ),
        (ApprovalMode::Run, Reach::Other, true, true, None),
        // A path the guard refuses (`.env`) never prompts, in any mode (T10).
        (
            ApprovalMode::Ask,
            Reach::Refused,
            true,
            false,
            Some(Decision::Unprompted),
        ),
        (
            ApprovalMode::Auto,
            Reach::Refused,
            true,
            false,
            Some(Decision::Unprompted),
        ),
        (
            ApprovalMode::Run,
            Reach::Unshowable,
            false,
            false,
            Some(Decision::Denied(UNSHOWABLE)),
        ),
    ] {
        let (mut a, turn) = live();
        set_mode(&mut a, mode);
        let path = if reach == Reach::Unshowable {
            ""
        } else {
            "~/x/a.rs"
        };
        let ask = ApprovalAsk::Read {
            path: path.into(),
            reach,
            cloud,
        };
        let effects = announce(&mut a, turn, 1, ask);
        let case = format!("{mode:?} {reach:?} cloud={cloud}");
        assert_eq!(pending(&a).is_some(), prompts, "{case}");
        let want: Vec<Effect> = decided
            .map(|decision| Effect::ToolDecision {
                pane: id2(),
                worker: None,
                turn,
                call: CallId(1),
                decision,
            })
            .into_iter()
            .collect();
        assert_eq!(effects, want, "{case}");
    }
}

// #816: the end-of-turn line for a command typed and not run ------------------

const WAITING: &str =
    "waiting for you: press Enter in pane 1 to run it; the chat goes on when it finishes";

fn ends_turn(a: &mut App, turn: u64) -> Vec<String> {
    a.update(AppEvent::Chat(
        id2(),
        turn,
        StreamEvent::Done(crate::core::ai::chat::engine::Finish::Stop),
    ));
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            crate::core::ai::chat::Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn y_typing_a_command_ends_the_turn_with_the_waiting_line() {
    let (mut a, turn) = live();
    a.run_policy = crate::core::ai::chat::approval::RunPolicy { y_runs: false };
    announce(&mut a, turn, 1, run("secret-cmd"));
    a.update(key('y'));
    let lines = ends_turn(&mut a, turn);
    assert!(lines.iter().any(|l| l == WAITING), "{lines:?}");
    assert!(!WAITING.contains("secret-cmd"));
}

#[test]
fn a_y_the_pane_refuses_leaves_no_waiting_line() {
    let (mut a, turn) = live();
    announce(&mut a, turn, 1, run("secret-cmd"));
    shell_output(&mut a, b"\x1b[?1049h");
    a.update(key('y'));
    let lines = ends_turn(&mut a, turn);
    assert!(
        !lines.iter().any(|l| l.contains("waiting for you")),
        "{lines:?}"
    );
}
