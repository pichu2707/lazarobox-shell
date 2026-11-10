//! Rolling compaction in the app (chat-memory S3, S4, S8): a run goes out
//! after a turn ends, with the `summarize` task's engine, and never holds the
//! user's next send.

use super::chat_notes::{CWD, PROJECT, ai, ctrl, oldest_save, plain, systems, type_text};
use super::*;
use crate::core::ai::{
    EngineState, Status,
    chat::{
        Entry,
        compact::COMPACT_PROMPT,
        engine::{ChatError, ChatRequest, Finish, StreamEvent},
    },
    config::{Assignment, CompactionMode, Task},
    memory::port::MemReply,
};

fn chat_id() -> PaneId {
    id2()
}

fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

/// An open chat on a ready ollama; `summarize` is assigned `model` when given.
fn app(memory: bool, summarize: Option<&str>) -> App {
    let mut config = ai(memory);
    if let Some(model) = summarize {
        config.set(
            Task::Summarize,
            Assignment {
                engine: Some("ollama".into()),
                model: Some(model.into()),
            },
        );
    }
    let mut a = launched(80, 25).with_ai(config);
    a.update(cwd_event(CWD));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".to_owned(), "tiny".to_owned()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
            binary: None,
        }],
        ..Default::default()
    }));
    press(&mut a, "c");
    a.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    if memory {
        a.update(AppEvent::Memory(MemReply::Project {
            pane: chat_id(),
            epoch: 0,
            result: Ok(PROJECT.into()),
        }));
    }
    a.take_dirty();
    a
}

/// The effects of one exchange: the question sent, then the answer streamed.
/// Returns the effects of the final `Done` and the request of the question.
fn exchange(a: &mut App, size: usize) -> (Vec<Effect>, ChatRequest) {
    a.chat_type(chat_id(), &"q".repeat(size));
    let sent = a.update(plain(KeyCode::Enter));
    let Some(Effect::ChatSend { turn, request, .. }) = sent
        .iter()
        .find(|e| matches!(e, Effect::ChatSend { .. }))
        .cloned()
    else {
        panic!("expected a ChatSend, got {sent:?}");
    };
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("a".repeat(size)),
    ));
    let end = a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    (end, request)
}

fn compaction_of(effects: &[Effect]) -> Option<(u64, ChatRequest)> {
    effects.iter().find_map(|e| match e {
        Effect::ChatCompact { turn, request, .. } => Some((*turn, request.clone())),
        _ => None,
    })
}

/// Drives exchanges of 1000 characters until a run goes out.
fn until_compaction(a: &mut App) -> (u64, ChatRequest) {
    for _ in 0..14 {
        let (end, _) = exchange(a, 1_000);
        if let Some(run) = compaction_of(&end) {
            return run;
        }
    }
    panic!("no compaction run went out");
}

#[test]
fn a_run_goes_out_after_a_turn_ends_with_the_summarize_engine() {
    let mut a = app(true, Some("tiny"));
    let (turn, request) = until_compaction(&mut a);
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("tiny"));
    assert_eq!(turn, chat_of(&a).turn());
    assert_eq!(chat_of(&a).phase(), Phase::Idle, "the pane stays free");
}

#[test]
fn an_unassigned_summarize_task_uses_the_chat_engine_never_the_general_one() {
    let mut config = ai(true);
    config.set(
        Task::General,
        Assignment {
            engine: Some("claude".into()),
            model: Some("opus".into()),
        },
    );
    let mut a = app(true, None).with_ai(config);
    let (_, request) = until_compaction(&mut a);
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
}

#[test]
fn an_explicit_summarize_task_wins_over_the_chat_engine() {
    let mut config = ai(true);
    config.set(
        Task::General,
        Assignment {
            engine: Some("claude".into()),
            model: Some("opus".into()),
        },
    );
    config.set(
        Task::Summarize,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("tiny".into()),
        },
    );
    let mut a = app(true, None).with_ai(config);
    let (_, request) = until_compaction(&mut a);
    assert_eq!(request.model.as_deref(), Some("tiny"));
}

