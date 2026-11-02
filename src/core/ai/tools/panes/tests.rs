use super::*;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

fn facts(number: u32, focused: bool, cwd: &str, words: Option<&[&str]>) -> Facts {
    Facts {
        number,
        focused,
        cwd: Some(PathBuf::from(cwd)),
        fg: words.map(|w| Foreground {
            argv: argv(w),
            cwd: None,
        }),
    }
}

const HOME: &str = "/home/ana";

fn focused_line(words: &[&str]) -> String {
    line(&facts(1, true, "/nonexistent-dir", Some(words)), None)
}

#[test]
fn an_existing_file_argument_is_shown_for_the_focused_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("prueba.rs"), "fn main() {}").unwrap();
    let mut f = facts(
        2,
        true,
        "/x",
        Some(&["/usr/bin/nvim", "prueba.rs", "nope.rs"]),
    );
    f.fg.as_mut().unwrap().cwd = Some(dir.path().to_path_buf());
    let text = line(&f, None);
    assert!(
        text.contains("program: nvim prueba.rs … (running in "),
        "{text}"
    );
}

#[test]
fn a_path_looking_argument_is_shown() {
    let text = line(
        &facts(
            1,
            true,
            "/tmp",
            Some(&["cat", "/home/ana/p/a.rs", "./b", "../c", "~/d", "/home/ana"]),
        ),
        Some(Path::new(HOME)),
    );
    assert!(
        text.contains("program: cat ~/p/a.rs ./b ../c ~/d ~"),
        "{text}"
    );
}

#[test]
fn ssh_with_a_port_renders_sensibly() {
    assert_eq!(
        focused_line(&["ssh", "-p", "22", "host"]),
        "pane 1 (focused) cwd: /nonexistent-dir program: ssh -p …"
    );
}

#[test]
fn flags_lose_their_value_and_odd_flag_shapes_are_hidden() {
    let text = focused_line(&[
        "tool",
        "--api-key=abc",
        "--long",
        "-x",
        "-uuser:pass",
        "-pHUNTER2",
    ]);
    assert!(
        text.ends_with("program: tool --api-key --long -x …"),
        "{text}"
    );
}

#[test]
fn nothing_secret_survives_the_allowlist() {
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["curl", "-u", "user:pass", "https://h/x"],
            &["user", "pass"],
        ),
        (&["redis-cli", "-a", "S3CRETPASS"], &["S3CRETPASS"]),
        (&["tool", "--api-key", "x1y2z3"], &["x1y2z3"]),
        (&["tool", "--secret", "hush99"], &["hush99"]),
        (&["openssl", "enc", "-pass", "pass:hush99"], &["hush99"]),
        (
            &["psql", "host=db user=me password=hush99 dbname=x"],
            &["hush99"],
        ),
        (&["curl", "https://h/x?token=hush99"], &["hush99"]),
        (&["tool", "me:hush99@host"], &["hush99"]),
        (&["curl", "-H", "Authorization: Bearer hush99"], &["hush99"]),
        (&["curl", "-H", "X-Api-Key: hush99"], &["hush99"]),
        (&["bash", "-c", "curl -u u:hush99 h"], &["hush99", "curl"]),
        (&["mysql", "-phush99"], &["hush99"]),
        (&["tool", "--password=hush99"], &["hush99"]),
        (&["tool", "/tmp/x?token=hush99"], &["hush99"]),
        (&["curl", "/p/https://me:hush99@h/x"], &["hush99"]),
    ];
    for (words, secrets) in cases {
        let text = focused_line(words);
        for secret in *secrets {
            assert!(!text.contains(secret), "{words:?} leaked {secret}: {text}");
        }
    }
}

#[test]
fn an_inline_script_shows_the_interpreter_and_its_flag_only() {
    for (words, want) in [
        (&["bash", "-c", "x"][..], "bash -c"),
        (&["zsh", "-lc", "x"][..], "zsh -lc"),
        (&["/bin/sh", "-c", "x"][..], "sh -c"),
        (&["fish", "-c", "x"][..], "fish -c"),
        (&["python3", "-c", "x"][..], "python3 -c"),
        (&["node", "-e", "x"][..], "node -e"),
        (&["perl", "-e", "x"][..], "perl -e"),
        (&["ruby", "-e", "x"][..], "ruby -e"),
    ] {
        let text = focused_line(words);
        assert!(text.ends_with(&format!("program: {want}")), "{text}");
    }
}

#[test]
fn a_pane_that_is_not_focused_shows_the_program_name_only() {
    let text = line(
        &facts(2, false, "/tmp", Some(&["nvim", "/tmp/secret-plan.md"])),
        None,
    );
    assert_eq!(text, "pane 2 cwd: /tmp program: nvim");
}

