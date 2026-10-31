//! A turn with tools on starts the user's MCP servers, tells the chat, and
//! gives the engine their tools; nothing starts with tools off. Real unix
//! sockets, real fake-server processes and the real clock, every wait bounded.

use std::{sync::Mutex, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::Notify,
    time::{sleep, timeout},
};

use super::{
    tests::{done, request},
    *,
};
use crate::{
    core::ai::{
        chat::engine::{Availability, OrchSend},
        chat::{
            approval::ApprovalAsk,
            engine::{Decision, Tools},
        },
        config::{McpServer, Profile, Task},
        jail::{JailOff, JailSupport},
        orchestra::{
            Toolset,
            run::{Roster, RunLimits},
        },
    },
    runtime::{
        mcp::external::fake::{Lab as Fake, lab as fake_lab},
        orchestra::RunManager,
        proc::process_state,
        tools::{Limits, ToolHost},
    },
};

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

struct Lab {
    dir: tempfile::TempDir,
    host: Arc<ToolHost>,
    fake: Fake,
}

fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let host = ToolHost::new(None, None, None, Limits::default());
    Lab {
        dir,
        host,
        fake: fake_lab(),
    }
}

fn open_chats(
    lab: &Lab,
    runner: Runner,
    servers: Vec<McpServer>,
) -> (Chats, mpsc::Receiver<Tagged>) {
    open_chats_with(lab, runner, servers, Arc::new(|_| true))
}

fn open_chats_with(
    lab: &Lab,
    runner: Runner,
    servers: Vec<McpServer>,
    wires: Wires,
) -> (Chats, mpsc::Receiver<Tagged>) {
    let (tx, rx) = mpsc::channel(APP_CHANNEL);
    let dirs = vec![lab.dir.path().join("run")];
    let chats = Chats::new(runner, tx)
        .with_tools(lab.host.clone(), dirs)
        .with_servers(servers, lab.fake.home.clone(), wires);
    (chats, rx)
}

fn turn_on(engine: &str) -> ChatRequest {
    let mut req = request("");
    req.engine = engine.into();
    req.tools = Tools::On {
        origin: None,
        external: Default::default(),
        mcp: None,
    };
    req
}

/// A runner that records the tools it was given, ends the turn, and says it ran.
fn recording(seen: &Arc<Mutex<Vec<Tools>>>) -> Runner {
    let seen = seen.clone();
    Arc::new(move |request, tx| {
        seen.lock().unwrap().push(request.tools);
        Box::pin(async move {
            let _ = tx.send(done()).await;
        })
    })
}

/// The lines of the notices of the turn, up to its end.
async fn notices(rx: &mut mpsc::Receiver<Tagged>) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let (_, _, event) = timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("an event within 10 s")
            .expect("the channel is open");
        match event {
            StreamEvent::Notice(line) => lines.push(line),
            StreamEvent::Done(_) | StreamEvent::Error(_) => return lines,
            _ => {}
        }
    }
}

fn external_names(tools: &Tools) -> Vec<String> {
    match tools {
        Tools::On { external, .. } => external.external().into_iter().map(|t| t.name).collect(),
        Tools::Off => Vec::new(),
    }
}

