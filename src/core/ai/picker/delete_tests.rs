//! The Delete screen of the ollama picker: what `d` and `y` do, and what
//! keeps a model from going (config-menu, Delete a model).

use super::delete::{HINT_DELETE, HINT_DELETING};
use super::*;
use crate::core::ai::EngineState;
use crate::core::ai::config::Assignment;

fn state(id: &'static str, status: Status, models: &[&str]) -> EngineState {
    EngineState {
        binary: None,
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Default::default(),
    }
}

/// Ollama lists `qwen3:4b` (2.4 GB) and LM Studio lists a model of its own.
fn machine() -> Detection {
    Detection {
        engines: vec![
            state("ollama", Status::Available, &["qwen3:4b"]),
            state("lmstudio", Status::Available, &["gemma:7b"]),
        ],
        ollama_sizes: vec![("qwen3:4b".to_owned(), 2_400_000_000)],
        ..Default::default()
    }
}

fn assign(config: &mut AiConfig, task: Task, model: &str) {
    config.set(
        task,
        Assignment {
            engine: Some("ollama".to_owned()),
            model: Some(model.to_owned()),
        },
    );
}

fn free(draft: &AiConfig) -> InUse<'_> {
    InUse {
        draft,
        saved: draft,
        turns: &[],
    }
}

/// A picker on `engine`'s model level, the cursor on its first row.
fn at_models(engine: &str, draft: &AiConfig, d: &Detection) -> Picker {
    let mut picker = Picker::open(Target::Task(Task::Code), draft, Some(d));
    picker.level = Level::Models {
        engine: engine.to_owned(),
    };
    picker.cursor = 0;
    picker
}

fn labels(picker: &Picker, draft: &AiConfig, d: &Detection) -> Vec<String> {
    picker
        .rows(draft, Some(d))
        .into_iter()
        .map(|r| r.label)
        .collect()
}

fn running() -> Running {
    Running {
        source: Source::Ollama,
        file: "other:1b".to_owned(),
        license: String::new(),
        done: 0,
        total: 100,
    }
}

/// A confirmation of `qwen3:4b`, drawn and waiting for a key.
fn confirming(draft: &AiConfig, d: &Detection) -> Picker {
    let mut picker = at_models("ollama", draft, d);
    assert_eq!(picker.open_delete(free(draft), Some(d)), Outcome::Stay);
    picker.install_arm();
    picker
}

fn ask_of(outcome: Outcome) -> DeleteAsk {
    match outcome {
        Outcome::Delete(ask) => ask,
        other => panic!("expected a delete: {other:?}"),
    }
}

// Spec: `d` on a downloaded model opens the confirmation.
#[test]
fn d_on_a_downloaded_ollama_row_opens_a_confirmation_that_names_it_and_its_size() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert!(picker.delete().is_some(), "{:?}", picker.level);
    let shown = labels(&picker, &draft, &d).join("\n");
    assert!(shown.contains("qwen3:4b"), "{shown}");
    assert!(shown.contains("2.4 GB"), "{shown}");
    assert!(shown.contains("cannot be undone"), "{shown}");
    assert_eq!(picker.delete().unwrap().hint(), HINT_DELETE);
}

#[test]
fn a_model_with_no_known_size_is_confirmed_all_the_same() {
    let draft = AiConfig::default();
    let mut d = machine();
    d.ollama_sizes.clear();
    let mut picker = confirming(&draft, &d);
    assert!(
        labels(&picker, &draft, &d)
            .join("\n")
            .contains("size: unknown"),
        "{:?}",
        labels(&picker, &draft, &d)
    );
    let ask = ask_of(picker.delete_answer(true, free(&draft), Some(&d)));
    assert_eq!(ask.target.deleted(), "deleted qwen3:4b");
}

// Spec: `d` does nothing on any other row.
#[test]
fn d_is_inert_on_a_row_that_cannot_be_deleted() {
    let draft = AiConfig::default();
    let d = machine();
    // Engines level.
    let mut picker = Picker::open(Target::Task(Task::Code), &draft, Some(&d));
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert_eq!(picker.level, Level::Engines);
    // A model of another engine, and every row of ollama but the listed one.
    let mut other = at_models("lmstudio", &draft, &d);
    assert_eq!(other.open_delete(free(&draft), Some(&d)), Outcome::Stay);
    assert!(other.delete().is_none());
    let mut picker = at_models("ollama", &draft, &d);
    let rows = picker.rows(&draft, Some(&d));
    assert!(rows.iter().any(|r| r.mark == Mark::DownloadRequired));
    assert!(rows.iter().any(|r| r.key == PickKey::OllamaDownload));
    for row in rows.iter().filter(|r| r.mark != Mark::Downloaded) {
        picker.cursor = rows.iter().position(|r| r == row).unwrap();
        assert_eq!(
            picker.open_delete(free(&draft), Some(&d)),
            Outcome::Stay,
            "{row:?}"
        );
        assert!(picker.delete().is_none(), "{row:?}");
    }
}

