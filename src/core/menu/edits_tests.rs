//! Tests of `edits()` with an AI draft: Settings first, then the changed
//! tasks in `Task::ALL` order, both keys of each.

use super::ai_tests::{ch, machine, menu, press};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::config::Assignment;

/// Picks the first model of claude for the task row `label`.
fn assign_claude(m: &mut MenuState, label: &str) {
    open(m, label);
    press(m, 'j');
    press(m, 'j');
    send(m, key(KeyCode::Enter));
    send(m, key(KeyCode::Enter));
}

fn edit(table: &'static str, key: &'static str, value: &'static str) -> ConfigEdit {
    ConfigEdit {
        table,
        key,
        value: value.into(),
    }
}

// Spec: a changed task writes both keys under its own table.
#[test]
fn a_changed_task_writes_engine_and_model_under_its_table() {
    let mut m = menu(None, None, Some(machine()));
    assign_claude(&mut m, "code");
    assert_eq!(
        m.edits(&Config::default()),
        vec![
            edit("ai.tasks.code", "engine", "claude"),
            edit("ai.tasks.code", "model", "default"),
        ]
    );
}

// Spec: Settings first, then the changed tasks in `Task::ALL` order.
#[test]
fn settings_come_first_then_the_changed_tasks_in_task_order() {
    let mut m = menu(None, None, Some(machine()));
    assign_claude(&mut m, "summarize");
    assign_claude(&mut m, "chat");
    let mut live = Config::default();
    live.bars.statusline = BarPosition::Top;
    let tables: Vec<_> = m.edits(&live).iter().map(|e| (e.table, e.key)).collect();
    assert_eq!(
        tables,
        vec![
            ("statusline", "position"),
            ("ai.tasks.chat", "engine"),
            ("ai.tasks.chat", "model"),
            ("ai.tasks.summarize", "engine"),
            ("ai.tasks.summarize", "model"),
        ]
    );
}

// Spec: a task set back to unset is written as empty strings.
#[test]
fn a_task_cleared_from_a_saved_value_writes_empty_strings() {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Code,
        Assignment {
            engine: Some("claude".into()),
            model: Some("default".into()),
        },
    );
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(machine()));
    open(&mut m, "code");
    while cursor_label(&m) != "(default · use general)" {
        press(&mut m, 'k');
    }
    send(&mut m, key(KeyCode::Enter));
    assert_eq!(
        m.edits(&Config::default()),
        vec![
            edit("ai.tasks.code", "engine", ""),
            edit("ai.tasks.code", "model", ""),
        ]
    );
}

// ---- Characterization: the output of `edits()` -------------------------------
//
// These tests pin the exact `Vec<ConfigEdit>` and the TOML `apply_edit`
// writes from an empty file, row by row. They are the safety net of the
// descriptor refactors that follow the tab split: they must pass untouched.

use crate::core::ai::Status;
use crate::core::ai::config::MemoryConfig;
use crate::core::config_edit::apply_edit;
use std::path::PathBuf;

/// The machine of the other tests, with a stopped llama-server and one local
/// model, so a task can be given to `llamacpp` and turn its managed server on.
fn with_local_model() -> Detection {
    let mut d = machine();
    d.engines
        .iter_mut()
        .find(|e| e.id == "llamacpp")
        .expect("llamacpp is in the catalog")
        .status = Status::NotRunning;
    d.local_models = vec![PathBuf::from("/m/a.gguf")];
    d
}

/// A saved AI config with an off-list idle stop (45 is not a step) and a
/// memory that was hand-edited to `enabled = true` without its
/// acknowledgement.
fn saved() -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set_idle_minutes(45);
    ai.memory = MemoryConfig {
        enabled: true,
        acknowledged: false,
        port: 7437,
        save_summaries: false,
    };
    ai
}

fn opened() -> MenuState {
    MenuState::open(&Config::default()).with_ai(saved(), Some(with_local_model()))
}

/// Moves to `label` and presses `keys` on it; the live settings persist in
/// `live` like they do in the app.
fn change(m: &mut MenuState, live: &mut Config, label: &str, keys: &[char]) {
    m.goto(label);
    for c in keys {
        m.on_key(&ch(*c), false, live);
    }
}

/// Gives the task row `label` its first model of `engine`.
fn give(m: &mut MenuState, label: &str, engine: &str) {
    open(m, label);
    for _ in 0..20 {
        if cursor_label(m) == engine {
            break;
        }
        press(m, 'j');
    }
    assert_eq!(cursor_label(m), engine);
    send(m, key(KeyCode::Enter));
    send(m, key(KeyCode::Enter));
}

/// One changed row of every kind, in a fixed visit order.
fn every_kind(m: &mut MenuState, live: &mut Config) {
    change(m, live, "Statusline position", &['l']);
    change(m, live, "Mouse", &['l']);
    give(m, "code", "claude");
    give(m, "chat", "llamacpp");
    change(m, live, "Profile", &['l']);
    change(m, live, "Context", &['l']);
    change(m, live, "Memory", &['l', 'y']);
    change(m, live, "Notify", &['l']);
    change(m, live, "Sound", &['l']);
    change(m, live, "Tools", &['l']);
    change(m, live, "Search", &['l']);
    change(m, live, "Git write", &['l']);
    change(m, live, "Approval", &['l']);
    change(m, live, "Orchestrator", &['l']);
    change(m, live, "llama-server idle", &['l']);
}