#[test]
fn compaction_off_sends_no_run_and_keeps_the_marker() {
    let mut config = ai(true);
    config.set_compaction(CompactionMode::Off);
    let mut a = app(true, Some("tiny")).with_ai(config);
    for _ in 0..14 {
        let (end, _) = exchange(&mut a, 1_000);
        assert_eq!(compaction_of(&end), None);
    }
    assert!(
        chat_of(&a).entries().iter().any(
            |e| matches!(e, Entry::System(s) if s == crate::core::ai::chat::TRUNCATION_MARKER)
        )
    );
}

#[test]
fn a_short_chat_sends_no_run() {
    let mut a = app(true, Some("tiny"));
    for _ in 0..8 {
        let (end, _) = exchange(&mut a, 10);
        assert_eq!(compaction_of(&end), None);
    }
}

#[test]
fn memory_off_still_compacts_and_the_summary_goes_with_the_next_send() {
    let mut a = app(false, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("- the decision".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert!(a.take_dirty(), "the transcript changed");
    assert!(
        chat_of(&a)
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::System(s) if s.ends_with("messages summarized")))
    );
    let (_, request) = exchange(&mut a, 1_000);
    assert!(request.memory.unwrap().contains("- the decision"));
}

#[test]
fn a_failed_run_changes_nothing_and_does_not_block_the_next_send() {
    let mut a = app(true, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    let before = chat_of(&a).entries().len();
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("down".into())),
    ));
    assert_eq!(chat_of(&a).entries().len(), before);
    let (_, request) = exchange(&mut a, 1_000);
    assert_eq!(request.memory, None);
}

#[test]
fn a_send_while_the_run_is_in_flight_goes_at_once_and_the_late_summary_is_dropped() {
    let mut a = app(true, Some("tiny"));
    let (turn, _) = until_compaction(&mut a);
    a.chat_type(chat_id(), "next question");
    let sent = a.update(plain(KeyCode::Enter));
    assert!(
        sent.iter()
            .any(|e| matches!(e, Effect::ChatSend { request, .. } if request.system != COMPACT_PROMPT.trim_end())),
        "{sent:?}"
    );
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("- late".into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    assert_eq!(chat_of(&a).summary_block(), None);
}

#[test]
fn a_summarize_model_that_is_missing_sends_no_run() {
    let mut a = app(true, Some("gone"));
    for _ in 0..14 {
        let (end, _) = exchange(&mut a, 1_000);
        assert_eq!(compaction_of(&end), None);
    }
}

// chat-memory T2 (S5, S6): the summary goes to engram only on opt-in --------

use crate::core::ai::{
    chat::store::StoreReply,
    memory::{
        kind::MemKind,
        port::{MemCmd, Reason},
    },
};

const CONVERSATION: i64 = 42;

fn opted_in(save: bool) -> AiConfig {
    let mut config = ai(true);
    config.memory.save_summaries = save;
    config
}

const CREATED_AT: i64 = 1_000;

/// The privacy notices among the system lines of the transcript.
fn notices_in_transcript(a: &App) -> Vec<String> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::System(line) if line.contains("save_summaries") => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn known_conversation(a: &mut App) {
    created_conversation(a, CONVERSATION, CREATED_AT);
}

fn created_conversation(a: &mut App, conversation: i64, created_at: i64) {
    a.update(AppEvent::Store(StoreReply::Created {
        pane: chat_id(),
        conversation,
        evicted: false,
        evicted_notes: 0,
        created_at,
    }));
}

thread_local! {
    static NOW_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static BASE: Instant = Instant::now();
}

fn mock_now() -> Instant {
    BASE.with(|b| *b + Duration::from_millis(NOW_MS.with(|n| n.get())))
}

fn saved_key(effects: &[Effect]) -> Option<String> {
    match saves(effects).as_slice() {
        [MemCmd::Save { topic_key, .. }] => topic_key.clone(),
        other => panic!("expected one save, got {other:?}"),
    }
}

/// Runs one compaction to its end with `summary` as the model's text and
/// returns the effects of the final `Done`.
fn summarized(a: &mut App, summary: &str) -> Vec<Effect> {
    let (turn, _) = until_compaction(a);
    finish(a, turn, summary)
}

fn finish(a: &mut App, turn: u64, summary: &str) -> Vec<Effect> {
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta(summary.into()),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ))
}

