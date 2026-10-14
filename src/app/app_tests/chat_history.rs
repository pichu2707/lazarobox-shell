//! Chat history in the app (ai-chat C9): the list a chat opens on, what is
//! saved and when, resume and delete.

use super::*;
use crate::core::{
    ai::{
        EngineState, Status,
        chat::{
            Entry, Msg, MsgStatus,
            engine::{ChatError, Finish, Role, StreamEvent},
            list::{DELETE_FAILED, OPEN_ELSEWHERE, RESUME_FAILED},
            prompts::Resolved,
            store::{
                ConvRow, DELETED_LINE, NoteRow, SAVE_FAILED_LINE, StoreCmd, StoreReply,
                UNAVAILABLE_PREFIX,
            },
        },
        config::{Assignment, Profile, Task},
    },
    layout::PaneId,
};

fn chat_id() -> PaneId {
    id2()
}

fn second_chat() -> PaneId {
    PaneId::for_test(3)
}

fn chat_at(a: &App, id: PaneId) -> &ChatState {
    match a.pane_view(id) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane {id:?} is not a chat"),
    }
}

fn chat_of(a: &App) -> &ChatState {
    chat_at(a, chat_id())
}

fn code(code: KeyCode, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, mods))
}

fn plain(code_: KeyCode) -> AppEvent {
    code(code_, KeyModifiers::NONE)
}

fn probe(_: &Path) -> Option<String> {
    Some("proj".into())
}

fn no_project(_: &Path) -> Option<String> {
    None
}

fn ollama() -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Chat,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("llama3:8b".into()),
        },
    );
    ai
}

fn detection() -> Detection {
    Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned()],
            version: None,
        }],
    }
}

fn prompt_for(id: PaneId) -> AppEvent {
    AppEvent::PromptReady(
        id,
        Resolved {
            text: "system prompt".to_owned(),
            notices: Vec::new(),
        },
    )
}

/// A launched app with history on and a ready ollama engine, before any chat.
fn history_app() -> App {
    let mut a = launched(120, 30).with_ai(ollama()).with_chat_history(probe);
    a.update(AppEvent::EnginesDetected(detection()));
    a
}

/// `Prefix c` on `history_app`: the chat pane (id 2) shows its list, which
/// has not been answered yet.
fn opened() -> (App, Vec<Effect>) {
    let mut a = history_app();
    let effects = press(&mut a, "c");
    assert_eq!(a.focused(), chat_id());
    (a, effects)
}

fn row(id: i64, updated_at: i64, tag: Option<&str>, profile: &str, first: &str) -> ConvRow {
    ConvRow {
        id,
        updated_at,
        profile: profile.into(),
        tag: tag.map(str::to_owned),
        first: first.into(),
    }
}

fn rows() -> Vec<ConvRow> {
    vec![
        row(30, 3_000, Some("proj"), "programmer", "newest"),
        row(20, 2_000, Some("other"), "programmer", "elsewhere"),
        row(10, 1_000, Some("proj"), "analyst", "oldest"),
    ]
}

fn listed(a: &mut App, id: PaneId, rows: Vec<ConvRow>) -> Vec<Effect> {
    a.update(AppEvent::Store(StoreReply::Listed { pane: id, rows }))
}

/// A chat pane with its list answered.
fn listing() -> App {
    let (mut a, _) = opened();
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), rows());
    a
}

/// A chat in the conversation view: the list closed with `n`.
fn conversation() -> App {
    let mut a = listing();
    a.update(key('n'));
    a.take_dirty();
    a
}

fn type_text(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
}

fn stores(effects: &[Effect]) -> Vec<StoreCmd> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Store(cmd) => Some(cmd.clone()),
            _ => None,
        })
        .collect()
}

fn sent_turn(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::ChatSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a ChatSend")
}

fn ask(a: &mut App, question: &str) -> Vec<Effect> {
    type_text(a, question);
    a.update(plain(KeyCode::Enter))
}

fn created(id: PaneId, conversation: i64) -> AppEvent {
    AppEvent::Store(StoreReply::Created {
        pane: id,
        conversation,
        evicted: false,
        evicted_notes: 0,
    })
}

fn delta(id: PaneId, turn: u64, text: &str) -> AppEvent {
    AppEvent::Chat(id, turn, StreamEvent::Delta(text.into()))
}

fn finish(id: PaneId, turn: u64) -> AppEvent {
    AppEvent::Chat(id, turn, StreamEvent::Done(Finish::Stop))
}

