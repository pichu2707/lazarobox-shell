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
            lookup: &lookup,
            refresh_db: &refresh,
        };
        let mut out = Vec::new();
        let result = execute(options, &ctx, &mut out);
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
    assert!(!out.contains("note:"), "kitty needs no notice: {out}");
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
        lookup: &lookup,
        refresh_db: &refresh,
    };
    assert!(execute(&install(None), &ctx, &mut Vec::new()).is_err());
    assert!(!fx.target().exists());
}

#[test]
fn reports_notes_for_non_kitty_unknown_and_fallback() {
    let fx = Fixture::new();
    let (_, out) = fx.run(&install(Some("mytty")), None, None, &["mytty"]);
    assert!(out.contains("no launch syntax known for mytty"), "{out}");
    assert!(out.contains("tested in Kitty"), "{out}");

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
