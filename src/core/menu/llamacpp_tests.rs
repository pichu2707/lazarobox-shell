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

/// Opens the chat task, goes to llamacpp and presses Enter on its first model.
fn choose_first_model(m: &mut MenuState) {
    open(m, "chat");
    for _ in 0..20 {
        if cursor_label(m) == "llamacpp" {
            break;
        }
        press(m, 'j');
    }
    assert_eq!(cursor_label(m), "llamacpp");
    send(m, key(KeyCode::Enter));
    send(m, key(KeyCode::Enter));
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
