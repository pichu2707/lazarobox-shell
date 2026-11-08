//! The write family in the host: staging touches no disk and asks nothing,
//! `propose_batch` seals the batch and asks the user, and only an answer of
//! `Sent` applies it. What the model is told is counts and paths.

// What only the file-tool tests use is dead code on other systems.
#![cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(dead_code, unused_imports)
)]

use std::sync::Mutex;

use super::{
    tests::{events, pair},
    *,
};
use crate::core::{
    ai::{
        chat::{
            approval::ApprovalAsk,
            review::{BatchEvent, TOO_LARGE},
        },
        tools::{
            registry::{Family, FamilySet},
            report::RunReport,
            stage::StageKey,
        },
    },
    layout::PaneId,
};

/// Answers every ask the same way, remembers the asks, and can run a hook
/// first (to play a user or a pane that acts before the answer arrives).
struct Script {
    answer: Decision,
    asked: Mutex<Vec<ApprovalAsk>>,
    first: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Script {
    fn new(answer: Decision) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::new(Vec::new()),
            first: Mutex::new(None),
        })
    }

    fn asks(&self) -> Vec<ApprovalAsk> {
        self.asked.lock().unwrap().clone()
    }
}

impl Approver for Script {
    fn ask(&self, _: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        self.asked.lock().unwrap().push(ask.clone());
        if let Some(hook) = self.first.lock().unwrap().take() {
            hook();
        }
        let answer = self.answer;
        async move { answer.into() }.boxed()
    }
}

struct World {
    _dir: tempfile::TempDir,
    proj: PathBuf,
    host: Arc<ToolHost>,
}

fn key() -> StageKey {
    StageKey::Pane(PaneId::for_test(1))
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let host = ToolHost::new(Some(home.clone().into()), None, None, Limits::default());
    World {
        _dir: dir,
        proj,
        host,
    }
}

fn on() -> FamilySet {
    FamilySet::LEGACY.with(Family::Write).with(Family::Search)
}

impl World {
    /// A turn of the pane with the write family on.
    fn turn(
        &self,
        approver: Arc<dyn Approver>,
    ) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
        self.turn_with(approver, on(), Some(key()))
    }

    fn turn_with(
        &self,
        approver: Arc<dyn Approver>,
        families: FamilySet,
        stage_key: Option<StageKey>,
    ) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(32);
        let mut scope = TurnScope::new(tx, Locality::Local, self.proj.clone());
        scope.approver = approver;
        scope.families = families;
        scope.stage_key = stage_key;
        (self.host.dispatch(scope), rx)
    }

    fn staged_of(&self, key: StageKey) -> usize {
        let stages = self.host.stages.lock().unwrap();
        stages.batch(key).map_or(0, |b| b.entries().len())
    }

    fn staged(&self) -> usize {
        let stages = self.host.stages.lock().unwrap();
        stages.batch(key()).map_or(0, |b| b.entries().len())
    }
}

fn write(path: &str, content: &str) -> Call {
    Call::WriteFile {
        path: path.into(),
        content: content.into(),
    }
}

fn edit(path: &str, old: &str, new: &str) -> Call {
    Call::EditFile {
        path: path.into(),
        old: old.into(),
        new: new.into(),
    }
}

fn folder(path: &str) -> Call {
    Call::CreateDir { path: path.into() }
}

/// The write refusal of a chat opened in `~/proj` (local-model-writes D2).
const HINTED: &str = "path not available (paths are relative to the chat's folder ~/proj)";

/// The write refusal for a missing parent `folder` that the policy allowed.
#[cfg(target_os = "linux")]
fn missing(folder: &str) -> String {
    format!(
        "path not available: the folder {folder}/ does not exist (paths are relative to the chat's folder ~/proj)"
    )
}

fn read(path: &str) -> Call {
    Call::ReadFile {
        path: path.into(),
        lines: None,
    }
}

// Spec (code-write, create_dir and write_file staging; Nothing writes
// without approval): staging adds to the batch, asks nothing, touches no disk.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn staging_asks_nothing_and_leaves_the_disk_alone() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let (d, mut rx) = w.turn(approver.clone());
    let out = d.call(write("a.txt", "hello new")).await;
    assert!(!out.is_error, "{out:?}");
    assert_eq!(
        out.text,
        "staged a.txt (batch: 1 of 50 entries, 9 of 1048576 bytes)"
    );
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Ok));
    let out = d.call(folder("sub")).await;
    assert_eq!(
        out.text,
        "staged sub (batch: 2 of 50 entries, 9 of 1048576 bytes)"
    );
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Ok));
    assert!(approver.asks().is_empty());
    assert_eq!(w.staged(), 2);
    assert!(!w.proj.join("a.txt").exists() && !w.proj.join("sub").exists());
    // A repeated call for one path replaces its entry.
    d.call(write("a.txt", "second")).await;
    assert_eq!(w.staged(), 2);
}

// Spec (code-write, edit_file staging): applies to the staged version when
// there is one, otherwise to the disk, and never touches the disk.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn edits_chain_on_the_staged_text_and_leave_the_disk_alone() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar").unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(edit("b.txt", "bar", "baz")).await.is_error);
    // The second edit matches text only the first one introduced.
    assert!(!d.call(edit("b.txt", "baz", "qux")).await.is_error);
    assert_eq!(
        std::fs::read_to_string(w.proj.join("b.txt")).unwrap(),
        "foo bar"
    );
    let seen = d.call(read("b.txt")).await;
    assert!(
        seen.text.contains("(pending, not on disk)"),
        "{}",
        seen.text
    );
    assert!(seen.text.contains("foo qux"), "{}", seen.text);
}

// Spec (code-search/code-write): the read tools see the batch of the chat's
// key as soon as the Write family is on, marked as pending.
// Staging needs the write side, Linux only for now.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_read_tools_see_the_staged_batch() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    // A file below a folder that is neither on disk nor staged is refused.
    let early = d.call(write("src/new.txt", "needle text")).await;
    assert!(early.is_error && early.text == missing("src"), "{early:?}");
    assert!(!d.call(folder("src")).await.is_error);
    assert!(!d.call(write("src/new.txt", "needle text")).await.is_error);
    let read_it = d.call(read("src/new.txt")).await;
    assert!(
        read_it.text.contains("(pending, not on disk)\nneedle text"),
        "{}",
        read_it.text
    );
    let glob = d
        .call(Call::Glob {
            pattern: "**/*.txt".into(),
            dir: None,
        })
        .await;
    assert!(
        glob.text.contains("src/new.txt (pending, not on disk)"),
        "{}",
        glob.text
    );
    let grep = d
        .call(Call::Grep {
            pattern: "needle".into(),
            dir: None,
        })
        .await;
    assert!(
        grep.text
            .contains("src/new.txt (pending, not on disk):1:needle text"),
        "{}",
        grep.text
    );
    // The view is the chat's own: another pane sees nothing.
    let (tx, _) = mpsc::channel(32);
    let mut scope = TurnScope::new(tx, Locality::Local, w.proj.clone());
    scope.families = on();
    scope.stage_key = Some(StageKey::Pane(PaneId::for_test(2)));
    let other = w.host.dispatch(scope).call(read("src/new.txt")).await;
    assert!(
        other.is_error && !other.text.contains("needle"),
        "{other:?}"
    );
}

// Spec (code-write, Root policy; Deny rules): every refusal is the same text
// and stages nothing; a content refusal says why but never quotes the text.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_refused_call_stages_nothing_and_says_so_without_the_text() {
    let w = world();
    std::fs::write(w.proj.join("x.txt"), "x x").unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    for call in [
        write("../escape.txt", "t"),
        write(".env", "t"),
        folder("../up"),
        edit("missing.txt", "a", "b"),
    ] {
        let out = d.call(call.clone()).await;
        assert!(out.is_error, "{call:?}");
        assert!(
            out.text == HINTED || out.text.contains("does not exist"),
            "{}",
            out.text
        );
    }
    let many = d.call(edit("x.txt", "x", "SECRET-NEW")).await;
    assert!(
        many.is_error && many.text.contains("more than once"),
        "{many:?}"
    );
    assert!(!many.text.contains("SECRET-NEW"));
    let big = d.call(write("big.txt", &"y".repeat(32 * 1024 + 1))).await;
    assert!(big.is_error && big.text.contains("32 KiB"), "{big:?}");
    assert_eq!(w.staged(), 0);
}

// Spec (code-write, Propose batch and size cap): an empty batch is refused
// and nothing opens.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn proposing_an_empty_batch_is_an_error_and_asks_nothing() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let (d, mut rx) = w.turn(approver.clone());
    let out = d.call(Call::ProposeBatch).await;
    assert!(
        out.is_error && out.text.contains("nothing is staged"),
        "{out:?}"
    );
    assert!(approver.asks().is_empty());
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert_eq!((ask, status), (None, ToolStatus::Error));
}

// Spec (code-write, Batch review; Apply): the batch asks on any engine, and
// only `Sent` writes. The result is the change list: paths and counts.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_approved_batch_is_applied_and_the_model_gets_counts_and_paths() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let (d, mut rx) = w.turn(approver.clone());
    d.call(folder("sub")).await;
    d.call(write("sub/a.txt", "one\ntwo\n")).await;
    events(&mut rx);
    let out = d.call(Call::ProposeBatch).await;
    assert!(!out.is_error, "{out:?}");
    assert!(out.text.starts_with("applied 2 paths:"), "{}", out.text);
    assert!(out.text.contains("sub/a.txt"), "{}", out.text);
    // #439: a small model stops after the review; the result tells it to go on.
    assert!(
        out.text
            .ends_with("\nContinue with any step the user asked for that is not finished yet."),
        "{}",
        out.text
    );
    assert!(!out.text.contains("one"), "{}", out.text);
    assert_eq!(
        std::fs::read_to_string(w.proj.join("sub/a.txt")).unwrap(),
        "one\ntwo\n"
    );
    let asks = approver.asks();
    assert!(
        matches!(asks.as_slice(), [ApprovalAsk::Batch(r)] if r.doc().reviewable()),
        "{asks:?}"
    );
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert!(matches!(ask, Some(ApprovalAsk::Batch(_))));
    assert_eq!(status, ToolStatus::Ok);
    // The batch is closed: it left the store.
    assert_eq!(w.staged(), 0);
}

