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
        origin: Some("/work/proj".into()),
        open: vec![7],
    }
}

fn created(conversation: i64) -> StoreReply {
    StoreReply::Created {
        pane: pane(),
        conversation,
        evicted: false,
        evicted_notes: 0,
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
            origin: Some("/work/proj".into()),
            open: vec![7],
        })
    );
    assert_eq!(store.conversation(), None);
}

#[test]
fn saves_before_created_append_at_once_and_the_worker_resolves_the_id() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    // No id yet: the append names the pane only; the store thread knows the
    // conversation it created for it.
    assert_eq!(
        store.save(pane(), assistant("Hel", MsgStatus::Cancelled), &new_conv()),
        Some(StoreCmd::Append {
            pane: pane(),
            conversation: None,
            message: assistant("Hel", MsgStatus::Cancelled),
        })
    );
    assert_eq!(
        store.save(pane(), user("again"), &new_conv()),
        Some(StoreCmd::Append {
            pane: pane(),
            conversation: None,
            message: user("again"),
        })
    );
    store.on_reply(pane(), &created(42));
    assert_eq!(store.conversation(), Some(42));
    assert!(matches!(
        store.save(pane(), user("later"), &new_conv()),
        Some(StoreCmd::Append {
            conversation: Some(42),
            ..
        })
    ));
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
            conversation: Some(3),
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
        MsgStatus::Error,
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

// An answer that failed after partial text keeps that text, with status
// `error`; the error line itself is a system line and never reaches a save.
#[test]
fn an_error_answer_is_saved_like_any_partial_one() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.on_reply(pane(), &created(1));
    assert!(matches!(
        store.save(pane(), assistant("par", MsgStatus::Error), &new_conv()),
        Some(StoreCmd::Append { message, .. }) if message.status == MsgStatus::Error
    ));
}

#[test]
fn an_unavailable_store_drops_later_saves() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.save(pane(), assistant("x", MsgStatus::Complete), &new_conv());
    store.on_reply(
        pane(),
        &StoreReply::Unavailable {
            pane: pane(),
            reason: "database is corrupt".into(),
        },
    );
    assert_eq!(store.save(pane(), user("more"), &new_conv()), None);
    assert_eq!(store.conversation(), None);
}

#[test]
fn a_failed_create_detaches_the_conversation() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.on_reply(
        pane(),
        &StoreReply::Failed {
            pane: pane(),
            notify: true,
        },
    );
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
        evicted_notes: 0,
    };
    store.on_reply(pane(), &reply);
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

#[test]
fn link_replies_do_not_change_where_the_conversation_is() {
    let mut store = PaneStore::new();
    store.save(pane(), user("hi"), &new_conv());
    store.on_reply(pane(), &created(5));
    let replies = [
        StoreReply::SessionLinked {
            pane: pane(),
            conversation: 5,
            session_id: "lazarobox-00112233445566778899aabbccddeeff".into(),
            project: "proj".into(),
        },
        StoreReply::LinkFailed {
            pane: pane(),
            kind: LinkKind::Note,
        },
    ];
    for reply in &replies {
        store.on_reply(pane(), reply);
        assert_eq!(store.conversation(), Some(5));
    }
}

#[test]
fn nothing_evicted_adds_no_line() {
    assert!(eviction_lines(false, 0).is_empty());
}

#[test]
fn an_eviction_without_notes_adds_only_the_first_line() {
    assert_eq!(
        eviction_lines(true, 0),
        ["oldest conversation removed (limit 200)"]
    );
}

#[test]
fn the_note_line_follows_the_first_and_agrees_in_number() {
    assert_eq!(
        eviction_lines(true, 1),
        [
            "oldest conversation removed (limit 200)",
            "1 note of a removed conversation remains in engram"
        ]
    );
    assert_eq!(
        eviction_lines(true, 3),
        [
            "oldest conversation removed (limit 200)",
            "3 notes of removed conversations remain in engram"
        ]
    );
}

#[test]
fn a_reply_gives_its_lines_and_other_replies_none() {
    let evicted = StoreReply::Created {
        pane: pane(),
        conversation: 1,
        evicted: true,
        evicted_notes: 2,
    };
    assert_eq!(
        eviction_lines_of(&evicted),
        [
            "oldest conversation removed (limit 200)",
            "2 notes of removed conversations remain in engram"
        ]
    );
    assert!(eviction_lines_of(&created(1)).is_empty());
    let failed = StoreReply::Failed {
        pane: pane(),
        notify: true,
    };
    assert!(eviction_lines_of(&failed).is_empty());
}

// A resumed conversation starts open, a deleted one is detached.
#[test]
fn an_opened_store_appends_to_that_conversation() {
    let mut store = PaneStore::open(42);
    assert_eq!(store.conversation(), Some(42));
    assert_eq!(
        store.save(pane(), user("hi"), &new_conv()),
        Some(StoreCmd::Append {
            pane: pane(),
            conversation: Some(42),
            message: user("hi"),
        })
    );
}

#[test]
fn a_detached_store_drops_every_save() {
    let mut store = PaneStore::open(42);
    assert!(!store.is_detached());
    store.detach();
    assert!(store.is_detached());
    assert_eq!(store.conversation(), None);
    assert_eq!(store.save(pane(), user("hi"), &new_conv()), None);
}

#[test]
fn a_fresh_store_is_not_detached() {
    assert!(!PaneStore::new().is_detached());
}
