//! A worker's commands (agent-orchestrator "Worker commands [W6]",
//! command-runner "Worker commands run after the worker's batch is applied"):
//! after its batch is approved and applied, a `code` worker's `cargo test`
//! asks with the worker label in every mode, runs against the disk only after
//! `y`, and its outcome reaches the coordinator through `collect`. That no
//! approval mode answers the ask is the app's (`tool_approval`).
//!
//! `cargo` is a script and the sandbox helper a fake (as in the host's
//! `exec_tests`): no real build, no network.

use super::*;
use crate::core::ai::orchestra::commands::COMMANDS_HEAD;

/// A `cargo` that leaves a mark, reads the applied file from the disk and
/// fails one test.
const CARGO: &str = "#!/bin/sh\n\
    echo ran > ran.txt\n\
    cat a.txt\n\
    echo 'test parse ... FAILED'\n\
    exit 101\n";

/// A writing lab whose host runs commands through the fake toolchain. Its
/// HOME is under `target`: the command sandbox refuses a project in `/tmp`.
/// The hub's sockets live in a short temp folder (a socket path is short),
/// held by the lab in the slot of a link folder.
fn command_lab(script: Script) -> Lab {
    let hub = tempfile::tempdir().unwrap();
    let sockets = hub.path().to_path_buf();
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&target).unwrap();
    let home = tempfile::Builder::new()
        .prefix("lbx-orch-exec-")
        .tempdir_in(target)
        .unwrap();
    let via = home.path().canonicalize().unwrap();
    std::fs::create_dir_all(via.join("dev/app")).unwrap();
    lab_over(script, home, via, Some(hub), |m, home| {
        let mut host = ToolHost::new(Some(home.into()), None, None, Limits::default());
        let base = home.parent().unwrap().join(format!(
            "{}-tools",
            home.file_name().unwrap().to_string_lossy()
        ));
        crate::runtime::tools::host::fake_toolchain(
            Arc::get_mut(&mut host).unwrap(),
            &base,
            home,
            CARGO,
        );
        RunManager {
            scratch: |_| false,
            ..m.with_tools(host, vec![sockets])
        }
    })
}

/// The next ask that reaches the orchestrator turn.
async fn next_ask(lab: &mut Lab) -> (CallId, ApprovalAsk) {
    loop {
        let event = timeout(Duration::from_secs(5), lab.turn.1.recv()).await;
        if let StreamEvent::ToolCall {
            id,
            approval: Some(ask),
            ..
        } = event.unwrap().unwrap()
        {
            return (id, *ask);
        }
    }
}

async fn reply_line(client: &mut Client) -> String {
    let mut reply = String::new();
    let read = timeout(Duration::from_secs(10), client.0.read_line(&mut reply));
    read.await.unwrap().unwrap();
    reply
}

// Spec ("Command asks with the worker label", "Runs after the batch is
// applied", "Output reaches the coordinator"): task 16.4 and the end of 16.5.
#[tokio::test]
async fn a_workers_command_after_its_applied_batch_asks_runs_and_reaches_collect() {
    let mut lab = command_lab(Script::Held("tests fail"));
    let waits = Waits::new();
    let (run, handle) = lab.start_writing_run(&waits, TEN_MINUTES);
    handle.call(spawn(Task::Code, "write a and test")).await;
    let mut client = worker_client(&lab.seen).await;
    let worker = SessionKey::Worker(
        pane(),
        WorkerKey {
            run,
            worker: WorkerId(1),
        },
    );
    let write = tool_call(
        1,
        "write_file",
        json!({"path": "a.txt", "content": "one\n"}),
    );
    let staged = exchange(&mut client, write).await;
    assert!(reply_text(&staged).starts_with("staged a.txt"), "{staged}");
    let propose = format!("{}\n", tool_call(2, "propose_batch", json!({})));
    client.1.write_all(propose.as_bytes()).await.unwrap();
    let (id, ask) = next_ask(&mut lab).await;
    assert!(matches!(ask, ApprovalAsk::Batch(_)), "{ask:?}");
    assert!(waits.decide(worker, 7, id, Decision::Sent));
    reply_line(&mut client).await;
    assert!(lab.app().join("a.txt").exists(), "the batch was applied");

    let test = tool_call(
        3,
        "run_command",
        json!({"program": "cargo", "args": ["test"]}),
    );
    client
        .1
        .write_all(format!("{test}\n").as_bytes())
        .await
        .unwrap();
    let (id, ask) = next_ask(&mut lab).await;
    let ApprovalAsk::Exec {
        program,
        args,
        worker: label,
        ..
    } = &ask
    else {
        panic!("{ask:?}");
    };
    assert_eq!(
        (program.as_str(), &args[..]),
        ("cargo", &["test".to_owned()][..])
    );
    let key = WorkerKey {
        run,
        worker: WorkerId(1),
    };
    assert_eq!(label, &Some((key, "1/1 code codex>default".to_owned())));
    // No approval mode answers it: `app_tests::tool_approval::
    // no_approval_mode_answers_a_workers_command_ask`.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!lab.app().join("ran.txt").exists(), "nothing before the y");
    assert!(waits.decide(worker, 7, id, Decision::Sent));
    let reply = reply_line(&mut client).await;
    assert!(reply.contains("exit code 101"), "{reply}");
    assert!(reply.contains("one"), "it read the applied file: {reply}");
    assert!(lab.app().join("ran.txt").exists());

    lab.seen.release.notify_one();
    assert_eq!(handle.call(wait(1)).await, fine("1: done"));
    let collected = handle.call(collect(1)).await.text;
    let at = |needle: &str| {
        collected
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {collected}"))
    };
    // lazarobox's record first, outside the worker's wrapper; the command
    // line and its output inside it, after the ledger.
    assert!(collected.starts_with(COMMANDS_HEAD), "{collected}");
    assert!(at("- 1 cargo: exit code 101 after ") < at("<untrusted-tool-output"));
    assert!(at("<untrusted-tool-output") < at("files changed:"));
    assert!(at("files changed:") < at("commands run:"));
    assert!(at("commands run:") < at("- 1 cargo test:"));
    assert!(at("- 1 cargo test:") < at("test parse ... FAILED"));
    assert!(at("test parse ... FAILED") < at("tests fail"));
    assert!(at("tests fail") < at("</untrusted-tool-output"));
}