// Spec (code-write, Reject discards): a rejection, or nobody to ask, writes
// nothing and drops the batch.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_rejected_batch_writes_nothing_and_is_discarded() {
    let w = world();
    for approver in [
        Script::new(Decision::Denied("declined")) as Arc<dyn Approver>,
        Arc::new(Unwired),
    ] {
        let (d, _rx) = w.turn(approver);
        d.call(write("a.txt", "hello")).await;
        let out = d.call(Call::ProposeBatch).await;
        assert!(out.is_error && out.text.starts_with("denied: "), "{out:?}");
        assert!(!w.proj.join("a.txt").exists());
        assert_eq!(w.staged(), 0);
    }
}

// Spec (code-write, Propose batch): a proposed batch accepts no staging until
// the user answers.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn nothing_is_staged_while_a_batch_waits_for_the_user() {
    let w = world();
    let (d, _rx) = w.turn(Arc::new(super::tests::Waiting));
    d.call(write("a.txt", "hello")).await;
    let waiting = tokio::spawn({
        let d = d.clone();
        async move { d.call(Call::ProposeBatch).await }
    });
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let late = d.call(write("b.txt", "late")).await;
    assert!(
        late.is_error && late.text.contains("waiting for review"),
        "{late:?}"
    );
    assert_eq!(w.staged(), 1);
    waiting.abort();
}

// Spec (code-write, Orphan, close and quit): a call dropped while its batch
// waits (the turn was cancelled, or a CLI timed out) leaves the batch proposed
// for the user: an orphan that stays until the user answers or the pane closes.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_call_dropped_while_its_batch_waits_leaves_a_proposed_orphan() {
    let w = world();
    let (d, mut rx) = w.turn(Arc::new(super::tests::Waiting));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    let mut propose = d.call(Call::ProposeBatch);
    // Until the ask is announced the user has seen nothing.
    loop {
        tokio::select! {
            _ = &mut propose => panic!("nobody answered"),
            event = rx.recv() => match event {
                Some(StreamEvent::ToolCall { ref name, .. }) if name == "propose_batch" => break,
                other => assert!(other.is_some(), "the channel closed"),
            },
        }
    }
    let during = d.call(write("b.txt", "late")).await;
    assert!(during.text.contains("waiting for review"), "{during:?}");
    drop(propose);
    assert_eq!(w.staged(), 1, "the orphan stays");
    let after = d.call(write("b.txt", "again")).await;
    assert!(after.text.contains("waiting for review"), "{after:?}");
    // The user can still review it, as the same batch.
    let (id, review) = w
        .host
        .review_batch(key().pane(), false)
        .expect("an orphan to review");
    assert_eq!(review.counts(), "1 file, +1 -0");
    let BatchEvent::Done { shown, .. } = w.host.discard_batch(id) else {
        panic!("a discard is done");
    };
    assert_eq!(shown, "discarded 1 staged entry");
    assert_eq!(w.staged(), 0);
}

// The turn ended while the ask was open (a CLI timeout, a cancel that reached
// the approver first): the answer is `ENDED`, which is no user's no.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_ended_turn_keeps_its_batch_but_a_user_no_discards_it() {
    use crate::runtime::tools::approver::ENDED;
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Denied(ENDED)));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.is_error && out.text.contains(ENDED), "{out:?}");
    assert_eq!(w.staged(), 1);
    let (id, _) = w.host.review_batch(key().pane(), false).unwrap();
    w.host.discard_batch(id);
    let (d, _rx) = w.turn(Script::new(Decision::Denied("no")));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    assert!(d.call(Call::ProposeBatch).await.is_error);
    assert_eq!(w.staged(), 0, "a user's no closes the batch");
}

/// A key of a worker of pane 1, for the re-propose tests.
fn worker_key(worker: u32) -> StageKey {
    StageKey::Worker(
        PaneId::for_test(1),
        crate::core::ai::orchestra::WorkerKey::for_test(1, worker),
    )
}

// Design, Corrections row on the denied ask: the ask lives under the turn
// that made it; when the turn ends it is denied with `ENDED`, which is no
// user's no, and the batch stays proposed. The writer that owns the batch may
// propose it again and the user is asked again, in a worker as in a plain
// chat turn. Every proposal asks.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_batch_whose_ask_ended_is_asked_again_when_its_owner_proposes_it_again() {
    use crate::runtime::tools::approver::ENDED;
    for owner in [key(), worker_key(1)] {
        let w = world();
        let ended = Script::new(Decision::Denied(ENDED));
        let (d, _rx) = w.turn_with(ended.clone(), on(), Some(owner));
        assert!(!d.call(write("a.txt", "hello")).await.is_error);
        let first = d.call(Call::ProposeBatch).await;
        assert!(first.text.contains(ENDED), "{owner:?}: {first:?}");
        assert_eq!(w.staged_of(owner), 1, "{owner:?}: the batch stays");
        // Again, with the turn over and nobody asked: a fresh ask.
        let again = Script::new(Decision::Denied(ENDED));
        let (d, _rx) = w.turn_with(again.clone(), on(), Some(owner));
        let second = d.call(Call::ProposeBatch).await;
        assert!(second.text.contains(ENDED), "{owner:?}: {second:?}");
        assert_eq!(again.asks().len(), 1, "{owner:?}: asked again");
        // And again, now answered: it applies the same files.
        let yes = Script::new(Decision::Sent);
        let (d, _rx) = w.turn_with(yes.clone(), on(), Some(owner));
        let third = d.call(Call::ProposeBatch).await;
        assert!(!third.is_error, "{owner:?}: {third:?}");
        assert_eq!(yes.asks().len(), 1);
        assert_eq!(
            std::fs::read_to_string(w.proj.join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(w.staged_of(owner), 0);
    }
}

// Staying refused: while the first ask is open a second proposal is refused
// and no second ask is made; a call dropped with its ask open (a cancel)
// closes that ask, so the next proposal asks again.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_second_proposal_is_refused_while_the_first_ask_is_open_and_asks_after_a_drop() {
    let w = world();
    let owner = worker_key(1);
    let (d, mut rx) = w.turn_with(Arc::new(super::tests::Waiting), on(), Some(owner));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    let mut propose = d.call(Call::ProposeBatch);
    loop {
        tokio::select! {
            _ = &mut propose => panic!("nobody answered"),
            event = rx.recv() => match event {
                Some(StreamEvent::ToolCall { ref name, .. }) if name == "propose_batch" => break,
                other => assert!(other.is_some(), "the channel closed"),
            },
        }
    }
    // The ask is open: a parallel proposal is refused, with no new ask.
    let during = d.call(Call::ProposeBatch).await;
    assert!(
        during.is_error && during.text.contains("waiting for review"),
        "{during:?}"
    );
    while let Ok(event) = rx.try_recv() {
        let StreamEvent::ToolCall { approval, .. } = event else {
            continue;
        };
        assert!(approval.is_none(), "no second ask was announced");
    }
    drop(propose);
    // The ask is gone with the call: the owner proposes again and is asked.
    let asked = Script::new(Decision::Sent);
    let (d, _rx) = w.turn_with(asked.clone(), on(), Some(owner));
    let again = d.call(Call::ProposeBatch).await;
    assert!(!again.is_error, "{again:?}");
    assert_eq!(asked.asks().len(), 1);
}

// Staying refused: a proposal is never answered with another key's batch. A
// worker with nothing staged gets no ask because a sibling has an orphan.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_worker_cannot_propose_the_orphan_batch_of_another_key() {
    use crate::runtime::tools::approver::ENDED;
    let w = world();
    let (a, _rx_a) = w.turn_with(
        Script::new(Decision::Denied(ENDED)),
        on(),
        Some(worker_key(1)),
    );
    assert!(!a.call(write("a.txt", "hello")).await.is_error);
    assert!(a.call(Call::ProposeBatch).await.is_error);
    let asked = Script::new(Decision::Sent);
    let (b, _rx_b) = w.turn_with(asked.clone(), on(), Some(worker_key(2)));
    let out = b.call(Call::ProposeBatch).await;
    assert!(
        out.is_error && out.text.contains("nothing is staged"),
        "{out:?}"
    );
    assert!(asked.asks().is_empty());
    assert_eq!(
        w.staged_of(worker_key(1)),
        1,
        "the sibling's batch is as it was"
    );
    assert!(!w.proj.join("a.txt").exists());
}

// A run report is no answer to a batch: the call is denied, and the batch is
// closed, not left as an orphan (only a call dropped while it waits is one).
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_report_answering_a_batch_denies_it_and_closes_the_batch() {
    use crate::core::ai::tools::settle::End;
    let w = world();
    let report = RunReport {
        number: 1,
        end: End::Settled,
        cwd: None,
        cd_target: None,
        output: None,
    };
    let (d, _rx) = w.turn(Arc::new(super::tests::Ran(report)));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.is_error, "{out:?}");
    assert_eq!(w.staged(), 0, "a denied batch is not an orphan");
    assert!(w.host.review_batch(key().pane(), false).is_none());
}

