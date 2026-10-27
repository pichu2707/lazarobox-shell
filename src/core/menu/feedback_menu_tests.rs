//! Tests of the `Feedback` row of the Settings tab (config-menu, Feedback),
//! at the `MenuState` level.

use super::ai_tests::value_of;
use super::feedback::{
    self, Facts, HINT_ATTACH, HINT_DONE, HINT_KIND, HINT_SENT, HINT_SENT_COPY, HINT_TEXT, SendReply,
};
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
        "←/→ kind · Tab · Ctrl+S send · Ctrl+O open · Esc"
    );
    // The focused text field draws its cursor.
    send(&mut m, KeyCode::Tab);
    assert_eq!(texts(&m)[2], "› Title");
    assert_eq!(texts(&m)[3], "  ▏");
    assert_eq!(hint(&m), HINT_TEXT);
    assert_eq!(HINT_TEXT, "Tab next · Ctrl+S send · Ctrl+O open · Esc back");
    send(&mut m, KeyCode::Tab);
    send(&mut m, KeyCode::Tab);
    assert_eq!(texts(&m)[6], "› [x] Attach the environment");
    assert_eq!(hint(&m), HINT_ATTACH);
    assert_eq!(
        HINT_ATTACH,
        "Space attach · Ctrl+S send · Ctrl+O open · Esc"
    );
    // Every footer fits the 48 columns of the narrow popup.
    for h in [
        HINT_KIND,
        HINT_TEXT,
        HINT_ATTACH,
        HINT_DONE,
        HINT_SENT,
        HINT_SENT_COPY,
        feedback::SENDING,
    ] {
        assert!(h.chars().count() <= 48, "{h}");
    }
    // The form fits the view.
    assert!(texts(&m).len() <= VIEW_BODY);
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

/// The prefilled link of [`filled`], as Ctrl+O copies it with no opener.
fn prefilled() -> String {
    match ctrl(&mut filled(false), 'o') {
        MenuCommand::Copy(link) => link,
        other => panic!("no copy: {other:?}"),
    }
}

