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
            changes::{Change, ChangeLedger},
            review::Assessment,
            run::{Roster, Run, UserChoice, WorkerEvent},
            scope::ReadScope,
            tools::{MAX_PROMPT_BYTES, OrchCall, WAIT_MAX_SECS},
            worker::ErrorClass,
        },
        tools::{
            Lines,
            paths::{REFUSED, SecretRoots},
            registry::Family,
            stage::StageKey,
            summary::SUMMARY_CAP,
        },
    },
    layout::PaneId,
};
use crate::runtime::tools::{Answered, Orchestra, Pending, Waits, WorkerScope, approver::ENDED};

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

    fn allows(&self, _: RunId, _: WorkerId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
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
        worker: WorkerId(1),
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
// server tool; the orchestrator's `run_in_pane`) ends as "unknown tool":
// never asked, never run, never routed.
#[tokio::test]
async fn an_out_of_set_call_is_refused_in_execute_unasked() {
    let (_dir, home, host) = fixture();
    let cases: [(&str, bool, Call); 4] = [
        ("worker run_in_pane", true, run_cmd("ls")),
        ("worker spawn_agent", true, spawn()),
        ("worker server tool", true, server_tool()),
        ("orchestrator run_in_pane", false, run_cmd("ls")),
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

// Spec (agent-tools, Toolsets by session role; user decision 2026-10-29): the
// orchestrator turn may call the user's server tools, each call asking `y`
// as in a chat: after `y` it runs, after a decline nothing reaches the
// server.
#[tokio::test]
async fn an_orchestrator_server_tool_asks_and_runs_only_after_y() {
    use crate::runtime::mcp::{
        client::Timeouts,
        external::{
            ExternalSet,
            fake::{host_env, lab, timeouts},
        },
    };
    for (decision, runs) in [
        (Decision::Denied("the user declined"), false),
        (Decision::Sent, true),
    ] {
        let lab = lab();
        let server = lab.slow("fake", "echo", "echoed", "0");
        let limit = Duration::from_secs(10);
        let set = ExternalSet::start(
            &[&server],
            &lab.home,
            host_env,
            Timeouts {
                call: limit,
                ..timeouts(limit)
            },
        )
        .await;
        let fake = Fake::new("spawned 1");
        let (mut scope, mut rx) = orchestrator(&lab.home, Some(fake.clone()));
        assert!(scope.external.set(Arc::new(set)).is_ok());
        let approver = Scripted::new(decision);
        scope.approver = approver.clone();
        let host = ToolHost::new(Some(lab.home.clone().into()), None, None, Limits::default());
        let out = host.dispatch(scope).call(server_tool()).await;
        let (_, ask, status, _) = pair(&events(&mut rx));
        assert!(matches!(ask, Some(ApprovalAsk::External { .. })), "{ask:?}");
        assert_eq!(approver.asked.lock().unwrap().len(), 1, "asked once");
        let received = lab.received("fake");
        if runs {
            assert!(!out.is_error, "{out:?}");
            assert!(out.text.contains("echoed"), "{}", out.text);
            assert_eq!(status, ToolStatus::Ok);
            assert!(received.contains("\"name\":\"echo\""), "{received}");
        } else {
            assert!(out.is_error && out.text.contains("declined"), "{out:?}");
            assert_eq!(status, ToolStatus::Denied);
            assert_eq!(received, "", "nothing reached the server");
        }
        assert!(fake.calls().is_empty(), "never routed to the run");
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
    /// What the host reported a worker staged and applied.
    ledger: Mutex<ChangeLedger>,
    /// What happened, in order: the review hook and the approver write here.
    log: Arc<Mutex<Vec<String>>>,
    /// The worker is the correction of a review.
    corrector: bool,
    /// `review` cannot start, for this reason.
    start_error: Option<&'static str>,
    /// Stages while the review runs, as a sibling worker would.
    sibling: Mutex<Option<(Arc<dyn Dispatch>, &'static str)>>,
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
            ledger: Mutex::default(),
            log: Arc::default(),
            corrector: false,
            start_error: None,
            sibling: Mutex::new(None),
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
        _: WorkerId,
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

    fn allows_write(&self, run: RunId, _: WorkerId, real: &Path) -> bool {
        let live = self.run.lock().unwrap();
        live.id() == run && self.scope.allows_write(&live, real)
    }

    fn staged(&self, _: RunId, _: WorkerId, change: Change) {
        self.ledger.lock().unwrap().staged(change);
    }

    fn review(
        &self,
        _: RunId,
        _: WorkerId,
        _: Vec<Change>,
        _: Assessment,
        answered: Answered,
    ) -> BoxFuture<'static, Result<(), String>> {
        self.log
            .lock()
            .unwrap()
            .push(format!("review {answered:?}"));
        let error = self.start_error;
        let sibling = self.sibling.lock().unwrap().clone();
        Box::pin(async move {
            if let Some((dispatch, path)) = sibling {
                dispatch.call(write_call(path, "late")).await;
            }
            error.map_or(Ok(()), |why| Err(why.to_owned()))
        })
    }

    fn late(&self, _: RunId, _: WorkerId, changes: Vec<Change>, a: Assessment, reviewed: bool) {
        let paths: Vec<_> = changes.iter().map(|c| c.path.clone()).collect();
        let line = format!("late {} {:?} reviewed={reviewed}", paths.join(","), a.tier);
        self.log.lock().unwrap().push(line);
    }

    fn unread(&self, _: RunId, _: WorkerId, path: &str) {
        self.log.lock().unwrap().push(format!("unread {path}"));
    }

    fn corrects(&self, _: RunId, _: WorkerId) -> bool {
        self.corrector
    }

    fn applied(&self, _: RunId, _: WorkerId, paths: &[String]) {
        self.ledger.lock().unwrap().applied(paths);
    }
}

/// A run manager whose scope hands back `real` for any read.
struct Hands(PathBuf);

impl Orchestra for Hands {
    fn call(&self, _: OrchCall) -> BoxFuture<'static, Outcome> {
        std::future::pending().boxed()
    }

    fn allows(&self, _: RunId, _: WorkerId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
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
        worker: crate::core::ai::orchestra::WorkerKey::for_test(1, 1),
        path: home.join(path).to_str().unwrap().to_owned(),
        label: "1/1 review claude>opus".into(),
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
        let mut want = worker_ask(&home, "dev/app/a.rs");
        if let (ApprovalAsk::WorkerRead { worker, .. }, "another run") = (&mut want, case) {
            worker.run = RunId(2);
        }
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

    fn allows(&self, _: RunId, _: WorkerId, _: &SecretRoots, _: &Path, _: &str) -> Option<PathBuf> {
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
// the run manager, a forged `run_in_pane`, a user tool, which raises an ask
// of its own) answer nothing; the read stays pending until the user's key
// decides it.
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
    let asks = Pending::new();
    scope.approver = waits.approver(PANE, TURN, asks.clone());
    let dispatch = host.dispatch(scope);
    for call in [
        Call::Orchestra(OrchCall::Collect { id: WorkerId(1) }),
        wait_for(1),
        run_cmd("y"),
    ] {
        dispatch.call(call).await;
    }
    // A user tool waits for its own `y` (user decision, 2026-10-29).
    let asking = tokio::spawn({
        let dispatch = dispatch.clone();
        async move { dispatch.call(server_tool()).await }
    });
    assert_eq!(fake.calls().len(), 2);
    for _ in 0..100 {
        if asks.count() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asks.count(), 1, "the user tool reached its own ask");
    assert!(!task.is_finished());
    assert!(!asking.is_finished(), "the user tool waits for the user");
    assert_eq!(pending.count(), 1);
    // The user's key: the read was still waiting for it.
    assert!(waits.decide(PANE, TURN, id, Decision::Sent));
    let out = task.await.unwrap();
    assert!(out.text.contains("outside the scope"), "{out:?}");
    assert_eq!(pending.count(), 0);
    asking.abort();
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

// Spec (agent-orchestrator, Cancel denies pending asks): a worker ended while
// its ask waits for room in the events channel is not announced at all: the
// user is never shown a prompt nobody can answer.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_ended_before_its_ask_is_announced_never_shows_it() {
    let (_dir, home, host) = fixture();
    project(&home);
    let waits = Waits::new();
    let key = crate::core::ai::orchestra::WorkerKey::for_test(1, 1);
    let (mut scope, _rx) = worker(Locality::Cloud, &home, Scoped::over(&home, &["~/dev/app"]));
    let session = crate::core::ai::orchestra::SessionKey::Worker(PANE, key);
    scope.approver = waits.approver(session, TURN, Pending::new());
    // A channel with no room: the announcement has to wait for a slot.
    let (tx, mut rx) = mpsc::channel(2);
    for _ in 0..2 {
        tx.send(StreamEvent::Notice("full".into())).await.unwrap();
    }
    scope.events = tx;
    let task = tokio::spawn(host.dispatch(scope).call(read("~/dev/other/a.rs")));
    while waits.registered() == 0 {
        tokio::task::yield_now().await;
    }
    waits.end_worker(PANE, key);
    for _ in 0..2 {
        assert!(matches!(next(&mut rx).await, StreamEvent::Notice(_)));
    }
    let out = task.await.unwrap();
    assert!(out.is_error, "{out:?}");
    assert!(rx.try_recv().is_err(), "no ToolCall was announced");
}

/// A `code` worker turn of pane [`PANE`] in `~/dev/app`: the read and write
/// families and the pane's batch, as the run manager opens it (S7).
fn writer(
    host: &Arc<ToolHost>,
    locality: Locality,
    home: &Path,
    handle: Arc<dyn Orchestra>,
    approver: Arc<dyn Approver>,
) -> Arc<dyn Dispatch> {
    let (mut scope, _rx) = worker_in_app(locality, home, handle);
    scope.families = Toolset::ReadOnly.families().with(Family::Write);
    scope.stage_key = Some(StageKey::Pane(PANE));
    scope.approver = approver;
    // The rows are not looked at here: a full channel would only wait.
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    host.dispatch(scope)
}

fn write_call(path: &str, content: &str) -> Call {
    Call::WriteFile {
        path: path.into(),
        content: content.into(),
    }
}

fn paths_of(ledger: &ChangeLedger) -> Vec<(String, Option<u64>, bool)> {
    let each = ledger.changes().iter();
    each.map(|c| (c.path.clone(), c.size, c.applied)).collect()
}

// Spec (agent-orchestrator, Writes wait for the batch review [S7]): a `code`
// worker's write inside its run's scope is staged with no ask (an edit's
// read included, on a cloud engine), goes to its ledger, and nothing reaches
// the disk. Its `propose_batch` asks a batch review labelled with the
// worker; the approval mode's answer (`run` included) never approves it, and
// only the user's `y` writes, which the ledger then marks applied.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_write_in_scope_is_staged_and_only_the_users_review_writes_it() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let unprompted = Scripted::new(Decision::Unprompted);
    let d = writer(
        &host,
        Locality::Cloud,
        &home,
        handle.clone(),
        unprompted.clone(),
    );
    let staged = d.call(write_call("new.rs", "fn x() {}")).await;
    assert!(
        !staged.is_error && staged.text.starts_with("staged new.rs"),
        "{staged:?}"
    );
    let edit = Call::EditFile {
        path: "a.rs".into(),
        old: "inside".into(),
        new: "changed".into(),
    };
    let edited = d.call(edit).await;
    assert!(!edited.is_error, "{edited:?}");
    assert!(
        unprompted.asked.lock().unwrap().is_empty(),
        "staging asks nothing"
    );
    let app = home.join("dev/app");
    assert!(!app.join("new.rs").exists());
    assert_eq!(
        std::fs::read_to_string(app.join("a.rs")).unwrap(),
        "inside the scope"
    );
    assert_eq!(
        paths_of(&handle.ledger.lock().unwrap()),
        [
            ("new.rs".into(), Some(9), false),
            ("a.rs".into(), Some(17), false)
        ]
    );
    let denied = d.call(Call::ProposeBatch).await;
    assert!(
        denied.is_error && denied.text.starts_with("denied"),
        "{denied:?}"
    );
    let asked = unprompted.asked.lock().unwrap().clone();
    let [ApprovalAsk::Batch(review)] = asked.as_slice() else {
        panic!("{asked:?}");
    };
    assert_eq!(review.label(), Some("1/1 review claude>opus"));
    assert!(
        !app.join("new.rs").exists(),
        "the approval mode wrote nothing"
    );
    let user = Scripted::new(Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!d.call(write_call("new.rs", "fn y() {}")).await.is_error);
    let applied = d.call(Call::ProposeBatch).await;
    assert!(!applied.is_error, "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(app.join("new.rs")).unwrap(),
        "fn y() {}"
    );
    assert_eq!(
        paths_of(&handle.ledger.lock().unwrap()),
        [
            ("new.rs".into(), Some(9), true),
            ("a.rs".into(), Some(17), false)
        ]
    );
}

// Spec (agent-orchestrator, Worker toolsets [S7]: no scope pre-approves a
// write, and a write outside it is refused as a chat write is): a write
// inside the chat's root but outside the run's scope, into a secret path or
// out of the root is refused with the one refusal text, asks nothing and
// stages nothing; one inside the scope is staged.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_write_outside_its_scope_or_into_a_secret_is_refused() {
    let (_dir, home, host) = fixture();
    project(&home);
    std::fs::create_dir_all(home.join("dev/app/src")).unwrap();
    let handle = Scoped::over(&home, &["~/dev/app/src"]);
    let approver = Scripted::new(Decision::Sent);
    let d = writer(
        &host,
        Locality::Local,
        &home,
        handle.clone(),
        approver.clone(),
    );
    for path in ["README.md", ".env", "../other/x.rs", "~/.ssh/new"] {
        let out = d.call(write_call(path, "x")).await;
        assert!(out.is_error, "{path}");
        assert_eq!(
            out.text,
            format!("{REFUSED} (paths are relative to the chat's folder ~/dev/app)"),
            "{path}"
        );
    }
    let out = d.call(write_call("src/ok.rs", "ok")).await;
    assert!(!out.is_error, "{out:?}");
    assert!(approver.asked.lock().unwrap().is_empty());
    assert_eq!(
        paths_of(&handle.ledger.lock().unwrap()),
        [("src/ok.rs".into(), Some(2), false)]
    );
    let stages = host.stages.lock().unwrap();
    let batch = stages.batch(StageKey::Pane(PANE)).unwrap();
    assert_eq!(batch.entries().len(), 1);
}

/// Answers a consent and a batch as told and logs which ask came, in order.
struct ByAsk {
    consent: Decision,
    batch: Decision,
    log: Arc<Mutex<Vec<String>>>,
    ids: Mutex<Vec<(CallId, &'static str)>>,
    asks: Mutex<Vec<ApprovalAsk>>,
}

impl ByAsk {
    fn new(handle: &Scoped, consent: Decision, batch: Decision) -> Arc<Self> {
        Arc::new(Self {
            consent,
            batch,
            log: handle.log.clone(),
            ids: Mutex::default(),
            asks: Mutex::default(),
        })
    }
}

impl Approver for ByAsk {
    fn ask(&self, id: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        let (kind, answer) = match ask {
            ApprovalAsk::Consent { .. } => ("consent", self.consent),
            ApprovalAsk::Batch(_) => ("batch", self.batch),
            other => panic!("{other:?}"),
        };
        self.log.lock().unwrap().push(kind.to_owned());
        self.ids.lock().unwrap().push((id, kind));
        self.asks.lock().unwrap().push(ask.clone());
        async move { answer.into() }.boxed()
    }
}

const DECLINED: Decision = Decision::Denied("the user declined");

// Spec (agent-orchestrator, Consent first [S7b]): a `high` change asks the
// user for consent, on its own wait, before the review runs and before the
// batch review. The ask is built from the assessment and the staged ledger.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_high_change_asks_consent_first_then_reviews_then_the_batch_asks() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Sent, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    let out = d.call(Call::ProposeBatch).await;
    assert!(!out.is_error, "{out:?}");
    assert_eq!(
        *handle.log.lock().unwrap(),
        ["consent", "review Granted", "batch"]
    );
    let asks = user.asks.lock().unwrap().clone();
    let ApprovalAsk::Consent {
        worker,
        label,
        change,
    } = &asks[0]
    else {
        panic!("{asks:?}");
    };
    assert_eq!(worker.worker, WorkerId(1));
    assert_eq!(label, "1/1 review claude>opus");
    assert!(change.contains("Cargo.lock"), "{change}");
    let ids = user.ids.lock().unwrap().clone();
    assert_ne!(ids[0].0, ids[1].0, "the consent has a wait of its own");
}

// Spec (agent-orchestrator, Declined goes on unreviewed [S7b]): `n` to the
// consent runs no review and is no failure of the worker's call; the batch
// review still opens and its answer decides, whatever the consent said.
// W1: the decline never reaches the worker's own tool call.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_declined_consent_goes_on_unreviewed_and_the_batch_review_still_decides() {
    let (_dir, home, host) = fixture();
    project(&home);
    let app = home.join("dev/app");
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, DECLINED, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    let out = d.call(Call::ProposeBatch).await;
    assert!(
        !out.is_error,
        "the decline is not the worker's failure: {out:?}"
    );
    assert_eq!(
        *handle.log.lock().unwrap(),
        ["consent", "review Declined", "batch"]
    );
    assert_eq!(
        std::fs::read_to_string(app.join("Cargo.lock")).unwrap(),
        "x"
    );
    // The same decline on the batch itself denies it.
    let user = ByAsk::new(&handle, Decision::Sent, DECLINED);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user);
    assert!(!d.call(write_call("Cargo.lock", "y")).await.is_error);
    let denied = d.call(Call::ProposeBatch).await;
    assert!(
        denied.is_error && denied.text.starts_with("denied"),
        "{denied:?}"
    );
    assert_eq!(
        std::fs::read_to_string(app.join("Cargo.lock")).unwrap(),
        "x"
    );
}

// A passive change asks nothing before its batch review.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_passive_change_goes_straight_to_the_batch_review() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Sent, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user);
    assert!(!d.call(write_call("new.rs", "fn x() {}")).await.is_error);
    assert!(!d.call(Call::ProposeBatch).await.is_error);
    assert_eq!(*handle.log.lock().unwrap(), ["batch"]);
}

