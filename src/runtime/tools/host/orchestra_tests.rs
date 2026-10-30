//! The host in an orchestrated turn: the role toolset filter in `execute`
//! and the orchestration calls routed to the run manager.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::{sync::Mutex, time::Duration};

use serde_json::json;
use tokio::time::timeout;

use super::{
    tests::{Scripted, events, fixture, pair, read, run_cmd, scope},
    *,
};
use crate::core::{
    ai::{
        chat::{
            approval::{ApprovalAsk, Reach, worker_label},
            engine::Availability,
        },
        config::Task,
        orchestra::{
            RunId, Toolset, WorkerId,
            run::{Roster, Run, UserChoice, WorkerEvent},
            scope::ReadScope,
            tools::{MAX_PROMPT_BYTES, OrchCall, WAIT_MAX_SECS},
            worker::ErrorClass,
        },
        tools::{Lines, paths::SecretRoots, summary::SUMMARY_CAP},
    },
    layout::PaneId,
};
use crate::runtime::tools::{Orchestra, Pending, Waits, WorkerScope, approver::ENDED};

/// A run manager that answers every call with `answer` and remembers them.
struct Fake {
    answer: &'static str,
    calls: Mutex<Vec<OrchCall>>,
}

impl Fake {
    fn new(answer: &'static str) -> Arc<Self> {
        Arc::new(Self {
            answer,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<OrchCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Orchestra for Fake {
    fn call(&self, call: OrchCall) -> BoxFuture<'static, Outcome> {
        self.calls.lock().unwrap().push(call);
        let text = self.answer.to_owned();
        async move {
            Outcome {
                text,
                is_error: false,
            }
        }
        .boxed()
    }

    fn allows(&self, _: RunId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
        None
    }
}

fn spawn() -> Call {
    Call::Orchestra(OrchCall::Spawn {
        task: Task::Review,
        prompt: "review the diff".into(),
    })
}

fn server_tool() -> Call {
    Call::External {
        name: "fake__echo".into(),
        server: "fake".into(),
        tool: "echo".into(),
        args: json!({"q": "y"}),
    }
}

/// A worker turn of run 1 on `locality`: read-only tools and a read scope.
fn worker(
    locality: Locality,
    home: &Path,
    handle: Arc<dyn Orchestra>,
) -> (TurnScope, mpsc::Receiver<StreamEvent>) {
    let (mut scope, rx) = scope(locality, home);
    scope.families = Toolset::ReadOnly.families();
    scope.read_scope = Some(WorkerScope {
        handle,
        run: RunId(1),
        label: worker_label(Task::Review, "claude", "opus"),
    });
    (scope, rx)
}

/// An orchestrator turn: read tools, the orchestration tools and `handle`.
fn orchestrator(
    home: &Path,
    handle: Option<Arc<dyn Orchestra>>,
) -> (TurnScope, mpsc::Receiver<StreamEvent>) {
    let (mut scope, rx) = scope(Locality::Cloud, home);
    scope.families = Toolset::Orchestrator.families();
    scope.orchestra = handle;
    (scope, rx)
}

/// Asserts `out` and its row are the one "unknown tool" error, never asked.
fn refused_unknown(out: &Outcome, rx: &mut mpsc::Receiver<StreamEvent>, what: &str) {
    assert!(out.is_error, "{what}: {out:?}");
    assert_eq!(out.text, "unknown tool", "{what}");
    let (_, ask, status, summary) = pair(&events(rx));
    assert_eq!(
        (ask, status, summary.as_str()),
        (None, ToolStatus::Error, "unknown tool"),
        "{what}"
    );
}

// Spec (tasks 1c.3, D7): the role filter is applied in `execute` too. A call
// outside the turn's set (a worker's `run_in_pane`, `spawn_agent` or user
// server tool; the orchestrator's `run_in_pane` or user server tool) ends as
// "unknown tool": never asked, never run, never routed.
#[tokio::test]
async fn an_out_of_set_call_is_refused_in_execute_unasked() {
    let (_dir, home, host) = fixture();
    let cases: [(&str, bool, Call); 5] = [
        ("worker run_in_pane", true, run_cmd("ls")),
        ("worker spawn_agent", true, spawn()),
        ("worker server tool", true, server_tool()),
        ("orchestrator run_in_pane", false, run_cmd("ls")),
        ("orchestrator server tool", false, server_tool()),
    ];
    for (what, is_worker, call) in cases {
        let fake = Fake::new("spawned 1");
        let (mut scope, mut rx) = if is_worker {
            worker(Locality::Local, &home, fake.clone())
        } else {
            orchestrator(&home, Some(fake.clone()))
        };
        let approver = Scripted::new(Decision::Sent);
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(call).await;
        refused_unknown(&out, &mut rx, what);
        assert!(approver.asked.lock().unwrap().is_empty(), "{what}");
        assert!(fake.calls().is_empty(), "{what}");
    }
}

// Spec (tasks 1c.3, D3): `Call::Orchestra` without a handle is `UnknownTool`,
// even in a turn whose toolset lists it; with one, the call goes to it.
#[tokio::test]
async fn an_orchestration_call_without_a_handle_is_an_unknown_tool() {
    let (_dir, home, host) = fixture();
    let (scope, mut rx) = orchestrator(&home, None);
    let out = host.dispatch(scope).call(spawn()).await;
    refused_unknown(&out, &mut rx, "no handle");

    let fake = Fake::new("spawned 1");
    let (scope, mut rx) = orchestrator(&home, Some(fake.clone()));
    let out = host.dispatch(scope).call(spawn()).await;
    assert_eq!((out.is_error, out.text.as_str()), (false, "spawned 1"));
    let OrchCall::Spawn { task, prompt } = &fake.calls()[0] else {
        panic!("not routed: {:?}", fake.calls())
    };
    assert_eq!((*task, prompt.as_str()), (Task::Review, "review the diff"));
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Ok));
}

// Regression: a chat turn (`TurnScope::new`) keeps every chat tool through
// the filter: `run_in_pane` and a user server tool still ask, a cloud read
// still asks with the plain read ask.
#[tokio::test]
async fn a_chat_turn_keeps_its_tools_through_the_filter() {
    let (_dir, home, host) = fixture();
    for call in [run_cmd("ls"), server_tool(), read("~/note.txt")] {
        let (mut scope, mut rx) = scope(Locality::Cloud, &home);
        let approver = Scripted::new(Decision::Denied("declined"));
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(call.clone()).await;
        assert_eq!(out.text, "denied: declined", "{call:?}");
        let (_, ask, status, _) = pair(&events(&mut rx));
        assert_eq!(status, ToolStatus::Denied, "{call:?}");
        let asked = approver.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "{call:?}");
        assert_eq!(ask.as_ref(), asked.first(), "{call:?}");
        if let Call::ReadFile { .. } = call {
            let path = home.join("note.txt").to_str().unwrap().to_owned();
            let (reach, cloud) = (Reach::Broad, true);
            assert_eq!(asked[0], ApprovalAsk::Read { path, reach, cloud });
        }
    }
}

