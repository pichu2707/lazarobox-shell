//! Worker tests: a scripted `Transport` on tokio's paused clock, replaying the
//! recorded engram 3.4.0 responses.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{sync::mpsc, time::Instant};

use super::*;
use crate::core::{
    ai::memory::{
        health::Health,
        kind::MemKind,
        port::{Note, Reason},
        reply::ProjectError,
        testing::split,
    },
    layout::PaneId,
};

const PORT: u16 = 9100;
const INSTANCE: &str = "999100b4a245378075d419132ab1c06e";

macro_rules! fx {
    ($name:literal) => {{
        let (status, body) = split(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/core/ai/memory/fixtures/3.4.0/",
            $name,
            ".http"
        )));
        (status, body.to_vec())
    }};
}

/// What the scripted server does with one call.
#[derive(Clone)]
enum Act {
    Reply(u16, Vec<u8>),
    Fail(CallError),
    Hang,
    Panic,
}

/// How the server as a whole behaves.
#[derive(Clone)]
enum Mode {
    Up(String),
    Down,
    Old,
    Foreign,
}

#[derive(Clone, Debug)]
struct Seen {
    key: String,
    total: Duration,
}

struct State {
    mode: Mode,
    overrides: Vec<(String, Act)>,
    seen: Vec<Seen>,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn new(mode: Mode) -> Self {
        Self(Arc::new(Mutex::new(State {
            mode,
            overrides: Vec::new(),
            seen: Vec::new(),
        })))
    }

    fn mode(&self, mode: Mode) {
        self.0.lock().unwrap().mode = mode;
    }

    /// Calls whose `METHOD target` starts with `prefix` get `act`.
    fn on(&self, prefix: &str, act: Act) {
        self.0.lock().unwrap().overrides.push((prefix.into(), act));
    }

    fn keys(&self) -> Vec<String> {
        let state = self.0.lock().unwrap();
        state.seen.iter().map(|s| s.key.clone()).collect()
    }

    fn count(&self, prefix: &str) -> usize {
        self.keys().iter().filter(|k| k.starts_with(prefix)).count()
    }

    fn total_of(&self, prefix: &str) -> Duration {
        let state = self.0.lock().unwrap();
        state
            .seen
            .iter()
            .find(|s| s.key.starts_with(prefix))
            .unwrap_or_else(|| panic!("no call {prefix}"))
            .total
    }
}

fn verb(call: &Call) -> &'static str {
    use crate::core::ai::memory::route::Method;
    match call.method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
    }
}

fn health_body(instance: &str) -> Vec<u8> {
    let (_, mut body) = fx!("health_3_4_0");
    let text = String::from_utf8(body.clone()).unwrap();
    body = text.replace(INSTANCE, instance).into_bytes();
    body
}

impl Transport for Fake {
    async fn call(&self, call: &Call, limits: Limits) -> Result<Reply, CallError> {
        let key = format!("{} {}", verb(call), call.target);
        let act = {
            let mut state = self.0.lock().unwrap();
            state.seen.push(Seen {
                key: key.clone(),
                total: limits.total,
            });
            let over = state
                .overrides
                .iter()
                .rev()
                .find(|(prefix, _)| key.starts_with(prefix))
                .map(|(_, act)| act.clone());
            over.unwrap_or_else(|| default_act(&state.mode, &key))
        };
        match act {
            Act::Reply(status, body) => Ok(Reply { status, body }),
            Act::Fail(error) => Err(error),
            Act::Hang => std::future::pending().await,
            Act::Panic => panic!("scripted panic"),
        }
    }
}

fn reply((status, body): (u16, Vec<u8>)) -> Act {
    Act::Reply(status, body)
}

fn default_act(mode: &Mode, key: &str) -> Act {
    if matches!(mode, Mode::Down) {
        return Act::Fail(CallError::Connect);
    }
    if key.starts_with("GET /health") {
        return match mode {
            Mode::Up(id) => Act::Reply(200, health_body(id)),
            Mode::Old => reply(fx!("health_1_20")),
            _ => reply(fx!("health_html")),
        };
    }
    if key.starts_with("GET /project/current") {
        reply(fx!("project_200"))
    } else if key.starts_with("GET /context") {
        reply(fx!("context_compact"))
    } else if key.starts_with("POST /sessions/") {
        reply(fx!("session_end_200"))
    } else if key.starts_with("POST /sessions") {
        reply(fx!("session_created"))
    } else if key.starts_with("POST /observations") {
        reply(fx!("observation_saved_201"))
    } else if key.starts_with("DELETE /observations") {
        reply(fx!("delete_200"))
    } else {
        Act::Reply(404, b"{}".to_vec())
    }
}

