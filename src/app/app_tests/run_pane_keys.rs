//! The keys of the run-in-pane confirm, the accept-time rechecks, the one
//! write, and the lifecycle of the confirm (pane-commands, modal-input).

use super::chat_live::{ollama, repeat};
use super::run_pane::{BLOCK, answer, chat, one_shell_and_a_chat};
use super::*;
use crate::core::ai::chat::approval::{ApprovalAsk, Origin};

fn pending_id(a: &App, id: PaneId) -> Option<u64> {
    chat(a, id).approvals().pending().map(|p| p.id())
}

fn writes(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::WritePty(..)))
        .collect()
}

/// A shell (pane 1), a focused chat (pane 2) and an open confirm for `text`.
fn opened(text: &str) -> App {
    let mut a = one_shell_and_a_chat(text);
    press(&mut a, "e");
    assert!(pending_id(&a, id2()).is_some(), "the confirm is open");
    a
}

fn shell_output(a: &mut App, pane: PaneId, bytes: &[u8]) {
    a.update(AppEvent::Pty(pane, PtyEvent::Output(bytes.to_vec())));
}

fn plain_key(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn with_mods(code: KeyCode, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, mods))
}

// 1b.3: the keys -------------------------------------------------------------

#[test]
fn y_types_the_command_with_one_write_and_no_enter() {
    let mut a = opened(BLOCK);
    let effects = a.update(key('y'));
    assert_eq!(
        effects,
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"cd ~/dev/curso-rust".to_vec()
        )]
    );
    assert_eq!(pending_id(&a, id2()), None);
    // A second y after the confirm closed writes nothing.
    assert_eq!(writes(&a.update(key('y'))), Vec::<&Effect>::new());
}

#[test]
fn n_and_esc_decline_and_write_nothing_and_keep_the_composer() {
    for decline in [key('n'), plain_key(KeyCode::Esc)] {
        let mut a = one_shell_and_a_chat(BLOCK);
        for c in "draft".chars() {
            a.update(key(c));
        }
        press(&mut a, "e");
        assert_eq!(a.update(decline), vec![]);
        assert_eq!(pending_id(&a, id2()), None);
        assert_eq!(chat(&a, id2()).composer().text(), "draft");
    }
}