// -- worker reads under the run's read scope (D6) --

/// Adds a project to the fixture's home: `dev/app` is the scope and
/// `dev/other` is outside it. `dev/app/link` leads outside, `dev/app/b.rs`
/// is another file of the scope, `dev/app/hard` is a hard link and `.ssh/id`
/// is a secret.
fn project(home: &Path) {
    for dir in ["dev/app", "dev/other", ".ssh"] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
    }
    for (file, text) in [
        ("dev/app/a.rs", "inside the scope"),
        ("dev/other/a.rs", "outside the scope"),
        ("dev/app/b.rs", "picked by the scope"),
        ("dev/app/c.rs", "hard linked"),
        (".ssh/id", "secret key"),
    ] {
        std::fs::write(home.join(file), text).unwrap();
    }
    std::os::unix::fs::symlink(home.join("dev/other/a.rs"), home.join("dev/app/link")).unwrap();
    std::fs::hard_link(home.join("dev/app/c.rs"), home.join("dev/app/hard")).unwrap();
}

/// A run manager holding run 1 and its read scope.
struct Scoped {
    scope: ReadScope,
    run: Mutex<Run>,
}

impl Scoped {
    /// Run 1 with a scope over `roots`, none of them dropped.
    fn over(home: &Path, roots: &[&str]) -> Arc<Self> {
        let secrets = SecretRoots::load(home, &[]).unwrap();
        let (scope, dropped) = ReadScope::new(RunId(1), &secrets, home, roots, &|_| false);
        assert!(dropped.is_empty(), "{dropped:?}");
        Self::with(scope)
    }

