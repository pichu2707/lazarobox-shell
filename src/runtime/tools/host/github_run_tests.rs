//! The GitHub reads in the host, with a fake `gh` on a private PATH and a
//! real temporary repository for `origin`. Spec: github-tools (The
//! repository is always origin, Cloud consent, Fixed read, Visible failures,
//! No content in logs) and pane-execution (Block starts at the first read).

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{Mutex, atomic::AtomicU64},
    time::Duration,
};

use futures::FutureExt;
use tokio::sync::mpsc;

use super::{exec_tests::Asks, github::gh_env, *};
use crate::core::ai::{
    chat::approval::ApprovalAsk,
    tools::{
        github::{GhCall, State},
        registry::Family,
    },
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/github");

/// The main reply of the fake `gh` for every call but the fork probe.
const LIST: &str = "cat \"$FIX/issue-list.json\"";

struct Rig {
    _tmp: tempfile::TempDir,
    host: Arc<ToolHost>,
    dir: PathBuf,
    proj: PathBuf,
}

fn git(dir: &Path, home: &Path, args: &[&str]) {
    let out = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
}

/// A host whose `gh` is the script `main` (the probe answers `probe`, by
/// default the fixture of a plain repository), and a folder whose `origin`
/// is `origin`.
fn rig_with(origin: Option<&str>, main: &str, probe: &str, bound: Duration) -> Rig {
    rig_core(origin, main, probe, bound, true)
}

fn rig_core(
    origin: Option<&str>,
    main: &str,
    probe: &str,
    bound: Duration,
    gh_on_path: bool,
) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, home, proj) = (
        tmp.path().join("bin"),
        tmp.path().join("home"),
        tmp.path().join("proj"),
    );
    for d in [&dir, &home, &proj] {
        fs::create_dir_all(d).unwrap();
    }
    git(&proj, &home, &["init", "-q", "."]);
    if let Some(url) = origin {
        git(&proj, &home, &["remote", "add", "origin", url]);
    }
    let script = format!(
        "#!/bin/sh\nDIR='{dir}'\nFIX='{FIXTURES}'\n\
         echo \"$*\" >> \"$DIR/calls\"\npwd > \"$DIR/pwd\"\nenv > \"$DIR/env\"\n\
         case \"$1 $2\" in\n'repo view') {probe};;\n*) {main};;\nesac\n",
        dir = dir.display()
    );
    let program = dir.join("gh");
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let host = Arc::new(ToolHost {
        home: Some(home.into()),
        xdg_config_home: None,
        xdg_data_home: None,
        limits: Limits::default(),
        next: AtomicU64::new(1),
        stages: Arc::default(),
        exec: exec::Exec::system(),
        cli_cwds: std::sync::OnceLock::new(),
        roster: Default::default(),
        gh: super::github::Gh::with_bound(bound),
    });
    // Without `gh` on the login PATH git is still found in the system places.
    let login = if gh_on_path {
        format!("{}:/usr/bin:/bin", dir.display())
    } else {
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        empty.display().to_string()
    };
    host.set_gh_path(OsString::from(login));
    Rig {
        _tmp: tmp,
        host,
        dir,
        proj,
    }
}

fn rig(origin: Option<&str>, main: &str) -> Rig {
    rig_with(
        origin,
        main,
        "cat \"$FIX/repo-view.json\"",
        Duration::from_secs(30),
    )
}

const ORIGIN: &str = "https://github.com/o/r.git";

impl Rig {
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.dir.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn turn(
        &self,
        locality: Locality,
        approver: Arc<dyn Approver>,
    ) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(64);
        let mut scope = TurnScope::new(tx, locality, self.proj.clone());
        scope.approver = approver;
        scope.families = FamilySet::LEGACY.with(Family::Github);
        (self.host.dispatch(scope), rx)
    }
}

fn list() -> Call {
    Call::Github(GhCall::IssueList {
        state: State::Open,
        limit: 20,
    })
}

fn view(number: u32, comments: bool) -> Call {
    Call::Github(GhCall::IssueView { number, comments })
}

