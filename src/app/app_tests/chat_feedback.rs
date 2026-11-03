//! The `/feedback` command (#551): user-only, never a message nor a request.
//! It opens the Feedback form with its text, and only Ctrl+S sends the issue.

use super::chat_history::*;
use super::*;

fn sends(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::ChatSend { .. }))
        .count()
}

fn issues(effects: &[Effect]) -> Vec<(String, String)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SendIssue(issue) => Some((issue.title.clone(), issue.body.clone())),
            _ => None,
        })
        .collect()
}

fn ctrl_s() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
}

#[test]
fn feedback_opens_the_filled_form_and_sends_nothing_before_ctrl_s() {
    let mut a = conversation();
    let before = chat_of(&a).messages().count();
    let effects = ask(&mut a, "/feedback The chat hangs");
    assert_eq!(sends(&effects), 0, "{effects:?}");
    assert_eq!(issues(&effects), Vec::new());
    assert_eq!(a.input(), InputMode::Menu);
    assert_eq!(chat_of(&a).messages().count(), before);
    assert!(chat_of(&a).composer().is_empty());

    let effects = a.update(ctrl_s());
    let sent = issues(&effects);
    assert_eq!(sent.len(), 1, "{effects:?}");
    assert_eq!(sent[0].0, "The chat hangs");
    assert!(
        sent[0].1.starts_with("### What happened\n\nThe chat hangs"),
        "{}",
        sent[0].1
    );
}

#[test]
fn feedback_alone_opens_an_empty_form_that_ctrl_s_cannot_send() {
    let mut a = conversation();
    ask(&mut a, "/feedback");
    assert_eq!(a.input(), InputMode::Menu);
    assert_eq!(issues(&a.update(ctrl_s())), Vec::new());
}

#[test]
fn leaving_the_form_with_esc_writes_nothing_and_reaches_the_chat() {
    let mut a = conversation();
    let before = chat_of(&a).messages().count();
    ask(&mut a, "/feedback The chat hangs");
    let plain_esc = || AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let mut effects = a.update(plain_esc());
    // Released first: a second press of a key still down counts as held.
    a.update(AppEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    )));
    effects.extend(a.update(plain_esc()));
    assert_eq!(issues(&effects), Vec::new());
    assert!(a.menu().is_none());
    assert_eq!(chat_of(&a).messages().count(), before);
}

// A message that starts with the command is still a message when escaped.
#[test]
fn a_double_slash_sends_feedback_as_a_message() {
    let mut a = conversation();
    let effects = ask(&mut a, "//feedback is a command");
    assert_eq!(sends(&effects), 1, "{effects:?}");
    assert!(a.menu().is_none());
}
