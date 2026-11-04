//! Tests of the feedback form (config-menu, Feedback), on its own state.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::feedback::{
    DESCRIPTION_REQUIRED, Facts, Feedback, Field, HINT_SIMILAR, HINT_TEXT, Issue, Kind,
    NO_GH_TOO_LONG, Outcome, REDACTED, Request, SENDING, SIMILAR, SIMILAR_ADD, SIMILAR_NEW,
    Similar, TITLE_REQUIRED, TOO_LONG, comment_body, comment_url, gh_message, host, issue_url,
    search_query, similar,
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

fn ctrl_o(form: &mut Feedback) -> Outcome {
    form.on_key(
        &KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        false,
    )
}

/// The issue Ctrl+S sends, or a panic.
fn sent(form: &mut Feedback) -> Issue {
    match ctrl_s(form) {
        Outcome::Send(Request::Find(issue)) => issue,
        other => panic!("not sent: {other:?}"),
    }
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

// Spec (open-links S4): Ctrl+O still gives the prefilled page.
#[test]
fn ctrl_o_gives_the_prefilled_bug_report_link() {
    let mut form = filled(Kind::Bug);
    assert_eq!(
        ctrl_o(&mut form),
        Outcome::Open(format!(
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
        ctrl_o(&mut form),
        Outcome::Open(format!(
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
    assert!(!form.sending(), "nothing was sent");
    let issue = sent(&mut form);
    assert!(!issue.body.contains("hf_abc"), "{}", issue.body);
    assert!(issue.body.contains("[REDACTED]"), "{}", issue.body);
    assert_eq!(
        REDACTED,
        "Secrets were masked; review and press Ctrl+S again"
    );
}

// Spec (open-links S4): the link cap is only for the Ctrl+O link; the
// same text is sent with Ctrl+S.
#[test]
fn a_link_too_long_for_github_is_refused_but_still_sent() {
    let mut form = filled(Kind::Bug);
    form.paste(&"ñ".repeat(1990));
    assert_eq!(ctrl_o(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(TOO_LONG));
    assert_eq!(TOO_LONG, "Too long for a link: shorten the description");
    let description = form.description();
    let issue = sent(&mut form);
    assert!(issue.body.contains(&description), "{}", issue.body);
    assert_eq!(form.error(), None);
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

const ENVIRONMENT: &str = "### Environment\n\n```text\n\
     lazarobox 0.25.1 · linux (WSL) · zsh · WezTerm\n\
     engines: ollama available · llamacpp not running\n```";

// Spec (open-links S4, S6): Ctrl+S sends a bug report shaped like the
// repository's bug form, labelled `bug`, and the form says it is sending.
#[test]
fn ctrl_s_sends_the_bug_report_shaped_like_the_issue_form() {
    let mut form = filled(Kind::Bug);
    assert_eq!(
        ctrl_s(&mut form),
        Outcome::Send(Request::Find(Issue {
            repo: "pichu2707/lazarobox-shell".to_owned(),
            title: "Scroll jumps".to_owned(),
            body: format!("### What happened\n\nIt jumps\nto the top & back\n\n{ENVIRONMENT}"),
            labels: vec!["bug".to_owned()],
        }))
    );
    assert!(form.sending());
    assert_eq!(form.hint(), SENDING);
    assert_eq!(SENDING, "Sending to GitHub…");
    assert_eq!(form.error(), None);
}

// Spec (open-links S6): an idea goes as the idea form, labelled `idea`,
// with no environment when Attach is off.
#[test]
fn ctrl_s_sends_an_idea_without_the_environment() {
    let mut form = filled(Kind::Idea);
    while form.field() != Field::Attach {
        send(&mut form, KeyCode::Tab);
    }
    send(&mut form, KeyCode::Char(' '));
    let issue = sent(&mut form);
    assert_eq!(issue.title, "Scroll jumps");
    assert_eq!(issue.body, "### The idea\n\nIt jumps\nto the top & back");
    assert_eq!(issue.labels, ["idea"]);
    assert_eq!(issue.repo, "pichu2707/lazarobox-shell");
}

// Spec (open-links S4): validation still stops the send.
#[test]
fn an_incomplete_form_is_not_sent() {
    let mut form = Feedback::new(facts());
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(form.error(), Some(TITLE_REQUIRED));
    assert!(!form.sending());
}

// Spec (open-links S7): one send at a time; while it runs every key,
// Ctrl+S and Esc included, does nothing, so the form sent stays as shown.
#[test]
fn while_sending_every_key_does_nothing() {
    let mut form = filled(Kind::Bug);
    sent(&mut form);
    let before = form.clone();
    assert_eq!(ctrl_s(&mut form), Outcome::Handled);
    assert_eq!(ctrl_o(&mut form), Outcome::Handled);
    assert_eq!(send(&mut form, KeyCode::Esc), Outcome::Handled);
    send(&mut form, KeyCode::Char('x'));
    send(&mut form, KeyCode::Tab);
    form.paste("pasted");
    assert_eq!(form, before);
}

// Spec (open-links S7): a failure keeps everything typed, shows gh's
// message on one line, and Ctrl+S sends again.
#[test]
fn a_failed_send_keeps_the_form_and_shows_why() {
    let mut form = filled(Kind::Bug);
    sent(&mut form);
    form.failed("HTTP 502:\n\u{1b}[31mBad gateway\u{7}");
    assert!(!form.sending());
    assert_eq!(form.error(), Some("gh: HTTP 502: Bad gateway"));
    assert_eq!(form.title(), "Scroll jumps");
    assert_eq!(form.description(), "It jumps\nto the top & back");
    sent(&mut form);
    assert_eq!(form.error(), None);
}

// Spec (open-links S5): without gh the prefilled link is the fallback; a
// link too long for GitHub keeps the form and says why.
#[test]
fn without_gh_the_link_is_the_fallback() {
    let mut form = filled(Kind::Bug);
    let Outcome::Open(link) = ctrl_o(&mut form) else {
        panic!("no link");
    };
    sent(&mut form);
    assert_eq!(form.fallback(), Some(link));
    assert!(!form.sending());
    let mut long = filled(Kind::Bug);
    long.paste(&"ñ".repeat(1990));
    sent(&mut long);
    assert_eq!(long.fallback(), None);
    assert!(!long.sending());
    assert_eq!(long.error(), Some(NO_GH_TOO_LONG));
    assert!(NO_GH_TOO_LONG.chars().count() <= 48);
}

// Spec (open-links S7): gh's message is shown on one line, with no control
// character, capped at the footer's 48 columns.
#[test]
fn gh_messages_are_one_capped_line() {
    assert_eq!(
        gh_message("\u{1b}[1;31mHTTP 422:\tValidation\r\nFailed\u{7}\n"),
        "gh: HTTP 422: Validation Failed"
    );
    assert_eq!(gh_message(" \n\u{1b}[0m"), "gh failed");
    let long = gh_message(&"x".repeat(200));
    assert_eq!(long.chars().count(), 48);
    assert!(long.starts_with("gh: xxx") && long.ends_with('…'), "{long}");
}

// Spec (open-links S7): only a one-line link to an issue of this
// repository counts as the created issue.
#[test]
fn only_an_issue_link_of_the_repository_is_accepted() {
    let repo = "pichu2707/lazarobox-shell";
    let url = "https://github.com/pichu2707/lazarobox-shell/issues/42";
    assert_eq!(issue_url(&format!("{url}\n"), repo), Some(url.to_owned()));
    for bad in [
        "",
        "created",
        "https://github.com/other/repo/issues/42",
        "https://github.com/pichu2707/lazarobox-shell/issues/",
        "https://github.com/pichu2707/lazarobox-shell/issues/4a",
        "https://github.com/pichu2707/lazarobox-shell/pull/42",
        "http://github.com/pichu2707/lazarobox-shell/issues/42",
        "https://github.com/pichu2707/lazarobox-shell/issues/42\nmore",
        "https://github.com/pichu2707/lazarobox-shell/issues/42 x",
    ] {
        assert_eq!(issue_url(bad, repo), None, "{bad:?}");
    }
}

fn prefilled(text: &str) -> Feedback {
    Feedback::prefilled(Facts::default(), text)
}

// Issue #551: the form needs both fields, so one line fills both.
#[test]
fn one_line_of_prefilled_text_is_the_title_and_the_description() {
    let form = prefilled("The chat hangs");
    assert_eq!(form.title(), "The chat hangs");
    assert_eq!(form.description(), "The chat hangs");
    assert_eq!(form.field(), Field::Kind);
    assert_eq!(form.kind(), Kind::Bug);
}

// Issue #551.
#[test]
fn the_first_prefilled_line_is_the_title_and_the_rest_the_description() {
    let form = prefilled("The chat hangs\r\n\r\nAfter a tool call.\nEvery time.");
    assert_eq!(form.title(), "The chat hangs");
    assert_eq!(form.description(), "After a tool call.\nEvery time.");
}

// Issue #551: a first line too long for the title stays whole below it.
#[test]
fn a_first_line_longer_than_the_title_is_kept_in_the_description() {
    let line = "x".repeat(crate::core::menu::feedback::TITLE_CAP + 5);
    let form = prefilled(&format!("{line}\nmore"));
    assert_eq!(
        form.title().chars().count(),
        crate::core::menu::feedback::TITLE_CAP
    );
    assert_eq!(form.description(), format!("{line}\nmore"));
}

// Issue #551.
#[test]
fn no_prefilled_text_is_the_empty_form() {
    assert_eq!(prefilled("  "), Feedback::new(Facts::default()));
}

// Issue duplicates: what the search keys on, and what it keeps.

fn found(issues: &[(u64, &str)]) -> Vec<Similar> {
    (issues.iter())
        .map(|(number, title)| Similar {
            number: *number,
            title: (*title).to_owned(),
        })
        .collect()
}

#[test]
fn the_search_ors_the_key_words_of_the_title() {
    assert_eq!(
        search_query("Scroll jumps to the top").as_deref(),
        Some("scroll OR jumps OR top in:title,body")
    );
    assert_eq!(
        search_query("¿Por qué el menú se cierra?").as_deref(),
        Some("qué OR menú OR cierra in:title,body")
    );
    // Only letters and digits reach the query: no qualifier, quote or
    // parenthesis of the title gets through.
    let query = search_query("repo:other/x \"is:closed\" (label:bug) crash").unwrap();
    assert_eq!(
        query,
        "repo OR other OR closed OR label OR bug OR crash in:title,body"
    );
    // At most six words, so at most five OR.
    let long = search_query("one1 two2 three3 four4 five5 six6 seven7").unwrap();
    assert_eq!(long.matches(" OR ").count(), 5);
    assert_eq!(search_query("a: b, to"), None);
}

#[test]
fn only_issues_sharing_two_key_words_are_similar() {
    let title = "write: an env template is refused";
    let issues = found(&[
        (
            583,
            "write: an env template like .env.example is refused without asking",
        ),
        (540, "Goal: storage management for users"),
        (12, "env var not read"),
        (77, "Refused write in a template folder"),
    ]);
    let kept: Vec<u64> = similar(title, issues).iter().map(|s| s.number).collect();
    assert_eq!(kept, [583, 77], "one shared word (`env`) is not enough");
    // A title of one key word needs only that one.
    let kept = similar("Crash", found(&[(1, "crash on start"), (2, "slow")]));
    assert_eq!(kept.iter().map(|s| s.number).collect::<Vec<_>>(), [1]);
    // At most five.
    let many: Vec<(u64, &str)> = (1..=8).map(|n| (n, "env template refused")).collect();
    assert_eq!(similar(title, found(&many)).len(), 5);
}

#[test]
fn the_comment_is_the_title_and_the_issue_body() {
    let issue = Issue {
        repo: "o/r".to_owned(),
        title: "Scroll jumps".to_owned(),
        body: "### What happened\n\nIt jumps".to_owned(),
        labels: vec!["bug".to_owned()],
    };
    assert_eq!(
        comment_body(&issue),
        "**Scroll jumps**\n\n### What happened\n\nIt jumps"
    );
}

#[test]
fn only_a_comment_link_of_the_repository_is_accepted() {
    let repo = "pichu2707/lazarobox-shell";
    let url = "https://github.com/pichu2707/lazarobox-shell/issues/583#issuecomment-123";
    assert_eq!(comment_url(&format!("{url}\n"), repo).as_deref(), Some(url));
    for out in [
        "https://github.com/pichu2707/lazarobox-shell/issues/583",
        "https://github.com/other/repo/issues/583#issuecomment-1",
        "https://github.com/pichu2707/lazarobox-shell/issues/583#issuecomment-x",
        "done",
    ] {
        assert_eq!(comment_url(out, repo), None, "{out}");
    }
}

/// A filled form after its Ctrl+S, told GitHub has `issues` like it.
fn with_similar(issues: &[(u64, &str)]) -> Feedback {
    let mut form = filled(Kind::Bug);
    sent(&mut form);
    form.found(found(issues));
    form
}

// S3: similar issues replace the fields, the first selected; ↑/↓ move.
#[test]
fn similar_issues_are_listed_instead_of_the_fields() {
    let mut form = with_similar(&[
        (583, "Scroll jumps up"),
        (540, "Scroll\u{1b}[31m jumps\n down"),
    ]);
    assert!(!form.sending());
    assert_eq!(
        form.lines(),
        [
            SIMILAR,
            "› #583 Scroll jumps up",
            "  #540 Scroll [31m jumps down",
            "",
            SIMILAR_ADD,
            SIMILAR_NEW,
        ]
    );
    assert_eq!(form.hint(), HINT_SIMILAR);
    assert!(HINT_SIMILAR.chars().count() <= 48);
    send(&mut form, KeyCode::Down);
    send(&mut form, KeyCode::Down);
    assert_eq!(form.lines()[2], "› #540 Scroll [31m jumps down");
    send(&mut form, KeyCode::Up);
    assert_eq!(form.lines()[1], "› #583 Scroll jumps up");
    // Typing does not reach the fields while the list is shown.
    send(&mut form, KeyCode::Char('x'));
    form.paste("pasted");
    assert_eq!(form.title(), "Scroll jumps");
}

// S2: Enter adds the report to the selected issue, with no new issue.
#[test]
fn enter_adds_the_report_to_the_selected_issue() {
    let mut form = with_similar(&[(583, "a"), (540, "b")]);
    send(&mut form, KeyCode::Down);
    let Outcome::Send(Request::Comment { issue, number }) = send(&mut form, KeyCode::Enter) else {
        panic!("not a comment");
    };
    assert_eq!(number, 540);
    assert_eq!(issue.title, "Scroll jumps");
    assert!(issue.body.contains("It jumps"));
    assert!(form.sending());
    assert_eq!(form.similar_issues(), None);
}

// S3: Ctrl+S on the list creates the issue anyway; Esc goes back to the
// fields as typed.
#[test]
fn ctrl_s_on_the_list_creates_and_esc_goes_back() {
    let mut form = with_similar(&[(583, "a")]);
    let Outcome::Send(Request::Create(issue)) = ctrl_s(&mut form) else {
        panic!("not created");
    };
    assert_eq!(issue.title, "Scroll jumps");
    let mut form = with_similar(&[(583, "a")]);
    assert_eq!(send(&mut form, KeyCode::Esc), Outcome::Handled);
    assert_eq!(form.similar_issues(), None);
    assert_eq!(form.title(), "Scroll jumps");
    assert_eq!(form.hint(), HINT_TEXT);
    // A second Esc closes the form as before.
    assert_eq!(send(&mut form, KeyCode::Esc), Outcome::Close);
}

// S3: an empty answer is the form as before.
#[test]
fn no_similar_issue_keeps_the_fields() {
    let form = with_similar(&[]);
    assert_eq!(form.similar_issues(), None);
    assert!(!form.sending());
    assert_eq!(form.lines()[0], "  Kind: Bug report");
    assert_eq!(form.hint(), HINT_TEXT);
}
