//! `/github` and its dialog (chat-pane: The `/github` command, The grant
//! dialog; github-tools: The chat grant). Only a human keypress grants.

use std::path::PathBuf;

use super::chat_live::{composer, type_text};
use super::github_grant::{grant, granted, tools_app};
use super::run_pane::chat;
use super::tool_approval::{announce_now, pending};
use super::*;
use crate::core::ai::{
    chat::{
        Entry, Msg, MsgStatus,
        approval::{ApprovalAsk, GITHUB_GRANT_KEYS},
        engine::{CallId, Decision, Finish, Role, StreamEvent, ToolStatus},
    },
    config::ToolToggle,
    tools::{
        github::{GhRepo, slug_of},
        registry::Family,
    },
};

fn slug(url: &str) -> GhRepo {
    GhRepo::Found(slug_of(url).expect("a github url"))
}

fn lines(a: &App) -> Vec<String> {
    (chat(a, id2()).entries().iter())
        .filter_map(|e| match e {
            Entry::System(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn messages(a: &App) -> usize {
    (chat(a, id2()).entries().iter())
        .filter(|e| matches!(e, Entry::Message(_)))
        .count()
}

fn folder(a: &App) -> PathBuf {
    chat(a, id2()).origin_cwd().expect("a folder").to_path_buf()
}

fn probe_effect(a: &App, dialog: bool) -> Effect {
    Effect::ProbeOrigin {
        pane: id2(),
        folder: folder(a),
        dialog,
    }
}

fn answer(a: &mut App, repo: GhRepo) -> Vec<Effect> {
    let folder = folder(a);
    a.update(AppEvent::OriginProbed {
        pane: id2(),
        folder,
        dialog: true,
        repo,
    })
}

/// Types `text` in the composer and presses Enter.
fn submit(a: &mut App, text: &str) -> Vec<Effect> {
    type_text(a, text);
    a.update(super::chat_live::plain(KeyCode::Enter))
}

/// The dialog is open for `o/r`.
fn dialog(a: &mut App) {
    assert_eq!(submit(a, "/github"), vec![probe_effect(a, true)]);
    assert_eq!(answer(a, slug("https://github.com/o/r.git")), vec![]);
    assert!(matches!(
        pending(a).map(|(ask, _)| ask),
        Some(ApprovalAsk::GithubGrant { .. })
    ));
}

fn wait_for_arming() {
    NOW_MS.with(|n| n.set(n.get() + 1_000));
}

fn sent(effects: &[Effect]) -> Option<(u64, crate::core::ai::chat::engine::ChatRequest)> {
    match effects {
        [Effect::ChatSend { turn, request, .. }] => Some((*turn, request.clone())),
        _ => None,
    }
}

// Spec (The command is not a message): the dialog opens from the probe, the
// composer is empty, nothing is added, nothing is sent or stored.
#[test]
fn the_command_probes_the_origin_and_is_not_a_message() {
    let mut a = tools_app();
    let before = messages(&a);
    let effects = submit(&mut a, "/github");
    assert_eq!(effects, vec![probe_effect(&a, true)]);
    assert_eq!(composer(&a), "");
    assert_eq!(messages(&a), before);
    assert_eq!(pending(&a), None, "the dialog waits for the answer");
    // The answer opens the dialog that names the repository.
    assert_eq!(answer(&mut a, slug("https://github.com/o/r.git")), vec![]);
    let prompt = a.approval_prompt(id2()).expect("the dialog");
    assert_eq!(prompt.command, "o/r");
    assert_eq!(prompt.keys, GITHUB_GRANT_KEYS);
    assert_eq!(messages(&a), before);
}

// Spec (Not in the history or the store).
#[test]
fn the_command_is_in_neither_the_request_nor_the_store() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    a.update(key('y'));
    let effects = submit(&mut a, "hello");
    let (_, request) = sent(&effects).unwrap_or_else(|| panic!("{effects:?}"));
    assert!(
        request
            .messages
            .iter()
            .all(|m| !m.content.contains("/github"))
    );
    assert_eq!(
        request.messages.last().map(|m| m.content.as_str()),
        Some("hello")
    );
    assert!(
        effects
            .iter()
            .chain(a.update(key('z')).iter())
            .all(|e| match e {
                Effect::Store(cmd) => !format!("{cmd:?}").contains("/github"),
                _ => true,
            })
    );
}

// Spec (Arguments refused, Escaped, Other text is a normal message).
#[test]
fn arguments_are_refused_an_escape_is_sent_and_other_text_is_a_message() {
    let mut a = tools_app();
    assert_eq!(submit(&mut a, "/github now"), vec![]);
    assert_eq!(composer(&a), "/github now");
    assert!(lines(&a).contains(&"/github takes no arguments".to_owned()));
    assert_eq!(pending(&a), None);

    let mut a = tools_app();
    let effects = submit(&mut a, "//github is a command");
    let (_, request) = sent(&effects).unwrap_or_else(|| panic!("{effects:?}"));
    assert_eq!(
        request.messages.last().map(|m| m.content.as_str()),
        Some("/github is a command")
    );

    let mut a = tools_app();
    let effects = submit(&mut a, "what does /github do");
    let (_, request) = sent(&effects).unwrap_or_else(|| panic!("{effects:?}"));
    assert_eq!(
        request.messages.last().map(|m| m.content.as_str()),
        Some("what does /github do")
    );
}

// Spec: refused with a visible line, the composer cleared, no dialog.
#[test]
fn the_command_is_refused_with_a_line_when_it_cannot_apply() {
    // Tools off.
    let mut a = tools_app();
    a.ai.set_tools_enabled(false);
    assert_eq!(submit(&mut a, "/github"), vec![]);
    assert!(
        lines(&a).iter().any(|l| l.contains("tools are off")),
        "{:?}",
        lines(&a)
    );
    assert_eq!(composer(&a), "");
    // Already on by the toggle, and by an earlier grant.
    for by_toggle in [true, false] {
        let mut a = tools_app();
        if by_toggle {
            a.ai.set_toggle(ToolToggle::Github, true);
        } else {
            grant(&mut a, id2());
        }
        assert_eq!(submit(&mut a, "/github"), vec![], "toggle {by_toggle}");
        assert!(
            lines(&a).iter().any(|l| l.contains("already on")),
            "{:?}",
            lines(&a)
        );
        assert_eq!(composer(&a), "");
    }
    // A pending confirm keeps the composer and says so.
    let mut a = tools_app();
    type_text(&mut a, "/github");
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(
            ApprovalAsk::read("a.rs"),
            crate::core::ai::chat::approval::Origin::Block,
        )
        .unwrap();
    let effects = a.chat_submit(id2());
    assert_eq!(effects, vec![]);
    assert_eq!(composer(&a), "/github");
    assert!(lines(&a).contains(&"Answer the pending confirm first".to_owned()));
    assert_eq!(
        a.notice.as_deref(),
        Some("Answer the pending confirm first")
    );
}

// Spec: the answer of the probe says why there is no dialog.
#[test]
fn a_repository_that_is_not_there_is_told_and_opens_nothing() {
    for repo in [
        GhRepo::NoRepo,
        GhRepo::NoOrigin,
        GhRepo::NotGithub,
        GhRepo::Unreadable,
    ] {
        let mut a = tools_app();
        submit(&mut a, "/github");
        assert_eq!(answer(&mut a, repo.clone()), vec![], "{repo:?}");
        let sentence = repo.refusal().unwrap();
        assert!(
            lines(&a).iter().any(|l| l.contains(sentence)),
            "{repo:?}: {:?}",
            lines(&a)
        );
        assert_eq!(pending(&a), None, "{repo:?}");
    }
}

// Design: a stale answer after a folder change is dropped.
#[test]
fn an_answer_for_another_folder_is_dropped() {
    let mut a = tools_app();
    submit(&mut a, "/github");
    let effects = a.update(AppEvent::OriginProbed {
        pane: id2(),
        folder: PathBuf::from("/elsewhere"),
        dialog: true,
        repo: slug("https://github.com/o/r.git"),
    });
    assert_eq!(effects, vec![]);
    assert_eq!(pending(&a), None);
    // Dropped, but not silently: the user typed the command.
    assert!(
        lines(&a).contains(&"/github cancelled: the folder changed".to_owned()),
        "{:?}",
        lines(&a)
    );
}

// Spec (Works during a stream): the stream is unaffected and the grant waits
// for the next message.
#[test]
fn the_command_works_during_a_stream_and_applies_next_message() {
    let mut a = tools_app();
    let effects = submit(&mut a, "first");
    let (turn, running) = sent(&effects).unwrap();
    assert_eq!(submit(&mut a, "/github"), vec![probe_effect(&a, true)]);
    answer(&mut a, slug("https://github.com/o/r.git"));
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(&a, id2()));
    assert!(!running.families.contains(Family::Github));
    assert!(
        !lines(&a)
            .iter()
            .any(|l| l.contains("wait") || l.contains("cancel"))
    );
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    let (_, next) = sent(&submit(&mut a, "second")).unwrap();
    assert!(next.families.contains(Family::Github));
}

// Spec (y grants, a grant never writes the file).
#[test]
fn y_grants_with_a_line_and_writes_nothing() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    let effects = a.update(key('y'));
    assert_eq!(effects, vec![], "no config write, no store row");
    assert!(granted(&a, id2()));
    assert_eq!(pending(&a), None);
    let granted_line = lines(&a)
        .into_iter()
        .find(|l| l.contains("next message"))
        .expect("the grant notice");
    // The three facts of the notice.
    assert!(granted_line.contains("cloud"), "{granted_line}");
    assert!(granted_line.contains("typed"), "{granted_line}");
    assert!(
        granted_line.is_ascii() && granted_line.len() <= 80,
        "{granted_line}"
    );
    assert!(!a.ai.toggle(ToolToggle::Github));
}

// Spec (n and Esc decline, Decline).
#[test]
fn n_and_esc_decline_and_the_dialog_does_not_come_back() {
    for decline in [
        key('n'),
        AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    ] {
        let mut a = tools_app();
        dialog(&mut a);
        wait_for_arming();
        assert_eq!(a.update(decline), vec![]);
        assert!(!granted(&a, id2()));
        assert_eq!(pending(&a), None);
        assert!(lines(&a).contains(&"GitHub tools stay off".to_owned()));
        assert!(!has_line(&a, "cancelled"), "declining says it once");
        let (_, request) = sent(&submit(&mut a, "hi")).unwrap();
        assert!(!request.families.contains(Family::Github));
        assert_eq!(pending(&a), None);
    }
}

// Spec (Other keys are swallowed): each says so and the dialog stays open.
#[test]
fn every_other_key_is_swallowed_with_a_notice() {
    let keys = [
        KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    ];
    for key in keys {
        let mut a = tools_app();
        dialog(&mut a);
        wait_for_arming();
        a.update(AppEvent::Key(key));
        assert!(!granted(&a, id2()), "{key:?}");
        assert!(
            matches!(
                pending(&a).map(|(ask, _)| ask),
                Some(ApprovalAsk::GithubGrant { .. })
            ),
            "{key:?}"
        );
        assert_eq!(
            a.notice.as_deref(),
            Some("Answer the pending confirm first"),
            "{key:?}"
        );
    }
    // A held key and a release are not answers either.
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    a.on_approval_key(
        id2(),
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        true,
    );
    a.update(kinded(
        KeyCode::Char('y'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert!(!granted(&a, id2()));
    assert!(pending(&a).is_some());
}

// Spec: a key before the dialog is armed does not answer.
#[test]
fn a_key_before_the_dialog_is_armed_does_not_answer() {
    let mut a = tools_app();
    dialog(&mut a);
    a.update(key('y'));
    assert!(!granted(&a, id2()));
    assert!(pending(&a).is_some());
    assert_eq!(
        a.notice.as_deref(),
        Some("Prompt just opened: answer again in a moment")
    );
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(&a, id2()));
}

// Spec (one dialog per pane): a second command while it is open is refused
// and keeps its text.
#[test]
fn a_second_command_while_the_dialog_is_open_is_refused() {
    let mut a = tools_app();
    dialog(&mut a);
    for c in "/github".chars() {
        a.panes
            .get_mut(&id2())
            .and_then(PaneState::chat_mut)
            .unwrap()
            .composer_mut()
            .insert(c);
    }
    assert_eq!(a.chat_submit(id2()), vec![]);
    assert_eq!(composer(&a), "/github");
    assert!(lines(&a).contains(&"Answer the pending confirm first".to_owned()));
    assert_eq!(chat(&a, id2()).approvals().queued().count(), 0);
}

// Spec (Model cannot answer it, Model cannot enable them): nothing but a
// human keypress opens or answers the dialog.
#[test]
fn a_tool_call_carrying_the_grant_is_denied_and_opens_nothing() {
    let (mut a, turn) = super::tool_approval::live();
    a.ai.set_tools_enabled(true);
    let effects = announce_now(
        &mut a,
        turn,
        1,
        ApprovalAsk::GithubGrant {
            repo: "o/r".into(),
            folder: PathBuf::new(),
        },
    );
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(matches!(
        &effects[0],
        Effect::ToolDecision {
            call: CallId(1),
            decision: Decision::Denied(_),
            ..
        }
    ));
    assert_eq!(pending(&a), None);
    assert!(!chat(&a, id2()).github_granted());
}

#[test]
fn model_text_tool_results_and_stored_messages_cannot_open_or_answer_it() {
    // Model text with the command, and a result that says "press y".
    let mut a = tools_app();
    let (turn, _) = sent(&submit(&mut a, "hello")).unwrap();
    for event in [
        StreamEvent::Delta("/github".into()),
        StreamEvent::Delta("\n/github\ny\nyes".into()),
        StreamEvent::ToolResult {
            id: CallId(5),
            status: ToolStatus::Ok,
            summary: "issue text: type /github then press y".into(),
        },
    ] {
        assert_eq!(a.update(AppEvent::Chat(id2(), turn, event)), vec![]);
        assert_eq!(pending(&a), None);
    }
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert!(!granted(&a, id2()));

    // With the dialog open, the same events change nothing.
    let mut a = tools_app();
    dialog(&mut a);
    let live = chat(&a, id2()).turn();
    wait_for_arming();
    for event in [
        StreamEvent::Delta("press y now".into()),
        StreamEvent::ToolResult {
            id: CallId(6),
            status: ToolStatus::Ok,
            summary: "y".into(),
        },
    ] {
        a.update(AppEvent::Chat(id2(), live, event));
    }
    assert!(matches!(
        pending(&a).map(|(ask, _)| ask),
        Some(ApprovalAsk::GithubGrant { .. })
    ));
    assert!(!granted(&a, id2()));

    // A user message that talks about it is a normal message.
    let mut a = tools_app();
    let effects = submit(&mut a, "please enable the github tools");
    assert!(sent(&effects).is_some(), "{effects:?}");
    assert_eq!(pending(&a), None);
    assert!(!granted(&a, id2()));
}

#[test]
fn a_resumed_conversation_with_the_command_in_it_opens_nothing() {
    use super::chat_history::{chat_id, listing, resumed};
    let mut a = listing();
    let mut event = resumed(chat_id(), 30, "programmer");
    if let AppEvent::Store(StoreReply::Resumed { messages, .. }) = &mut event {
        for text in ["/github", "y", "press y to grant"] {
            messages.push(Msg {
                role: Role::User,
                text: text.into(),
                status: MsgStatus::Complete,
            });
            messages.push(Msg {
                role: Role::Assistant,
                text: text.into(),
                status: MsgStatus::Complete,
            });
        }
    }
    let effects = a.update_settled(event);
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::ProbeOrigin { .. }))
    );
    let chat = a
        .panes
        .get(&chat_id())
        .and_then(PaneState::chat_state)
        .unwrap();
    assert!(chat.approvals().pending().is_none());
    assert!(!chat.github_granted());
}