fn pairs(edits: &[ConfigEdit]) -> Vec<(&'static str, &'static str, String)> {
    edits
        .iter()
        .map(|e| (e.table, e.key, e.value.to_string()))
        .collect()
}

#[test]
fn every_kind_of_row_writes_its_edit_in_the_fixed_save_order() {
    let mut m = opened();
    let mut live = Config::default();
    every_kind(&mut m, &mut live);
    assert_eq!(
        pairs(&m.edits(&live)),
        vec![
            ("statusline", "position", "top".to_owned()),
            ("", "mouse", "false".to_owned()),
            ("ai.tasks.code", "engine", "claude".to_owned()),
            ("ai.tasks.code", "model", "default".to_owned()),
            ("ai.tasks.chat", "engine", "llamacpp".to_owned()),
            ("ai.tasks.chat", "model", "/m/a.gguf".to_owned()),
            ("ai", "profile", "analyst".to_owned()),
            ("ai", "context", "false".to_owned()),
            ("ai.memory", "acknowledged", "true".to_owned()),
            ("ai", "notify", "auto".to_owned()),
            ("ai", "notify_sound", "false".to_owned()),
            ("ai.tools", "enabled", "false".to_owned()),
            ("ai.tools", "search", "true".to_owned()),
            ("ai.tools", "git_write", "true".to_owned()),
            ("ai.tools", "approval", "auto".to_owned()),
            ("ai.orchestrator", "enabled", "true".to_owned()),
            ("ai.servers.llamacpp", "idle_minutes", "0".to_owned()),
            ("ai.servers.llamacpp", "managed", "true".to_owned()),
            ("ai.servers.llamacpp", "model", "/m/a.gguf".to_owned()),
        ]
    );
}

#[test]
fn every_kind_of_row_writes_this_toml_from_an_empty_file() {
    let mut m = opened();
    let mut live = Config::default();
    every_kind(&mut m, &mut live);
    let text = apply_edit(None, &m.edits(&live)).expect("the edits apply to an empty file");
    assert_eq!(
        text,
        r#"mouse = false
[statusline]
position = "top"

[ai]
profile = "analyst"
context = false
notify = "auto"
notify_sound = false

[ai.tasks.code]
engine = "claude"
model = "default"

[ai.tasks.chat]
engine = "llamacpp"
model = "/m/a.gguf"

[ai.memory]
acknowledged = true

[ai.tools]
enabled = false
search = true
git_write = true
approval = "auto"

[ai.orchestrator]
enabled = true

[ai.servers.llamacpp]
idle_minutes = 0
managed = true
model = "/m/a.gguf"
"#
    );
}

#[test]
fn the_edits_do_not_depend_on_the_order_the_rows_were_visited() {
    let mut forward = opened();
    let mut live = Config::default();
    every_kind(&mut forward, &mut live);
    let mut backward = opened();
    let mut other = Config::default();
    change(&mut backward, &mut other, "llama-server idle", &['l']);
    change(&mut backward, &mut other, "Orchestrator", &['l']);
    change(&mut backward, &mut other, "Approval", &['l']);
    change(&mut backward, &mut other, "Git write", &['l']);
    change(&mut backward, &mut other, "Search", &['l']);
    change(&mut backward, &mut other, "Tools", &['l']);
    change(&mut backward, &mut other, "Sound", &['l']);
    change(&mut backward, &mut other, "Notify", &['l']);
    change(&mut backward, &mut other, "Memory", &['l', 'y']);
    change(&mut backward, &mut other, "Context", &['l']);
    change(&mut backward, &mut other, "Profile", &['l']);
    give(&mut backward, "chat", "llamacpp");
    give(&mut backward, "code", "claude");
    change(&mut backward, &mut other, "Mouse", &['l']);
    change(&mut backward, &mut other, "Statusline position", &['l']);
    assert_eq!(forward.edits(&live), backward.edits(&other));
}

#[test]
fn visiting_every_tab_without_changing_a_row_writes_nothing() {
    let mut m = opened();
    let mut live = Config::default();
    for label in [
        "Feedback",
        "Hugging Face",
        "orchestrate",
        "Orchestrator",
        "Memory",
        "Statusline position",
    ] {
        m.goto(label);
    }
    for _ in 0..8 {
        m.on_key(&key(KeyCode::Tab), false, &mut live);
        m.on_key(&ch('j'), false, &mut live);
    }
    assert_eq!(m.edits(&live), vec![]);
}

#[test]
fn a_change_on_one_tab_writes_only_that_edit() {
    let mut m = opened();
    let mut live = Config::default();
    change(&mut m, &mut live, "Context", &['l']);
    for label in ["Mouse", "code", "Search", "Statusline position"] {
        m.goto(label);
    }
    assert_eq!(
        pairs(&m.edits(&live)),
        vec![("ai", "context", "false".to_owned())]
    );
}
