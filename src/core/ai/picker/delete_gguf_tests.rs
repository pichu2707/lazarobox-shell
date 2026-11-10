//! The Delete screen of a llama.cpp file: what `d` and `y` ask, and what
//! keeps a file from going (config-menu, Delete a model; D1, D3, D4).

use std::path::PathBuf;

use super::delete::{HINT_DELETE, HINT_MEASURING, SHARED_CACHE};
use super::*;
use crate::core::ai::EngineState;
use crate::core::ai::config::Assignment;

const HUB_FILE: &str = "/h/.cache/huggingface/hub/models--org--tiny/snapshots/c0ffee/tiny-q4.gguf";
const PLAIN_FILE: &str = "/h/.local/share/llama.cpp/models/qwen.gguf";

fn state(status: Status, models: &[&str]) -> EngineState {
    EngineState {
        binary: None,
        id: "llamacpp",
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Default::default(),
    }
}

/// A stopped llama-server and the two files the scan found.
fn machine() -> Detection {
    Detection {
        engines: vec![state(Status::NotRunning, &[])],
        local_models: vec![PathBuf::from(HUB_FILE), PathBuf::from(PLAIN_FILE)],
        ..Default::default()
    }
}

fn free(draft: &AiConfig) -> InUse<'_> {
    InUse {
        draft,
        saved: draft,
        turns: &[],
    }
}

/// A picker on llamacpp's model level, the cursor on `path`'s row.
fn on_file(path: &str, draft: &AiConfig, d: &Detection) -> Picker {
    let mut picker = Picker::open(Target::Task(Task::Code), draft, Some(d));
    picker.level = Level::Models {
        engine: "llamacpp".to_owned(),
    };
    let rows = picker.rows(draft, Some(d));
    picker.cursor = rows
        .iter()
        .position(|r| r.key == PickKey::Model(path.to_owned()))
        .unwrap_or_else(|| panic!("no row {path}: {rows:?}"));
    picker
}

fn labels(picker: &Picker, draft: &AiConfig, d: &Detection) -> String {
    picker
        .rows(draft, Some(d))
        .into_iter()
        .map(|r| r.label)
        .collect::<Vec<_>>()
        .join("\n")
}

fn gguf(path: &str) -> GgufFile {
    GgufFile {
        path: path.to_owned(),
        size: None,
        parts: 1,
        in_use: Vec::new(),
    }
}

fn measure_of(outcome: Outcome) -> DeleteAsk {
    match outcome {
        Outcome::Measure(ask) => ask,
        other => panic!("expected a measure: {other:?}"),
    }
}

/// The screen of `path`, drawn and measured at `size` in `parts`.
fn measured(path: &str, size: u64, parts: usize, draft: &AiConfig, d: &Detection) -> Picker {
    let mut picker = on_file(path, draft, d);
    let mut ask = measure_of(picker.open_delete(free(draft), Some(d)));
    picker.install_arm();
    if let DeleteTarget::Gguf(file) = &mut ask.target {
        file.size = Some(size);
        file.parts = parts;
    }
    picker.delete_measured(ask);
    picker
}

fn assign(config: &mut AiConfig, task: Task, model: &str) {
    config.set(
        task,
        Assignment {
            engine: Some("llamacpp".to_owned()),
            model: Some(model.to_owned()),
        },
    );
}

// Spec: `d` on a GGUF row opens the confirmation and asks for its size.
#[test]
fn d_on_a_file_row_opens_the_screen_and_asks_the_runtime_to_measure_it() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    let ask = measure_of(picker.open_delete(free(&draft), Some(&d)));
    assert_eq!(ask.target, DeleteTarget::Gguf(gguf(PLAIN_FILE)));
    let delete = picker.delete().expect("the screen is open");
    assert_eq!(delete.measuring(), Some(ask.id));
    assert_eq!(delete.hint(), HINT_MEASURING);
    let shown = labels(&picker, &draft, &d);
    assert!(shown.contains("delete qwen.gguf?"), "{shown}");
    assert!(
        shown.contains("folder: /h/.local/share/llama.cpp/models"),
        "{shown}"
    );
    assert!(shown.contains("size: measuring…"), "{shown}");
    assert!(shown.contains("This cannot be undone."), "{shown}");
    assert!(!shown.contains("Hugging Face"), "{shown}");
}

// Spec (D4): the confirmation shows the size, of all the parts.
#[test]
fn the_measure_shows_the_size_of_every_part() {
    let draft = AiConfig::default();
    let d = machine();
    let picker = measured(PLAIN_FILE, 3_000_000_000, 3, &draft, &d);
    let shown = labels(&picker, &draft, &d);
    assert!(shown.contains("size: 3.0 GB in 3 parts"), "{shown}");
    assert_eq!(picker.delete().unwrap().hint(), HINT_DELETE);
    let one = measured(PLAIN_FILE, 2_000_000, 1, &draft, &d);
    assert!(labels(&one, &draft, &d).contains("size: 2 MB\n"));
}

