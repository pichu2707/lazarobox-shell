//! The chat folder (chat-folder): when it may change, and what a change does.

use super::run_pane::one_shell_and_a_chat;
use super::tool_approval::{announce, run};
use super::tool_run::run_live;
use super::*;

#[test]
fn an_idle_chat_may_change_folder() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(id2()), None);
}

#[test]
fn a_command_running_for_the_chat_blocks_the_change() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/dev"));
    assert_eq!(a.folder_busy(id2()), Some("a command is running"));
}

fn folder_of(a: &App) -> Option<PathBuf> {
    super::run_pane::chat(a, id2())
        .origin_cwd()
        .map(Path::to_path_buf)
}

#[test]
fn the_change_moves_the_chat_and_prepares_its_prompt_again() {
    let mut a = one_shell_and_a_chat("hello");
    let effects = a.apply_folder(id2(), "/w/b".into());
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/b")));
    let prepared: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::PreparePrompt { pane, origin, .. } => Some((*pane, origin.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(prepared, vec![(id2(), Some(PathBuf::from("/w/b")))]);
    assert!(a.take_dirty());
}

#[test]
fn the_change_ends_the_tool_session_of_the_old_folder_once() {
    let mut a = one_shell_and_a_chat("hello");
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .set_tool_session(true);
    let first = a.apply_folder(id2(), "/w/b".into());
    assert!(first.contains(&Effect::EndToolSession(id2())));
    let second = a.apply_folder(id2(), "/w/c".into());
    assert!(!second.contains(&Effect::EndToolSession(id2())));
}

#[test]
fn a_parked_send_blocks_the_change() {
    let mut a = one_shell_and_a_chat("hello");
    a.waiting_sends.push((id2(), "hi".into()));
    assert_eq!(
        a.folder_busy(id2()),
        Some("a message is waiting to be sent")
    );
}

#[test]
fn an_open_note_editor_blocks_the_change() {
    let a = super::chat_notes::ready("body");
    assert_eq!(a.folder_busy(id2()), Some("a note or summary is open"));
}

#[test]
fn a_chat_that_is_gone_is_not_busy() {
    let a = one_shell_and_a_chat("hello");
    assert_eq!(a.folder_busy(PaneId::for_test(99)), None);
}

// ---- the /cd command (chat-folder A2) --------------------------------------

use super::chat_history::{ask, code, plain, systems, type_text};
use super::chat_live::ollama;
use crate::core::ai::chat::folder::{Probed, Purpose, Refusal, Verdict};

fn composer(a: &App) -> String {
    super::run_pane::chat(a, id2()).composer().text()
}

fn messages(a: &App) -> usize {
    super::run_pane::chat(a, id2()).messages().count()
}

fn lines(a: &App) -> Vec<String> {
    systems(super::run_pane::chat(a, id2()))
}

fn probes(effects: &[Effect]) -> Vec<(PathBuf, Purpose)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolder { path, purpose, .. } => Some((path.clone(), *purpose)),
            _ => None,
        })
        .collect()
}

/// The token of the `/cd` probe the chat waits for.
fn cd_token(a: &App) -> u64 {
    a.cd_requests.get(&id2()).map_or(0, |request| request.token)
}

/// The token a `/cd` probe was asked with.
fn cd_purposes(effects: &[Effect]) -> Vec<u64> {
    probes(effects)
        .into_iter()
        .filter_map(|(_, purpose)| match purpose {
            Purpose::Cd(token) => Some(token),
            Purpose::Resume(_) => None,
        })
        .collect()
}

fn has_send(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::ChatSend { .. }))
}

/// A chat whose folder is `/w/a`, idle.
fn in_folder() -> App {
    let mut a = one_shell_and_a_chat("hello");
    a.apply_folder(id2(), "/w/a".into());
    // The prompt is prepared again for the new folder.
    a.update(AppEvent::PromptReady(
        id2(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".to_owned(),
            notices: Vec::new(),
        },
    ));
    a
}

