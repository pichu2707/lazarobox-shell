//! The spawn path of the run manager, with a fake runner: how a worker ends,
//! the request it sends, its tool session, the run ids and the log.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    time::timeout,
};

use super::*;
use crate::{
    core::ai::{
        Detection, EngineState, Status,
        chat::{
            adapters::{Invocation, Job},
            engine::{Availability, Finish, availability_for},
        },
        config::{AiConfig, Assignment, DEFAULT_WORKER_TIMEOUT_SECS},
        jail::{Backend, JailOff, JailSupport},
        orchestra::Toolset,
        tools::registry::FamilySet,
    },
    runtime::tools::{Limits, Unwired},
};

const JAIL: JailSupport = JailSupport::Available {
    backend: Backend::Landlock,
    abi: 6,
};
const NO_JAIL: JailSupport = JailSupport::Unavailable(JailOff::NoBackend);

/// What the fake engine does with a turn.
#[derive(Clone, Copy)]
enum Script {
    /// Streams the text, then ends.
    Answer(&'static str),
    /// Streams the text, then fails with the message.
    Fail(&'static str, &'static str),
    /// Never ends, like a process that hangs.
    Hang,
}

/// What the fake engine saw.
#[derive(Default)]
struct Seen {
    requests: Mutex<Vec<ChatRequest>>,
    /// A hanging turn was dropped: its process group would be killed.
    dropped: AtomicBool,
}

impl Seen {
    fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// The first request, once the worker's turn has made it.
    async fn first(&self) -> ChatRequest {
        for _ in 0..500 {
            if let Some(first) = self.requests().into_iter().next() {
                return first;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no request");
    }

    fn engines(&self) -> Vec<String> {
        self.requests().into_iter().map(|r| r.engine).collect()
    }
}

struct OnDrop(Arc<Seen>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

fn fake(script: Script) -> (Runner, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let mine = seen.clone();
    let runner: Runner = Arc::new(move |request, tx| {
        mine.requests.lock().unwrap().push(request);
        let mine = mine.clone();
        Box::pin(async move {
            let (text, end) = match script {
                Script::Answer(text) => (text, StreamEvent::Done(Finish::Stop)),
                Script::Fail(text, error) => {
                    (text, StreamEvent::Error(ChatError::Failed(error.into())))
                }
                Script::Hang => {
                    let _dropped = OnDrop(mine);
                    return std::future::pending().await;
                }
            };
            let _ = tx.send(StreamEvent::Delta(text.into())).await;
            let _ = tx.send(end).await;
        })
    });
    (runner, seen)
}

fn found(id: &'static str, models: &[&str]) -> EngineState {
    EngineState {
        id,
        status: Status::Available,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        model_labels: Vec::new(),
        version: None,
        diagnostics: Default::default(),
        binary: None,
    }
}

/// The roster the app builds: every task through `availability_for`, over
/// these assignments and the engines detection found.
fn roster(assigned: &[(Task, &str, &str)], engines: Vec<EngineState>, jail: JailSupport) -> Roster {
    let mut cfg = AiConfig::default();
    for (task, engine, model) in assigned {
        let assignment = Assignment {
            engine: Some((*engine).to_owned()),
            model: Some((*model).to_owned()),
        };
        cfg.set(*task, assignment);
    }
    let detection = Detection {
        engines,
        ..Default::default()
    };
    let each = Task::ALL.map(|task| (task, availability_for(task, &cfg, Some(&detection), jail)));
    Roster::new(each.into())
}

/// `orchestrate` on claude, `review` on codex, `general` on claude.
fn codex_review() -> Roster {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Review, "codex", "default"),
        (Task::General, "claude", "sonnet"),
    ];
    roster(
        &assigned,
        vec![found("claude", &[]), found("codex", &[])],
        JAIL,
    )
}

struct Lab {
    manager: Arc<RunManager>,
    seen: Arc<Seen>,
    events: mpsc::UnboundedReceiver<(PaneId, OrchestraEvent)>,
    /// The orchestrator turn's channel: worker tool rows would land here.
    turn: (mpsc::Sender<StreamEvent>, mpsc::Receiver<StreamEvent>),
    home: tempfile::TempDir,
}

fn lab(script: Script) -> Lab {
    lab_with(script, |manager, _| manager)
}

fn lab_with(script: Script, build: impl FnOnce(RunManager, &Path) -> RunManager) -> Lab {
    let home = tempfile::tempdir().unwrap();
    let (runner, seen) = fake(script);
    let (tx, events) = mpsc::unbounded_channel();
    let manager = Arc::new(build(RunManager::new(runner, None, tx), home.path()));
    Lab {
        manager,
        seen,
        events,
        turn: mpsc::channel(TURN_CHANNEL),
        home,
    }
}

/// A manager whose workers get tools: a real host over the temp HOME and a
/// real hub in it.
fn tooled(manager: RunManager, home: &Path) -> RunManager {
    let host = ToolHost::new(Some(home.into()), None, None, Limits::default());
    manager.with_tools(host, vec![home.join("run")])
}

fn pane() -> PaneId {
    PaneId::for_test(1)
}

impl Lab {
    fn scope(&self, roster: Roster, roots: &[&str]) -> OrchScope {
        OrchScope {
            roster,
            origin: self.home.path().to_path_buf(),
            roots: roots.iter().map(|r| (*r).to_owned()).collect(),
            deadline: Duration::from_secs(u64::from(DEFAULT_WORKER_TIMEOUT_SECS)),
            profile: Profile::Programmer,
            events: self.turn.0.clone(),
            approver: Arc::new(Unwired),
            pending: Pending::new(),
        }
    }