// The command was typed but the turn ended: that is no user's no, but it is
// no orphan either: the batch of a call that ran something is closed.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_ran_ended_answer_discards_the_batch() {
    use crate::runtime::tools::approver::RAN_ENDED;
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Denied(RAN_ENDED)));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    assert!(d.call(Call::ProposeBatch).await.is_error);
    assert_eq!(w.staged(), 0);
}

// A batch the turn never proposed is a staging orphan: reviewing it seals it.
// What the user is told holds counts only, never the staged text.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_unproposed_batch_is_sealed_when_the_user_reviews_it() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(
        !d.call(write("a.txt", "sk-ant-api03-SECRETSECRET"))
            .await
            .is_error
    );
    assert!(!d.call(folder("sub")).await.is_error);
    let (id, review) = w
        .host
        .review_batch(key().pane(), false)
        .expect("a staging orphan");
    assert_eq!(review.counts(), "2 files, +1 -0");
    assert_eq!(
        w.host
            .review_batch(key().pane(), false)
            .map(|(again, _)| again),
        Some(id)
    );
    let after = d.call(write("b.txt", "x")).await;
    assert!(after.text.contains("waiting for review"), "{after:?}");
    let BatchEvent::Done { shown, told } = w.host.apply_batch(id, false, None) else {
        panic!("an apply is done");
    };
    assert_eq!(shown, "applied 2 paths");
    // Model-facing: counts only, never the staged text.
    let told = told.expect("the model is told");
    assert_eq!(
        told,
        "The user applied the batch you left staged: applied 2 paths."
    );
    assert!(!told.contains("SECRET") && !shown.contains("SECRET"));
    assert_eq!(
        std::fs::read_to_string(w.proj.join("a.txt")).unwrap(),
        "sk-ant-api03-SECRETSECRET"
    );
    assert!(w.proj.join("sub").is_dir());
    let gone = BatchEvent::Done {
        shown: "that batch is no longer there".into(),
        told: None,
    };
    assert_eq!(w.host.apply_batch(id, false, None), gone);
    assert_eq!(w.host.discard_batch(id), gone);
    assert!(
        w.host.review_batch(key().pane(), false).is_none(),
        "nothing to review"
    );
}

// `unproposed` is true only for a batch with entries still being staged; no
// batch, an empty batch and a batch the user was handed (sealed) are false.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_batch_is_unproposed_only_while_it_holds_entries_and_is_not_sealed() {
    let w = world();
    assert!(!w.host.unproposed(key()), "no batch");
    w.host.stages.lock().unwrap().stage(key()).unwrap();
    assert!(!w.host.unproposed(key()), "an empty batch");
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(write("a.txt", "one")).await.is_error);
    assert!(w.host.unproposed(key()), "staging with an entry");
    w.host
        .review_batch(key().pane(), false)
        .expect("sealed for review");
    assert!(!w.host.unproposed(key()), "proposed");
}

// The same for a call whose channel is gone before it can announce: the batch
// it sealed is not left behind.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_call_that_cannot_announce_does_not_leave_its_batch_sealed() {
    let w = world();
    let (live, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!live.call(write("a.txt", "hello")).await.is_error);
    let (closed, rx) = w.turn(Script::new(Decision::Sent));
    drop(rx);
    let out = closed.call(Call::ProposeBatch).await;
    assert!(out.is_error && out.text == "cancelled", "{out:?}");
    assert_eq!(w.staged(), 0);
    assert!(!live.call(write("b.txt", "again")).await.is_error);
}

// A re-ask that never reaches the user must not destroy what the user was
// already offered: the batch was proposed before (its ask ended with the
// turn), so a call that dies before its announcement only ends its own ask.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_re_ask_that_cannot_announce_keeps_the_batch_the_user_was_offered() {
    use crate::core::ai::tools::stage::State;
    use crate::runtime::tools::approver::ENDED;
    for owner in [key(), worker_key(1)] {
        let w = world();
        let (d, _rx) = w.turn_with(Script::new(Decision::Denied(ENDED)), on(), Some(owner));
        assert!(!d.call(write("a.txt", "hello")).await.is_error);
        assert!(d.call(Call::ProposeBatch).await.text.contains(ENDED));
        let (closed, rx) = w.turn_with(Script::new(Decision::Sent), on(), Some(owner));
        drop(rx);
        let out = closed.call(Call::ProposeBatch).await;
        assert!(
            out.is_error && out.text == "cancelled",
            "{owner:?}: {out:?}"
        );
        assert_eq!(w.staged_of(owner), 1, "{owner:?}: the offered batch stays");
        let state = w
            .host
            .stages
            .lock()
            .unwrap()
            .batch(owner)
            .map(|b| b.state());
        assert_eq!(state, Some(State::Proposed));
        // No ask is open any more: the owner can propose it once more.
        let yes = Script::new(Decision::Sent);
        let (d, _rx) = w.turn_with(yes.clone(), on(), Some(owner));
        assert!(!d.call(Call::ProposeBatch).await.is_error);
        assert_eq!(yes.asks().len(), 1);
    }
}

// The user seals an orphan through the review and the model proposes the same
// batch: the model's call asks too. When the user applies the orphan first,
// the model's answer finds the batch gone and says so; nothing applies twice.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_model_ask_over_an_orphan_under_review_fails_cleanly_once_the_user_applied_it() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    // A staging orphan sealed by the user's review.
    let (id, _) = w.host.review_batch(key().pane(), false).unwrap();
    let (live, mut rx) = w.turn(Arc::new(super::tests::Waiting));
    let mut propose = live.call(Call::ProposeBatch);
    loop {
        tokio::select! {
            _ = &mut propose => panic!("nobody answered"),
            event = rx.recv() => match event {
                Some(StreamEvent::ToolCall { ref name, .. }) if name == "propose_batch" => break,
                other => assert!(other.is_some(), "the channel closed"),
            },
        }
    }
    let BatchEvent::Done { shown, .. } = w.host.apply_batch(id, false, None) else {
        panic!("an apply is done");
    };
    assert_eq!(shown, "applied 1 paths");
    assert_eq!(
        std::fs::read_to_string(w.proj.join("a.txt")).unwrap(),
        "hello"
    );
    // The call of the model is dropped; its ask finds nothing and writes nothing.
    drop(propose);
    assert_eq!(w.staged(), 0);
    assert_eq!(
        w.host.apply_batch(id, false, None),
        BatchEvent::Done {
            shown: "that batch is no longer there".into(),
            told: None,
        }
    );
    assert_eq!(
        std::fs::read_to_string(w.proj.join("a.txt")).unwrap(),
        "hello"
    );
}

// The disk is read before the lock is taken: staging never holds the mutex of
// every chat's batches across a read.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn staging_reads_the_disk_before_it_takes_the_lock() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo").unwrap();
    let env = w.host.env(&w.proj);
    for call in [
        write("a.txt", "x"),
        edit("b.txt", "foo", "bar"),
        folder("d"),
    ] {
        let probed = std::cell::Cell::new(None);
        let text = files::stage_with_probe(&w.host.stages, key(), &env, &call, |roots, real| {
            probed.set(Some(w.host.stages.try_lock().is_ok()));
            crate::runtime::tools::stage::disk_bytes(roots, real)
        })
        .unwrap();
        assert_eq!(probed.get(), Some(true), "{call:?}: the lock was held");
        assert!(text.starts_with("staged "), "{text}");
    }
    assert_eq!(w.staged(), 3);
}

// An async worker never waits for the mutex: while an apply holds it, a call
// that needs it parks on the blocking pool and the worker keeps serving tasks.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_busy_store_does_not_block_the_async_worker() {
    for call in [Call::ProposeBatch, read("a.txt")] {
        let w = world();
        let (d, _rx) = w.turn(Script::new(Decision::Denied("no")));
        assert!(!d.call(write("a.txt", "hello")).await.is_error);
        let held = w.host.stages.lock().unwrap();
        let waiting = tokio::spawn({
            let d = d.clone();
            async move { d.call(call).await }
        });
        // Plain sleeps: with the worker blocked the timers would not fire.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tokio::spawn({
            let ran = ran.clone();
            async move { ran.store(true, std::sync::atomic::Ordering::SeqCst) }
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        let alive = ran.load(std::sync::atomic::Ordering::SeqCst);
        drop(held);
        assert!(alive, "the worker is blocked on the store lock");
        waiting.await.unwrap();
    }
}

impl World {
    /// A turn of the pane on a cloud engine, with the write family on.
    fn cloud(
        &self,
        approver: Arc<dyn Approver>,
    ) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(32);
        let mut scope = TurnScope::new(tx, Locality::Cloud, self.proj.clone());
        scope.approver = approver;
        scope.families = on();
        scope.stage_key = Some(key());
        (self.host.dispatch(scope), rx)
    }
}

