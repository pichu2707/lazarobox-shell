//! The `Feedback` row of the Settings tab (config-menu, Feedback): the link
//! leaves through the clipboard, with the host the runtime found and the
//! engines detected so far.

use super::*;
use crate::core::ai::{Detection, Diagnostics, EngineState, Status};
use crate::core::menu::feedback::Facts;

fn host() -> Facts {
    Facts {
        version: "9.9.9".to_owned(),
        os: "linux".to_owned(),
        wsl: true,
        shell: Some("zsh".to_owned()),
        terminal: None,
        engines: None,
    }
}

fn ctrl(c: char) -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn special(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn ctrl_s_in_the_form_copies_the_link_with_host_and_engines() {
    let mut a = app().with_host(host());
    a.engines = Some(Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: Vec::new(),
            model_labels: Vec::new(),
            version: None,
            diagnostics: Diagnostics::default(),
        }],
        local_models: Vec::new(),
    });
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "Feedback");
    a.update(special(KeyCode::Enter));
    a.update(special(KeyCode::Tab));
    a.update(AppEvent::Paste("Crash".to_owned()));
    a.update(special(KeyCode::Tab));
    a.update(AppEvent::Paste("It crashed".to_owned()));
    let shown: Vec<String> = a
        .menu()
        .unwrap()
        .rows(&a.config)
        .into_iter()
        .filter_map(|row| match row {
            Row::Text(t) => Some(t.into_owned()),
            _ => None,
        })
        .collect();
    assert!(
        shown.contains(&"      lazarobox 9.9.9 · linux (WSL) · zsh".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown.contains(&"      engines: ollama available".to_owned()),
        "{shown:?}"
    );
    let effects = a.update(ctrl('s'));
    let Some(Effect::SetClipboard(sequence)) = effects.first() else {
        panic!("{effects:?}");
    };
    let text = String::from_utf8_lossy(sequence).into_owned();
    assert!(text.starts_with("\u{1b}]52;c;"), "{text}");
    assert!(a.notice().is_some_and(|n| n.starts_with("Copied ")));
    assert!(
        a.menu().unwrap().in_sub_level(),
        "the view says what to do next"
    );
}