#[test]
fn other_keys_are_swallowed_and_the_confirm_stays() {
    let mut a = one_shell_and_a_chat(BLOCK);
    for c in "draft".chars() {
        a.update(key(c));
    }
    press(&mut a, "e");
    let id = pending_id(&a, id2());
    let keys = [
        with_mods(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        plain_key(KeyCode::Enter),
        key(' '),
        with_mods(KeyCode::Char('y'), KeyModifiers::CONTROL),
        with_mods(KeyCode::Char('y'), KeyModifiers::ALT),
        key('a'),
        plain_key(KeyCode::Tab),
        plain_key(KeyCode::F(5)),
    ];
    for event in keys {
        assert_eq!(a.update(event.clone()), vec![], "{event:?}");
        assert_eq!(pending_id(&a, id2()), id, "{event:?}");
    }
    assert_eq!(chat(&a, id2()).composer().text(), "draft");
}

#[test]
fn repeat_release_and_paste_do_not_answer() {
    let mut a = one_shell_and_a_chat(BLOCK);
    for c in "draft".chars() {
        a.update(key(c));
    }
    press(&mut a, "e");
    let id = pending_id(&a, id2());
    assert_eq!(
        a.update(repeat(KeyCode::Char('y'), KeyModifiers::NONE)),
        vec![]
    );
    let release = AppEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char('y'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert_eq!(a.update(release), vec![]);
    assert_eq!(a.update(AppEvent::Paste("y".into())), vec![]);
    assert_eq!(pending_id(&a, id2()), id);
    assert_eq!(chat(&a, id2()).composer().text(), "draft");
}

#[test]
fn ctrl_c_denies_a_streaming_chat_and_never_quits() {
    let mut a = one_shell_and_a_chat(BLOCK);
    a.update(key('q'));
    let effects = a.update(plain_key(KeyCode::Enter));
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
    // A tool call asks while the answer streams.
    let chat_state = a.focused_state_mut().chat_mut().unwrap();
    chat_state
        .approvals_mut()
        .request(
            ApprovalAsk::Run {
                command: "ls".into(),
            },
            Origin::Tool,
        )
        .unwrap();
    chat_state.set_run_target(Some(PaneId::FIRST));
    let effects = a.update(with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(effects, vec![Effect::ChatCancel(id2())]);
    assert_eq!(pending_id(&a, id2()), None);
}

#[test]
fn ctrl_c_with_an_idle_confirm_closes_it_without_quitting() {
    let mut a = opened(BLOCK);
    let effects = a.update(with_mods(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(effects, vec![]);
    assert_eq!(pending_id(&a, id2()), None);
    assert_ne!(a.input(), InputMode::Confirm(Confirm::Quit));
}

#[test]
fn the_prefix_still_works_and_the_confirm_stays() {
    let mut a = opened(BLOCK);
    let id = pending_id(&a, id2());
    press(&mut a, "wv");
    assert_eq!(a.tiling().panes.len(), 3, "a split was made");
    assert_eq!(pending_id(&a, id2()), id);
}

#[test]
fn the_menu_shields_the_confirm() {
    let mut a = opened(BLOCK);
    let id = pending_id(&a, id2());
    press(&mut a, "m");
    assert_eq!(a.input(), InputMode::Menu);
    let effects = a.update(key('y'));
    assert!(writes(&effects).is_empty());
    assert_eq!(pending_id(&a, id2()), id);
}

// 1b.4: accept-time rechecks and the write ------------------------------------

#[test]
fn a_multiline_command_is_one_bracketed_paste_when_the_shell_asked_for_it() {
    let mut a = opened("```bash\na\nb\n```");
    shell_output(&mut a, PaneId::FIRST, b"\x1b[?2004h");
    let effects = a.update(key('y'));
    assert_eq!(
        effects,
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"\x1b[200~a\nb\x1b[201~".to_vec()
        )]
    );
}

#[test]
fn without_bracketed_paste_multiline_and_tabs_are_refused_and_one_line_is_raw() {
    for text in ["```bash\na\nb\n```", "```bash\na\tb\n```"] {
        let mut a = opened(text);
        let effects = a.update(key('y'));
        assert!(writes(&effects).is_empty(), "{text:?}");
        assert_eq!(
            a.notice(),
            Some("The shell cannot take a multi-line command safely"),
            "{text:?}"
        );
        assert_eq!(pending_id(&a, id2()), None, "{text:?}");
    }
    let mut a = opened("```bash\ncd /tmp\n```");
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(PaneId::FIRST, b"cd /tmp".to_vec())]
    );
}

#[test]
fn the_bracketed_paste_state_is_read_at_accept_time() {
    let mut a = one_shell_and_a_chat("```bash\na\nb\n```");
    shell_output(&mut a, PaneId::FIRST, b"\x1b[?2004h");
    press(&mut a, "e");
    shell_output(&mut a, PaneId::FIRST, b"\x1b[?2004l");
    let effects = a.update(key('y'));
    assert!(writes(&effects).is_empty());
}

#[test]
fn no_accept_ever_writes_a_carriage_return() {
    for text in [
        "```bash\nls\n\n\n```",
        "```bash\nls   \n```",
        "```bash\na\nb\n```",
    ] {
        let mut a = opened(text);
        shell_output(&mut a, PaneId::FIRST, b"\x1b[?2004h");
        let effects = a.update(key('y'));
        let [Effect::WritePty(_, bytes)] = effects.as_slice() else {
            panic!("{effects:?}");
        };
        let body = bytes
            .strip_prefix(b"\x1b[200~")
            .and_then(|b| b.strip_suffix(b"\x1b[201~"))
            .expect("framed");
        assert!(!body.contains(&b'\r') && !body.ends_with(b"\n"), "{text:?}");
    }
}

#[test]
fn a_full_screen_app_refuses_and_closes_the_confirm() {
    let mut a = opened(BLOCK);
    shell_output(&mut a, PaneId::FIRST, b"\x1b[?1049h");
    let effects = a.update(key('y'));
    assert!(writes(&effects).is_empty());
    assert_eq!(a.notice(), Some("Pane 1 runs a full-screen app"));
    assert_eq!(pending_id(&a, id2()), None);
}

/// Shells 1 and 2 and a focused chat (pane 3) with an open confirm aimed at 2.
fn two_shells() -> (App, PaneId, PaneId) {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "wv");
    let second = a.focused();
    press(&mut a, "c");
    let chat_id = a.focused();
    answer(&mut a, chat_id, BLOCK);
    press(&mut a, "e");
    assert_eq!(chat(&a, chat_id).run_target(), Some(second));
    (a, second, chat_id)
}

