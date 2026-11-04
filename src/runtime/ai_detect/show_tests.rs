//! The Ollama tool-support probe against fake loopback servers.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;
use crate::core::ai::jail::{JailOff, JailSupport};

/// What the fake answers to one `POST /api/show`.
#[derive(Clone)]
enum Show {
    Body(String),
    Status(u16),
    Raw(Vec<u8>),
    Silent,
}

fn caps(list: &str) -> Show {
    Show::Body(format!(r#"{{"capabilities":[{list}]}}"#))
}

struct Seen {
    head: String,
    body: String,
}

struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    peak: Arc<AtomicUsize>,
}

impl Fake {
    fn posts(&self) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|s| s.head.starts_with("POST "))
            .map(|s| s.body.clone())
            .collect()
    }
}

fn tags(models: &[(String, &str, String)]) -> String {
    let items: Vec<String> = models
        .iter()
        .map(|(name, size, digest)| {
            format!(
                r#"{{"name":"{name}","digest":"{digest}","details":{{"parameter_size":"{size}"}}}}"#
            )
        })
        .collect();
    format!(r#"{{"models":[{}]}}"#, items.join(","))
}

fn http(status: u16, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.0 {status} X\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// An Ollama double: `GET /api/tags` lists `tags`, each `POST /api/show` is
/// answered by `show(model)` after `delay`.
async fn fake_ollama(
    tags: String,
    delay: Duration,
    show: impl Fn(&str) -> Show + Send + Sync + 'static,
) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let peak = Arc::new(AtomicUsize::new(0));
    let now = Arc::new(AtomicUsize::new(0));
    let show = Arc::new(show);
    let (seen_in, peak_in) = (seen.clone(), peak.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (seen, peak, now) = (seen_in.clone(), peak_in.clone(), now.clone());
            let (tags, show) = (tags.clone(), show.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                let head_end = loop {
                    if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let wanted = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while buf.len() < head_end + wanted {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
                seen.lock().unwrap().push(Seen {
                    head: head.clone(),
                    body: body.clone(),
                });
                let reply = if head.starts_with("GET /api/tags ") {
                    http(200, tags.as_bytes())
                } else if head.starts_with("POST /api/show ") {
                    let active = now.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(active, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    let model = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v["model"].as_str().map(str::to_owned))
                        .unwrap_or_default();
                    let answer = show(&model);
                    now.fetch_sub(1, Ordering::SeqCst);
                    match answer {
                        Show::Body(b) => http(200, b.as_bytes()),
                        Show::Status(code) => http(code, b"{}"),
                        Show::Raw(bytes) => bytes,
                        Show::Silent => std::future::pending().await,
                    }
                } else {
                    http(404, b"{}")
                };
                let _ = stream.write_all(&reply).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Fake { port, seen, peak }
}

fn env() -> (tempfile::TempDir, CliEnv) {
    let dir = tempfile::tempdir().unwrap();
    let mut env = CliEnv::from_env(JailSupport::Unavailable(JailOff::NotProbed));
    env.path = dir.path().as_os_str().to_owned();
    env.codex_home = None;
    (dir, env)
}

async fn detect(port: u16, show: &ShowCache) -> Detection {
    let (_dir, env) = env();
    let cache = ListingCache::new(Arc::new(Instant::now));
    let ports = ServerPorts::default().with("ollama", port);
    detect_all(&env, &cache, show, false, &ports, &[]).await
}

fn named(n: usize) -> Vec<(String, &'static str, String)> {
    (0..n)
        .map(|i| (format!("m{i}:8b"), "8.0B", format!("digest{i}")))
        .collect()
}

fn tools_of(d: &Detection, name: &str) -> Tools {
    d.facts("ollama", name).unwrap().tools
}

#[tokio::test]
async fn tools_declared_and_not_declared() {
    let models = vec![
        ("llama3:8b".to_owned(), "8.0B", "d1".to_owned()),
        ("gemma:2b".to_owned(), "2B", "d2".to_owned()),
    ];
    let fake = fake_ollama(tags(&models), Duration::ZERO, |m| match m {
        "llama3:8b" => caps(r#""completion","tools""#),
        _ => caps(r#""completion""#),
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(tools_of(&d, "llama3:8b"), Tools::Yes);
    assert_eq!(tools_of(&d, "gemma:2b"), Tools::No);
    assert_eq!(
        d.facts("ollama", "llama3:8b").unwrap().params,
        Some(8_000_000_000)
    );
}

#[tokio::test]
async fn an_old_ollama_without_capabilities_is_unknown_and_changes_nothing() {
    let fake = fake_ollama(tags(&named(2)), Duration::ZERO, |_| {
        Show::Body(include_str!("../../../tests/fixtures/ollama/api-show-old.json").into())
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Unknown);
    let ollama = d.engines.iter().find(|e| e.id == "ollama").unwrap();
    assert_eq!(ollama.status, Status::Available);
    assert_eq!(ollama.models, ["m0:8b", "m1:8b"]);
}

#[tokio::test]
async fn a_failing_probe_never_breaks_detection_nor_outlasts_the_bound() {
    let fake = fake_ollama(tags(&named(3)), Duration::ZERO, |m| match m {
        "m0:8b" => Show::Silent,
        "m1:8b" => Show::Status(500),
        _ => Show::Body("{not json".into()),
    })
    .await;
    let started = Instant::now();
    let d = detect(fake.port, &ShowCache::default()).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    for name in ["m0:8b", "m1:8b", "m2:8b"] {
        assert_eq!(tools_of(&d, name), Tools::Unknown, "{name}");
    }
    let ollama = d.engines.iter().find(|e| e.id == "ollama").unwrap();
    assert_eq!(ollama.status, Status::Available);
    assert_eq!(ollama.models.len(), 3);
}

#[tokio::test]
async fn an_oversized_reply_is_unknown() {
    let mut huge = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
    huge.resize(huge.len() + 1024 * 1024 + 1, b'x');
    let fake = fake_ollama(tags(&named(1)), Duration::ZERO, move |_| {
        Show::Raw(huge.clone())
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Unknown);
}

#[tokio::test]
async fn the_traffic_is_one_tags_get_and_one_show_post_per_model() {
    let fake = fake_ollama(tags(&named(2)), Duration::ZERO, |_| caps(r#""tools""#)).await;
    detect(fake.port, &ShowCache::default()).await;
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    let gets: Vec<_> = seen.iter().filter(|s| s.head.starts_with("GET ")).collect();
    assert_eq!(gets.len(), 1);
    assert!(gets[0].head.starts_with("GET /api/tags "));
    assert!(gets[0].body.is_empty());
    for s in seen.iter() {
        let lower = s.head.to_ascii_lowercase();
        assert!(
            !lower.contains("authorization") && !lower.contains("cookie"),
            "{}",
            s.head
        );
        assert!(s.head.contains("Host: 127.0.0.1:"), "{}", s.head);
    }
    assert!(
        seen.iter()
            .filter(|s| s.head.starts_with("POST "))
            .all(|s| s.head.starts_with("POST /api/show "))
    );
    drop(seen);
    let mut bodies = fake.posts();
    bodies.sort();
    assert_eq!(bodies, [r#"{"model":"m0:8b"}"#, r#"{"model":"m1:8b"}"#]);
}

#[tokio::test]
async fn the_post_request_is_exactly_the_documented_one() {
    let fake = fake_ollama(tags(&named(1)), Duration::ZERO, |_| caps(r#""tools""#)).await;
    http_post_show(fake.port, "m0:8b").await;
    let seen = fake.seen.lock().unwrap();
    let port = fake.port;
    assert_eq!(
        seen[0].head,
        format!(
            "POST /api/show HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n"
        )
    );
    assert_eq!(seen[0].body, r#"{"model":"m0:8b"}"#);
}

#[tokio::test]
async fn at_most_32_calls_and_4_at_once() {
    let fake = fake_ollama(tags(&named(40)), Duration::from_millis(40), |_| {
        caps(r#""tools""#)
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(fake.posts().len(), 32);
    assert!(
        fake.peak.load(Ordering::SeqCst) <= 4,
        "{}",
        fake.peak.load(Ordering::SeqCst)
    );
    assert!(
        fake.peak.load(Ordering::SeqCst) >= 2,
        "calls ran in parallel"
    );
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Yes);
    assert_eq!(tools_of(&d, "m31:8b"), Tools::Yes);
    assert_eq!(tools_of(&d, "m32:8b"), Tools::Unknown);
    assert_eq!(tools_of(&d, "m39:8b"), Tools::Unknown);
}

#[tokio::test]
async fn the_whole_phase_ends_at_the_two_second_deadline() {
    let fake = fake_ollama(tags(&named(12)), Duration::ZERO, |_| Show::Silent).await;
    let started = Instant::now();
    let d = detect(fake.port, &ShowCache::default()).await;
    let took = started.elapsed();
    // 12 silent calls, 4 at a time, 1 s each would take 3 s.
    assert!(
        took >= Duration::from_millis(1900) && took < Duration::from_secs(5),
        "{took:?}"
    );
    assert_eq!(tools_of(&d, "m11:8b"), Tools::Unknown);
}

#[tokio::test]
async fn an_unchanged_digest_is_not_probed_again_but_a_new_one_is() {
    let cache = ShowCache::default();
    let first = fake_ollama(tags(&named(2)), Duration::ZERO, |_| caps(r#""tools""#)).await;
    detect(first.port, &cache).await;
    assert_eq!(first.posts().len(), 2);
    // Same digests on the next run; m1 was pulled again (new digest).
    let mut next = named(2);
    next[1].2 = "digest-new".to_owned();
    let second = fake_ollama(tags(&next), Duration::ZERO, |_| caps(r#""completion""#)).await;
    let d = detect(second.port, &cache).await;
    assert_eq!(second.posts(), [r#"{"model":"m1:8b"}"#]);
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Yes, "from the cache");
    assert_eq!(tools_of(&d, "m1:8b"), Tools::No);
}

#[tokio::test]
async fn unknown_is_never_cached() {
    let cache = ShowCache::default();
    let broken = fake_ollama(tags(&named(1)), Duration::ZERO, |_| Show::Status(500)).await;
    detect(broken.port, &cache).await;
    let healed = fake_ollama(tags(&named(1)), Duration::ZERO, |_| caps(r#""tools""#)).await;
    let d = detect(healed.port, &cache).await;
    assert_eq!(healed.posts().len(), 1);
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Yes);
}

#[tokio::test]
async fn no_show_call_when_ollama_is_not_available() {
    let refused = crate::runtime::refused_port::RefusedPort::new();
    let d = detect(refused.port(), &ShowCache::default()).await;
    assert!(d.model_facts.is_empty());
    // A foreign service on the port: not Ollama, so no facts and no POST.
    let foreign = fake_ollama("{}".into(), Duration::ZERO, |_| caps(r#""tools""#)).await;
    let d = detect(foreign.port, &ShowCache::default()).await;
    assert!(d.model_facts.is_empty());
    assert!(foreign.posts().is_empty());
}

#[tokio::test]
async fn the_other_servers_never_get_a_post() {
    let ollama = fake_ollama(tags(&named(1)), Duration::ZERO, |_| caps(r#""tools""#)).await;
    let others = [
        fake_ollama(
            r#"{"data":[{"id":"m","owned_by":"llamacpp"}]}"#.into(),
            Duration::ZERO,
            |_| Show::Status(404),
        )
        .await,
        fake_ollama("{}".into(), Duration::ZERO, |_| Show::Status(404)).await,
        fake_ollama("{}".into(), Duration::ZERO, |_| Show::Status(404)).await,
    ];
    let (_dir, env) = env();
    let cache = ListingCache::new(Arc::new(Instant::now));
    let ports = ServerPorts::default()
        .with("ollama", ollama.port)
        .with("llamacpp", others[0].port)
        .with("lmstudio", others[1].port)
        .with("vllm", others[2].port);
    detect_all(&env, &cache, &ShowCache::default(), false, &ports, &[]).await;
    assert_eq!(ollama.posts().len(), 1);
    for other in &others {
        assert!(other.posts().is_empty());
        assert!(!other.seen.lock().unwrap().is_empty(), "it was probed");
    }
}

#[test]
fn the_cache_keeps_only_answers_and_clears_when_full() {
    let cache = ShowCache::default();
    cache.put("a", (Tools::Yes, Some(8192)));
    cache.put("b", (Tools::No, None));
    cache.put("c", (Tools::Unknown, Some(4096)));
    assert_eq!(cache.get("a"), Some((Tools::Yes, Some(8192))));
    assert_eq!(cache.get("b"), Some((Tools::No, None)));
    assert_eq!(cache.get("c"), None);
    for i in 0..256 {
        cache.put(&format!("k{i}"), (Tools::Yes, None));
    }
    // The 257th insert found it full: everything went, then it was stored.
    assert_eq!(cache.get("a"), None);
    assert_eq!(cache.get("k255"), Some((Tools::Yes, None)));
}

/// A llama.cpp double: lists a model, never answers `/props` (the window
/// probe then runs into its 800 ms timeout).
async fn slow_props_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if String::from_utf8_lossy(&buf[..n]).starts_with("GET /props") {
                    std::future::pending::<()>().await;
                }
                let body = r#"{"data":[{"id":"m","owned_by":"llamacpp"}]}"#;
                let _ = stream.write_all(&http(200, body.as_bytes())).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    port
}

/// A server that closes every connection at once, for the engines a test does
/// not run. Their default ports cannot stand in: under WSL's mirrored
/// networking a connect to a closed local port is not refused, it waits for
/// the probe's whole timeout.
async fn closing_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            drop(stream);
        }
    });
    port
}

#[tokio::test]
async fn a_hung_show_overlaps_the_other_slow_probes() {
    let ollama = fake_ollama(tags(&named(1)), Duration::ZERO, |_| Show::Silent).await;
    let llamacpp = slow_props_server().await;
    let closed = closing_server().await;
    let (_dir, env) = env();
    let cache = ListingCache::new(Arc::new(Instant::now));
    let ports = ServerPorts::default()
        .with("ollama", ollama.port)
        .with("llamacpp", llamacpp)
        .with("lmstudio", closed)
        .with("vllm", closed);
    let started = Instant::now();
    let d = detect_all(&env, &cache, &ShowCache::default(), false, &ports, &[]).await;
    let took = started.elapsed();
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Unknown);
    // 0.8 s of /props and 1 s of show: one after the other would be 1.8 s.
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_millis(1500),
        "{took:?}"
    );
}

fn with_window(arch: &str, length: u64) -> Show {
    Show::Body(format!(
        r#"{{"capabilities":["tools"],"model_info":{{"general.architecture":"{arch}","{arch}.context_length":{length}}}}}"#
    ))
}

fn window_of(d: &Detection, name: &str) -> Option<usize> {
    d.facts("ollama", name).unwrap().window
}

#[tokio::test]
async fn one_show_reply_fills_the_tools_and_the_window() {
    let fake = fake_ollama(tags(&named(2)), Duration::ZERO, |m| match m {
        "m0:8b" => with_window("llama", 131_072),
        _ => caps(r#""tools""#),
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(fake.posts().len(), 2, "no extra call for the window");
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Yes);
    assert_eq!(window_of(&d, "m0:8b"), Some(131_072));
    assert_eq!(d.model_window("ollama", "m0:8b"), Some(131_072));
    assert_eq!(window_of(&d, "m1:8b"), None, "no model_info is unknown");
    assert_eq!(d.model_window("ollama", "m1:8b"), None);
}

#[tokio::test]
async fn the_window_is_cached_by_digest_with_the_tools() {
    let cache = ShowCache::default();
    let first = fake_ollama(tags(&named(1)), Duration::ZERO, |_| {
        with_window("qwen3", 40_960)
    })
    .await;
    detect(first.port, &cache).await;
    let second = fake_ollama(tags(&named(1)), Duration::ZERO, |_| caps(r#""completion""#)).await;
    let d = detect(second.port, &cache).await;
    assert!(second.posts().is_empty(), "a cache hit makes no call");
    assert_eq!(window_of(&d, "m0:8b"), Some(40_960));
    assert_eq!(tools_of(&d, "m0:8b"), Tools::Yes);
}

#[tokio::test]
async fn a_failed_probe_leaves_the_window_unknown() {
    let fake = fake_ollama(tags(&named(1)), Duration::ZERO, |_| Show::Status(500)).await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(window_of(&d, "m0:8b"), None);
}

#[tokio::test]
async fn model_window_matches_the_latest_tag_like_the_other_lookups() {
    let models = vec![("tiny:latest".to_owned(), "1B", "dl".to_owned())];
    let fake = fake_ollama(tags(&models), Duration::ZERO, |_| {
        with_window("llama", 4_096)
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(d.model_window("ollama", "tiny"), Some(4_096));
    assert_eq!(d.model_window("ollama", "tiny:latest"), Some(4_096));
    assert_eq!(d.model_window("ollama", "other"), None);
}

#[tokio::test]
async fn a_finished_detection_feeds_the_windows_to_the_live_servers() {
    use crate::runtime::chat::LiveServers;
    let fake = fake_ollama(tags(&named(1)), Duration::ZERO, |_| {
        with_window("llama", 8_192)
    })
    .await;
    let live = LiveServers::new(ServerPorts::default().with("ollama", fake.port), None);
    let (_dir, env) = env();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    spawn_detection(
        tx,
        env,
        Arc::new(ListingCache::new(Arc::new(Instant::now))),
        Arc::new(ShowCache::default()),
        false,
        Arc::from(Vec::new()),
        live.clone(),
    );
    rx.recv().await.expect("a detection");
    let mut request = crate::core::ai::chat::engine::ChatRequest {
        engine: "ollama".into(),
        model: Some("m0:8b".into()),
        system: String::new(),
        memory: None,
        messages: Vec::new(),
        tools: crate::core::ai::chat::engine::Tools::Off,
        families: crate::core::ai::tools::registry::FamilySet::LEGACY,
        ollama: Default::default(),
        cache: Default::default(),
    };
    live.stamp(&mut request);
    assert_eq!(request.ollama.num_ctx, Some(8_192));
}

/// Runs one detection of `models` through `spawn_detection` with `cache`,
/// feeding `live`, and returns what the app would receive.
async fn rerun(live: &crate::runtime::chat::LiveServers, cache: &Arc<ShowCache>) -> Detection {
    let (_dir, env) = env();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    spawn_detection(
        tx,
        env,
        Arc::new(ListingCache::new(Arc::new(Instant::now))),
        cache.clone(),
        true,
        Arc::from(Vec::new()),
        live.clone(),
    );
    rx.recv().await.expect("a detection")
}

#[tokio::test]
async fn a_failed_probe_on_the_next_run_keeps_the_last_known_window() {
    use crate::runtime::chat::LiveServers;
    let good = fake_ollama(tags(&named(2)), Duration::ZERO, |_| {
        with_window("llama", 8_192)
    })
    .await;
    let live = LiveServers::new(ServerPorts::default().with("ollama", good.port), None);
    let first = rerun(&live, &Arc::new(ShowCache::default())).await;
    assert_eq!(window_of(&first, "m0:8b"), Some(8_192));
    // Same models, but every probe fails now and nothing is cached.
    let flaky = fake_ollama(tags(&named(2)), Duration::ZERO, |_| Show::Status(500)).await;
    live.set_ports(ServerPorts::default().with("ollama", flaky.port));
    let second = rerun(&live, &Arc::new(ShowCache::default())).await;
    assert_eq!(window_of(&second, "m0:8b"), Some(8_192), "the app keeps it");
    let mut request = crate::core::ai::chat::engine::ChatRequest {
        engine: "ollama".into(),
        model: Some("m0:8b".into()),
        system: String::new(),
        memory: None,
        messages: Vec::new(),
        tools: crate::core::ai::chat::engine::Tools::Off,
        families: crate::core::ai::tools::registry::FamilySet::LEGACY,
        ollama: Default::default(),
        cache: Default::default(),
    };
    live.stamp(&mut request);
    assert_eq!(request.ollama.num_ctx, Some(8_192), "so does the wire");
}

#[tokio::test]
async fn a_new_answer_replaces_the_kept_window_and_an_unlisted_model_drops_it() {
    use crate::runtime::chat::LiveServers;
    let one = fake_ollama(tags(&named(2)), Duration::ZERO, |_| {
        with_window("llama", 8_192)
    })
    .await;
    let live = LiveServers::new(ServerPorts::default().with("ollama", one.port), None);
    rerun(&live, &Arc::new(ShowCache::default())).await;
    // m1 is gone from the listing; m0 answers with a new window.
    let next = fake_ollama(tags(&named(1)), Duration::ZERO, |_| {
        with_window("llama", 4_096)
    })
    .await;
    live.set_ports(ServerPorts::default().with("ollama", next.port));
    let d = rerun(&live, &Arc::new(ShowCache::default())).await;
    assert_eq!(window_of(&d, "m0:8b"), Some(4_096));
    assert_eq!(live.ollama_windows(), [("m0:8b".to_owned(), 4_096)]);
}

#[tokio::test]
async fn bad_replies_leave_only_their_own_window_unknown() {
    // An oversized reply that starts like a good one: the cap rejects it whole.
    let mut huge = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
    huge.extend_from_slice(
        br#"{"capabilities":["tools"],"model_info":{"general.architecture":"llama","llama.context_length":8192}}"#,
    );
    huge.resize(huge.len() + 1024 * 1024 + 1, b' ');
    let fake = fake_ollama(tags(&named(6)), Duration::ZERO, move |m| match m {
        "m0:8b" => Show::Body("{not json".into()),
        "m1:8b" => with_window("llama", 0),
        "m2:8b" => with_window("llama", 99_999_999_999),
        "m3:8b" => Show::Raw(huge.clone()),
        "m4:8b" => Show::Body(
            r#"{"capabilities":["tools"],"model_info":{"general.architecture":"llama","llama.context_length":"8192"}}"#
                .into(),
        ),
        _ => with_window("llama", 8_192),
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    for bad in ["m0:8b", "m1:8b", "m2:8b", "m3:8b", "m4:8b"] {
        assert_eq!(window_of(&d, bad), None, "{bad}");
    }
    assert_eq!(
        window_of(&d, "m5:8b"),
        Some(8_192),
        "the others keep theirs"
    );
}

#[tokio::test]
async fn the_call_limit_leaves_the_rest_of_the_windows_unknown() {
    let fake = fake_ollama(tags(&named(40)), Duration::ZERO, |_| {
        with_window("llama", 8_192)
    })
    .await;
    let d = detect(fake.port, &ShowCache::default()).await;
    assert_eq!(fake.posts().len(), 32);
    assert_eq!(window_of(&d, "m0:8b"), Some(8_192));
    assert_eq!(window_of(&d, "m31:8b"), Some(8_192));
    assert_eq!(window_of(&d, "m32:8b"), None);
    assert_eq!(window_of(&d, "m39:8b"), None);
}
