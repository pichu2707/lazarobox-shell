//! The tag editor in the app (ai-chat C9, chat-history: Edit tags): `t` on a
//! list row, `Ctrl+Space T` in a conversation, what Enter stores, and how the
//! editor shares the pane with the other views.

use super::chat_history::{
    ask, chat_at, chat_id, chat_of, conversation, created, delta, detection, finish, listed,
    listing, ollama, opened, plain, prompt_for, resumed, row, second_chat, stores, systems,
    type_text,
};
use super::chat_notes::ai;
use super::*;
use crate::core::ai::chat::{
    list::ListView,
    store::{ConvRow, StoreCmd, StoreReply, TAGS_NOT_SAVED_LINE},
    tag_editor::TagTarget,
    tags::Tags,
};

fn ctrl(c: char) -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn tagged_rows() -> Vec<ConvRow> {
    let mut with_extras = row(30, 3_000, Some("proj"), "programmer", "newest");
    with_extras.extras = vec!["sql".into(), "etl".into()];
    vec![
        with_extras,
        row(20, 2_000, Some("other"), "programmer", "elsewhere"),
        row(10, 1_000, Some("proj"), "analyst", "oldest"),
    ]
}

/// A chat showing its list with `tagged_rows`.
fn tag_listing() -> App {
    let (mut a, _) = opened();
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), tagged_rows());
    a
}

fn list_of(a: &App) -> &ListView {
    chat_of(a).list().expect("the list is shown")
}

fn editor_text(a: &App) -> String {
    chat_of(a).tag_editor().expect("an editor").text()
}

fn editor_open(a: &App) -> bool {
    chat_of(a).tag_editor().is_some()
}

/// The visible row ids of the list.
fn ids(a: &App) -> Vec<i64> {
    list_of(a).visible().iter().map(|r| r.id).collect()
}

fn saved(pane: PaneId, conversation: i64, tags: &str) -> AppEvent {
    AppEvent::Store(StoreReply::TagsSaved {
        pane,
        conversation,
        tags: Tags::parse(tags),
        updated_at: 10_000,
    })
}

/// A conversation in its first view whose pane tag is `proj`, nothing sent.
fn fresh() -> App {
    conversation()
}

/// A conversation that sent a message and whose store answered `Created(7)`.
fn stored() -> App {
    let mut a = fresh();
    ask(&mut a, "hello");
    a.update(created(chat_id(), 7));
    a
}

// ---- from the list (`t`) ----------------------------------------------------

// Spec (Edit tags): `t` on a row.
#[test]
fn t_opens_the_editor_on_the_tags_of_the_row() {
    let mut a = tag_listing();
    assert_eq!(a.update(key('t')), Vec::new());
    assert_eq!(editor_text(&a), "proj, sql, etl");
    assert_eq!(
        chat_of(&a).tag_editor().unwrap().target(),
        TagTarget::Row(30)
    );
    assert!(chat_of(&a).list().is_some(), "the list stays underneath");
}

#[test]
fn t_on_the_second_row_edits_that_row() {
    let mut a = tag_listing();
    a.update(key('j'));
    a.update(key('t'));
    assert_eq!(
        chat_of(&a).tag_editor().unwrap().target(),
        TagTarget::Row(10)
    );
    assert_eq!(editor_text(&a), "proj");
}

#[test]
fn the_editor_over_the_list_swallows_the_list_keys() {
    let mut a = tag_listing();
    a.update(key('t'));
    for c in ['d', 'n', 'a', 'j', 'y'] {
        assert_eq!(a.update(key(c)), Vec::new());
    }
    assert!(!list_of(&a).confirming());
    assert!(chat_of(&a).list().is_some());
    assert_eq!(editor_text(&a), "proj, sql, etldnajy");
}