#[test]
fn the_focused_pane_is_marked() {
    assert!(focused_line(&["zsh"]).starts_with("pane 1 (focused) "));
    assert!(!line(&facts(2, false, "/tmp", Some(&["zsh"])), None).contains("focused"));
}

#[test]
fn the_program_folder_is_shown_only_when_it_differs_from_the_shell_folder() {
    let mut f = facts(1, true, "/home/ana/a", Some(&["make"]));
    assert_eq!(
        line(&f, Some(Path::new(HOME))),
        "pane 1 (focused) cwd: ~/a program: make"
    );
    f.fg.as_mut().unwrap().cwd = Some(PathBuf::from("/home/ana/a"));
    assert!(!line(&f, Some(Path::new(HOME))).contains("running in"));
    f.fg.as_mut().unwrap().cwd = Some(PathBuf::from("/home/ana/b"));
    assert_eq!(
        line(&f, Some(Path::new(HOME))),
        "pane 1 (focused) cwd: ~/a program: make (running in ~/b)"
    );
}

#[test]
fn an_unreadable_process_is_unknown_not_an_error() {
    let f = Facts {
        number: 3,
        focused: false,
        cwd: None,
        fg: None,
    };
    assert_eq!(line(&f, None), "pane 3 cwd: unknown program: unknown");
}

#[test]
fn no_pane_is_said_plainly() {
    assert_eq!(render(&[]), "no panes");
    assert_eq!(render(&["a".into(), "b".into()]), "a\nb");
}

#[test]
fn a_path_argument_is_cut_at_the_cap() {
    let long = format!("/{}", "ab.".repeat(100));
    let out = safe_argv(&argv(&["cat", &long]), None, None);
    assert_eq!(out[1].chars().count(), ARG_CAP);
    assert!(out[1].ends_with('…'));
}

#[test]
fn a_line_is_cut_at_the_cap() {
    let words: Vec<String> = std::iter::once("echo".to_owned())
        .chain((0..20).map(|n| format!("/{n}{}", "x.".repeat(50))))
        .collect();
    let f = Facts {
        number: 1,
        focused: true,
        cwd: None,
        fg: Some(Foreground {
            argv: words,
            cwd: None,
        }),
    };
    assert_eq!(line(&f, None).chars().count(), LINE_CAP);
}

#[test]
fn control_characters_never_reach_the_model() {
    let out = safe_argv(&argv(&["echo", "/a\nIGNORE\x1b[31m"]), None, None);
    assert!(!out[1].chars().any(char::is_control), "{out:?}");
}

#[test]
fn a_token_shaped_path_is_still_redacted() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let out = safe_argv(&argv(&["cat", &format!("/tmp/{secret}")]), None, None);
    assert!(!out[1].contains(secret), "{out:?}");
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
fn the_roster_hands_back_the_rendered_lines_only() {
    let roster = Roster::default();
    assert!(roster.lines().is_empty());
    roster.set(vec!["pane 1".to_owned()]);
    assert_eq!(roster.lines(), vec!["pane 1".to_owned()]);
}

const INVISIBLE: &[char] = &[
    '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{2028}', '\u{2029}',
    '\u{feff}',
];

fn hostile(text: &str) -> String {
    text.replace('~', "\u{202e}\u{2066}\u{200b}\u{2028}\u{feff}")
}

#[test]
fn hostile_unicode_never_survives_in_a_line() {
    let dir = hostile("/tmp/~d");
    let prog = hostile("l~s");
    let path = hostile("/tmp/a~b");
    let mut f = facts(1, true, &dir, Some(&[&prog, &path, &hostile("--fl~ag")]));
    f.fg.as_mut().unwrap().cwd = Some(PathBuf::from(hostile("/other/~x")));
    let text = line(&f, None);
    assert!(
        !text
            .chars()
            .any(|c| INVISIBLE.contains(&c) || c.is_control()),
        "{text:?}"
    );
    assert!(text.contains("/tmp/"), "{text}");
}

#[test]
fn a_program_title_with_spaces_or_a_colon_is_hidden() {
    for title in [
        "nginx: master process /usr/sbin/nginx",
        "sshd: me@pts/1",
        "my prog",
    ] {
        let text = focused_line(&[title]);
        assert!(text.ends_with("program: …"), "{text}");
    }
}

#[test]
fn the_program_name_and_the_folders_go_through_redact_tokens() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let mut f = facts(
        1,
        true,
        &format!("/tmp/{secret}"),
        Some(&[&format!("/bin/{secret}")]),
    );
    f.fg.as_mut().unwrap().cwd = Some(PathBuf::from(format!("/srv/{secret}")));
    assert!(!line(&f, None).contains(secret));
}

