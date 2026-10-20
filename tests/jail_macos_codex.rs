//! macOS only: a REAL codex, started the way a chat turn starts it (the codex
//! adapter, the macOS jail rules and the `__jail-exec` helper), must reach
//! OpenAI from inside the Seatbelt jail. There is no login, so the honest
//! outcome is an authentication failure from the server. A TLS, DNS or sandbox
//! error would mean the jail breaks codex.
//!
//! This is its own test binary because it sets `HOME` for the whole process
//! (the child inherits the environment), and `set_var` is only sound while no
//! other test thread runs.
//!
//! Skipped only when codex is not installed. With `LAZAROBOX_REQUIRE_JAIL=1`
//! a missing codex, or a system that cannot jail, FAILS instead.
//!
//! No real credential is used: `HOME` is a fresh empty folder, `OPENAI_API_KEY`
//! is removed and `CODEX_API_KEY` is a made-up value, so the server answers
//! 401 and the request has to cross the network to get that answer.

#![cfg(target_os = "macos")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use lazarobox_shell::{
    core::ai::{
        chat::engine::{ChatMessage, ChatRequest, Role, StreamEvent, Tools},
        jail::JailSupport,
    },
    runtime::{
        chat::{CliEnv, runner_with},
        jail::probe,
    },
};
use tokio::{sync::mpsc, time::timeout};

/// The key codex sees: not a credential, only a way to make it call the API.
const BOGUS_KEY: &str = "sk-lazarobox-smoke-not-a-real-key";

/// What an authentication failure looks like, from the server or from codex.
const AUTH: [&str; 8] = [
    "401",
    "unauthorized",
    "incorrect api key",
    "invalid api key",
    "not logged in",
    "login",
    "authentication",
    "api key",
];

/// What a broken jail looks like.
const BROKEN: [&str; 9] = [
    "operation not permitted",
    "sandbox_apply",
    "sandbox-exec",
    "certificate",
    "tls",
    "dns",
    "resolve",
    "error sending request",
    "could not be applied",
];

fn required() -> bool {
    std::env::var_os("LAZAROBOX_REQUIRE_JAIL").is_some_and(|v| v == "1")
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// `log stream` of the sandbox, written to a file while the turn runs.
fn watch_sandbox(log: &Path) -> Option<Child> {
    let out = fs::File::create(log).ok()?;
    Command::new("/usr/bin/log")
        .args([
            "stream",
            "--style",
            "compact",
            "--predicate",
            "sender == \"Sandbox\"",
        ])
        .stdout(out)
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

fn describe(events: &[StreamEvent]) -> String {
    events
        .iter()
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_real_codex_reaches_openai_through_the_jail() {
    let Some(codex) = on_path("codex") else {
        assert!(
            !required(),
            "LAZAROBOX_REQUIRE_JAIL=1 but codex is not installed (npm i -g @openai/codex)"
        );
        eprintln!("skipped: codex is not installed");
        return;
    };
    let jail = probe();
    if !matches!(jail, JailSupport::Available { .. }) {
        assert!(
            !required(),
            "LAZAROBOX_REQUIRE_JAIL=1 but this system cannot jail ({jail:?})"
        );
        eprintln!("skipped: no Seatbelt jail on this system ({jail:?})");
        return;
    }
    println!("codex: {}", codex.display());

    // The jail refuses scratch folders for a state dir, so the fresh home lives
    // in the crate folder, like the other runtime tests.
    let home_dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = fs::canonicalize(home_dir.path()).unwrap();
    fs::create_dir(home.join(".codex")).unwrap();
    // SAFETY: this is the only test of this binary, so no other thread reads
    // the environment while it is changed.
    unsafe {
        std::env::set_var("HOME", &home);
        for name in [
            "CODEX_HOME",
            "XDG_DATA_HOME",
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
        ] {
            std::env::remove_var(name);
        }
        std::env::set_var("CODEX_API_KEY", BOGUS_KEY);
    }

    let mut env = CliEnv::from_env(jail);
    env.helper = fs::canonicalize(env!("CARGO_BIN_EXE_lazarobox-shell")).unwrap();
    env.shim = None;
    assert_eq!(env.home.as_deref(), Some(home.as_path()));

    let log = home.join("sandbox.log");
    let mut watcher = watch_sandbox(&log);
    // `log stream` needs a moment to attach.
    std::thread::sleep(Duration::from_secs(3));

    let request = ChatRequest {
        engine: "codex".into(),
        model: None,
        system: "Answer in one word.".into(),
        memory: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: "hi".into(),
        }],
        tools: Tools::Off,
    };
    let events = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let run = runner_with(None, env, None);
            let (tx, mut rx) = mpsc::channel(4096);
            let turn = timeout(Duration::from_secs(120), run(request, tx)).await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            (turn.is_ok(), events)
        });

    std::thread::sleep(Duration::from_secs(2));
    let denials = if let Some(child) = watcher.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
        fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("deny"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        vec!["(log stream could not be started)".to_owned()]
    };
    let (finished, events) = events;
    let shown = describe(&events);
    let version = Command::new(&codex).arg("--version").output();
    let version = version
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    println!("codex version: {version}");
    println!("--- events ---\n{shown}");
    println!("--- sandbox denials ({}) ---", denials.len());
    for line in denials.iter().take(200) {
        println!("{line}");
    }

    assert!(finished, "the turn did not end in 120 s:\n{shown}");
    let lower = shown.to_lowercase();
    let broken: Vec<_> = BROKEN.iter().filter(|p| lower.contains(*p)).collect();
    assert!(
        broken.is_empty(),
        "the jail seems to break codex ({broken:?}):\n{shown}"
    );
    assert!(
        AUTH.iter().any(|p| lower.contains(p)),
        "expected an authentication failure from OpenAI:\n{shown}"
    );
    println!("RESULT: codex reached OpenAI through the jail and was refused for authentication");
}
