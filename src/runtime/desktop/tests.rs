//! Every test works inside a temp directory: `HOME` and `XDG_DATA_HOME` are
//! injected, `PATH` is a fake lookup, and no terminal is ever executed.

use std::cell::RefCell;

use super::*;

const EXE: &str = "/opt/lazarobox/bin/lazarobox-shell";

struct Fixture {
    home: tempfile::TempDir,
    refreshed: RefCell<Vec<PathBuf>>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            refreshed: RefCell::new(Vec::new()),
        }
    }

    fn run(
        &self,
        options: &Options,
        xdg: Option<&str>,
        env_terminal: Option<&str>,
        names: &[&str],
    ) -> (Result<(), String>, String) {
        let lookup = |cmd: &str| {
            names
                .contains(&cmd)
                .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
        };
        let refresh = |dir: &Path| self.refreshed.borrow_mut().push(dir.to_path_buf());
        let ctx = Context {
            env_terminal: env_terminal.map(String::from),
            xdg_data_home: xdg.map(OsString::from),
            home: Some(self.home.path().as_os_str().to_owned()),
            exe: PathBuf::from(EXE),
            cwd: self.home.path().to_path_buf(),
            wsl: false,
            lookup: &lookup,
            refresh_db: &refresh,
        };
        let mut out = Vec::new();
        let result = execute(options, &ctx, None, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    /// A run with a PATH of `names`, an optional typed answer (`None` is no
    /// TTY) and the WSL flag.
    fn run_with(
        &self,
        options: &Options,
        names: &[&str],
        typed: Option<&str>,
        wsl: bool,
    ) -> (Result<(), String>, String) {
        let lookup = |cmd: &str| {
            names
                .contains(&cmd)
                .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
        };
        let refresh = |_: &Path| {};
        let ctx = Context {
            env_terminal: None,
            xdg_data_home: None,
            home: Some(self.home.path().as_os_str().to_owned()),
            exe: PathBuf::from(EXE),
            cwd: self.home.path().to_path_buf(),
            wsl,
            lookup: &lookup,
            refresh_db: &refresh,
        };
        let mut reader = typed.map(|t| std::io::Cursor::new(t.as_bytes().to_vec()));
        let input = reader.as_mut().map(|r| r as &mut dyn BufRead);
        let mut out = Vec::new();
        let result = execute(options, &ctx, input, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    fn target(&self) -> PathBuf {
        self.home
            .path()
            .join(".local/share/applications/lazarobox.desktop")
    }
}

fn install(terminal: Option<&str>) -> Options {
    Options::Install {
        terminal: terminal.map(String::from),
        dir: None,
    }
}

#[test]
fn installs_into_the_injected_home_with_absolute_paths() {
    let fx = Fixture::new();
    let (result, out) = fx.run(&install(None), None, None, &["kitty", "foot"]);
    result.unwrap();
    let written = fs::read_to_string(fx.target()).unwrap();
    assert!(written.contains(&format!(
        "Exec=/usr/bin/kitty --class lazarobox --title Lazarobox {EXE}\n"
    )));
    assert!(out.contains("Installed "));
    assert!(out.contains(&format!(
        "Exec: /usr/bin/kitty --class lazarobox --title Lazarobox {EXE}"
    )));
    assert!(out.contains("kitty has a record"), "{out}");
    assert!(!out.contains("no test record"), "{out}");
    assert_eq!(
        fs::metadata(fx.target()).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn leaves_no_temp_file_behind() {
    let fx = Fixture::new();
    fx.run(&install(None), None, None, &["foot"]).0.unwrap();
    let names: Vec<_> = fs::read_dir(fx.target().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![OsString::from("lazarobox.desktop")]);
}

#[test]
fn honors_an_absolute_xdg_data_home_and_ignores_a_relative_one() {
    let fx = Fixture::new();
    let xdg = tempfile::tempdir().unwrap();
    let xdg_str = xdg.path().to_str().unwrap();
    fx.run(&install(None), Some(xdg_str), None, &["kitty"])
        .0
        .unwrap();
    assert!(xdg.path().join("applications/lazarobox.desktop").is_file());
    assert!(!fx.target().exists());

    fx.run(&install(None), Some("relative/share"), None, &["kitty"])
        .0
        .unwrap();
    assert!(fx.target().is_file());
}

#[test]
fn passes_the_applications_dir_to_the_database_refresh() {
    let fx = Fixture::new();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    fx.run(&Options::Uninstall, None, None, &[]).0.unwrap();
    let expected = fx.target().parent().unwrap().to_path_buf();
    assert_eq!(*fx.refreshed.borrow(), vec![expected.clone(), expected]);
}

#[test]
fn uninstall_removes_the_file_and_is_idempotent() {
    let fx = Fixture::new();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    assert!(fx.target().exists());
    let (result, out) = fx.run(&Options::Uninstall, None, None, &[]);
    result.unwrap();
    assert!(out.starts_with("Removed "));
    assert!(!fx.target().exists());
    fx.run(&Options::Uninstall, None, None, &[]).0.unwrap();
}

#[test]
fn uninstall_without_a_file_says_nothing_to_remove() {
    let fx = Fixture::new();
    let (result, out) = fx.run(&Options::Uninstall, None, None, &[]);
    result.unwrap();
    assert!(out.starts_with("Nothing to remove"), "{out}");
    assert!(!out.contains("Removed"), "{out}");
}

#[test]
fn a_stale_temp_file_from_a_crashed_run_does_not_block_install() {
    let fx = Fixture::new();
    let apps = fx.target().parent().unwrap().to_path_buf();
    fs::create_dir_all(&apps).unwrap();
    let stale = apps.join(format!(".lazarobox.desktop.{}.tmp", std::process::id()));
    fs::write(&stale, "stale").unwrap();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    assert!(fx.target().is_file());
    assert!(!stale.exists());
}

#[test]
fn a_build_artefact_exe_is_reported_with_a_note() {
    let fx = Fixture::new();
    let lookup = |_: &str| None;
    let refresh = |_: &Path| {};
    let ctx = Context {
        env_terminal: None,
        xdg_data_home: None,
        home: Some(fx.home.path().as_os_str().to_owned()),
        exe: PathBuf::from("/src/lazarobox-shell/target/debug/lazarobox-shell"),
        cwd: fx.home.path().to_path_buf(),
        wsl: false,
        lookup: &lookup,
        refresh_db: &refresh,
    };
    let mut out = Vec::new();
    execute(&install(None), &ctx, None, &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("cargo clean"));
}

#[test]
fn reinstall_replaces_a_symlink_instead_of_writing_through_it() {
    let fx = Fixture::new();
    let apps = fx.target().parent().unwrap().to_path_buf();
    fs::create_dir_all(&apps).unwrap();
    let elsewhere = fx.home.path().join("elsewhere.desktop");
    fs::write(&elsewhere, "untouched").unwrap();
    std::os::unix::fs::symlink(&elsewhere, fx.target()).unwrap();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "untouched");
    assert!(
        !fs::symlink_metadata(fx.target())
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn unsafe_binary_path_writes_nothing() {
    let fx = Fixture::new();
    let lookup = |_: &str| None;
    let refresh = |_: &Path| {};
    let ctx = Context {
        env_terminal: None,
        xdg_data_home: None,
        home: Some(fx.home.path().as_os_str().to_owned()),
        exe: PathBuf::from("/opt/my app/lazarobox-shell"),
        cwd: fx.home.path().to_path_buf(),
        wsl: false,
        lookup: &lookup,
        refresh_db: &refresh,
    };
    assert!(execute(&install(None), &ctx, None, &mut Vec::new()).is_err());
    assert!(!fx.target().exists());
}

#[test]
fn reports_notes_for_non_kitty_unknown_and_fallback() {
    let fx = Fixture::new();
    let (_, out) = fx.run(&install(Some("mytty")), None, None, &["mytty"]);
    assert!(out.contains("no launch syntax known for mytty"), "{out}");
    assert!(out.contains("mytty has no test record yet"), "{out}");
    assert!(!out.contains("tested in Kitty"), "{out}");

    let (_, out) = fx.run(&install(None), None, Some("nope"), &[]);
    assert!(out.contains("ignoring $TERMINAL=nope"), "{out}");
    assert!(out.contains("Terminal=true"), "{out}");
    let written = fs::read_to_string(fx.target()).unwrap();
    assert!(written.contains("Terminal=true\n"));
}

#[test]
fn missing_terminal_flag_target_is_an_error_and_writes_nothing() {
    let fx = Fixture::new();
    let (result, _) = fx.run(&install(Some("nope")), None, None, &["kitty"]);
    assert!(result.is_err());
    assert!(!fx.target().exists());
}

#[test]
fn which_searches_only_absolute_path_entries_for_executables() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("fake-term");
    fs::write(&exe, "").unwrap();
    assert_eq!(which("fake-term", Some(dir.path().as_os_str())), None);
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        which("fake-term", Some(dir.path().as_os_str())),
        Some(exe.clone())
    );
    assert_eq!(which(exe.to_str().unwrap(), None), Some(exe));
    assert_eq!(which("fake-term", Some(OsStr::new("relative"))), None);
    assert_eq!(which("./fake-term", Some(dir.path().as_os_str())), None);
    assert_eq!(which("fake-term", None), None);
}

fn install_in(dir: &str) -> Options {
    Options::Install {
        terminal: None,
        dir: Some(dir.into()),
    }
}

#[test]
fn install_dir_writes_path_and_exec_for_an_existing_folder() {
    let fx = Fixture::new();
    let proj = fx.home.path().join("my proj");
    fs::create_dir(&proj).unwrap();
    let (result, _) = fx.run(&install_in(proj.to_str().unwrap()), None, None, &["kitty"]);
    result.unwrap();
    let written = fs::read_to_string(fx.target()).unwrap();
    assert!(
        written.contains(&format!("\nPath={}\n", proj.display())),
        "{written}"
    );
    assert!(
        written.contains(&format!("\"--dir={}\"", proj.display())),
        "{written}"
    );
}

#[test]
fn install_dir_resolves_relative_and_tilde_at_install_time() {
    let fx = Fixture::new();
    fs::create_dir(fx.home.path().join("code")).unwrap();
    for raw in ["code", "~/code", "./code"] {
        fs::remove_file(fx.target()).ok();
        fx.run(&install_in(raw), None, None, &["kitty"]).0.unwrap();
        let written = fs::read_to_string(fx.target()).unwrap();
        let want = format!("\nPath={}\n", fx.home.path().join("code").display());
        assert!(written.contains(&want), "{raw}: {written}");
    }
}

#[test]
fn install_dir_that_is_not_a_folder_fails_and_writes_nothing() {
    let fx = Fixture::new();
    let file = fx.home.path().join("f");
    fs::write(&file, "x").unwrap();
    for bad in ["/nope-lazarobox", file.to_str().unwrap()] {
        let (result, out) = fx.run(&install_in(bad), None, None, &["kitty"]);
        let why = result.unwrap_err();
        assert!(why.contains("--dir"), "{why}");
        assert!(!why.contains('\n') && out.is_empty());
        assert!(!fx.target().exists());
    }
}

#[test]
fn reinstalling_without_dir_removes_a_previous_one() {
    let fx = Fixture::new();
    let proj = fx.home.path().join("p");
    fs::create_dir(&proj).unwrap();
    fx.run(&install_in(proj.to_str().unwrap()), None, None, &["kitty"])
        .0
        .unwrap();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    let written = fs::read_to_string(fx.target()).unwrap();
    assert!(
        !written.contains("Path=") && !written.contains("--dir"),
        "{written}"
    );
}

#[test]
fn install_dir_with_shell_special_characters_stays_valid() {
    let fx = Fixture::new();
    for name in ["a$b", "a`b", "a\"b", "50%x"] {
        let proj = fx.home.path().join(name);
        fs::create_dir(&proj).unwrap();
        let options = install_in(proj.to_str().unwrap());
        fx.run(&options, None, None, &["kitty"]).0.unwrap();
        let written = fs::read_to_string(fx.target()).unwrap();
        assert!(written.contains("\nPath="), "{name}: {written}");
        let exec = written.lines().find(|l| l.starts_with("Exec=")).unwrap();
        assert!(exec.contains("\"--dir="), "{name}: {exec}");
        match name {
            "a$b" => assert!(exec.contains("a\\\\$b"), "{exec}"),
            "a`b" => assert!(exec.contains("a\\\\`b"), "{exec}"),
            "a\"b" => assert!(exec.contains("a\\\\\"b"), "{exec}"),
            _ => assert!(exec.contains("50%%x"), "{exec}"),
        }
    }
}

#[test]
fn install_dir_with_uninstall_is_a_parse_error_and_touches_nothing() {
    let fx = Fixture::new();
    fx.run(&install(None), None, None, &["kitty"]).0.unwrap();
    let before = fs::read_to_string(fx.target()).unwrap();
    let args: Vec<OsString> = ["--dir", "/w", "--uninstall"].map(OsString::from).to_vec();
    assert!(parse_args(&args).is_err());
    assert_eq!(fs::read_to_string(fx.target()).unwrap(), before);
}

fn written(fx: &Fixture) -> String {
    fs::read_to_string(fx.target()).unwrap()
}

#[test]
fn enter_writes_the_automatic_choice_and_the_list_marks_tested_terminals() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(&install(None), &["foot", "kitty"], Some("\n"), false);
    result.unwrap();
    assert!(written(&fx).contains("Exec=/usr/bin/kitty "), "{out}");
    assert!(out.contains("1) kitty (tested) [default]"), "{out}");
    assert!(out.contains("2) foot\n"), "{out}");
}

#[test]
fn a_number_picks_that_terminal() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(&install(None), &["kitty", "foot"], Some("2\n"), false);
    result.unwrap();
    let file = written(&fx);
    assert!(file.contains("Exec=/usr/bin/foot "), "{file}");
    assert!(out.contains("foot has no test record yet"), "{out}");
}

#[test]
fn invalid_input_shows_a_notice_and_asks_again() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(
        &install(None),
        &["kitty", "foot"],
        Some("abc\n9\n2\n"),
        false,
    );
    result.unwrap();
    assert_eq!(out.matches("is not valid").count(), 2, "{out}");
    assert!(written(&fx).contains("Exec=/usr/bin/foot "));
}

#[test]
fn eof_cancels_without_writing_and_says_so() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(&install(None), &["kitty", "foot"], Some(""), false);
    let why = result.unwrap_err();
    assert!(why.contains("cancelled"), "{why}");
    assert!(!fx.target().exists());
    assert!(!out.contains("Installed"), "{out}");
    // EOF after an invalid answer cancels too.
    let (result, _) = fx.run_with(&install(None), &["kitty", "foot"], Some("x\n"), false);
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(!fx.target().exists());
}