    fn with(scope: ReadScope) -> Arc<Self> {
        let ready = Availability::Ready {
            engine: "claude".into(),
            model: Some("opus".into()),
        };
        let run = Run::new(
            RunId(1),
            Roster::new(vec![(Task::Review, ready)]),
            Duration::from_secs(600),
        );
        Arc::new(Self {
            scope,
            run: Mutex::new(run),
        })
    }

    /// Ends run 1: a worker fails and the user cancels the run.
    fn cancel(&self) {
        let (now, mut run) = (std::time::Instant::now(), self.run.lock().unwrap());
        let id = run.spawn(now, Task::Review, "p").unwrap();
        let failed = WorkerEvent::Failed {
            class: ErrorClass::Other,
            text: "x".into(),
        };
        run.on_event(now, id, failed);
        run.decide(now, UserChoice::Cancel).unwrap();
    }
}

impl Orchestra for Scoped {
    fn call(&self, _: OrchCall) -> BoxFuture<'static, Outcome> {
        std::future::pending().boxed()
    }

    fn allows(
        &self,
        run: RunId,
        secrets: &SecretRoots,
        origin: &Path,
        input: &str,
    ) -> Option<PathBuf> {
        let live = self.run.lock().unwrap();
        if live.id() != run {
            return None;
        }
        self.scope.allows(&live, secrets, origin, input)
    }
}

/// A run manager whose scope hands back `real` for any read.
struct Hands(PathBuf);

impl Orchestra for Hands {
    fn call(&self, _: OrchCall) -> BoxFuture<'static, Outcome> {
        std::future::pending().boxed()
    }

    fn allows(&self, _: RunId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

/// The ask of a worker read of `path` (under the fixture's home) on a cloud
/// engine whose chat origin is the home itself: a read there is `Broad`, as
/// a chat read of it is.
fn worker_ask(home: &Path, path: &str) -> ApprovalAsk {
    worker_ask_as(home, path, Reach::Broad, true)
}

/// The ask of a worker read of `path` that leads to `reach`, on a cloud
/// engine or not.
fn worker_ask_as(home: &Path, path: &str, reach: Reach, cloud: bool) -> ApprovalAsk {
    ApprovalAsk::WorkerRead {
        path: home.join(path).to_str().unwrap().to_owned(),
        label: "review claude>opus".into(),
        reach,
        cloud,
    }
}

/// A worker turn like [`worker`] whose chat origin is `~/dev/app`, the root
/// of its scope, as a run's is.
fn worker_in_app(
    locality: Locality,
    home: &Path,
    handle: Arc<dyn Orchestra>,
) -> (TurnScope, mpsc::Receiver<StreamEvent>) {
    let (mut scope, rx) = worker(locality, home, handle);
    scope.origin = home.join("dev/app");
    (scope, rx)
}

// Spec (agent-tools, Inside runs, outside asks; D6): on a cloud engine a
// read inside the scope runs with no approval and one outside asks. A path
// inside by text whose real path leaves the scope asks too: both must be in.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn inside_the_scope_runs_unasked_and_outside_asks() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    for (input, shown, text) in [
        ("~/dev/app/a.rs", None, "inside the scope"),
        (
            "~/dev/other/a.rs",
            Some("dev/other/a.rs"),
            "outside the scope",
        ),
        ("~/dev/app/link", Some("dev/app/link"), "outside the scope"),
    ] {
        let (mut scope, mut rx) = worker(Locality::Cloud, &home, handle.clone());
        let approver = Scripted::new(Decision::Denied("declined"));
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read(input)).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        let asked = approver.asked.lock().unwrap().clone();
        let want = shown.map(|path| worker_ask(&home, path));
        assert_eq!(ask, want, "{input}");
        assert_eq!(asked, Vec::from_iter(want.clone()), "{input}");
        assert_eq!(out.text.contains(text), want.is_none(), "{input}: {out:?}");
        let status_want = if want.is_none() {
            ToolStatus::Ok
        } else {
            ToolStatus::Denied
        };
        assert_eq!(status, status_want, "{input}");
    }
}

