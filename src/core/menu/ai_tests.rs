//! Tests of the AI section of the menu, at the `MenuState` level.

use super::*;
use crate::core::ai::{CATALOG, EngineState, Status, config::Assignment};

pub(super) fn engine(
    id: &'static str,
    status: Status,
    models: &[&str],
    version: Option<&str>,
) -> EngineState {
    EngineState {
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: version.map(str::to_owned),
    }
}

/// All eight catalog engines in order; the ones not listed are
/// `NotInstalled`.
pub(super) fn detection(found: &[EngineState]) -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| {
                found
                    .iter()
                    .find(|s| s.id == e.id)
                    .cloned()
                    .unwrap_or_else(|| engine(e.id, Status::NotInstalled, &[], None))
            })
            .collect(),
    }
}

pub(super) fn machine() -> Detection {
    detection(&[
        engine(
            "ollama",
            Status::Available,
            &["llama3:8b", "qwen2:7b"],
            None,
        ),
        engine("lmstudio", Status::NotRunning, &[], None),
        engine("claude", Status::Available, &["default"], Some("2.1.0")),
        engine("codex", Status::Available, &["default"], Some("0.5.0")),
    ])
}

fn saved(engine: Option<&str>, model: Option<&str>) -> AiConfig {
    AiConfig::general(engine, model)
}

pub(super) fn menu(
    engine: Option<&str>,
    model: Option<&str>,
    found: Option<Detection>,
) -> MenuState {
    MenuState::open(&Config::default()).with_ai(saved(engine, model), found)
}

// Spec: Unchanged AI draft, Undetected saved engine survives (edit part).
#[test]
fn an_unchanged_ai_draft_adds_no_edits() {
    for m in [
        menu(Some("ollama"), Some("llama3:8b"), Some(machine())),
        menu(Some("codex"), Some("gone"), Some(detection(&[]))),
        menu(Some("claude"), None, None),
        menu(None, None, Some(machine())),
    ] {
        assert_eq!(m.edits(&Config::default()), vec![]);
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        assert_eq!(
            m.edits(&live),
            vec![ConfigEdit {
                table: "statusline",
                key: "position",
                value: "top".into()
            }]
        );
    }
}

pub(super) fn ch(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

pub(super) fn press(menu: &mut MenuState, c: char) -> MenuCommand {
    menu.on_key(&ch(c), false, &mut Config::default())
}

pub(super) fn selected_label(menu: &MenuState) -> &'static str {
    let labels: Vec<_> = menu
        .rows(&Config::default())
        .iter()
        .filter_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(*label),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 1, "exactly one selected row");
    labels[0]
}

const ORDER: [&str; 10] = [
    "Statusline position",
    "Tab bar position",
    "Set all tasks",
    "general",
    "code",
    "chat",
    "review",
    "summarize",
    "Profile",
    "Context",
];

// Spec: Descriptor integrity, Sections and items are data.
#[test]
fn the_ai_descriptors_are_well_formed() {
    assert_eq!(AI_SECTION.title, "AI");
    let labels: Vec<_> = AI_SECTION.items.iter().map(|i| i.label).collect();
    assert_eq!(labels, ORDER[2..]);
    let mut paths: Vec<&str> = Vec::new();
    for (item, task) in AI_SECTION.items[1..6].iter().zip(Task::ALL) {
        match item.kind {
            ItemKind::Assign(Some(t)) => {
                assert_eq!(t, task);
                assert_eq!(item.label, t.id());
                paths.push(t.table());
            }
            _ => panic!("{}: not a task row", item.label),
        }
    }
    assert!(
        matches!(AI_SECTION.items[0].kind, ItemKind::Assign(None)),
        "Set all tasks has no task and so no key path"
    );
    let mut unique = paths.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), paths.len(), "duplicate key path");
    assert_eq!(
        paths,
        [
            "ai",
            "ai.tasks.code",
            "ai.tasks.chat",
            "ai.tasks.review",
            "ai.tasks.summarize"
        ]
    );
}