fn probed(a: &mut App, result: Result<Probed, Refusal>) -> Vec<Effect> {
    let token = cd_token(a);
    probed_for(a, token, result)
}

fn probed_for(a: &mut App, token: u64, result: Result<Probed, Refusal>) -> Vec<Effect> {
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Cd(token),
        result,
    })
}

fn concrete(path: &str) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict: Verdict::Concrete,
    })
}

#[test]
fn cd_is_not_a_message_and_asks_for_a_probe() {
    let mut a = in_folder();
    let before = messages(&a);
    let effects = ask(&mut a, "/cd /w/b");
    assert!(!has_send(&effects), "{effects:?}");
    assert_eq!(
        probes(&effects),
        vec![(PathBuf::from("/w/b"), Purpose::Cd(cd_token(&a)))]
    );
    assert_eq!(messages(&a), before);
    assert_eq!(composer(&a), "/cd /w/b");
}

#[test]
fn bare_cd_shows_the_usage_and_keeps_the_composer() {
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd");
    assert!(probes(&effects).is_empty() && !has_send(&effects));
    assert!(lines(&a).contains(&"usage: /cd <folder>".to_owned()));
    assert_eq!(composer(&a), "/cd");
}

#[test]
fn a_relative_path_is_read_against_the_chat_folder() {
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd src");
    assert_eq!(probes(&effects)[0].0, PathBuf::from("/w/a/src"));
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd ..");
    assert_eq!(probes(&effects)[0].0, PathBuf::from("/w"));
}

#[test]
fn other_slash_text_is_a_normal_message() {
    for text in ["/cdx foo", "/etc/passwd explain this"] {
        let mut a = in_folder();
        let effects = ask(&mut a, text);
        assert!(has_send(&effects), "{text}: {effects:?}");
        assert!(probes(&effects).is_empty());
    }
}

#[test]
fn a_double_slash_cd_is_sent_as_cd() {
    let mut a = in_folder();
    let effects = ask(&mut a, "//cd /w/b");
    assert!(has_send(&effects));
    assert!(probes(&effects).is_empty());
    let last = super::run_pane::chat(&a, id2()).messages().last().cloned();
    assert_eq!(last.map(|m| m.text), Some("/cd /w/b".to_owned()));
}

#[test]
fn cd_works_while_the_engine_state_is_unknown() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "c");
    let effects = ask(&mut a, "/cd /w/b");
    assert_eq!(probes(&effects).len(), 1, "{effects:?}");
}

#[test]
fn a_busy_chat_refuses_cd_with_the_reason_and_keeps_the_text() {
    let (mut a, turn) = run_live();
    announce(&mut a, turn, 1, run("cd ~/dev"));
    type_text(&mut a, "/cd /w/b");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(probes(&effects).is_empty());
    assert!(lines(&a).contains(&"/cd refused: a command is running".to_owned()));
    assert_eq!(composer(&a), "/cd /w/b");
}

#[test]
fn a_streaming_chat_refuses_cd() {
    let mut a = in_folder();
    ask(&mut a, "again");
    type_text(&mut a, "/cd /w/b");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(probes(&effects).is_empty());
    assert!(lines(&a).contains(&"/cd refused: the model is answering".to_owned()));
}

#[test]
fn a_refused_probe_shows_its_reason_and_keeps_the_text() {
    for (refusal, text) in [
        (Refusal::NotFound, "folder not found"),
        (Refusal::NotAFolder, "not a folder"),
        (Refusal::NotAllowed, "not allowed"),
        (Refusal::Root, "/ is not a valid chat folder"),
    ] {
        let mut a = in_folder();
        ask(&mut a, "/cd /x");
        let effects = probed(&mut a, Err(refusal));
        assert!(effects.is_empty());
        assert!(
            lines(&a).contains(&format!("/cd refused: {text}")),
            "{:?}",
            lines(&a)
        );
        assert_eq!(composer(&a), "/cd /x");
        assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    }
}

