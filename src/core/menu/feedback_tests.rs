//! Tests of the feedback form (config-menu, Feedback), on its own state.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::feedback::{
    DESCRIPTION_REQUIRED, Facts, Feedback, Field, Kind, Outcome, REDACTED, TITLE_REQUIRED,
    TOO_LONG, host,
};

const ISSUES: &str = "https://github.com/pichu2707/lazarobox-shell/issues/new";

fn facts() -> Facts {
    Facts {
        version: "0.25.1".to_owned(),
        os: "linux".to_owned(),
        wsl: true,
        shell: Some("zsh".to_owned()),
        terminal: Some("WezTerm".to_owned()),
        engines: Some(vec![
            "ollama available".to_owned(),
            "llamacpp not running".to_owned(),
        ]),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn send(form: &mut Feedback, code: KeyCode) -> Outcome {
    form.on_key(&key(code), false)
}

fn ctrl_s(form: &mut Feedback) -> Outcome {
    form.on_key(
        &KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        false,
    )
}

fn type_text(form: &mut Feedback, text: &str) {
    for c in text.chars() {
        send(form, KeyCode::Char(c));
    }
}

/// A form with a title and a description, focus back on the kind.
fn filled(kind: Kind) -> Feedback {
    let mut form = Feedback::new(facts());
    if kind == Kind::Idea {
        send(&mut form, KeyCode::Right);
    }
    send(&mut form, KeyCode::Tab);
    type_text(&mut form, "Scroll jumps");
    send(&mut form, KeyCode::Tab);
    type_text(&mut form, "It jumps");
    send(&mut form, KeyCode::Enter);
    type_text(&mut form, "to the top & back");
    form
}

#[test]
fn the_facts_read_as_two_lines() {
    assert_eq!(
        facts().text(),
        "lazarobox 0.25.1 · linux (WSL) · zsh · WezTerm\n\
         engines: ollama available · llamacpp not running"
    );
    let bare = Facts {
        version: "0.25.1".to_owned(),
        os: "macos".to_owned(),
        wsl: false,
        shell: None,
        terminal: None,
        engines: None,
    };
    assert_eq!(
        bare.text(),
        "lazarobox 0.25.1 · macos\nengines: not detected"
    );
}

#[test]
fn the_form_starts_on_a_bug_report_and_left_right_cycle_the_kind() {
    let mut form = Feedback::new(facts());
    assert_eq!(form.kind(), Kind::Bug);
    assert_eq!(form.field(), Field::Kind);
    assert!(form.attach(), "the environment is attached by default");
    send(&mut form, KeyCode::Right);
    assert_eq!(form.kind(), Kind::Idea);
    send(&mut form, KeyCode::Left);
    assert_eq!(form.kind(), Kind::Bug);
    // Tab walks the fields and wraps; BackTab goes back.
    for field in [Field::Title, Field::Description, Field::Attach, Field::Kind] {
        send(&mut form, KeyCode::Tab);
        assert_eq!(form.field(), field);
    }
    send(&mut form, KeyCode::BackTab);
    assert_eq!(form.field(), Field::Attach);
}

#[test]
fn ctrl_s_gives_the_prefilled_bug_report_link() {
    let mut form = filled(Kind::Bug);
    assert_eq!(
        ctrl_s(&mut form),
        Outcome::Copy(format!(
            "{ISSUES}?template=bug.yml&title=Scroll%20jumps\
             &what-happened=It%20jumps%0Ato%20the%20top%20%26%20back\
             &environment=lazarobox%200.25.1%20%C2%B7%20linux%20%28WSL%29%20%C2%B7%20zsh\
             %20%C2%B7%20WezTerm%0Aengines%3A%20ollama%20available%20%C2%B7%20llamacpp\
             %20not%20running"
        ))
    );
}

#[test]
fn an_idea_goes_to_the_idea_form_and_the_environment_can_be_left_out() {
    let mut form = filled(Kind::Idea);
    while form.field() != Field::Attach {
        send(&mut form, KeyCode::Tab);
    }
    send(&mut form, KeyCode::Char(' '));
    assert!(!form.attach());
    assert_eq!(
        ctrl_s(&mut form),
        Outcome::Copy(format!(
            "{ISSUES}?template=idea.yml&title=Scroll%20jumps\
             &idea=It%20jumps%0Ato%20the%20top%20%26%20back"
        ))
    );
}

#[test]
fn a_title_and_a_description_are_required() {
    let mut form = Feedback::new(facts());
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(TITLE_REQUIRED));
    assert_eq!(form.field(), Field::Title, "the cursor goes to the field");
    type_text(&mut form, "t");
    assert_eq!(form.error(), None, "typing clears the error");
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(DESCRIPTION_REQUIRED));
    assert_eq!(form.field(), Field::Description);
    assert_eq!(TITLE_REQUIRED, "Title is required");
    assert_eq!(DESCRIPTION_REQUIRED, "Description is required");
}