#[test]
fn enter_on_a_row_stores_the_tags_at_once() {
    let mut a = tag_listing();
    a.update(key('t'));
    for _ in 0..3 {
        a.update(plain(KeyCode::Backspace));
    }
    type_text(&mut a, "ml");
    let effects = a.update(plain(KeyCode::Enter));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::SetTags {
            pane: chat_id(),
            conversation: Some(30),
            tags: Tags::parse("proj, sql, ml"),
        }]
    );
    assert!(!editor_open(&a));
    assert!(chat_of(&a).list().is_some());
}

// Spec: the list filter reflects the change.
#[test]
fn the_reply_updates_the_row_and_the_filter() {
    let mut a = tag_listing();
    assert_eq!(ids(&a), vec![30, 10]);
    a.update(key('t'));
    a.update(plain(KeyCode::Enter));
    a.update(saved(chat_id(), 30, "client-a, sql"));
    // `proj` no longer lists it; its tags are the stored ones.
    assert_eq!(ids(&a), vec![10]);
    assert_eq!(list_of(&a).tags_of(30), Some(Tags::parse("client-a, sql")));
    a.update(key('a'));
    assert_eq!(ids(&a), vec![30, 20, 10]);
}

// Spec (Discard): the stored tags are unchanged.
#[test]
fn escape_discards_and_sends_nothing() {
    let mut a = tag_listing();
    a.update(key('t'));
    type_text(&mut a, "zzz");
    let effects = a.update(plain(KeyCode::Esc));
    assert_eq!(effects, Vec::new());
    assert!(!editor_open(&a));
    assert_eq!(list_of(&a).tags_of(30), Some(Tags::parse("proj, sql, etl")));
    // The list is back: `d` asks again for a delete.
    a.update(key('d'));
    assert!(list_of(&a).confirming());
}

#[test]
fn a_saved_edit_moves_the_row_to_the_top_of_the_list() {
    let mut a = tag_listing();
    a.update(AppEvent::Store(StoreReply::TagsSaved {
        pane: chat_id(),
        conversation: 10,
        tags: Tags::parse("proj"),
        updated_at: 9_999,
    }));
    assert_eq!(ids(&a), vec![10, 30]);
}

#[test]
fn a_reply_updates_every_open_list() {
    let mut a = tag_listing();
    let first = chat_id();
    // A second chat with its own list.
    a.update(ctrl_space());
    a.update(key('c'));
    let second = second_chat();
    assert_eq!(a.focused(), second);
    listed(&mut a, second, tagged_rows());
    a.update(saved(first, 30, "client-b"));
    for id in [first, second] {
        assert_eq!(
            chat_at(&a, id).list().unwrap().tags_of(30),
            Some(Tags::parse("client-b")),
            "{id:?}"
        );
    }
}

#[test]
fn a_failed_save_says_so_in_the_list() {
    let mut a = tag_listing();
    a.update(key('t'));
    a.update(plain(KeyCode::Enter));
    a.update(AppEvent::Store(StoreReply::TagsFailed { pane: chat_id() }));
    assert_eq!(list_of(&a).line(), Some("could not save tags"));
    // The row keeps the tags it had.
    assert_eq!(list_of(&a).tags_of(30), Some(Tags::parse("proj, sql, etl")));
}

#[test]
fn a_failed_save_in_a_conversation_is_a_system_line() {
    let mut a = stored();
    press(&mut a, "T");
    a.update(plain(KeyCode::Enter));
    a.update(AppEvent::Store(StoreReply::TagsFailed { pane: chat_id() }));
    assert!(systems(chat_of(&a)).contains(&"could not save tags".to_owned()));
}

#[test]
fn a_resume_closes_the_editor_of_the_list_it_replaces() {
    let mut a = tag_listing();
    a.update(key('t'));
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    assert!(!editor_open(&a));
    assert!(chat_of(&a).list().is_none());
}

#[test]
fn a_delete_of_the_row_being_edited_closes_the_editor() {
    let mut a = tag_listing();
    a.update(key('t'));
    a.update(AppEvent::Store(StoreReply::Deleted {
        pane: second_chat(),
        conversation: 30,
        notes: Vec::new(),
    }));
    assert!(!editor_open(&a));
}