fn systems(chat: &ChatState) -> Vec<String> {
    chat.entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// ---- opening ---------------------------------------------------------------

// Spec (Conversation list): opening a chat pane shows the list first.
#[test]
fn a_chat_opens_on_the_list_and_asks_the_store_for_it() {
    let (a, effects) = opened();
    assert!(chat_of(&a).list().is_some());
    assert_eq!(chat_of(&a).list().unwrap().project(), Some("proj"));
    assert_eq!(chat_of(&a).tag(), Some("proj"));
    assert_eq!(stores(&effects), vec![StoreCmd::List { pane: chat_id() }]);
}

#[test]
fn without_history_a_chat_opens_on_the_conversation() {
    let mut a = launched(120, 30).with_ai(ollama());
    let effects = press(&mut a, "c");
    assert!(chat_of(&a).list().is_none());
    assert_eq!(stores(&effects), Vec::new());
}

// Spec: No project.
#[test]
fn an_origin_without_a_project_lists_everything() {
    let mut a = launched(120, 30)
        .with_ai(ollama())
        .with_chat_history(no_project);
    press(&mut a, "c");
    assert_eq!(chat_of(&a).tag(), None);
    assert_eq!(chat_of(&a).list().unwrap().project(), None);
}

#[test]
fn the_new_conversation_takes_the_current_profile() {
    let mut ai = ollama();
    ai.set_profile(Profile::Analyst);
    let mut a = launched(120, 30).with_ai(ai).with_chat_history(probe);
    press(&mut a, "c");
    assert_eq!(chat_of(&a).profile(), Profile::Analyst);
}

#[test]
fn the_list_rows_arrive_with_the_store_reply() {
    let (mut a, _) = opened();
    assert!(!chat_of(&a).list().unwrap().loaded());
    listed(&mut a, chat_id(), rows());
    let list = chat_of(&a).list().unwrap();
    assert_eq!(
        list.visible().iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![30, 10]
    );
    assert!(a.take_dirty());
}

#[test]
fn a_list_reply_for_another_pane_changes_nothing() {
    let (mut a, _) = opened();
    listed(&mut a, PaneId::for_test(9), rows());
    assert!(!chat_of(&a).list().unwrap().loaded());
}

// ---- keys in the list ------------------------------------------------------

// Spec modal-input: Keys in the list view.
#[test]
fn list_keys_move_and_never_type_into_the_composer_or_a_pty() {
    let mut a = listing();
    let mut effects = Vec::new();
    for ev in [key('j'), key('x'), key('k'), ctrl('r'), plain(KeyCode::Tab)] {
        effects.extend(a.update(ev));
    }
    assert_eq!(effects, Vec::new());
    assert_eq!(chat_of(&a).composer().text(), "");
    assert!(chat_of(&a).list().is_some());
}

fn ctrl(c: char) -> AppEvent {
    code(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn ctrl_c_in_the_list_never_quits() {
    let mut a = listing();
    let effects = a.update(ctrl('c'));
    assert!(!effects.contains(&Effect::Quit));
    assert!(chat_of(&a).list().is_some());
}

#[test]
fn a_paste_into_the_list_goes_nowhere() {
    let mut a = listing();
    a.update(AppEvent::Paste("pasted".into()));
    assert_eq!(chat_of(&a).composer().text(), "");
}

// Spec: New conversation.
#[test]
fn n_and_esc_open_the_conversation_and_store_nothing() {
    for ev in [key('n'), plain(KeyCode::Esc)] {
        let mut a = listing();
        let effects = a.update(ev);
        assert!(chat_of(&a).list().is_none());
        assert_eq!(stores(&effects), Vec::new());
        assert_eq!(chat_of(&a).tag(), Some("proj"), "the proposed tag stays");
    }
}

#[test]
fn after_n_the_keys_edit_the_composer() {
    let mut a = conversation();
    type_text(&mut a, "hi");
    assert_eq!(chat_of(&a).composer().text(), "hi");
}

// ---- saving ----------------------------------------------------------------

// Spec: Created on first send.
#[test]
fn the_first_send_creates_the_conversation_before_the_engine_starts() {
    let mut a = conversation();
    let effects = ask(&mut a, "hello");
    let [
        Effect::Store(StoreCmd::Create {
            pane,
            profile,
            tag,
            first,
            open,
        }),
        Effect::ChatSend { .. },
    ] = effects.as_slice()
    else {
        panic!("expected Create then ChatSend, got {effects:?}");
    };
    assert_eq!(*pane, chat_id());
    assert_eq!(profile, "programmer");
    assert_eq!(tag.as_deref(), Some("proj"));
    assert_eq!(
        (first.role, first.text.as_str(), first.status),
        (Role::User, "hello", MsgStatus::Complete)
    );
    assert_eq!(
        (first.engine.as_str(), first.model.as_deref()),
        ("ollama", Some("llama3:8b"))
    );
    assert_eq!(*open, Vec::<i64>::new());
}

#[test]
fn a_second_send_appends_once_the_conversation_exists() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 5));
    a.update(delta(chat_id(), turn, "answer"));
    let done = a.update(finish(chat_id(), turn));
    let saved = stores(&done);
    assert_eq!(saved.len(), 1);
    let StoreCmd::Append {
        conversation,
        message,
        ..
    } = &saved[0]
    else {
        panic!("expected Append, got {saved:?}");
    };
    assert_eq!(*conversation, 5);
    assert_eq!(
        (message.role, message.text.as_str(), message.status),
        (Role::Assistant, "answer", MsgStatus::Complete)
    );
    let next = ask(&mut a, "two");
    assert!(matches!(
        stores(&next).as_slice(),
        [StoreCmd::Append {
            conversation: 5,
            ..
        }]
    ));
}

