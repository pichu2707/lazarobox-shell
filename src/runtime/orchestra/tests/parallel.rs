//! Parallel writers (agent-orchestrator [W10b]): one batch per writing worker,
//! reviews that read their proposer's batch, and the notice for workers that
//! share a local engine (owner decision 5, design D11).

use super::*;

// The engine notice -------------------------------------------------------

/// `general` and `review` assigned to `engine>model` pairs, with `ollama`
/// and `llamacpp` found.
fn assigned(general: (&str, &str), review: (&str, &str)) -> Roster {
    let assigned = [
        (Task::Orchestrate, "claude", "opus"),
        (Task::General, general.0, general.1),
        (Task::Review, review.0, review.1),
    ];
    let engines = vec![
        found("claude", &[]),
        found("codex", &[]),
        found("ollama", &["qwen3:8b", "llama3:8b", "gpt-oss:120b-cloud"]),
        found("llamacpp", &["x"]),
    ];
    roster(&assigned, engines, JAIL)
}

fn local_pair() -> Roster {
    assigned(("ollama", "qwen3:8b"), ("ollama", "llama3:8b"))
}

fn with_parallel(lab: &Lab, roster: Roster, parallel: usize) -> (RunId, OrchHandle) {
    let limits = RunLimits {
        parallel,
        ..RunLimits::default()
    };
    let scope = OrchScope {
        limits,
        ..lab.scope(roster, &[])
    };
    lab.manager.start(pane(), scope).unwrap()
}

/// The texts of the notices the pane got.
fn notices(lab: &mut Lab) -> Vec<String> {
    let mut out = Vec::new();
    for event in lab.sent() {
        if let OrchestraEvent::Notice { text } = event {
            out.push(text);
        }
    }
    out
}

const SHARED: &str = "workers 1 and 2 share ollama, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time";

// Decision 5: two workers in flight on one local server are told once, with
// the one key that fixes it; a third on the same engine says nothing more.
#[tokio::test]
async fn two_workers_on_a_local_engine_get_one_notice() {
    let mut lab = lab(Script::Hang);
    let (_, handle) = with_parallel(&lab, local_pair(), 3);
    handle.call(spawn(Task::General, "a")).await;
    assert!(notices(&mut lab).is_empty(), "alone on the server");
    handle.call(spawn(Task::Review, "b")).await;
    handle.call(spawn(Task::General, "c")).await;
    lab.seen.nth(2).await;
    let told = notices(&mut lab);
    assert_eq!(told, [SHARED]);
    assert!(told[0].is_ascii());
}

// Triangulation: no notice when the workers do not meet on one local server.
#[tokio::test]
async fn workers_that_do_not_meet_on_a_local_server_get_no_notice() {
    let cases = [
        // one at a time: the second waits in the queue
        ("sequential", local_pair(), 1),
        // two local servers
        (
            "two engines",
            assigned(("ollama", "qwen3:8b"), ("llamacpp", "x")),
            2,
        ),
        // a provider's cloud
        (
            "a cloud engine",
            assigned(("codex", "default"), ("codex", "default")),
            2,
        ),
        // Ollama serving a cloud model
        (
            "a cloud model",
            assigned(("ollama", "qwen3:8b"), ("ollama", "gpt-oss:120b-cloud")),
            2,
        ),
    ];
    for (name, roster, parallel) in cases {
        let mut lab = lab(Script::Hang);
        let (_, handle) = with_parallel(&lab, roster, parallel);
        handle.call(spawn(Task::General, "a")).await;
        handle.call(spawn(Task::Review, "b")).await;
        lab.seen.nth(parallel.min(2) - 1).await;
        assert!(notices(&mut lab).is_empty(), "{name}");
    }
}