#[test]
fn the_row_editor_survives_the_delete_of_another_row() {
    let mut a = tag_listing();
    a.update(key('t'));
    a.update(AppEvent::Store(StoreReply::Deleted {
        pane: second_chat(),
        conversation: 10,
        notes: Vec::new(),
    }));
    assert!(editor_open(&a));
}

// ---- from a conversation (Ctrl+Space T) ------------------------------------

// Deviation: the spec said Ctrl+T; the key is `Ctrl+Space T` (no new direct
// Ctrl+letter chat keys), because `Ctrl+Space t` is the tab group.
#[test]
fn prefix_t_capital_opens_the_editor_on_the_proposed_tag() {
    let mut a = fresh();
    assert_eq!(press(&mut a, "T"), Vec::new());
    assert_eq!(editor_text(&a), "proj");
    assert_eq!(chat_of(&a).tag_editor().unwrap().target(), TagTarget::Pane);
    assert_eq!(a.input(), InputMode::Terminal);
}

// A terminal that sends ^H for Backspace delivers Ctrl+H.
#[test]
fn ctrl_h_deletes_like_backspace_in_the_tag_editor() {
    let mut a = fresh();
    press(&mut a, "T");
    a.update(ctrl('h'));
    assert_eq!(editor_text(&a), "pro");
}

// A held Enter saves once; the rest of the hold does not send the composer.
#[test]
fn a_held_enter_saves_the_tags_once_and_sends_nothing() {
    NOW_MS.with(|n| n.set(0));
    let mut a = fresh().with_clock(mock_now);
    type_text(&mut a, "draft");
    press(&mut a, "T");
    at(&mut a, 0, plain(KeyCode::Enter));
    assert!(!editor_open(&a));
    for ms in [100, 200] {
        let effects = at(&mut a, ms, plain(KeyCode::Enter));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })),
            "Enter at {ms}: {effects:?}"
        );
    }
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn prefix_lowercase_t_is_still_the_tab_group() {
    let mut a = fresh();
    press(&mut a, "t");
    assert!(!editor_open(&a));
    assert!(matches!(a.input(), InputMode::Group(_)));
}

#[test]
fn a_conversation_not_yet_created_keeps_the_tags_for_its_create() {
    let mut a = fresh();
    press(&mut a, "T");
    type_text(&mut a, ", sql");
    let effects = a.update(plain(KeyCode::Enter));
    // Nothing is stored yet: no row exists.
    assert_eq!(stores(&effects), Vec::new());
    assert!(!editor_open(&a));
    assert_eq!(chat_of(&a).tags(), Tags::parse("proj, sql"));
    let effects = ask(&mut a, "hello");
    let cmds = stores(&effects);
    let [StoreCmd::Create { tag, extras, .. }] = cmds.as_slice() else {
        panic!("expected one Create");
    };
    assert_eq!(tag.as_deref(), Some("proj"));
    assert_eq!(extras, &["sql".to_owned()]);
}

#[test]
fn a_late_list_answer_does_not_undo_an_edited_tag() {
    let mut a = conversation();
    press(&mut a, "T");
    for _ in 0..4 {
        a.update(plain(KeyCode::Backspace));
    }
    type_text(&mut a, "mine");
    a.update(plain(KeyCode::Enter));
    // The store answers the list for the origin, as `conversation` already did
    // once; a second answer must keep the user's tag.
    a.update(AppEvent::Store(StoreReply::Listed {
        pane: chat_id(),
        tag: Some("proj".into()),
        rows: Vec::new(),
    }));
    assert_eq!(chat_of(&a).tag(), Some("mine"));
}

