//! The `Feedback` row of the Settings tab (config-menu, Feedback;
//! open-links S4-S7): Ctrl+S sends the issue through the runtime's `gh`,
//! with the host the runtime found and the engines detected so far; the
//! answer comes back as `AppEvent::IssueSent`.

use super::*;
use crate::core::ai::{Detection, Diagnostics, EngineState, Status};
use crate::core::menu::Footer;
use crate::core::menu::feedback::{Facts, Issue, SendReply};

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

fn texts(a: &App) -> Vec<String> {
    a.menu()
        .unwrap()
        .rows(&a.config)
        .into_iter()
        .filter_map(|row| match row {
            Row::Text(t) => Some(t.into_owned()),
            _ => None,
        })
        .collect()
}

/// An app with `opener`, the menu open on the Feedback form filled in.
fn filled(opener: bool) -> App {
    let mut a = app().with_host(host()).with_opener(opener);
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
    a
}

/// [`filled`] after its Ctrl+S.
fn sending(opener: bool) -> App {
    let mut a = filled(opener);
    let effects = a.update(ctrl('s'));
    assert!(
        matches!(effects.as_slice(), [Effect::SendIssue(_)]),
        "{effects:?}"
    );
    a
}

const ISSUE_URL: &str = "https://github.com/pichu2707/lazarobox-shell/issues/7";

// Spec (open-links S4, S6): Ctrl+S sends the issue, with the host and the
// engines attached, and the form says it is sending.
#[test]
fn ctrl_s_in_the_form_sends_the_issue_with_host_and_engines() {
    let mut a = filled(true);
    let shown = texts(&a);
    assert!(
        shown.contains(&"      lazarobox 9.9.9 · linux (WSL) · zsh".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown.contains(&"      engines: ollama available".to_owned()),
        "{shown:?}"
    );
    let effects = a.update(ctrl('s'));
    assert_eq!(
        effects,
        vec![Effect::SendIssue(Box::new(Issue {
            repo: "pichu2707/lazarobox-shell".to_owned(),
            title: "Crash".to_owned(),
            body: "### What happened\n\nIt crashed\n\n### Environment\n\n```text\n\
                   lazarobox 9.9.9 · linux (WSL) · zsh\nengines: ollama available\n```"
                .to_owned(),
            labels: vec!["bug".to_owned()],
        }))]
    );
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("Sending to GitHub…".into())
    );
    // One send at a time.
    assert_eq!(a.update(ctrl('s')), vec![]);
    assert_eq!(a.input(), InputMode::Menu);
}

// Spec (open-links S7): the answer shows the issue's link, which Ctrl+O
// opens; Esc goes back to the row.
#[test]
fn a_sent_issue_shows_its_link_and_ctrl_o_opens_it() {
    let mut a = sending(true);
    a.take_dirty();
    assert_eq!(
        a.update(AppEvent::IssueSent(SendReply::Sent(ISSUE_URL.to_owned()))),
        vec![]
    );
    assert!(a.take_dirty());
    assert_eq!(texts(&a), ["Sent:", ISSUE_URL]);
    assert_eq!(
        a.update(ctrl('o')),
        vec![Effect::OpenUrl(ISSUE_URL.to_owned())]
    );
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Hint("Opened in your browser".into())
    );
    a.update(special(KeyCode::Esc));
    assert!(!a.menu().unwrap().in_sub_level());
    assert_eq!(a.input(), InputMode::Menu);
}

// Spec (open-links S7): a failure keeps the form with everything typed,
// and its footer shows gh's message, sanitized.
#[test]
fn a_failed_send_keeps_the_form_and_shows_the_message() {
    let mut a = sending(true);
    let form = texts(&a);
    a.take_dirty();
    let effects = a.update(AppEvent::IssueSent(SendReply::Failed(
        "\u{1b}[31mHTTP 403\u{1b}[0m:\nforbidden".to_owned(),
    )));
    assert_eq!(effects, vec![]);
    assert!(a.take_dirty());
    assert_eq!(texts(&a), form);
    assert_eq!(
        a.menu().unwrap().footer(),
        Footer::Error("gh: HTTP 403: forbidden")
    );
    assert!(matches!(
        a.update(ctrl('s')).as_slice(),
        [Effect::SendIssue(_)]
    ));
}

// Spec (open-links S5): without gh the prefilled page opens in the
// browser, as Ctrl+O would, and the view says why.
#[test]
fn without_gh_the_prefilled_page_opens_instead() {
    let link = match filled(true).update(ctrl('o')).as_slice() {
        [Effect::OpenUrl(link)] => link.clone(),
        other => panic!("{other:?}"),
    };
    let mut a = sending(true);
    assert_eq!(
        a.update(AppEvent::IssueSent(SendReply::NoGh)),
        vec![Effect::OpenUrl(link)]
    );
    assert_eq!(texts(&a)[0], "gh is not installed or not logged in:");
    // With no opener the link is copied instead.
    let mut a = sending(false);
    let effects = a.update(AppEvent::IssueSent(SendReply::NoGh));
    assert!(
        matches!(effects.as_slice(), [Effect::SetClipboard(_)]),
        "{effects:?}"
    );
    assert_eq!(texts(&a)[0], "gh is not installed or not logged in: link");
    assert!(a.notice().is_some_and(|n| n.starts_with("Copied ")));
}

// Spec (open-links S7): an answer after the menu closed is dropped.
#[test]
fn an_answer_with_the_menu_closed_is_dropped() {
    let mut a = app();
    a.take_dirty();
    assert_eq!(
        a.update(AppEvent::IssueSent(SendReply::Sent(ISSUE_URL.to_owned()))),
        vec![]
    );
    assert_eq!(a.input(), InputMode::Terminal);
}
