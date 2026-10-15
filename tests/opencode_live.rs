//! Manual check: the real `opencode` CLI, started the way the app starts it
//! (the jail helper, the deny-all config, the private config, state and
//! session database), answers a harmless prompt.
//!
//! It uses the user's own opencode login and quota, so it never runs with a
//! plain `cargo test`:
//!
//! ```text
//! LAZAROBOX_LIVE_MODEL=<provider/model> cargo test --test opencode_live -- --ignored --nocapture
//! ```
//!
//! - `LAZAROBOX_LIVE_MODEL` picks the model (`opencode models` lists them).
//!   Without it the CLI picks its own default, which can be a free-tier model
//!   that only answers inside the opencode app.
//! - `LAZAROBOX_LIVE_HELPER` swaps the jail helper binary.
//! - `LAZAROBOX_LIVE_PROMPT` swaps the prompt (ask it to read `~/.ssh/config`,
//!   list the home, run a command, create a file: it must refuse or fail).
//!
//! The test only stats `~/.local/share/opencode/opencode.db` (the user's
//! session database, which a chat turn must not touch) and
//! `/tmp/lazarobox-opencode-test`: it never opens a credential file.

#![cfg(target_os = "linux")]

use std::{fs, os::unix::fs::MetadataExt, path::PathBuf, time::Duration};

use lazarobox_shell::{
    core::{
        ai::{
            chat::engine::{ChatMessage, ChatRequest, Role, StreamEvent},
            jail::JailSupport,
        },
        layout::PaneId,
    },
    runtime::{
        chat::{Chats, CliEnv, runner_with},
        jail::probe,
    },
};

async fn turn(env: CliEnv, model: Option<String>, text: &str) -> Vec<StreamEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut chats = Chats::new(runner_with(None, env), tx);
    let request = ChatRequest {
        engine: "opencode".into(),
        model,
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
#[ignore = "manual: runs the real opencode CLI with the user's login"]
async fn a_jailed_opencode_starts_and_answers() {
    let JailSupport::Available { abi, .. } = probe() else {
        eprintln!("skipped: no Landlock jail on this system");
        return;
    };
    eprintln!("landlock abi {abi}");
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let database = home.join(".local/share/opencode/opencode.db");
    let stamp = |path: &PathBuf| {
        fs::metadata(path)
            .ok()
            .map(|m| (m.mtime(), m.mtime_nsec(), m.len()))
    };
    let marker = PathBuf::from("/tmp/lazarobox-opencode-test");
    assert!(!marker.exists(), "remove {} first", marker.display());

    let scratch = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut env = CliEnv::from_env(probe());
    env.data = Some(fs::canonicalize(scratch.path()).unwrap().join("lazarobox"));
    env.helper = std::env::var_os("LAZAROBOX_LIVE_HELPER").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_lazarobox-shell")),
        PathBuf::from,
    );

    let before = stamp(&database);
    let prompt = std::env::var("LAZAROBOX_LIVE_PROMPT")
        .unwrap_or_else(|_| "Reply with the word ok".to_owned());
    let model = std::env::var("LAZAROBOX_LIVE_MODEL").ok();
    let events = turn(env, model, &prompt).await;
    eprintln!("events: {events:?}");
    assert!(!marker.exists(), "opencode created {}", marker.display());
    // The session lives in the turn's private database, never in the user's.
    assert_eq!(
        stamp(&database),
        before,
        "the user's opencode database changed"
    );
    assert!(
        matches!(events.last(), Some(StreamEvent::Done(_))),
        "opencode should answer: {events:?}"
    );
}
