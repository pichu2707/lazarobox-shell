//! Picking and saving local llama.cpp models: edge cases found by the
//! independent verifier (llamacpp-pick-model S2-S3).

use std::path::PathBuf;

use super::ai_tests::{detection, engine, press};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::config::read_with_port_env;
use crate::core::ai::{EngineState, Status};
use crate::core::config_edit::apply_edit;

fn found(models: &[&str], extra: &[EngineState]) -> Option<Detection> {
    let mut list = vec![engine("llamacpp", Status::NotRunning, &[], None)];
    list.extend_from_slice(extra);
    let mut d = detection(&list);
    d.local_models = models.iter().map(PathBuf::from).collect();
    Some(d)
}

fn ai_of(text: &str) -> AiConfig {
    let table: toml::Table = text.parse().unwrap();
    let mut problems = Vec::new();
    let ai = read_with_port_env(&table, &mut problems, None);
    assert_eq!(problems, Vec::<String>::new(), "{text}");
    ai
}

fn menu_of(text: &str, d: Option<Detection>) -> MenuState {
    MenuState::open(&Config::default()).with_ai(ai_of(text), d)
}

/// Opens `task`, moves to `id` and presses Enter on its first model.
fn choose(m: &mut MenuState, task: &str, id: &str) {
    open(m, task);
    for _ in 0..20 {
        if cursor_label(m) == id {
            break;
        }
        press(m, 'j');
    }
    assert_eq!(cursor_label(m), id);
    send(m, key(KeyCode::Enter));
    send(m, key(KeyCode::Enter));
}

fn server_edits(m: &MenuState) -> Vec<ConfigEdit> {
    m.edits(&Config::default())
        .into_iter()
        .filter(|e| e.table == "ai.servers.llamacpp")
        .collect()
}

fn ollama() -> EngineState {
    engine("ollama", Status::Available, &["llama3:8b"], None)
}

#[test]
fn another_engine_with_local_models_never_touches_the_servers() {
    let mut m = menu_of("", found(&["/m/a.gguf"], &[ollama()]));
    choose(&mut m, "chat", "ollama");
    assert_eq!(m.ai_draft().label(Task::Chat), "ollama › llama3:8b");
    assert_eq!(server_edits(&m), vec![]);
    assert_eq!(m.ai_draft().managed(), None);
}

// The user picks a local model, then changes their mind and picks another
// engine for the same task before saving.
#[test]
fn a_local_model_replaced_by_another_engine_before_saving() {
    let mut m = menu_of("", found(&["/m/a.gguf"], &[ollama()]));
    choose(&mut m, "chat", "llamacpp");
    choose(&mut m, "chat", "ollama");
    assert_eq!(m.ai_draft().label(Task::Chat), "ollama › llama3:8b");
    // No task uses llamacpp any more: nothing about the managed server
    // should be saved, and nothing should start.
    assert_eq!(
        server_edits(&m),
        vec![],
        "draft {:?}",
        m.ai_draft().managed()
    );
}

// Two tasks get two different local models: the server can load one only.
#[test]
fn two_tasks_with_two_local_models() {
    let mut m = menu_of("", found(&["/m/a.gguf", "/m/b.gguf"], &[]));
    choose(&mut m, "chat", "llamacpp");
    open(&mut m, "code");
    for _ in 0..20 {
        if cursor_label(&m) == "llamacpp" {
            break;
        }
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, 'j');
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(draft.label(Task::Code), "llamacpp › /m/b.gguf");
    assert_eq!(draft.label(Task::Chat), "llamacpp › /m/b.gguf");
    assert_eq!(
        draft.managed().map(|s| s.model.clone()),
        Some(PathBuf::from("/m/b.gguf"))
    );
}

#[test]
fn unrelated_keys_tables_and_comments_survive_byte_for_byte() {
    let before = "\
# top comment
mouse = true

[statusline]
position = \"top\" # mine

[ai.servers.llamacpp]
# my llama
port = 39080 # keep
ctx = 4096

[ai.servers.vllm]
port = 8001 # vllm
";
    let mut m = menu_of(before, found(&["/m/a.gguf"], &[]));
    choose(&mut m, "chat", "llamacpp");
    let after = apply_edit(Some(before), &server_edits(&m)).unwrap();
    for line in before.lines() {
        assert!(
            after.lines().any(|l| l == line),
            "lost {line:?} in\n{after}"
        );
    }
    let added: Vec<_> = after
        .lines()
        .filter(|l| !before.lines().any(|b| b == *l))
        .collect();
    assert_eq!(
        added,
        ["managed = true", "model = \"/m/a.gguf\""],
        "{after}"
    );
    let ai = ai_of(&after);
    assert_eq!(ai.servers().port("llamacpp"), Some(39080));
    assert_eq!(ai.servers().port("vllm"), Some(8001));
    assert_eq!(ai.managed().unwrap().model, PathBuf::from("/m/a.gguf"));
}

#[test]
fn an_off_managed_and_old_model_are_replaced_in_their_slots() {
    let before = "\
[ai.servers.llamacpp]
managed = false # off for now
model = \"/old.gguf\" # old one
port = 39080
";
    let mut m = menu_of(before, found(&["/m/a.gguf"], &[]));
    choose(&mut m, "chat", "llamacpp");
    let after = apply_edit(Some(before), &server_edits(&m)).unwrap();
    assert_eq!(
        after,
        "\
[ai.servers.llamacpp]
managed = true # off for now
model = \"/m/a.gguf\" # old one
port = 39080
"
    );
    assert_eq!(m.ai_draft().servers().port("llamacpp"), Some(39080));
}

#[test]
fn inline_and_dotted_tables_read_back() {
    for before in [
        "[ai.servers]\nllamacpp = { port = 39080 }\n",
        "[ai]\nservers.llamacpp.port = 39080\n",
    ] {
        let mut m = menu_of(before, found(&["/m/a.gguf"], &[]));
        choose(&mut m, "chat", "llamacpp");
        let after = apply_edit(Some(before), &m.edits(&Config::default())).unwrap();
        let ai = ai_of(&after);
        assert_eq!(ai.servers().port("llamacpp"), Some(39080), "{after}");
        assert_eq!(
            ai.managed().map(|s| s.model.clone()),
            Some(PathBuf::from("/m/a.gguf")),
            "{after}"
        );
    }
}

#[test]
fn a_path_with_quotes_backslashes_and_a_newline_round_trips() {
    let path = "/m/we\"ird \\ 'name'\n#x.gguf";
    let mut m = menu_of("", found(&[path], &[]));
    choose(&mut m, "chat", "llamacpp");
    let after = apply_edit(None, &m.edits(&Config::default())).unwrap();
    let ai = ai_of(&after);
    assert_eq!(ai.managed().unwrap().model, PathBuf::from(path), "{after}");
    assert_eq!(ai.label(Task::Chat), format!("llamacpp › {path}"));
}

#[test]
fn no_local_models_keeps_the_start_note() {
    let mut m = menu_of("", found(&[], &[]));
    open(&mut m, "chat");
    for _ in 0..20 {
        if cursor_label(&m) == "llamacpp" {
            break;
        }
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    let rows = format!("{:?}", m.rows(&Config::default()));
    assert!(rows.contains("not running"), "{rows}");
    assert_eq!(server_edits(&m), vec![]);
}