// Spec (code-write, edit_file staging): on a cloud engine an edit asks like a
// read of the file, once per file per turn; a denial stages nothing; a read
// the user already allowed counts; a new turn asks again; a local engine never
// asks. The ask carries the path, never the text.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_cloud_edit_asks_once_per_file_per_turn() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar").unwrap();
    std::fs::write(w.proj.join("c.txt"), "one two").unwrap();
    let asked = |a: &Script| a.asks().len();

    // Local: never.
    let local = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(local.clone());
    assert!(!d.call(edit("b.txt", "foo", "SECRET-NEW")).await.is_error);
    assert_eq!(asked(&local), 0);
    w.host.stages.lock().unwrap().discard_key(key());

    // Cloud: asks, with the path and no text, then not again for that file.
    let approver = Script::new(Decision::Sent);
    let (d, mut rx) = w.cloud(approver.clone());
    let first = d.call(edit("b.txt", "foo", "SECRET-NEW")).await;
    assert!(!first.is_error, "{first:?}");
    let asks = approver.asks();
    assert!(
        matches!(asks.as_slice(), [ApprovalAsk::Read { path, .. }] if path.ends_with("/b.txt")),
        "{asks:?}"
    );
    assert!(!format!("{asks:?}").contains("SECRET"));
    let (_, ask, status, _) = pair(&events(&mut rx));
    assert!(matches!(ask, Some(ApprovalAsk::Read { .. })));
    assert_eq!(status, ToolStatus::Ok);
    assert!(!d.call(edit("b.txt", "bar", "baz")).await.is_error);
    assert_eq!(asked(&approver), 1);
    // Another file asks again.
    assert!(!d.call(edit("c.txt", "one", "uno")).await.is_error);
    assert_eq!(asked(&approver), 2);
    w.host.stages.lock().unwrap().discard_key(key());

    // A new turn asks again.
    let next = Script::new(Decision::Sent);
    let (d2, _rx2) = w.cloud(next.clone());
    assert!(!d2.call(edit("b.txt", "foo", "x")).await.is_error);
    assert_eq!(asked(&next), 1);
    w.host.stages.lock().unwrap().discard_key(key());

    // A read allowed first counts for the edit.
    let both = Script::new(Decision::Sent);
    let (d3, _rx3) = w.cloud(both.clone());
    assert!(!d3.call(read("b.txt")).await.is_error);
    assert!(!d3.call(edit("b.txt", "foo", "y")).await.is_error);
    assert_eq!(asked(&both), 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_denied_cloud_edit_stages_nothing_and_a_denial_is_not_remembered() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar").unwrap();
    let approver = Script::new(Decision::Denied("declined"));
    let (d, _rx) = w.cloud(approver.clone());
    for _ in 0..2 {
        let out = d.call(edit("b.txt", "foo", "SECRET-NEW")).await;
        assert!(out.is_error && out.text == "denied: declined", "{out:?}");
    }
    assert_eq!(approver.asks().len(), 2);
    assert_eq!(w.staged(), 0);
}

// A path the ask could not show as it is opened is refused before any ask: the
// user never approves a path that differs from the one that is used.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_cloud_edit_of_an_unshowable_path_is_refused_without_an_ask() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.cloud(approver.clone());
    for path in ["b\u{202e}.txt", "b\u{200b}.txt"] {
        for _ in 0..2 {
            let out = d.call(edit(path, "foo", "x")).await;
            assert!(out.is_error && out.text == UNSHOWABLE, "{out:?}");
        }
    }
    // Nothing was remembered either: the refusal repeats and nothing asks.
    assert!(approver.asks().is_empty());
    assert_eq!(w.staged(), 0);
}

// Spelling does not matter: the key is the path as `display_target` resolves
// it. A read records into `allowed` like an edit; a read never consults it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn different_spellings_of_one_path_share_a_single_cloud_ask() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar baz").unwrap();
    std::fs::create_dir_all(w.proj.join("sub")).unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.cloud(approver.clone());
    assert!(!d.call(edit("b.txt", "foo", "1")).await.is_error);
    for (path, old) in [
        ("./b.txt", "bar"),
        ("sub/../b.txt", "baz"),
        ("~/proj/b.txt", "1"),
    ] {
        // The staging path may refuse these spellings; only the asks count.
        d.call(edit(path, old, "2")).await;
    }
    assert_eq!(approver.asks().len(), 1);

    w.host.stages.lock().unwrap().discard_key(key());
    // A read of one spelling allows an edit of another.
    let mixed = Script::new(Decision::Sent);
    let (d2, _rx2) = w.cloud(mixed.clone());
    assert!(!d2.call(read("./b.txt")).await.is_error);
    assert!(!d2.call(edit("b.txt", "foo", "y")).await.is_error);
    assert_eq!(mixed.asks().len(), 1);

    w.host.stages.lock().unwrap().discard_key(key());
    // A read does not consult `allowed`: it asks even after an allowed edit.
    let reverse = Script::new(Decision::Sent);
    let (d3, _rx3) = w.cloud(reverse.clone());
    assert!(!d3.call(edit("b.txt", "foo", "z")).await.is_error);
    assert!(!d3.call(read("b.txt")).await.is_error);
    assert_eq!(reverse.asks().len(), 2);
}

// An edit with an empty `old` can only fail, so the user is not asked for it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_cloud_edit_with_an_empty_old_is_refused_before_the_ask() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar").unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.cloud(approver.clone());
    let out = d.call(edit("b.txt", "", "x")).await;
    assert!(
        out.is_error && out.text == "old must not be empty",
        "{out:?}"
    );
    assert!(approver.asks().is_empty());
    assert_eq!(w.staged(), 0);
}

// Only `Sent` is remembered: a busy answer (and any other non-`Sent` one)
// leaves the next edit of the file asking again.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_busy_cloud_edit_stages_nothing_and_is_not_remembered() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo bar").unwrap();
    let approver = Script::new(Decision::Busy(2));
    let (d, _rx) = w.cloud(approver.clone());
    for _ in 0..2 {
        let out = d.call(edit("b.txt", "foo", "x")).await;
        assert!(out.is_error, "{out:?}");
    }
    assert_eq!(approver.asks().len(), 2);
    assert_eq!(w.staged(), 0);
}

// Spec (code-write, Propose batch; design D6/unreviewable): a batch whose diff
// is truncated is sealed, the user can only reject it, and `Sent` never
// applies it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_unreviewable_batch_cannot_be_applied_and_the_model_is_told_to_split() {
    let w = world();
    let lines = "x\n".repeat(3000);
    for answer in [Decision::Sent, Decision::Denied(TOO_LARGE)] {
        let approver = Script::new(answer);
        let (d, _rx) = w.turn(approver.clone());
        assert!(!d.call(write("long.txt", &lines)).await.is_error);
        let out = d.call(Call::ProposeBatch).await;
        assert!(out.is_error && out.text.contains(TOO_LARGE), "{out:?}");
        assert!(!out.text.contains("x\nx"), "{}", out.text);
        let asks = approver.asks();
        assert!(
            matches!(asks.as_slice(), [ApprovalAsk::Batch(r)] if !r.doc().reviewable()),
            "{asks:?}"
        );
        assert!(!w.proj.join("long.txt").exists());
        assert_eq!(w.staged(), 0);
    }
}

// Spec (code-write, Apply): the answer reaches the batch by id. A batch that
// was discarded meanwhile (pane closed) is not applied by a late `y`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_answer_for_a_batch_that_is_gone_applies_nothing() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let host = w.host.clone();
    *approver.first.lock().unwrap() = Some(Box::new(move || {
        host.stages.lock().unwrap().discard_key(key());
    }));
    let (d, _rx) = w.turn(approver);
    d.call(write("a.txt", "hello")).await;
    let out = d.call(Call::ProposeBatch).await;
    assert!(
        out.is_error && out.text.contains("no longer the current one"),
        "{out:?}"
    );
    assert!(!w.proj.join("a.txt").exists());
}

// Spec (code-write, Drift refuses the batch): a file changed after staging
// refuses the whole batch, nothing is written, and the batch is closed so the
// model can stage it again.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_drifted_batch_writes_nothing_and_leaves_the_model_free_to_restage() {
    let w = world();
    std::fs::write(w.proj.join("d.txt"), "before").unwrap();
    let approver = Script::new(Decision::Sent);
    let path = w.proj.join("d.txt");
    *approver.first.lock().unwrap() = Some(Box::new(move || {
        std::fs::write(path, "someone else").unwrap();
    }));
    let (d, _rx) = w.turn(approver);
    d.call(write("new.txt", "fresh")).await;
    d.call(edit("d.txt", "before", "after")).await;
    let out = d.call(Call::ProposeBatch).await;
    assert!(
        out.is_error && out.text.starts_with("refused: 1 paths changed"),
        "{out:?}"
    );
    assert!(out.text.contains("d.txt"), "{}", out.text);
    assert!(!out.text.contains("someone else"), "{}", out.text);
    assert!(!w.proj.join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(w.proj.join("d.txt")).unwrap(),
        "someone else"
    );
    assert_eq!(w.staged(), 0);
    assert!(!d.call(write("new.txt", "again")).await.is_error);
}

// Spec (agent-tools, Off equals 0.18.0): a write call with the family off is
// refused like an unknown tool and stages nothing, even if it got this far.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_write_call_with_the_family_off_is_unknown_and_stages_nothing() {
    let w = world();
    for families in [FamilySet::LEGACY, FamilySet::LEGACY.with(Family::Search)] {
        let approver = Script::new(Decision::Sent);
        let (d, mut rx) = w.turn_with(approver.clone(), families, Some(key()));
        for call in [
            write("a.txt", "t"),
            folder("d"),
            edit("a.txt", "a", "b"),
            Call::ProposeBatch,
        ] {
            let out = d.call(call).await;
            assert!(out.is_error && out.text == "unknown tool", "{out:?}");
        }
        assert!(approver.asks().is_empty());
        assert!(events(&mut rx).len() >= 8);
        assert_eq!(w.staged(), 0);
        assert!(!w.proj.join("a.txt").exists());
    }
    // No key to stage under is the same refusal.
    let (d, _rx) = w.turn_with(Script::new(Decision::Sent), on(), None);
    assert_eq!(d.call(write("a.txt", "t")).await.text, "unknown tool");
}