const PANE: PaneId = PaneId::FIRST;
const TURN: u64 = 4;

/// The next event, failing the test when none arrives.
async fn next(rx: &mut mpsc::Receiver<StreamEvent>) -> StreamEvent {
    timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no event")
        .expect("channel closed")
}

// Spec (agent-orchestrator, Outside read asks with a label): a `review`
// worker on `claude>opus` reading outside the scope asks with the sanitized
// path and its label, and the file is not opened before `y`; declined, it
// ends `denied` and the file is never opened.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_outside_read_asks_with_the_label_and_opens_nothing_before_y() {
    for answer in [Decision::Sent, Decision::Denied("the user declined")] {
        let (_dir, home, host) = fixture();
        project(&home);
        let (waits, pending) = (Waits::new(), Pending::new());
        let (mut scope, mut rx) =
            worker(Locality::Cloud, &home, Scoped::over(&home, &["~/dev/app"]));
        scope.approver = waits.approver(PANE, TURN, pending.clone());
        let task = tokio::spawn(host.dispatch(scope).call(read("~/dev/other/a.rs")));
        let StreamEvent::ToolCall { id, approval, .. } = next(&mut rx).await else {
            panic!("not a ToolCall")
        };
        assert_eq!(
            approval.map(|ask| *ask),
            Some(worker_ask(&home, "dev/other/a.rs"))
        );
        std::fs::write(home.join("dev/other/a.rs"), "written after the prompt").unwrap();
        assert_eq!(pending.count(), 1);
        assert!(!task.is_finished());
        assert!(waits.decide(PANE, TURN, id, answer));
        let out = task.await.unwrap();
        let StreamEvent::ToolResult { status, .. } = next(&mut rx).await else {
            panic!("not a result")
        };
        let sent = answer == Decision::Sent;
        assert_eq!(
            out.text.contains("written after the prompt"),
            sent,
            "{out:?}"
        );
        assert_eq!(out.is_error, !sent);
        let want = if sent {
            ToolStatus::Ok
        } else {
            ToolStatus::Denied
        };
        assert_eq!(status, want);
        assert_eq!(pending.count(), 0);
    }
}

// Spec (agent-orchestrator, Outside read follows the mode as a chat read):
// on a local engine a worker read outside the scope asks as a chat read
// outside its root does, with the facts the approval mode reads and the
// label; one inside still runs unasked there.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_read_outside_the_scope_on_a_local_engine_asks_as_a_chat_read() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    for (input, outside, text) in [
        ("~/dev/other/a.rs", true, "outside the scope"),
        ("~/dev/app/a.rs", false, "inside the scope"),
    ] {
        let (mut scope, mut rx) = worker_in_app(Locality::Local, &home, handle.clone());
        let approver = Scripted::new(Decision::Denied("declined"));
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read(input)).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        let want = outside.then(|| worker_ask_as(&home, "dev/other/a.rs", Reach::Other, false));
        assert_eq!(ask, want, "{input}");
        assert_eq!(
            *approver.asked.lock().unwrap(),
            Vec::from_iter(want),
            "{input}"
        );
        assert_eq!(out.text.contains(text), !outside, "{input}: {out:?}");
        let status_want = if outside {
            ToolStatus::Denied
        } else {
            ToolStatus::Ok
        };
        assert_eq!(status, status_want, "{input}");
    }
}

