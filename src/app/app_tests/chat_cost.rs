//! The `/cost` command and its overlay (ai-cost 1c): user-only, never a
//! message, never a request, read off the UI thread, Esc closes it.

use super::chat_history::*;
use super::*;
use crate::core::ai::chat::{
    engine::StreamEvent,
    store::{StoreCmd, StoreReply},
    usage::TurnUsage,
    usage_view::{HEADER, HISTORY_OFF_LINE, READING_LINE, UNAVAILABLE_LINE, UsageRow},
};

fn sends(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::ChatSend { .. }))
        .count()
}

fn reports(effects: &[Effect]) -> Vec<Option<i64>> {
    stores(effects)
        .into_iter()
        .filter_map(|cmd| match cmd {
            StoreCmd::UsageReport { conversation, .. } => Some(conversation),
            _ => None,
        })
        .collect()
}

fn composer_text(a: &App) -> String {
    chat_of(a).composer().text()
}

fn view_lines(a: &App) -> Vec<String> {
    chat_of(a)
        .cost()
        .map(|v| v.lines().to_vec())
        .unwrap_or_default()
}

fn answer(rows: Option<Vec<UsageRow>>) -> AppEvent {
    AppEvent::Store(StoreReply::UsageReport {
        pane: chat_id(),
        rows,
    })
}

fn today_row() -> UsageRow {
    UsageRow {
        days_ago: 0,
        engine: "claude".into(),
        model: Some("sonnet".into()),
        in_chat: true,
        turns: 2,
        input: Some(2),
        output: Some(4),
        cache_read: None,
        cost_micros: Some(4_936),
        total: Some(6),
        partial: false,
    }
}

fn history_off_chat() -> App {
    let mut a = launched(120, 30).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(super::chat_history::detection()));
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    a
}

#[test]
fn cost_is_not_a_message_nor_a_request_and_asks_the_store_off_the_ui_thread() {
    let mut a = conversation();
    let before = chat_of(&a).messages().count();
    let effects = ask(&mut a, "/cost");
    assert_eq!(sends(&effects), 0, "{effects:?}");
    assert_eq!(reports(&effects), vec![None]);
    assert_eq!(chat_of(&a).messages().count(), before);
    assert_eq!(composer_text(&a), "");
    assert_eq!(view_lines(&a), vec![HEADER, READING_LINE]);
}

#[test]
fn the_store_answer_replaces_the_reading_line() {
    let mut a = conversation();
    ask(&mut a, "/cost");
    a.update(answer(Some(vec![today_row()])));
    let lines = view_lines(&a);
    assert_eq!(lines[0], "AI usage (alpha)");
    assert_eq!(lines[1], "today: 6 tokens · 2 turns");
    assert!(
        lines.contains(
            &"  claude › sonnet: in 2 · out 4 · $0.0049 list-price equivalent".to_owned()
        ),
        "{lines:?}"
    );
    assert!(!lines.contains(&READING_LINE.to_owned()));
}

#[test]
fn an_unreadable_store_says_so_and_nothing_else() {
    let mut a = conversation();
    ask(&mut a, "/cost");
    a.update(answer(None));
    assert_eq!(view_lines(&a), vec![UNAVAILABLE_LINE]);
}

#[test]
fn a_detached_store_does_not_wait_for_an_answer() {
    let mut a = conversation();
    a.update(AppEvent::Store(StoreReply::Unavailable {
        pane: chat_id(),
        reason: "database is corrupt".into(),
    }));
    let effects = ask(&mut a, "/cost");
    assert_eq!(reports(&effects), Vec::<Option<i64>>::new());
    assert_eq!(view_lines(&a), vec![UNAVAILABLE_LINE]);
}

#[test]
fn the_report_names_the_conversation_once_it_exists() {
    let mut a = conversation();
    ask(&mut a, "hello");
    a.update(created(chat_id(), 7));
    let effects = ask(&mut a, "/cost");
    assert_eq!(reports(&effects), vec![Some(7)]);
}

#[test]
fn esc_closes_the_overlay_and_the_keys_never_reach_the_composer() {
    let mut a = conversation();
    ask(&mut a, "/cost");
    a.update(key('x'));
    assert_eq!(composer_text(&a), "");
    assert!(chat_of(&a).cost().is_some());
    a.update(plain(KeyCode::Esc));
    assert!(chat_of(&a).cost().is_none());
    assert!(chat_of(&a).conversation_shown());
}

#[test]
fn a_late_answer_after_esc_changes_nothing() {
    let mut a = conversation();
    ask(&mut a, "/cost");
    a.update(plain(KeyCode::Esc));
    a.update(answer(Some(vec![today_row()])));
    assert!(chat_of(&a).cost().is_none());
}

#[test]
fn cost_works_while_a_stream_runs_and_leaves_it_alone() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "question"));
    a.update(delta(chat_id(), turn, "par"));
    let effects = ask(&mut a, "/cost");
    assert_eq!(sends(&effects), 0);
    assert!(chat_of(&a).cost().is_some());
    assert!(!systems(chat_of(&a)).iter().any(|l| l.contains("wait")));
    a.update(plain(KeyCode::Esc));
    a.update(delta(chat_id(), turn, "tial"));
    a.update(finish(chat_id(), turn));
    assert_eq!(chat_of(&a).messages().last().unwrap().text, "partial");
}

#[test]
fn cost_with_arguments_is_refused_visibly_and_keeps_the_text() {
    let mut a = conversation();
    let effects = ask(&mut a, "/cost now");
    assert_eq!(sends(&effects), 0);
    assert_eq!(reports(&effects), Vec::<Option<i64>>::new());
    assert!(systems(chat_of(&a)).contains(&"/cost takes no arguments".to_owned()));
    assert_eq!(composer_text(&a), "/cost now");
    assert!(chat_of(&a).cost().is_none());
}

#[test]
fn a_double_slash_sends_cost_as_a_normal_message() {
    let mut a = conversation();
    let effects = ask(&mut a, "//cost");
    assert_eq!(sends(&effects), 1);
    assert_eq!(chat_of(&a).messages().last().unwrap().text, "/cost");
    assert!(chat_of(&a).cost().is_none());
}

#[test]
fn other_text_with_cost_in_it_is_a_normal_message() {
    let mut a = conversation();
    assert_eq!(sends(&ask(&mut a, "what does /cost do")), 1);
}

#[test]
fn history_off_shows_this_session_from_memory_and_asks_the_store_nothing() {
    let mut a = history_off_chat();
    let turn = sent_turn(&ask(&mut a, "hi"));
    a.update(delta(chat_id(), turn, "answer"));
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::usage(TurnUsage {
            input: Some(10),
            output: Some(5),
            model: Some("llama3".into()),
            ..TurnUsage::default()
        }),
    ));
    a.update(finish(chat_id(), turn));
    let effects = ask(&mut a, "/cost");
    assert_eq!(stores(&effects), Vec::new());
    let lines = view_lines(&a);
    assert_eq!(lines[0], HEADER);
    assert_eq!(lines[1], HISTORY_OFF_LINE);
    assert_eq!(lines[2], "this session: 15 tokens · 1 turns");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("  ollama › llama3: in 10 · out 5"))
    );
}

#[test]
fn history_off_without_usage_says_nothing_was_recorded() {
    let mut a = history_off_chat();
    ask(&mut a, "/cost");
    assert_eq!(
        view_lines(&a),
        vec![HEADER, HISTORY_OFF_LINE, "no usage recorded yet"]
    );
}