    fn start(&self, roster: Roster) -> (RunId, OrchHandle) {
        self.manager.start(pane(), self.scope(roster, &[])).unwrap()
    }

    /// The next event, from pane 1.
    async fn next(&mut self) -> OrchestraEvent {
        let (from, event) = timeout(Duration::from_secs(5), self.events.recv())
            .await
            .expect("no event")
            .expect("channel open");
        assert_eq!(from, pane());
        event
    }

    /// The events already sent.
    fn sent(&mut self) -> Vec<OrchestraEvent> {
        let mut out = Vec::new();
        while let Ok((_, event)) = self.events.try_recv() {
            out.push(event);
        }
        out
    }
}

fn spawn(task: Task, prompt: &str) -> OrchCall {
    OrchCall::Spawn {
        task,
        prompt: prompt.into(),
    }
}

fn wait(id: u32) -> OrchCall {
    OrchCall::Wait {
        ids: vec![WorkerId(id)],
        timeout: 120,
    }
}

fn collect(id: u32) -> OrchCall {
    OrchCall::Collect { id: WorkerId(id) }
}

fn fine(text: &str) -> Outcome {
    Outcome {
        text: text.into(),
        is_error: false,
    }
}

fn status_of(event: &OrchestraEvent) -> Option<WorkerStatus> {
    match event {
        OrchestraEvent::Worker(row) => Some(row.status),
        OrchestraEvent::Halted { .. } => None,
    }
}

/// The nonce of a wrapped text.
fn nonce_of(text: &str) -> String {
    let start = text.find("id=\"").unwrap() + 4;
    text[start..start + 16].to_owned()
}

// Spec (agent-orchestrator, Review worker runs on the Review assignment): the
// fake runner gets a request for codex with model `default`, and the result
// is `done`. The request carries the review prompt and the worker prompt as
// its one message (design: the worker's system prompt is `load_prompt`).
#[tokio::test]
async fn a_review_worker_runs_on_the_review_assignment_and_ends_done() {
    let mut lab = lab(Script::Answer("looks fine"));
    let (run, handle) = lab.start(codex_review());
    let spawned = handle.call(spawn(Task::Review, "check the parser")).await;
    assert_eq!(spawned, fine("worker 1: running"));
    let waited = handle.call(wait(1)).await;
    assert_eq!((waited.text.as_str(), waited.is_error), ("1: done", false));
    let requests = lab.seen.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.engine, "codex");
    assert_eq!(request.model.as_deref(), Some("default"));
    let review = load_prompt(None, Profile::Programmer, Task::Review).text;
    assert_eq!(request.system, review);
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].content, "check the parser");
    let collected = handle.call(collect(1)).await;
    assert!(!collected.is_error);
    assert!(collected.text.contains("looks fine"), "{collected:?}");
    let row = |status| {
        OrchestraEvent::Worker(WorkerRow {
            run,
            id: WorkerId(1),
            kind: Task::Review,
            engine: "codex".into(),
            model: Some("default".into()),
            status,
            elapsed: None,
        })
    };
    assert_eq!(lab.next().await, row(WorkerStatus::Running));
    let done = lab.next().await;
    assert_eq!(status_of(&done), Some(WorkerStatus::Done));
}

