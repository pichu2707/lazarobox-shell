//! Manual checks M6, M10 and M14 (ai-chat 18.1, 18.2, 18.5): the real `gemini`
//! CLI, started the way the app starts it (the jail helper, the library
//! folders of its node and the deny-all policy file included), answers a
//! harmless prompt.
//!
//! It uses the user's own gemini login and quota, so it never runs with a
//! plain `cargo test`:
//!
//! ```text
//! LAZAROBOX_LIVE_MODEL=<model> cargo test --test gemini_live -- --ignored --nocapture
//! ```
//!
//! - `LAZAROBOX_LIVE_MODEL` picks the model (the CLI's `auto` default can
//!   answer `503 high demand`, and a model can be out of quota or closed to
//!   new users: gemini 0.63.0 answered with `gemini-3-flash-preview`).
//! - `LAZAROBOX_LIVE_HELPER` swaps the jail helper binary.
//! - `LAZAROBOX_LIVE_PROMPT` swaps the prompt (M10: ask it to read
//!   `~/.ssh/config`, list the home, run ssh or curl, create a file).
//!
//! The test only lists names and modification times under `~/.gemini/tmp`
//! (where the CLI keeps its session files) and stats
//! `/tmp/lazarobox-m10-test`: it never opens a credential file.

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
            jail::JailSupport,
        },
        layout::PaneId,
    },
    runtime::{
        chat::{Chats, CliEnv, runner_with},
        jail::probe,
    },
};

/// Names and modification times (ns) of everything under `dir`, nothing else.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = fs::symlink_metadata(&path) {
                out.push(format!(
                    "{} {}.{:09}",
                    path.display(),
                    meta.mtime(),
                    meta.mtime_nsec()
                ));
                if meta.is_dir() {
                    walk(&path, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

async fn turn(env: CliEnv, model: Option<String>, text: &str) -> Vec<StreamEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut chats = Chats::new(runner_with(None, env), tx);
    let request = ChatRequest {
        engine: "gemini".into(),
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
#[ignore = "manual M6/M10/M14: runs the real gemini CLI with the user's login"]
async fn a_jailed_gemini_starts_and_answers() {
    let JailSupport::Available { abi, .. } = probe() else {
        eprintln!("skipped: no Landlock jail on this system");
        return;
    };
    eprintln!("landlock abi {abi}");
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    // gemini keeps one folder per working folder name, with `chats/session-*`
    // files: there is no flag that stops it (M6).
    let sessions = home.join(".gemini").join("tmp");
    let marker = Path::new("/tmp/lazarobox-m10-test");
    assert!(!marker.exists(), "remove {} first", marker.display());

    let scratch = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut env = CliEnv::from_env(probe());
    env.data = Some(fs::canonicalize(scratch.path()).unwrap().join("lazarobox"));
    // `LAZAROBOX_LIVE_HELPER` swaps the helper (a wrapper that logs its argv
    // and execs the real binary, to see the rules the app builds).
    env.helper = std::env::var_os("LAZAROBOX_LIVE_HELPER").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_lazarobox-shell")),
        PathBuf::from,
    );

    let before = listing(&sessions);
    let prompt = std::env::var("LAZAROBOX_LIVE_PROMPT")
        .unwrap_or_else(|_| "Reply with the word ok".to_owned());
    let model = std::env::var("LAZAROBOX_LIVE_MODEL").ok();
    let events = turn(env, model, &prompt).await;
    let after = listing(&sessions);
    eprintln!(
        "session entries before {} after {}",
        before.len(),
        after.len()
    );
    let names = |list: &[String]| -> Vec<String> {
        list.iter()
            .filter_map(|l| l.split(' ').next().map(str::to_owned))
            .collect()
    };
    let known = names(&before);
    for new in names(&after).iter().filter(|n| !known.contains(n)) {
        eprintln!("new under ~/.gemini/tmp: {new}");
    }
    eprintln!("events: {events:?}");
    assert!(!marker.exists(), "gemini created {}", marker.display());
    assert!(
        matches!(events.last(), Some(StreamEvent::Done(_))),
        "gemini should answer: {events:?}"
    );
}