// Spec (One correction at most): the correction worker stages its fixes in
// the pane's batch; the worker under review proposes it. Asks nothing.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_correction_worker_cannot_propose_the_batch() {
    let (_dir, home, host) = fixture();
    project(&home);
    let mut fake = Scoped::over(&home, &["~/dev/app"]);
    Arc::get_mut(&mut fake).unwrap().corrector = true;
    let user = ByAsk::new(&fake, Decision::Sent, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, fake.clone(), user);
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.is_error, "{out:?}");
    assert!(out.text.contains("correction"), "{out:?}");
    assert!(fake.log.lock().unwrap().is_empty(), "nothing asked");
}

// Spec (Each lens runs as a read-only worker over the staged change): a
// read-only review worker reads the staged text, marked pending.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_review_worker_reads_the_staged_view_and_cannot_stage() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = Scripted::new(Decision::Sent);
    let w = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!w.call(write_call("a.rs", "staged text")).await.is_error);
    let (mut scope, _rx) = worker_in_app(Locality::Cloud, &home, handle.clone());
    scope.families = Toolset::ReadOnly.families();
    scope.stage_key = Some(StageKey::Pane(PANE));
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let lens = host.dispatch(scope);
    let seen = lens.call(read("a.rs")).await;
    assert!(seen.text.contains("staged text"), "{seen:?}");
    assert!(seen.text.contains("pending, not on disk"), "{seen:?}");
    let write = lens.call(write_call("evil.rs", "x")).await;
    assert!(write.is_error, "{write:?}");
}

