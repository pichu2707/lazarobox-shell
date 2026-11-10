//! The confirmation of deleting a model (config-menu, Delete a model), drawn
//! in the picker's popup.

use super::*;

/// The picker of `code` at ollama's models, the cursor on `qwen3:4b`.
fn on_the_model(size: Option<u64>) -> MenuState {
    let detection = Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: None,
                id: e.id,
                status: if e.id == "ollama" {
                    Status::Available
                } else {
                    Status::NotInstalled
                },
                models: if e.id == "ollama" {
                    vec!["qwen3:4b".to_owned()]
                } else {
                    Vec::new()
                },
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            })
            .collect(),
        ollama_sizes: size
            .map(|size| ("qwen3:4b".to_owned(), size))
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let mut menu = fresh().with_ai(AiConfig::default(), Some(detection));
    goto(&mut menu, "code");
    press(&mut menu, &[KeyCode::Enter]);
    for _ in 0..12 {
        let on_ollama = menu
            .rows(&Config::default())
            .iter()
            .any(|r| matches!(r, Row::Pick { label, selected: true, .. } if label == "ollama"));
        if on_ollama {
            break;
        }
        press(&mut menu, &[KeyCode::Char('j')]);
    }
    press(&mut menu, &[KeyCode::Enter]);
    menu
}

fn confirming(size: Option<u64>) -> MenuState {
    let mut menu = on_the_model(size);
    press(&mut menu, &[KeyCode::Char('d')]);
    menu.install_arm();
    menu
}

#[test]
fn snapshot_delete_confirmation() {
    insta::assert_snapshot!(text(&draw(80, POPUP_H, &confirming(Some(2_400_000_000)))));
}

#[test]
fn snapshot_delete_confirmation_of_a_model_with_no_size() {
    insta::assert_snapshot!(text(&draw(80, POPUP_H, &confirming(None))));
}

#[test]
fn snapshot_delete_running() {
    let mut menu = confirming(Some(2_400_000_000));
    press(&mut menu, &[KeyCode::Char('y')]);
    insta::assert_snapshot!(text(&draw(80, POPUP_H, &menu)));
}

// The hint fits the popup: `d delete` is not cut.
#[test]
fn the_delete_hint_fits_the_popup() {
    let shown = text(&draw(62, POPUP_H, &on_the_model(Some(1))));
    assert!(shown.contains("d delete · Esc back · s save"), "{shown}");
}

/// The Delete screen of a file in the Hugging Face cache, measured.
fn confirming_a_file() -> MenuState {
    let path = "/home/u/.cache/huggingface/hub/models--org--tiny/snapshots/c0ffee/tiny-q4.gguf";
    let detection = Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: None,
                id: e.id,
                status: if e.id == "llamacpp" {
                    Status::NotRunning
                } else {
                    Status::NotInstalled
                },
                models: Vec::new(),
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            })
            .collect(),
        local_models: vec![std::path::PathBuf::from(path)],
        ..Default::default()
    };
    let mut menu = fresh().with_ai(AiConfig::default(), Some(detection));
    goto(&mut menu, "code");
    press(&mut menu, &[KeyCode::Enter]);
    for _ in 0..12 {
        let on_llamacpp = menu
            .rows(&Config::default())
            .iter()
            .any(|r| matches!(r, Row::Pick { label, selected: true, .. } if label == "llamacpp"));
        if on_llamacpp {
            break;
        }
        press(&mut menu, &[KeyCode::Char('j')]);
    }
    press(&mut menu, &[KeyCode::Enter]);
    let d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
    let measure = match menu.on_key(&d, false, &mut Config::default()) {
        crate::core::menu::MenuCommand::MeasureModel(ask) => ask,
        other => panic!("expected a measure: {other:?}"),
    };
    menu.install_arm();
    let mut ask = measure;
    if let crate::core::ai::picker::DeleteTarget::Gguf(file) = &mut ask.target {
        file.size = Some(4_920_000_000);
        file.parts = 2;
    }
    menu.delete_measured(ask, Ok(()));
    menu
}

#[test]
fn snapshot_delete_confirmation_of_a_hugging_face_file() {
    insta::assert_snapshot!(text(&draw(80, POPUP_H, &confirming_a_file())));
}
