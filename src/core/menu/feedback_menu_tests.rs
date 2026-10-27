//! Tests of the `Feedback` row of the Settings tab (config-menu, Feedback),
//! at the `MenuState` level.

use super::ai_tests::value_of;
use super::feedback::{self, Facts, HINT_ATTACH, HINT_DONE, HINT_KIND, HINT_TEXT};
use super::footer_tests::hint;
use super::*;
use crate::core::ai::{Detection, Diagnostics, EngineState, Status};

fn facts() -> Facts {
    Facts {
        version: "0.25.1".to_owned(),
        os: "linux".to_owned(),
        wsl: false,
        shell: Some("zsh".to_owned()),
        terminal: None,
        engines: None,
    }
}

fn on_row() -> MenuState {
    let mut m = MenuState::open(&Config::default()).with_feedback(facts());
    m.goto("Feedback");
    m
}

fn key_with(menu: &mut MenuState, code: KeyCode, mods: KeyModifiers) -> MenuCommand {
    menu.on_key(&KeyEvent::new(code, mods), false, &mut Config::default())
}

fn send(menu: &mut MenuState, code: KeyCode) -> MenuCommand {
    key_with(menu, code, KeyModifiers::NONE)
}

fn texts(menu: &MenuState) -> Vec<String> {
    menu.rows(&Config::default())
        .into_iter()
        .map(|row| match row {
            Row::Title(t) | Row::Text(t) => t.into_owned(),
            Row::Field(t) => t,
            other => format!("{other:?}"),
        })
        .collect()
}

fn engine(id: &'static str, status: Status, version: Option<&str>) -> EngineState {
    EngineState {
        id,
        status,
        models: Vec::new(),
        model_labels: Vec::new(),
        version: version.map(str::to_owned),
        diagnostics: Diagnostics::default(),
    }
}

// Spec (config-menu, Feedback): the row is the last of the Settings tab.
#[test]
fn the_feedback_row_is_last_in_settings() {
    let items = SECTIONS[0].items;
    let last = items.last().unwrap();
    assert_eq!(last.label, "Feedback");
    assert!(matches!(last.kind, ItemKind::Feedback));
    assert_eq!(items[items.len() - 2].label, "Updates");
    let m = on_row();
    assert_eq!(value_of(&m, "Feedback"), "bug report or idea");
    assert_eq!(hint(&m), "Enter open · j/k move · s save · Esc cancel");
}

// Spec (config-menu, Feedback): Enter opens the form, with what is attached
// shown before anything leaves.
#[test]
fn enter_opens_the_form_with_the_environment_shown() {
    let mut m = on_row();
    assert_eq!(send(&mut m, KeyCode::Enter), MenuCommand::None);
    assert!(m.in_sub_level(), "the tab keys are inert in the form");
    assert_eq!(
        texts(&m),
        [
            "Feedback",
            "› Kind: Bug report",
            "  Title",
            "  ",
            "  Description",
            "  ",
            "  [x] Attach the environment",
            "      lazarobox 0.25.1 · linux · zsh",
            "      engines: not detected",
        ]
    );
    assert_eq!(hint(&m), HINT_KIND);
    assert_eq!(
        HINT_KIND,
        "←/→ kind · Tab · Ctrl+S copy · Ctrl+O open · Esc"
    );
    // The focused text field draws its cursor.
    send(&mut m, KeyCode::Tab);
    assert_eq!(texts(&m)[2], "› Title");
    assert_eq!(texts(&m)[3], "  ▏");
    assert_eq!(hint(&m), HINT_TEXT);
    assert_eq!(HINT_TEXT, "Tab next · Ctrl+S copy · Ctrl+O open · Esc back");
    send(&mut m, KeyCode::Tab);
    send(&mut m, KeyCode::Tab);
    assert_eq!(texts(&m)[6], "› [x] Attach the environment");
    assert_eq!(hint(&m), HINT_ATTACH);
    assert_eq!(
        HINT_ATTACH,
        "Space attach · Ctrl+S copy · Ctrl+O open · Esc"
    );
    // Every footer fits the 48 columns of the narrow popup.
    for h in [HINT_KIND, HINT_TEXT, HINT_ATTACH, HINT_DONE] {
        assert!(h.chars().count() <= 48, "{h}");
    }
    // The form fits the view.
    assert!(texts(&m).len() <= VIEW_BODY);
}

