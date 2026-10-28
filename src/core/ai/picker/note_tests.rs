//! The wrapped, copyable note of a failed model listing and the copyable
//! no-jail reason of an engine row.

use super::machine_tests::{engine_key, index_of, state};
use super::*;

fn codex(models_why: Option<&str>, jail: Option<&str>) -> Detection {
    let mut codex = state("codex", Status::Available, &["default"]);
    codex.diagnostics.models = models_why.map(str::to_owned);
    codex.diagnostics.jail = jail.map(str::to_owned);
    Detection {
        engines: vec![codex],
        ..Default::default()
    }
}

fn notes(rows: &[PickRow]) -> Vec<&PickRow> {
    rows.iter()
        .filter(|r| matches!(r.key, PickKey::Note { .. }))
        .collect()
}

const LONG: &str = "jail: no node found to run the CLI script; install it next to the CLI or on PATH (looked in /usr/local/bin and /usr/bin)";

#[test]
fn a_short_reason_is_one_row_and_a_long_one_wraps_within_the_width() {
    let short = codex(Some("codex exited 1: not logged in"), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&short));
    let note = notes(&rows);
    assert_eq!(note.len(), 1);
    assert_eq!(
        note[0].label,
        "model list unavailable: codex exited 1: not logged in"
    );

    let long = codex(Some(LONG), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&long));
    let note = notes(&rows);
    assert!(note.len() > 1 && note.len() <= NOTE_ROWS, "{}", note.len());
    for r in &note {
        assert!(r.label.chars().count() <= NOTE_WIDTH, "{:?}", r.label);
        assert!(!r.enabled && r.mark == Mark::Plain && !r.current);
    }
    let joined: Vec<&str> = note.iter().map(|r| r.label.as_str()).collect();
    assert!(
        joined
            .join(" ")
            .contains("no node found to run the CLI script")
    );
    assert!(!joined.join(" ").contains('…'), "{joined:?}");
}

#[test]
fn every_wrapped_row_carries_the_whole_sanitized_reason() {
    let d = codex(Some(&format!("{LONG}\u{1b}[31m\n\u{202e}")), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
    let full = format!("model list unavailable: {LONG}");
    for r in notes(&rows) {
        assert!(matches!(&r.key, PickKey::Note { text, .. } if *text == full));
    }
}

#[test]
fn a_huge_reason_is_bounded_to_four_rows_and_the_last_is_clipped() {
    let d = codex(Some(&"word ".repeat(200)), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
    let note = notes(&rows);
    assert_eq!(note.len(), NOTE_ROWS);
    assert!(note[NOTE_ROWS - 1].label.ends_with('…'));
    assert!(note[NOTE_ROWS - 1].label.chars().count() <= NOTE_WIDTH);
}

#[test]
fn a_word_longer_than_the_row_is_broken() {
    let d = codex(Some(&"x".repeat(100)), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
    for r in notes(&rows) {
        assert!(r.label.chars().count() <= NOTE_WIDTH);
    }
}

fn at_codex_models(d: &Detection) -> (Picker, AiConfig) {
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(d));
    let rows = p.rows(&draft, Some(d));
    p.cursor = index_of(&rows, engine_key("codex"));
    assert_eq!(p.enter(&draft, Some(d)), Outcome::Stay);
    (p, draft)
}

#[test]
fn enter_and_y_on_any_note_row_copy_the_full_reason_and_nothing_else() {
    let d = codex(Some(LONG), None);
    let (mut p, draft) = at_codex_models(&d);
    let rows = p.rows(&draft, Some(&d));
    let full = format!("model list unavailable: {LONG}");
    for (i, r) in rows.iter().enumerate() {
        if !matches!(r.key, PickKey::Note { .. }) {
            continue;
        }
        p.cursor = i;
        assert_eq!(p.enter(&draft, Some(&d)), Outcome::Copy(full.clone()));
        assert_eq!(p.copy(&draft, Some(&d)), Outcome::Copy(full.clone()));
        assert!(matches!(p.level, Level::Models { .. }), "still in the list");
    }
}

#[test]
fn wrapped_rows_have_distinct_keys_and_a_refresh_keeps_the_cursor_on_its_row() {
    let d = codex(Some(&"word ".repeat(40)), None);
    let (mut p, draft) = at_codex_models(&d);
    let rows = p.rows(&draft, Some(&d));
    let first = rows
        .iter()
        .position(|r| matches!(r.key, PickKey::Note { .. }))
        .unwrap();
    let keys: Vec<_> = rows.iter().map(|r| &r.key).collect();
    for (i, k) in keys.iter().enumerate() {
        assert!(keys.iter().skip(i + 1).all(|o| o != k), "duplicate key");
    }
    p.cursor = first + 2;
    let previous = p.key(&draft, Some(&d));
    p.reanchor(previous, &draft, Some(&d));
    assert_eq!(p.cursor(), first + 2);
}

#[test]
fn wide_characters_are_wrapped_by_display_width() {
    use unicode_width::UnicodeWidthStr;
    let d = codex(Some(&"漢".repeat(60)), None);
    let rows = model_rows("codex", Target::All, &AiConfig::default(), Some(&d));
    for r in notes(&rows) {
        assert!(r.label.width() <= NOTE_WIDTH, "{:?}", r.label);
    }
}

#[test]
fn a_note_row_in_the_custom_level_copies_nothing() {
    let d = codex(Some(LONG), None);
    let (mut p, draft) = at_codex_models(&d);
    let rows = p.rows(&draft, Some(&d));
    p.cursor = rows
        .iter()
        .position(|r| matches!(r.key, PickKey::Note { .. }))
        .unwrap();
    p.level = Level::Custom {
        engine: "codex".to_owned(),
        text: String::new(),
    };
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
}

#[test]
fn y_on_an_engine_row_that_is_not_available_copies_nothing() {
    let mut gone = state("codex", Status::NotInstalled, &[]);
    gone.diagnostics.jail = Some(LONG.to_owned());
    let d = Detection {
        engines: vec![gone],
        ..Default::default()
    };
    let mut draft = AiConfig::default();
    draft.set(
        Task::Code,
        Assignment {
            engine: Some("codex".to_owned()),
            model: None,
        },
    );
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, engine_key("codex"));
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
}

#[test]
fn y_on_a_model_or_custom_row_copies_nothing() {
    let d = codex(Some(LONG), None);
    let (mut p, draft) = at_codex_models(&d);
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, PickKey::Custom);
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
    p.cursor = 0;
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
}

#[test]
fn y_on_an_engine_row_copies_its_no_jail_reason_and_enter_still_enters() {
    let d = codex(None, Some(LONG));
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, engine_key("codex"));
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Copy(LONG.to_owned()));
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert!(matches!(p.level, Level::Models { .. }));
}

#[test]
fn y_on_an_engine_row_without_a_reason_or_on_none_copies_nothing() {
    let d = codex(None, None);
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    let rows = p.rows(&draft, Some(&d));
    p.cursor = index_of(&rows, engine_key("codex"));
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
    p.cursor = index_of(&rows, PickKey::Clear);
    assert_eq!(p.copy(&draft, Some(&d)), Outcome::Stay);
}