// The Write family off: the read tools get no view, so a batch staged earlier
// under the same key is not shown (and not forgeable text either).
// Staging needs the write side, Linux only for now.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn with_the_family_off_the_read_tools_see_no_batch() {
    let w = world();
    let (on_turn, _rx) = w.turn(Script::new(Decision::Sent));
    on_turn.call(write("new.txt", "needle text")).await;
    assert!(!on_turn.call(read("new.txt")).await.is_error);
    let (off, _rx) = w.turn_with(Script::new(Decision::Sent), FamilySet::LEGACY, Some(key()));
    let out = off.call(read("new.txt")).await;
    assert!(out.is_error && !out.text.contains("needle"), "{out:?}");
}

// Review-only invariant (Wd review): the base and the new text of an edit,
// and so every diff row, reach nothing the model sees.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_secret_in_the_base_or_the_new_text_reaches_no_model_facing_string() {
    const KEY: &str = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const NEW: &str = "hunter2";
    let outcomes = [
        Decision::Sent,
        Decision::Denied("declined"),
        Decision::Denied(TOO_LARGE),
    ];
    for answer in outcomes {
        let w = world();
        std::fs::write(w.proj.join("s.txt"), format!("token = {KEY}\nport = 1\n")).unwrap();
        let (d, mut rx) = w.turn(Script::new(answer));
        let staged = d.call(edit("s.txt", "port = 1", NEW)).await.text;
        assert!(staged.starts_with("staged s.txt (batch: 1 of"), "{staged}");
        let proposed = d.call(Call::ProposeBatch).await.text;
        let expect = match answer {
            Decision::Sent => "applied 1 paths:\nedited ",
            Decision::Denied(reason) => &format!("denied: {reason}"),
            _ => unreachable!(),
        };
        assert!(proposed.starts_with(expect), "{proposed}");
        assert_eq!(
            proposed.contains("Continue with any step"),
            matches!(answer, Decision::Sent),
            "{proposed}"
        );
        let rows: Vec<String> = events(&mut rx).iter().map(|e| format!("{e:?}")).collect();
        // One ToolCall and one ToolResult for each of the two calls.
        assert_eq!(rows.len(), 4, "{rows:?}");
        let mut seen = vec![staged, proposed];
        seen.extend(rows);
        for text in &seen {
            assert!(
                !text.contains(KEY) && !text.contains("AAAAAAAAAAAAAAAA"),
                "{text}"
            );
            assert!(!text.contains(NEW), "{text}");
        }
    }
}

// Review-only invariant, over every other output a secret could reach: the
// text of a `write_file`, the staged view the read tools show, the edit
// refusals, the apply summaries and the wait for review.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_secret_reaches_no_other_model_facing_output() {
    use crate::runtime::tools::apply::ApplyResult;
    const KEY: &str = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const NEW: &str = "hunter2";
    let clean = |label: &str, text: &str| {
        assert!(
            !text.contains(KEY) && !text.contains("AAAAAAAAAAAAAAAA") && !text.contains(NEW),
            "{label}: {text}"
        );
    };
    // The view of the model's own staged text is redacted for a recognizable
    // token; a plain word it wrote itself is its own text and stays.
    let redacted = |label: &str, text: &str| {
        assert!(text.contains("[REDACTED]"), "{label}: {text}");
        assert!(
            !text.contains(KEY) && !text.contains("AAAAAAAAAAAAAAAA"),
            "{label}: {text}"
        );
    };
    let w = world();
    std::fs::write(w.proj.join("s.txt"), format!("a = {KEY}\na = {KEY}\n")).unwrap();
    std::fs::write(w.proj.join("d.txt"), "before").unwrap();
    let approver = Script::new(Decision::Sent);
    let path = w.proj.join("d.txt");
    *approver.first.lock().unwrap() = Some(Box::new(move || {
        std::fs::write(path, format!("{KEY} {NEW}")).unwrap();
    }));
    let (d, mut rx) = w.turn(approver);

    // write_file whose content holds a secret.
    let wrote = d
        .call(write("n.txt", &format!("key = {KEY}\npw = {NEW}\n")))
        .await;
    assert!(!wrote.is_error, "{wrote:?}");
    assert!(
        wrote
            .text
            .starts_with("staged n.txt (batch: 1 of 50 entries,"),
        "{}",
        wrote.text
    );
    clean("write_file", &wrote.text);

    // The staged view and the disk file the read tools show are redacted.
    let read_staged = d.call(read("n.txt")).await;
    assert!(
        read_staged.text.contains("(pending, not on disk)"),
        "{}",
        read_staged.text
    );
    assert!(read_staged.text.contains("key = "), "{}", read_staged.text);
    redacted("read_file staged", &read_staged.text);
    let grep = d
        .call(Call::Grep {
            pattern: "key".into(),
            dir: None,
        })
        .await;
    assert!(
        grep.text.contains("n.txt (pending, not on disk):1:key = "),
        "{}",
        grep.text
    );
    redacted("grep staged", &grep.text);

    // The edit refusals never quote old or new.
    let missing = d.call(edit("s.txt", &format!("{KEY}-x"), NEW)).await;
    assert!(
        missing.is_error && missing.text.contains("does not occur"),
        "{missing:?}"
    );
    clean("edit NoMatch", &missing.text);
    let twice = d.call(edit("s.txt", KEY, NEW)).await;
    assert!(
        twice.is_error && twice.text.contains("more than once"),
        "{twice:?}"
    );
    clean("edit Ambiguous", &twice.text);

    // Drift: the disk text changed to a secret after the review.
    assert!(!d.call(edit("d.txt", "before", "after")).await.is_error);
    let drifted = d.call(Call::ProposeBatch).await;
    assert!(
        drifted.is_error && drifted.text.starts_with("refused: 1 paths changed"),
        "{drifted:?}"
    );
    clean("Drifted", &drifted.text);
    // Every apply summary is paths and counts, whatever the path is called.
    let named = std::path::PathBuf::from("dir/s.txt");
    for result in [
        ApplyResult::Drifted(vec![named.clone()]),
        ApplyResult::Failed {
            path: named.clone(),
            undone: true,
        },
        ApplyResult::Failed {
            path: named.clone(),
            undone: false,
        },
        ApplyResult::Invalid {
            path: named.clone(),
        },
        ApplyResult::Partial {
            changed: vec![named.clone()],
        },
        ApplyResult::Denied {
            path: named.clone(),
            undone: true,
        },
        ApplyResult::VolumeUnsupported {
            path: named.clone(),
            undone: true,
        },
        ApplyResult::Unsupported,
    ] {
        clean("summary", &result.summary());
    }

    // The wait for review: staging is refused with a fixed text.
    let (waiting, _rx2) = w.turn(Arc::new(super::tests::Waiting));
    assert!(!waiting.call(write("w.txt", "x")).await.is_error);
    let pending = tokio::spawn({
        let waiting = waiting.clone();
        async move { waiting.call(Call::ProposeBatch).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let late = waiting.call(write("l.txt", &format!("{KEY} {NEW}"))).await;
    assert!(
        late.is_error && late.text.contains("waiting for review"),
        "{late:?}"
    );
    clean("waiting for review", &late.text);
    let late_edit = waiting.call(edit("s.txt", KEY, NEW)).await;
    assert!(
        late_edit.is_error && late_edit.text.contains("waiting for review"),
        "{late_edit:?}"
    );
    clean("waiting for review (edit)", &late_edit.text);
    pending.abort();

    // And no event the chat gets carries it either.
    for row in events(&mut rx) {
        clean("event", &format!("{row:?}"));
    }
}

// Wd review, applied: the result of an approved edit names the path and counts.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_applied_edit_reports_its_path_and_counts() {
    let w = world();
    std::fs::write(w.proj.join("e.txt"), "a\nb\n").unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    d.call(edit("e.txt", "b", "c")).await;
    let out = d.call(Call::ProposeBatch).await;
    assert!(
        out.text.starts_with("applied 1 paths:\nedited "),
        "{}",
        out.text
    );
    assert!(out.text.contains("e.txt (+1 -1,"), "{}", out.text);
    assert_eq!(
        std::fs::read_to_string(w.proj.join("e.txt")).unwrap(),
        "a\nc\n"
    );
}

// The host is the trust boundary: an orphan the user could not have reviewed
// (a truncated diff) is refused here, whatever the caller sends. Nothing is
// written and the batch stays for the user to discard.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_unreviewable_orphan_is_not_applied() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(
        !d.call(write("long.txt", &"x\n".repeat(3000)))
            .await
            .is_error
    );
    let (id, review) = w.host.review_batch(key().pane(), false).unwrap();
    assert!(!review.doc().reviewable());
    let event = w.host.apply_batch(id, false, None);
    assert_eq!(
        event,
        BatchEvent::Done {
            shown: TOO_LARGE.into(),
            told: None
        }
    );
    assert!(!w.proj.join("long.txt").exists());
    assert_eq!(w.staged(), 1, "the batch stays for the user to discard");
}

// A turn of the pane is live: the batch is the model's, so an apply that
// arrives anyway writes nothing.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_orphan_is_not_applied_while_a_turn_of_the_pane_is_live() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    let (id, _) = w.host.review_batch(key().pane(), false).unwrap();
    let event = w.host.apply_batch(id, true, None);
    assert_eq!(
        event,
        BatchEvent::Done {
            shown: super::orphan::LIVE.into(),
            told: None
        }
    );
    assert!(!w.proj.join("a.txt").exists());
    assert_eq!(w.staged(), 1);
}