fn saves(effects: &[Effect]) -> Vec<&MemCmd> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Memory(cmd @ MemCmd::Save { .. }) => Some(cmd),
            _ => None,
        })
        .collect()
}

#[test]
fn an_opted_in_summary_is_saved_once_under_its_conversation_key() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- we chose Axum");
    let [
        MemCmd::Save {
            pane,
            session,
            project,
            note,
            topic_key,
            ..
        },
    ] = saves(&effects).as_slice()
    else {
        panic!("expected one save, got {effects:?}");
    };
    assert_eq!(
        (*pane, session, project.as_str()),
        (chat_id(), &None, PROJECT)
    );
    assert_eq!(topic_key.as_deref(), Some("chat/42-1000/summary"));
    assert_eq!(note.kind, MemKind::Discovery);
    assert_eq!(note.content, "- we chose Axum");
}

#[test]
fn the_t1_redaction_keeps_a_secret_out_of_the_summary_that_is_saved() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- the login uses password=hunter2 here");
    let [MemCmd::Save { note, .. }] = saves(&effects).as_slice() else {
        panic!("expected one save, got {effects:?}");
    };
    assert!(!note.content.contains("hunter2"), "{}", note.content);
}

#[test]
fn a_later_summary_saves_again_under_the_same_key() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let first = summarized(&mut a, "- one");
    let second = summarized(&mut a, "- one\n- two");
    let keys: Vec<_> = [&first, &second]
        .iter()
        .flat_map(|e| saves(e))
        .map(|cmd| match cmd {
            MemCmd::Save { topic_key, .. } => topic_key.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(keys, vec![Some("chat/42-1000/summary".to_owned()); 2]);
}

#[test]
fn without_the_flag_nothing_is_saved_and_the_notice_comes_once() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(false));
    known_conversation(&mut a);
    let first = summarized(&mut a, "- one");
    assert!(saves(&first).is_empty(), "{first:?}");
    assert_eq!(a.notice, None, "not the transient notice");
    let lines = notices_in_transcript(&a);
    assert_eq!(lines.len(), 1, "{lines:?}");
    for part in [
        "save_summaries = true",
        "[ai.memory]",
        "engram",
        "stay in engram",
    ] {
        assert!(lines[0].contains(part), "{part}: {}", lines[0]);
    }
    let second = summarized(&mut a, "- one\n- two");
    assert!(saves(&second).is_empty(), "{second:?}");
    assert_eq!(notices_in_transcript(&a).len(), 1, "shown once per run");
    assert_eq!(a.notice, None);
}

#[test]
fn memory_off_or_unavailable_writes_nothing_and_says_nothing() {
    // Memory off (S8), even with the flag set.
    let mut off = ai(false);
    off.memory.save_summaries = true;
    let mut a = app(false, Some("tiny")).with_ai(off);
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- one");
    assert!(saves(&effects).is_empty(), "{effects:?}");
    assert_eq!(a.notice, None);

    // Memory on but engram not answering.
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .expect("a chat")
        .set_unavailable(&Reason::NotRunning.text());
    a.notice = None;
    let effects = summarized(&mut a, "- one");
    assert!(saves(&effects).is_empty(), "{effects:?}");
    assert_eq!(a.notice, None);
}

#[test]
fn a_conversation_the_store_never_created_is_not_saved() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    let effects = summarized(&mut a, "- one");
    assert!(saves(&effects).is_empty(), "{effects:?}");
    assert_eq!(a.notice, None);
}

#[test]
fn a_failed_or_empty_run_saves_nothing() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let (turn, _) = until_compaction(&mut a);
    let failed = a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(ChatError::Failed("down".into())),
    ));
    assert!(saves(&failed).is_empty());
    let empty = summarized(&mut a, "   ");
    assert!(saves(&empty).is_empty(), "{empty:?}");
}

