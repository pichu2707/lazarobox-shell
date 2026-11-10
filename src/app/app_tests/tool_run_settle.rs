//! A run that was written is settled by the app (pane-execution, design D22):
//! the runtime probes the pane on its tick, the settle machine decides, and
//! one `Effect::RunDone` carries the report to the parked call.

use super::run_pane_keys::system_lines;
use super::tool_approval::{announce, run, shell_output};
use super::tool_run::{chat_of, checked, in_shell, run_live, slot};
use super::*;
use crate::app::RunEvent;
use crate::core::ai::{
    chat::engine::{CallId, Finish},
    tools::{
        report::{CwdSource, RunReport, words},
        settle::End,
    },
};

const SHELL: PaneId = PaneId::FIRST;

fn at(ms: u64) {
    NOW_MS.with(|n| n.set(ms));
}

fn now() -> u64 {
    NOW_MS.with(|n| n.get())
}

/// A chat in `run` mode whose command `command` was checked and written.
fn written(command: &str, cd_target: Option<&str>) -> (App, u64) {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run(command));
    let effects = if cd_target.is_some() {
        a.update(AppEvent::Run(
            SHELL,
            RunEvent::Checked {
                call: CallId(1),
                cwd: LAUNCH.into(),
                cd_target: cd_target.map(Into::into),
            },
        ))
    } else {
        checked(&mut a, 1)
    };
    assert!(
        matches!(effects[..], [Effect::AutoRun { .. }]),
        "{effects:?}"
    );
    (a, turn)
}

fn probe(a: &mut App, foreground: Option<bool>, cwd: Option<&str>) -> Vec<Effect> {
    a.update(AppEvent::Run(
        SHELL,
        RunEvent::Probe {
            foreground,
            cwd: cwd.map(Into::into),
        },
    ))
}

fn probe_at(a: &mut App, after: u64, t0: u64) -> Vec<Effect> {
    at(t0 + after);
    probe(a, Some(true), Some(LAUNCH))
}

fn done(turn: u64, report: RunReport) -> Effect {
    Effect::RunDone {
        chat: id2(),
        turn,
        call: CallId(1),
        report: Box::new(report),
    }
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

/// The shell prints a line and shows its next prompt.
fn prompt_again(a: &mut App) {
    shell_output(a, b"\r\nout\r\n$ ");
}

#[test]
fn only_a_written_run_is_probed() {
    let (mut a, turn) = run_live();
    assert_eq!(a.run_probes(), vec![], "idle");
    announce(&mut a, turn, 1, run("pwd"));
    assert_eq!(a.run_probes(), vec![], "checking, nothing written yet");
    checked(&mut a, 1);
    assert_eq!(a.run_probes(), vec![SHELL]);
}

#[test]
fn a_probe_without_a_written_run_changes_nothing() {
    let (mut a, _) = run_live();
    assert_eq!(probe(&mut a, Some(true), Some(LAUNCH)), vec![]);
}

#[test]
fn a_quiet_new_prompt_ends_the_run_with_one_report() {
    let (mut a, turn) = written("ls", None);
    let t0 = now();
    at(t0 + 10);
    prompt_again(&mut a);
    assert_eq!(
        probe_at(&mut a, 100, t0),
        vec![],
        "not quiet for 250 ms yet"
    );
    assert_eq!(
        probe_at(&mut a, 300, t0),
        vec![done(turn, report(End::Settled))]
    );
    assert_eq!(slot(&a), None, "the executing mark is released at settle");
    assert_eq!(a.run_probes(), vec![]);
    assert_eq!(probe_at(&mut a, 400, t0), vec![], "one report only");
}

#[test]
fn output_restarts_the_quiet() {
    let (mut a, turn) = written("ls", None);
    let t0 = now();
    at(t0 + 200);
    prompt_again(&mut a);
    assert_eq!(probe_at(&mut a, 400, t0), vec![]);
    assert_eq!(
        probe_at(&mut a, 450, t0),
        vec![done(turn, report(End::Settled))]
    );
}

#[test]
fn a_prompt_that_did_not_move_keeps_waiting_until_the_timeout() {
    for (foreground, end) in [(Some(false), End::TimedOut), (None, End::Unconfirmed)] {
        let (mut a, turn) = written("ls", None);
        let t0 = now();
        at(t0 + 5_000);
        assert_eq!(probe(&mut a, foreground, Some(LAUNCH)), vec![], "{end:?}");
        at(t0 + 10_000);
        assert_eq!(
            probe(&mut a, foreground, Some(LAUNCH)),
            vec![done(turn, report(end))],
            "{end:?}: no echo of the command on the screen, so no output"
        );
    }
}

#[test]
fn a_cd_reports_the_real_cwd_and_adds_the_system_line() {
    let (mut a, turn) = written("cd dev", Some("/launch/dev"));
    let t0 = now();
    at(t0 + 10);
    prompt_again(&mut a);
    at(t0 + 400);
    let effects = probe(&mut a, Some(true), Some("/launch/dev"));
    let want = RunReport {
        cwd: Some(("/launch/dev".into(), CwdSource::Proc)),
        cd_target: Some("/launch/dev".into()),
        ..report(End::Settled)
    };
    assert_eq!(effects, vec![done(turn, want)]);
    assert_eq!(
        system_lines(&a),
        vec!["pane 1: cd dev \u{2192} /launch/dev".to_owned()]
    );
}

#[test]
fn a_cd_settles_on_the_cwd_even_when_the_prompt_did_not_move() {
    let (mut a, _) = written("cd dev", Some("/launch/dev"));
    let t0 = now();
    at(t0 + 300);
    let effects = probe(&mut a, Some(true), Some("/launch/dev"));
    assert!(
        matches!(effects[..], [Effect::RunDone { .. }]),
        "{effects:?}"
    );
}

#[test]
fn another_command_adds_no_system_line() {
    let (mut a, _) = written("ls", None);
    let t0 = now();
    at(t0 + 10);
    prompt_again(&mut a);
    probe_at(&mut a, 300, t0);
    assert_eq!(system_lines(&a), Vec::<String>::new());
}

// OSC 7 is any program's claim: it is used only when the system cannot say,
// and labelled, and then the transcript gets no line.
#[test]
fn without_the_real_cwd_the_osc7_one_is_used_and_labelled() {
    let (mut a, turn) = written("cd dev", Some("/launch/dev"));
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"\x1b]7;file:///launch/dev\x07\r\n$ ");
    at(t0 + 300);
    let want = RunReport {
        cwd: Some(("/launch/dev".into(), CwdSource::Osc7)),
        cd_target: Some("/launch/dev".into()),
        ..report(End::Settled)
    };
    assert_eq!(probe(&mut a, Some(true), None), vec![done(turn, want)]);
    assert_eq!(system_lines(&a), Vec::<String>::new());
}