// User decision: a drifted apply writes nothing, leaves the batch proposed
// for the user, and does not tell the model.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_drifted_orphan_apply_leaves_the_batch_proposed_and_tells_nothing() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "foo\n").unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(edit("b.txt", "foo", "bar")).await.is_error);
    let (id, _) = w.host.review_batch(key().pane(), false).unwrap();
    std::fs::write(w.proj.join("b.txt"), "changed\n").unwrap();
    let BatchEvent::Done { shown, told } = w.host.apply_batch(id, false, None) else {
        panic!("done");
    };
    assert!(shown.starts_with("refused"), "{shown}");
    assert_eq!(told, None);
    assert_eq!(
        std::fs::read_to_string(w.proj.join("b.txt")).unwrap(),
        "changed\n"
    );
    assert_eq!(w.staged(), 1);
    let stages = w.host.stages.lock().unwrap();
    assert_eq!(
        stages.batch(key()).map(|b| b.state()),
        Some(crate::core::ai::tools::stage::State::Proposed)
    );
}

// Model-facing text of an orphan answer carries counts only: the base of an
// edit (disk text the model never saw) reaches neither line.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_orphan_answer_never_tells_the_model_the_base_or_the_diff() {
    let w = world();
    std::fs::write(w.proj.join("b.txt"), "key=sk-ant-api03-BASESECRET\nfoo\n").unwrap();
    for apply in [true, false] {
        let (d, _rx) = w.turn(Script::new(Decision::Sent));
        assert!(!d.call(edit("b.txt", "foo", "bar")).await.is_error);
        let (id, _) = w.host.review_batch(key().pane(), false).unwrap();
        let event = if apply {
            w.host.apply_batch(id, false, None)
        } else {
            w.host.discard_batch(id)
        };
        let BatchEvent::Done { shown, told } = event else {
            panic!("done");
        };
        let all = format!("{shown} {}", told.expect("told"));
        assert!(!all.contains("BASESECRET") && !all.contains("foo"), "{all}");
        std::fs::write(w.proj.join("b.txt"), "key=sk-ant-api03-BASESECRET\nfoo\n").unwrap();
    }
}

// local-model-writes D2: every write refusal is one text that names the
// chat's folder, whatever the reason, so a model learns where paths start
// but cannot tell an excluded path from a missing one.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn every_write_refusal_names_the_chat_folder() {
    let w = world();
    std::fs::write(w.proj.join("x.txt"), "x").unwrap();
    std::fs::create_dir(w.proj.join("dir")).unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    for call in [
        write("../escape.txt", "t"),
        write(".env", "t"),
        write("/etc/x", "t"),
        folder("../up"),
        folder("dir"),
        write("x.txt/y", "t"),
        edit(".git/config", "a", "b"),
    ] {
        let out = d.call(call.clone()).await;
        assert!(out.is_error, "{call:?}");
        assert_eq!(out.text, HINTED, "{call:?}");
    }
    assert_eq!(w.staged(), 0);
}

// A chat folder that is HOME, above it or `/` has no concrete root: every
// write says so and asks for `/cd`, instead of hinting at relative paths.
// The refusals above, inside a concrete folder, keep the uniform text.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn writes_in_a_broad_folder_ask_for_a_project_folder() {
    let w = world();
    let home = w.proj.parent().unwrap().to_path_buf();
    let above = home.parent().unwrap().to_path_buf();
    let above_shown = above.display().to_string();
    for (origin, shown) in [
        (home, "~".to_owned()),
        (above, above_shown),
        (PathBuf::from("/"), "/".to_owned()),
    ] {
        let (tx, _rx) = mpsc::channel(32);
        let mut scope = TurnScope::new(tx, Locality::Local, origin);
        scope.approver = Script::new(Decision::Sent);
        scope.families = on();
        scope.stage_key = Some(key());
        let d = w.host.dispatch(scope);
        for call in [
            write("fibonacci.py", "t"),
            folder("project"),
            edit("a", "a", "b"),
        ] {
            let out = d.call(call.clone()).await;
            assert!(out.is_error, "{call:?}");
            assert_eq!(
                out.text,
                format!(
                    "path not available: the chat's folder is {shown}, where files cannot be written; ask the user to run /cd <project folder>"
                ),
                "{call:?}"
            );
        }
    }
    assert_eq!(w.staged(), 0);
}

// The folder-level text is for broad folders only: a concrete folder inside
// HOME and one outside it keep the uniform text.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn writes_in_a_concrete_folder_keep_the_uniform_text() {
    let w = world();
    let outside = tempfile::tempdir().unwrap();
    for origin in [
        w.proj.clone(),
        outside.path().canonicalize().unwrap(),
        PathBuf::from("rel"),
    ] {
        let (tx, _rx) = mpsc::channel(32);
        let mut scope = TurnScope::new(tx, Locality::Local, origin.clone());
        scope.approver = Script::new(Decision::Sent);
        scope.families = on();
        scope.stage_key = Some(key());
        let d = w.host.dispatch(scope);
        let out = d.call(write("../escape.txt", "t")).await;
        assert!(out.is_error, "{origin:?}");
        assert!(
            out.text
                .starts_with("path not available (paths are relative to the chat's folder "),
            "{origin:?}: {}",
            out.text
        );
        assert!(!out.text.contains("/cd"), "{origin:?}");
    }
}

// #438: a `$HOME` reached through a symlink still shows the chat's folder as
// `~/…`: the origin is a real path, so it is compared with the real home.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_symlinked_home_still_shows_the_folder_from_tilde() {
    let w = world();
    let home = w.proj.parent().unwrap();
    let link = home.parent().unwrap().join("home-link");
    std::os::unix::fs::symlink(home, &link).unwrap();
    let host = ToolHost::new(Some(link.into()), None, None, Limits::default());
    let (tx, _rx) = mpsc::channel(32);
    let mut scope = TurnScope::new(tx, Locality::Local, w.proj.clone());
    scope.approver = Script::new(Decision::Sent);
    scope.families = on();
    scope.stage_key = Some(key());
    let d = host.dispatch(scope);
    let out = d.call(write("../escape.txt", "t")).await;
    assert_eq!(out.text, HINTED);
    let out = d.call(write("gone/x.txt", "t")).await;
    assert_eq!(out.text, missing("gone"));
}

// local-model-writes D2: once the policy allowed the path, a parent folder
// that does not exist is named, as the model wrote it.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_missing_parent_folder_is_named_after_the_policy_passed() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    let out = d.call(write("Descargas/fibonacci.py", "print(1)")).await;
    assert!(out.is_error);
    assert_eq!(out.text, missing("Descargas"));
    let deep = d.call(folder("a/b/c")).await;
    assert_eq!(deep.text, missing("a/b"));
    let edited = d.call(edit("gone/x.txt", "a", "b")).await;
    assert_eq!(edited.text, missing("gone"));
    // A path the policy refuses keeps the one text, even below a missing folder.
    let secret = d.call(write("nope/.env", "t")).await;
    assert_eq!(secret.text, HINTED);
    let deps = d.call(write("node_modules/gone/x.js", "t")).await;
    assert_eq!(deps.text, HINTED);
    // A link on the way, in the root or out of it or dangling, is never named.
    std::os::unix::fs::symlink(w.proj.join("gone"), w.proj.join("dangling")).unwrap();
    std::os::unix::fs::symlink("/tmp", w.proj.join("out")).unwrap();
    for path in ["dangling/x.txt", "dangling/sub/x.txt", "out/missing/x.txt"] {
        let linked = d.call(write(path, "t")).await;
        assert_eq!(linked.text, HINTED, "{path}");
    }
    assert_eq!(w.staged(), 0);
}

// #583: an env template reaches the user's review like any other write:
// staging asks nothing and writes nothing, and a denied batch leaves no file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_env_template_stops_at_the_review_and_a_denial_writes_nothing() {
    let w = world();
    let approver = Script::new(Decision::Denied("no"));
    let (d, _rx) = w.turn(approver.clone());
    let staged = d.call(write(".env.example", "A=1\n")).await;
    assert!(
        !staged.is_error && staged.text.starts_with("staged .env.example"),
        "{staged:?}"
    );
    assert!(approver.asks().is_empty(), "staging asks nothing");
    assert!(!w.proj.join(".env.example").exists());
    d.call(Call::ProposeBatch).await;
    assert_eq!(approver.asks().len(), 1, "the batch asks the user");
    assert!(!w.proj.join(".env.example").exists());
}

// #583: with the user's answer the template is written, and a read of it is
// still refused.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_approved_env_template_is_written_and_stays_unreadable() {
    let w = world();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    d.call(write(".env.example", "A=1\n")).await;
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.text.starts_with("applied 1 paths:"), "{out:?}");
    assert_eq!(approver.asks().len(), 1);
    assert_eq!(
        std::fs::read_to_string(w.proj.join(".env.example")).unwrap(),
        "A=1\n"
    );
    assert!(d.call(read(".env.example")).await.is_error);
}

// #583: an existing template can be changed, by write_file and by edit_file,
// again only through the review.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_existing_env_template_is_replaced_and_edited_after_the_answer() {
    let w = world();
    let (replace, edited) = (w.proj.join(".env.sample"), w.proj.join(".env.dist"));
    std::fs::write(&replace, "OLD=1\n").unwrap();
    std::fs::write(&edited, "A=1\nB=2\n").unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    assert!(!d.call(write(".env.sample", "NEW=1\n")).await.is_error);
    assert!(!d.call(edit(".env.dist", "B=2", "B=3")).await.is_error);
    assert_eq!(std::fs::read_to_string(&replace).unwrap(), "OLD=1\n");
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.text.starts_with("applied 2 paths:"), "{out:?}");
    assert_eq!(std::fs::read_to_string(&replace).unwrap(), "NEW=1\n");
    assert_eq!(std::fs::read_to_string(&edited).unwrap(), "A=1\nB=3\n");
}