// Spec (D1): a file of the shared cache can go, and the screen says who
// else loses it.
#[test]
fn a_file_of_the_hugging_face_cache_says_other_tools_lose_it_too() {
    let draft = AiConfig::default();
    let d = machine();
    let picker = measured(HUB_FILE, 1_000_000_000, 1, &draft, &d);
    let shown = labels(&picker, &draft, &d).replace('\n', " ");
    assert!(shown.contains(SHARED_CACHE), "{shown}");
    assert!(
        shown.contains("folder: /h/.cache/huggingface/hub/models--org--tiny"),
        "{shown}"
    );
}

// Spec (D4): `y` waits for the size.
#[test]
fn y_waits_for_the_measure_then_asks_to_delete_the_file() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    let mut ask = measure_of(picker.open_delete(free(&draft), Some(&d)));
    picker.install_arm();
    assert_eq!(
        picker.delete_answer(true, free(&draft), Some(&d)),
        Outcome::Stay
    );
    assert!(picker.delete().is_some(), "still waiting");
    if let DeleteTarget::Gguf(file) = &mut ask.target {
        file.size = Some(5);
    }
    picker.delete_measured(ask);
    let Outcome::Delete(asked) = picker.delete_answer(true, free(&draft), Some(&d)) else {
        panic!("expected a delete");
    };
    assert_eq!(
        asked.target,
        DeleteTarget::Gguf(GgufFile {
            size: Some(5),
            ..gguf(PLAIN_FILE)
        })
    );
    assert_eq!(asked.target.deleted(), "deleted qwen.gguf · freed 5 B");
}

#[test]
fn a_measure_of_another_screen_changes_nothing_and_a_failed_one_closes_it() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    let ask = measure_of(picker.open_delete(free(&draft), Some(&d)));
    let stale = DeleteAsk {
        id: ask.id + 9,
        target: DeleteTarget::Gguf(GgufFile {
            size: Some(1),
            ..gguf(PLAIN_FILE)
        }),
    };
    picker.delete_measured(stale.clone());
    picker.delete_reply(stale.id, &draft, Some(&d));
    assert_eq!(picker.delete().unwrap().measuring(), Some(ask.id));
    picker.delete_reply(ask.id, &draft, Some(&d));
    assert!(picker.delete().is_none());
    assert_eq!(
        picker.key(&draft, Some(&d)),
        Some(PickKey::Model(PLAIN_FILE.to_owned())),
        "back on the file's row"
    );
}

#[test]
fn the_notice_of_a_split_model_names_its_parts_and_what_was_freed() {
    let target = DeleteTarget::Gguf(GgufFile {
        size: Some(3_000_000_000),
        parts: 3,
        ..gguf("/m/big-00001-of-00003.gguf")
    });
    assert_eq!(
        target.deleted(),
        "deleted big-00001-of-00003.gguf (3 parts) · freed 3.0 GB"
    );
    assert_eq!(
        target.failed("why"),
        "could not delete big-00001-of-00003.gguf: why"
    );
}

// Spec: a row with no file behind it offers no `d`.
#[test]
fn d_is_inert_on_a_model_a_running_llama_server_lists() {
    let draft = AiConfig::default();
    let d = Detection {
        engines: vec![state(Status::Available, &[PLAIN_FILE])],
        local_models: vec![PathBuf::from(PLAIN_FILE)],
        ..Default::default()
    };
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    assert_eq!(picker.delete_target(&draft, Some(&d)), None);
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert!(picker.delete().is_none());
}

#[test]
fn d_is_inert_on_a_managed_model_the_scan_did_not_find() {
    let mut draft = AiConfig::default();
    draft.manage(PathBuf::from("/elsewhere/own.gguf"));
    let d = machine();
    let mut picker = on_file("/elsewhere/own.gguf", &draft, &d);
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert!(picker.delete().is_none());
}

#[test]
fn d_is_inert_on_the_entry_rows_of_llamacpp() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    let rows = picker.rows(&draft, Some(&d));
    picker.cursor = rows
        .iter()
        .position(|r| r.key == PickKey::HfDownload)
        .unwrap();
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert!(picker.delete().is_none());
}

/// What `d` and then `y` say for `uses`, on the plain file.
fn refusals(uses: InUse<'_>, d: &Detection) -> (Outcome, Outcome) {
    let draft = AiConfig::default();
    let mut picker = on_file(PLAIN_FILE, uses.draft, d);
    let on_d = picker.open_delete(uses, Some(d));
    assert!(picker.delete().is_none(), "{on_d:?}");
    let mut confirming = measured(PLAIN_FILE, 5, 1, &draft, d);
    let on_y = confirming.delete_answer(true, uses, Some(d));
    assert!(confirming.delete().is_none(), "{on_y:?}");
    (on_d, on_y)
}