struct Rig {
    link: MemoryLink,
    rx: mpsc::Receiver<MemReply>,
    fake: Fake,
}

fn rig(mode: Mode) -> Rig {
    let fake = Fake::new(mode);
    let (tx, rx) = mpsc::channel(256);
    let link = spawn(fake.clone(), PORT, tx);
    Rig { link, rx, fake }
}

fn up() -> Rig {
    rig(Mode::Up(INSTANCE.into()))
}

fn pane(n: u32) -> PaneId {
    PaneId::for_test(n)
}

fn resolve(n: u32, cwd: &str) -> MemCmd {
    MemCmd::Resolve {
        pane: pane(n),
        cwd: cwd.into(),
    }
}

fn save(n: u32, session: Option<&str>) -> MemCmd {
    MemCmd::Save {
        pane: pane(n),
        session: session.map(Into::into),
        project: "widget".into(),
        note: Note {
            kind: MemKind::Decision,
            title: "Use Axum".into(),
            content: "We chose Axum.".into(),
        },
    }
}

impl Rig {
    /// The next reply, within 30 s of paused time.
    async fn next(&mut self) -> MemReply {
        tokio::time::timeout(Duration::from_secs(30), self.rx.recv())
            .await
            .expect("a reply in time")
            .expect("the worker is alive")
    }

    /// The next reply that is not a health announcement.
    async fn answer(&mut self) -> MemReply {
        loop {
            let reply = self.next().await;
            if !matches!(reply, MemReply::Health { .. }) {
                return reply;
            }
        }
    }

    fn quiet(&mut self) -> bool {
        self.rx.try_recv().is_err()
    }
}

fn saved_session(reply: &MemReply) -> String {
    match reply {
        MemReply::Saved { session, .. } => session.clone(),
        other => panic!("not saved: {other:?}"),
    }
}

// ---- 5.2 session id ----