// #583: the folder of a template can be new in the same batch, and a missing
// one is named.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_env_template_can_go_in_a_folder_of_the_same_batch() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    d.call(folder("web")).await;
    d.call(write("web/.env.example", "A=1\n")).await;
    let out = d.call(Call::ProposeBatch).await;
    assert!(out.text.starts_with("applied 2 paths:"), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(w.proj.join("web/.env.example")).unwrap(),
        "A=1\n"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_env_template_below_a_missing_folder_names_the_folder() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    let out = d.call(write("gone/.env.example", "A=1\n")).await;
    assert_eq!(out.text, missing("gone"));
    assert_eq!(w.staged(), 0);
}

// #583: the opening is for the template's own name. Real env files, a folder
// with a template's name, and a link in its place stay refused.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn real_env_files_folders_and_links_stay_refused() {
    let w = world();
    std::fs::write(w.proj.join("secret.txt"), "s").unwrap();
    std::os::unix::fs::symlink(w.proj.join("secret.txt"), w.proj.join(".env.sample")).unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    for path in [".env", ".env.local", "web/.env", ".env.sample"] {
        let out = d.call(write(path, "A=1\n")).await;
        assert_eq!(out.text, HINTED, "{path}");
    }
    let out = d.call(folder(".env.example")).await;
    assert_eq!(out.text, HINTED);
    assert_eq!(w.staged(), 0);
    assert!(approver.asks().is_empty());
    assert!(!w.proj.join(".env").exists());
    assert!(!w.proj.join(".env.example").exists());
}

// #583: `~/.env -> proj/.env.example` makes the template the real target of a
// secret: judging it by its folder alone must not open it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_template_behind_a_secret_link_of_home_stays_refused() {
    let w = world();
    let target = w.proj.join(".env.example");
    std::fs::write(&target, "A=1\n").unwrap();
    let home = w.proj.parent().unwrap();
    std::os::unix::fs::symlink(&target, home.join(".env")).unwrap();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    let out = d.call(write(".env.example", "B=2\n")).await;
    assert_eq!(out.text, HINTED);
    assert_eq!(w.staged(), 0);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "A=1\n");
}

// #583: `edit_file` answers whether a text occurs, and a read of a template is
// refused, so on a local engine too it asks before it says anything. A denial
// stages nothing and tells nothing about the file.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_local_edit_of_an_env_template_asks_first_and_a_denial_tells_nothing() {
    let w = world();
    std::fs::write(w.proj.join(".env.example"), "KEY=topsecret\n").unwrap();
    let approver = Script::new(Decision::Denied("no"));
    let (d, _rx) = w.turn(approver.clone());
    let hit = d.call(edit(".env.example", "KEY=top", "KEY=top")).await;
    let miss = d.call(edit(".env.example", "KEY=wrong", "KEY=x")).await;
    assert_eq!(approver.asks().len(), 2, "each denied edit asks");
    assert!(matches!(approver.asks()[0], ApprovalAsk::Read { .. }));
    assert_eq!(hit.text, miss.text);
    assert_eq!(w.staged(), 0);
}

// #583: a yes covers the file for the turn, as it does for a cloud edit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_local_edit_of_an_env_template_asks_once_per_turn() {
    let w = world();
    std::fs::write(w.proj.join(".env.example"), "A=1\nB=2\n").unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    assert!(!d.call(edit(".env.example", "A=1", "A=3")).await.is_error);
    assert!(!d.call(edit(".env.example", "B=2", "B=3")).await.is_error);
    assert_eq!(approver.asks().len(), 1);
}

/// Existing templates that `write_file` used to answer differently for: a
/// plain one, one with a secret-looking run, one with CRLF and one over the
/// read cap (#583).
fn template_contents() -> Vec<(&'static str, String)> {
    vec![
        (".env.plain.example", "A=1\n".to_owned()),
        (
            ".env.secret.example",
            "KEY=sk-abcDEF1234567890abcdef\n".to_owned(),
        ),
        (".env.crlf.example", "A=1\r\nB=2\r\n".to_owned()),
        (".env.big.example", "A=1\n".repeat(30_000)),
    ]
}

// #583: `write_file` on an existing template answered "staged" or "use
// edit_file" by what the file holds, with no ask. It asks first now, like an
// edit, and a denial is the same whatever the file holds.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_write_over_an_existing_env_template_asks_first_and_a_denial_tells_nothing() {
    let w = world();
    for (name, content) in template_contents() {
        std::fs::write(w.proj.join(name), content).unwrap();
    }
    let approver = Script::new(Decision::Denied("no"));
    let (d, _rx) = w.turn(approver.clone());
    let mut answers = Vec::new();
    for (name, _) in template_contents() {
        answers.push(d.call(write(name, "x\n")).await);
    }
    assert_eq!(approver.asks().len(), 4, "each denied write asks");
    assert!(matches!(approver.asks()[0], ApprovalAsk::Read { .. }));
    assert!(
        answers.iter().all(|a| a.text == answers[0].text),
        "{answers:?}"
    );
    assert!(answers[0].text.starts_with("denied"), "{answers:?}");
    assert_eq!(w.staged(), 0);
}

// #583: the approval mode never answers for the user here, so an `ask` or
// `auto` run gets the same denial and nothing is staged.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_unprompted_answer_does_not_let_a_write_over_a_template_through() {
    let w = world();
    for (name, content) in template_contents() {
        std::fs::write(w.proj.join(name), content).unwrap();
    }
    let (d, _rx) = w.turn(Script::new(Decision::Unprompted));
    let mut answers = Vec::new();
    for (name, _) in template_contents() {
        answers.push(d.call(write(name, "x\n")).await);
    }
    assert!(
        answers.iter().all(|a| a.text == answers[0].text),
        "{answers:?}"
    );
    assert!(answers[0].text.starts_with("denied"), "{answers:?}");
    assert_eq!(w.staged(), 0);
}

// #583: with the user's yes the write stages as it did, and the yes covers
// the file for the turn.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn an_allowed_write_over_an_env_template_stages_and_asks_once_per_turn() {
    let w = world();
    std::fs::write(w.proj.join(".env.example"), "A=1\n").unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    assert!(!d.call(write(".env.example", "A=2\n")).await.is_error);
    assert!(!d.call(write(".env.example", "A=3\n")).await.is_error);
    d.call(Call::ProposeBatch).await;
    assert_eq!(
        approver.asks().len(),
        2,
        "one for the file, one for the batch"
    );
    assert_eq!(
        std::fs::read_to_string(w.proj.join(".env.example")).unwrap(),
        "A=3\n"
    );
}

// #583: staging itself never tells the content of a template that nobody
// allowed, even if the file appeared after the ask was decided.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn staging_a_write_over_an_unallowed_template_refuses_whatever_it_holds() {
    let w = world();
    for (name, content) in template_contents() {
        std::fs::write(w.proj.join(name), content).unwrap();
    }
    let env = w.host.env(&w.proj);
    let mut answers = Vec::new();
    for (name, _) in template_contents() {
        let call = write(name, "x\n");
        let out = files::stage_with_probe(&w.host.stages, key(), &env, &call, |roots, real| {
            crate::runtime::tools::stage::disk_bytes(roots, real)
        });
        answers.push(out);
    }
    assert!(answers.iter().all(|a| a == &answers[0]), "{answers:?}");
    assert!(answers[0].is_err(), "{answers:?}");
    assert_eq!(w.staged(), 0);
}

// The ask is for the template alone: a local edit of an ordinary file stays
// free, and one the policy refuses opens no prompt.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_local_edit_asks_for_nothing_else() {
    let w = world();
    std::fs::write(w.proj.join("a.txt"), "A=1\n").unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    assert!(!d.call(edit("a.txt", "A=1", "A=2")).await.is_error);
    assert_eq!(d.call(edit(".env", "A=1", "A=2")).await.text, HINTED);
    assert!(approver.asks().is_empty());
}

// #583: the ask reads the name as the model wrote it. A path that only reaches
// a template by stepping back is refused by the policy, so no edit runs unasked.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_path_that_steps_back_to_an_env_template_is_refused() {
    let w = world();
    std::fs::write(w.proj.join(".env.example"), "KEY=topsecret\n").unwrap();
    let approver = Script::new(Decision::Sent);
    let (d, _rx) = w.turn(approver.clone());
    for path in [
        ".env.example/x/..",
        ".env.example/.",
        ".env.example/",
        "./.env.example/",
        "a/../.env.example",
    ] {
        let out = d.call(edit(path, "KEY=top", "KEY=top")).await;
        assert_eq!(out.text, HINTED, "{path}");
    }
    assert_eq!(w.staged(), 0);
}