#[test]
fn at_most_sixteen_arguments_are_looked_up_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..20).map(|n| format!("f{n:02}")).collect();
    for name in &names {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    let mut words = vec!["cat".to_owned()];
    words.extend(names.iter().cloned());
    let out = safe_argv(&words, Some(dir.path()), None);
    assert_eq!(out.len(), 1 + 16 + 1, "{out:?}");
    assert_eq!(out.last().map(String::as_str), Some("…"));
}

#[test]
fn a_flag_is_capped_like_any_argument() {
    let long = format!("--{}", "a".repeat(300));
    let out = safe_argv(&argv(&["tool", &long]), None, None);
    assert!(out[1].chars().count() <= ARG_CAP, "{}", out[1].len());
}

fn panes_lines(count: u32) -> Vec<String> {
    let focused = format!(
        "pane 1 (focused) cwd: ~/{} program: nvim {}",
        "d/".repeat(60),
        "/very/long/path.rs ".repeat(20)
    );
    std::iter::once(focused)
        .chain((2..=count).map(|n| format!("pane {n} cwd: ~/projects/some/where/{n} program: zsh")))
        .collect()
}

fn prompt_of(text: String) -> crate::core::ai::chat::approval::ApprovalPrompt {
    crate::core::ai::chat::approval::ApprovalPrompt {
        target: "send what your panes run to the engine".into(),
        command: text,
        keys: crate::core::ai::chat::approval::PANES_KEYS,
        armed: true,
        why: None,
        notice: None,
    }
}

// The ask stays answerable: eight panes and a huge focused line still fit
// the prompt's row cap, at a wide pane and at a narrow one.
#[test]
fn a_big_roster_still_fits_the_approval_prompt() {
    use crate::core::ai::chat::approval_rows::fits;
    for count in [1, 3, 5, 8, 40] {
        let text = render(&panes_lines(count));
        let prompt = prompt_of(text.clone());
        for width in [80, PROMPT_COLS] {
            assert!(fits(&prompt, width, 20), "{count} panes at {width}: {text}");
        }
        assert!(text.starts_with("pane 1 (focused)"), "{text}");
    }
    // A huge focused line is shortened and crowds the rest out, said plainly.
    assert!(render(&panes_lines(8)).ends_with("+7 more"));
    // Short lines: five panes are named and the rest counted.
    let short: Vec<String> = (1..=8)
        .map(|n| format!("pane {n} cwd: ~ program: zsh"))
        .collect();
    let text = render(&short);
    assert!(text.lines().count() <= MAX_PANES + 1, "{text}");
    assert!(
        text.ends_with("+3 more") || text.ends_with("+4 more"),
        "{text}"
    );
    assert!(fits(&prompt_of(text.clone()), PROMPT_COLS, 20), "{text}");
}

#[test]
fn a_roster_that_fits_is_sent_whole() {
    let lines = vec![
        "pane 1 (focused) cwd: ~ program: zsh".to_owned(),
        "pane 2 cwd: ~ program: zsh".to_owned(),
    ];
    assert_eq!(render(&lines), lines.join("\n"));
}

#[test]
fn a_roster_knows_how_old_it_is() {
    let roster = Roster::default();
    assert_eq!(roster.age(), None);
    roster.set(vec!["pane 1".to_owned()]);
    assert!(roster.age().unwrap() < Duration::from_secs(5));
    roster.backdate(Duration::from_secs(42));
    assert!(roster.age().unwrap() >= Duration::from_secs(42));
}

#[test]
fn an_old_roster_says_so_in_the_text_that_is_sent() {
    let lines = vec!["pane 1 (focused) cwd: ~ program: zsh".to_owned()];
    let fresh = render_aged(&lines, Some(Duration::from_secs(9)));
    assert_eq!(fresh, lines[0]);
    let old = render_aged(&lines, Some(Duration::from_secs(42)));
    assert_eq!(
        old,
        format!(
            "{}\n(pane list is 42s old; a lookup may be stuck)",
            lines[0]
        )
    );
    // The note counts for the prompt's rows too.
    use crate::core::ai::chat::approval_rows::fits;
    for count in [1, 8, 40] {
        let text = render_aged(&panes_lines(count), Some(Duration::from_secs(99)));
        assert!(text.ends_with("a lookup may be stuck)"), "{text}");
        assert!(fits(&prompt_of(text.clone()), PROMPT_COLS, 20), "{text}");
    }
}
