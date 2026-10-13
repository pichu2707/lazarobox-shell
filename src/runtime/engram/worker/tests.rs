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
const CWD: &str = "/work/widget";
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
    body: String,
}

struct State {
    mode: Mode,
    overrides: Vec<(String, Act)>,
    /// Acts used by the next matching call only.
    once: Vec<(String, Act)>,
    seen: Vec<Seen>,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn new(mode: Mode) -> Self {
        Self(Arc::new(Mutex::new(State {
            mode,
            overrides: Vec::new(),
            once: Vec::new(),
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

    /// The next call whose `METHOD target` starts with `prefix` gets `act`.
    fn once(&self, prefix: &str, act: Act) {
        self.0.lock().unwrap().once.push((prefix.into(), act));
    }

    /// The body of the `nth` (from 0) call that starts with `prefix`.
    fn body_of(&self, prefix: &str, nth: usize) -> String {
        let state = self.0.lock().unwrap();
        state
            .seen
            .iter()
            .filter(|s| s.key.starts_with(prefix))
            .nth(nth)
            .unwrap_or_else(|| panic!("no call {prefix} #{nth}"))
            .body
            .clone()
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
                body: String::from_utf8_lossy(call.body.as_deref().unwrap_or_default())
                    .into_owned(),
            });
            let first = state.once.iter().position(|(p, _)| key.starts_with(p));
            let over = first.map(|i| state.once.remove(i).1).or_else(|| {
                state
                    .overrides
                    .iter()
                    .rev()
                    .find(|(prefix, _)| key.starts_with(prefix))
                    .map(|(_, act)| act.clone())
            });
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
        cwd: CWD.into(),
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

#[cfg(target_os = "linux")]
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

// ---- review fixes ----

fn directory_sent(rig: &Rig, nth: usize) -> serde_json::Value {
    let body = rig.fake.body_of("POST /sessions", nth);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    value["directory"].clone()
}

// W1: the session record carries the real cwd of the chat, never a guess.
#[tokio::test(start_paused = true)]
async fn a_session_registers_with_the_cwd_of_the_chat() {
    let mut rig = up();
    rig.link.send(save(1, None));
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
    assert_eq!(directory_sent(&rig, 0), CWD);
    assert_eq!(
        rig.fake.count("GET /project/current"),
        0,
        "no project lookup"
    );
}

#[tokio::test(start_paused = true)]
async fn the_cwd_is_sent_even_after_the_project_cache_was_cleared() {
    let mut rig = up();
    rig.link.send(resolve(1, "/other"));
    rig.answer().await;
    rig.fake.mode(Mode::Down);
    rig.link.send(resolve(1, "/b"));
    rig.answer().await;
    rig.fake.mode(Mode::Up("0123456789abcdef".into()));
    tokio::time::sleep(REPROBE).await;
    rig.link.send(save(1, None));
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
    assert_eq!(directory_sent(&rig, 0), CWD);
}

// W2: a save that finds its session gone registers it again and retries once.
#[tokio::test(start_paused = true)]
async fn a_save_into_a_session_the_server_lost_registers_again_and_retries_once() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.fake.once(
        "POST /observations",
        reply(fx!("save_unregistered_session_404")),
    );
    rig.link.send(save(1, Some(&session)));
    assert_eq!(saved_session(&rig.answer().await), session);
    assert_eq!(rig.fake.count("POST /sessions"), 2, "registered again");
    assert_eq!(rig.fake.count("POST /observations"), 3, "one retry");
    assert_eq!(directory_sent(&rig, 1), CWD);
}

#[tokio::test(start_paused = true)]
async fn a_second_session_not_found_is_a_save_failure_not_a_loop() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    for _ in 0..2 {
        rig.fake.once(
            "POST /observations",
            reply(fx!("save_unregistered_session_404")),
        );
    }
    rig.link.send(save(1, Some(&session)));
    match rig.answer().await {
        MemReply::SaveFailed { pane: p, reason } => {
            assert_eq!(p, pane(1));
            assert!(!reason.contains("Axum"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.fake.count("POST /sessions"), 2);
    assert_eq!(rig.fake.count("POST /observations"), 3, "one retry only");
}

#[tokio::test(start_paused = true)]
async fn another_404_is_not_taken_for_a_lost_session() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.fake
        .once("POST /observations", Act::Reply(404, b"{}".to_vec()));
    rig.link.send(save(1, Some(&session)));
    assert!(matches!(rig.answer().await, MemReply::SaveFailed { .. }));
    assert_eq!(rig.fake.count("POST /sessions"), 1, "no new registration");
    assert_eq!(rig.fake.count("POST /observations"), 2);
}

// W4: only a body that cannot be read is no memory.
#[tokio::test(start_paused = true)]
async fn a_context_server_error_is_unavailable_and_degrades_the_health() {
    let mut rig = up();
    rig.fake
        .on("GET /context", Act::Reply(500, b"boom".to_vec()));
    let context = |n| MemCmd::Context {
        pane: pane(n),
        project: "widget".into(),
    };
    rig.link.send(context(1));
    assert!(matches!(rig.next().await, MemReply::Health { .. }));
    assert_eq!(
        rig.next().await,
        MemReply::Health {
            state: Health::Down,
            offer: false
        }
    );
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::NoAnswer
        }
    );
    rig.link.send(context(2));
    assert!(matches!(rig.answer().await, MemReply::Unavailable { .. }));
    assert_eq!(
        rig.fake.count("GET /context"),
        1,
        "down is trusted for 10 s"
    );
}

#[tokio::test(start_paused = true)]
async fn any_non_200_context_other_than_unknown_project_is_unavailable() {
    for (status, body) in [(404, &br#"{"error":"nope"}"#[..]), (401, b"{}"), (503, b"")] {
        let mut rig = up();
        rig.fake
            .on("GET /context", Act::Reply(status, body.to_vec()));
        rig.link.send(MemCmd::Context {
            pane: pane(1),
            project: "widget".into(),
        });
        assert_eq!(
            rig.answer().await,
            MemReply::Unavailable {
                pane: Some(pane(1)),
                reason: Reason::NoAnswer
            },
            "{status}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_200_context_that_cannot_be_read_is_no_memory_and_stays_healthy() {
    let mut rig = up();
    rig.fake
        .on("GET /context", Act::Reply(200, b"<html>".to_vec()));
    for n in 1..=2 {
        rig.link.send(MemCmd::Context {
            pane: pane(n),
            project: "widget".into(),
        });
        assert_eq!(
            rig.answer().await,
            MemReply::Context {
                pane: pane(n),
                block: None
            }
        );
    }
    assert_eq!(rig.fake.count("GET /context"), 2, "still healthy");
}

// S2: a panic keeps the registered sessions, so their End still goes out.
#[tokio::test(start_paused = true)]
async fn an_end_still_goes_out_after_a_panic() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.fake.once("GET /project/current", Act::Panic);
    rig.link.send(resolve(1, "/a"));
    assert!(matches!(rig.answer().await, MemReply::Unavailable { .. }));
    rig.link.send(MemCmd::End {
        session: session.clone(),
        project: "widget".into(),
        summary: "x".into(),
    });
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert_eq!(rig.fake.count(&format!("POST /sessions/{session}/end")), 1);
}

// S2: but a new instance after a panic still clears the sessions.
#[tokio::test(start_paused = true)]
async fn a_new_instance_after_a_panic_still_clears_the_sessions() {
    let mut rig = up();
    rig.link.send(save(1, None));
    let session = saved_session(&rig.answer().await);
    rig.fake.once("GET /project/current", Act::Panic);
    rig.link.send(resolve(1, "/a"));
    rig.answer().await;
    rig.fake.mode(Mode::Up("0123456789abcdef".into()));
    rig.link.send(save(1, Some(&session)));
    assert!(matches!(rig.answer().await, MemReply::Saved { .. }));
    assert_eq!(rig.fake.count("POST /sessions"), 2, "registered anew");
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

// ---- 7.4 the engram serve offer and start ----

use std::{io, path::Path};

use tokio::sync::oneshot;

use crate::{
    core::ai::memory::{
        offer::{Binary, Offer},
        port::ServerOutcome,
    },
    runtime::engram::serve::{BinaryChanged, Launcher, Running},
};

const BIN: &str = "/opt/engram/bin/engram";

/// When the fake child "exits".
#[derive(Clone, Copy)]
enum Exit {
    Never,
    Now,
}

struct Launched {
    binary: Binary,
    finds: usize,
    spawns: Vec<(String, u16)>,
    fail_spawn: bool,
    /// The launcher refuses: the binary is no longer the file offered.
    changed: bool,
    /// The OS error the spawn fails with, when set (else a custom one).
    errno: Option<i32>,
    exit: Exit,
    log: Option<String>,
    on_spawn: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Keeps the exit senders alive, as the real reaper task does.
    held: Vec<oneshot::Sender<()>>,
}

#[derive(Clone)]
struct Launch(Arc<Mutex<Launched>>);

impl Launch {
    fn new(binary: Binary) -> Self {
        Self(Arc::new(Mutex::new(Launched {
            binary,
            finds: 0,
            spawns: Vec::new(),
            fail_spawn: false,
            changed: false,
            errno: None,
            exit: Exit::Never,
            log: None,
            on_spawn: None,
            held: Vec::new(),
        })))
    }

    fn with(self, change: impl FnOnce(&mut Launched)) -> Self {
        change(&mut self.0.lock().unwrap());
        self
    }

    fn finds(&self) -> usize {
        self.0.lock().unwrap().finds
    }

    fn spawns(&self) -> Vec<(String, u16)> {
        self.0.lock().unwrap().spawns.clone()
    }
}

impl Launcher for Launch {
    async fn find(&self) -> Binary {
        let mut state = self.0.lock().unwrap();
        state.finds += 1;
        state.binary.clone()
    }

    async fn spawn(&self, binary: &Path, port: u16) -> io::Result<Running> {
        let (hook, running) = {
            let mut state = self.0.lock().unwrap();
            state
                .spawns
                .push((binary.to_string_lossy().into_owned(), port));
            if state.changed {
                return Err(io::Error::other(BinaryChanged));
            }
            if let Some(code) = state.errno {
                return Err(io::Error::from_raw_os_error(code));
            }
            if state.fail_spawn {
                return Err(io::Error::other("no such file"));
            }
            let (tx, exit) = oneshot::channel();
            match state.exit {
                Exit::Now => {
                    let _ = tx.send(());
                }
                Exit::Never => state.held.push(tx),
            }
            (
                state.on_spawn.clone(),
                Running {
                    pid: 4242,
                    exit,
                    log_from: 0,
                },
            )
        };
        if let Some(hook) = hook {
            hook();
        }
        Ok(running)
    }

    async fn log_line(&self, _from: u64) -> Option<String> {
        self.0.lock().unwrap().log.clone()
    }
}

fn found(version: &str) -> Binary {
    Binary::Found {
        path: BIN.into(),
        version: Some(version.into()),
    }
}

fn serve_rig(mode: Mode, launch: &Launch) -> Rig {
    let fake = Fake::new(mode);
    let (tx, rx) = mpsc::channel(256);
    let link = spawn_with(fake.clone(), launch.clone(), PORT, tx);
    Rig { link, rx, fake }
}

fn the_offer() -> Offer {
    Offer {
        path: BIN.into(),
        port: PORT,
    }
}

/// Gets the offer the way the app does: a resolve finds nothing running.
async fn take_the_offer(rig: &mut Rig) {
    rig.link.send(resolve(1, "/a"));
    assert_eq!(rig.answer().await, MemReply::ServerOffer(the_offer()));
    rig.next().await; // the Unavailable of the resolve that found it down
}

fn unavailable_global(reason: Reason) -> MemReply {
    MemReply::Unavailable { pane: None, reason }
}

#[tokio::test(start_paused = true)]
async fn a_down_server_with_a_3x_binary_gets_one_offer_with_the_absolute_path() {
    let launch = Launch::new(found("engram 3.4.0"));
    let mut rig = serve_rig(Mode::Down, &launch);
    rig.link.send(resolve(1, "/a"));
    assert!(matches!(
        rig.next().await,
        MemReply::Health {
            state: Health::Down,
            offer: true
        }
    ));
    assert_eq!(rig.next().await, MemReply::ServerOffer(the_offer()));
    assert_eq!(
        rig.next().await,
        MemReply::Unavailable {
            pane: Some(pane(1)),
            reason: Reason::NotRunning
        }
    );
    assert!(rig.fake.keys().iter().all(|k| k.starts_with("GET /health")));
    // Down again 10 s later (a Ready cycle in between): no second offer.
    rig.fake.mode(Mode::Up(INSTANCE.into()));
    tokio::time::sleep(REPROBE).await;
    rig.link.send(resolve(2, "/a"));
    rig.answer().await;
    rig.fake.mode(Mode::Down);
    rig.fake.on("GET /project", Act::Fail(CallError::Connect));
    rig.link.send(resolve(3, "/b"));
    loop {
        let reply = rig.next().await;
        assert!(!matches!(reply, MemReply::ServerOffer(_)), "second offer");
        if matches!(reply, MemReply::Unavailable { .. }) {
            break;
        }
    }
    assert_eq!(launch.finds(), 1, "the binary is looked up once per run");
    assert!(launch.spawns().is_empty(), "an offer spawns nothing");
}

#[tokio::test(start_paused = true)]
async fn a_missing_or_1x_binary_is_told_once_and_never_offered() {
    for (binary, reason) in [
        (Binary::Missing, Reason::NoBinary),
        (found("engram 1.20.0"), Reason::OldBinary),
    ] {
        let launch = Launch::new(binary);
        let mut rig = serve_rig(Mode::Down, &launch);
        rig.link.send(resolve(1, "/a"));
        assert!(matches!(rig.next().await, MemReply::Health { .. }));
        assert_eq!(rig.next().await, unavailable_global(reason.clone()));
        assert_eq!(
            rig.next().await,
            MemReply::Unavailable {
                pane: Some(pane(1)),
                reason: Reason::NotRunning
            }
        );
        assert!(rig.quiet());
    }
}

#[tokio::test(start_paused = true)]
async fn old_foreign_and_silent_servers_get_no_offer_and_no_binary_lookup() {
    let launch = Launch::new(found("engram 3.4.0"));
    for mode in [Mode::Old, Mode::Foreign] {
        let mut rig = serve_rig(mode, &launch);
        rig.link.send(resolve(1, "/a"));
        while let Some(reply) = tokio::time::timeout(Duration::from_secs(1), rig.rx.recv())
            .await
            .ok()
            .flatten()
        {
            assert!(!matches!(reply, MemReply::ServerOffer(_)), "{reply:?}");
        }
    }
    // A probe that times out is a busy server, not a missing one.
    let mut rig = serve_rig(Mode::Down, &launch);
    rig.fake.on("GET /health", Act::Hang);
    rig.link.send(resolve(1, "/a"));
    while let Some(reply) = tokio::time::timeout(Duration::from_secs(1), rig.rx.recv())
        .await
        .ok()
        .flatten()
    {
        assert!(!matches!(reply, MemReply::ServerOffer(_)), "{reply:?}");
    }
    assert_eq!(launch.finds(), 0);
}

fn down_up_after(fake: &Fake, after: Duration) -> Arc<dyn Fn() + Send + Sync> {
    let fake = fake.clone();
    Arc::new(move || {
        let fake = fake.clone();
        if after.is_zero() {
            fake.mode(Mode::Up(INSTANCE.into()));
            return;
        }
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            fake.mode(Mode::Up(INSTANCE.into()));
        });
    })
}

#[tokio::test(start_paused = true)]
async fn a_ready_server_at_the_re_probe_is_adopted_with_zero_spawns() {
    let launch = Launch::new(found("engram 3.4.0"));
    let mut rig = serve_rig(Mode::Up(INSTANCE.into()), &launch);
    rig.link.send(MemCmd::StartServer);
    assert_eq!(rig.answer().await, MemReply::Server(ServerOutcome::Adopted));
    assert!(launch.spawns().is_empty());
    assert_eq!(launch.finds(), 0, "no need to look for a binary");
}

#[tokio::test(start_paused = true)]
async fn the_confirmed_binary_is_spawned_once_with_the_port_and_reported_with_its_pid() {
    let launch = Launch::new(found("engram 3.4.0"));
    let mut rig = serve_rig(Mode::Down, &launch);
    launch
        .0
        .lock()
        .unwrap()
        .on_spawn
        .replace(down_up_after(&rig.fake, Duration::from_millis(250)));
    rig.link.send(resolve(1, "/a"));
    assert_eq!(rig.answer().await, MemReply::ServerOffer(the_offer()));
    rig.next().await; // the Unavailable of the resolve that found it down
    let start = Instant::now();
    rig.link.send(MemCmd::StartServer);
    assert_eq!(
        rig.answer().await,
        MemReply::Server(ServerOutcome::Started { pid: 4242 })
    );
    let took = Instant::now() - start;
    assert!(
        took >= Duration::from_millis(250) && took <= Duration::from_millis(500),
        "polled every 100 ms until it answered: {took:?}"
    );
    assert_eq!(launch.spawns(), [(BIN.to_string(), PORT)]);
    // The worker now knows the server is ready: no second probe is needed.
    rig.link.send(resolve(2, "/a"));
    assert!(matches!(rig.answer().await, MemReply::Project { .. }));
    // Quit leaves the server alone and spawns nothing more.
    rig.link.shutdown(Duration::from_secs(1)).await;
    assert_eq!(launch.spawns().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn losing_the_port_race_adopts_the_winner_without_a_second_spawn() {
    let launch = Launch::new(found("engram 3.4.0")).with(|l| l.exit = Exit::Now);
    let mut rig = serve_rig(Mode::Down, &launch);
    launch
        .0
        .lock()
        .unwrap()
        .on_spawn
        .replace(down_up_after(&rig.fake, Duration::ZERO));
    take_the_offer(&mut rig).await;
    rig.link.send(MemCmd::StartServer);
    assert_eq!(rig.answer().await, MemReply::Server(ServerOutcome::Adopted));
    assert_eq!(launch.spawns().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_child_that_exits_with_nothing_answering_fails_with_its_last_log_line() {
    let launch = Launch::new(found("engram 3.4.0")).with(|l| {
        l.exit = Exit::Now;
        l.log = Some("fatal: cannot open database".into());
    });
    let mut rig = serve_rig(Mode::Down, &launch);
    take_the_offer(&mut rig).await;
    rig.link.send(MemCmd::StartServer);
    let reply = rig.answer().await;
    assert_eq!(
        reply,
        MemReply::Server(ServerOutcome::Failed(Reason::StartFailed(Some(
            "fatal: cannot open database".into()
        ))))
    );
    // No further spawn in this run, however often it is asked.
    rig.link.send(MemCmd::StartServer);
    assert!(matches!(
        rig.answer().await,
        MemReply::Server(ServerOutcome::Failed(_))
    ));
    assert_eq!(launch.spawns().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_child_that_stays_up_but_never_answers_gives_up_after_3_seconds() {
    let launch = Launch::new(found("engram 3.4.0")).with(|l| l.log = None);
    let mut rig = serve_rig(Mode::Down, &launch);
    let start = Instant::now();
    take_the_offer(&mut rig).await;
    rig.link.send(MemCmd::StartServer);
    assert_eq!(
        rig.answer().await,
        MemReply::Server(ServerOutcome::Failed(Reason::StartFailed(None)))
    );
    let took = Instant::now() - start;
    assert!(
        took >= Duration::from_secs(3) && took < Duration::from_millis(3300),
        "{took:?}"
    );
    let probes = rig.fake.count("GET /health");
    assert!(
        (28..=33).contains(&probes),
        "one probe per 100 ms: {probes}"
    );
    assert_eq!(launch.spawns().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_spawn_that_fails_is_a_failure_and_not_retried() {
    let launch = Launch::new(found("engram 3.4.0")).with(|l| l.fail_spawn = true);
    let mut rig = serve_rig(Mode::Down, &launch);
    take_the_offer(&mut rig).await;
    rig.link.send(MemCmd::StartServer);
    assert_eq!(
        rig.answer().await,
        MemReply::Server(ServerOutcome::Failed(Reason::StartFailed(Some(
            "no such file".into()
        ))))
    );
    rig.link.send(MemCmd::StartServer);
    rig.answer().await;
    assert_eq!(launch.spawns().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_spawn_refused_by_the_os_says_why_in_a_few_words() {
    let launch = Launch::new(found("engram 3.4.0")).with(|l| l.errno = Some(libc::EACCES));
    let mut rig = serve_rig(Mode::Down, &launch);
    take_the_offer(&mut rig).await;
    rig.link.send(MemCmd::StartServer);
    let MemReply::Server(ServerOutcome::Failed(reason)) = rig.answer().await else {
        panic!("not a failed start");
    };
    assert_eq!(
        reason.line(),
        "memory unavailable: could not start engram: permission denied"
    );
}

#[tokio::test(start_paused = true)]
async fn an_old_or_foreign_server_at_the_re_probe_is_a_failure_without_a_spawn() {
    for (mode, reason) in [
        (Mode::Old, Reason::OldServer { port: PORT }),
        (Mode::Foreign, Reason::Foreign { port: PORT }),
    ] {
        let launch = Launch::new(found("engram 3.4.0"));
        let mut rig = serve_rig(mode, &launch);
        rig.link.send(MemCmd::StartServer);
        assert_eq!(
            rig.answer().await,
            MemReply::Server(ServerOutcome::Failed(reason))
        );
        assert!(launch.spawns().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn a_start_without_an_offer_spawns_nothing_and_looks_for_nothing() {
    // Only the absolute path the user was shown may run: with no offer there
    // is nothing to run, whatever the PATH holds now.
    for binary in [
        found("engram 3.4.0"),
        Binary::Missing,
        found("engram 1.20.0"),
        Binary::NotChecked,
    ] {
        let launch = Launch::new(binary);
        let mut rig = serve_rig(Mode::Down, &launch);
        rig.link.send(MemCmd::StartServer);
        assert_eq!(
            rig.answer().await,
            MemReply::Server(ServerOutcome::Failed(Reason::StartFailed(Some(
                "nothing was offered".into()
            ))))
        );
        assert!(launch.spawns().is_empty());
        assert_eq!(launch.finds(), 0, "the PATH is not consulted again");
    }
}

#[tokio::test(start_paused = true)]
async fn a_binary_that_changed_since_the_offer_is_not_started() {
    let launch = Launch::new(found("engram 3.4.0"));
    let mut rig = serve_rig(Mode::Down, &launch);
    rig.link.send(resolve(1, "/a"));
    assert_eq!(rig.answer().await, MemReply::ServerOffer(the_offer()));
    rig.next().await; // the Unavailable of the resolve that found it down
    launch.0.lock().unwrap().changed = true;
    rig.link.send(MemCmd::StartServer);
    // The refusal is the launcher's; the worker reports it as `Changed`.
    assert_eq!(
        rig.answer().await,
        MemReply::Server(ServerOutcome::Failed(Reason::Changed))
    );
    assert_eq!(launch.spawns(), [(BIN.to_string(), PORT)]);
}

#[tokio::test(start_paused = true)]
async fn the_binary_the_user_confirmed_is_the_one_that_runs() {
    let launch = Launch::new(found("engram 3.4.0"));
    let mut rig = serve_rig(Mode::Down, &launch);
    rig.link.send(resolve(1, "/a"));
    assert_eq!(rig.answer().await, MemReply::ServerOffer(the_offer()));
    // The PATH changes after the question: the answer still means BIN.
    launch.0.lock().unwrap().binary = Binary::Found {
        path: "/tmp/evil/engram".into(),
        version: Some("engram 3.9.9".into()),
    };
    rig.link.send(MemCmd::StartServer);
    while !matches!(rig.answer().await, MemReply::Server(_)) {}
    assert_eq!(launch.spawns(), [(BIN.to_string(), PORT)]);
}

// ---- 7.4 end to end: the worker with real fake `engram` scripts ----

mod with_processes {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use super::*;
    use crate::runtime::engram::serve::Server;

    struct Setup {
        _dir: tempfile::TempDir,
        server: Server,
        script: PathBuf,
    }

    fn setup(body: &str) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let script = bin.join("engram");
        let text = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'engram 3.4.0'; exit 0; fi\n{body}\n"
        );
        fs::write(&script, text).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let server = Server::new(bin.into(), dir.path().join("state"));
        Setup {
            _dir: dir,
            server,
            script,
        }
    }

    fn kill(pid: u32) {
        // SAFETY: a plain signal to the fake server this test spawned.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }

    #[tokio::test]
    async fn a_script_that_exits_1_shows_could_not_start_engram_with_its_log_line() {
        let setup =
            setup("echo \"\x1b[31mlisten tcp :9100: bind: address already in use\" >&2; exit 1");
        let fake = Fake::new(Mode::Down);
        let (tx, mut rx) = mpsc::channel(16);
        let link = spawn_with(fake.clone(), setup.server, PORT, tx);
        link.send(resolve(1, "/a"));
        let offer = loop {
            match rx.recv().await.unwrap() {
                MemReply::ServerOffer(offer) => break offer,
                _ => continue,
            }
        };
        assert_eq!(offer.path, setup.script.to_string_lossy());
        link.send(MemCmd::StartServer);
        let outcome = loop {
            if let MemReply::Server(outcome) = rx.recv().await.unwrap() {
                break outcome;
            }
        };
        let ServerOutcome::Failed(reason) = outcome else {
            panic!("{outcome:?}");
        };
        // The escape sequence the script printed is gone from the line.
        assert_eq!(
            reason.line(),
            "memory unavailable: could not start engram: \
             listen tcp :9100: bind: address already in use"
        );
    }

    #[tokio::test]
    async fn a_script_that_stays_up_is_started_and_survives_the_worker() {
        let setup = setup("exec sleep 30");
        let fake = Fake::new(Mode::Down);
        let (tx, mut rx) = mpsc::channel(16);
        let link = spawn_with(fake.clone(), setup.server, PORT, tx);
        link.send(resolve(1, "/a"));
        link.send(MemCmd::StartServer);
        // The "server" starts answering a moment after the spawn.
        let flip = fake.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            flip.mode(Mode::Up(INSTANCE.into()));
        });
        let pid = loop {
            if let MemReply::Server(outcome) = rx.recv().await.unwrap() {
                let ServerOutcome::Started { pid } = outcome else {
                    panic!("{outcome:?}");
                };
                break pid;
            }
        };
        link.shutdown(Duration::from_secs(1)).await;
        drop(rx);
        tokio::time::sleep(Duration::from_millis(100)).await;
        // SAFETY: signal 0 only checks that the pid exists.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        kill(pid);
        assert!(alive, "quitting the worker must not end the server");
    }
}