/// A WSL drive `<tmp>/mnt/c` beside HOME, set as the host's drives, with a
/// project at `Users/ana/Documents/app` holding `a`.
#[cfg(target_os = "linux")]
fn drive_app(w: &World) -> (PathBuf, PathBuf) {
    let drive = w.proj.parent().unwrap().parent().unwrap().join("mnt/c");
    let app = drive.join("Users/ana/Documents/app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("a"), "a").unwrap();
    w.host.set_drives(WinDrives::new(
        vec![drive.clone()],
        Some(drive.join("Users/ana")),
    ));
    (drive, app)
}

/// A turn of the pane in `origin`, with the write family on, `grant` as
/// the chat's drive grant and every ask answered `answer`.
#[cfg(target_os = "linux")]
fn drive_turn(
    w: &World,
    origin: &Path,
    grant: Option<PathBuf>,
    answer: Decision,
) -> (Arc<dyn Dispatch>, mpsc::Receiver<StreamEvent>) {
    let (tx, rx) = mpsc::channel(32);
    let mut scope = TurnScope::new(tx, Locality::Local, origin.to_path_buf());
    scope.approver = Script::new(answer);
    scope.families = on();
    scope.stage_key = Some(key());
    scope.drive_grant = grant;
    (w.host.dispatch(scope), rx)
}

// openspec `wsl-drive-access` PR 2 and PR 3 (code-write, Drive folder without
// a grant): every write gets the folder-level text, whatever the path, and
// nothing is staged.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn writes_in_an_ungranted_drive_folder_get_the_folder_text() {
    let w = world();
    let (_, app) = drive_app(&w);
    let (d, _rx) = drive_turn(&w, &app, None, Decision::Sent);
    let shown = app.display();
    let text = format!(
        "path not available: the chat's folder {shown} is on a Windows drive, where writes, commands and git need your permission; ask the user to run /cd {shown}"
    );
    for call in [
        write("main.py", "t"),
        write("../x", "t"),
        write(".env", "t"),
        folder("src"),
        edit("a", "a", "b"),
    ] {
        let out = d.call(call.clone()).await;
        assert!(out.is_error, "{call:?}");
        assert_eq!(out.text, text, "{call:?}");
    }
    assert_eq!(w.staged(), 0);
    assert!(!app.join("main.py").exists());
    // The grant of another folder is no grant of this one.
    let (other, _rx2) = drive_turn(&w, &app, Some(app.join("sub")), Decision::Sent);
    assert_eq!(other.call(write("main.py", "t")).await.text, text);
    assert_eq!(w.staged(), 0);
}

// PR 2 (code-write, Granted drive folder): a write in the folder stages and
// applies to the disk; one outside it gets the one path text.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn writes_in_a_granted_drive_folder_stage_and_apply() {
    let w = world();
    let (_, app) = drive_app(&w);
    let (d, _rx) = drive_turn(&w, &app, Some(app.clone()), Decision::Sent);
    let outside = d.call(write("../x", "t")).await;
    assert!(outside.is_error);
    assert_eq!(
        outside.text,
        format!(
            "path not available (paths are relative to the chat's folder {})",
            app.display()
        )
    );
    assert_eq!(w.staged(), 0);
    let staged = d.call(write("main.py", "print(1)\n")).await;
    assert!(!staged.is_error, "{staged:?}");
    assert!(
        staged.text.starts_with("staged main.py (batch: 1 of"),
        "{staged:?}"
    );
    assert!(!d.call(edit("a", "a", "b")).await.is_error);
    assert_eq!(w.staged(), 2);
    let applied = d.call(Call::ProposeBatch).await;
    assert!(!applied.is_error, "{applied:?}");
    assert!(applied.text.starts_with("applied 2 paths:"), "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(app.join("main.py")).unwrap(),
        "print(1)\n"
    );
    assert_eq!(std::fs::read_to_string(app.join("a")).unwrap(), "b");
    assert!(!app.parent().unwrap().join("x").exists());
}

// PR 2: the apply judges the grant again. A batch staged under the grant and
// applied without it (an orphan the user applies outside the turn) writes
// nothing.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_apply_without_the_grant_writes_nothing_on_the_drive() {
    let w = world();
    let (_, app) = drive_app(&w);
    let (d, _rx) = drive_turn(&w, &app, Some(app.clone()), Decision::Sent);
    assert!(!d.call(write("main.py", "t")).await.is_error);
    drop((d, _rx));
    let (id, _) = w
        .host
        .review_batch(key().pane(), false)
        .expect("a staging orphan");
    let done = w.host.apply_batch(id, false, None);
    assert!(!app.join("main.py").exists(), "{done:?}");
    assert_eq!(std::fs::read_to_string(app.join("a")).unwrap(), "a");
}

// Verifier B1: an orphan left in a granted drive folder applies with the
// grant the chat holds when the user presses `y`.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn an_orphan_in_a_granted_drive_folder_applies_with_the_chats_grant() {
    let w = world();
    let (_, app) = drive_app(&w);
    let (d, _rx) = drive_turn(&w, &app, Some(app.clone()), Decision::Sent);
    assert!(!d.call(write("main.py", "t")).await.is_error);
    drop((d, _rx));
    let (id, _) = w
        .host
        .review_batch(key().pane(), false)
        .expect("a staging orphan");
    let BatchEvent::Done { shown, told } = w.host.apply_batch(id, false, Some(app.clone())) else {
        panic!("expected the apply result");
    };
    assert_eq!(shown, "applied 1 paths");
    assert!(told.is_some());
    assert_eq!(std::fs::read_to_string(app.join("main.py")).unwrap(), "t");
    assert_eq!(w.staged(), 0);
}

// PR 2: a broad drive folder (a drive root, a profile root) is broad like
// HOME, and no grant changes that.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn writes_in_a_broad_drive_folder_ask_for_a_project_folder() {
    let w = world();
    let (drive, _) = drive_app(&w);
    for origin in [drive.clone(), drive.join("Users/ana")] {
        let (d, _rx) = drive_turn(&w, &origin, Some(origin.clone()), Decision::Sent);
        let out = d.call(write("x.txt", "t")).await;
        assert!(out.is_error);
        assert_eq!(
            out.text,
            format!(
                "path not available: the chat's folder is {}, where files cannot be written; ask the user to run /cd <project folder>",
                origin.display()
            )
        );
    }
    assert_eq!(w.staged(), 0);
}

// Verifier B3: a broad drive folder (a drive root, a profile root) is broad
// like HOME: reads and searches there do not get the reach of a root.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_broad_drive_folder_has_a_broad_reach() {
    use crate::core::ai::chat::approval::Reach;
    let w = world();
    let drive = w.proj.parent().unwrap().parent().unwrap().join("mnt/c");
    let app = drive.join("Users/ana/Documents/app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("README.md"), "x").unwrap();
    w.host.set_drives(WinDrives::new(
        vec![drive.clone()],
        Some(drive.join("Users/ana")),
    ));
    let readme = app.join("README.md");
    for (origin, want) in [
        (drive.clone(), Reach::Broad),
        (drive.join("Users/ana"), Reach::Broad),
        (app.clone(), Reach::Root),
    ] {
        let (tx, _rx) = mpsc::channel(32);
        let scope = TurnScope::new(tx, Locality::Cloud, origin.clone());
        let got = super::files::reach(&w.host, &scope, readme.to_str().unwrap()).await;
        assert_eq!(got, want, "{origin:?}");
        assert_eq!(
            super::files::search_reach(&w.host, &scope).await,
            want,
            "{origin:?}"
        );
    }
}

// DIAGNOSIS (investigate/orch-orphans, F2b): Ctrl+C on an open batch review
// makes the app answer the ask with `Denied("cancelled")` (see the app test
// that expects `decision(turn, 1, Decision::Denied("cancelled"))`), and the
// README says the batch then stays as an orphan. `answered` keeps the batch
// only for `ENDED`, so the user's cancel is treated as a "no" and the whole
// batch is discarded.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_cancel_answered_on_the_review_keeps_the_batch_as_an_orphan() {
    for owner in [key(), worker_key(1)] {
        let w = world();
        let cancel = Script::new(Decision::Denied("cancelled"));
        let (d, _rx) = w.turn_with(cancel, on(), Some(owner));
        assert!(!d.call(write("a.txt", "hello")).await.is_error);
        assert!(d.call(Call::ProposeBatch).await.is_error);
        assert_eq!(
            w.staged_of(owner),
            1,
            "{owner:?}: the cancel must keep the batch"
        );
    }
}

// DIAGNOSIS (investigate/orch-orphans, F2a candidate): a propose_batch call
// dropped after the batch was sealed but before its ask was announced (the
// events channel is full, so `reserve_owned` waits) discards the whole staged
// batch (`Sealed::drop` with `keep == false`), although the user never saw it
// and the work is only staged. A cancel there must leave the staged work as an
// orphan, as a cancel while staging does.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn a_propose_dropped_before_its_ask_is_announced_keeps_the_staged_work() {
    let w = world();
    let owner = worker_key(1);
    let (tx, mut rx) = mpsc::channel(4);
    let mut scope = TurnScope::new(tx.clone(), Locality::Local, w.proj.clone());
    scope.approver = Arc::new(super::tests::Waiting);
    scope.families = on();
    scope.stage_key = Some(owner);
    let d = w.host.dispatch(scope);
    assert!(!d.call(write("a.txt", "hello")).await.is_error);
    while rx.try_recv().is_ok() {}
    // Fill the channel so the announcement of the ask has no room.
    while tx.try_send(StreamEvent::Slow).is_ok() {}
    let mut propose = d.call(Call::ProposeBatch);
    // Wait for the seal itself (the proposed state), not for a fixed time.
    let sealed = || {
        let stages = w.host.stages.lock().unwrap();
        stages
            .batch(owner)
            .is_some_and(|b| b.state() == crate::core::ai::tools::stage::State::Proposed)
    };
    for _ in 0..2000 {
        assert!(futures::poll!(&mut propose).is_pending());
        if sealed() {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(sealed(), "the batch never reached the sealed state");
    // Sealed, not announced: the cancel arrives now.
    assert!(futures::poll!(&mut propose).is_pending());
    drop(propose);
    assert_eq!(
        w.staged_of(owner),
        1,
        "a cancel before the announcement lost the batch"
    );
    assert!(
        w.host.review_batch(owner.pane(), false).is_some(),
        "the kept batch is not a reviewable orphan"
    );
}