fn drain(rx: &mut mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// `ToolCall`, `GithubStarted`, `ToolResult` as short words, and the result.
fn shape(events: &[StreamEvent]) -> (Vec<&'static str>, Option<(ToolStatus, String)>) {
    let words = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCall { .. } => Some("call"),
            StreamEvent::GithubStarted => Some("started"),
            StreamEvent::ToolResult { .. } => Some("result"),
            _ => None,
        })
        .collect();
    let result = events.iter().find_map(|e| match e {
        StreamEvent::ToolResult {
            status, summary, ..
        } => Some((*status, summary.clone())),
        _ => None,
    });
    (words, result)
}

async fn one(rig: &Rig, call: Call) -> (Outcome, Vec<StreamEvent>) {
    let (dispatch, mut rx) = rig.turn(Locality::Local, Asks::new(Decision::Sent));
    let out = dispatch.call(call).await;
    (out, drain(&mut rx))
}

// --- the run: argv, folder, environment (Injection in arguments) ---

#[tokio::test]
async fn gh_runs_the_fixed_argv_from_the_root_with_a_clean_environment() {
    let rig = rig(Some(ORIGIN), LIST);
    let (out, events) = one(&rig, list()).await;
    assert!(!out.is_error, "{out:?}");
    // Spec (Issue list): the three issues, inside the untrusted-data wrapper.
    assert!(
        out.text.starts_with("<untrusted-tool-output id="),
        "{}",
        out.text
    );
    let issues = out.text.lines().filter(|l| l.starts_with('#')).count();
    assert_eq!(issues, 3, "{}", out.text);
    assert!(
        out.text.contains("GitHub issues of o/r (open), 3 shown:"),
        "{}",
        out.text
    );
    let slug = crate::core::ai::tools::github::slug_of(ORIGIN).unwrap();
    let call = GhCall::IssueList {
        state: State::Open,
        limit: 20,
    };
    let want = [
        crate::core::ai::tools::github::fork_argv(&slug),
        crate::core::ai::tools::github::argv(&call, &slug),
    ]
    .map(|argv| argv.join(" "));
    assert_eq!(rig.calls(), want);
    assert_eq!(fs::read_to_string(rig.dir.join("pwd")).unwrap().trim(), "/");
    let env = fs::read_to_string(rig.dir.join("env")).unwrap();
    for line in [
        "GH_PROMPT_DISABLED=1",
        "GH_NO_UPDATE_NOTIFIER=1",
        "NO_COLOR=1",
        &format!("PATH={}:/usr/bin:/bin", rig.dir.display()),
    ] {
        assert!(env.lines().any(|l| l == line), "{line} in {env}");
    }
    assert_eq!(shape(&events).0, ["call", "started", "result"]);
}