// Spec (agent-orchestrator, Outside read follows the mode as a chat read):
// an answer of the approval mode (`Unprompted`) to a worker read outside the
// scope is taken exactly as for a chat read: a local read outside the root
// runs, a cloud read outside the root is denied and reads nothing, and a
// cloud read the path guard refuses returns the refusal and opens nothing.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_unprompted_answer_to_a_worker_read_is_taken_as_for_a_chat_read() {
    let (_dir, home, host) = fixture();
    project(&home);
    std::fs::write(home.join("dev/other/.env"), "SECRET=1").unwrap();
    let handle = Scoped::over(&home, &["~/dev/app"]);
    for (locality, input, reach, ran) in [
        (Locality::Local, "dev/other/a.rs", Reach::Other, true),
        (Locality::Cloud, "dev/other/a.rs", Reach::Other, false),
        (Locality::Cloud, "dev/other/.env", Reach::Refused, false),
    ] {
        let (mut scope, mut rx) = worker_in_app(locality, &home, handle.clone());
        let approver = Scripted::new(Decision::Unprompted);
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read(&format!("~/{input}"))).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        let cloud = locality == Locality::Cloud;
        let want = worker_ask_as(&home, input, reach, cloud);
        assert_eq!(ask, Some(want.clone()), "{input} {locality:?}");
        assert_eq!(*approver.asked.lock().unwrap(), vec![want], "{input}");
        assert_eq!(out.text.contains("outside the scope"), ran, "{out:?}");
        assert!(!out.text.contains("SECRET"), "{out:?}");
        let status_want = match (ran, reach) {
            (true, _) => ToolStatus::Ok,
            (false, Reach::Refused) => ToolStatus::Error,
            (false, _) => ToolStatus::Denied,
        };
        assert_eq!(status, status_want, "{input} {locality:?}");
        if reach == Reach::Other && !ran {
            assert_eq!(out.text, format!("denied: {NOT_APPROVED}"));
        }
    }
}

// Regression: a read in a turn with no read scope keeps the chat rules: free
// on a local engine, and on a cloud one the plain read ask, with no label.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_read_outside_any_scope_keeps_the_chat_rules() {
    let (_dir, home, host) = fixture();
    project(&home);
    let path = home.join("dev/other/a.rs").to_str().unwrap().to_owned();
    for (locality, want) in [
        (Locality::Local, None),
        (
            Locality::Cloud,
            Some(ApprovalAsk::Read {
                path,
                reach: Reach::Broad,
                cloud: true,
            }),
        ),
    ] {
        let (mut scope, mut rx) = scope(locality, &home);
        let approver = Scripted::new(Decision::Sent);
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read("~/dev/other/a.rs")).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        assert_eq!(ask, want, "{locality:?}");
        assert_eq!(
            *approver.asked.lock().unwrap(),
            Vec::from_iter(want),
            "{locality:?}"
        );
        assert!(out.text.contains("outside the scope"), "{out:?}");
        assert_eq!(status, ToolStatus::Ok);
    }
}

// Obligation (S1b-2 security review): a scoped read opens exactly the path
// the scope returned, never the request resolved again, with no link
// followed, and the opened file is judged again: a secret, a link and a
// hard-linked file are refused, and none of them asks.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_scoped_read_opens_exactly_the_returned_path_and_judges_it_again() {
    let (_dir, home, host) = fixture();
    project(&home);
    for (returned, opened) in [
        ("dev/app/b.rs", true),
        (".ssh/id", false),
        ("dev/app/link", false),
        ("dev/app/hard", false),
    ] {
        let handle = Arc::new(Hands(home.join(returned)));
        let (mut scope, mut rx) = worker(Locality::Cloud, &home, handle);
        let approver = Scripted::new(Decision::Sent);
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read("~/dev/app/a.rs")).await;
        let (_, ask, status, summary) = pair(&events(&mut rx));
        assert_eq!(ask, None, "{returned}");
        assert!(approver.asked.lock().unwrap().is_empty(), "{returned}");
        assert!(
            !out.text.contains("inside the scope"),
            "{returned}: {out:?}"
        );
        if opened {
            assert!(out.text.contains("picked by the scope"), "{out:?}");
            assert_eq!(status, ToolStatus::Ok);
        } else {
            assert_eq!(
                (out.is_error, out.text.as_str()),
                (true, "path not available"),
                "{returned}"
            );
            assert_eq!(
                (status, summary.as_str()),
                (ToolStatus::Error, "path not available")
            );
        }
    }
}