// Spec: Task rows in order, Wrap, Disabled row is skipped.
#[test]
fn navigation_visits_settings_then_the_six_ai_rows_and_wraps() {
    let mut m = menu(None, None, Some(machine()));
    let mut visited = vec![selected_label(&m)];
    for _ in 0..10 {
        press(&mut m, 'j');
        visited.push(selected_label(&m));
    }
    assert_eq!(visited[..10], ORDER);
    assert_eq!(visited[10], "Statusline position", "wraps past Context");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Context", "wraps before the first");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Profile");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "summarize");
    let mut back = Vec::new();
    for _ in 0..7 {
        press(&mut m, 'k');
        back.push(selected_label(&m));
    }
    assert_eq!(
        back,
        [
            "review",
            "chat",
            "code",
            "general",
            "Set all tasks",
            "Tab bar position",
            "Statusline position"
        ]
    );
}

#[test]
fn the_mouse_row_is_skipped_in_both_directions() {
    let mut m = menu(None, None, None);
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(selected_label(&m), "Set all tasks");
    press(&mut m, 'k');
    assert_eq!(selected_label(&m), "Tab bar position");
}

// Spec: Repeat moves.
#[test]
fn repeat_moves_into_the_task_rows_like_a_press() {
    let mut m = menu(None, None, None);
    for _ in 0..3 {
        m.on_key(&ch('j'), true, &mut Config::default());
    }
    assert_eq!(selected_label(&m), "general");
}

// Spec: Cycling keys do nothing on task rows.
#[test]
fn cycling_keys_do_nothing_on_task_rows() {
    let mut m = menu(Some("ollama"), Some("llama3:8b"), Some(machine()));
    for _ in 0..4 {
        press(&mut m, 'j');
    }
    assert_eq!(selected_label(&m), "code");
    let before = m.clone();
    for key in [
        ch('h'),
        ch('l'),
        ch(' '),
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    ] {
        let mut live = Config::default();
        assert_eq!(m.on_key(&key, false, &mut live), MenuCommand::None);
        assert_eq!(live, Config::default());
    }
    assert_eq!(m, before);
}

// Spec: Refresh key on a task row (menu part).
#[test]
fn r_on_a_task_row_asks_for_a_refresh_and_elsewhere_does_not() {
    let mut m = menu(None, None, Some(machine()));
    assert_eq!(press(&mut m, 'r'), MenuCommand::None, "Statusline row");
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh, "Set all tasks");
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(press(&mut m, 'r'), MenuCommand::Refresh, "code");
    assert_eq!(
        m.on_key(&ch('r'), true, &mut Config::default()),
        MenuCommand::None,
        "repeat"
    );
}

pub(super) fn assigned(entries: &[(Task, &str, &str)], found: Option<Detection>) -> MenuState {
    let mut ai = AiConfig::default();
    for (task, engine, model) in entries {
        ai.set(
            *task,
            Assignment {
                engine: Some((*engine).to_owned()),
                model: Some((*model).to_owned()),
            },
        );
    }
    MenuState::open(&Config::default()).with_ai(ai, found)
}