fn server_pid(lab: &Lab, name: &str) -> Option<libc::pid_t> {
    let path = format!("{}.pid", lab.fake.calls(name).display());
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

async fn gone(pid: libc::pid_t) -> bool {
    for _ in 0..100 {
        if matches!(process_state(pid), None | Some('Z')) {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn a_turn_with_tools_starts_the_servers_says_so_and_hands_their_tools_to_the_engine() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    assert_eq!(notices(&mut rx).await, ["mcp: started fs"]);
    assert_eq!(external_names(&seen.lock().unwrap()[0]), ["fs__search"]);
    chats.shutdown(Duration::from_secs(5)).await;
}

#[tokio::test]
async fn one_server_that_cannot_start_is_a_notice_and_the_others_and_the_turn_go_on() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut broken = lab.fake.server("broken", "x", "never");
    broken.command = "/nonexistent/lazarobox-fake".to_owned();
    let servers = vec![broken, lab.fake.server("fine", "ping", "pong")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    let lines = notices(&mut rx).await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], "mcp: started fine");
    assert!(
        lines[1].starts_with("mcp: broken unavailable: "),
        "{lines:?}"
    );
    assert_eq!(external_names(&seen.lock().unwrap()[0]), ["fine__ping"]);
    chats.shutdown(Duration::from_secs(5)).await;
}

#[tokio::test]
async fn tools_off_or_no_servers_start_nothing() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    let mut off = request("");
    off.tools = Tools::Off;
    chats.send(pane(1), 1, off);
    assert!(notices(&mut rx).await.is_empty());
    assert_eq!(server_pid(&lab, "fs"), None);
    assert_eq!(external_names(&seen.lock().unwrap()[0]), [] as [String; 0]);
    // Tools on but nothing configured: nothing to start either.
    let (mut none, mut rx) = open_chats(&lab, recording(&seen), Vec::new());
    none.send(pane(1), 1, turn_on("ollama"));
    assert!(notices(&mut rx).await.is_empty());
    assert_eq!(server_pid(&lab, "fs"), None);
}

#[tokio::test]
async fn a_second_turn_of_the_chat_reuses_the_servers_and_closing_the_chat_stops_them() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    assert_eq!(notices(&mut rx).await, ["mcp: started fs"]);
    let pid = server_pid(&lab, "fs").expect("started");
    chats.send(pane(1), 2, turn_on("ollama"));
    assert!(notices(&mut rx).await.is_empty(), "started once per chat");
    assert_eq!(server_pid(&lab, "fs"), Some(pid));
    assert_eq!(external_names(&seen.lock().unwrap()[1]), ["fs__search"]);
    chats.end_session(pane(1));
    assert!(gone(pid).await, "the server of a closed chat is stopped");
}

#[tokio::test]
async fn a_turn_without_tools_after_one_with_them_stops_the_servers() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    notices(&mut rx).await;
    let pid = server_pid(&lab, "fs").expect("started");
    chats.send(pane(1), 2, request(""));
    notices(&mut rx).await;
    assert!(gone(pid).await, "no open chat uses it any more");
}

#[tokio::test]
async fn a_background_run_leaves_the_servers_of_the_pane_running() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    notices(&mut rx).await;
    let pid = server_pid(&lab, "fs").expect("started");
    chats.send_background(pane(1), 2, request(""));
    notices(&mut rx).await;
    assert!(!gone(pid).await, "a compaction run is not a tool-less turn");
}

#[tokio::test]
async fn quitting_stops_every_server() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![
        lab.fake.server("fs", "search", "found"),
        lab.fake.server("docs", "look", "seen"),
    ];
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    notices(&mut rx).await;
    let pids = [
        server_pid(&lab, "fs").unwrap(),
        server_pid(&lab, "docs").unwrap(),
    ];
    chats.shutdown(Duration::from_secs(3)).await;
    for pid in pids {
        assert!(gone(pid).await, "{pid} still runs after the quit");
    }
}

// The CLI engines reach a user tool only through the session's socket: the
// shim lists it, a call asks `y`, and only then the server is contacted.
#[tokio::test]
async fn a_cli_engine_lists_the_user_tool_and_a_call_waits_for_y() {
    user_tool_listed_and_asked(false).await;
}

// Spec (agent-tools, Toolsets by session role; user decision 2026-10-29):
// an orchestrator turn offers the user's server tools beside its own, and a
// call asks `y` exactly as in a chat turn.
#[tokio::test]
async fn an_orchestrator_turn_lists_the_user_tool_and_a_call_waits_for_y() {
    user_tool_listed_and_asked(true).await;
}

