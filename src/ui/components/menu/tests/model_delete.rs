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