#[test]
fn a_target_closed_while_pending_refuses_y_and_p_picks_another() {
    let (mut a, second, chat_id) = two_shells();
    a.update(AppEvent::Pty(second, PtyEvent::Exited));
    let effects = a.update(key('y'));
    assert!(writes(&effects).is_empty());
    assert_eq!(
        a.notice(),
        Some("That pane is gone: press p to pick another")
    );
    assert!(pending_id(&a, chat_id).is_some(), "the confirm stays");
    assert_eq!(a.update(key('p')), vec![]);
    assert_eq!(chat(&a, chat_id).run_target(), Some(PaneId::FIRST));
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(
            PaneId::FIRST,
            b"cd ~/dev/curso-rust".to_vec()
        )]
    );
}

#[test]
fn a_late_event_for_a_closed_target_is_ignored() {
    let (mut a, second, _) = two_shells();
    a.update(AppEvent::Pty(second, PtyEvent::Exited));
    assert_eq!(
        a.update(AppEvent::Pty(second, PtyEvent::Output(b"late".to_vec()))),
        vec![]
    );
    assert_eq!(a.update(AppEvent::Pty(second, PtyEvent::Exited)), vec![]);
}

#[test]
fn the_write_goes_to_the_named_target_whatever_the_focus_did() {
    let (mut a, second, chat_id) = two_shells();
    press(&mut a, "h");
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    press(&mut a, "l");
    press(&mut a, "l");
    assert_eq!(a.focused(), chat_id);
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(second, b"cd ~/dev/curso-rust".to_vec())]
    );
}

#[test]
fn a_pane_removed_that_is_neither_chat_nor_target_keeps_the_confirm() {
    let (mut a, second, chat_id) = two_shells();
    a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(second, b"cd ~/dev/curso-rust".to_vec())]
    );
    let _ = chat_id;
}

// 1b.5: lifecycle ------------------------------------------------------------

#[test]
fn the_confirm_survives_a_tab_switch() {
    let mut a = opened(BLOCK);
    let id = pending_id(&a, id2());
    press(&mut a, "tn");
    press(&mut a, "b1");
    assert_eq!(a.focused(), id2());
    assert_eq!(pending_id(&a, id2()), id);
}

#[test]
fn a_second_request_is_refused_and_the_first_is_untouched() {
    let mut a = opened(BLOCK);
    let id = pending_id(&a, id2());
    assert_eq!(press(&mut a, "e"), vec![]);
    assert_eq!(
        a.notice(),
        Some("A confirm is already open: answer it first")
    );
    assert_eq!(pending_id(&a, id2()), id);
}

#[test]
fn y_on_a_shell_pane_goes_to_the_shell_and_the_confirm_stays() {
    let mut a = opened(BLOCK);
    let id = pending_id(&a, id2());
    press(&mut a, "h");
    assert_eq!(a.focused(), PaneId::FIRST);
    assert_eq!(
        a.update(key('y')),
        vec![Effect::WritePty(PaneId::FIRST, b"y".to_vec())]
    );
    assert_eq!(pending_id(&a, id2()), id);
}

#[test]
fn closing_the_chat_pane_ends_the_confirm_without_a_write() {
    let mut a = opened(BLOCK);
    let mut effects = press(&mut a, "wq");
    effects.extend(a.update(key('y')));
    assert!(writes(&effects).is_empty(), "{effects:?}");
    assert!(a.pane_view(id2()).is_none());
}

#[test]
fn turning_tools_off_ends_the_confirm_and_the_hint() {
    let mut a = opened(BLOCK);
    a.ai.set_tools_enabled(false);
    let effects = a.sync_tools();
    assert!(writes(&effects).is_empty());
    assert_eq!(pending_id(&a, id2()), None);
    assert!(!chat(&a, id2()).run_hint());
}

// A hidden confirm must never be answerable --------------------------------

const PENDING_FIRST: &str = "Answer the pending confirm first";

fn overlays_open(a: &App) -> bool {
    let c = chat(a, id2());
    c.tag_editor().is_some()
        || c.note_editor().is_some()
        || c.summary_draft().is_some()
        || c.list().is_some()
}