// Spec (agent-orchestrator, No jail refuses): `review` on gemini without a
// jail ends `refused` with the no-jail reason; no process starts and no other
// engine is contacted.
#[tokio::test]
async fn without_a_jail_a_gemini_worker_is_refused_and_nothing_starts() {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Review, "gemini", "pro"),
        (Task::General, "claude", "sonnet"),
    ];
    let roster = roster(
        &assigned,
        vec![found("claude", &[]), found("gemini", &[])],
        NO_JAIL,
    );
    let reason = guidance(roster.get(Task::Review)).unwrap();
    assert!(reason.contains("not available on this system"), "{reason}");
    let mut lab = lab_with(Script::Answer("x"), tooled);
    let (run, handle) = lab.start(roster);
    let spawned = handle.call(spawn(Task::Review, "check")).await;
    assert_eq!(spawned, error("worker 1: refused"));
    assert!(lab.seen.requests().is_empty(), "no engine contacted");
    let events = lab.sent();
    assert_eq!(status_of(&events[0]), Some(WorkerStatus::Refused));
    let halted = OrchestraEvent::Halted {
        run,
        worker: WorkerId(1),
        class: ErrorClass::Other,
        text: reason,
    };
    assert_eq!(events[1..], [halted]);
    // No session was opened: the hub was never made.
    assert!(!lab.home.path().join("run").exists());
}

// Spec (agent-orchestrator, Model missing refuses): the reason names the
// model and `download required`.
#[tokio::test]
async fn a_worker_on_a_model_not_downloaded_is_refused_naming_it() {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::Code, "ollama", "llama3:70b"),
    ];
    let engines = vec![found("claude", &[]), found("ollama", &["llama3:8b"])];
    let mut lab = lab(Script::Answer("x"));
    let (_, handle) = lab.start(roster(&assigned, engines, JAIL));
    let spawned = handle.call(spawn(Task::Code, "write it")).await;
    assert_eq!(spawned, error("worker 1: refused"));
    assert!(lab.seen.requests().is_empty());
    let events = lab.sent();
    let Some(OrchestraEvent::Halted { text, .. }) = events.last() else {
        panic!("{events:?}");
    };
    assert!(
        text.contains("llama3:70b") && text.contains("download required"),
        "{text}"
    );
}

// Spec (agent-orchestrator, Deadline kills the worker): with the default
// deadline and a process that never ends, after 10 minutes the process group
// is killed (the adapter future is dropped) and the status is `timed_out`.
#[tokio::test(start_paused = true)]
async fn the_deadline_kills_a_worker_that_never_ends_and_times_it_out() {
    let mut lab = lab(Script::Hang);
    let (run, handle) = lab.start(codex_review());
    let started = Instant::now();
    handle.call(spawn(Task::Review, "check")).await;
    assert_eq!(status_of(&lab.next().await), Some(WorkerStatus::Running));
    let (_, ended) = lab.events.recv().await.unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(600));
    assert!(
        lab.seen.dropped.load(Ordering::SeqCst),
        "the process is killed"
    );
    let OrchestraEvent::Worker(row) = ended else {
        panic!("{ended:?}");
    };
    assert_eq!(row.status, WorkerStatus::TimedOut);
    assert_eq!(row.elapsed, Some(Duration::from_secs(600)));
    let (_, halted) = lab.events.recv().await.unwrap();
    let OrchestraEvent::Halted { run: of, class, .. } = halted else {
        panic!("{halted:?}");
    };
    assert_eq!((of, class), (run, ErrorClass::Timeout));
}

