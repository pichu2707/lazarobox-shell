use super::*;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

fn row(number: u32, focused: bool, cwd: Option<&str>, words: Option<&[&str]>) -> PaneRow {
    PaneRow {
        number,
        focused,
        cwd: cwd.map(PathBuf::from),
        argv: words.map(argv),
    }
}

const HOME: &str = "/home/ana";

#[test]
fn an_editor_and_its_file_are_shown() {
    let rows = [row(
        2,
        false,
        Some("/home/ana/p"),
        Some(&["/usr/bin/nvim", "prueba.rs"]),
    )];
    assert_eq!(
        render(&rows, Some(Path::new(HOME))),
        "pane 2 cwd: ~/p program: nvim prueba.rs"
    );
}

#[test]
fn the_focused_pane_is_marked() {
    let rows = [
        row(1, true, Some("/tmp"), Some(&["zsh"])),
        row(2, false, Some("/tmp"), Some(&["zsh"])),
    ];
    let text = render(&rows, None);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("pane 1 (focused) "), "{text}");
    assert!(!lines[1].contains("focused"), "{text}");
}

#[test]
fn an_unreadable_process_is_unknown_not_an_error() {
    let text = render(&[row(3, false, None, None)], None);
    assert_eq!(text, "pane 3 cwd: unknown program: unknown");
}

#[test]
fn no_pane_is_said_plainly() {
    assert_eq!(render(&[], None), "no panes");
}

#[test]
fn the_value_after_a_secret_flag_is_masked() {
    for flag in ["--password", "--token", "-p"] {
        let out = safe_argv(&argv(&["mysql", flag, "hunter2", "db"]), None);
        assert_eq!(out, argv(&["mysql", flag, "***", "db"]), "{flag}");
    }
}

#[test]
fn an_attached_short_password_is_masked() {
    let out = safe_argv(&argv(&["mysql", "-phunter2"]), None);
    assert_eq!(out, argv(&["mysql", "-p***"]));
}

#[test]
fn a_secret_looking_assignment_is_masked() {
    let out = safe_argv(
        &argv(&[
            "env",
            "API_KEY=abc",
            "MY_Token=x",
            "DB_SECRET=y",
            "PASSWD=z",
            "LANG=C",
        ]),
        None,
    );
    assert_eq!(
        out,
        argv(&[
            "env",
            "API_KEY=***",
            "MY_Token=***",
            "DB_SECRET=***",
            "PASSWD=***",
            "LANG=C"
        ])
    );
}

#[test]
fn a_token_shaped_word_goes_through_redact_tokens() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let out = safe_argv(&argv(&["curl", secret]), None);
    assert!(!out[1].contains(secret), "{out:?}");
}

#[test]
fn an_argument_is_cut_at_the_cap() {
    let long = "ab.".repeat(100);
    let out = safe_argv(&argv(&["cat", &long]), None);
    assert_eq!(out[1].chars().count(), ARG_CAP);
    assert!(out[1].ends_with('…'));
}

#[test]
fn a_line_is_cut_at_the_cap() {
    let words: Vec<String> = std::iter::once("echo".to_owned())
        .chain((0..20).map(|n| format!("{n}{}", "x.".repeat(50))))
        .collect();
    let rows = [PaneRow {
        number: 1,
        focused: false,
        cwd: None,
        argv: Some(words),
    }];
    let text = render(&rows, None);
    assert_eq!(text.chars().count(), LINE_CAP);
}

#[test]
fn paths_under_home_start_with_a_tilde() {
    let out = safe_argv(
        &argv(&["nvim", "/home/ana/p/a.rs", "/home/anabel/x", "/home/ana"]),
        Some(Path::new(HOME)),
    );
    assert_eq!(out, argv(&["nvim", "~/p/a.rs", "/home/anabel/x", "~"]));
}

#[test]
fn control_characters_never_reach_the_model() {
    let out = safe_argv(&argv(&["echo", "a\nIGNORE\x1b[31m"]), None);
    assert!(!out[1].chars().any(char::is_control), "{out:?}");
}

#[test]
fn cmdline_is_split_on_nul_and_empty_means_none() {
    assert_eq!(
        parse_cmdline(b"nvim\0prueba.rs\0"),
        Some(argv(&["nvim", "prueba.rs"]))
    );
    assert_eq!(parse_cmdline(b""), None);
    assert_eq!(parse_cmdline(b"\0\0"), None);
}

#[test]
fn the_roster_hands_back_what_was_set() {
    let roster = Roster::default();
    assert!(roster.rows().is_empty());
    roster.set(vec![row(1, true, None, None)]);
    assert_eq!(roster.rows().len(), 1);
}