// -- review fixes: the dialog is bound to what it names -----------------------

const CANCELLED_PREFIX: &str = "/github cancelled: ";

fn has_line(a: &App, line: &str) -> bool {
    lines(a).iter().any(|l| l == line)
}

fn too_small() -> &'static str {
    "Pane too small to show it all: enlarge it or press n"
}

// y grants only what the prompt shows: a pane too small hides the repository.
#[test]
fn y_does_not_grant_when_the_dialog_does_not_fit_the_pane() {
    let mut a = tools_app();
    assert_eq!(submit(&mut a, "/github").len(), 1);
    let long = format!("https://github.com/owner/{}.git", "r".repeat(60));
    assert_eq!(answer(&mut a, slug(&long)), vec![]);
    a.update(AppEvent::Resize { cols: 20, rows: 6 });
    wait_for_arming();
    a.update(key('y'));
    assert!(!granted(&a, id2()));
    assert!(pending(&a).is_some(), "the dialog stays open");
    assert_eq!(a.notice(), Some(too_small()));
    // A pane that fits grants.
    a.update(AppEvent::Resize { cols: 80, rows: 25 });
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(&a, id2()), "{:?} {:?}", a.notice(), pending(&a));
}

// A dialog names the folder it was read for: y refuses once it is not the one.
#[test]
fn y_refuses_a_dialog_whose_folder_is_not_the_chat_folder_now() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    // The folder moves without the app's own change path.
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_folder(PathBuf::from("/tmp"));
    a.update(key('y'));
    assert!(!granted(&a, id2()));
    assert_eq!(pending(&a), None, "the dialog is withdrawn");
    assert!(has_line(&a, "/github cancelled: the folder changed"));
}