// A second run is a new condition: it tells again.
#[tokio::test]
async fn a_new_run_tells_again() {
    let mut lab = lab(Script::Hang);
    let (_, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(notices(&mut lab), [SHARED]);
    lab.manager.cancel(pane());
    lab.sent();
    let (_, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(notices(&mut lab), [SHARED]);
}

/// `ollama` for `general`, `codex` for `review`: the answer of `codex` is
/// `done`, anything else hangs.
fn codex_answers(request: &ChatRequest) -> Option<&'static str> {
    (request.engine == "codex").then_some("done")
}

// Queue promote reaches the notice: the third worker waits, a slot frees, and
// it starts next to the first, on the same server.
#[tokio::test]
async fn a_worker_promoted_from_the_queue_next_to_a_local_one_is_told() {
    let mut lab = lab(Script::By(codex_answers));
    let roster = assigned(("ollama", "qwen3:8b"), ("codex", "default"));
    let (_, handle) = with_parallel(&lab, roster, 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    let third = handle.call(spawn(Task::General, "c")).await;
    assert_eq!(third, fine("worker 3: queued"));
    assert!(notices(&mut lab).is_empty(), "nobody shares yet");
    assert_eq!(handle.call(wait(2)).await, fine("2: done"));
    lab.seen.nth(2).await;
    assert_eq!(
        notices(&mut lab),
        [
            "workers 1 and 3 share ollama, a local server: it may queue their requests; set max_parallel = 1 in [ai.orchestrator] to run one at a time"
        ]
    );
}

// Relaunch reaches it: a worker that failed on a cloud engine is relaunched
// on the local server where another one runs.
#[tokio::test]
async fn a_worker_relaunched_onto_a_local_engine_is_told() {
    let mut lab = lab(Script::FailOn(2, "409 conflict: session busy"));
    let roster = assigned(("ollama", "qwen3:8b"), ("codex", "default"));
    let (run, handle) = with_parallel(&lab, roster, 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(handle.call(wait(2)).await, fine("2: awaiting_user"));
    assert!(notices(&mut lab).is_empty());
    let onto = UserChoice::Relaunch(local_pair());
    let decided = lab.manager.decide(pane(), run, onto, FamilySet::ALL);
    assert_eq!(decided, Ok(()));
    lab.seen.nth(2).await;
    assert_eq!(notices(&mut lab), [SHARED]);
}

// GUARD: a retry on the engine already told does not tell again (the memory
// is per run and engine, not per launch).
#[tokio::test]
async fn a_retry_on_the_engine_already_told_says_nothing_more() {
    let mut lab = lab(Script::FailOn(2, "409 conflict: session busy"));
    let (run, handle) = with_parallel(&lab, local_pair(), 2);
    handle.call(spawn(Task::General, "a")).await;
    handle.call(spawn(Task::Review, "b")).await;
    assert_eq!(handle.call(wait(2)).await, fine("2: awaiting_user"));
    assert_eq!(notices(&mut lab), [SHARED]);
    let retried = lab
        .manager
        .decide(pane(), run, UserChoice::Retry, FamilySet::ALL);
    assert_eq!(retried, Ok(()));
    lab.seen.nth(2).await;
    assert!(notices(&mut lab).is_empty(), "told once");
}

// Writers on their own keys -----------------------------------------------
//
// These drive the writing lab (real socket I/O, so real deadlines and never a
// paused clock) and need the write primitives: Linux only, like the helpers
// they carry.

#[cfg(target_os = "linux")]
mod writers {
    use super::super::review::Role;
    use super::*;
    use crate::core::ai::chat::review::Badge;
    use crate::core::ai::tools::stage::StageKey;

    fn key_of(run: RunId, worker: u32) -> StageKey {
        StageKey::Worker(
            pane(),
            WorkerKey {
                run,
                worker: WorkerId(worker),
            },
        )
    }

    fn session_of(run: RunId, worker: u32) -> SessionKey {
        SessionKey::Worker(
            pane(),
            WorkerKey {
                run,
                worker: WorkerId(worker),
            },
        )
    }

    /// A run of `parallel` slots whose `code` workers write in the lab's app.
    fn start(lab: &Lab, waits: &Arc<Waits>, parallel: usize) -> (RunId, OrchHandle) {
        let pending = Pending::new();
        let limits = RunLimits {
            parallel,
            ..RunLimits::default()
        };
        let scope = OrchScope {
            origin: lab.app(),
            approver: waits.approver(pane(), 7, pending.clone()),
            pending,
            deadline: TEN_MINUTES,
            limits,
            ..lab.scope(codex_both(), &["~/dev/app"])
        };
        lab.manager.start(pane(), scope).unwrap()
    }

    /// The socket of request `n` (from 0), connected as its CLI would be.
    async fn client_of(seen: &Seen, n: usize) -> Client {
        let request = seen.nth(n).await;
        let Tools::On {
            mcp: Some(wire), ..
        } = &request.tools
        else {
            panic!("{:?}", request.tools);
        };
        ready(wire).await
    }

    async fn stage(client: &mut Client, id: u32, path: &str, text: &str) {
        let call = tool_call(id, "write_file", json!({"path": path, "content": text}));
        let reply = exchange(client, call).await;
        assert!(reply_text(&reply).starts_with("staged"), "{reply}");
    }

    async fn propose(client: &mut Client, id: u32) {
        let line = format!("{}\n", tool_call(id, "propose_batch", json!({})));
        client.1.write_all(line.as_bytes()).await.unwrap();
    }

    /// Whether `client` reads `path` through the staged view.
    async fn sees(client: &mut Client, id: u32, path: &str) -> bool {
        let reply = exchange(client, tool_call(id, "read_file", json!({"path": path}))).await;
        reply["result"]["isError"] != json!(true)
    }

    /// The next ask the lab's orchestrator turn got.
    async fn next_ask(lab: &mut Lab) -> (CallId, ApprovalAsk) {
        loop {
            let event = timeout(Duration::from_secs(5), lab.turn.1.recv()).await;
            if let StreamEvent::ToolCall {
                id,
                approval: Some(ask),
                ..
            } = event.expect("an ask").unwrap()
            {
                return (id, *ask);
            }
        }
    }

    /// The next batch review: its call, its label and how many files it lists.
    async fn next_batch(lab: &mut Lab) -> (CallId, String, usize) {
        let (id, ask) = next_ask(lab).await;
        let ApprovalAsk::Batch(review) = ask else {
            panic!("{ask:?}");
        };
        let label = review.label().unwrap_or_default().to_owned();
        (id, label, review.doc().files())
    }

    async fn paths(lab: &Lab, key: StageKey) -> Vec<String> {
        let origin = lab.app();
        let kept = lab.host().staged_changes(key, &origin).await;
        kept.unwrap().into_iter().map(|c| c.path).collect()
    }

    fn applied(lab: &Lab, worker: u32) -> Vec<(String, bool)> {
        let runs = lock(&lab.manager.runs);
        let ledger = runs[&pane()].run.ledger(WorkerId(worker)).unwrap();
        (ledger.changes().iter())
            .map(|c| (c.path.clone(), c.applied))
            .collect()
    }

    // Spec (agent-orchestrator, Two writers, two batches [W10, auto]; code-
    // write, One proposal does not seal another's files): worker 1 proposes
    // and the ask lists its file only; worker 2 keeps staging, proposes its
    // own, and approving 1 writes 1's file and flips 1's ledger alone.
    #[tokio::test]
    async fn two_writers_seal_and_apply_only_their_own_files() {
        let mut lab = writing_lab(Script::Held("done"));
        let waits = Waits::new();
        let (run, handle) = start(&lab, &waits, 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        propose(&mut one, 2).await;
        let (first, label, files) = next_batch(&mut lab).await;
        assert_eq!(label, "1/2 code codex>default");
        assert_eq!(files, 1, "only worker 1's file");
        // Worker 2 is not sealed by 1's proposal: it stages again and proposes.
        stage(&mut two, 2, "c.txt", "three\n").await;
        propose(&mut two, 3).await;
        let (second, label, files) = next_batch(&mut lab).await;
        assert_eq!(label, "2/2 code codex>default");
        assert_eq!(files, 2, "b.txt and c.txt");
        assert!(waits.decide(session_of(run, 1), 7, first, Decision::Sent));
        let mut reply = String::new();
        let read = timeout(Duration::from_secs(5), one.0.read_line(&mut reply));
        read.await.unwrap().unwrap();
        assert!(lab.app().join("a.txt").exists(), "{reply}");
        assert!(!lab.app().join("b.txt").exists());
        assert!(!lab.app().join("c.txt").exists());
        assert_eq!(applied(&lab, 1), [("a.txt".to_owned(), true)]);
        assert_eq!(
            applied(&lab, 2),
            [("b.txt".to_owned(), false), ("c.txt".to_owned(), false)]
        );
        assert!(waits.decide(session_of(run, 2), 7, second, Decision::Denied("no")));
    }

    // Decision 9 (D9): two pending proposals are two labelled reviews, in
    // arrival order, and answering the first does not touch the second.
    #[tokio::test]
    async fn two_proposals_are_two_labelled_reviews_in_arrival_order() {
        let mut lab = writing_lab(Script::Held("done"));
        let waits = Waits::new();
        let (run, handle) = start(&lab, &waits, 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        // Worker 2 proposes first.
        propose(&mut two, 2).await;
        let (early, label, files) = next_batch(&mut lab).await;
        assert_eq!((label.as_str(), files), ("2/2 code codex>default", 1));
        propose(&mut one, 2).await;
        let (late, label, files) = next_batch(&mut lab).await;
        assert_eq!((label.as_str(), files), ("1/2 code codex>default", 1));
        // The first denial leaves the second proposal whole.
        assert!(waits.decide(session_of(run, 2), 7, early, Decision::Denied("no")));
        assert!(waits.decide(session_of(run, 1), 7, late, Decision::Sent));
        let mut reply = String::new();
        let read = timeout(Duration::from_secs(5), one.0.read_line(&mut reply));
        read.await.unwrap().unwrap();
        assert!(lab.app().join("a.txt").exists(), "{reply}");
        assert!(!lab.app().join("b.txt").exists(), "denied");
    }

    // Spec (D7): the review and the refuter read the PROPOSER's batch. With
    // worker 1 under review and worker 2 holding other staged files, the
    // lens prompts show worker 1's file only.
    #[tokio::test]
    async fn a_lens_prompt_shows_its_proposers_files_and_not_a_siblings() {
        let mut lab = writing_lab(Script::By(quiet));
        let waits = Waits::new();
        let (run, handle) = start(&lab, &waits, 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "Cargo.lock", "yanked = 1\n").await;
        stage(&mut two, 1, "b.txt", "sibling\n").await;
        propose(&mut one, 2).await;
        let (consent, ask) = next_ask(&mut lab).await;
        assert!(matches!(ask, ApprovalAsk::Consent { .. }), "{ask:?}");
        assert!(waits.decide(session_of(run, 1), 7, consent, Decision::Sent));
        let (_, _, files) = next_batch(&mut lab).await;
        assert_eq!(files, 1, "the batch review lists worker 1's file only");
        let requests = lab.seen.requests();
        let lenses: Vec<_> = requests.iter().filter(|r| is_lens(r)).collect();
        assert_eq!(lenses.len(), 2);
        for lens in lenses {
            let prompt = user_of(lens);
            assert!(prompt.contains("+yanked = 1"), "{prompt}");
            assert!(!prompt.contains("sibling"), "{prompt}");
            assert!(!prompt.contains("b.txt"), "{prompt}");
        }
    }

    // The two sites a review reads its proposer's batch with, on their own
    // (the correction check re-reads the staged set; every lens and the
    // refuter read the diff).
    #[tokio::test]
    async fn restaged_and_diff_text_read_the_proposers_batch() {
        let lab = writing_lab(Script::Hang);
        let (run, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        for (worker, path) in [(1, "a.txt"), (2, "b.txt")] {
            let id = WorkerId(worker);
            let changes = lab.manager.restaged(pane(), run, id, &[]).await;
            let seen: Vec<_> = changes.unwrap().into_iter().map(|c| c.path).collect();
            assert_eq!(seen, [path], "restaged for worker {worker}");
            let diff = lab.manager.diff_text(pane(), run, id).await;
            let seen: Vec<_> = diff.files.into_iter().map(|f| f.path).collect();
            assert_eq!(seen, [path], "diff for worker {worker}");
        }
    }

    // The correction worker stages into its proposer's batch, not its own and
    // not the sibling's.
    #[tokio::test]
    async fn a_correction_stages_into_its_proposers_batch() {
        let lab = writing_lab(Script::Hang);
        let (run, handle) = start(&lab, &Waits::new(), 3);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        let fixer = lab
            .manager
            .start_review(
                pane(),
                run,
                Role::Correction,
                WorkerId(1),
                "fix".into(),
                None,
            )
            .unwrap();
        assert_eq!(fixer, WorkerId(3));
        let mut fix = client_of(&lab.seen, 2).await;
        stage(&mut fix, 1, "fix.txt", "fixed\n").await;
        assert_eq!(paths(&lab, key_of(run, 1)).await, ["a.txt", "fix.txt"]);
        assert_eq!(paths(&lab, key_of(run, 2)).await, ["b.txt"]);
        assert_eq!(paths(&lab, key_of(run, 3)).await, Vec::<String>::new());
    }

    /// Three slots: worker 1 and 2 stage `Cargo.lock` and `b.txt`; returns the
    /// run for a lens of worker 1 to start.
    async fn two_staged(lab: &Lab) -> (RunId, OrchHandle) {
        let (run, handle) = start(lab, &Waits::new(), 3);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "Cargo.lock", "yanked = 1\n").await;
        stage(&mut two, 1, "b.txt", "sibling\n").await;
        (run, handle)
    }

    /// Worker 3 is a lens of worker 1: it reads worker 1's staged file and not
    /// worker 2's.
    async fn reads_only_its_proposers(lab: &Lab, request: usize) {
        let mut lens = client_of(&lab.seen, request).await;
        assert!(
            sees(&mut lens, 1, "Cargo.lock").await,
            "its proposer's file"
        );
        assert!(!sees(&mut lens, 2, "b.txt").await, "a sibling's file");
    }

    fn lens(lab: &Lab, run: RunId) -> WorkerId {
        let started =
            lab.manager
                .start_review(pane(), run, Role::Reader, WorkerId(1), "look".into(), None);
        started.unwrap()
    }

    // Reader lenses see the staged view of the proposer.
    #[tokio::test]
    async fn a_lens_reads_its_proposers_staged_files_only() {
        let lab = writing_lab(Script::Hang);
        let (run, _handle) = two_staged(&lab).await;
        assert_eq!(lens(&lab, run), WorkerId(3));
        reads_only_its_proposers(&lab, 2).await;
    }

    // Bypass: a lens retried after a failure keeps its proposer.
    #[tokio::test]
    async fn a_retried_lens_keeps_its_proposer() {
        let lab = writing_lab(Script::FailOn(3, "409 conflict: session busy"));
        let (run, handle) = two_staged(&lab).await;
        let id = lens(&lab, run);
        assert_eq!(handle.call(wait(id.0)).await, fine("3: awaiting_user"));
        let retry = lab
            .manager
            .decide(pane(), run, UserChoice::Retry, FamilySet::ALL);
        assert_eq!(retry, Ok(()));
        reads_only_its_proposers(&lab, 3).await;
    }

    // Bypass: a lens relaunched under another roster keeps its proposer.
    #[tokio::test]
    async fn a_relaunched_lens_keeps_its_proposer() {
        let lab = writing_lab(Script::FailOn(3, "409 conflict: session busy"));
        let (run, handle) = two_staged(&lab).await;
        let id = lens(&lab, run);
        assert_eq!(handle.call(wait(id.0)).await, fine("3: awaiting_user"));
        let relaunch = UserChoice::Relaunch(codex_both());
        let decided = lab.manager.decide(pane(), run, relaunch, FamilySet::ALL);
        assert_eq!(decided, Ok(()));
        reads_only_its_proposers(&lab, 3).await;
    }

    // Bypass: a lens that waited in the queue and starts when a slot frees
    // keeps its proposer.
    #[tokio::test]
    async fn a_lens_started_from_the_queue_keeps_its_proposer() {
        let lab = writing_lab(Script::Hang);
        let (run, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "Cargo.lock", "yanked = 1\n").await;
        stage(&mut two, 1, "b.txt", "sibling\n").await;
        let id = lens(&lab, run);
        let queued = lock(&lab.manager.runs)[&pane()].run.status(id);
        assert_eq!(queued, Some(WorkerStatus::Queued));
        // Worker 1 parks on its review: its slot goes to the lens.
        lab.manager.on_run(pane(), run, |live| {
            live.run.park(now(), WorkerId(1));
            lab.manager.sync(pane(), live);
        });
        reads_only_its_proposers(&lab, 2).await;
    }

    // Bypass: a correction retried after a failure still stages into its
    // proposer's batch, and is still no proposer.
    #[tokio::test]
    async fn a_retried_correction_stays_a_correction_of_its_proposer() {
        let lab = writing_lab(Script::FailOn(3, "409 conflict: session busy"));
        let (run, handle) = two_staged(&lab).await;
        let started = lab.manager.start_review(
            pane(),
            run,
            Role::Correction,
            WorkerId(1),
            "fix".into(),
            None,
        );
        let fixer = started.unwrap();
        assert_eq!(handle.call(wait(fixer.0)).await, fine("3: awaiting_user"));
        let retry = lab
            .manager
            .decide(pane(), run, UserChoice::Retry, FamilySet::ALL);
        assert_eq!(retry, Ok(()));
        let mut fix = client_of(&lab.seen, 3).await;
        stage(&mut fix, 1, "fix.txt", "fixed\n").await;
        assert_eq!(paths(&lab, key_of(run, 1)).await, ["Cargo.lock", "fix.txt"]);
        assert_eq!(paths(&lab, key_of(run, 2)).await, ["b.txt"]);
        assert!(handle.corrects(run, fixer), "still no proposer");
    }

    // Bypass: a path a SIBLING stages while worker 1's consent is open is not
    // a late path of worker 1's review.
    #[tokio::test]
    async fn a_sibling_staging_during_a_review_is_not_a_late_path_of_it() {
        let mut lab = writing_lab(Script::By(quiet));
        let waits = Waits::new();
        let (run, handle) = start(&lab, &waits, 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "Cargo.lock", "yanked = 1\n").await;
        propose(&mut one, 2).await;
        let (consent, _) = next_ask(&mut lab).await;
        // Worker 2 stages while worker 1's review is still to begin.
        stage(&mut two, 1, "x.txt", "sibling\n").await;
        assert!(waits.decide(session_of(run, 1), 7, consent, Decision::Sent));
        // The batch review comes after the late-path check.
        next_batch(&mut lab).await;
        let kept = lab.manager.reviews(pane(), run);
        assert_eq!(kept.len(), 1);
        assert!(kept[0].unreviewed.is_empty(), "{:?}", kept[0].unreviewed);
        let said = review_lines(&mut lab);
        assert!(!said.iter().any(|l| l.contains("x.txt")), "{said:?}");
    }

    /// Worker `silent` stages and never proposes; the other proposes.
    async fn one_proposes_one_stays_silent(silent: u32) -> (Lab, RunId, OrchHandle) {
        let mut lab = writing_lab(Script::Hang);
        let waits = Waits::new();
        let (run, handle) = start(&lab, &waits, 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        let proposer = if silent == 1 { &mut two } else { &mut one };
        propose(proposer, 2).await;
        next_batch(&mut lab).await;
        (lab, run, handle)
    }

    // Spec (Staged and never proposed [W1]): the notice names the writer that
    // left a batch unproposed, and only that one. A sibling's proposal does
    // not hide it, and the sibling that proposed is not named.
    #[tokio::test]
    async fn the_unproposed_notice_names_the_silent_writer_only() {
        for silent in [1, 2] {
            let (mut lab, run, handle) = one_proposes_one_stays_silent(silent).await;
            let other = 3 - silent;
            handle
                .call(OrchCall::Cancel {
                    id: WorkerId(silent),
                })
                .await;
            let OrchestraEvent::Disclosed {
                worker,
                what,
                run: from,
                ..
            } = disclosed(&mut lab).await
            else {
                unreachable!()
            };
            assert_eq!((from, worker), (run, WorkerId(silent)));
            assert_eq!(what, Disclosure::StagedNotProposed { files: 1 });
            // The writer that proposed is cancelled with its ask open: its
            // batch is proposed, so nothing was left unsaid.
            handle
                .call(OrchCall::Cancel {
                    id: WorkerId(other),
                })
                .await;
            let told: Vec<_> = lab
                .sent()
                .into_iter()
                .filter(
                    |e| matches!(e, OrchestraEvent::Disclosed { worker, .. } if worker.0 == other),
                )
                .collect();
            assert!(told.is_empty(), "silent {silent}: {told:?}");
        }
    }

    // Stale state: ending the run with two staged writers leaves ONE orphan
    // badge that counts both, and the orphan review opens them oldest first.
    #[tokio::test]
    async fn ending_a_run_with_two_staged_writers_leaves_two_orphans_oldest_first() {
        let lab = writing_lab(Script::Hang);
        let (run, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        lab.manager.cancel(pane());
        let host = lab.host();
        let badges = host.badges(|_| false).unwrap();
        assert_eq!(badges.get(&pane()), Some(&Badge::Orphan(2)));
        let (first, review) = host.review_batch(pane(), false).expect("an orphan");
        assert_eq!(first.key, key_of(run, 1), "the oldest first");
        assert_eq!(review.counts(), "1 file, +1 -0");
        host.discard_batch(first);
        let (second, _) = host.review_batch(pane(), false).expect("the next orphan");
        assert_eq!(second.key, key_of(run, 2));
        host.discard_batch(second);
        assert!(host.review_batch(pane(), false).is_none());
    }

    // Closing the pane drops every worker's batch and says how many entries.
    #[tokio::test]
    async fn closing_the_pane_drops_the_batches_of_both_writers() {
        let lab = writing_lab(Script::Hang);
        let (_, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write a")).await;
        handle.call(spawn(Task::Code, "write b")).await;
        let (mut one, mut two) = (client_of(&lab.seen, 0).await, client_of(&lab.seen, 1).await);
        stage(&mut one, 1, "a.txt", "one\n").await;
        stage(&mut two, 1, "b.txt", "two\n").await;
        lab.manager.cancel(pane());
        let host = lab.host();
        assert_eq!(host.close_batch(pane()), Some(2));
        assert!(host.badges(|_| false).unwrap().is_empty());
    }

    // A run that replaces another in the pane starts with no worker batches:
    // its first writer does not inherit what the old run left staged.
    #[tokio::test]
    async fn a_new_run_starts_with_no_worker_batches() {
        let lab = writing_lab(Script::Hang);
        let (_, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write a")).await;
        let mut old = client_of(&lab.seen, 0).await;
        stage(&mut old, 1, "old.txt", "left\n").await;
        let (run, handle) = start(&lab, &Waits::new(), 2);
        handle.call(spawn(Task::Code, "write new")).await;
        let mut new = client_of(&lab.seen, 1).await;
        stage(&mut new, 1, "new.txt", "fresh\n").await;
        assert_eq!(paths(&lab, key_of(run, 1)).await, ["new.txt"]);
    }
}