// Spec (agent-orchestrator, Collect before done): an error result says it is
// not finished and no text is returned.
#[tokio::test]
async fn collecting_a_running_worker_says_it_is_not_finished() {
    let lab = lab(Script::Hang);
    let (_, handle) = lab.start(codex_review());
    handle.call(spawn(Task::Review, "check")).await;
    let collected = handle.call(collect(1)).await;
    assert_eq!(collected, error("worker 1 is not finished"));
}

// Spec (agent-orchestrator, No silent fallback; Error shown as it came;
// Orchestrator paused): a failing codex worker contacts no other engine, the
// alert shows the provider message, and no other worker starts.
#[tokio::test]
async fn a_failing_worker_contacts_no_other_engine_and_starts_nothing_more() {
    let mut lab = lab(Script::Fail("", "409 conflict: session busy"));
    let (_, handle) = lab.start(codex_review());
    handle.call(spawn(Task::Review, "check")).await;
    let waited = handle.call(wait(1)).await;
    assert_eq!(waited.text, "1: awaiting_user");
    let again = handle.call(spawn(Task::General, "other")).await;
    assert_eq!(again, error("run paused"));
    assert_eq!(lab.seen.engines(), ["codex"]);
    let events = lab.sent();
    let statuses: Vec<_> = events.iter().filter_map(status_of).collect();
    assert_eq!(statuses, [WorkerStatus::Running, WorkerStatus::Failed]);
    let Some(OrchestraEvent::Halted { class, text, .. }) = events.last() else {
        panic!("{events:?}");
    };
    assert_eq!(
        (*class, text.as_str()),
        (ErrorClass::Other, "409 conflict: session busy")
    );
}

// Spec (chat-engines, Same jail as chat): a worker's command on codex equals
// the chat command for codex apart from prompt and folder. The worker turn
// goes through the chat's runner, so the command is what the adapter builds
// from the worker's request.
#[tokio::test]
async fn a_codex_worker_command_equals_the_chat_command_apart_from_the_prompt() {
    let lab = lab(Script::Answer("x"));
    let (_, handle) = lab.start(codex_review());
    handle.call(spawn(Task::Review, "check the parser")).await;
    handle.call(wait(1)).await;
    let worker = lab.seen.requests().remove(0);
    let chat = ChatRequest {
        engine: "codex".into(),
        model: Some("default".into()),
        system: load_prompt(None, Profile::Programmer, Task::Chat).text,
        memory: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: "hello".into(),
        }],
        tools: tools_for(
            "codex",
            true,
            Some(PathBuf::from("/elsewhere")),
            MCP_VERIFIED,
        ),
        families: FamilySet::LEGACY,
    };
    let job = |request: &ChatRequest| -> Job {
        match adapter_for("codex").unwrap().build(request) {
            Invocation::Process(job) => job,
            other => panic!("{other:?}"),
        }
    };
    let (worker, chat) = (job(&worker), job(&chat));
    assert_ne!(worker.stdin, chat.stdin, "the prompts differ");
    let same = |job: Job| Job {
        stdin: Vec::new(),
        ..job
    };
    assert_eq!(same(worker), same(chat));
}