/// A turn on `claude` with the fake `fs` server: its `search` tool is listed
/// (with `spawn_agent` in an orchestrator turn, never otherwise), a call asks
/// `y`, and the server is contacted only after it.
async fn user_tool_listed_and_asked(orchestrated: bool) {
    let lab = lab();
    let started = Arc::new(Notify::new());
    let runner: Runner = {
        let started = started.clone();
        Arc::new(move |_request, _tx| {
            let started = started.clone();
            Box::pin(async move {
                started.notify_one();
                std::future::pending::<()>().await;
            })
        })
    };
    let servers = vec![lab.fake.server("fs", "search", "found it")];
    let (mut chats, mut rx) = open_chats(&lab, runner.clone(), servers);
    if orchestrated {
        let (events, _) = mpsc::unbounded_channel();
        let manager = Arc::new(RunManager::new(runner, None, events));
        chats = chats.with_orchestra(manager);
        let mut request = turn_on("claude");
        request.families = Toolset::Orchestrator.families();
        let ready = Availability::Ready {
            engine: "claude".into(),
            model: Some("opus".into()),
        };
        let orch = OrchSend {
            roster: Roster::new(vec![(Task::Orchestrate, ready)]),
            profile: Profile::default(),
            deadline: Duration::from_secs(60),
            limits: RunLimits::default(),
        };
        chats.send_orchestrated(pane(1), 4, request, orch);
    } else {
        chats.send(pane(1), 4, turn_on("claude"));
    }
    timeout(Duration::from_secs(10), started.notified())
        .await
        .expect("the engine starts after the servers");
    let session = chats.session(pane(1)).expect("a session");
    let (reader, mut writer) = UnixStream::connect(&session.socket)
        .await
        .unwrap()
        .into_split();
    let mut reader = BufReader::new(reader);
    let mut send = async |value: Value| {
        writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    };
    send(json!({"lazarobox": 1, "token": session.token.reveal()})).await;
    send(json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18"}}))
    .await;
    send(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    send(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": {"name": "fs__search", "arguments": {"q": "cats"}}}))
    .await;
    let mut listed = false;
    let mut orchestration = None;
    let mut line = String::new();
    // The call is announced with its ask, and nothing reached the server.
    let (_, turn, id, ask) = loop {
        if let (pane_id, turn, StreamEvent::ToolCall { id, approval, .. }) =
            timeout(Duration::from_secs(10), rx.recv())
                .await
                .unwrap()
                .unwrap()
        {
            assert_eq!(pane_id, pane(1));
            break (pane_id, turn, id, approval.map(|ask| *ask));
        }
    };
    let Some(ApprovalAsk::External { server, tool, .. }) = ask else {
        panic!("expected a user tool ask, got {ask:?}");
    };
    assert_eq!((server.as_str(), tool.as_str()), ("fs", "search"));
    assert_eq!(
        lab.fake.received("fs"),
        "",
        "the server saw nothing before y"
    );
    assert!(chats.decide(pane(1), turn, id, Decision::Sent));
    while let Ok(Ok(n)) = timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        if n == 0 {
            break;
        }
        let reply: Value = serde_json::from_str(line.trim()).unwrap();
        line.clear();
        if reply["id"] == 1 {
            let names = reply["result"]["tools"].as_array().unwrap();
            listed = names.iter().any(|t| t["name"] == "fs__search");
            orchestration = Some(names.iter().any(|t| t["name"] == "spawn_agent"));
        }
        if reply["id"] == 5 {
            let text = reply["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("<untrusted-tool-output"), "{text}");
            assert!(text.contains("found it"), "{text}");
            break;
        }
    }
    assert!(listed, "tools/list did not offer fs__search");
    assert_eq!(orchestration, Some(orchestrated), "spawn_agent offered");
    assert!(lab.fake.received("fs").contains("\"q\":\"cats\""));
    chats.shutdown(Duration::from_secs(5)).await;
}

/// `server`, but it takes `secs` seconds to start (before its first answer).
fn slow_start(server: McpServer, secs: u32) -> McpServer {
    let mut args = vec![
        "-c".to_owned(),
        format!("sleep {secs}; exec \"$@\""),
        "sh".to_owned(),
        server.command.clone(),
    ];
    args.extend(server.args.clone());
    McpServer {
        command: "/bin/sh".to_owned(),
        args,
        ..server
    }
}

// W1: the chat closes while its servers are still starting. The set that
// comes up afterwards belongs to nobody and is stopped at once.
#[tokio::test]
async fn closing_the_chat_during_a_start_leaves_no_server_behind() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![slow_start(lab.fake.server("fs", "search", "found"), 1)];
    let (mut chats, _rx) = open_chats(&lab, recording(&seen), servers);
    chats.send(pane(1), 1, turn_on("ollama"));
    sleep(Duration::from_millis(300)).await;
    assert_eq!(server_pid(&lab, "fs"), None, "still starting");
    chats.end_session(pane(1));
    let mut pid = None;
    for _ in 0..100 {
        pid = server_pid(&lab, "fs");
        if pid.is_some() {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    let pid = pid.expect("the start finished");
    assert!(gone(pid).await, "the server of a closed chat outlived it");
    chats.shutdown(Duration::from_secs(3)).await;
}

// W2: a later turn whose active list is empty stops the old servers.
#[tokio::test]
async fn a_turn_whose_list_became_empty_stops_the_old_servers() {
    let lab = lab();
    let repo = lab.dir.path().canonicalize().unwrap().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let mut server = lab.fake.server("fs", "search", "found");
    server.project = Some(repo.clone());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (mut chats, mut rx) = open_chats(&lab, recording(&seen), vec![server]);
    let mut inside = turn_on("ollama");
    if let Tools::On { origin, .. } = &mut inside.tools {
        *origin = Some(repo);
    }
    chats.send(pane(1), 1, inside);
    assert_eq!(notices(&mut rx).await, ["mcp: started fs"]);
    let pid = server_pid(&lab, "fs").expect("started");
    // Origin unknown: no git root, so the project server is not active.
    chats.send(pane(1), 2, turn_on("ollama"));
    notices(&mut rx).await;
    assert!(gone(pid).await, "a stale set kept running");
    assert_eq!(external_names(&seen.lock().unwrap()[1]), [] as [String; 0]);
}

// W3: a CLI that will run bare drops the tools, so nothing is started for it.
#[tokio::test]
async fn an_engine_that_will_not_be_wired_starts_no_server() {
    let lab = lab();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let servers = vec![lab.fake.server("fs", "search", "found")];
    let only_ollama: Wires = Arc::new(|request| request.engine == "ollama");
    let (mut chats, mut rx) = open_chats_with(&lab, recording(&seen), servers, only_ollama);
    chats.send(pane(1), 1, turn_on("claude"));
    assert!(notices(&mut rx).await.is_empty());
    assert_eq!(server_pid(&lab, "fs"), None);
    chats.send(pane(2), 1, turn_on("ollama"));
    assert_eq!(notices(&mut rx).await, ["mcp: started fs"]);
    chats.shutdown(Duration::from_secs(3)).await;
}

fn jail(on: bool) -> JailSupport {
    if on {
        JailSupport::Available {
            backend: crate::core::ai::jail::Backend::Landlock,
            abi: 6,
        }
    } else {
        JailSupport::Unavailable(JailOff::NotProbed)
    }
}

// The real check: Ollama runs in-process; a CLI only when its launch is jailed
// and there is a shim to wire.
#[test]
fn tools_are_wired_for_ollama_and_for_a_jailed_cli_only() {
    let mut env = CliEnv::from_env(jail(true));
    env.shim = Some("/usr/bin/lazarobox-shell".into());
    assert!(wires_tools(&env, &turn_on("ollama")));
    // claude is exempt on macOS: bare there, so never wired.
    assert_eq!(
        wires_tools(&env, &turn_on("claude")),
        crate::core::ai::jail::Os::CURRENT != crate::core::ai::jail::Os::Macos
    );
    assert!(wires_tools(&env, &turn_on("codex")));
    assert!(wires_tools(&env, &turn_on("gemini")));
    let mut no_shim = env.clone();
    no_shim.shim = None;
    assert!(!wires_tools(&no_shim, &turn_on("claude")));
    let mut bare = env.clone();
    bare.jail = jail(false);
    assert!(!wires_tools(&bare, &turn_on("claude")));
    assert!(wires_tools(&bare, &turn_on("ollama")));
}