#[test]
fn a_session_id_is_lazarobox_and_32_lowercase_hex() {
    let id = session_id_from([0xab; 16]);
    assert_eq!(id, format!("lazarobox-{}", "ab".repeat(16)));
    let id = session_id_from([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    assert_eq!(id, "lazarobox-000102030405060708090a0b0c0d0e0f");
}

#[test]
fn fresh_session_ids_are_valid_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        let id = new_session_id().expect("the OS has randomness");
        let hex = id.strip_prefix("lazarobox-").expect("prefix");
        assert_eq!(hex.len(), 32, "{id}");
        assert!(hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        assert!(seen.insert(id), "duplicate id");
    }
}

// ---- 5.1 worker logic ----

#[tokio::test(start_paused = true)]
async fn commands_run_in_fifo_order() {
    let mut rig = up();
    for n in 1..=3 {
        rig.link.send(resolve(n, &format!("/work/{n}")));
    }
    for n in 1..=3 {
        match rig.answer().await {
            MemReply::Project { pane: p, result } => {
                assert_eq!(p, pane(n));
                assert_eq!(result, Ok("widget".to_string()));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(rig.fake.count("GET /health"), 1, "one probe, then cached");
}

#[tokio::test(start_paused = true)]
async fn the_first_ready_probe_is_announced_once() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.link.send(resolve(2, "/b"));
    match rig.next().await {
        MemReply::Health { state, offer } => {
            assert_eq!(
                state,
                Health::Ready {
                    instance_id: INSTANCE.into(),
                    version: "3.4.0".into()
                }
            );
            assert!(!offer);
        }
        other => panic!("{other:?}"),
    }
    rig.answer().await;
    rig.answer().await;
    assert!(rig.quiet(), "no second health announcement");
}

#[tokio::test(start_paused = true)]
async fn a_session_is_registered_once_then_notes_are_saved() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let first = rig.answer().await;
    let session = saved_session(&first);
    match &first {
        MemReply::Saved {
            observation_id,
            topic_key,
            title,
            ..
        } => {
            assert_eq!(*observation_id, 1);
            assert_eq!(topic_key, "chat/decision/use-axum");
            assert_eq!(title, "Use Axum");
        }
        other => panic!("{other:?}"),
    }
    rig.link.send(save(1, Some(&session)));
    assert_eq!(saved_session(&rig.answer().await), session);
    let keys = rig.fake.keys();
    assert_eq!(
        keys.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "GET /health",
            "POST /sessions",
            "POST /observations",
            "POST /observations"
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn an_already_ended_session_counts_as_registered() {
    let mut rig = up();
    rig.fake.on(
        "POST /sessions",
        reply(fx!("session_reregister_after_end_409")),
    );
    rig.link.send(save(1, None));
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
}

#[tokio::test(start_paused = true)]
async fn a_failed_registration_is_a_short_save_failure() {
    let mut rig = up();
    rig.fake.on(
        "POST /sessions",
        reply(fx!("observation_400_session_project_mismatch")),
    );
    rig.link.send(save(1, None));
    match rig.answer().await {
        MemReply::SaveFailed { pane: p, reason } => {
            assert_eq!(p, pane(1));
            assert!(reason.chars().count() <= 120);
            assert!(!reason.contains("Axum"), "no content in errors");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.fake.count("POST /observations"), 0);
}

#[tokio::test(start_paused = true)]
async fn a_changed_instance_clears_the_caches_and_registers_again() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.link.send(save(1, None));
    rig.answer().await;
    let session = saved_session(&rig.answer().await);
    // The server goes away and comes back as another instance.
    rig.fake.mode(Mode::Down);
    rig.link.send(resolve(1, "/b"));
    assert!(matches!(rig.answer().await, MemReply::Unavailable { .. }));
    rig.fake.mode(Mode::Up("0123456789abcdef".into()));
    tokio::time::sleep(REPROBE).await;
    rig.link.send(resolve(1, "/a"));
    rig.link.send(save(1, Some(&session)));
    rig.answer().await;
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
    assert_eq!(rig.fake.count("GET /project/current"), 3, "cache cleared");
    assert_eq!(rig.fake.count("POST /sessions"), 2, "registered again");
}

#[tokio::test(start_paused = true)]
async fn the_same_instance_after_an_outage_keeps_the_caches() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.link.send(save(1, None));
    rig.answer().await;
    let session = saved_session(&rig.answer().await);
    rig.fake.mode(Mode::Down);
    rig.link.send(resolve(1, "/b"));
    rig.answer().await;
    rig.fake.mode(Mode::Up(INSTANCE.into()));
    tokio::time::sleep(REPROBE).await;
    rig.link.send(resolve(1, "/a"));
    rig.link.send(save(1, Some(&session)));
    rig.answer().await;
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
    assert_eq!(
        rig.fake.count("GET /project/current"),
        2,
        "/a stayed cached"
    );
    assert_eq!(rig.fake.count("POST /sessions"), 1);
}

#[tokio::test(start_paused = true)]
async fn the_project_is_cached_per_cwd() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.link.send(resolve(2, "/a"));
    rig.link.send(resolve(3, "/b"));
    for _ in 0..3 {
        rig.answer().await;
    }
    assert_eq!(rig.fake.count("GET /project/current"), 2);
}

#[tokio::test(start_paused = true)]
async fn a_directory_that_is_not_git_is_not_a_project() {
    let mut rig = up();
    rig.fake.on(
        "GET /project/current",
        reply(fx!("project_dir_basename_200")),
    );
    rig.link.send(resolve(1, "/tmp"));
    match rig.answer().await {
        MemReply::Project { result, .. } => assert_eq!(result, Err(ProjectError::NotGit)),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_context_for_an_unknown_project_is_no_memory_yet() {
    let mut rig = up();
    rig.fake.on(
        "GET /context",
        Act::Reply(404, br#"{"code":"unknown_project","error":"x"}"#.to_vec()),
    );
    rig.link.send(MemCmd::Context {
        pane: pane(1),
        project: "widget".into(),
    });
    assert_eq!(
        rig.answer().await,
        MemReply::Context {
            pane: pane(1),
            block: None
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_context_is_returned_as_a_block() {
    let mut rig = up();
    rig.link.send(MemCmd::Context {
        pane: pane(1),
        project: "widget".into(),
    });
    match rig.answer().await {
        MemReply::Context { block, .. } => assert!(block.is_some()),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn deletes_are_counted_one_by_one() {
    let mut rig = up();
    rig.fake.on(
        "DELETE /observations/2",
        reply(fx!("delete_404_already_deleted")),
    );
    rig.fake
        .on("DELETE /observations/3", Act::Reply(500, b"{}".to_vec()));
    rig.link.send(MemCmd::Delete {
        pane: pane(1),
        notes: vec![
            ("widget".into(), 1),
            ("widget".into(), 2),
            ("widget".into(), 3),
        ],
    });
    assert_eq!(
        rig.answer().await,
        MemReply::Deleted {
            pane: pane(1),
            ok: 2,
            failed: 1,
            refused: false
        }
    );
}

#[tokio::test(start_paused = true)]
async fn a_refused_delete_is_reported_and_stops_the_rest() {
    let mut rig = up();
    rig.fake
        .on("DELETE /observations", reply(fx!("delete_401_wrong_token")));
    rig.link.send(MemCmd::Delete {
        pane: pane(1),
        notes: vec![("widget".into(), 1), ("widget".into(), 2)],
    });
    assert_eq!(
        rig.answer().await,
        MemReply::Deleted {
            pane: pane(1),
            ok: 0,
            failed: 2,
            refused: true
        }
    );
    assert_eq!(rig.fake.count("DELETE"), 1, "stopped at the refusal");
}

// ---- degrade ----

#[tokio::test(start_paused = true)]
async fn a_down_server_answers_unavailable_at_once_and_probes_every_10_seconds() {
    let mut rig = rig(Mode::Down);
    let start = Instant::now();
    rig.link.send(resolve(1, "/a"));
    match rig.next().await {
        MemReply::Health { state, offer } => {
            assert_eq!(state, Health::Down);
            assert!(offer, "nothing answers: engram serve may be offered");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::NotRunning
        }
    );
    rig.link.send(resolve(2, "/a"));
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(2)),
            reason: Reason::NotRunning
        }
    );
    assert_eq!(Instant::now() - start, Duration::ZERO, "answered at once");
    assert_eq!(rig.fake.count("GET /health"), 1, "no probe yet");
    tokio::time::sleep(Duration::from_secs(9)).await;
    rig.link.send(resolve(3, "/a"));
    rig.next().await;
    assert_eq!(rig.fake.count("GET /health"), 1, "still inside 10 s");
    tokio::time::sleep(Duration::from_secs(1)).await;
    rig.link.send(resolve(4, "/a"));
    rig.next().await;
    assert_eq!(rig.fake.count("GET /health"), 2, "re-probed after 10 s");
    assert!(rig.quiet(), "still down: no repeated health announcement");
    rig.fake.mode(Mode::Up(INSTANCE.into()));
    tokio::time::sleep(REPROBE).await;
    rig.link.send(resolve(5, "/a"));
    assert!(matches!(rig.next().await, MemReply::Health { .. }));
    assert!(matches!(rig.next().await, MemReply::Project { .. }));
}

#[tokio::test(start_paused = true)]
async fn an_old_or_foreign_server_is_unavailable_with_its_reason() {
    let mut rig = rig(Mode::Old);
    rig.link.send(resolve(1, "/a"));
    match rig.next().await {
        MemReply::Health { state, offer } => {
            assert!(matches!(state, Health::Old { .. }));
            assert!(!offer, "the port is taken: no offer");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::OldServer { port: PORT }
        }
    );
    let mut rig = self::rig(Mode::Foreign);
    rig.link.send(resolve(1, "/a"));
    match rig.next().await {
        MemReply::Health { state, offer } => {
            assert_eq!(state, Health::Foreign);
            assert!(!offer);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::Foreign { port: PORT }
        }
    );
    assert_eq!(
        rig.fake.count("GET /project"),
        0,
        "nothing sent to a foreign port"
    );
}

#[tokio::test(start_paused = true)]
async fn a_transport_failure_after_ready_sets_down() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.answer().await;
    rig.fake.mode(Mode::Down);
    rig.fake
        .on("GET /project/current", Act::Fail(CallError::Connect));
    rig.link.send(resolve(2, "/b"));
    assert_eq!(
        rig.answer().await,
        MemReply::Unavailable {
            pane: Some(pane(2)),
            reason: Reason::NotRunning
        }
    );
    rig.link.send(resolve(3, "/c"));
    rig.answer().await;
    assert_eq!(
        rig.fake.count("GET /project/current"),
        2,
        "no call while down"
    );
}

#[tokio::test(start_paused = true)]
async fn a_full_queue_answers_busy_at_once() {
    let mut rig = up();
    rig.fake.on("GET /health", Act::Hang);
    for n in 0..QUEUE as u32 {
        rig.link.send(resolve(n + 1, "/a"));
    }
    assert!(rig.quiet(), "64 fit");
    rig.link.send(resolve(99, "/a"));
    assert_eq!(
        rig.rx.try_recv().unwrap(),
        MemReply::Unavailable {
            pane: Some(pane(99)),
            reason: Reason::Busy
        }
    );
}

// ---- timeouts per call class ----

#[tokio::test(start_paused = true)]
async fn each_call_class_has_its_own_limit() {
    let mut rig = up();
    rig.link.send(resolve(1, "/a"));
    rig.link.send(MemCmd::Context {
        pane: pane(1),
        project: "widget".into(),
    });
    rig.link.send(save(1, None));
    rig.link.send(MemCmd::Delete {
        pane: pane(1),
        notes: vec![("widget".into(), 1)],
    });
    for _ in 0..4 {
        rig.answer().await;
    }
    let ms = Duration::from_millis;
    assert_eq!(rig.fake.total_of("GET /health"), ms(300));
    assert_eq!(rig.fake.total_of("GET /project/current"), ms(800));
    assert_eq!(rig.fake.total_of("GET /context"), ms(800));
    assert_eq!(rig.fake.total_of("POST /sessions"), ms(2000));
    assert_eq!(rig.fake.total_of("POST /observations"), ms(2000));
    assert_eq!(rig.fake.total_of("DELETE"), ms(2000));
}

#[tokio::test(start_paused = true)]
async fn a_silent_health_probe_gives_up_at_300_ms() {
    let mut rig = up();
    rig.fake.on("GET /health", Act::Hang);
    let start = Instant::now();
    rig.link.send(resolve(1, "/a"));
    assert_eq!(
        rig.answer().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::NoAnswer
        }
    );
    assert_eq!(Instant::now() - start, Duration::from_millis(300));
}

#[tokio::test(start_paused = true)]
async fn a_silent_read_gives_up_at_800_ms_and_a_silent_write_at_2_s() {
    let mut rig = up();
    rig.fake.on("GET /project/current", Act::Hang);
    let start = Instant::now();
    rig.link.send(resolve(1, "/a"));
    assert!(matches!(rig.answer().await, MemReply::Unavailable { .. }));
    assert_eq!(Instant::now() - start, Duration::from_millis(800));

    let mut rig = up();
    rig.fake.on("POST /observations", Act::Hang);
    rig.link.send(save(1, None));
    let start = Instant::now();
    // The health probe and registration are instant; the save waits 2 s.
    assert!(matches!(
        rig.answer().await,
        MemReply::Unavailable {
            reason: Reason::NoAnswer,
            ..
        }
    ));
    assert_eq!(Instant::now() - start, Duration::from_secs(2));
}

// ---- panics ----

#[tokio::test(start_paused = true)]
async fn a_panic_while_handling_still_gives_a_reply_and_the_worker_lives() {
    let mut rig = up();
    rig.fake.on("GET /project/current", Act::Panic);
    rig.link.send(resolve(1, "/a"));
    rig.link.send(MemCmd::Context {
        pane: pane(2),
        project: "widget".into(),
    });
    assert_eq!(
        rig.answer().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::NoAnswer
        }
    );
    assert!(matches!(rig.answer().await, MemReply::Context { .. }));
}

// ---- 5.3 shutdown ----

#[tokio::test(start_paused = true)]
async fn quit_flushes_a_pending_end() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.link.send(MemCmd::End {
        session: session.clone(),
        project: "widget".into(),
        summary: "Decided Axum.".into(),
    });
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert_eq!(rig.fake.count(&format!("POST /sessions/{session}/end")), 1);
}

#[tokio::test(start_paused = true)]
async fn an_end_is_sent_once_and_only_for_a_registered_session() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    let end = |s: &str| MemCmd::End {
        session: s.into(),
        project: "widget".into(),
        summary: "x".into(),
    };
    rig.link.send(end(&session));
    rig.link.send(end(&session));
    rig.link
        .send(end("lazarobox-00000000000000000000000000000000"));
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert_eq!(rig.fake.count("POST /sessions/"), 1);
}

#[tokio::test(start_paused = true)]
async fn quit_against_a_server_that_never_answers_is_bounded_by_one_second() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.fake.on("POST /sessions/", Act::Hang);
    rig.link.send(MemCmd::End {
        session,
        project: "widget".into(),
        summary: "x".into(),
    });
    let start = Instant::now();
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert_eq!(Instant::now() - start, Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn quit_with_a_full_queue_is_still_bounded() {
    let rig = up();
    rig.fake.on("GET /health", Act::Hang);
    for n in 0..QUEUE as u32 + 1 {
        rig.link.send(resolve(n, "/a"));
    }
    let start = Instant::now();
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert!(Instant::now() - start <= Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn a_silent_server_never_blocks_the_caller() {
    let rig = up();
    rig.fake.on("GET /health", Act::Hang);
    let start = Instant::now();
    for n in 0..10 {
        rig.link.send(resolve(n, "/a"));
    }
    // `send` is synchronous: no await point, so no time can pass.
    assert_eq!(Instant::now() - start, Duration::ZERO);
}
