//! The pane-side queue and the shape of the messages it sends.

use super::*;

fn pane() -> PaneId {
    PaneId::FIRST
}

fn msg(role: Role, text: &str, status: MsgStatus) -> StoredMessage {
    StoredMessage {
        role,
        text: text.into(),
        status,
        engine: "ollama".into(),
        model: Some("llama3".into()),
    }
}

fn user(text: &str) -> StoredMessage {
    msg(Role::User, text, MsgStatus::Complete)
}

fn assistant(text: &str, status: MsgStatus) -> StoredMessage {
    msg(Role::Assistant, text, status)
}

fn new_conv() -> NewConversation {
    NewConversation {
        profile: "programmer".into(),
        tag: Some("proj".into()),
        open: vec![7],
    }
}

fn created(conversation: i64) -> StoreReply {
    StoreReply::Created {
        pane: pane(),
        conversation,
        evicted: false,
    }
}

#[test]
fn the_first_save_creates_the_conversation() {
    let mut store = PaneStore::new();
    let cmd = store.save(pane(), user("hi"), &new_conv());
    assert_eq!(
        cmd,
        Some(StoreCmd::Create {
            pane: pane(),
            profile: "programmer".into(),
            tag: Some("proj".into()),
            first: user("hi"),
            open: vec![7],
        })
    );
    assert_eq!(store.conversation(), None);
}

#[test]
fn saves_before_created_wait_and_are_released_in_order() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    assert_eq!(
        store.save(pane(), assistant("Hel", MsgStatus::Cancelled), &new_conv()),
        None
    );
    assert_eq!(store.save(pane(), user("again"), &new_conv()), None);

    let released = store.on_reply(pane(), &created(42));
    assert_eq!(
        released,
        vec![
            StoreCmd::Append {
                pane: pane(),
                conversation: 42,
                message: assistant("Hel", MsgStatus::Cancelled),
            },
            StoreCmd::Append {
                pane: pane(),
                conversation: 42,
                message: user("again"),
            },
        ]
    );
    assert_eq!(store.conversation(), Some(42));
}

#[test]
fn after_created_saves_append_at_once() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.on_reply(pane(), &created(3));
    assert_eq!(
        store.save(pane(), assistant("yo", MsgStatus::Complete), &new_conv()),
        Some(StoreCmd::Append {
            pane: pane(),
            conversation: 3,
            message: assistant("yo", MsgStatus::Complete),
        })
    );
}

#[test]
fn every_partial_status_is_saved_as_it_is() {
    for status in [
        MsgStatus::Complete,
        MsgStatus::Cancelled,
        MsgStatus::Interrupted,
        MsgStatus::Truncated,
    ] {
        let mut store = PaneStore::new();
        store.save(pane(), user("hi"), &new_conv());
        store.on_reply(pane(), &created(1));
        let cmd = store.save(pane(), assistant("x", status), &new_conv());
        assert!(
            matches!(&cmd, Some(StoreCmd::Append { message, .. }) if message.status == status),
            "{status:?}"
        );
    }
}

#[test]
fn errors_are_never_saved() {
    let mut store = PaneStore::new();
    assert_eq!(
        store.save(pane(), assistant("boom", MsgStatus::Error), &new_conv()),
        None
    );
    // And it did not start a conversation.
    assert!(matches!(
        store.save(pane(), user("hi"), &new_conv()),
        Some(StoreCmd::Create { .. })
    ));
}

#[test]
fn an_unavailable_store_drops_the_queue_and_later_saves() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.save(pane(), assistant("x", MsgStatus::Complete), &new_conv());
    let released = store.on_reply(
        pane(),
        &StoreReply::Unavailable {
            pane: pane(),
            reason: "database is corrupt".into(),
        },
    );
    assert!(released.is_empty());
    assert_eq!(store.save(pane(), user("more"), &new_conv()), None);
    assert_eq!(store.conversation(), None);
}

#[test]
fn a_failed_create_detaches_the_conversation() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    let released = store.on_reply(
        pane(),
        &StoreReply::Failed {
            pane: pane(),
            notify: true,
        },
    );
    assert!(released.is_empty());
    assert_eq!(store.save(pane(), user("more"), &new_conv()), None);
}

#[test]
fn a_failed_append_keeps_the_conversation_open() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.on_reply(pane(), &created(9));
    store.on_reply(
        pane(),
        &StoreReply::Failed {
            pane: pane(),
            notify: true,
        },
    );
    assert_eq!(store.conversation(), Some(9));
    assert!(store.save(pane(), user("more"), &new_conv()).is_some());
}

#[test]
fn a_reply_for_another_pane_changes_nothing() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    let other = PaneId::for_test(9);
    let reply = StoreReply::Created {
        pane: other,
        conversation: 5,
        evicted: false,
    };
    assert!(store.on_reply(pane(), &reply).is_empty());
    assert_eq!(store.conversation(), None);
}

#[test]
fn error_text_is_sanitized_flat_and_bounded() {
    let raw = format!("bad \u{1b}[31mthing\u{1b}[0m\n{}", "x".repeat(500));
    let text = bounded_error(&raw);
    assert!(text.starts_with("bad thing x"), "{text}");
    assert!(!text.contains('\u{1b}') && !text.contains('\n'));
    assert_eq!(text.chars().count(), ERROR_MAX);
}

#[test]
fn short_error_text_is_kept() {
    assert_eq!(bounded_error("database is corrupt"), "database is corrupt");
}