// Spec ("Before the batch is applied"): with the batch staged and not yet
// proposed the worker's command is refused `propose them first`, no prompt
// opens, the pane is told, and nothing reaches `collect`.
#[tokio::test]
async fn a_workers_command_before_its_batch_is_proposed_is_refused_and_told() {
    let mut lab = command_lab(Script::Held("staged"));
    let waits = Waits::new();
    let handle = lab.start_writing(&waits, TEN_MINUTES);
    handle.call(spawn(Task::Code, "write a and test")).await;
    let mut client = worker_client(&lab.seen).await;
    let write = tool_call(
        1,
        "write_file",
        json!({"path": "a.txt", "content": "one\n"}),
    );
    exchange(&mut client, write).await;
    let test = tool_call(
        2,
        "run_command",
        json!({"program": "cargo", "args": ["test"]}),
    );
    let reply = exchange(&mut client, test).await;
    assert!(reply_text(&reply).contains("propose them first"), "{reply}");
    let mut told = None;
    while let Ok(event) = lab.turn.1.try_recv() {
        match event {
            StreamEvent::ToolCall {
                approval: Some(ask),
                ..
            } => panic!("no prompt: {ask:?}"),
            StreamEvent::Notice(line) => told = Some(line),
            _ => {}
        }
    }
    let told = told.expect("a notice in the pane");
    assert!(told.contains("worker 1/1 code codex>default"), "{told}");
    assert!(!lab.app().join("ran.txt").exists());
    lab.seen.release.notify_one();
    handle.call(wait(1)).await;
    let collected = handle.call(collect(1)).await.text;
    assert!(!collected.contains("commands run"), "{collected}");
}

/// Calls `cargo test` from the worker and returns its reply, failing if an
/// ask opens instead: a refused command opens none.
async fn refused_command(lab: &mut Lab, client: &mut Client, id: u32) -> String {
    let test = tool_call(
        id,
        "run_command",
        json!({"program": "cargo", "args": ["test"]}),
    );
    client
        .1
        .write_all(format!("{test}\n").as_bytes())
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_secs(3), next_ask(lab))
            .await
            .is_err(),
        "an ask opened"
    );
    reply_line(client).await
}

// Spec ("Worker commands [W6]": a command runs only after the worker's own
// batch was approved and applied): a worker with no batch at all is refused
// `propose them first` with no ask.
#[tokio::test]
async fn a_workers_command_with_no_batch_is_refused_without_an_ask() {
    let mut lab = command_lab(Script::Held("x"));
    let handle = lab.start_writing(&Waits::new(), TEN_MINUTES);
    handle.call(spawn(Task::Code, "test")).await;
    let mut client = worker_client(&lab.seen).await;
    let reply = refused_command(&mut lab, &mut client, 3).await;
    assert!(reply.contains("propose them first"), "{reply}");
    assert!(!lab.app().join("ran.txt").exists());
}

// ... and one whose batch the user denied (discarded) is refused the same way.
#[tokio::test]
async fn a_workers_command_after_its_batch_was_denied_is_refused_without_an_ask() {
    let mut lab = command_lab(Script::Held("x"));
    let waits = Waits::new();
    let (run, handle) = lab.start_writing_run(&waits, TEN_MINUTES);
    handle.call(spawn(Task::Code, "write a and test")).await;
    let mut client = worker_client(&lab.seen).await;
    let write = tool_call(
        1,
        "write_file",
        json!({"path": "a.txt", "content": "one\n"}),
    );
    exchange(&mut client, write).await;
    let propose = format!("{}\n", tool_call(2, "propose_batch", json!({})));
    client.1.write_all(propose.as_bytes()).await.unwrap();
    let (id, _) = next_ask(&mut lab).await;
    let worker = SessionKey::Worker(
        pane(),
        WorkerKey {
            run,
            worker: WorkerId(1),
        },
    );
    assert!(waits.decide(worker, 7, id, Decision::Denied("no")));
    reply_line(&mut client).await;
    assert!(!lab.app().join("a.txt").exists(), "nothing was applied");
    let reply = refused_command(&mut lab, &mut client, 3).await;
    assert!(reply.contains("propose them first"), "{reply}");
    assert!(!lab.app().join("ran.txt").exists());
}