#[test]
fn the_answer_to_a_summary_save_adds_no_transcript_line_and_a_failure_none() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- one");
    let [MemCmd::Save { id, .. }] = saves(&effects).as_slice() else {
        panic!("expected one save, got {effects:?}");
    };
    let before = chat_of(&a).entries().len();
    a.update(AppEvent::Memory(MemReply::SaveFailed {
        pane: chat_id(),
        id: *id,
        reason: "boom".into(),
    }));
    // The first failure of the run is told once, as a line (never a note
    // line); the later ones are quiet.
    assert_eq!(chat_of(&a).entries().len(), before + 1);
    assert_eq!(a.notice, None);

    let effects = summarized(&mut a, "- one\n- two");
    let [MemCmd::Save { id, .. }] = saves(&effects).as_slice() else {
        panic!("expected one save, got {effects:?}");
    };
    let before = chat_of(&a).entries().len();
    // The worker could not even try (engram went away): silent too.
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: *id,
        reason: Reason::NotRunning,
    }));
    assert_eq!(chat_of(&a).entries().len(), before);
    assert_eq!(a.notice, None);

    let effects = summarized(&mut a, "- one\n- two\n- three");
    let [MemCmd::Save { id, .. }] = saves(&effects).as_slice() else {
        panic!("expected one save, got {effects:?}");
    };
    let before = chat_of(&a).entries().len();
    a.update(AppEvent::Memory(MemReply::Saved {
        pane: chat_id(),
        id: *id,
        project: PROJECT.into(),
        session: "lazarobox-0123456789abcdef0123456789abcdef".into(),
        observation_id: 9,
        topic_key: "chat/42-1000/summary".into(),
        title: "Conversation summary".into(),
    }));
    assert_eq!(chat_of(&a).entries().len(), before, "a save is silent");
}

#[test]
fn a_reused_conversation_id_saves_under_a_different_key() {
    // SQLite reuses the id of a deleted conversation: the creation time keeps
    // the new chat from upserting over the summary of the old one.
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    created_conversation(&mut a, CONVERSATION, 1_000);
    let old = saved_key(&summarized(&mut a, "- the old chat"));
    created_conversation(&mut a, CONVERSATION, 2_000);
    let new = saved_key(&summarized(&mut a, "- the new chat"));
    assert_eq!(old.as_deref(), Some("chat/42-1000/summary"));
    assert_eq!(new.as_deref(), Some("chat/42-2000/summary"));
}

#[test]
fn a_resumed_conversation_does_not_update_its_saved_summary() {
    // The rolling summary is not persisted: after a resume it covers only the
    // later messages and would replace the fuller one already saved.
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    a.panes
        .get_mut(&chat_id())
        .and_then(PaneState::chat_mut)
        .expect("a chat")
        .set_conversation(CONVERSATION);
    let effects = summarized(&mut a, "- narrower");
    assert!(saves(&effects).is_empty(), "{effects:?}");
    assert!(notices_in_transcript(&a).is_empty(), "nothing is offered");
}

#[test]
fn a_summary_save_that_fails_says_so_once_per_run() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    for (i, summary) in ["- one", "- one\n- two", "- one\n- two\n- three"]
        .into_iter()
        .enumerate()
    {
        let effects = summarized(&mut a, summary);
        let Some(MemCmd::Save { id, .. }) = saves(&effects).first().copied() else {
            panic!("expected a save, got {effects:?}");
        };
        let before = systems(&a);
        a.update(AppEvent::Memory(MemReply::SaveFailed {
            pane: chat_id(),
            id: *id,
            reason: "engram refused".into(),
        }));
        let after = systems(&a);
        if i == 0 {
            assert_eq!(
                &after[before.len()..],
                ["could not save the conversation summary to engram: engram refused"]
            );
        } else {
            assert_eq!(after, before, "told once, then quiet");
        }
    }
    assert_eq!(a.notice, None);
}

#[test]
fn an_unavailable_engram_stays_silent_for_a_summary_save() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- one");
    let Some(MemCmd::Save { id, .. }) = saves(&effects).first().copied() else {
        panic!("expected a save, got {effects:?}");
    };
    let before = systems(&a);
    a.update(AppEvent::Memory(MemReply::SaveUnavailable {
        pane: chat_id(),
        id: *id,
        reason: Reason::NotRunning,
    }));
    assert_eq!(systems(&a), before);
}

