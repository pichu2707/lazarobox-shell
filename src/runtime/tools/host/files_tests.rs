//! The write family in the host: staging touches no disk and asks nothing,
//! `propose_batch` seals the batch and asks the user, and only an answer of
//! `Sent` applies it. What the model is told is counts and paths.

// What only the Linux-only tests use is dead code elsewhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

use std::sync::Mutex;

use super::{
    tests::{events, pair},
    *,
};
use crate::core::{
    ai::{
        chat::{approval::ApprovalAsk, review::TOO_LARGE},
        tools::{
            paths::REFUSED,
            registry::{Family, FamilySet},
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

fn read(path: &str) -> Call {
    Call::ReadFile {
        path: path.into(),
        lines: None,
    }
}

// Spec (code-write, create_dir and write_file staging; Nothing writes
// without approval): staging adds to the batch, asks nothing, touches no disk.
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_read_tools_see_the_staged_batch() {
    let w = world();
    let (d, _rx) = w.turn(Script::new(Decision::Sent));
    // A file below a folder that is neither on disk nor staged is refused.
    let early = d.call(write("src/new.txt", "needle text")).await;
    assert!(early.is_error && early.text == REFUSED, "{early:?}");
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
#[cfg(target_os = "linux")]
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
            out.text == REFUSED || out.text.contains("does not exist"),
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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

// Spec (code-write, Propose batch; design D6/unreviewable): a batch whose diff
// is truncated is sealed, the user can only reject it, and `Sent` never
// applies it.
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
