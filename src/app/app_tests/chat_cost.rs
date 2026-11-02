//! The `/cost` command and its overlay (ai-cost 1c): user-only, never a
//! message, never a request, read off the UI thread, Esc closes it.

use super::chat_history::*;
use super::*;
use crate::core::ai::chat::{
    cost_view::READING_LINE,
    engine::StreamEvent,
    store::{StoreCmd, StoreReply},
    usage::TurnUsage,
    usage_view::{HEADER, UNAVAILABLE_LINE, UsageRow},
};
use crate::core::ai::config::{AiConfig, Assignment, Task};

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
        records: 2,
        costed: 2,
        input: Some(2),
        output: Some(4),
        cache_read: None,
        cache_hit: None,
        cache_base: None,
        cache_gap: 0,
        cost_micros: Some(4_936),
        total: Some(6),
        partial: false,
        workers: 0,
        worker_total: None,
        worker_partial: false,
    }
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
fn a_detached_pane_still_asks_and_the_reply_decides() {
    let mut a = conversation();
    a.update(AppEvent::Store(StoreReply::Unavailable {
        pane: chat_id(),
        reason: "database is corrupt".into(),
    }));
    let effects = ask(&mut a, "/cost");
    assert_eq!(reports(&effects), vec![None]);
    assert_eq!(view_lines(&a), vec![HEADER, READING_LINE]);
    // A store that does read answers with rows, whatever the pane believes.
    a.update(answer(Some(vec![today_row()])));
    assert_eq!(view_lines(&a)[1], "today: 6 tokens · 2 turns");
}

#[test]
fn after_a_failed_create_this_chat_has_no_data() {
    let mut a = conversation();
    ask(&mut a, "hello");
    a.update(AppEvent::Store(StoreReply::Failed {
        pane: chat_id(),
        notify: true,
    }));
    let effects = ask(&mut a, "/cost");
    assert_eq!(reports(&effects), vec![None]);
    let mut row = today_row();
    row.in_chat = false;
    a.update(answer(Some(vec![row])));
    let lines = view_lines(&a);
    let charts = lines
        .iter()
        .position(|l| l.starts_with("per day"))
        .expect("charts shown");
    assert_eq!(lines[charts - 1], "this chat: no data", "{lines:?}");
}

#[test]
fn a_first_reply_arriving_after_esc_and_a_second_cost_ends_with_the_data() {
    let mut a = conversation();
    ask(&mut a, "/cost");
    a.update(plain(KeyCode::Esc));
    ask(&mut a, "/cost");
    a.update(answer(Some(vec![today_row()])));
    a.update(answer(Some(vec![today_row()])));
    assert_eq!(view_lines(&a)[1], "today: 6 tokens · 2 turns");
    assert_eq!(
        view_lines(&a)
            .iter()
            .filter(|l| l.starts_with("today"))
            .count(),
        1
    );
}

#[test]
fn ctrl_c_with_the_overlay_open_says_what_to_do_and_the_stream_goes_on() {
    let mut a = conversation();
    let turn = sent_turn(&ask(&mut a, "question"));
    a.update(delta(chat_id(), turn, "par"));
    ask(&mut a, "/cost");
    a.update(super::chat_history::code(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        chat_of(&a).cost().and_then(|v| v.note()),
        Some(crate::core::ai::chat::cost_view::CLOSE_FIRST)
    );
    assert!(chat_of(&a).cost().is_some());
    a.update(delta(chat_id(), turn, "tial"));
    a.update(finish(chat_id(), turn));
    assert_eq!(chat_of(&a).messages().last().unwrap().text, "partial");
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

fn gemini_row(model: Option<&str>) -> UsageRow {
    UsageRow {
        engine: "gemini".into(),
        model: model.map(str::to_owned),
        ..today_row()
    }
}

fn gemini_chat() -> App {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Chat,
        Assignment {
            engine: Some("gemini".into()),
            model: Some("default".into()),
        },
    );
    let mut a = launched(120, 30).with_ai(ai).with_chat_history();
    press(&mut a, "c");
    a.update(prompt_for(chat_id()));
    listed(&mut a, chat_id(), rows());
    a.update(key('n'));
    a
}

#[test]
fn a_mixed_row_is_shown_as_mixed_and_a_null_one_as_unknown() {
    let mut a = gemini_chat();
    ask(&mut a, "/cost");
    let mut claude = gemini_row(Some("mixed"));
    claude.engine = "claude".into();
    a.update(answer(Some(vec![
        gemini_row(Some("mixed")),
        claude,
        gemini_row(None),
    ])));
    let pairs: Vec<String> = view_lines(&a)
        .into_iter()
        .filter(|l| l.starts_with("  gemini › ") || l.starts_with("  claude › "))
        .collect();
    assert!(
        pairs.iter().any(|l| l.starts_with("  gemini › mixed: ")),
        "{pairs:?}"
    );
    assert!(
        pairs.iter().any(|l| l.starts_with("  claude › mixed: ")),
        "{pairs:?}"
    );
    assert!(
        pairs.iter().any(|l| l.starts_with("  gemini › unknown: ")),
        "{pairs:?}"
    );
}

#[test]
fn the_chat_config_never_relabels_a_stored_row() {
    let mut a = gemini_chat();
    assert_eq!(a.status_right(), "gemini › default");
    ask(&mut a, "/cost");
    let mut worker = gemini_row(None);
    worker.workers = 1;
    a.update(answer(Some(vec![
        worker,
        gemini_row(Some("gemini-2.5-pro")),
    ])));
    let lines = view_lines(&a);
    assert!(
        lines.iter().any(|l| l.starts_with("  gemini › unknown: ")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("  gemini › gemini-2.5-pro: ")),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains("› default")), "{lines:?}");
}

#[test]
fn a_real_turn_with_history_on_shows_this_session_under_the_stored_report() {
    let mut a = conversation();
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
    ask(&mut a, "/cost");
    a.update(answer(Some(vec![today_row()])));
    let lines = view_lines(&a);
    assert!(
        lines.contains(&"this session: 15 tokens · 1 turns".to_owned()),
        "{lines:?}"
    );
}