pub(super) fn value_of(menu: &MenuState, label: &str) -> String {
    menu.rows(&Config::default())
        .iter()
        .find_map(|row| match row {
            Row::Item {
                label: l, value, ..
            } if *l == label => value.as_deref().map(str::to_owned),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {label}"))
}

fn ai_rows(menu: &MenuState) -> Vec<(&'static str, String)> {
    let rows = menu.rows(&Config::default());
    let Some(at) = rows.iter().position(|r| *r == Row::Title("AI".into())) else {
        panic!("no AI title");
    };
    rows[at + 1..]
        .iter()
        .map(|row| match row {
            Row::Item {
                label,
                value,
                enabled,
                ..
            } => {
                assert!(enabled, "{label} is selectable");
                (*label, value.as_deref().unwrap().to_owned())
            }
            other => panic!("not an item: {other:?}"),
        })
        .collect()
}

// Spec: Settings rows (AI part): six rows, no Engine/Model/Status.
#[test]
fn the_ai_section_lists_the_task_rows_and_the_profile_with_their_values() {
    let m = assigned(&[(Task::General, "ollama", "llama3:8b")], Some(machine()));
    let got: Vec<_> = ai_rows(&m)
        .iter()
        .map(|(l, v)| format!("{l}={v}"))
        .collect();
    assert_eq!(
        got,
        [
            "Set all tasks=ollama › llama3:8b",
            "general=ollama › llama3:8b",
            "code=(default)",
            "chat=(default)",
            "review=(default)",
            "summarize=(default)",
            "Profile=programmer",
            "Context=on",
        ]
    );
    assert_eq!(
        MenuState::open(&Config::default())
            .rows(&Config::default())
            .len(),
        4,
        "no AI section without AI"
    );
}

// Spec: Aggregate values, Nothing assigned.
#[test]
fn unset_and_mixed_values() {
    let m = menu(None, None, None);
    assert_eq!(value_of(&m, "general"), "(not set)");
    assert_eq!(value_of(&m, "Set all tasks"), "(not set)");
    let mixed = assigned(
        &[
            (Task::General, "claude", "opus"),
            (Task::Code, "ollama", "llama3:8b"),
        ],
        Some(machine()),
    );
    assert_eq!(value_of(&mixed, "Set all tasks"), "(mixed)");
    assert_eq!(value_of(&mixed, "code"), "ollama › llama3:8b");
    assert_eq!(value_of(&mixed, "chat"), "(default)");
}
// Spec: Row marks.
#[test]
fn rows_carry_download_and_not_detected_marks() {
    let m = assigned(
        &[
            (Task::General, "ollama", "llama3:8b"),
            (Task::Code, "ollama", "gone:7b"),
            (Task::Chat, "claude", "opus"),
            (Task::Review, "codex", "default"),
            (Task::Summarize, "lmstudio", "m"),
        ],
        Some(detection(&[
            engine("ollama", Status::Available, &["llama3:8b"], None),
            engine("claude", Status::Available, &["default"], None),
            engine("lmstudio", Status::NotRunning, &[], None),
        ])),
    );
    assert_eq!(value_of(&m, "general"), "ollama › llama3:8b");
    assert_eq!(value_of(&m, "code"), "ollama › gone:7b · download required");
    assert_eq!(value_of(&m, "chat"), "claude › opus", "CLI: no model mark");
    assert_eq!(value_of(&m, "review"), "codex (not detected) › default");
    assert_eq!(value_of(&m, "summarize"), "lmstudio (not detected) › m");
}

// Spec: Before first detection.
#[test]
fn nothing_is_marked_before_the_first_detection() {
    let m = assigned(
        &[
            (Task::General, "ollama", "llama3:8b"),
            (Task::Code, "codex", "default"),
        ],
        None,
    );
    assert_eq!(value_of(&m, "general"), "ollama › llama3:8b");
    assert_eq!(value_of(&m, "code"), "codex › default");
}

// Spec: Result arrives on the task list.
#[test]
fn a_result_marks_the_row_in_place_and_keeps_selection_and_draft() {
    let mut m = assigned(&[(Task::Code, "ollama", "llama3:8b")], None);
    for _ in 0..4 {
        press(&mut m, 'j');
    }
    assert_eq!(value_of(&m, "code"), "ollama › llama3:8b");
    let draft = m.ai_draft();
    m.set_engines(Some(detection(&[engine(
        "ollama",
        Status::Available,
        &["qwen2.5:7b"],
        None,
    )])));
    assert_eq!(
        value_of(&m, "code"),
        "ollama › llama3:8b · download required"
    );
    assert_eq!(selected_label(&m), "code");
    assert_eq!(m.ai_draft(), draft);
}

// Spec: Engine disappears while open.
#[test]
fn a_vanished_engine_stays_assigned_and_reads_not_detected() {
    let mut m = assigned(&[(Task::Code, "ollama", "llama3:8b")], Some(machine()));
    assert_eq!(value_of(&m, "code"), "ollama › llama3:8b");
    let draft = m.ai_draft();
    m.set_engines(Some(detection(&[engine(
        "ollama",
        Status::NotRunning,
        &[],
        None,
    )])));
    assert_eq!(value_of(&m, "code"), "ollama (not detected) › llama3:8b");
    assert_eq!(m.ai_draft(), draft);
}