#[test]
fn no_prompt_without_a_tty_or_with_an_explicit_terminal() {
    let fx = Fixture::new();
    let (_, plain) = fx.run_with(&install(None), &["kitty", "foot"], None, false);
    assert!(
        !plain.contains("1)") && !plain.contains("Choose"),
        "{plain}"
    );
    let (_, flagged) = fx.run_with(
        &install(Some("foot")),
        &["kitty", "foot"],
        Some("1\n"),
        false,
    );
    assert!(!flagged.contains("Choose"), "{flagged}");
    assert!(written(&fx).contains("Exec=/usr/bin/foot "));
    // Nothing installed: nothing to ask, today's fallback applies.
    let (result, out) = fx.run_with(&install(None), &[], Some("\n"), false);
    result.unwrap();
    assert!(
        !out.contains("Choose") && out.contains("Terminal=true"),
        "{out}"
    );
}

#[test]
fn finding_candidates_never_runs_a_terminal() {
    let fx = Fixture::new();
    let bin = fx.home.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let marker = fx.home.path().join("ran");
    for name in ["kitty", "foot", "zutty"] {
        let path = bin.join(name);
        fs::write(&path, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path_var = std::env::join_paths([&bin]).unwrap();
    let lookup = |cmd: &str| which(cmd, Some(&path_var));
    let refresh = |_: &Path| {};
    let ctx = Context {
        env_terminal: Some("zutty".into()),
        xdg_data_home: None,
        home: Some(fx.home.path().as_os_str().to_owned()),
        exe: PathBuf::from(EXE),
        cwd: fx.home.path().to_path_buf(),
        wsl: false,
        lookup: &lookup,
        refresh_db: &refresh,
    };
    let mut reader = std::io::Cursor::new(b"2\n".to_vec());
    let mut out = Vec::new();
    execute(&install(None), &ctx, Some(&mut reader), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("1) zutty") && out.contains("2) kitty"),
        "{out}"
    );
    assert!(!marker.exists(), "a candidate was executed");
}

#[test]
fn a_symlinked_alternative_is_offered_under_its_target_name() {
    let fx = Fixture::new();
    let bin = fx.home.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let real = bin.join("konsole");
    fs::write(&real, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    // `konsole` is not on PATH, only the alternative that points at it.
    let alt_dir = fx.home.path().join("alt");
    fs::create_dir_all(&alt_dir).unwrap();
    std::os::unix::fs::symlink(&real, alt_dir.join("x-terminal-emulator")).unwrap();
    let path_var = std::env::join_paths([&alt_dir]).unwrap();
    let lookup = |cmd: &str| which(cmd, Some(&path_var));
    let refresh = |_: &Path| {};
    let ctx = Context {
        env_terminal: None,
        xdg_data_home: None,
        home: Some(fx.home.path().as_os_str().to_owned()),
        exe: PathBuf::from(EXE),
        cwd: fx.home.path().to_path_buf(),
        wsl: false,
        lookup: &lookup,
        refresh_db: &refresh,
    };
    let mut reader = std::io::Cursor::new(b"\n".to_vec());
    let mut out = Vec::new();
    execute(&install(None), &ctx, Some(&mut reader), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("1) x-terminal-emulator [default]"), "{out}");
    assert!(out.contains("2) konsole"), "{out}");
}

#[test]
fn wsl_names_the_written_path_and_adds_the_notes() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(&install(None), &["kitty"], None, true);
    result.unwrap();
    assert!(
        out.contains(&format!("Installed {}", fx.target().display())),
        "{out}"
    );
    assert!(
        out.contains("may not appear in the Windows Start menu"),
        "{out}"
    );
    let (_, linux) = fx.run_with(&install(None), &["kitty"], None, false);
    assert!(!linux.contains("WSL"), "{linux}");
}

#[test]
fn installing_twice_leaves_one_entry_and_says_so() {
    let fx = Fixture::new();
    let (_, first) = fx.run_with(&install(None), &["kitty"], None, false);
    assert!(!first.contains("already"), "{first}");
    let (result, second) = fx.run_with(&install(None), &["kitty"], None, false);
    result.unwrap();
    assert!(
        second.contains(&format!(
            "Updated {} (already installed)",
            fx.target().display()
        )),
        "{second}"
    );
    assert_eq!(
        fs::read_dir(fx.target().parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn an_unwritable_target_fails_with_the_reason() {
    let fx = Fixture::new();
    // The applications folder cannot be created under a regular file.
    let blocker = fx.home.path().join("blocker");
    fs::write(&blocker, "x").unwrap();
    let (result, out) = fx.run(&install(None), blocker.to_str(), None, &["kitty"]);
    let why = result.unwrap_err();
    assert!(why.contains("cannot write"), "{why}");
    assert!(!out.contains("Installed"), "{out}");
}

#[test]
fn a_single_default_terminal_is_not_asked_about() {
    let fx = Fixture::new();
    let (result, out) = fx.run_with(&install(None), &["kitty"], Some("2\n"), false);
    result.unwrap();
    assert!(!out.contains("Choose") && !out.contains("1)"), "{out}");
    assert!(written(&fx).contains("Exec=/usr/bin/kitty "));
}

#[test]
fn the_list_explains_tested_with_the_records_link() {
    let fx = Fixture::new();
    let (_, out) = fx.run_with(&install(None), &["kitty", "foot"], Some("\n"), false);
    assert!(
        out.contains(&format!(
            "(tested) = has a record: {}",
            crate::core::desktop::RECORDS_URL
        )),
        "{out}"
    );
}

struct Broken;
impl std::io::Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("boom"))
    }
}

#[test]
fn a_failing_reader_is_reported_and_writes_nothing() {
    let fx = Fixture::new();
    let lookup = |cmd: &str| {
        ["kitty", "foot"]
            .contains(&cmd)
            .then(|| PathBuf::from(format!("/usr/bin/{cmd}")))
    };
    let refresh = |_: &Path| {};
    let ctx = Context {
        env_terminal: None,
        xdg_data_home: None,
        home: Some(fx.home.path().as_os_str().to_owned()),
        exe: PathBuf::from(EXE),
        cwd: fx.home.path().to_path_buf(),
        wsl: false,
        lookup: &lookup,
        refresh_db: &refresh,
    };
    let mut reader = std::io::BufReader::new(Broken);
    let why = execute(&install(None), &ctx, Some(&mut reader), &mut Vec::new()).unwrap_err();
    assert!(
        why.contains("cannot read the answer") && why.contains("boom"),
        "{why}"
    );
    assert!(!fx.target().exists());
}