// Obligation (6c88355, main's `read_file` paging): a scoped read is paged
// like a chat read, and the window opens no way around the scope's checks:
// a returned secret, link out, hard link or `.git/config` is still refused.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_scoped_read_is_paged_and_keeps_its_refusals() {
    let (_dir, home, host) = fixture();
    project(&home);
    let numbered: String = (1..=50).map(|n| format!("line {n}\n")).collect();
    std::fs::write(home.join("dev/app/long.rs"), numbered).unwrap();
    std::fs::create_dir_all(home.join("dev/app/.git")).unwrap();
    std::fs::write(home.join("dev/app/.git/config"), "[remote]\n").unwrap();
    let paged = Call::ReadFile {
        path: "~/dev/app/a.rs".into(),
        lines: Some(Lines { start: 10, max: 3 }),
    };
    let handle = Arc::new(Hands(home.join("dev/app/long.rs")));
    let (scope, mut rx) = worker(Locality::Cloud, &home, handle);
    let out = host.dispatch(scope).call(paged.clone()).await;
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Ok));
    assert!(out.text.contains("line 10\nline 11\nline 12\n"), "{out:?}");
    assert!(
        !out.text.contains("line 9\n") && !out.text.contains("line 13\n"),
        "{out:?}"
    );
    for returned in [
        ".ssh/id",
        "dev/app/link",
        "dev/app/hard",
        "dev/app/.git/config",
    ] {
        let handle = Arc::new(Hands(home.join(returned)));
        let (scope, mut rx) = worker(Locality::Cloud, &home, handle);
        let out = host.dispatch(scope).call(paged.clone()).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        assert_eq!((ask, status), (None, ToolStatus::Error), "{returned}");
        assert_eq!(
            (out.is_error, out.text.as_str()),
            (true, "path not available"),
            "{returned}"
        );
    }
}

// Obligation (S1b-2 security review): a denied scope, the scope of an ended
// run and a turn of another run grant nothing: the read asks as the same chat
// read would (here a cloud one), never allowed and never refused in silence
// (after `y` the file is read).
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_denied_or_expired_scope_asks() {
    let (_dir, home, host) = fixture();
    project(&home);
    for case in ["denied", "ended run", "another run"] {
        let handle = match case {
            "denied" => Scoped::with(ReadScope::denied(RunId(1))),
            _ => Scoped::over(&home, &["~/dev/app"]),
        };
        if case == "ended run" {
            handle.cancel();
        }
        let (mut scope, mut rx) = worker(Locality::Cloud, &home, handle);
        if case == "another run" {
            scope.read_scope.as_mut().unwrap().run = RunId(2);
        }
        let approver = Scripted::new(Decision::Sent);
        scope.approver = approver.clone();
        let out = host.dispatch(scope).call(read("~/dev/app/a.rs")).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        let want = worker_ask(&home, "dev/app/a.rs");
        assert_eq!(ask.as_ref(), Some(&want), "{case}");
        assert_eq!(*approver.asked.lock().unwrap(), [want], "{case}");
        assert!(out.text.contains("inside the scope"), "{case}: {out:?}");
        assert_eq!(status, ToolStatus::Ok, "{case}");
    }
}