// Spec (tasks.md 1d.3a, no tool leak; S1c-1 obligation): a worker session is
// opened with its toolset's families, never LEGACY: it lists the read tools
// only (no `run_in_pane`, no orchestration tool) and refuses a forged
// `spawn_agent`. `Family::Pane` is in no worker's set.
#[tokio::test]
async fn a_worker_session_lists_only_the_read_tools_and_refuses_the_rest() {
    let lab = lab_with(Script::Hang, tooled);
    let (run, handle) = lab.start(codex_review());
    handle.call(spawn(Task::Review, "check")).await;
    let request = lab.seen.first().await;
    let families = Toolset::ReadOnly.families();
    assert_eq!(request.families, families);
    assert!(!families.contains(Family::Pane));
    let Tools::On {
        mcp: Some(wire), ..
    } = &request.tools
    else {
        panic!("{:?}", request.tools);
    };
    let tail = format!("-w{}-1.sock", run.0);
    assert!(wire.socket.to_str().unwrap().ends_with(&tail), "{wire:?}");
    let mut client = ready(wire).await;
    let listed = exchange(
        &mut client,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["find_dir", "list_dir", "read_file"]);
    for name in ["spawn_agent", "run_in_pane"] {
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": name, "arguments": {"kind": "review", "prompt": "x", "command": "ls"}}});
        let refused = exchange(&mut client, call).await;
        assert_eq!(
            refused["result"]["content"][0]["text"], "unknown tool",
            "{name}"
        );
    }
}

type Client = (BufReader<OwnedReadHalf>, OwnedWriteHalf);

/// Connects to a session as a CLI's shim does: the token, then `initialize`.
async fn ready(wire: &McpWire) -> Client {
    let (r, w) = UnixStream::connect(&wire.socket)
        .await
        .unwrap()
        .into_split();
    let mut client = (BufReader::new(r), w);
    let hello = json!({"lazarobox": 1, "token": wire.token.reveal()});
    assert_eq!(exchange(&mut client, hello).await, json!({"ok": true}));
    let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18"}});
    assert!(exchange(&mut client, init).await["result"].is_object());
    client
}

async fn exchange(client: &mut Client, line: Value) -> Value {
    client
        .1
        .write_all(format!("{line}\n").as_bytes())
        .await
        .unwrap();
    let mut reply = String::new();
    let read = timeout(Duration::from_secs(5), client.0.read_line(&mut reply));
    read.await.unwrap().unwrap();
    serde_json::from_str(&reply).unwrap()
}

// Spec (agent-orchestrator, Log hygiene): with a failing worker the log holds
// the class and status and none of the prompt, output or error text; nor do
// the results the orchestrator gets.
#[tokio::test]
async fn the_log_holds_status_and_class_but_no_content() {
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let log: Log = Arc::new(move |line| sink.lock().unwrap().push(line.to_string()));
    let lab = lab_with(
        Script::Fail("SECRET-OUTPUT", "SECRET-ERROR at /home/me"),
        |m, _| m.with_log(log),
    );
    let (run, handle) = lab.start(codex_review());
    let mut results = vec![handle.call(spawn(Task::Review, "SECRET-PROMPT")).await];
    results.push(handle.call(wait(1)).await);
    results.push(handle.call(collect(1)).await);
    let lines = lines.lock().unwrap().clone();
    let id = run.0;
    assert_eq!(
        lines,
        [
            format!("run {id} worker 1 review running"),
            format!("run {id} worker 1 review failed other"),
        ]
    );
    let said: Vec<_> = results.iter().map(|o| o.text.as_str()).collect();
    for text in lines.iter().map(String::as_str).chain(said) {
        assert!(
            !text.contains("SECRET") && !text.contains("/home"),
            "{text}"
        );
    }
}

// Spec (agent-orchestrator, Unusable orchestrator engine), as far as the run
// manager goes: with `orchestrate` on a codex that is not installed, `start`
// starts nothing, answers the engine guidance and contacts no engine, the
// `chat` one included.
#[tokio::test]
async fn an_unusable_orchestrate_engine_starts_no_run() {
    let assigned = [
        (Task::Orchestrate, "codex", "default"),
        (Task::Chat, "ollama", "llama3:8b"),
    ];
    let mut codex = found("codex", &[]);
    codex.status = Status::NotInstalled;
    let roster = roster(
        &assigned,
        vec![codex, found("ollama", &["llama3:8b"])],
        JAIL,
    );
    let Availability::Undetected { status, .. } = roster.get(Task::Orchestrate) else {
        panic!("{:?}", roster.get(Task::Orchestrate));
    };
    assert_eq!(status, "not installed");
    let expected = guidance(roster.get(Task::Orchestrate)).unwrap();
    let mut lab = lab(Script::Answer("x"));
    let started = lab.manager.start(pane(), lab.scope(roster, &[]));
    assert_eq!(started.err(), Some(expected));
    assert!(lab.seen.requests().is_empty());
    assert!(lab.sent().is_empty());
    assert!(lock(&lab.manager.runs).is_empty());
}