#[test]
fn a_key_the_user_types_ends_the_run() {
    let (mut a, turn) = written("ls", None);
    let t0 = now();
    in_shell(&mut a, key('x'));
    assert_eq!(
        probe_at(&mut a, 50, t0),
        vec![done(turn, report(End::UserTyped))]
    );
}

#[test]
fn a_full_screen_program_ends_the_run() {
    let (mut a, turn) = written("ls", None);
    let t0 = now();
    shell_output(&mut a, b"\x1b[?1049h");
    assert_eq!(
        probe_at(&mut a, 50, t0),
        vec![done(turn, report(End::FullScreen))]
    );
}

#[test]
fn an_end_clears_the_fallbacks_like_a_call_that_ended_ok() {
    let (mut a, _) = written("ls", None);
    chat_mut_of(&mut a).note_run_fallback();
    let t0 = now();
    in_shell(&mut a, key('x'));
    probe_at(&mut a, 50, t0);
    assert!(!chat_of(&a).run_paused());
}

fn chat_mut_of(a: &mut App) -> &mut ChatState {
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
}

// W3: the pane closes mid-run.
#[test]
fn closing_the_pane_during_a_run_reports_it_gone_and_denies_nothing() {
    let (mut a, turn) = written("ls", None);
    let effects = a.remove_pane(SHELL);
    let gone = RunReport {
        cwd: None,
        ..report(End::PaneGone)
    };
    assert!(effects.contains(&done(turn, gone)), "{effects:?}");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::ToolDecision { .. })),
        "{effects:?}"
    );
    assert_eq!(a.run_probes(), vec![]);
    assert_eq!(
        probe(&mut a, Some(true), Some(LAUNCH)),
        vec![],
        "late probe"
    );
}

// W3: the end of the turn, a cancel and a closed chat drop the run.
#[test]
fn the_end_of_the_turn_a_cancel_and_a_closed_chat_stop_the_probing() {
    let ends: [fn(&mut App, u64); 3] = [
        |a, turn| {
            a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
        },
        |a, _| {
            a.cancel_chat(id2());
        },
        |a, _| {
            a.remove_pane(id2());
        },
    ];
    for (n, end) in ends.into_iter().enumerate() {
        let (mut a, turn) = written("ls", None);
        end(&mut a, turn);
        assert_eq!(a.run_probes(), vec![], "end {n}");
        at(now() + 20_000);
        assert_eq!(probe(&mut a, Some(true), Some(LAUNCH)), vec![], "end {n}");
    }
}