// Obligation (S1b-2 security review): a spawn prompt (up to 16 KiB) is cut
// before it is shown: its row is one line of at most `SUMMARY_CAP`
// characters and it asks nothing; the run manager still gets all of it.
#[tokio::test]
async fn a_long_spawn_prompt_is_shown_clipped() {
    let (_dir, home, host) = fixture();
    let prompt = "word ".repeat(MAX_PROMPT_BYTES / 5);
    let fake = Fake::new("spawned 1");
    let (scope, mut rx) = orchestrator(&home, Some(fake.clone()));
    let call = Call::Orchestra(OrchCall::Spawn {
        task: Task::Review,
        prompt: prompt.clone(),
    });
    host.dispatch(scope).call(call).await;
    let seen = events(&mut rx);
    let StreamEvent::ToolCall {
        summary, approval, ..
    } = &seen[0]
    else {
        panic!("not a ToolCall: {seen:?}")
    };
    assert!(summary.chars().count() <= SUMMARY_CAP, "{summary}");
    assert!(!summary.contains('\n'));
    assert_eq!(*approval, None);
    let [OrchCall::Spawn { prompt: got, .. }] = &fake.calls()[..] else {
        panic!("not routed")
    };
    assert_eq!(*got, prompt);
}

// -- held calls and pending worker asks: only the user decides --

/// A run manager whose calls never answer, as a `wait` held by an
/// unresolved failure.
struct Held(Mutex<Vec<OrchCall>>);

impl Orchestra for Held {
    fn call(&self, call: OrchCall) -> BoxFuture<'static, Outcome> {
        self.0.lock().unwrap().push(call);
        std::future::pending().boxed()
    }

    fn allows(&self, _: RunId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
        None
    }
}

fn wait_for(id: u32) -> Call {
    Call::Orchestra(OrchCall::Wait {
        ids: vec![WorkerId(id)],
        timeout: WAIT_MAX_SECS,
    })
}

// Spec (lazarobox-mcp-server, Bound held): with a failure unresolved the
// run manager holds `wait`; the host still returns `awaiting_user` within
// 120 s, as a result the model can read, and asks nothing.
#[tokio::test(start_paused = true)]
async fn a_held_wait_returns_awaiting_user_within_120_s() {
    let (_dir, home, host) = fixture();
    let held = Arc::new(Held(Mutex::new(Vec::new())));
    let (scope, mut rx) = orchestrator(&home, Some(held.clone()));
    let start = tokio::time::Instant::now();
    let bound = Duration::from_secs(u64::from(WAIT_MAX_SECS));
    let out = timeout(bound * 10, host.dispatch(scope).call(wait_for(1)))
        .await
        .expect("wait held past the bound");
    assert!(start.elapsed() <= bound, "{:?}", start.elapsed());
    assert_eq!((out.is_error, out.text.as_str()), (false, "awaiting_user"));
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Ok));
    assert_eq!(held.0.lock().unwrap().len(), 1);
}

// Regression: a `wait` the run manager answers in time is its answer.
#[tokio::test]
async fn a_wait_answered_in_time_is_the_run_managers_answer() {
    let (_dir, home, host) = fixture();
    let fake = Fake::new("1: done");
    let (scope, _rx) = orchestrator(&home, Some(fake.clone()));
    let out = host.dispatch(scope).call(wait_for(1)).await;
    assert_eq!((out.is_error, out.text.as_str()), (false, "1: done"));
    assert_eq!(fake.calls().len(), 1);
}

/// A worker turn of run 1 on a cloud engine, scoped to `~/dev/app`, whose
/// asks wait in `waits` under `PANE` and `TURN`, and its read of a file
/// outside the scope, announced: its task, call id and pending count.
async fn pending_outside_read(
    host: &Arc<ToolHost>,
    home: &Path,
    waits: &Arc<Waits>,
) -> (
    tokio::task::JoinHandle<Outcome>,
    CallId,
    Pending,
    mpsc::Receiver<StreamEvent>,
) {
    let pending = Pending::new();
    let (mut scope, mut rx) = worker(Locality::Cloud, home, Scoped::over(home, &["~/dev/app"]));
    scope.approver = waits.approver(PANE, TURN, pending.clone());
    let task = tokio::spawn(host.dispatch(scope).call(read("~/dev/other/a.rs")));
    let StreamEvent::ToolCall { id, approval, .. } = next(&mut rx).await else {
        panic!("not a ToolCall")
    };
    assert_eq!(
        approval.map(|ask| *ask),
        Some(worker_ask(home, "dev/other/a.rs"))
    );
    assert_eq!(pending.count(), 1);
    (task, id, pending, rx)
}