// Spec: only an unmodified `y`, once drawn, deletes.
#[test]
fn y_before_the_screen_was_drawn_answers_nothing() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = at_models("ollama", &draft, &d);
    picker.open_delete(free(&draft), Some(&d));
    assert_eq!(
        picker.delete_answer(true, free(&draft), Some(&d)),
        Outcome::Stay
    );
    assert!(picker.delete().is_some_and(|c| c.in_flight().is_none()));
}

#[test]
fn y_once_drawn_asks_the_runtime_and_waits() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = confirming(&draft, &d);
    let ask = ask_of(picker.delete_answer(true, free(&draft), Some(&d)));
    assert_eq!(
        ask.target,
        DeleteTarget::Ollama {
            name: "qwen3:4b".to_owned(),
            size: Some(2_400_000_000),
        }
    );
    assert_eq!(picker.delete().and_then(Delete::in_flight), Some(ask.id));
    assert!(
        labels(&picker, &draft, &d)
            .join("\n")
            .contains("deleting qwen3:4b")
    );
    assert_eq!(picker.delete().unwrap().hint(), HINT_DELETING);
    // While it runs the screen ignores every answer.
    for yes in [true, false] {
        assert_eq!(
            picker.delete_answer(yes, free(&draft), Some(&d)),
            Outcome::Stay
        );
        assert_eq!(picker.delete().and_then(Delete::in_flight), Some(ask.id));
    }
}

#[test]
fn any_other_answer_goes_back_to_the_models_on_the_same_row() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = confirming(&draft, &d);
    assert_eq!(
        picker.delete_answer(false, free(&draft), Some(&d)),
        Outcome::Stay
    );
    assert_eq!(
        picker.level,
        Level::Models {
            engine: "ollama".to_owned()
        }
    );
    assert_eq!(
        picker.key(&draft, Some(&d)),
        Some(PickKey::Model("qwen3:4b".to_owned()))
    );
}

// Spec: the reply of the delete in flight goes back; a stale one is dropped.
#[test]
fn the_reply_of_the_delete_in_flight_goes_back_and_keeps_the_row() {
    let draft = AiConfig::default();
    let d = machine();
    let mut picker = confirming(&draft, &d);
    let ask = ask_of(picker.delete_answer(true, free(&draft), Some(&d)));
    picker.delete_reply(ask.id + 1, &draft, Some(&d));
    assert_eq!(
        picker.delete().and_then(Delete::in_flight),
        Some(ask.id),
        "a stale reply changes nothing"
    );
    picker.delete_reply(ask.id, &draft, Some(&d));
    assert_eq!(
        picker.level,
        Level::Models {
            engine: "ollama".to_owned()
        }
    );
    assert_eq!(
        picker.key(&draft, Some(&d)),
        Some(PickKey::Model("qwen3:4b".to_owned()))
    );
}

// Spec: a model in use is refused, and says who uses it (D3).
#[test]
fn a_task_of_the_draft_refuses_the_delete_and_is_named() {
    let d = machine();
    let mut draft = AiConfig::default();
    assign(&mut draft, Task::Chat, "qwen3:4b");
    let saved = AiConfig::default();
    let uses = InUse {
        draft: &draft,
        saved: &saved,
        turns: &[],
    };
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(
        picker.open_delete(uses, Some(&d)),
        Outcome::Refused("qwen3:4b is used by task chat · assign another model first".to_owned())
    );
    assert!(picker.delete().is_none());
}

#[test]
fn a_task_of_the_saved_config_refuses_it_too() {
    let d = machine();
    let draft = AiConfig::default();
    let mut saved = AiConfig::default();
    assign(&mut saved, Task::Code, "qwen3:4b");
    let uses = InUse {
        draft: &draft,
        saved: &saved,
        turns: &[],
    };
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(
        picker.open_delete(uses, Some(&d)),
        Outcome::Refused("qwen3:4b is used by task code · assign another model first".to_owned())
    );
}

