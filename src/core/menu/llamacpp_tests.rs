//! Choosing a local model for a stopped llama-server (spec S3 of
//! llamacpp-pick-model): the menu turns lazarobox's own server on.

use std::borrow::Cow;
use std::path::PathBuf;

use super::ai_tests::{detection, engine, menu, press};
use super::picker_tests::{cursor_label, key, open, send};
use super::*;
use crate::core::ai::Status;
use crate::core::ai::config::{MANAGED_PORT, ManagedServer};
use crate::core::config_edit::apply_edit;

fn stopped_llamacpp(models: &[&str]) -> Option<Detection> {
    let mut d = detection(&[engine("llamacpp", Status::NotRunning, &[], None)]);
    d.local_models = models.iter().map(PathBuf::from).collect();
    Some(d)
}

/// Opens `task`, goes to `engine` and presses Enter on its first model.
fn choose(m: &mut MenuState, task: &str, engine: &str) {
    open(m, task);
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

/// Opens the chat task, goes to llamacpp and presses Enter on its first model.
fn choose_first_model(m: &mut MenuState) {
    choose(m, "chat", "llamacpp");
}

fn server_edits(m: &MenuState) -> Vec<ConfigEdit> {
    m.edits(&Config::default())
        .into_iter()
        .filter(|e| e.table == "ai.servers.llamacpp")
        .collect()
}

fn server_edit(key: &'static str, value: &str) -> ConfigEdit {
    ConfigEdit {
        table: "ai.servers.llamacpp",
        key,
        value: Cow::Owned(value.to_owned()),
    }
}

#[test]
fn choosing_a_local_model_turns_the_managed_server_on() {
    let mut m = menu(None, None, stopped_llamacpp(&["/m/a.gguf", "/m/b.gguf"]));
    choose_first_model(&mut m);
    let draft = m.ai_draft();
    assert_eq!(draft.label(Task::Chat), "llamacpp › /m/a.gguf");
    assert_eq!(
        draft.managed(),
        Some(&ManagedServer {
            model: PathBuf::from("/m/a.gguf")
        })
    );
    assert_eq!(draft.servers().port("llamacpp"), Some(MANAGED_PORT));
    let edits = m.edits(&Config::default());
    assert_eq!(
        edits[edits.len() - 2..],
        [
            server_edit("managed", "true"),
            server_edit("model", "/m/a.gguf"),
        ]
    );
    assert!(edits[edits.len() - 2].is_flag());
    assert!(!edits[edits.len() - 1].is_flag());
}

#[test]
fn the_saved_table_reads_back_as_managed_and_keeps_its_other_keys() {
    let mut m = menu(None, None, stopped_llamacpp(&["/m/my model.gguf"]));
    choose_first_model(&mut m);
    let before = "[ai.servers.llamacpp]\nport = 39080 # mine\n";
    let text = apply_edit(Some(before), &m.edits(&Config::default())).unwrap();
    assert!(text.contains("port = 39080 # mine"), "{text}");
    assert!(text.contains("managed = true"), "{text}");
    let table: toml::Table = text.parse().unwrap();
    let mut problems = Vec::new();
    let ai = crate::core::ai::config::read_with_port_env(&table, &mut problems, None);
    assert_eq!(problems, Vec::<String>::new());
    assert_eq!(
        ai.managed().map(|s| s.model.clone()),
        Some(PathBuf::from("/m/my model.gguf"))
    );
    assert_eq!(ai.servers().port("llamacpp"), Some(39080));
}

#[test]
fn a_model_of_a_running_server_changes_nothing_about_managing_it() {
    let mut d = detection(&[engine("llamacpp", Status::Available, &["served"], None)]);
    d.local_models = vec![PathBuf::from("/m/a.gguf")];
    let mut m = menu(None, None, Some(d));
    choose_first_model(&mut m);
    assert_eq!(m.ai_draft().label(Task::Chat), "llamacpp › served");
    assert_eq!(m.ai_draft().managed(), None);
    assert!(
        m.edits(&Config::default())
            .iter()
            .all(|e| e.table != "ai.servers.llamacpp"),
    );
}

#[test]
fn choosing_the_model_already_managed_writes_no_server_keys() {
    let ai = AiConfig::default().with_managed(ManagedServer {
        model: PathBuf::from("/m/a.gguf"),
    });
    let mut m = MenuState::open(&Config::default()).with_ai(ai, stopped_llamacpp(&["/m/a.gguf"]));
    choose_first_model(&mut m);
    assert!(
        m.edits(&Config::default())
            .iter()
            .all(|e| e.table != "ai.servers.llamacpp"),
    );
}

// Verifier blocker 1: a local model the user then replaced is not saved.
#[test]
fn a_local_model_replaced_before_saving_leaves_the_server_alone() {
    let mut d = detection(&[
        engine("llamacpp", Status::NotRunning, &[], None),
        engine("ollama", Status::Available, &["llama3:8b"], None),
    ]);
    d.local_models = vec![PathBuf::from("/m/a.gguf")];
    let mut m = menu(None, None, Some(d));
    choose(&mut m, "chat", "llamacpp");
    choose(&mut m, "chat", "ollama");
    assert_eq!(m.ai_draft().label(Task::Chat), "ollama › llama3:8b");
    assert_eq!(m.ai_draft().managed(), None);
    assert_eq!(m.ai_draft().servers().port("llamacpp"), Some(8080));
    assert_eq!(server_edits(&m), vec![]);
}

// Verifier blocker 1: it is not saved either when general, which the task
// inherits, still points at it.
#[test]
fn a_local_model_inherited_through_general_stays_managed() {
    let mut m = menu(None, None, stopped_llamacpp(&["/m/a.gguf"]));
    choose(&mut m, "general", "llamacpp");
    assert_eq!(server_edits(&m).len(), 2);
}

// Verifier blocker 2 (user: "Pasan al nuevo"): llama-server loads one model,
// so a second local pick moves every task on a local model to it.
#[test]
fn a_second_local_model_moves_the_tasks_on_the_first_one_to_it() {
    let mut m = menu(None, None, stopped_llamacpp(&["/m/a.gguf", "/m/b.gguf"]));
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
    assert_eq!(cursor_label(&m), "b.gguf");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(draft.label(Task::Code), "llamacpp › /m/b.gguf");
    assert_eq!(draft.label(Task::Chat), "llamacpp › /m/b.gguf");
    assert_eq!(
        draft.managed(),
        Some(&ManagedServer {
            model: PathBuf::from("/m/b.gguf")
        })
    );
}

// Spec S12 (managed-llama-server-followups): "si no es bloqueante para usar
// otros modelos": with a llama-server of another model answering on the
// port, a local model is still lazarobox's own server.
#[test]
fn managed_a_local_model_is_chosen_while_another_server_answers() {
    let ai = AiConfig::general(None, None).with_managed(ManagedServer {
        model: PathBuf::from("/m/a.gguf"),
    });
    let mut d = detection(&[engine("llamacpp", Status::Available, &["/m/x.gguf"], None)]);
    d.local_models = vec![PathBuf::from("/m/a.gguf"), PathBuf::from("/m/b.gguf")];
    let mut m = MenuState::open(&Config::default()).with_ai(ai, Some(d));
    open(&mut m, "chat");
    for _ in 0..20 {
        if cursor_label(&m) == "llamacpp" {
            break;
        }
        press(&mut m, 'j');
    }
    send(&mut m, key(KeyCode::Enter));
    press(&mut m, 'j');
    assert_eq!(cursor_label(&m), "b.gguf");
    send(&mut m, key(KeyCode::Enter));
    let draft = m.ai_draft();
    assert_eq!(draft.label(Task::Chat), "llamacpp › /m/b.gguf");
    assert_eq!(
        draft.managed(),
        Some(&ManagedServer {
            model: PathBuf::from("/m/b.gguf")
        })
    );
}

// Only tasks on a model lazarobox starts move: a name of the user's own stays.
#[test]
fn a_task_on_a_name_of_its_own_keeps_it() {
    let mut ai = AiConfig::general(None, None);
    ai.set(
        Task::Review,
        crate::core::ai::config::Assignment {
            engine: Some("llamacpp".into()),
            model: Some("qwen".into()),
        },
    );
    let mut m = MenuState::open(&Config::default()).with_ai(ai, stopped_llamacpp(&["/m/a.gguf"]));
    choose(&mut m, "chat", "llamacpp");
    assert_eq!(m.ai_draft().label(Task::Review), "llamacpp › qwen");
}