#[test]
fn the_environment_filter_drops_what_could_redirect_gh_and_keeps_the_login() {
    let os = |s: &str| OsString::from(s);
    let vars = [
        "GH_HOST",
        "GH_REPO",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "DEBUG",
        "GH_PAGER",
        "PAGER",
        "GH_FORCE_TTY",
        "CLICOLOR_FORCE",
        "GH_ACCESSIBLE_PROMPTER",
        "GH_COLOR_LABELS",
        "GH_ACCESSIBLE_COLORS",
        "GH_MDWIDTH",
        "GLAMOUR_STYLE",
        "GH_SPINNER_DISABLED",
        "PATH",
        "NO_COLOR",
    ]
    .map(|n| (os(n), os("x")))
    .into_iter()
    .chain([
        (os("GH_TOKEN"), os("keep")),
        (os("HOME"), os("/h")),
        (os("DBUS_SESSION_BUS_ADDRESS"), os("bus")),
    ]);
    let mut got = gh_env(vars, &os("/login/bin"));
    got.sort();
    let want: Vec<(OsString, OsString)> = [
        ("DBUS_SESSION_BUS_ADDRESS", "bus"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_TOKEN", "keep"),
        ("HOME", "/h"),
        ("NO_COLOR", "1"),
        ("PATH", "/login/bin"),
    ]
    .map(|(n, v)| (os(n), os(v)))
    .to_vec();
    assert_eq!(got, want);
}

// --- the block: GithubStarted only once gh is about to start ---

#[tokio::test]
async fn a_call_refused_before_the_spawn_sends_no_started_event() {
    let cases = [
        (None, "no origin"),
        (Some("https://gitlab.com/o/r.git"), "not on GitHub"),
        (Some("https://ghe.example.com/o/r.git"), "not on GitHub"),
    ];
    for (origin, want) in cases {
        let rig = rig(origin, LIST);
        let (out, events) = one(&rig, list()).await;
        let (words, result) = shape(&events);
        assert_eq!(words, ["call", "result"], "{origin:?}");
        assert_eq!(result.unwrap().0, ToolStatus::Error);
        assert!(
            out.is_error && out.text.contains(want),
            "{origin:?}: {}",
            out.text
        );
        assert!(rig.calls().is_empty(), "gh must not start: {origin:?}");
    }
}

#[tokio::test]
async fn a_folder_that_is_not_a_repository_says_so_and_starts_nothing() {
    let rig = rig(Some(ORIGIN), LIST);
    let bare = rig._tmp.path().join("plain");
    fs::create_dir(&bare).unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let mut scope = TurnScope::new(tx, Locality::Local, bare);
    scope.families = FamilySet::LEGACY.with(Family::Github);
    let out = rig.host.dispatch(scope).call(list()).await;
    assert!(
        out.is_error
            && out.text.contains(
                "the folder has no repository, or git refused it (ownership or a broken config)"
            ),
        "{}",
        out.text
    );
    assert_eq!(shape(&drain(&mut rx)).0, ["call", "result"]);
    assert!(rig.calls().is_empty());
}

#[tokio::test]
async fn a_missing_gh_is_an_error_that_starts_no_block() {
    let rig = rig_core(Some(ORIGIN), LIST, "true", Duration::from_secs(30), false);
    let (out, events) = one(&rig, list()).await;
    assert!(
        out.is_error && out.text.contains("gh is not installed"),
        "{}",
        out.text
    );
    assert!(out.text.contains("https://cli.github.com"));
    assert_eq!(shape(&events).0, ["call", "result"]);
}

// Spec (Visible failures): each case is an error; every one that started gh
// has sent the event first.
#[tokio::test]
async fn failures_after_the_spawn_are_errors_with_fixed_words_and_still_block() {
    let stderr = |name: &str| format!("cat \"$FIX/{name}\" >&2");
    let cases: Vec<(String, Call, &str)> = vec![
        (
            format!("{}; exit 4", stderr("logged-out.stderr")),
            list(),
            "gh auth login",
        ),
        (
            format!("{}; exit 1", stderr("not-found-issue.stderr")),
            view(99_999_999, false),
            "#99999999 was not found in o/r",
        ),
        (
            format!("{}; exit 1", stderr("not-found-pr.stderr")),
            Call::Github(GhCall::PrView { number: 7 }),
            "#7 was not found in o/r",
        ),
        (
            "echo '[]' | head -c 3; echo not-json".into(),
            list(),
            "unreadable",
        ),
        (
            "echo 'SECRET-TOKEN-123' >&2; echo SECRET-TOKEN-123; exit 2".into(),
            list(),
            "gh failed",
        ),
        (
            "head -c 5000000 /dev/zero | tr '\\0' x".into(),
            view(3, true),
            "try without comments",
        ),
        (
            "head -c 5000000 /dev/zero | tr '\\0' x".into(),
            view(3, false),
            "too large",
        ),
    ];
    for (script, call, want) in cases {
        let rig = rig(Some(ORIGIN), &script);
        let (out, events) = one(&rig, call).await;
        let (words, result) = shape(&events);
        assert_eq!(words, ["call", "started", "result"], "{want}");
        let (status, row) = result.unwrap();
        assert_eq!(status, ToolStatus::Error, "{want}");
        assert!(
            out.is_error && out.text.contains(want),
            "{want}: {}",
            out.text
        );
        assert_eq!(row, out.text, "{want}");
        for text in [&out.text, &row] {
            assert!(!text.contains("SECRET-TOKEN-123"), "{text}");
        }
    }
}

// Spec (UI stays responsive, gh timeout): a hang is killed at the bound with
// a message and no partial text.
#[tokio::test]
async fn a_gh_that_never_answers_ends_at_its_bound() {
    let rig = rig_with(
        Some(ORIGIN),
        "echo partial; sleep 300",
        "cat \"$FIX/repo-view.json\"",
        Duration::from_millis(400),
    );
    let started = std::time::Instant::now();
    let (out, events) = one(&rig, list()).await;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(
        out.is_error && out.text.contains("did not answer in time"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("partial"));
    assert_eq!(shape(&events).0, ["call", "started", "result"]);
}

// --- the repository: origin at the moment of the call ---

#[tokio::test]
async fn both_origin_forms_read_the_same_repository_and_a_folder_change_moves_it() {
    let (a, b) = (
        rig(Some("git@github.com:o/a.git"), LIST),
        rig(Some("https://github.com/o/b"), LIST),
    );
    one(&a, list()).await;
    one(&b, list()).await;
    assert!(
        a.calls()[1].contains("--repo github.com/o/a "),
        "{:?}",
        a.calls()
    );
    assert!(
        b.calls()[1].contains("--repo github.com/o/b "),
        "{:?}",
        b.calls()
    );
    // One host, two folders: the next call reads the new `origin`.
    let (tx, _rx) = mpsc::channel(64);
    let mut scope = TurnScope::new(tx, Locality::Local, b.proj.clone());
    scope.families = FamilySet::LEGACY.with(Family::Github);
    let moved = a.host.dispatch(scope).call(list()).await;
    assert!(!moved.is_error, "{moved:?}");
    assert!(
        a.calls().last().unwrap().contains("github.com/o/b "),
        "{:?}",
        a.calls()
    );
}

// --- the fork note ---

#[tokio::test]
async fn a_fork_names_its_parent_once_and_the_parent_is_never_queried() {
    let rig = rig_with(
        Some(ORIGIN),
        LIST,
        "cat \"$FIX/repo-view-fork.json\"",
        Duration::from_secs(30),
    );
    let (first, _) = one(&rig, list()).await;
    let (second, _) = one(&rig, list()).await;
    for out in [&first, &second] {
        assert!(
            out.text.contains(
                "o/r is a fork of Gentleman-Programming/gentle-ai; the parent is not read"
            ),
            "{}",
            out.text
        );
    }
    let probes = rig
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("repo view"))
        .count();
    assert_eq!(probes, 1, "{:?}", rig.calls());
    assert!(
        rig.calls().iter().all(|c| !c.contains("gentle-ai")),
        "{:?}",
        rig.calls()
    );
}

#[tokio::test]
async fn a_plain_repository_has_no_fork_note_and_a_failed_probe_says_unknown() {
    let plain = rig(Some(ORIGIN), LIST);
    let (out, _) = one(&plain, list()).await;
    assert!(!out.text.contains("fork"), "{}", out.text);
    let failing = rig_with(
        Some(ORIGIN),
        LIST,
        "echo boom >&2; exit 1",
        Duration::from_secs(30),
    );
    let (out, _) = one(&failing, list()).await;
    assert!(!out.is_error, "the read goes on: {}", out.text);
    assert!(
        out.text.contains("note: fork status unknown"),
        "{}",
        out.text
    );
    // A failure is not cached: the next call probes again.
    one(&failing, list()).await;
    let probes = failing
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("repo view"))
        .count();
    assert_eq!(probes, 2);
}