// Spec (config-menu, Feedback; open-links S4): Ctrl+O opens the prefilled
// page and says so; Esc goes back to the row.
#[test]
fn ctrl_o_opens_the_prefilled_page_in_the_browser() {
    let link = prefilled();
    assert!(
        link.starts_with(
            "https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml&title=Scroll%20jumps&what-happened=It%20jumps%0Aback&environment="
        ),
        "{link}"
    );
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
    let mut m = filled(false);
    assert_eq!(ctrl(&mut m, 'o'), MenuCommand::Copy(prefilled()));
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

const ISSUE_URL: &str = "https://github.com/pichu2707/lazarobox-shell/issues/42";

/// The form of [`filled`] after its Ctrl+S, and the issue it sends.
fn sending(opener: bool) -> (MenuState, feedback::Issue) {
    let mut m = filled(opener);
    let MenuCommand::SendIssue(issue) = ctrl(&mut m, 's') else {
        panic!("not sent");
    };
    (m, *issue)
}

// Spec (open-links S4, S7): Ctrl+S sends the issue; the form stays, says
// it is sending, and ignores Esc and a second Ctrl+S until the answer.
#[test]
fn ctrl_s_sends_the_issue_and_waits_for_the_answer() {
    let (mut m, issue) = sending(true);
    assert_eq!(issue.title, "Scroll jumps");
    assert_eq!(issue.labels, ["bug"]);
    assert!(
        issue
            .body
            .starts_with("### What happened\n\nIt jumps\nback\n\n### Environment"),
        "{}",
        issue.body
    );
    let form = texts(&m);
    assert_eq!(form[3], "  Scroll jumps");
    assert_eq!(hint(&m), "Sending to GitHub…");
    assert_eq!(ctrl(&mut m, 's'), MenuCommand::None);
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(m.in_sub_level(), "Esc does not leave while sending");
    assert_eq!(texts(&m), form);
    assert_eq!(hint(&m), "Sending to GitHub…");
}

// Spec (open-links S7): once sent, the view shows the issue's link on a
// row of its own; Ctrl+O opens it and Esc goes back to the row.
#[test]
fn a_sent_issue_shows_its_link_which_ctrl_o_opens() {
    let (mut m, _) = sending(true);
    assert_eq!(
        m.feedback_reply(SendReply::Sent(ISSUE_URL.to_owned())),
        MenuCommand::None
    );
    assert_eq!(texts(&m), ["Feedback", "Sent:", ISSUE_URL]);
    assert_eq!(hint(&m), HINT_SENT);
    assert_eq!(HINT_SENT, "Ctrl+O open · Esc back");
    assert_eq!(ctrl(&mut m, 'o'), MenuCommand::Open(ISSUE_URL.to_owned()));
    assert_eq!(texts(&m), ["Feedback", "Sent:", ISSUE_URL]);
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(!m.in_sub_level());
    assert_eq!(value_of(&m, "Feedback"), "bug report or idea");
    // Without an opener Ctrl+O copies the link.
    let (mut m, _) = sending(false);
    m.feedback_reply(SendReply::Sent(ISSUE_URL.to_owned()));
    assert_eq!(hint(&m), HINT_SENT_COPY);
    assert_eq!(ctrl(&mut m, 'o'), MenuCommand::Copy(ISSUE_URL.to_owned()));
}

// Spec (open-links S7): a failure keeps the form with what was typed and
// shows gh's message, sanitized; Ctrl+S sends again.
#[test]
fn a_failed_send_keeps_the_form_with_the_message() {
    let (mut m, _) = sending(true);
    let form = texts(&m);
    assert_eq!(
        m.feedback_reply(SendReply::Failed(
            "HTTP 401: Bad credentials\n\u{1b}[0m".to_owned()
        )),
        MenuCommand::None
    );
    assert_eq!(texts(&m), form);
    assert_eq!(m.footer(), Footer::Error("gh: HTTP 401: Bad credentials"));
    assert!(matches!(ctrl(&mut m, 's'), MenuCommand::SendIssue(_)));
}

// Spec (open-links S5): without gh, the answer opens the prefilled page
// (or copies it with no opener) and the view says why.
#[test]
fn without_gh_the_prefilled_page_opens_and_the_view_says_why() {
    let (mut m, _) = sending(true);
    assert_eq!(
        m.feedback_reply(SendReply::NoGh),
        MenuCommand::Open(prefilled())
    );
    assert_eq!(
        texts(&m),
        [
            "Feedback",
            "gh is not installed or not logged in:",
            "opened the form in your browser, for you to",
            "review and send.",
        ]
    );
    assert_eq!(hint(&m), HINT_DONE);
    let (mut m, _) = sending(false);
    assert_eq!(
        m.feedback_reply(SendReply::NoGh),
        MenuCommand::Copy(prefilled())
    );
    assert_eq!(
        texts(&m),
        [
            "Feedback",
            "gh is not installed or not logged in: link",
            "copied. Paste it in your browser to review",
            "and send the issue.",
        ]
    );
    for line in feedback::NO_GH_OPENED.iter().chain(&feedback::NO_GH_COPIED) {
        assert!(line.chars().count() <= 46, "{line}");
    }
    assert_eq!(send(&mut m, KeyCode::Esc), MenuCommand::None);
    assert!(!m.in_sub_level());
}

// Review finding (opener failure): when the browser did not open, the view
// says the link was copied instead, and later links are copied.
#[test]
fn a_failed_open_turns_the_view_into_the_copied_one() {
    let (mut m, _) = sending(true);
    assert_eq!(
        m.feedback_reply(SendReply::NoGh),
        MenuCommand::Open(prefilled())
    );
    m.open_failed();
    assert_eq!(texts(&m)[1..], feedback::NO_GH_COPIED);
    assert_eq!(m.open_or_copy(prefilled()), MenuCommand::Copy(prefilled()));
    let mut m = filled(true);
    assert!(matches!(ctrl(&mut m, 'o'), MenuCommand::Open(_)));
    m.open_failed();
    assert_eq!(texts(&m)[1..], feedback::COPIED);
}

// Spec (open-links S7): an answer nobody waits for changes nothing.
#[test]
fn a_late_answer_changes_nothing() {
    let mut m = filled(true);
    let before = m.clone();
    for reply in [
        SendReply::Sent(ISSUE_URL.to_owned()),
        SendReply::Failed("x".to_owned()),
        SendReply::NoGh,
    ] {
        assert_eq!(m.feedback_reply(reply), MenuCommand::None);
        assert_eq!(m, before);
    }
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
        ..Default::default()
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