#[test]
fn a_typed_outside_folder_is_not_allowed() {
    let mut a = in_folder();
    ask(&mut a, "/cd /usr");
    probed(
        &mut a,
        Ok(Probed {
            path: "/usr".into(),
            verdict: Verdict::Outside,
        }),
    );
    assert!(lines(&a).contains(&"/cd refused: not allowed".to_owned()));
}

#[test]
fn the_same_folder_says_so_and_opens_nothing() {
    let mut a = in_folder();
    ask(&mut a, "/cd /w/a/");
    probed(&mut a, concrete("/w/a"));
    assert!(lines(&a).contains(&"already in /w/a".to_owned()));
    assert!(
        super::run_pane::chat(&a, id2())
            .approvals()
            .pending()
            .is_none()
    );
}

// ---- the confirmation (chat-folder A2) -------------------------------------

use crate::core::ai::chat::approval::ApprovalAsk;

/// `/cd /w/b` typed and probed: the confirmation is open.
fn asking() -> App {
    let mut a = in_folder();
    ask(&mut a, "/cd /w/b");
    probed(&mut a, concrete("/w/b"));
    a
}

fn pending_ask(a: &App) -> Option<ApprovalAsk> {
    super::run_pane::chat(a, id2())
        .approvals()
        .pending()
        .map(|p| p.ask().clone())
}

