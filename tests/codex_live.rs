//! Manual checks M5, M9 and M14 (ai-chat 17.1, 17.2, 17.5): the real `codex`
//! CLI, started the way the app starts it (the jail helper included), answers
//! a harmless prompt.
//!
//! It uses the user's own codex login and subscription, so it never runs with
//! a plain `cargo test`:
//!
//! ```text
//! LAZAROBOX_LIVE_MODEL=<model> cargo test --test codex_live -- --ignored --nocapture
//! ```
//!
//! - `LAZAROBOX_LIVE_MODEL` picks the model (the default one may not exist on
//!   the account: codex 0.142.5 answers `The 'gpt-5.5' model is not supported
//!   when using Codex with a ChatGPT account` for a ChatGPT login).
//! - `LAZAROBOX_LIVE_HELPER` swaps the jail helper binary.
//! - `LAZAROBOX_LIVE_PROMPT` swaps the prompt (M9: ask it to read
//!   `~/.ssh/config`, list the home, run ssh or curl, create a file).
//!
//! The test only lists names and modification times under `~/.codex/sessions`
//! and stats `/tmp/lazarobox-m9-test`: it never opens a credential file.

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
        engine: "codex".into(),
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
#[ignore = "manual M5/M9/M14: runs the real codex CLI with the user's login"]
async fn a_jailed_codex_starts_and_answers() {
    let JailSupport::Available { abi, .. } = probe() else {
        eprintln!("skipped: no Landlock jail on this system");
        return;
    };
    eprintln!("landlock abi {abi}");
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let sessions = std::env::var_os("CODEX_HOME")
        .map_or_else(|| home.join(".codex"), PathBuf::from)
        .join("sessions");
    let marker = Path::new("/tmp/lazarobox-m9-test");
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
        "sessions entries before {} after {}",
        before.len(),
        after.len()
    );
    eprintln!("events: {events:?}");
    assert_eq!(before, after, "--ephemeral must write nothing to sessions");
    assert!(!marker.exists(), "codex created {}", marker.display());
    assert!(
        matches!(events.last(), Some(StreamEvent::Done(_))),
        "codex should answer: {events:?}"
    );
}

/// `codex debug models` through the jail, the way detection runs it. It may
/// refresh codex's catalog from OpenAI with the user's login (and rewrite
/// `models_cache.json`), like running codex does.
#[tokio::test]
#[ignore = "runs the real codex through the jail"]
async fn the_model_listing_runs_through_the_jail() {
    let support = probe();
    if !support.is_available() {
        eprintln!("jail unavailable here: {support:?}");
        return;
    }
    let scratch = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut env = CliEnv::from_env(support);
    env.data = Some(fs::canonicalize(scratch.path()).unwrap().join("lazarobox"));
    env.helper = PathBuf::from(env!("CARGO_BIN_EXE_lazarobox-shell"));
    let raw = lazarobox_shell::runtime::ai_detect::codex_listing(&env).await;
    let raw = raw.expect("the jailed listing should succeed");
    let text = String::from_utf8(raw).unwrap();
    let models = lazarobox_shell::core::ai::parse::codex_models(&text);
    eprintln!("jailed listing: {} bytes, models {models:?}", text.len());
    assert!(!models.is_empty());
}