/// A writer whose rows are kept.
fn writer_rows(
    host: &Arc<ToolHost>,
    home: &Path,
    handle: Arc<dyn Orchestra>,
    approver: Arc<dyn Approver>,
) -> (Arc<dyn Dispatch>, Arc<Mutex<Vec<StreamEvent>>>) {
    let (mut scope, _rx) = worker_in_app(Locality::Cloud, home, handle);
    scope.families = Toolset::ReadOnly.families().with(Family::Write);
    scope.stage_key = Some(StageKey::Pane(PANE));
    scope.approver = approver;
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    let rows = Arc::new(Mutex::new(Vec::new()));
    let kept = rows.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            kept.lock().unwrap().push(event);
        }
    });
    (host.dispatch(scope), rows)
}

fn result_summaries(rows: &Mutex<Vec<StreamEvent>>) -> Vec<(ToolStatus, String)> {
    let rows = rows.lock().unwrap();
    rows.iter()
        .filter_map(|event| match event {
            StreamEvent::ToolResult {
                status, summary, ..
            } => Some((*status, summary.clone())),
            _ => None,
        })
        .collect()
}

// W5: an ask nobody answered (cancelled, expired) is recorded as such, never
// as the user's decline.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_consent_nobody_answered_is_not_recorded_as_a_decline() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Denied("turn ended"), Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user);
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    assert!(!d.call(Call::ProposeBatch).await.is_error);
    let log = handle.log.lock().unwrap().clone();
    assert_eq!(log[1], "review Unanswered(\"turn ended\")", "{log:?}");
}

