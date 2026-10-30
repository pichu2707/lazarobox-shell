//! `y` in `run` mode: a model's command the user approves is run like a safe
//! one, with one Enter, and its output goes back to the model. When it cannot
//! run, `y` types it as in `ask` and says why. `ask`, `auto` and the confirm
//! of `Prefix e` keep typing without Enter.

use super::run_pane::{BLOCK, one_shell_and_a_chat};
use super::tool_approval::{announce, announce_now, decision, pending, result, run};
use super::tool_run::{chat_mut, chat_of, run_live, slot};
use super::*;
use crate::app::RunEvent;
use crate::core::ai::{
    chat::{
        Entry,
        approval::{HALF_TYPED, MULTI_LINE, NO_CWD, NON_UTF8, RUN_KEYS, RUN_NOW_KEYS, RUNNING},
        engine::{CallId, Decision, ToolStatus},
    },
    config::ApprovalMode,
};

fn run_effect(turn: u64, bytes: &[u8]) -> Effect {
    Effect::AutoRun {
        chat: id2(),
        target: PaneId::FIRST,
        bytes: bytes.to_vec(),
        turn,
        call: CallId(1),
        cwd: LAUNCH.into(),
    }
}

fn lines(a: &App) -> Vec<String> {
    (chat_of(a).entries().iter())
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn shell_pty(a: &mut App) -> &mut PtyPane {
    a.panes
        .get_mut(&PaneId::FIRST)
        .and_then(PaneState::pty_mut)
        .unwrap()
}

/// `y` on the prompt of `command` in a `run` chat, after its arming delay.
fn y_on(a: &mut App, turn: u64, command: &str) -> Vec<Effect> {
    assert_eq!(announce(a, turn, 1, run(command)), vec![], "{command} asks");
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    a.update(key('y'))
}

// S1: an off-list command, `cd` outside the folder among them, runs on `y`.
#[test]
fn y_runs_an_asked_command_with_one_enter_in_run_mode() {
    for command in ["cargo build", "mkdir x", "python3 test.py", "npm test"] {
        let (mut a, turn) = run_live();
        let bytes = format!("{command}\r");
        assert_eq!(
            y_on(&mut a, turn, command),
            vec![run_effect(turn, bytes.as_bytes())],
            "{command}"
        );
        assert_eq!(pending(&a), None, "{command}: the prompt is closed");
        assert_eq!(slot(&a).map(|s| s.2), Some(CallId(1)), "{command}");
        assert_eq!(a.run_probes(), vec![PaneId::FIRST], "{command}: it settles");
        assert!(
            lines(&a).is_empty(),
            "{command}: the run's report says what ran"
        );
    }
}

// S1: the user's run does not take one of the turn's unprompted slots.
#[test]
fn a_run_on_y_is_not_counted_as_an_unprompted_one() {
    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    assert_eq!(chat_of(&a).auto_typed(), 0);
}

// S1: the run ends with the run's report, which carries the output.
#[test]
fn a_run_on_y_ends_with_a_report() {
    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    a.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(b"cargo build\r\nok\r\n$ ".to_vec()),
    ));
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    let effects = a.update(AppEvent::Run(
        PaneId::FIRST,
        RunEvent::Probe {
            foreground: Some(true),
            cwd: Some(LAUNCH.into()),
        },
    ));
    assert!(
        matches!(effects[..], [Effect::RunDone { turn: t, call: CallId(1), .. }] if t == turn),
        "{effects:?}"
    );
    assert_eq!(slot(&a), None);
}

// S3: a run the write step refuses is typed, with the reason, and answered.
#[test]
fn a_run_on_y_that_is_not_written_is_typed_with_the_reason() {
    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    let effects = a.update(AppEvent::Run(
        PaneId::FIRST,
        RunEvent::NotRun {
            call: CallId(1),
            reason: "the pane is running a program",
        },
    ));
    assert_eq!(
        effects,
        vec![
            Effect::WritePty(PaneId::FIRST, b"cargo build".to_vec()),
            decision(turn, 1, Decision::Sent),
        ]
    );
    assert_eq!(pending(&a), None, "no second prompt");
    assert_eq!(slot(&a), None);
    assert_eq!(
        lines(&a),
        vec![
            "sent to pane 1; press Enter there to run it (not run: the pane is running a program)"
        ]
    );
}

// S3: what stops a run before the write types the command, with the reason.
#[test]
fn y_types_with_the_reason_when_the_command_cannot_run() {
    type Setup = fn(&mut App);
    let cases: [(&str, Setup, &str); 5] = [
        ("cargo build", |a| shell_pty(a).proc_cwd = None, NO_CWD),
        (
            "cargo build",
            |a| shell_pty(a).user_dirty = true,
            HALF_TYPED,
        ),
        (
            "cargo build",
            |a| shell_pty(a).line_pending = Some(CallId(9)),
            HALF_TYPED,
        ),
        ("mkdir café", |a| a.utf8_locale = false, NON_UTF8),
        (
            "echo a\necho b",
            |a| {
                a.update(AppEvent::Pty(
                    PaneId::FIRST,
                    PtyEvent::Output(b"\x1b[?2004h".to_vec()),
                ));
            },
            MULTI_LINE,
        ),
    ];
    for (command, setup, why) in cases {
        let (mut a, turn) = run_live();
        setup(&mut a);
        let effects = y_on(&mut a, turn, command);
        assert!(
            matches!(&effects[..], [Effect::WritePty(PaneId::FIRST, bytes), last]
                if !bytes.ends_with(b"\r") && *last == decision(turn, 1, Decision::Sent)),
            "{why}: {effects:?}"
        );
        assert_eq!(slot(&a), None, "{why}");
        assert_eq!(
            lines(&a),
            vec![format!(
                "sent to pane 1; press Enter there to run it (not run: {why})"
            )],
        );
    }
}