#[test]
fn a_summary_save_that_runs_out_of_time_stays_silent() {
    NOW_MS.with(|n| n.set(0));
    let mut a = app(true, Some("tiny"))
        .with_ai(opted_in(true))
        .with_clock(mock_now);
    known_conversation(&mut a);
    let effects = summarized(&mut a, "- one");
    assert_eq!(saves(&effects).len(), 1);
    assert_eq!(chat_of(&a).pending_save_ids().len(), 1);
    NOW_MS.with(|n| n.set(60_000));
    a.update(AppEvent::MemoryTick);
    assert!(
        chat_of(&a).pending_save_ids().is_empty(),
        "the wait was given up"
    );
    // (The tick may also give up the context request of the chat; that line
    // is not about the save.)
    for line in systems(&a) {
        assert!(
            !line.contains("not saved") && !line.contains("could not save"),
            "{line}"
        );
    }
    assert_eq!(a.notice, None);
}

#[test]
fn a_quiet_summary_save_leaves_a_saving_note_editor_untouched() {
    let mut a = app(true, Some("tiny")).with_ai(opted_in(true));
    known_conversation(&mut a);
    let (turn, _) = until_compaction(&mut a);
    press(&mut a, "n");
    type_text(&mut a, "My note");
    a.update(ctrl('s'));
    assert!(chat_of(&a).note_editor().expect("editor").is_saving());
    let editor_save = oldest_save(&a).0;

    let effects = finish(&mut a, turn, "- one");
    let Some(MemCmd::Save { id: quiet, .. }) = saves(&effects).first().copied() else {
        panic!("expected a quiet save, got {effects:?}");
    };
    assert_ne!(*quiet, editor_save);
    for reply in [
        MemReply::SaveFailed {
            pane: chat_id(),
            id: *quiet,
            reason: "engram refused".into(),
        },
        MemReply::SaveUnavailable {
            pane: chat_id(),
            id: *quiet,
            reason: Reason::NotRunning,
        },
    ] {
        a.update(AppEvent::Memory(reply));
        let editor = chat_of(&a).note_editor().expect("still open");
        assert!(editor.is_saving(), "its own answer is still awaited");
        assert_eq!(editor.notice(), None);
        assert_eq!(editor.title(), "My note");
    }
}

// local-model-writes D1: a compaction run after the clean `Done` keeps the
// pane live and bumps the turn; the batch the turn left still opens its
// review once the run ends.
#[test]
fn a_compaction_run_after_a_clean_end_still_opens_the_review_of_its_batch() {
    use crate::core::ai::chat::review::Badge;
    let badges = |badge: Badge| AppEvent::Badges([(chat_id(), badge)].into_iter().collect());
    let mut a = app(true, Some("tiny"));
    a.update(badges(Badge::Staged(1)));
    let (turn, _) = until_compaction(&mut a);
    finish(&mut a, turn, "summary");
    assert_eq!(
        a.update(badges(Badge::Orphan(1))),
        vec![Effect::BatchReview(chat_id())]
    );
}

// ai-cost 1b.4: the run's usage reaches the store with its role.
#[test]
fn a_compaction_run_that_reported_usage_is_recorded_with_history_on() {
    use crate::core::ai::chat::{store::StoreCmd, usage::TurnUsage};
    let mut a = app(false, Some("tiny")).with_chat_history();
    let (turn, _) = until_compaction(&mut a);
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::usage(TurnUsage {
            output: Some(9),
            ..TurnUsage::default()
        }),
    ));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("- the decision".into()),
    ));
    let end = a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let roles: Vec<String> = end
        .iter()
        .filter_map(|e| match e {
            Effect::Store(StoreCmd::RecordUsage { role, usage, .. }) => {
                assert_eq!(usage.model.as_deref(), Some("tiny"));
                Some(role.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(roles, vec!["compaction".to_owned()]);
}