#[test]
fn extras_typed_before_the_list_answers_leave_the_project_tag_to_the_answer() {
    let (mut a, _) = opened();
    a.update(key('n'));
    press(&mut a, "T");
    type_text(&mut a, ", sql");
    a.update(plain(KeyCode::Enter));
    assert_eq!(chat_of(&a).tag(), None);
    a.update(AppEvent::Store(StoreReply::Listed {
        pane: chat_id(),
        tag: Some("proj".into()),
        rows: Vec::new(),
    }));
    assert_eq!(chat_of(&a).tags(), Tags::parse("proj, sql"));
}

#[test]
fn a_detached_conversation_says_the_tags_stay_in_the_pane() {
    let mut a = stored();
    a.update(AppEvent::Store(StoreReply::Unavailable {
        pane: chat_id(),
        reason: "disk gone".into(),
    }));
    press(&mut a, "T");
    type_text(&mut a, ", sql");
    let effects = a.update(plain(KeyCode::Enter));
    assert_eq!(stores(&effects), Vec::new());
    assert!(systems(chat_of(&a)).contains(&TAGS_NOT_SAVED_LINE.to_owned()));
    assert_eq!(chat_of(&a).tags(), Tags::parse("proj, sql"));
}

#[test]
fn a_fresh_or_stored_conversation_does_not_say_it_is_not_saving() {
    let mut a = fresh();
    press(&mut a, "T");
    a.update(plain(KeyCode::Enter));
    assert!(!systems(chat_of(&a)).contains(&TAGS_NOT_SAVED_LINE.to_owned()));
    let mut b = stored();
    press(&mut b, "T");
    b.update(plain(KeyCode::Enter));
    assert!(!systems(chat_of(&b)).contains(&TAGS_NOT_SAVED_LINE.to_owned()));
}

#[test]
fn a_removed_project_tag_is_not_proposed_again_by_the_store() {
    let mut a = fresh();
    press(&mut a, "T");
    a.update(ctrl('u'));
    a.update(plain(KeyCode::Enter));
    let effects = ask(&mut a, "hello");
    let cmds = stores(&effects);
    let [StoreCmd::Create { tag, origin, .. }] = cmds.as_slice() else {
        panic!("expected one Create");
    };
    assert_eq!((tag, origin), (&None, &None));
}

#[test]
fn enter_in_a_stored_conversation_stores_the_tags_at_once() {
    let mut a = stored();
    press(&mut a, "T");
    type_text(&mut a, ", sql");
    let effects = a.update(plain(KeyCode::Enter));
    assert_eq!(
        stores(&effects),
        vec![StoreCmd::SetTags {
            pane: chat_id(),
            conversation: Some(7),
            tags: Tags::parse("proj, sql"),
        }]
    );
    assert_eq!(chat_of(&a).tags(), Tags::parse("proj, sql"));
}

#[test]
fn a_conversation_being_created_names_only_its_pane() {
    let mut a = fresh();
    ask(&mut a, "hello");
    press(&mut a, "T");
    type_text(&mut a, ", sql");
    let effects = a.update(plain(KeyCode::Enter));
    let cmds = stores(&effects);
    let [StoreCmd::SetTags { conversation, .. }] = cmds.as_slice() else {
        panic!("expected one SetTags");
    };
    assert_eq!(*conversation, None);
}

#[test]
fn the_reply_reaches_the_pane_that_has_the_conversation_open() {
    let mut a = tag_listing();
    a.update_settled(resumed(chat_id(), 30, "programmer"));
    // A second chat edits the same conversation from its list.
    a.update(ctrl_space());
    a.update(key('c'));
    let other = second_chat();
    listed(&mut a, other, tagged_rows());
    a.update(saved(other, 30, "client, sql"));
    assert_eq!(chat_at(&a, chat_id()).tags(), Tags::parse("client, sql"));
    assert_eq!(
        chat_at(&a, other).list().unwrap().tags_of(30),
        Some(Tags::parse("client, sql"))
    );
}