// A folder change withdraws the dialog, and says so; no generic line.
#[test]
fn a_folder_change_withdraws_the_dialog_with_a_line() {
    for restore in [false, true] {
        let mut a = tools_app();
        dialog(&mut a);
        if restore {
            a.restore_folder(id2(), PathBuf::from("/tmp"));
        } else {
            a.apply_folder(id2(), PathBuf::from("/tmp"));
        }
        assert_eq!(pending(&a), None, "restore {restore}");
        assert!(!granted(&a, id2()));
        assert!(has_line(&a, "/github cancelled: the folder changed"));
        assert!(!has_line(&a, "cancelled"), "no second line");
    }
}

fn list_app() -> App {
    use super::chat_history::{detection, listed, prompt_for, rows};
    let mut a = launched(120, 30)
        .with_ai(super::chat_live::ollama())
        .with_chat_history()
        .with_tool_engines(&["ollama"]);
    a.update(AppEvent::EnginesDetected(detection()));
    press(&mut a, "c");
    a.update(prompt_for(id2()));
    listed(&mut a, id2(), rows());
    assert!(a.list_open(id2()));
    a
}

// The sequence: the probe is in flight, the list opens, the answer arrives,
// then the resume lands. No dialog ever opens over the other conversation.
#[test]
fn a_probe_answer_with_the_list_open_is_cancelled_and_a_resume_opens_nothing() {
    use super::chat_history::resumed;
    let mut a = list_app();
    assert_eq!(a.github_submit(id2()), vec![probe_effect(&a, true)]);
    assert_eq!(
        answer(&mut a, slug("https://github.com/o/r.git")),
        vec![],
        "no second probe"
    );
    assert_eq!(pending(&a), None);
    assert!(
        lines(&a).iter().any(|l| l.starts_with(CANCELLED_PREFIX)),
        "{:?}",
        lines(&a)
    );
    a.update_settled(resumed(id2(), 30, "programmer"));
    assert_eq!(pending(&a), None);
    assert!(!granted(&a, id2()));
}