#[test]
fn every_task_using_it_is_named_once_and_a_tag_less_name_counts() {
    let d = Detection {
        engines: vec![state("ollama", Status::Available, &["qwen3:latest"])],
        ..Default::default()
    };
    let mut draft = AiConfig::default();
    assign(&mut draft, Task::Code, "qwen3");
    assign(&mut draft, Task::Chat, "qwen3:latest");
    let mut saved = AiConfig::default();
    assign(&mut saved, Task::Code, "qwen3:latest");
    let uses = InUse {
        draft: &draft,
        saved: &saved,
        turns: &[],
    };
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(
        picker.open_delete(uses, Some(&d)),
        Outcome::Refused(
            "qwen3:latest is used by tasks code, chat · assign another model first".to_owned()
        )
    );
}

#[test]
fn a_chat_turn_running_on_it_refuses_the_delete() {
    let d = machine();
    let draft = AiConfig::default();
    let turns = [("ollama".to_owned(), "qwen3:4b".to_owned())];
    let uses = InUse {
        draft: &draft,
        saved: &draft,
        turns: &turns,
    };
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(
        picker.open_delete(uses, Some(&d)),
        Outcome::Refused("qwen3:4b is in use by a running chat turn".to_owned())
    );
    assert!(picker.delete().is_none());
}

#[test]
fn a_turn_on_another_model_or_engine_does_not_refuse_it() {
    let d = machine();
    let draft = AiConfig::default();
    let turns = [
        ("ollama".to_owned(), "other:1b".to_owned()),
        ("lmstudio".to_owned(), "qwen3:4b".to_owned()),
    ];
    let uses = InUse {
        draft: &draft,
        saved: &draft,
        turns: &turns,
    };
    let mut picker = at_models("ollama", &draft, &d);
    assert_eq!(picker.open_delete(uses, Some(&d)), Outcome::Stay);
    assert!(picker.delete().is_some());
}

#[test]
fn a_running_download_refuses_the_delete() {
    let d = machine();
    let draft = AiConfig::default();
    let mut picker = at_models("ollama", &draft, &d);
    picker.set_download(Some(running()));
    assert_eq!(picker.open_delete(free(&draft), Some(&d)), Outcome::Busy);
    assert!(picker.delete().is_none());
}

// Spec: the check runs again on `y`, since something may have changed.
#[test]
fn the_checks_run_again_on_y_and_a_refusal_closes_the_screen() {
    let d = machine();
    let draft = AiConfig::default();
    let mut assigned = AiConfig::default();
    assign(&mut assigned, Task::Chat, "qwen3:4b");
    let turns = [("ollama".to_owned(), "qwen3:4b".to_owned())];
    let cases: [(InUse<'_>, &str); 3] = [
        (
            InUse {
                draft: &assigned,
                saved: &draft,
                turns: &[],
            },
            "qwen3:4b is used by task chat · assign another model first",
        ),
        (
            InUse {
                draft: &draft,
                saved: &assigned,
                turns: &[],
            },
            "qwen3:4b is used by task chat · assign another model first",
        ),
        (
            InUse {
                draft: &draft,
                saved: &draft,
                turns: &turns,
            },
            "qwen3:4b is in use by a running chat turn",
        ),
    ];
    for (uses, why) in cases {
        let mut picker = confirming(&draft, &d);
        assert_eq!(
            picker.delete_answer(true, uses, Some(&d)),
            Outcome::Refused(why.to_owned())
        );
        assert!(picker.delete().is_none(), "{why}");
    }
    let mut busy = confirming(&draft, &d);
    busy.set_download(Some(running()));
    assert_eq!(
        busy.delete_answer(true, free(&draft), Some(&d)),
        Outcome::Busy
    );
    assert!(busy.delete().is_none());
}

// The rest of the picker keys keep working on the model level.
#[test]
fn refresh_and_copy_keep_their_answers_on_the_model_level() {
    let draft = AiConfig::default();
    let d = machine();
    let picker = at_models("ollama", &draft, &d);
    assert_eq!(picker.refresh(), Outcome::Refresh);
    assert_eq!(picker.copy(&draft, Some(&d)), Outcome::Stay);
    let picker = confirming(&draft, &d);
    assert_eq!(
        picker.refresh(),
        Outcome::Stay,
        "refresh stays off the screen"
    );
}