#[test]
fn an_answer_that_ends_before_created_is_released_by_it() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(delta(chat_id(), turn, "A"));
    let done = a.update(finish(chat_id(), turn));
    assert_eq!(stores(&done), Vec::new(), "waits for the id");
    let released = a.update(created(chat_id(), 8));
    assert!(matches!(
        stores(&released).as_slice(),
        [StoreCmd::Append {
            conversation: 8,
            ..
        }]
    ));
}

// Spec: Partial saved on cancel.
#[test]
fn esc_saves_the_partial_answer_cancelled() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 5));
    a.update(delta(chat_id(), turn, "Hel"));
    let effects = a.update(plain(KeyCode::Esc));
    let saved = stores(&effects);
    let [StoreCmd::Append { message, .. }] = saved.as_slice() else {
        panic!("expected one Append, got {saved:?}");
    };
    assert_eq!(
        (message.text.as_str(), message.status),
        ("Hel", MsgStatus::Cancelled)
    );
}

// Spec: Error lines not stored.
#[test]
fn an_error_turn_stores_the_user_message_and_no_error_text() {
    let mut a = conversation();
    let first = ask(&mut a, "one");
    let turn = sent_turn(&first);
    let failed = a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("connection refused".into())),
    ));
    let everything = format!("{first:?}{failed:?}");
    assert_eq!(stores(&failed), Vec::new());
    assert!(!format!("{:?}", stores(&first)).contains("connection refused"));
    assert!(everything.contains("Create"));
}

// Spec: Reasoning never reaches the store.
#[test]
fn reasoning_never_leaves_the_app_in_a_store_command() {
    let mut a = conversation();
    let first = ask(&mut a, "q");
    let turn = sent_turn(&first);
    let mut effects = first;
    effects.extend(a.update(created(chat_id(), 7)));
    effects.extend(a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("secret plan".into()),
    )));
    effects.extend(a.update(delta(chat_id(), turn, "Hi")));
    effects.extend(a.update(finish(chat_id(), turn)));
    let saved = stores(&effects);
    assert!(!format!("{saved:?}").contains("secret plan"));
    let texts: Vec<&str> = saved
        .iter()
        .map(|c| match c {
            StoreCmd::Create { first, .. } => first.text.as_str(),
            StoreCmd::Append { message, .. } => message.text.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(texts, vec!["q", "Hi"]);
}

// Spec: Reasoning only, then cancel.
#[test]
fn a_turn_with_only_reasoning_saves_no_assistant_message() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "q"));
    a.update(created(chat_id(), 7));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::ThinkingDelta("secret plan".into()),
    ));
    let effects = a.update(plain(KeyCode::Esc));
    assert_eq!(stores(&effects), Vec::new());
}

// Spec (Retention): the conversations open in panes are never evicted.
#[test]
fn a_new_conversation_lists_the_ones_open_in_other_panes() {
    let mut a = conversation();
    ask(&mut a, "one");
    a.update(created(chat_id(), 5));
    // A second chat pane, in its own conversation view.
    let mut b = a;
    press(&mut b, "c");
    assert_eq!(b.focused(), second_chat());
    b.update(prompt_for(second_chat()));
    b.update(key('n'));
    let effects = ask(&mut b, "other");
    let saved = stores(&effects);
    let [StoreCmd::Create { open, .. }] = saved.as_slice() else {
        panic!("expected one Create, got {effects:?}");
    };
    assert_eq!(*open, vec![5]);
}