// A probe that timed out or found `gh` logged out is not asked again in the
// run: each later call would pay it again, up to the bound.
#[tokio::test]
async fn a_logged_out_or_timed_out_probe_is_not_repeated_and_still_says_unknown() {
    for (probe, bound) in [
        (
            "echo 'run gh auth login' >&2; exit 4",
            Duration::from_secs(30),
        ),
        ("sleep 5", Duration::from_millis(300)),
    ] {
        let rig = rig_with(Some(ORIGIN), LIST, probe, bound);
        for _ in 0..2 {
            let (out, _) = one(&rig, list()).await;
            assert!(
                out.text.contains("note: fork status unknown"),
                "{probe}: {}",
                out.text
            );
        }
        let probes = (rig.calls().into_iter())
            .filter(|c| c.starts_with("repo view"))
            .count();
        assert_eq!(probes, 1, "{probe}");
    }
}

#[tokio::test]
async fn a_hostile_parent_name_is_not_shown() {
    let probe = "echo '{\"isFork\":true,\"parent\":{\"name\":\"x\\u001b[2Jevil y\",\"owner\":{\"login\":\"o\"}}}'";
    let rig = rig_with(Some(ORIGIN), LIST, probe, Duration::from_secs(30));
    let (out, _) = one(&rig, list()).await;
    assert!(!out.text.contains("evil"), "{}", out.text);
    assert!(out.text.contains("fork"), "{}", out.text);
}

