//! The line-pending mark, set by a command `y` typed (not run) and read by a
//! later unprompted run to the same pane.

use super::chat_live;
use super::tool_approval::{announce, pending, result, run, shell_output};
use super::tool_run::run_live;
use super::*;
use crate::core::ai::chat::{approval::RunPolicy, engine::ToolStatus};

/// `y` types one command (typed-only policy), so pane 1 holds an unsent line.
fn marked() -> (App, u64) {
    let (mut a, turn) = run_live();
    a.run_policy = RunPolicy { y_runs: false };
    announce(&mut a, turn, 1, run("cargo build"));
    let effects = a.update(key('y'));
    assert!(matches!(effects.first(), Some(Effect::WritePty(..))));
    (a, turn)
}

/// A read-only call prompts when the pane is marked; the prompt is dismissed.
fn prompts(a: &mut App, turn: u64, call: u64) -> bool {
    let effects = announce(a, turn, call, run("ls"));
    let waits = pending(a).is_some_and(|(ask, _)| ask == run("ls"));
    if waits {
        a.update(key('n'));
        assert_eq!(effects, vec![]);
    }
    waits
}

fn user_in_the_shell(a: &mut App) {
    a.tab_mut().focus = PaneId::FIRST;
}

fn line(a: &App) -> Option<crate::core::ai::chat::engine::CallId> {
    a.panes
        .get(&PaneId::FIRST)
        .and_then(PaneState::pty)
        .unwrap()
        .line_pending
}

#[test]
fn a_paste_with_a_newline_clears_the_mark_when_paste_mode_is_off() {
    for newline in ["\r", "\n", "\r\n"] {
        let (mut a, _) = marked();
        user_in_the_shell(&mut a);
        a.update(AppEvent::Paste("abc".into()));
        assert!(line(&a).is_some(), "no newline: still pending");
        a.update(AppEvent::Paste(format!("abc{newline}def")));
        assert!(line(&a).is_none(), "{newline:?}");
    }
}

#[test]
fn a_bracketed_paste_never_clears_the_mark() {
    let (mut a, _) = marked();
    shell_output(&mut a, b"\x1b[?2004h");
    user_in_the_shell(&mut a);
    for text in ["abc\rdef", "abc\ndef", "abc\r\ndef"] {
        a.update(AppEvent::Paste(text.into()));
    }
    assert!(line(&a).is_some());
}

#[test]
fn a_denied_result_clears_only_its_own_mark_and_ok_leaves_it() {
    let (mut a, turn) = marked();
    result(&mut a, turn, 1, ToolStatus::Ok);
    assert!(line(&a).is_some(), "ok leaves the mark");
    assert!(prompts(&mut a, turn, 2));
    result(&mut a, turn, 9, ToolStatus::Denied);
    assert!(line(&a).is_some(), "another call's denial leaves it");
    result(&mut a, turn, 1, ToolStatus::Denied);
    assert!(line(&a).is_none(), "its own denial clears it");
}

#[test]
fn enter_clears_the_mark_also_with_an_extended_keyboard_protocol() {
    let (mut a, _) = marked();
    shell_output(&mut a, b"\x1b[>1u");
    user_in_the_shell(&mut a);
    a.update(chat_live::plain(KeyCode::Enter));
    assert!(line(&a).is_none());
}

#[test]
fn a_mark_on_one_pane_never_affects_another() {
    let (mut a, turn) = run_live();
    a.run_policy = RunPolicy { y_runs: false };
    press(&mut a, "wv");
    let second = a.focused();
    assert_ne!(second, PaneId::FIRST);
    a.tab_mut().focus = id2();
    a.tab_mut().last_shell = Some(second);
    announce(&mut a, turn, 1, run("cargo build"));
    let effects = a.update(key('y'));
    assert!(matches!(effects.first(), Some(Effect::WritePty(p, _)) if *p == second));
    assert!(
        a.panes
            .get(&second)
            .and_then(PaneState::pty)
            .is_some_and(|p| p.line_pending.is_some())
    );
    // The first shell is not marked: a read-only call to it goes to its check.
    a.tab_mut().last_shell = Some(PaneId::FIRST);
    let effects = announce(&mut a, turn, 2, run("ls"));
    assert_eq!(pending(&a), None, "{effects:?}");
    assert!(
        matches!(effects.as_slice(), [Effect::CheckRun { target, .. }] if *target == PaneId::FIRST),
        "{effects:?}"
    );
}