#[test]
fn prefix_actions_that_replace_the_conversation_refuse_while_a_confirm_waits() {
    for keys in ["T", "n", "s"] {
        let mut a = opened(BLOCK);
        let id = pending_id(&a, id2());
        assert_eq!(press(&mut a, keys), vec![], "{keys}");
        assert_eq!(a.notice(), Some(PENDING_FIRST), "{keys}");
        assert!(!overlays_open(&a), "{keys}");
        assert_eq!(pending_id(&a, id2()), id, "{keys}");
    }
}

#[test]
fn the_blind_y_after_prefix_t_writes_nothing() {
    let mut a = opened(BLOCK);
    press(&mut a, "T");
    let mut effects = Vec::new();
    for c in "python".chars() {
        effects.extend(a.update(key(c)));
    }
    effects.extend(a.update(key('y')));
    // The tag editor never opened, so the visible prompt took the keys.
    assert!(
        !overlays_open(&a),
        "no overlay ever replaced the prompt: {effects:?}"
    );
}

#[test]
fn keys_go_to_an_overlay_that_is_open_and_never_answer_the_hidden_confirm() {
    use crate::core::ai::chat::tag_editor::TagTarget;
    type Open = fn(&mut ChatState);
    let openers: [(&str, Open); 3] = [
        ("tags", |c| {
            assert!(c.open_tag_editor(TagTarget::Pane));
        }),
        ("note", |c| {
            c.open_note_editor().unwrap();
        }),
        ("list", ChatState::open_list),
    ];
    for (name, open) in openers {
        let mut a = opened(BLOCK);
        let id = pending_id(&a, id2());
        open(a.focused_state_mut().chat_mut().unwrap());
        let effects = a.update(key('y'));
        assert!(writes(&effects).is_empty(), "{name}: {effects:?}");
        assert_eq!(pending_id(&a, id2()), id, "{name}");
        assert_eq!(
            a.update(AppEvent::Paste("y".into())),
            vec![],
            "{name}: a paste goes to the overlay"
        );
        assert_eq!(pending_id(&a, id2()), id, "{name}");
    }
}

// The outcome line ------------------------------------------------------------

fn system_lines(a: &App) -> Vec<String> {
    chat(a, id2())
        .entries()
        .iter()
        .filter_map(|e| match e {
            crate::core::ai::chat::Entry::System(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_accept_leaves_a_sent_line_and_a_decline_a_cancelled_line() {
    let mut a = opened(BLOCK);
    a.update(key('y'));
    assert_eq!(
        system_lines(&a),
        vec!["sent to pane 1; press Enter there to run it".to_owned()]
    );
    for decline in [key('n'), plain_key(KeyCode::Esc)] {
        let mut a = opened(BLOCK);
        a.update(decline);
        assert_eq!(system_lines(&a), vec!["cancelled".to_owned()]);
    }
}

#[test]
fn a_refused_write_leaves_no_sent_line() {
    let mut a = opened(BLOCK);
    shell_output(&mut a, PaneId::FIRST, b"\x1b[?1049h");
    a.update(key('y'));
    assert!(system_lines(&a).iter().all(|l| !l.starts_with("sent to")));
}

#[test]
fn the_outcome_line_is_never_stored_or_sent_and_does_not_hide_the_block() {
    let mut a = opened(BLOCK);
    a.update(key('y'));
    let mut b = opened(BLOCK);
    b.update(key('n'));
    for app in [&mut a, &mut b] {
        // Not stored: no message of the conversation carries it.
        assert!(
            chat(app, id2())
                .messages()
                .all(|m| !m.text.contains("sent to pane") && m.text != "cancelled")
        );
        // Not sent: the next request's history holds no such line.
        app.update(key('q'));
        let effects = app.update(plain_key(KeyCode::Enter));
        let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
            panic!("{effects:?}");
        };
        assert!(
            request
                .messages
                .iter()
                .all(|m| !m.content.contains("sent to pane") && m.content != "cancelled"),
            "{:?}",
            request.messages
        );
    }
    // The run action stays available after an outcome line.
    let mut c = opened(BLOCK);
    c.update(key('n'));
    press(&mut c, "e");
    assert!(pending_id(&c, id2()).is_some());
}