#[test]
fn a_resumed_conversation_opens_the_editor_on_its_stored_tags() {
    let mut a = listing();
    a.update_settled(AppEvent::Store(StoreReply::Resumed {
        pane: chat_id(),
        conversation: 30,
        profile: "programmer".into(),
        tag: Some("proj".into()),
        extras: vec!["sql".into()],
        messages: Vec::new(),
        session: None,
        folder: None,
    }));
    press(&mut a, "T");
    assert_eq!(editor_text(&a), "proj, sql");
}

#[test]
fn a_streaming_conversation_can_be_tagged_and_keeps_streaming() {
    let mut a = fresh();
    let turn = {
        let effects = ask(&mut a, "hello");
        super::chat_history::sent_turn(&effects)
    };
    a.update(created(chat_id(), 7));
    press(&mut a, "T");
    // The stream goes on under the editor.
    a.update(delta(chat_id(), turn, "Hel"));
    a.update(plain(KeyCode::Enter));
    a.update(finish(chat_id(), turn));
    assert!(chat_of(&a).stream_phase().is_none());
}

// ---- where it does not open -------------------------------------------------

#[test]
fn prefix_t_capital_in_the_list_does_nothing() {
    let mut a = tag_listing();
    assert_eq!(press(&mut a, "T"), Vec::new());
    assert!(!editor_open(&a));
}

#[test]
fn a_delete_prompt_is_not_interrupted_by_the_tag_key() {
    let mut a = tag_listing();
    a.update(key('d'));
    press(&mut a, "T");
    assert!(!editor_open(&a));
    // `t` is not a yes: the prompt closes and nothing is deleted.
    a.update(key('t'));
    assert!(!list_of(&a).confirming());
    assert!(!editor_open(&a));
    assert_eq!(ids(&a), vec![30, 10]);
}

#[test]
fn a_shell_pane_says_tags_are_for_chats() {
    let mut a = launched(80, 25).with_chat_history();
    press(&mut a, "T");
    assert_eq!(
        a.notice(),
        Some("Tags are for chat panes; open one with Ctrl+Space c")
    );
}

#[test]
fn without_a_store_there_are_no_tags_to_edit() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection()));
    press(&mut a, "c");
    assert_eq!(press(&mut a, "T"), Vec::new());
    assert!(!editor_open(&a));
    assert_eq!(a.notice(), Some("Tags need the conversation history"));
}

// ---- swallowing -------------------------------------------------------------

#[test]
fn the_editor_swallows_what_the_composer_and_a_pty_would_take() {
    let mut a = stored();
    press(&mut a, "T");
    for ev in [
        ctrl('c'),
        ctrl('y'),
        ctrl('b'),
        ctrl('r'),
        ctrl('j'),
        plain(KeyCode::PageUp),
        plain(KeyCode::Tab),
    ] {
        assert_eq!(a.update(ev), Vec::new());
    }
    assert_eq!(editor_text(&a), "proj");
    assert!(chat_of(&a).composer().is_empty());
}

#[test]
fn typing_in_the_editor_does_not_touch_the_composer() {
    let mut a = fresh();
    type_text(&mut a, "draft");
    press(&mut a, "T");
    type_text(&mut a, "x");
    a.update(plain(KeyCode::Esc));
    assert_eq!(chat_of(&a).composer().text(), "draft");
}

#[test]
fn a_paste_goes_into_the_editor() {
    let mut a = fresh();
    press(&mut a, "T");
    a.update(AppEvent::Paste(", a\nb".into()));
    assert_eq!(editor_text(&a), "proj, a b");
    assert!(chat_of(&a).composer().is_empty());
}

#[test]
fn a_held_enter_does_not_save() {
    let mut a = stored();
    press(&mut a, "T");
    let held = AppEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    assert_eq!(a.update(held), Vec::new());
    assert!(editor_open(&a));
}

#[test]
fn the_prefix_still_works_over_the_editor() {
    let mut a = stored();
    press(&mut a, "T");
    press(&mut a, "q");
    assert_eq!(a.input(), InputMode::Confirm(Confirm::Quit));
}