// W5: a review that cannot start shows a row of its own and the batch goes on
// to the user's review.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_review_that_cannot_start_is_shown_and_the_batch_goes_on() {
    let (_dir, home, host) = fixture();
    project(&home);
    let mut fake = Scoped::over(&home, &["~/dev/app"]);
    Arc::get_mut(&mut fake).unwrap().start_error = Some("the run ended");
    let user = ByAsk::new(&fake, Decision::Sent, Decision::Sent);
    let (d, rows) = writer_rows(&host, &home, fake.clone(), user);
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    assert!(!d.call(Call::ProposeBatch).await.is_error);
    tokio::task::yield_now().await;
    let seen = result_summaries(&rows);
    assert!(
        seen.iter()
            .any(|(status, text)| *status == ToolStatus::Error
                && text.contains("review could not start: the run ended")),
        "{seen:?}"
    );
    assert_eq!(fake.log.lock().unwrap().last().unwrap(), "batch");
}

// W5: a staged set that cannot be read is never taken for passive: a row
// says so, no review runs and the batch still reaches the user.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_unreadable_staged_set_is_said_and_never_passive() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Sent, Decision::Sent);
    let (mut scope, _rx) = worker_in_app(Locality::Cloud, &home, handle.clone());
    scope.families = Toolset::ReadOnly.families().with(Family::Write);
    scope.stage_key = Some(StageKey::Pane(PANE));
    scope.approver = user;
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    let rows = Arc::new(Mutex::new(Vec::new()));
    let kept = rows.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            kept.lock().unwrap().push(event);
        }
    });
    super::review::before_batch_with(&host, &scope, async { None }).await;
    tokio::task::yield_now().await;
    let seen = result_summaries(&rows);
    assert!(
        seen.iter()
            .any(|(status, text)| *status == ToolStatus::Error
                && text.contains("could not read the staged changes")),
        "{seen:?}"
    );
    assert!(handle.log.lock().unwrap().is_empty(), "no ask, no review");
}