#[test]
fn a_valid_cd_opens_the_confirmation_with_both_folders() {
    let a = asking();
    let Some(ApprovalAsk::Folder { from, to, note }) = pending_ask(&a) else {
        panic!("expected the folder ask, got {:?}", pending_ask(&a));
    };
    assert_eq!((from.as_str(), to), ("/w/a", PathBuf::from("/w/b")));
    assert_eq!(note, None);
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn a_broad_folder_typed_by_the_user_is_confirmed_with_a_note() {
    let mut a = in_folder();
    a.home = Some("/h".into());
    ask(&mut a, "/cd ~");
    probed(
        &mut a,
        Ok(Probed {
            path: "/h".into(),
            verdict: Verdict::Home,
        }),
    );
    let Some(ApprovalAsk::Folder { note, .. }) = pending_ask(&a) else {
        panic!("expected the folder ask");
    };
    assert!(note.is_some_and(|n| n.contains("broad")));
}

#[test]
fn y_changes_the_folder_and_clears_the_composer() {
    let mut a = asking();
    a.update(key('y'));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/b")));
    assert_eq!(composer(&a), "");
    assert_eq!(pending_ask(&a), None);
    assert!(lines(&a).contains(&"folder changed: /w/a -> /w/b".to_owned()));
}

#[test]
fn every_other_key_cancels_and_keeps_the_text() {
    let keys = [
        plain(KeyCode::Char('n')),
        plain(KeyCode::Esc),
        plain(KeyCode::Char('x')),
        code(KeyCode::Char('Y'), KeyModifiers::SHIFT),
        plain(KeyCode::Enter),
    ];
    for event in keys {
        // The Enter that sent `/cd` must not read as the same key held.
        let mut a = asking().with_clock(mock_now);
        let_go();
        a.update(event.clone());
        assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")), "{event:?}");
        assert_eq!(composer(&a), "/cd /w/b", "{event:?}");
        assert_eq!(pending_ask(&a), None, "{event:?}");
        assert!(
            !lines(&a).iter().any(|l| l.ends_with("-> /w/b")),
            "{event:?}"
        );
    }
}

#[test]
fn a_modified_y_does_not_confirm() {
    let mut a = asking();
    a.update(code(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn the_chat_becoming_busy_before_y_refuses_the_change() {
    let mut a = asking();
    let ready = crate::core::ai::chat::engine::Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    // A stream started elsewhere while the prompt waited.
    let chat = a
        .panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap();
    let pending = chat.approvals().pending().unwrap().id();
    chat.approvals_mut().withdraw(pending);
    chat.send("again", &ready, "system");
    chat.approvals_mut()
        .request(
            ApprovalAsk::Folder {
                from: "/w/a".into(),
                to: "/w/b".into(),
                note: None,
            },
            crate::core::ai::chat::approval::Origin::Block,
        )
        .unwrap();
    a.update(key('y'));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    assert!(lines(&a).contains(&"/cd refused: the model is answering".to_owned()));
}

#[test]
fn the_confirmation_shows_the_old_and_the_new_folder() {
    use ratatui::{Terminal, backend::TestBackend};
    let theme = crate::ui::theme::LazaroboxTheme::default();
    let a = asking();
    let prompt = a.approval_prompt(id2());
    let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            let chat = super::run_pane::chat(&a, id2());
            frame.render_widget(
                crate::ui::components::chat::ChatView::new(chat, &theme).approval(prompt.as_ref()),
                area,
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let shot = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(shot.contains("/w/a") && shot.contains("/w/b"), "{shot}");
    insta::assert_snapshot!("folder_confirmation_60x10", shot);
}

// ---- what a confirmed change reaches (chat-folder A2) ----------------------

fn requests(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ChatSend { request, .. } => Some(request.system.clone()),
            _ => None,
        })
        .collect()
}

/// `in_folder` with history on and the first exchange stored as 30.
fn stored() -> App {
    let mut a = super::chat_history::conversation();
    let first = ask(&mut a, "hello");
    let turn = super::chat_history::sent_turn(&first);
    a.update(super::chat_history::created(id2(), 30));
    a.update(super::chat_history::delta(id2(), turn, "hi"));
    a.update(super::chat_history::finish(id2(), turn));
    a.apply_folder(id2(), "/w/a".into());
    a.update(super::chat_history::prompt_for(id2()));
    a
}

fn confirm(a: &mut App, target: &str) -> Vec<Effect> {
    ask(a, &format!("/cd {target}"));
    probed(a, concrete(target));
    a.update(key('y'))
}

#[test]
fn a_confirmed_change_stores_the_folder_of_the_conversation() {
    let mut a = stored();
    let effects = confirm(&mut a, "/w/b");
    let saved: Vec<_> = super::chat_history::stores(&effects);
    assert!(
        saved.contains(&StoreCmd::SetFolder {
            pane: id2(),
            conversation: Some(30),
            folder: "/w/b".into(),
        }),
        "{saved:?}"
    );
}

#[test]
fn a_chat_not_stored_yet_sends_no_set_folder() {
    let mut a = in_folder();
    let effects = a.apply_folder(id2(), "/w/b".into());
    assert!(super::chat_history::stores(&effects).is_empty());
}

#[test]
fn the_next_send_carries_the_notice_once_and_the_conversation_goes_on() {
    let mut a = stored();
    let before = messages(&a);
    confirm(&mut a, "/w/b");
    a.update(super::chat_history::prompt_for(id2()));
    let first = ask(&mut a, "one");
    let sent = requests(&first);
    assert!(sent[0].contains("changed from /w/a to /w/b"), "{}", sent[0]);
    let turn = super::chat_history::sent_turn(&first);
    a.update(super::chat_history::delta(id2(), turn, "ok"));
    a.update(super::chat_history::finish(id2(), turn));
    let second = ask(&mut a, "two");
    assert!(!requests(&second)[0].contains("changed from"));
    let turn = super::chat_history::sent_turn(&second);
    a.update(super::chat_history::delta(id2(), turn, "ok"));
    a.update(super::chat_history::finish(id2(), turn));
    // Four more messages in the same conversation, and no second `Create`.
    assert_eq!(messages(&a), before + 4);
    assert!(
        !super::chat_history::stores(&first)
            .iter()
            .any(|c| matches!(c, StoreCmd::Create { .. }))
    );
}

#[test]
fn a_pane_opened_from_the_chat_starts_in_the_new_folder() {
    let mut a = in_folder();
    a.apply_folder(id2(), "/w/b".into());
    let effects = a.new_tab();
    let cwds: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::SpawnPane(_, spec) => Some(spec.cwd.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cwds, vec![PathBuf::from("/w/b")]);
}

#[test]
fn nothing_the_model_can_call_changes_the_folder() {
    let tools = crate::core::ai::tools::registry(&[]);
    assert!(!tools.is_empty());
    assert!(
        !tools
            .iter()
            .any(|t| t.name == "cd" || t.name.contains("folder"))
    );
}

// ---- resume (chat-folder A2) -----------------------------------------------

/// The resume of conversation 30 with `folder` stored.
fn resumed_in(folder: Option<&str>) -> AppEvent {
    resumed_for(30, folder)
}

fn resumed_for(conversation_id: i64, folder: Option<&str>) -> AppEvent {
    let AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        session,
        ..
    }) = super::chat_history::resumed(id2(), conversation_id, "programmer")
    else {
        unreachable!("resumed builds a Resumed reply");
    };
    AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile,
        tag,
        extras,
        messages,
        session,
        folder: folder.map(PathBuf::from),
    })
}