// A secret never reaches the link: it is masked in the form first, so the
// user sees what changed before copying again.
#[test]
fn a_secret_is_masked_in_the_form_before_any_link() {
    let mut form = filled(Kind::Bug);
    form.paste(" token=hf_abcdefghijklmnopqrstuvwxyz");
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(REDACTED));
    assert!(
        form.description().contains("[REDACTED]"),
        "{}",
        form.description()
    );
    assert!(!form.description().contains("hf_abc"));
    let Outcome::Copy(link) = ctrl_s(&mut form) else {
        panic!("not copied");
    };
    assert!(!link.contains("hf_abc"), "{link}");
    assert_eq!(
        REDACTED,
        "Secrets were masked; review and press Ctrl+S again"
    );
}

#[test]
fn a_link_too_long_for_github_is_refused() {
    let mut form = filled(Kind::Bug);
    form.paste(&"ñ".repeat(1990));
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(TOO_LONG));
    assert_eq!(TOO_LONG, "Too long for a link: shorten the description");
}

#[test]
fn the_title_is_one_line_and_enter_there_moves_on() {
    let mut form = Feedback::new(facts());
    send(&mut form, KeyCode::Tab);
    form.paste("a\nb");
    assert_eq!(form.title(), "a b");
    send(&mut form, KeyCode::Enter);
    assert_eq!(form.field(), Field::Description);
    form.paste("x\r\ny");
    assert_eq!(form.description(), "x\ny");
}

#[test]
fn esc_closes_the_form() {
    let mut form = filled(Kind::Bug);
    assert_eq!(send(&mut form, KeyCode::Esc), Outcome::Close);
    assert_eq!(form.on_key(&key(KeyCode::Esc), true), Outcome::Handled);
}

// Spec (config-menu, Feedback): what is attached comes from the
// environment and never carries a path or a control character.
#[test]
fn the_host_facts_come_from_the_environment() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        }
    };
    let wsl = host(
        "0.25.1",
        "linux",
        env(&[("SHELL", "/usr/bin/zsh"), ("WT_SESSION", "x")]),
        Some("6.6.87.2-microsoft-standard-WSL2"),
    );
    assert_eq!(
        wsl,
        Facts {
            version: "0.25.1".to_owned(),
            os: "linux".to_owned(),
            wsl: true,
            shell: Some("zsh".to_owned()),
            terminal: Some("Windows Terminal".to_owned()),
            engines: None,
        }
    );
    // `WSL_DISTRO_NAME` is enough; `TERM_PROGRAM` wins over `WT_SESSION`.
    let named = host(
        "1",
        "linux",
        env(&[
            ("WSL_DISTRO_NAME", "Ubuntu"),
            ("TERM_PROGRAM", "WezTerm"),
            ("WT_SESSION", "x"),
        ]),
        None,
    );
    assert!(named.wsl);
    assert_eq!(named.terminal.as_deref(), Some("WezTerm"));
    // Plain Linux and macOS.
    let mac = host("1", "macos", env(&[("TERM_PROGRAM", "iTerm.app")]), None);
    assert!(!mac.wsl);
    assert_eq!(mac.shell, None);
    assert_eq!(mac.terminal.as_deref(), Some("iTerm.app"));
    assert!(!host("1", "linux", env(&[]), Some("6.8.0-45-generic")).wsl);
    // A value that is not a plain name is left out, and a long one is cut.
    let odd = host(
        "1",
        "linux",
        env(&[("SHELL", "/bin/a\u{1b}b"), ("TERM_PROGRAM", "x x")]),
        None,
    );
    assert_eq!(odd.shell, None);
    assert_eq!(odd.terminal.as_deref(), Some("x x"));
    let long = host(
        "1",
        "linux",
        env(&[(
            "TERM_PROGRAM",
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
        )]),
        None,
    );
    assert_eq!(long.terminal.map(|t| t.chars().count()), Some(40));
}