// S1b-2 security obligation: the nonce of each `collect` is drawn anew from
// the OS CSPRNG, never reused, and never shown to the worker.
#[tokio::test]
async fn every_collect_wraps_under_a_fresh_nonce_the_worker_never_saw() {
    let lab = lab(Script::Answer("the answer"));
    let (_, handle) = lab.start(codex_review());
    handle.call(spawn(Task::Review, "check")).await;
    handle.call(wait(1)).await;
    let first = nonce_of(&handle.call(collect(1)).await.text);
    let second = nonce_of(&handle.call(collect(1)).await.text);
    assert_ne!(first, second);
    for nonce in [&first, &second] {
        assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()), "{nonce}");
        for request in lab.seen.requests() {
            assert!(!request.system.contains(nonce.as_str()));
            assert!(
                request
                    .messages
                    .iter()
                    .all(|m| !m.content.contains(nonce.as_str()))
            );
        }
    }
}

// S1b-2 security obligation: run ids grow and are never reused, not by a
// later process either (it starts from a later clock), even if the clock
// stalls or goes back.
#[test]
fn run_ids_grow_with_the_clock_and_never_repeat() {
    let mut ids = RunIds::default();
    assert_eq!(ids.next(1_000), RunId(1_000));
    assert_eq!(ids.next(1_000), RunId(1_001), "the clock stalled");
    assert_eq!(ids.next(900), RunId(1_002), "the clock went back");
    assert_eq!(ids.next(5_000), RunId(5_000));
    let mut later = RunIds::default();
    let last = ids.next(0);
    assert_eq!(last, RunId(5_001));
    assert!(later.next(clock_micros()).0 > last.0);
}

// S1b-2 security obligation: a run's read scope lives with its run only. A
// later run, or a run of an earlier process resumed, has no scope until the
// user answers again: its reads ask.
#[tokio::test]
async fn a_scope_belongs_to_its_run_and_a_new_run_asks_again() {
    let lab = lab_with(Script::Answer("x"), |m, home| RunManager {
        scratch: |_| false,
        ..tooled(m, home)
    });
    let home = lab.home.path();
    std::fs::create_dir_all(home.join("dev/app")).unwrap();
    std::fs::write(home.join("dev/app/a.rs"), "fn main() {}").unwrap();
    let secrets = SecretRoots::load(home, &[]).unwrap();
    let scope = lab.scope(codex_review(), &["~/dev/app"]);
    let (first, old) = lab.manager.start(pane(), scope).unwrap();
    let allowed = old.allows(first, &secrets, home, "~/dev/app/a.rs");
    assert_eq!(
        allowed,
        Some(home.canonicalize().unwrap().join("dev/app/a.rs"))
    );
    let (second, new) = lab
        .manager
        .start(pane(), lab.scope(codex_review(), &[]))
        .unwrap();
    assert!(second.0 > first.0);
    for (handle, run) in [(&old, first), (&new, first), (&new, second)] {
        assert_eq!(handle.allows(run, &secrets, home, "~/dev/app/a.rs"), None);
    }
    // A process that starts later knows no run of this one.
    let (runner, _) = fake(Script::Answer("x"));
    let restarted = Arc::new(tooled(
        RunManager::new(runner, None, mpsc::unbounded_channel().0),
        home,
    ));
    let (third, fresh) = restarted
        .start(pane(), lab.scope(codex_review(), &[]))
        .unwrap();
    assert!(third.0 > second.0);
    assert_eq!(fresh.allows(first, &secrets, home, "~/dev/app/a.rs"), None);
}