// W5: a failed join is `None`, not an empty (passive) set.
#[tokio::test]
async fn a_failed_read_of_the_staged_set_is_none_not_empty() {
    let failed = tokio::task::spawn_blocking(|| -> Vec<Change> { panic!("boom") }).await;
    assert_eq!(super::review::joined(failed), None);
    assert_eq!(super::review::joined(Ok(Vec::new())), Some(Vec::new()));
}

// W1: a path staged while the review ran (a sibling worker, or the
// correction going beyond its blockers) was not reviewed: the run manager
// is told, with its tier.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_path_staged_during_the_review_is_reported_as_not_reviewed() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Sent, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    let other = writer(&host, Locality::Cloud, &home, handle.clone(), user);
    *handle.sibling.lock().unwrap() = Some((other, "Cargo.toml"));
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    assert!(!d.call(Call::ProposeBatch).await.is_error);
    let log = handle.log.lock().unwrap().clone();
    assert_eq!(
        log[..3],
        [
            "consent",
            "review Granted",
            "late Cargo.toml High reviewed=true"
        ],
        "{log:?}"
    );
    assert_eq!(log[3], "batch");
}

// W1: with no review at all (a passive set), a high path that got into the
// batch after the assessment is still reported, as not reviewed.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_high_path_staged_after_a_passive_assessment_is_reported() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = ByAsk::new(&handle, Decision::Sent, Decision::Sent);
    let d = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!d.call(write_call("new.rs", "fn x() {}")).await.is_error);
    assert!(!d.call(write_call("Cargo.lock", "x")).await.is_error);
    // The assessment saw only `new.rs`: `Cargo.lock` came after it.
    let (mut scope, _rx) = worker_in_app(Locality::Cloud, &home, handle.clone());
    scope.families = Toolset::ReadOnly.families().with(Family::Write);
    scope.stage_key = Some(StageKey::Pane(PANE));
    scope.approver = user;
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let seen = vec![Change {
        path: "new.rs".into(),
        size: Some(9),
        hash: "h".into(),
        applied: false,
    }];
    super::review::before_batch_with(&host, &scope, async { Some(seen) }).await;
    assert_eq!(
        *handle.log.lock().unwrap(),
        ["late Cargo.lock High reviewed=false"]
    );
}