// A dialog that is open when a resume lands is withdrawn with a line.
#[test]
fn a_resume_withdraws_a_pending_dialog_with_a_line() {
    use super::chat_history::resumed;
    let mut a = list_app();
    let ask = ApprovalAsk::GithubGrant {
        repo: "o/r".into(),
        folder: folder(&a),
    };
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .approvals_mut()
        .request(ask, crate::core::ai::chat::approval::Origin::Block)
        .unwrap();
    a.update_settled(resumed(id2(), 30, "programmer"));
    assert_eq!(pending(&a), None);
    assert!(!granted(&a, id2()));
    assert!(has_line(
        &a,
        "/github cancelled: a conversation was resumed"
    ));
    assert!(!has_line(&a, "cancelled"), "no second line");
}

// -- review fixes: Ctrl+C, focus ---------------------------------------------

#[test]
fn ctrl_c_declines_the_dialog_at_once_and_cancels_the_stream() {
    let mut a = tools_app();
    let (turn, _) = sent(&submit(&mut a, "first")).unwrap();
    dialog(&mut a);
    // Not armed yet: Ctrl+C always works.
    let effects = a.update(super::chat_live::ctrl('c'));
    assert!(effects.contains(&Effect::ChatCancel(id2())), "{effects:?}");
    assert!(!granted(&a, id2()));
    assert_eq!(pending(&a), None);
    assert!(has_line(&a, "GitHub tools stay off"));
    assert!(
        !has_line(&a, "cancelled"),
        "no generic line from the dialog"
    );
    let _ = turn;
}

#[test]
fn gaining_the_focus_arms_the_dialog_again() {
    let mut a = tools_app();
    dialog(&mut a);
    wait_for_arming();
    a.set_focus(PaneId::FIRST);
    a.set_focus(id2());
    a.update(key('y'));
    assert!(!granted(&a, id2()), "a stray y right after the switch");
    assert_eq!(
        a.notice.as_deref(),
        Some("Prompt just opened: answer again in a moment")
    );
    wait_for_arming();
    a.update(key('y'));
    assert!(granted(&a, id2()));
}