// Spec (agent-orchestrator, Orchestrator cannot approve): while a worker's
// read waits, the orchestrator turn's calls that look like `y` (answers of
// the run manager, a forged `run_in_pane`, a user tool) answer nothing; the
// read stays pending until the user's key decides it.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_orchestrator_cannot_answer_a_workers_pending_read() {
    let (_dir, home, host) = fixture();
    project(&home);
    let waits = Waits::new();
    let (task, id, pending, _rx) = pending_outside_read(&host, &home, &waits).await;
    let fake = Fake::new("y");
    let (mut scope, _orch_rx) = orchestrator(&home, Some(fake.clone()));
    scope.approver = waits.approver(PANE, TURN, Pending::new());
    let dispatch = host.dispatch(scope);
    for call in [
        Call::Orchestra(OrchCall::Collect { id: WorkerId(1) }),
        wait_for(1),
        run_cmd("y"),
        server_tool(),
    ] {
        dispatch.call(call).await;
    }
    assert_eq!(fake.calls().len(), 2);
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    assert_eq!(pending.count(), 1);
    // The user's key: the read was still waiting for it.
    assert!(waits.decide(PANE, TURN, id, Decision::Sent));
    let out = task.await.unwrap();
    assert!(out.text.contains("outside the scope"), "{out:?}");
    assert_eq!(pending.count(), 0);
}

// Spec (agent-orchestrator, Never silently decided): an outside read left
// pending while time passes (past the `wait` bound and the worker deadline)
// is not allowed by any timer; the timeout that ends the turn ends it as
// `denied`, and the file is never opened.
#[tokio::test]
async fn an_outside_read_is_never_decided_by_a_timer() {
    let (_dir, home, host) = fixture();
    project(&home);
    let waits = Waits::new();
    let (task, id, pending, mut rx) = pending_outside_read(&host, &home, &waits).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(3600)).await;
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    assert_eq!(pending.count(), 1);
    // What the turn's timeout does.
    waits.end_turn(PANE, TURN);
    let out = task.await.unwrap();
    let StreamEvent::ToolResult { status, .. } = next(&mut rx).await else {
        panic!("not a result")
    };
    assert_eq!(status, ToolStatus::Denied);
    assert!(out.is_error && out.text.contains(ENDED), "{out:?}");
    assert!(!out.text.contains("outside the scope"));
    assert_eq!(pending.count(), 0);
    assert!(!waits.decide(PANE, TURN, id, Decision::Sent));
}

// Spec (lazarobox-mcp-server, Closed session denies): a worker cancelled
// with a read pending: its call is dropped, or its session closes; either
// way the approval ends `denied`, nothing is opened and a late `y` finds
// nothing to answer.
#[tokio::test]
async fn a_cancelled_workers_pending_read_ends_denied() {
    for how in ["call dropped", "session closed"] {
        let (_dir, home, host) = fixture();
        project(&home);
        let waits = Waits::new();
        let (task, id, pending, mut rx) = pending_outside_read(&host, &home, &waits).await;
        match how {
            "call dropped" => task.abort(),
            _ => waits.end_pane(PANE),
        }
        let StreamEvent::ToolResult { status, .. } = next(&mut rx).await else {
            panic!("{how}: not a result")
        };
        assert_eq!(status, ToolStatus::Denied, "{how}");
        if let Ok(out) = task.await {
            assert!(out.is_error && out.text.contains(ENDED), "{how}: {out:?}");
            assert!(!out.text.contains("outside the scope"), "{how}");
        }
        assert_eq!(pending.count(), 0, "{how}");
        assert!(!waits.decide(PANE, TURN, id, Decision::Sent), "{how}");
    }
}