// Spec (D3): a task of the draft or of the saved config refuses the file.
#[test]
fn a_task_assignment_of_the_draft_or_saved_config_refuses_it_on_d_and_y() {
    let d = machine();
    let empty = AiConfig::default();
    let mut assigned = AiConfig::default();
    assign(&mut assigned, Task::Chat, PLAIN_FILE);
    let why =
        Outcome::Refused("qwen.gguf is used by task chat · assign another model first".to_owned());
    for (draft, saved) in [(&assigned, &empty), (&empty, &assigned)] {
        let uses = InUse {
            draft,
            saved,
            turns: &[],
        };
        assert_eq!(refusals(uses, &d), (why.clone(), why.clone()));
    }
}

// Spec (D3): the managed llama-server's model is refused.
#[test]
fn the_managed_model_of_the_draft_or_saved_config_refuses_it_on_d_and_y() {
    let d = machine();
    let empty = AiConfig::default();
    let mut managed = AiConfig::default();
    managed.manage(PathBuf::from(PLAIN_FILE));
    let why = Outcome::Refused(
        "qwen.gguf is the model of the managed llama-server · choose another one for it first"
            .to_owned(),
    );
    for (draft, saved) in [(&managed, &empty), (&empty, &managed)] {
        let uses = InUse {
            draft,
            saved,
            turns: &[],
        };
        assert_eq!(refusals(uses, &d), (why.clone(), why.clone()));
    }
}

#[test]
fn a_chat_turn_on_the_file_refuses_it() {
    let d = machine();
    let draft = AiConfig::default();
    let turns = [("llamacpp".to_owned(), PLAIN_FILE.to_owned())];
    let uses = InUse {
        draft: &draft,
        saved: &draft,
        turns: &turns,
    };
    let why = Outcome::Refused("qwen.gguf is in use by a running chat turn".to_owned());
    assert_eq!(refusals(uses, &d), (why.clone(), why));
}

// Spec: refused while a download runs.
#[test]
fn a_running_download_refuses_it_on_d_and_y() {
    let d = machine();
    let draft = AiConfig::default();
    let running = Running {
        source: Source::Ollama,
        file: "other:1b".to_owned(),
        license: String::new(),
        done: 0,
        total: 100,
    };
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    picker.set_download(Some(running.clone()));
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Busy);
    let mut confirming = measured(PLAIN_FILE, 5, 1, &draft, &d);
    confirming.set_download(Some(running));
    assert_eq!(
        confirming.delete_answer(true, free(&draft), Some(&d)),
        Outcome::Busy
    );
    assert!(confirming.delete().is_none());
}

// Safety: the runtime gets every model path the config uses, to refuse any
// spelling of them.
#[test]
fn the_ask_carries_the_paths_the_config_uses() {
    let d = machine();
    let mut draft = AiConfig::default();
    draft.manage(PathBuf::from("/m/managed.gguf"));
    assign(&mut draft, Task::Code, "/m/code.gguf");
    let mut saved = AiConfig::default();
    assign(&mut saved, Task::Chat, "/m/chat.gguf");
    let uses = InUse {
        draft: &draft,
        saved: &saved,
        turns: &[],
    };
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    let ask = measure_of(picker.open_delete(uses, Some(&d)));
    let DeleteTarget::Gguf(file) = ask.target else {
        panic!()
    };
    let mut in_use = file.in_use.clone();
    in_use.sort();
    assert_eq!(
        in_use,
        ["/m/chat.gguf", "/m/code.gguf", "/m/managed.gguf"].map(PathBuf::from)
    );
}

// Spec (D3): a file a running llama-server has loaded is refused. Only a
// managed setup lists files next to a server that answers.
#[test]
fn a_file_the_running_llama_server_loaded_is_refused_on_d_and_y() {
    let mut draft = AiConfig::default();
    draft.manage(PathBuf::from("/m/own.gguf"));
    let mut d = machine();
    d.engines = vec![state(Status::Available, &[PLAIN_FILE])];
    let uses = free(&draft);
    let why = Outcome::Refused("qwen.gguf is loaded by the running llama-server".to_owned());
    let mut picker = on_file(PLAIN_FILE, &draft, &d);
    assert_eq!(picker.open_delete(uses, Some(&d)), why);
    assert!(picker.delete().is_none());
    // Measured while the server was stopped, refused once it loaded it.
    let stopped = machine();
    let mut confirming = on_file(PLAIN_FILE, &draft, &stopped);
    let mut ask = measure_of(confirming.open_delete(uses, Some(&stopped)));
    confirming.install_arm();
    if let DeleteTarget::Gguf(file) = &mut ask.target {
        file.size = Some(5);
    }
    confirming.delete_measured(ask);
    assert_eq!(confirming.delete_answer(true, uses, Some(&d)), why);
    assert!(confirming.delete().is_none());
}
