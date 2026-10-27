//! The host in an orchestrated turn: the role toolset filter in `execute`
//! and the orchestration calls routed to the run manager.

use std::sync::Mutex;

use serde_json::json;

use super::{
    tests::{Scripted, events, fixture, pair, read, run_cmd, scope},
    *,
};
use crate::core::ai::{
    chat::approval::ApprovalAsk,
    config::Task,
    orchestra::{RunId, Toolset, tools::OrchCall},
    tools::paths::SecretRoots,
};
use crate::runtime::tools::{Orchestra, WorkerScope};

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
    scope.toolset = Toolset::ReadOnly.families();
    scope.read_scope = Some(WorkerScope {
        handle,
        run: RunId(1),
        label: "review claude>opus".into(),
    });
    (scope, rx)
}

/// An orchestrator turn: read tools, the orchestration tools and `handle`.
fn orchestrator(
    home: &Path,
    handle: Option<Arc<dyn Orchestra>>,
) -> (TurnScope, mpsc::Receiver<StreamEvent>) {
    let (mut scope, rx) = scope(Locality::Cloud, home);
    scope.toolset = Toolset::Orchestrator.families();
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
            assert_eq!(asked[0], ApprovalAsk::Read { path });
        }
    }
}