/// The token of the resume probe the chat waits for.
fn resume_token(a: &App) -> u64 {
    a.resume_folders.get(&id2()).map_or(0, |entry| entry.0)
}

fn resume_probed(a: &mut App, result: Result<Probed, Refusal>) -> Vec<Effect> {
    let token = resume_token(a);
    resume_answer(a, token, result)
}

fn resume_answer(a: &mut App, token: u64, result: Result<Probed, Refusal>) -> Vec<Effect> {
    a.update(AppEvent::FolderProbed {
        pane: id2(),
        purpose: Purpose::Resume(token),
        result,
    })
}

fn resume_purposes(effects: &[Effect]) -> Vec<u64> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ProbeFolder {
                purpose: Purpose::Resume(token),
                ..
            } => Some(*token),
            _ => None,
        })
        .collect()
}

fn probed_as(path: &str, verdict: Verdict) -> Result<Probed, Refusal> {
    Ok(Probed {
        path: path.into(),
        verdict,
    })
}

/// A chat on its list whose folder is `/w/a`, ready to resume.
fn resuming() -> App {
    let mut a = super::chat_history::listing();
    a.apply_folder(id2(), "/w/a".into());
    a.update(super::chat_history::prompt_for(id2()));
    a
}

#[test]
fn a_stored_folder_is_probed_when_the_conversation_is_resumed() {
    let mut a = resuming();
    let effects = a.update(resumed_in(Some("/w/stored")));
    assert_eq!(
        probes(&effects),
        vec![(
            PathBuf::from("/w/stored"),
            Purpose::Resume(resume_token(&a))
        )]
    );
    // Nothing changes until the probe answers.
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn a_concrete_stored_folder_is_set_without_asking_and_is_not_written_again() {
    let mut a = resuming();
    a.update(resumed_in(Some("/w/stored")));
    let effects = resume_probed(&mut a, probed_as("/w/stored", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/stored")));
    assert_eq!(pending_ask(&a), None);
    assert!(super::chat_history::stores(&effects).is_empty());
}

#[test]
fn a_stored_folder_equal_to_the_chat_folder_changes_nothing() {
    let mut a = resuming();
    a.update(resumed_in(Some("/w/a")));
    let effects = resume_probed(&mut a, probed_as("/w/a", Verdict::Concrete));
    assert!(effects.is_empty());
    // Only the move into `/w/a` the test made to start with.
    let moves = lines(&a)
        .iter()
        .filter(|l| l.starts_with("folder changed"))
        .count();
    assert_eq!(moves, 1);
}

#[test]
fn a_conversation_without_a_stored_folder_says_so_and_keeps_the_folder() {
    let mut a = resuming();
    let effects = a.update(resumed_in(None));
    assert!(probes(&effects).is_empty());
    assert!(lines(&a).contains(&"no stored folder".to_owned()));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn a_stored_folder_that_cannot_be_used_says_so_and_keeps_the_folder() {
    let cases = [
        Err(Refusal::NotFound),
        Err(Refusal::NotAllowed),
        probed_as("/usr", Verdict::Outside),
        probed_as("/h", Verdict::Home),
        probed_as("/home", Verdict::AboveHome),
    ];
    for result in cases {
        let mut a = resuming();
        a.update(resumed_in(Some("/w/stored")));
        resume_probed(&mut a, result.clone());
        assert!(
            lines(&a).contains(&"stored folder not available: /w/stored".to_owned()),
            "{result:?}: {:?}",
            lines(&a)
        );
        assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")), "{result:?}");
    }
}

#[test]
fn an_explicit_home_is_refused_on_every_resume() {
    for _ in 0..2 {
        let mut a = resuming();
        a.update(resumed_in(Some("/h")));
        resume_probed(&mut a, probed_as("/h", Verdict::Home));
        assert!(lines(&a).contains(&"stored folder not available: /h".to_owned()));
        assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    }
}

#[test]
fn a_busy_chat_keeps_its_folder_when_the_probe_answers() {
    let mut a = resuming();
    a.update(resumed_in(Some("/w/stored")));
    let ready = crate::core::ai::chat::engine::Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .send("go", &ready, "system");
    resume_probed(&mut a, probed_as("/w/stored", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    assert!(lines(&a).contains(&"stored folder not used: the model is answering".to_owned()));
}

#[test]
fn a_probe_answer_for_another_conversation_is_ignored() {
    let mut a = resuming();
    a.update(resumed_in(Some("/w/stored")));
    // The user went on to another conversation before the answer came.
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .open_list();
    a.update(super::chat_history::resumed(id2(), 20, "programmer"));
    resume_probed(&mut a, probed_as("/w/stored", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn resume_answers_out_of_order_never_cross_conversations() {
    let mut a = resuming();
    let first = a.update(resumed_for(30, Some("/w/one")));
    a.panes
        .get_mut(&id2())
        .and_then(PaneState::chat_mut)
        .unwrap()
        .open_list();
    let second = a.update(resumed_for(20, Some("/w/two")));
    let (one, two) = (resume_purposes(&first)[0], resume_purposes(&second)[0]);
    assert_ne!(one, two);
    // The answer of the first resume comes while the second waits.
    resume_answer(&mut a, one, probed_as("/w/one", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
    resume_answer(&mut a, two, probed_as("/w/two", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/two")));
}

#[test]
fn a_late_resume_answer_does_not_undo_a_cd_the_user_confirmed() {
    let mut a = resuming();
    let effects = a.update(resumed_in(Some("/w/stored")));
    let token = resume_purposes(&effects)[0];
    ask(&mut a, "/cd /w/b");
    probed(&mut a, concrete("/w/b"));
    a.update(key('y'));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/b")));
    resume_answer(&mut a, token, probed_as("/w/stored", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/b")));
}

#[test]
fn a_late_resume_answer_does_not_beat_a_cd_being_typed() {
    let mut a = resuming();
    let effects = a.update(resumed_in(Some("/w/stored")));
    let token = resume_purposes(&effects)[0];
    ask(&mut a, "/cd /w/b");
    resume_answer(&mut a, token, probed_as("/w/stored", Verdict::Concrete));
    assert_eq!(folder_of(&a), Some(PathBuf::from("/w/a")));
}

#[test]
fn an_answer_for_a_command_edited_since_is_dropped() {
    let mut a = in_folder();
    let effects = ask(&mut a, "/cd proj");
    let token = cd_purposes(&effects)[0];
    type_text(&mut a, "y");
    probed_for(&mut a, token, concrete("/w/a/proj"));
    assert_eq!(pending_ask(&a), None);
    assert!(!lines(&a).iter().any(|l| l.starts_with("already in")));
}

#[test]
fn only_the_latest_of_two_cd_commands_is_answered() {
    let mut a = in_folder().with_clock(mock_now);
    let first = cd_purposes(&ask(&mut a, "/cd /w/x"))[0];
    let_go();
    let second = cd_purposes(&a.update(plain(KeyCode::Enter)))[0];
    assert_ne!(first, second);
    probed_for(&mut a, first, concrete("/w/x"));
    assert_eq!(pending_ask(&a), None);
    probed_for(&mut a, second, concrete("/w/x"));
    assert!(pending_ask(&a).is_some());
}
