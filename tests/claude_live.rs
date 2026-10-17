//! Manual check M11 (ai-chat 16.6): the real `claude` CLI, started the way the
//! app starts it (the jail helper included), answers a harmless prompt.
//!
//! It uses the user's own claude login and subscription, so it never runs
//! with a plain `cargo test`:
//!
//! ```text
//! cargo test --test claude_live -- --ignored --nocapture
//! ```
//!
//! The test only stats `~/.claude.json` (inode, size, mtime) and lists the
//! names next to it: it never opens a credential file.

#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};

use lazarobox_shell::{
    core::{
        ai::{
            chat::engine::{ChatMessage, ChatRequest, Role, StreamEvent},
            jail::{JailOff, JailSupport},
        },
        layout::PaneId,
    },
    runtime::{
        chat::{Chats, CliEnv, runner_with},
        jail::probe,
    },
};

/// What `stat` says about a path, and nothing else.
fn stat(path: &Path) -> String {
    match fs::symlink_metadata(path) {
        Ok(m) => format!(
            "ino={} size={} mtime={}.{:09}",
            m.ino(),
            m.size(),
            m.mtime(),
            m.mtime_nsec()
        ),
        Err(e) => format!("missing ({e})"),
    }
}

/// The names in `dir` that start with `prefix`.
fn siblings(dir: &Path, prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with(prefix))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

async fn turn(env: CliEnv, text: &str) -> Vec<StreamEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut chats = Chats::new(runner_with(None, env), tx);
    let request = ChatRequest {
        engine: "claude".into(),
        model: None,
        system: "Be brief.".into(),
        memory: None,
        messages: vec![ChatMessage {
            role: Role::User,
            content: text.into(),
        }],
    };
    chats.send(PaneId::FIRST, 1, request);
    let mut events = Vec::new();
    loop {
        let (_, _, event) = tokio::time::timeout(Duration::from_secs(180), rx.recv())
            .await
            .expect("an event within 180 s")
            .expect("the channel is open");
        let last = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        events.push(event);
        if last {
            return events;
        }
    }
}

#[tokio::test]
#[ignore = "manual M11: runs the real claude CLI with the user's login"]
async fn a_jailed_claude_starts_and_answers() {
    let JailSupport::Available { abi, .. } = probe() else {
        eprintln!("skipped: no Landlock jail on this system");
        return;
    };
    eprintln!("landlock abi {abi}");
    live(probe()).await;
}

#[tokio::test]
#[ignore = "manual M11 (comparison): runs the real claude CLI bare, with no jail"]
async fn a_bare_claude_starts_and_answers() {
    live(JailSupport::Unavailable(JailOff::NotProbed)).await;
}

async fn live(jail: JailSupport) {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let config = home.join(".claude.json");
    let scratch = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut env = CliEnv::from_env(jail);
    env.data = Some(fs::canonicalize(scratch.path()).unwrap().join("lazarobox"));
    env.helper = PathBuf::from(env!("CARGO_BIN_EXE_lazarobox-shell"));

    eprintln!("before: ~/.claude.json {}", stat(&config));
    eprintln!("before: siblings {:?}", siblings(&home, ".claude.json"));
    // `LAZAROBOX_LIVE_PROMPT` swaps the prompt (M3: ask it to write a file).
    let prompt = std::env::var("LAZAROBOX_LIVE_PROMPT")
        .unwrap_or_else(|_| "Reply with the word ok".to_owned());
    let events = turn(env, &prompt).await;
    eprintln!("after:  ~/.claude.json {}", stat(&config));
    eprintln!("after:  siblings {:?}", siblings(&home, ".claude.json"));
    eprintln!("events: {events:?}");
    assert!(
        matches!(events.last(), Some(StreamEvent::Done(_))),
        "claude should answer: {events:?}"
    );
}