// Spec (Retention): User is told.
#[test]
fn an_eviction_shows_its_line_in_the_new_conversation() {
    let mut a = conversation();
    ask(&mut a, "one");
    a.update(AppEvent::Store(StoreReply::Created {
        pane: chat_id(),
        conversation: 5,
        evicted: true,
        evicted_notes: 0,
    }));
    assert!(systems(chat_of(&a)).contains(&"oldest conversation removed (limit 200)".to_owned()));
}

// ---- unavailable and failing stores ---------------------------------------

// Spec (Unavailable store): Corrupt file / Cannot open.
#[test]
fn an_unavailable_store_says_so_once_and_the_list_is_empty() {
    let (mut a, _) = opened();
    a.update(prompt_for(chat_id()));
    a.update(AppEvent::Store(StoreReply::Unavailable {
        pane: chat_id(),
        reason: "database is corrupt".into(),
    }));
    let list = chat_of(&a).list().unwrap();
    assert!(list.loaded());
    assert_eq!(list.visible().len(), 0);
    a.update(key('n'));
    let line = format!("{UNAVAILABLE_PREFIX}database is corrupt");
    assert_eq!(
        systems(chat_of(&a)).iter().filter(|l| **l == line).count(),
        1
    );
    // The chat works in memory and nothing is saved.
    let effects = ask(&mut a, "hello");
    assert_eq!(stores(&effects), Vec::new());
    assert!(effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })));
    // A second Unavailable (the create) adds no second line.
    a.update(AppEvent::Store(StoreReply::Unavailable {
        pane: chat_id(),
        reason: "database is corrupt".into(),
    }));
    assert_eq!(
        systems(chat_of(&a)).iter().filter(|l| **l == line).count(),
        1
    );
}

// Spec: Write fails mid-session.
#[test]
fn a_failed_write_says_so_once_per_run_and_never_stops_the_stream() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 5));
    a.update(delta(chat_id(), turn, "A"));
    a.update(finish(chat_id(), turn));
    for notify in [true, false] {
        a.update(AppEvent::Store(StoreReply::Failed {
            pane: chat_id(),
            notify,
        }));
    }
    assert_eq!(
        systems(chat_of(&a))
            .iter()
            .filter(|l| *l == SAVE_FAILED_LINE)
            .count(),
        1
    );
    // The next turn still streams and still tries to save.
    let next = ask(&mut a, "two");
    assert!(next.iter().any(|e| matches!(e, Effect::ChatSend { .. })));
    assert_eq!(stores(&next).len(), 1);
}

// ---- closing ---------------------------------------------------------------

// Spec (Saving conversations): saved when its pane is closed.
#[test]
fn closing_a_streaming_pane_saves_the_partial_answer() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 5));
    a.update(delta(chat_id(), turn, "par"));
    press(&mut a, "wq");
    let effects = a.update(key('y'));
    let saved = stores(&effects);
    let [StoreCmd::Append { message, .. }] = saved.as_slice() else {
        panic!("expected one Append, got {effects:?}");
    };
    assert_eq!(
        (message.text.as_str(), message.status),
        ("par", MsgStatus::Cancelled)
    );
}

#[test]
fn quitting_saves_the_partial_answer_before_the_quit() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 5));
    a.update(delta(chat_id(), turn, "par"));
    press(&mut a, "q");
    let effects = a.update(key('y'));
    let at = |pred: fn(&Effect) -> bool| effects.iter().position(pred);
    let append = at(|e| matches!(e, Effect::Store(StoreCmd::Append { .. })));
    let quit = at(|e| matches!(e, Effect::Quit));
    assert!(append.is_some() && quit.is_some(), "{effects:?}");
    assert!(append < quit, "the save goes before the quit: {effects:?}");
}

// ---- resume ----------------------------------------------------------------

fn stored(role: Role, text: &str, status: MsgStatus) -> Msg {
    Msg {
        role,
        text: text.into(),
        status,
    }
}

fn resumed(pane: PaneId, conversation: i64, profile: &str) -> AppEvent {
    AppEvent::Store(StoreReply::Resumed {
        pane,
        conversation,
        profile: profile.into(),
        tag: Some("proj".into()),
        messages: vec![
            stored(Role::User, "old question", MsgStatus::Complete),
            stored(Role::Assistant, "old answer", MsgStatus::Complete),
            stored(Role::User, "second", MsgStatus::Complete),
            stored(Role::Assistant, "par", MsgStatus::Interrupted),
        ],
    })
}

