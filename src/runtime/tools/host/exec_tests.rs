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
    let dir = tempfile::tempdir().unwrap();
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
        ("subcommand not allowed".into(), false)
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
        }]
    );
    let (_, ask, status, _) = super::tests::pair(&super::tests::events(&mut rx));
    assert!(ask.is_some());
    assert_eq!(status, ToolStatus::Denied);
    assert!(!w.host.exec.gate.is_busy(key()));
}
