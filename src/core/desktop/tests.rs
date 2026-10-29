use super::*;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// A fake PATH: resolves only the listed names, to `/usr/bin/<name>`.
fn path_with(names: &[&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
    let names: Vec<&'static str> = names.to_vec();
    move |cmd: &str| {
        if cmd.starts_with('/') {
            return names
                .iter()
                .any(|n| cmd == format!("/usr/bin/{n}"))
                .then(|| PathBuf::from(cmd));
        }
        names
            .contains(&cmd)
            .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
    }
}

fn pick(flag: Option<&str>, env: Option<&str>, names: &[&'static str]) -> Choice {
    choose_terminal(flag, env, &path_with(names)).unwrap().0
}

fn term_name(choice: &Choice) -> &str {
    match choice {
        Choice::Terminal { name, .. } => name,
        Choice::Fallback => "<fallback>",
    }
}

const BIN: &str = "/home/u/.cargo/bin/lazarobox-shell";

fn plan_for(name: &str) -> Plan {
    let choice = pick(None, None, &[Box::leak(name.to_string().into_boxed_str())]);
    plan_install(&choice, Path::new(BIN), None).unwrap()
}

#[test]
fn parses_the_three_forms() {
    assert_eq!(
        parse_args(&os(&[])),
        Ok(Options::Install {
            terminal: None,
            dir: None
        })
    );
    assert_eq!(parse_args(&os(&["--uninstall"])), Ok(Options::Uninstall));
    assert_eq!(
        parse_args(&os(&["--terminal", "foot"])),
        Ok(Options::Install {
            terminal: Some("foot".into()),
            dir: None
        })
    );
    assert_eq!(
        parse_args(&os(&["--terminal=foot"])),
        Ok(Options::Install {
            terminal: Some("foot".into()),
            dir: None
        })
    );
    assert_eq!(parse_args(&os(&["--help"])), Ok(Options::Help));
}

#[test]
fn rejects_bad_arguments() {
    assert!(parse_args(&os(&["--terminal"])).is_err());
    assert!(parse_args(&os(&["--terminal", "--uninstall"])).is_err());
    assert!(parse_args(&os(&["--terminal="])).is_err());
    assert!(parse_args(&os(&["--bogus"])).is_err());
    assert!(parse_args(&os(&["--uninstall", "--terminal", "foot"])).is_err());
}

#[test]
fn data_home_honors_only_an_absolute_xdg_value() {
    let home = Some(OsStr::new("/home/u"));
    assert_eq!(
        data_home(Some(OsStr::new("/data")), home),
        Ok(PathBuf::from("/data"))
    );
    assert_eq!(
        data_home(Some(OsStr::new("relative/dir")), home),
        Ok(PathBuf::from("/home/u/.local/share"))
    );
    assert_eq!(
        data_home(Some(OsStr::new("")), home),
        Ok(PathBuf::from("/home/u/.local/share"))
    );
    assert_eq!(
        data_home(None, home),
        Ok(PathBuf::from("/home/u/.local/share"))
    );
}

#[test]
fn data_home_needs_an_absolute_home() {
    assert!(data_home(None, None).is_err());
    assert!(data_home(None, Some(OsStr::new("rel"))).is_err());
}

#[test]
fn flag_wins_over_everything() {
    let all = ["kitty", "foot", "xdg-terminal-exec"];
    let choice = pick(Some("foot"), Some("kitty"), &all);
    assert_eq!(term_name(&choice), "foot");
}

#[test]
fn env_terminal_beats_xdg_terminal_exec_and_path() {
    let all = ["kitty", "alacritty", "xdg-terminal-exec"];
    let choice = pick(None, Some("alacritty"), &all);
    assert_eq!(term_name(&choice), "alacritty");
}

#[test]
fn xdg_terminal_exec_beats_known_terminals() {
    let choice = pick(None, None, &["kitty", "xdg-terminal-exec"]);
    assert_eq!(term_name(&choice), "xdg-terminal-exec");
}

#[test]
fn known_terminals_follow_the_documented_order() {
    let mut remaining: Vec<&'static str> = KNOWN_TERMINALS.to_vec();
    while !remaining.is_empty() {
        let choice = pick(None, None, &remaining);
        assert_eq!(term_name(&choice), remaining[0]);
        remaining.remove(0);
    }
    assert_eq!(pick(None, None, &[]), Choice::Fallback);
}

#[test]
fn unresolvable_env_terminal_is_skipped_with_a_note() {
    let (choice, notes) = choose_terminal(None, Some("nope"), &path_with(&["foot"])).unwrap();
    assert_eq!(term_name(&choice), "foot");
    assert!(notes.iter().any(|n| n.contains("$TERMINAL")));
}

#[test]
fn unresolvable_flag_is_an_error() {
    assert!(choose_terminal(Some("nope"), None, &path_with(&["foot"])).is_err());
}

#[test]
fn flag_accepts_an_absolute_path_and_keys_on_its_basename() {
    let choice = pick(Some("/usr/bin/alacritty"), None, &["alacritty"]);
    assert_eq!(
        choice,
        Choice::Terminal {
            name: "alacritty".into(),
            path: PathBuf::from("/usr/bin/alacritty"),
            known: true,
            source: Source::Flag,
        }
    );
}

#[test]
fn syntax_table_matches_each_terminals_documented_shape() {
    let expect = [
        (
            "kitty",
            "/usr/bin/kitty --class lazarobox --title Lazarobox",
        ),
        ("ghostty", "/usr/bin/ghostty -e"),
        ("wezterm", "/usr/bin/wezterm start --class lazarobox --"),
        ("alacritty", "/usr/bin/alacritty --class lazarobox -e"),
        ("foot", "/usr/bin/foot --app-id lazarobox"),
        ("gnome-terminal", "/usr/bin/gnome-terminal --"),
        ("konsole", "/usr/bin/konsole -e"),
        ("xfce4-terminal", "/usr/bin/xfce4-terminal -x"),
        ("x-terminal-emulator", "/usr/bin/x-terminal-emulator -e"),
        ("xdg-terminal-exec", "/usr/bin/xdg-terminal-exec"),
    ];
    for (name, prefix) in expect {
        assert_eq!(plan_for(name).exec, format!("{prefix} {BIN}"), "{name}");
    }
}

#[test]
fn startup_wm_class_only_when_the_terminal_sets_the_class() {
    for name in ["kitty", "wezterm", "alacritty", "foot"] {
        assert!(
            plan_for(name)
                .contents
                .contains("StartupWMClass=lazarobox\n"),
            "{name}"
        );
    }
    for name in [
        "ghostty",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "x-terminal-emulator",
        "xdg-terminal-exec",
    ] {
        let contents = plan_for(name).contents;
        assert!(!contents.contains("StartupWMClass"), "{name}");
        assert!(!contents.contains('@'), "{name}");
    }
}

#[test]
fn unknown_terminal_uses_dash_e_and_says_so() {
    let choice = pick(Some("mytty"), None, &["mytty"]);
    let plan = plan_install(&choice, Path::new(BIN), None).unwrap();
    assert_eq!(plan.exec, format!("/usr/bin/mytty -e {BIN}"));
    assert!(plan.notes.iter().any(|n| n.contains("no launch syntax")));
    assert!(!plan.contents.contains("StartupWMClass"));
}

#[test]
fn fallback_runs_the_binary_in_a_desktop_chosen_terminal() {
    let plan = plan_install(&Choice::Fallback, Path::new(BIN), None).unwrap();
    assert_eq!(plan.exec, BIN);
    assert!(plan.contents.contains("Terminal=true\n"));
    assert!(plan.contents.contains(&format!("Exec={BIN}\n")));
    assert!(!plan.contents.contains("StartupWMClass"));
}

#[test]
fn kitty_gets_no_compatibility_notice_and_others_do() {
    assert!(plan_for("kitty").notes.is_empty());
    assert!(
        plan_for("foot")
            .notes
            .contains(&NON_KITTY_NOTICE.to_string())
    );
    let fallback = plan_install(&Choice::Fallback, Path::new(BIN), None).unwrap();
    assert!(fallback.notes.contains(&NON_KITTY_NOTICE.to_string()));
}

#[test]
fn rendered_file_is_a_complete_desktop_entry() {
    let contents = plan_for("kitty").contents;
    assert!(contents.starts_with("[Desktop Entry]\n"));
    assert!(contents.contains("Terminal=false\n"));
    assert!(contents.contains(&format!(
        "Exec=/usr/bin/kitty --class lazarobox --title Lazarobox {BIN}\n"
    )));
    assert!(contents.ends_with("StartupWMClass=lazarobox\n"));
}

#[test]
fn reserved_characters_and_relative_paths_are_refused() {
    for bad in [
        "relative/bin",
        "/opt/my app/bin",
        "/opt/a\"b",
        "/opt/a'b",
        "/opt/a\\b",
        "/opt/a$b",
        "/opt/a%b",
        "/opt/a;b",
        "/opt/a&b",
        "/opt/a|b",
        "/opt/a`b",
        "/opt/a(b)",
        "/opt/a~b",
        "/opt/a\nb",
    ] {
        assert!(validate_path(Path::new(bad)).is_err(), "{bad}");
        assert!(
            plan_install(&Choice::Fallback, Path::new(bad), None).is_err(),
            "{bad}"
        );
    }
    assert!(validate_path(Path::new("/home/u/.cargo/bin/lazarobox-shell")).is_ok());
}

#[test]
fn unsafe_terminal_path_is_refused_too() {
    let choice = Choice::Terminal {
        name: "kitty".into(),
        path: PathBuf::from("/opt/my kitty/kitty"),
        known: true,
        source: Source::Flag,
    };
    assert!(plan_install(&choice, Path::new(BIN), None).is_err());
}

#[test]
fn build_artefact_paths_are_detected_by_component() {
    for yes in [
        "/src/lazarobox-shell/target/debug/lazarobox-shell",
        "/src/x/target/release/lazarobox-shell",
    ] {
        assert!(is_build_artefact(Path::new(yes)), "{yes}");
    }
    for no in [
        "/home/u/.cargo/bin/lazarobox-shell",
        "/opt/target/bin/lazarobox-shell",
        "/opt/mytarget/debug/lazarobox-shell",
        "/opt/target-debug/lazarobox-shell",
    ] {
        assert!(!is_build_artefact(Path::new(no)), "{no}");
    }
}

#[test]
fn a_build_artefact_binary_gets_a_cargo_clean_note() {
    let bin = Path::new("/src/lazarobox-shell/target/debug/lazarobox-shell");
    let plan = plan_install(&Choice::Fallback, bin, None).unwrap();
    assert!(plan.notes.iter().any(|n| n.contains("cargo clean")));
    assert!(
        !plan_for("kitty")
            .notes
            .iter()
            .any(|n| n.contains("cargo clean"))
    );
}

#[test]
fn placeholders_in_a_path_are_not_rewritten() {
    let choice = pick(None, None, &["kitty"]);
    let bin = Path::new("/opt/@TERMINAL@/@EXEC@/lazarobox-shell");
    let plan = plan_install(&choice, bin, None).unwrap();
    assert!(plan.contents.contains(&format!(
        "Exec=/usr/bin/kitty --class lazarobox --title Lazarobox {}\n",
        bin.display()
    )));
    assert!(plan.contents.contains("Terminal=false\n"));
}

fn with_dir(dir: &str) -> Plan {
    let choice = pick(None, None, &["kitty"]);
    plan_install(&choice, Path::new(BIN), Some(Path::new(dir))).unwrap()
}

// Spec (start-dir): install-desktop --dir.
#[test]
fn parses_the_dir_flag_in_both_forms() {
    let want = Ok(Options::Install {
        terminal: None,
        dir: Some("/w/proj".into()),
    });
    assert_eq!(parse_args(&os(&["--dir", "/w/proj"])), want);
    assert_eq!(parse_args(&os(&["--dir=/w/proj"])), want);
    assert!(parse_args(&os(&["--dir"])).is_err());
    assert!(parse_args(&os(&["--dir="])).is_err());
    assert!(parse_args(&os(&["--uninstall", "--dir", "/w"])).is_err());
}

#[test]
fn a_dir_writes_path_and_the_exec_argument() {
    let plan = with_dir("/w/proj");
    assert!(
        plan.contents.contains("\nPath=/w/proj\n"),
        "{}",
        plan.contents
    );
    let exec =
        format!("Exec=/usr/bin/kitty --class lazarobox --title Lazarobox {BIN} --dir=/w/proj\n");
    assert!(plan.contents.contains(&exec), "{}", plan.contents);
    let without = plan_for("kitty");
    let terminal = |c: &str| {
        c.lines()
            .find(|l| l.starts_with("Terminal="))
            .map(String::from)
    };
    assert_eq!(terminal(&plan.contents), terminal(&without.contents));
}

#[test]
fn without_a_dir_neither_path_nor_the_argument_is_written() {
    let plan = plan_for("kitty");
    assert!(!plan.contents.contains("Path="));
    assert!(!plan.contents.contains("--dir"));
    assert!(!plan.contents.contains('@'));
}

#[test]
fn the_fallback_launcher_also_carries_the_dir() {
    let plan = plan_install(
        &Choice::Fallback,
        Path::new(BIN),
        Some(Path::new("/w/proj")),
    )
    .unwrap();
    assert!(
        plan.contents
            .contains(&format!("Exec={BIN} --dir=/w/proj\n"))
    );
    assert!(plan.contents.contains("Path=/w/proj\n"));
}

#[test]
fn a_dir_with_spaces_keeps_the_path_verbatim_and_quotes_the_argument() {
    let plan = with_dir("/w/my proj");
    assert!(plan.contents.contains("\nPath=/w/my proj\n"));
    assert!(
        plan.contents
            .contains(&format!("{BIN} \"--dir=/w/my proj\"\n"))
    );
}

#[test]
fn backslashes_and_a_leading_space_are_escaped_and_controls_refused() {
    let plan = with_dir("/w/a\\b");
    assert!(
        plan.contents.contains("\nPath=/w/a\\\\b\n"),
        "{}",
        plan.contents
    );
    let choice = pick(None, None, &["kitty"]);
    let relative_space = plan_install(&choice, Path::new(BIN), Some(Path::new(" /w")));
    assert!(relative_space.is_err(), "a relative path is refused");
    for bad in ["/w/a\nb", "/w/a\tb", "/w/a\u{7f}b"] {
        let result = plan_install(&choice, Path::new(BIN), Some(Path::new(bad)));
        assert!(result.is_err(), "{bad:?}");
    }
}

#[test]
fn a_dir_in_the_exec_line_escapes_percent_and_dollar() {
    let plan = with_dir("/w/50%$x");
    assert!(
        plan.contents.contains("\"--dir=/w/50%%\\\\$x\""),
        "{}",
        plan.contents
    );
}

#[test]
fn a_dir_with_edge_whitespace_is_refused() {
    let choice = pick(None, None, &["kitty"]);
    for bad in ["/w/proj ", "/w/proj\u{a0}", "/w/proj\u{3000}"] {
        let result = plan_install(&choice, Path::new(BIN), Some(Path::new(bad)));
        assert!(result.is_err(), "{bad:?}");
    }
    assert!(plan_install(&choice, Path::new(BIN), Some(Path::new("/w/a b"))).is_ok());
}

#[test]
fn a_non_utf8_dir_value_has_its_own_message() {
    use std::os::unix::ffi::OsStringExt;
    let bad = OsString::from_vec(vec![b'/', 0xff]);
    let args = vec![OsString::from("--dir"), bad];
    assert_eq!(
        parse_args(&args),
        Err("--dir: folder name is not valid UTF-8".to_string())
    );
}

#[test]
fn dir_before_uninstall_is_refused_too() {
    assert!(parse_args(&os(&["--dir", "/w", "--uninstall"])).is_err());
}