// Spec (config-menu, Feedback): Ctrl+S copies the link and says how to use
// it; Esc goes back to the row, not out of the menu.
#[test]
fn ctrl_s_copies_the_link_and_esc_goes_back_to_the_row() {
    let mut m = on_row();
    send(&mut m, KeyCode::Enter);
    send(&mut m, KeyCode::Tab);
    m.on_paste("Scroll jumps");
    send(&mut m, KeyCode::Tab);
    m.on_paste("It jumps\nback");
    assert_eq!(texts(&m)[5], "  It jumps");
    assert_eq!(texts(&m)[6], "  back▏");
    let MenuCommand::Copy(link) = key_with(&mut m, KeyCode::Char('s'), KeyModifiers::CONTROL)
    else {
        panic!("no copy");
    };
    assert!(
        link.starts_with(
            "https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml&title=Scroll%20jumps&what-happened=It%20jumps%0Aback&environment="
        ),
        "{link}"
    );
    assert_eq!(
        texts(&m),
        [
            "Feedback",
            "Link copied. Paste it in your browser:",
            "GitHub opens the issue form filled in, for you",
            "to review and send.",
        ]
    );
    m.set_copied("Copied 300 characters".to_owned());
    assert_eq!(hint(&m), "Copied 300 characters");
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(!m.in_sub_level());
    assert_eq!(value_of(&m, "Feedback"), "bug report or idea");
    // A second Esc leaves the menu as usual.
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::Cancel);
}

/// The form with a title and a description, on a menu that has an opener
/// or not.
fn filled(opener: bool) -> MenuState {
    let mut m = on_row().with_opener(opener);
    send(&mut m, KeyCode::Enter);
    send(&mut m, KeyCode::Tab);
    m.on_paste("Scroll jumps");
    send(&mut m, KeyCode::Tab);
    m.on_paste("It jumps\nback");
    m
}

fn ctrl(menu: &mut MenuState, c: char) -> MenuCommand {
    key_with(menu, KeyCode::Char(c), KeyModifiers::CONTROL)
}

// Spec (config-menu, Feedback; open-links S4): Ctrl+O opens the prefilled
// page, the same link Ctrl+S copies, and says so; Esc goes back to the row.
#[test]
fn ctrl_o_opens_the_prefilled_page_in_the_browser() {
    let MenuCommand::Copy(link) = ctrl(&mut filled(true), 's') else {
        panic!("no copy");
    };
    let mut m = filled(true);
    assert_eq!(ctrl(&mut m, 'o'), MenuCommand::Open(link));
    assert_eq!(
        texts(&m),
        [
            "Feedback",
            "Opened in your browser. GitHub shows the issue",
            "form filled in, for you to review and send.",
        ]
    );
    assert_eq!(hint(&m), HINT_DONE);
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(!m.in_sub_level());
}

// Spec (config-menu, Feedback; open-links S3): with no opener on the host,
// Ctrl+O copies the link like Ctrl+S.
#[test]
fn without_an_opener_ctrl_o_copies_the_link() {
    let MenuCommand::Copy(link) = ctrl(&mut filled(false), 's') else {
        panic!("no copy");
    };
    let mut m = filled(false);
    assert_eq!(ctrl(&mut m, 'o'), MenuCommand::Copy(link));
    assert_eq!(texts(&m)[1], "Link copied. Paste it in your browser:");
}

#[test]
fn a_missing_title_reads_as_the_footer_error() {
    let mut m = on_row();
    send(&mut m, KeyCode::Enter);
    key_with(&mut m, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(m.footer(), Footer::Error(feedback::TITLE_REQUIRED));
    // Letters are typed, not menu keys.
    assert_eq!(send(&mut m, KeyCode::Char('s')), MenuCommand::None);
    assert_eq!(texts(&m)[3], "  s▏");
}

// Spec (config-menu, Feedback): the engines attached are the detected ones,
// with their version; one not installed is left out.
#[test]
fn the_attached_engines_are_the_detected_ones() {
    let detection = Detection {
        engines: vec![
            engine("ollama", Status::Available, Some("0.12.3")),
            engine("llamacpp", Status::NotRunning, None),
            engine("codex", Status::NotInstalled, None),
            engine("gemini", Status::Available, Some("1.2\u{1b}[31m")),
        ],
        local_models: Vec::new(),
    };
    assert_eq!(
        feedback::engines(&detection),
        [
            "ollama 0.12.3 available",
            "llamacpp not running",
            "gemini available",
        ]
    );
}

// Spec (config-menu, Feedback): every attached fact is shown whole, even in
// the 48 columns of the narrow popup: the facts wrap between items.
#[test]
fn the_attached_facts_wrap_and_are_never_cut() {
    let mut m = MenuState::open(&Config::default()).with_feedback(Facts {
        terminal: Some("WezTerm".to_owned()),
        wsl: true,
        engines: Some(vec![
            "ollama 0.12.3 available".to_owned(),
            "llamacpp not running".to_owned(),
            "codex 0.40.0 available".to_owned(),
        ]),
        ..facts()
    });
    m.goto("Feedback");
    send(&mut m, KeyCode::Enter);
    let shown = texts(&m);
    let at = shown
        .iter()
        .position(|l| l.ends_with("Attach the environment"))
        .unwrap();
    assert_eq!(
        shown[at + 1..],
        [
            "      lazarobox 0.25.1 · linux (WSL) · zsh",
            "      WezTerm",
            "      engines: ollama 0.12.3 available",
            "      llamacpp not running",
            "      codex 0.40.0 available",
        ]
    );
    for line in &shown {
        assert!(line.chars().count() <= 48, "{line}");
    }
}