// ---- sharing the pane with the other views ----------------------------------

/// A conversation with memory on and history on, one answered exchange and
/// its engram project known: `Ctrl+Space n` and `Ctrl+Space s` work.
fn with_memory() -> App {
    let mut a = launched(80, 25).with_ai(ai(true)).with_chat_history();
    a.update(AppEvent::EnginesDetected(detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), Vec::new());
    a.update(key('n'));
    let effects = ask(&mut a, "q");
    let turn = super::chat_history::sent_turn(&effects);
    a.update(created(chat_id(), 7));
    a.update(delta(chat_id(), turn, "We use Axum."));
    a.update(finish(chat_id(), turn));
    a.update(AppEvent::Memory(
        crate::core::ai::memory::port::MemReply::Project {
            pane: chat_id(),
            epoch: 0,
            result: Ok("lazarobox-shell".into()),
        },
    ));
    a.take_dirty();
    a
}

#[test]
fn the_tag_key_does_not_open_over_the_note_editor() {
    let mut a = with_memory();
    press(&mut a, "n");
    assert!(chat_of(&a).note_editor().is_some());
    press(&mut a, "T");
    assert!(!editor_open(&a));
    assert!(chat_of(&a).note_editor().is_some());
}

#[test]
fn the_note_key_does_not_open_over_the_tag_editor() {
    let mut a = with_memory();
    press(&mut a, "T");
    press(&mut a, "n");
    assert!(chat_of(&a).note_editor().is_none());
    assert!(editor_open(&a));
    // Keys still belong to the tag editor.
    type_text(&mut a, "z");
    assert_eq!(editor_text(&a), "projz");
}

#[test]
fn the_summary_key_does_not_start_while_the_tag_editor_is_open() {
    let mut a = with_memory();
    press(&mut a, "T");
    let effects = press(&mut a, "s");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::ChatSend { .. })),
        "{effects:?}"
    );
    assert!(editor_open(&a));
}

#[test]
fn the_tag_key_does_not_open_over_the_summary_draft() {
    let mut a = with_memory();
    let effects = press(&mut a, "s");
    let turn = super::chat_history::sent_turn(&effects);
    a.update(delta(chat_id(), turn, "A summary."));
    a.update(finish(chat_id(), turn));
    assert!(chat_of(&a).summary_draft().is_some());
    press(&mut a, "T");
    assert!(!editor_open(&a));
    assert!(chat_of(&a).summary_draft().is_some());
}

#[test]
fn the_server_offer_does_not_take_the_keys_of_the_tag_editor() {
    use crate::core::ai::memory::{offer::Offer, port::MemReply};
    let mut a = with_memory();
    a.update(AppEvent::Memory(MemReply::ServerOffer(Offer {
        path: "/usr/local/bin/engram".into(),
        port: 7437,
    })));
    press(&mut a, "T");
    // `n` and `y` are tag characters here, not answers to the question.
    let effects = [a.update(key('y')), a.update(key('n'))].concat();
    assert_eq!(effects, Vec::new());
    assert_eq!(editor_text(&a), "projyn");
    a.update(plain(KeyCode::Esc));
    // The question is still there once the editor is gone.
    assert!(a.chat_guidance(chat_id()).is_some());
}

#[test]
fn a_summary_draft_that_arrives_waits_behind_the_tag_editor() {
    let mut a = with_memory();
    let effects = press(&mut a, "s");
    let turn = super::chat_history::sent_turn(&effects);
    press(&mut a, "T");
    a.update(delta(chat_id(), turn, "A summary."));
    a.update(finish(chat_id(), turn));
    // Keys still go to the tag editor; Esc closes it and the draft is next.
    type_text(&mut a, "x");
    assert_eq!(editor_text(&a), "projx");
    a.update(plain(KeyCode::Esc));
    assert!(!editor_open(&a));
    assert!(chat_of(&a).summary_draft().is_some());
}