// Spec (Resume): the request names the row under the cursor.
#[test]
fn enter_asks_the_store_for_the_conversation() {
    let mut a = listing();
    a.update(key('j'));
    let effects = a.update(plain(KeyCode::Enter));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::Resume {
            pane: chat_id(),
            conversation: 10
        }]
    );
    assert!(
        chat_of(&a).list().is_some(),
        "the list stays until the reply"
    );
}

// Spec: Full history restored.
#[test]
fn a_resumed_conversation_shows_every_stored_message_in_order() {
    let mut a = listing();
    a.update(resumed(chat_id(), 30, "programmer"));
    let chat = chat_of(&a);
    assert!(chat.list().is_none());
    let got: Vec<(Role, &str, MsgStatus)> = chat
        .messages()
        .map(|m| (m.role, m.text.as_str(), m.status))
        .collect();
    assert_eq!(
        got,
        vec![
            (Role::User, "old question", MsgStatus::Complete),
            (Role::Assistant, "old answer", MsgStatus::Complete),
            (Role::User, "second", MsgStatus::Complete),
            (Role::Assistant, "par", MsgStatus::Interrupted),
        ]
    );
    assert_eq!(chat.conversation(), Some(30));
    assert!(
        !chat
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::Reasoning(_))),
        "no reasoning is shown after a resume"
    );
}

// Spec: Appends to the same conversation / Uses the current engine.
#[test]
fn a_message_after_a_resume_appends_and_uses_the_current_engine() {
    let mut a = listing();
    a.update(resumed(chat_id(), 30, "programmer"));
    let effects = ask(&mut a, "third");
    let saved = stores(&effects);
    assert!(matches!(
        saved.as_slice(),
        [StoreCmd::Append {
            conversation: 30,
            ..
        }]
    ));
    let Some(Effect::ChatSend { request, .. }) = effects
        .iter()
        .find(|e| matches!(e, Effect::ChatSend { .. }))
    else {
        panic!("no ChatSend in {effects:?}");
    };
    assert_eq!(request.engine, "ollama");
    // Spec: Reasoning is not engine history after resume: stored texts only.
    let history: Vec<&str> = request
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(
        history,
        vec!["old question", "old answer", "second", "par", "third"]
            .into_iter()
            .collect::<Vec<_>>()
    );
}

// Spec: the system prompt uses the stored profile.
#[test]
fn a_stored_profile_other_than_the_current_prepares_the_prompt_again() {
    let mut a = listing();
    let effects = a.update(resumed(chat_id(), 10, "analyst"));
    let [Effect::PreparePrompt { pane, profile, .. }] = effects.as_slice() else {
        panic!("expected one PreparePrompt, got {effects:?}");
    };
    assert_eq!((*pane, *profile), (chat_id(), Profile::Analyst));
    assert_eq!(chat_of(&a).profile(), Profile::Analyst);
    assert_eq!(chat_of(&a).prompt(), None, "the old prompt is dropped");
    // A send waits for the new prompt.
    let waiting = ask(&mut a, "third");
    assert!(!waiting.iter().any(|e| matches!(e, Effect::ChatSend { .. })));
    let ready = a.update(prompt_for(chat_id()));
    assert!(ready.iter().any(|e| matches!(e, Effect::ChatSend { .. })));
}

#[test]
fn the_same_profile_keeps_the_prompt() {
    let mut a = listing();
    let effects = a.update(resumed(chat_id(), 30, "programmer"));
    assert_eq!(effects, Vec::new());
    assert_eq!(chat_of(&a).prompt(), Some("system prompt"));
}

#[test]
fn a_profile_change_while_the_first_prompt_is_in_flight_drops_that_answer() {
    let (mut a, _) = opened();
    listed(&mut a, chat_id(), rows());
    // The resume arrives before the prompt prepared at open.
    let none = a.update(resumed(chat_id(), 10, "analyst"));
    assert_eq!(none, Vec::new());
    let again = a.update(prompt_for(chat_id()));
    let [Effect::PreparePrompt { profile, .. }] = again.as_slice() else {
        panic!("expected a second PreparePrompt, got {again:?}");
    };
    assert_eq!(*profile, Profile::Analyst);
    assert_eq!(chat_of(&a).prompt(), None);
    a.update(prompt_for(chat_id()));
    assert_eq!(chat_of(&a).prompt(), Some("system prompt"));
}