// S3: a second command for a pane whose `y` run still settles is typed, and
// the first run keeps its slot.
#[test]
fn y_types_into_a_pane_that_still_runs_a_command() {
    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    announce(&mut a, turn, 2, run("npm test"));
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    assert_eq!(
        a.update(key('y')),
        vec![
            Effect::WritePty(PaneId::FIRST, b"npm test".to_vec()),
            decision(turn, 2, Decision::Sent),
        ]
    );
    assert_eq!(slot(&a).map(|s| s.2), Some(CallId(1)));
    assert_eq!(
        lines(&a),
        vec![format!(
            "sent to pane 1; press Enter there to run it (not run: {RUNNING})"
        )]
    );
}

// S1, S2: the mode when `y` is pressed decides, not the mode the prompt
// opened in.
#[test]
fn the_mode_at_y_decides_whether_it_runs() {
    for (from, to, runs) in [
        (ApprovalMode::Run, ApprovalMode::Ask, false),
        (ApprovalMode::Run, ApprovalMode::Auto, false),
        (ApprovalMode::Ask, ApprovalMode::Run, true),
    ] {
        let (mut a, turn) = run_live();
        chat_mut(&mut a).set_approval_mode(from);
        announce(&mut a, turn, 1, run("cargo build"));
        chat_mut(&mut a).set_approval_mode(to);
        NOW_MS.with(|n| n.set(n.get() + 1_000));
        let expected = if runs {
            vec![run_effect(turn, b"cargo build\r")]
        } else {
            vec![
                Effect::WritePty(PaneId::FIRST, b"cargo build".to_vec()),
                decision(turn, 1, Decision::Sent),
            ]
        };
        assert_eq!(a.update(key('y')), expected, "{from:?} -> {to:?}");
    }
}

// The prompt's arming delay and a full-screen pane still stop `y`.
#[test]
fn y_before_the_arming_delay_or_on_a_full_screen_pane_writes_nothing() {
    let (mut a, turn) = run_live();
    announce_now(&mut a, turn, 1, run("cargo build"));
    assert_eq!(a.update(key('y')), vec![]);
    assert!(pending(&a).is_some(), "the prompt stays");

    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cargo build"));
    a.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(b"\x1b[?1049h".to_vec()),
    ));
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    assert_eq!(
        a.update(key('y')),
        vec![decision(
            turn,
            1,
            Decision::Denied("the pane runs a full-screen app")
        )]
    );
}

// S3: a refusal of the write step answers the call once, and none after the
// call is over.
#[test]
fn a_not_run_answers_a_y_run_once_and_never_after_the_call_ended() {
    let not_run = || {
        AppEvent::Run(
            PaneId::FIRST,
            RunEvent::NotRun {
                call: CallId(1),
                reason: "the pane is running a program",
            },
        )
    };
    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    assert_eq!(a.update(not_run()).len(), 2, "typed and answered");
    assert_eq!(a.update(not_run()), vec![], "once");

    let (mut a, turn) = run_live();
    y_on(&mut a, turn, "cargo build");
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert_eq!(slot(&a), None);
    assert_eq!(a.update(not_run()), vec![], "the call is over");
}

// S2: `ask` and `auto` keep typing on `y`.
#[test]
fn y_in_ask_and_auto_still_types_without_enter() {
    for mode in [ApprovalMode::Ask, ApprovalMode::Auto] {
        let (mut a, turn) = run_live();
        chat_mut(&mut a).set_approval_mode(mode);
        // `auto` asks when the target has the user's text.
        shell_pty(&mut a).user_dirty = true;
        assert_eq!(
            y_on(&mut a, turn, "cargo build"),
            vec![
                Effect::WritePty(PaneId::FIRST, b"cargo build".to_vec()),
                decision(turn, 1, Decision::Sent),
            ],
            "{mode:?}"
        );
        assert_eq!(slot(&a), None, "{mode:?}");
    }
}

// S2: the confirm of `Prefix e` is the user's own command: it only types.
#[test]
fn the_prefix_e_confirm_still_types_in_run_mode() {
    let mut a = one_shell_and_a_chat(BLOCK);
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Run);
    press(&mut a, "e");
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"cd ~/dev/curso-rust".to_vec()
        )]
    );
    assert_eq!(slot(&a), None);
}

// S4: the hint says what `y` does in each mode.
#[test]
fn the_hint_of_a_model_run_says_y_runs_it_only_in_run_mode() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cargo build"));
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_NOW_KEYS);
    assert_eq!(RUN_NOW_KEYS, "y run it · n cancel · p next pane");

    let (mut a, turn) = run_live();
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Ask);
    announce(&mut a, turn, 1, run("cargo build"));
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_KEYS);

    // S3: when `y` would only type, the hint says so before the key.
    let (mut a, turn) = run_live();
    shell_pty(&mut a).user_dirty = true;
    announce(&mut a, turn, 1, run("cargo build"));
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_KEYS);

    let mut a = one_shell_and_a_chat(BLOCK);
    chat_mut(&mut a).set_approval_mode(ApprovalMode::Run);
    press(&mut a, "e");
    assert_eq!(a.approval_prompt(id2()).unwrap().keys, RUN_KEYS);
}