// --- consent ---

/// Records, at the moment of each ask, whether `gh` had started.
struct Watch {
    calls: PathBuf,
    answer: Decision,
    seen: Mutex<Vec<(ApprovalAsk, bool)>>,
}

impl Approver for Watch {
    fn ask(&self, _: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let started = self.calls.exists();
        self.seen.lock().unwrap().push((ask.clone(), started));
        let answer = self.answer;
        async move { answer.into() }.boxed()
    }
}

fn watch(rig: &Rig, answer: Decision) -> Arc<Watch> {
    Arc::new(Watch {
        calls: rig.dir.join("calls"),
        answer,
        seen: Mutex::default(),
    })
}

#[tokio::test]
async fn a_cloud_engine_asks_once_per_repository_and_turn_before_gh_starts() {
    let rig = rig(Some(ORIGIN), LIST);
    let approver = watch(&rig, Decision::Sent);
    let (dispatch, mut rx) = rig.turn(Locality::Cloud, approver.clone());
    for _ in 0..3 {
        assert!(!dispatch.call(list()).await.is_error);
    }
    let seen = approver.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    let (ask, started) = &seen[0];
    assert!(!started, "gh had already started");
    let ApprovalAsk::GithubRead { repo, what, .. } = ask else {
        panic!("{ask:?}")
    };
    assert_eq!(
        (repo.as_str(), what.as_str()),
        ("o/r", "gh_issue_list open (max 20)")
    );
    // The announced ToolCall carries the same ask, with the repository.
    let announced = drain(&mut rx)
        .into_iter()
        .find_map(|e| match e {
            StreamEvent::ToolCall { approval, .. } => approval,
            _ => None,
        })
        .unwrap();
    assert_eq!(*announced, *ask);
    // The next turn asks again.
    let (next, _rx) = rig.turn(Locality::Cloud, approver.clone());
    next.call(list()).await;
    assert_eq!(approver.seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_local_engine_never_asks() {
    let rig = rig(Some(ORIGIN), LIST);
    let asks = Asks::new(Decision::Sent);
    let (dispatch, _rx) = rig.turn(Locality::Local, asks.clone());
    assert!(!dispatch.call(list()).await.is_error);
    assert_eq!(asks.asked(), Vec::new());
}

#[tokio::test]
async fn a_declined_consent_is_denied_and_the_turn_is_not_asked_again() {
    for answer in [
        Decision::Denied("the user declined"),
        Decision::Denied("cancelled"),
        Decision::Denied("timed out"),
    ] {
        let rig = rig(Some(ORIGIN), LIST);
        let approver = watch(&rig, answer);
        let (dispatch, mut rx) = rig.turn(Locality::Cloud, approver.clone());
        let first = dispatch.call(list()).await;
        let second = dispatch.call(view(2, false)).await;
        assert!(first.is_error && second.is_error, "{answer:?}");
        assert!(second.text.contains("denied"), "{}", second.text);
        assert_eq!(approver.seen.lock().unwrap().len(), 1, "{answer:?}");
        assert!(rig.calls().is_empty(), "gh must not start: {answer:?}");
        let events = drain(&mut rx);
        let statuses: Vec<ToolStatus> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolResult { status, .. } => Some(*status),
                _ => None,
            })
            .collect();
        assert_eq!(
            statuses,
            [ToolStatus::Denied, ToolStatus::Denied],
            "{answer:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, StreamEvent::GithubStarted))
        );
    }
}