// Spec: Already open.
#[test]
fn a_conversation_open_in_another_pane_is_refused_in_the_list() {
    let mut a = conversation();
    ask(&mut a, "one");
    a.update(created(chat_id(), 30));
    press(&mut a, "c");
    let b = second_chat();
    assert_eq!(a.focused(), b);
    a.update(prompt_for(b));
    listed(&mut a, b, rows());
    let effects = a.update(plain(KeyCode::Enter));
    assert_eq!(stores(&effects), Vec::new());
    assert_eq!(chat_at(&a, b).list().unwrap().line(), Some(OPEN_ELSEWHERE));
    assert!(chat_at(&a, b).list().is_some());
}

#[test]
fn a_resume_that_lost_the_race_is_refused_too() {
    let mut a = conversation();
    ask(&mut a, "one");
    press(&mut a, "c");
    let b = second_chat();
    a.update(prompt_for(b));
    listed(&mut a, b, rows());
    a.update(plain(KeyCode::Enter));
    // Pane A's conversation turns out to be 30 before B's reply lands.
    a.update(created(chat_id(), 30));
    a.update(resumed(b, 30, "programmer"));
    assert!(chat_at(&a, b).list().is_some());
    assert_eq!(chat_at(&a, b).list().unwrap().line(), Some(OPEN_ELSEWHERE));
}

#[test]
fn a_failed_resume_says_so_in_the_list() {
    let mut a = listing();
    a.update(AppEvent::Store(StoreReply::ResumeFailed {
        pane: chat_id(),
    }));
    assert_eq!(chat_of(&a).list().unwrap().line(), Some(RESUME_FAILED));
}

// ---- delete ----------------------------------------------------------------

// Spec (Delete): Delete confirmed.
#[test]
fn d_then_y_deletes_the_conversation_and_the_row_goes() {
    let mut a = listing();
    a.update(key('j'));
    let ask_effects = a.update(key('d'));
    assert_eq!(ask_effects, Vec::new());
    assert!(chat_of(&a).list().unwrap().confirming());
    let effects = a.update(key('y'));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::Delete {
            pane: chat_id(),
            conversation: 10
        }]
    );
    a.update(AppEvent::Store(StoreReply::Deleted {
        pane: chat_id(),
        conversation: 10,
        notes: Vec::<NoteRow>::new(),
    }));
    let list = chat_of(&a).list().unwrap();
    assert_eq!(
        list.visible().iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![30]
    );
}

// Spec: Delete declined.
#[test]
fn any_other_key_declines_the_delete() {
    for ev in [
        key('n'),
        plain(KeyCode::Esc),
        key('x'),
        code(KeyCode::Char('Y'), KeyModifiers::SHIFT),
    ] {
        let mut a = listing();
        a.update(key('d'));
        let effects = a.update(ev);
        assert_eq!(stores(&effects), Vec::new());
        assert!(!chat_of(&a).list().unwrap().confirming());
        assert!(
            chat_of(&a).list().is_some(),
            "declining does not start a chat"
        );
    }
}

// Spec: Deleted while open elsewhere.
#[test]
fn deleting_a_conversation_open_elsewhere_stops_its_pane_saving() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "one"));
    a.update(created(chat_id(), 30));
    a.update(delta(chat_id(), turn, "A"));
    a.update(finish(chat_id(), turn));
    press(&mut a, "c");
    let b = second_chat();
    a.update(prompt_for(b));
    listed(&mut a, b, rows());
    a.update(key('d'));
    a.update(key('y'));
    a.update(AppEvent::Store(StoreReply::Deleted {
        pane: b,
        conversation: 30,
        notes: Vec::new(),
    }));
    assert!(systems(chat_of(&a)).contains(&DELETED_LINE.to_owned()));
    // The first pane still answers, and nothing is stored.
    press(&mut a, "h");
    assert_eq!(a.focused(), chat_id());
    let effects = ask(&mut a, "two");
    assert!(effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })));
    assert_eq!(stores(&effects), Vec::new());
}

// Spec: Delete failure.
#[test]
fn a_failed_delete_keeps_the_row_and_says_so() {
    let mut a = listing();
    a.update(key('d'));
    a.update(key('y'));
    a.update(AppEvent::Store(StoreReply::DeleteFailed {
        pane: chat_id(),
    }));
    let list = chat_of(&a).list().unwrap();
    assert_eq!(list.line(), Some(DELETE_FAILED));
    assert_eq!(list.visible().len(), 2);
}