// W6: a review worker reads a file that is only staged (not on disk yet)
// inside the run's scope with no prompt; a path outside the scope is refused,
// not asked, and the review is told it could not be read.
// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_review_worker_reads_staged_only_files_in_scope_and_never_asks() {
    let (_dir, home, host) = fixture();
    project(&home);
    let handle = Scoped::over(&home, &["~/dev/app"]);
    let user = Scripted::new(Decision::Sent);
    let w = writer(&host, Locality::Cloud, &home, handle.clone(), user.clone());
    assert!(!w.call(write_call("fresh.rs", "only staged")).await.is_error);
    let reader = Scripted::new(Decision::Denied("must not be asked"));
    let (mut scope, _rx) = worker_in_app(Locality::Cloud, &home, handle.clone());
    scope.families = Toolset::ReadOnly.families();
    scope.stage_key = Some(StageKey::Pane(PANE));
    scope.approver = reader.clone();
    let (tx, mut rx) = mpsc::channel(64);
    scope.events = tx;
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let lens = host.dispatch(scope);
    let seen = lens.call(read("fresh.rs")).await;
    assert!(seen.text.contains("only staged"), "{seen:?}");
    let outside = lens.call(read("../other/a.rs")).await;
    assert!(outside.is_error, "{outside:?}");
    assert!(reader.asked.lock().unwrap().is_empty(), "never asked");
    assert!(
        handle
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("unread")),
        "the review is told"
    );
}