/// Answers the first ask with a decline once both are asked, and the second
/// with a yes a moment later: two parallel calls whose asks were both
/// computed before either was answered.
struct Parallel {
    asked: std::sync::atomic::AtomicUsize,
}

impl Approver for Parallel {
    fn ask(&self, _: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let nth = self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            if nth == 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Decision::Denied("the user declined").into()
            } else {
                tokio::time::sleep(Duration::from_millis(250)).await;
                Decision::Sent.into()
            }
        }
        .boxed()
    }
}

// A decline holds for the turn even against a parallel call that was already
// asked: the answer is read again before `gh` starts.
#[tokio::test]
async fn a_decline_beats_a_parallel_call_asked_before_it() {
    let rig = rig(Some(ORIGIN), LIST);
    let approver = Arc::new(Parallel {
        asked: Default::default(),
    });
    let (dispatch, mut rx) = rig.turn(Locality::Cloud, approver);
    let (first, second) = tokio::join!(dispatch.call(list()), dispatch.call(view(2, false)));
    assert!(first.is_error && second.is_error, "{first:?} {second:?}");
    assert!(second.text.contains("denied"), "{}", second.text);
    assert!(rig.calls().is_empty(), "gh must not start");
    assert!(
        !drain(&mut rx)
            .iter()
            .any(|e| matches!(e, StreamEvent::GithubStarted))
    );
}

// The event travels the turn's ordered channel: it precedes the result of its
// own call and so the next round's call, whatever `gh` then does.
#[tokio::test]
async fn started_comes_before_the_result_and_the_next_rounds_call() {
    let rig = rig(Some(ORIGIN), "exit 2");
    let (dispatch, mut rx) = rig.turn(Locality::Local, Asks::new(Decision::Sent));
    assert!(dispatch.call(list()).await.is_error);
    assert!(dispatch.call(list()).await.is_error);
    assert_eq!(
        shape(&drain(&mut rx)).0,
        ["call", "started", "result", "call", "started", "result"]
    );
}

// Spec (Ollama cloud model asks): an Ollama `*-cloud` model is a local
// adapter that counts as cloud for a GitHub read, and once `gh` has started
// the bit that makes its file reads ask is up for the next call.
#[tokio::test]
async fn an_ollama_cloud_model_asks_for_a_github_read_and_the_start_blocks_its_reads() {
    use std::sync::atomic::Ordering;
    let rig = rig(Some(ORIGIN), LIST);
    let approver = watch(&rig, Decision::Sent);
    let (tx, _rx) = mpsc::channel(64);
    let mut scope = TurnScope::new(tx, Locality::Local, rig.proj.clone());
    scope.ollama_cloud = true;
    scope.approver = approver.clone();
    scope.families = FamilySet::LEGACY.with(Family::Github);
    let scope = Arc::new(scope);
    assert!(!scope.github_read.load(Ordering::Relaxed));
    let out = execute(rig.host.clone(), scope.clone(), list()).await;
    assert!(!out.is_error, "{out:?}");
    let seen = approver.seen.lock().unwrap().clone();
    assert!(
        matches!(&seen[..], [(ApprovalAsk::GithubRead { repo, ollama_cloud: true, .. }, false)] if repo == "o/r"),
        "{seen:?}"
    );
    assert!(scope.github_read.load(Ordering::Relaxed));
    // A refused call starts nothing and leaves the bit down.
    let none = rig_with(None, LIST, "true", Duration::from_secs(30));
    let (tx, _rx) = mpsc::channel(64);
    let mut refused = TurnScope::new(tx, Locality::Local, none.proj.clone());
    refused.ollama_cloud = true;
    refused.families = FamilySet::LEGACY.with(Family::Github);
    let refused = Arc::new(refused);
    assert!(
        execute(none.host.clone(), refused.clone(), list())
            .await
            .is_error
    );
    assert!(!refused.github_read.load(Ordering::Relaxed));
}