#[test]
fn a_write_the_runtime_refused_ends_the_probing_and_asks_the_user() {
    let (mut a, _) = written("ls", None);
    a.update(AppEvent::Run(
        SHELL,
        RunEvent::NotRun {
            call: CallId(1),
            reason: "the pane is running a program",
        },
    ));
    assert_eq!(a.run_probes(), vec![]);
    assert_eq!(slot(&a), None);
}

// W1: a cursor hidden at the write gives no line to move past.
#[test]
fn a_cursor_hidden_at_the_write_anchors_on_the_first_visible_line() {
    let (mut a, turn) = run_live();
    shell_output(&mut a, b"\x1b[?25l");
    announce(&mut a, turn, 1, run("ls"));
    assert!(matches!(checked(&mut a, 1)[..], [Effect::AutoRun { .. }]));
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"\x1b[?25h\r\nout\r\n");
    assert_eq!(
        probe_at(&mut a, 300, t0),
        vec![],
        "the anchor, not progress"
    );
    at(t0 + 400);
    prompt_again(&mut a);
    assert_eq!(
        probe_at(&mut a, 700, t0),
        vec![done(turn, report(End::Settled))]
    );
}

#[test]
fn a_cursor_never_seen_ends_unconfirmed_at_the_timeout() {
    let (mut a, turn) = run_live();
    shell_output(&mut a, b"\x1b[?25l");
    announce(&mut a, turn, 1, run("ls"));
    checked(&mut a, 1);
    let t0 = now();
    assert_eq!(
        probe_at(&mut a, 10_000, t0),
        vec![done(turn, report(End::Unconfirmed))]
    );
}

// W3: whatever the user sends to the pane ends the run.
#[test]
fn a_paste_and_the_prefix_literal_end_the_run_as_user_typed() {
    for n in 0..2 {
        let (mut a, turn) = written("ls", None);
        let t0 = now();
        a.tab_mut().focus = SHELL;
        if n == 0 {
            a.update(AppEvent::Paste("x".into()));
        } else {
            a.update(AppEvent::Key(ratatui::crossterm::event::KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::CONTROL,
            )));
            a.update(AppEvent::Key(ratatui::crossterm::event::KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::CONTROL,
            )));
        }
        a.tab_mut().focus = id2();
        assert_eq!(
            probe_at(&mut a, 50, t0),
            vec![done(turn, report(End::UserTyped))],
            "case {n}"
        );
    }
}

// S2: the prompt's `y` types into the same pane.
#[test]
fn text_typed_by_y_into_the_pane_of_a_running_run_ends_it() {
    let (mut a, turn) = written("ls", None);
    let t0 = now();
    announce(&mut a, turn, 2, run("cargo build"));
    NOW_MS.with(|n| n.set(n.get() + 1_000));
    let effects = a.update(key('y'));
    assert!(
        effects.contains(&Effect::WritePty(SHELL, b"cargo build".to_vec())),
        "{effects:?}"
    );
    assert_eq!(
        probe_at(&mut a, 2_000, t0),
        vec![done(turn, report(End::UserTyped))]
    );
}

// A written run is never checked or refused again.
#[test]
fn a_late_check_answer_after_the_write_is_ignored() {
    let (mut a, _) = written("ls", None);
    assert_eq!(checked(&mut a, 1), vec![]);
    let refused = AppEvent::Run(
        SHELL,
        RunEvent::Refused {
            call: CallId(1),
            reason: "late",
        },
    );
    assert_eq!(a.update(refused), vec![]);
    assert_eq!(a.run_probes(), vec![SHELL], "still settling");
}

#[test]
fn turning_the_tools_off_drops_a_written_run() {
    let (mut a, _) = written("ls", None);
    a.ai.set_tools_enabled(false);
    a.sync_tools();
    assert_eq!(slot(&a), None);
    assert_eq!(a.run_probes(), vec![]);
}

#[test]
fn closing_the_tab_of_a_written_run_leaves_nothing_to_probe() {
    let (mut a, _) = written("ls", None);
    a.new_tab();
    let effects = a.drop_tab(0);
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::RunDone { .. })),
        "the chat closes with the pane: {effects:?}"
    );
    assert_eq!(a.run_probes(), vec![]);
}

// A `cd` that has not finished has no new directory to show.
#[test]
fn a_cd_that_timed_out_or_could_not_be_confirmed_adds_no_system_line() {
    for foreground in [Some(false), None] {
        let (mut a, _) = written("cd dev", Some("/launch/dev"));
        let t0 = now();
        at(t0 + 10_000);
        let effects = probe(&mut a, foreground, Some("/launch/dev"));
        assert!(
            matches!(effects[..], [Effect::RunDone { .. }]),
            "{effects:?}"
        );
        assert_eq!(system_lines(&a), Vec::<String>::new(), "{foreground:?}");
    }
}

// S3: a write step with nobody to answer to drops the slot and asks nobody.
#[test]
fn a_write_step_with_no_call_to_answer_denies_and_drops_the_slot() {
    let (mut a, turn) = written("ls", None);
    let effects = a.update(AppEvent::Run(
        SHELL,
        RunEvent::NotRun {
            call: CallId(1),
            reason: crate::app::CALL_OVER,
        },
    ));
    assert_eq!(slot(&a), None);
    assert_eq!(a.run_probes(), vec![]);
    assert!(
        effects
            .iter()
            .all(|e| matches!(e, Effect::ToolDecision { turn: t, .. } if *t == turn)),
        "no prompt: {effects:?}"
    );
}

// S4b: the screen text of a settled run goes back to the model.

const NONCE: &str = "0123456789abcdef";

/// A chat in `run` mode whose shell shows a prompt, and `command` written.
fn at_prompt(command: &str) -> (App, u64) {
    let (mut a, turn) = run_live();
    shell_output(&mut a, b"$ ");
    announce(&mut a, turn, 1, run(command));
    assert!(matches!(checked(&mut a, 1)[..], [Effect::AutoRun { .. }]));
    (a, turn)
}

fn report_of(effects: &[Effect]) -> RunReport {
    match effects {
        [Effect::RunDone { report, .. }] => (**report).clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_settled_run_reports_the_lines_it_printed() {
    let (mut a, _) = at_prompt("ls");
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"ls\r\nfile one\r\nfile two\r\n$ ");
    let got = report_of(&probe_at(&mut a, 300, t0));
    assert_eq!(got.end, End::Settled);
    assert_eq!(got.output.as_deref(), Some("file one\nfile two"));
}

#[test]
fn a_silent_run_reports_an_empty_output() {
    let (mut a, _) = at_prompt("rg zz");
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"rg zz\r\n$ ");
    let got = report_of(&probe_at(&mut a, 300, t0));
    assert_eq!(got.output.as_deref(), Some(""));
}

#[test]
fn the_model_text_wraps_the_output_and_redacts_a_secret_in_it() {
    let (mut a, _) = at_prompt("ls");
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"ls\r\nkey sk-abcDEF1234567890abcdef\r\nok\r\n$ ");
    let got = report_of(&probe_at(&mut a, 300, t0));
    let text = words("ls", &got, Some(NONCE)).text;
    assert!(!text.contains("sk-abcDEF"), "{text}");
    assert!(text.contains("key [REDACTED]\nok"), "{text}");
    assert!(text.contains(&format!("<untrusted-tool-output id=\"{NONCE}\">")));
}

#[test]
fn a_resize_during_the_run_leaves_its_output_unavailable() {
    let (mut a, _) = at_prompt("ls");
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"ls\r\nx\r\n$ ");
    let pty = a
        .panes
        .get_mut(&SHELL)
        .and_then(PaneState::pty_mut)
        .unwrap();
    pty.emu
        .resize(crate::core::pane::PaneSize { rows: 20, cols: 70 });
    let got = report_of(&probe_at(&mut a, 300, t0));
    assert_eq!(got.output, None);
    assert!(
        words("ls", &got, Some(NONCE))
            .text
            .contains("not available")
    );
}

#[test]
fn a_run_that_timed_out_reports_the_output_so_far() {
    let (mut a, _) = at_prompt("ls");
    let t0 = now();
    at(t0 + 10);
    shell_output(&mut a, b"ls\r\nfirst\r\npart");
    at(t0 + 10_000);
    let got = report_of(&probe(&mut a, Some(false), Some(LAUNCH)));
    assert_eq!(got.end, End::TimedOut);
    assert_eq!(got.output.as_deref(), Some("first\npart"));
}

#[test]
fn a_full_screen_program_a_typing_user_and_a_closed_pane_carry_no_output() {
    for end in [End::FullScreen, End::UserTyped] {
        let (mut a, _) = at_prompt("ls");
        let t0 = now();
        shell_output(&mut a, b"ls\r\nx\r\n");
        if end == End::FullScreen {
            shell_output(&mut a, b"\x1b[?1049h");
        } else {
            in_shell(&mut a, key('x'));
        }
        assert_eq!(report_of(&probe_at(&mut a, 50, t0)).output, None, "{end:?}");
    }
}
