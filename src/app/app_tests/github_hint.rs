//! The GitHub hint (chat-pane: The GitHub hint): one muted system line that
//! suggests `/github` and names the repository, once per pane and repository.
//! It is not a message: never stored, never sent.

use std::path::PathBuf;

use super::chat_live::{composer, plain};
use super::github_grant::{grant, granted, tools_app};
use super::run_pane::chat;
use super::*;
use crate::core::ai::{
    chat::{
        Entry,
        engine::{Finish, StreamEvent},
    },
    config::ToolToggle,
    tools::github::{GhRepo, slug_of},
};

const HINT: &str = "Type /github to let this chat read o/r's issues and PRs";

fn slug(url: &str) -> GhRepo {
    GhRepo::Found(slug_of(url).expect("a github url"))
}

fn folder(a: &App) -> PathBuf {
    chat(a, id2()).origin_cwd().expect("a folder").to_path_buf()
}

/// The runtime answered the hint's probe of the chat folder.
fn probed(a: &mut App, repo: GhRepo) -> Vec<Effect> {
    let folder = folder(a);
    a.update(AppEvent::OriginProbed {
        pane: id2(),
        folder,
        dialog: false,
        repo,
    })
}

/// A chat whose origin is `github.com/o/r`, idle, tools on, toggle off.
fn hinted_app() -> App {
    let mut a = tools_app();
    // The pane is wide enough for the whole hint (the default one is 40).
    a.panes.get_mut(&id2()).unwrap().size.cols = 100;
    probed(&mut a, slug("https://github.com/o/r.git"));
    a
}

fn guidance(a: &App) -> Option<String> {
    a.chat_guidance(id2())
}

fn send(a: &mut App, text: &str) -> Vec<Effect> {
    for c in text.chars() {
        a.update(key(c));
    }
    a.update(plain(KeyCode::Enter))
}

// Spec (Hint shown): one muted line that names the repository, and no dialog.
#[test]
fn the_hint_names_the_repository_and_is_quiet() {
    let a = hinted_app();
    assert_eq!(guidance(&a).as_deref(), Some(HINT));
    assert!(a.guidance_is_hint(id2()), "drawn muted, not as a warning");
    assert!(chat(&a, id2()).approvals().pending().is_none());
}

// The chat asks for the origin when it opens and when its folder changes,
// off the render thread.
#[test]
fn opening_a_chat_and_changing_its_folder_probe_the_origin() {
    let mut a = launched(80, 25)
        .with_ai(super::chat_live::ollama())
        .with_tool_engines(&["ollama"]);
    let effects = press(&mut a, "c");
    let probes: Vec<_> = effects
        .iter()
        .filter(|e| matches!(e, Effect::ProbeOrigin { .. }))
        .collect();
    assert_eq!(
        probes,
        vec![&Effect::ProbeOrigin {
            pane: id2(),
            folder: folder(&a),
            dialog: false,
        }]
    );
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(effects.contains(&Effect::ProbeOrigin {
        pane: id2(),
        folder: PathBuf::from("/tmp"),
        dialog: false,
    }));
    // With the toggle on the hint could never show: nothing is read.
    let mut a = tools_app();
    a.ai.set_toggle(ToolToggle::Github, true);
    let effects = a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::ProbeOrigin { .. }))
    );
}

// Spec (Hint shown once): the first send hides it for good, whatever happens.
#[test]
fn the_hint_is_shown_once_per_repository() {
    let mut a = hinted_app();
    assert!(guidance(&a).is_some());
    a.update(key('q'));
    assert!(guidance(&a).is_some(), "typing does not hide it");
    let effects = a.update(plain(KeyCode::Enter));
    let turn = effects
        .iter()
        .find_map(|e| match e {
            Effect::ChatSend { turn, .. } => Some(*turn),
            _ => None,
        })
        .expect("a send");
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    assert_eq!(guidance(&a), None, "gone after the first send");
    for n in 0..5 {
        let effects = send(&mut a, &format!("again {n}"));
        let turn = effects
            .iter()
            .find_map(|e| match e {
                Effect::ChatSend { turn, .. } => Some(*turn),
                _ => None,
            })
            .expect("a send");
        a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
        assert_eq!(guidance(&a), None, "send {n}");
    }
    // The same answer again does not bring it back.
    probed(&mut a, slug("https://github.com/o/r.git"));
    assert_eq!(guidance(&a), None);
}

// Spec (No hint after decline or grant).
#[test]
fn a_decline_and_a_grant_end_the_hint_for_that_repository() {
    for answer in ['n', 'y'] {
        let mut a = hinted_app();
        send(&mut a, "/github");
        a.update(AppEvent::OriginProbed {
            pane: id2(),
            folder: folder(&a),
            dialog: true,
            repo: slug("https://github.com/o/r.git"),
        });
        NOW_MS.with(|n| n.set(n.get() + 1_000));
        a.update(key(answer));
        assert_eq!(granted(&a, id2()), answer == 'y');
        assert_eq!(guidance(&a), None, "after {answer}");
        // A later grant ending (folder change) back to the same repository
        // does not tell it again either.
        probed(&mut a, slug("https://github.com/o/r.git"));
        assert_eq!(guidance(&a), None, "after {answer}, again");
    }
}

// Spec (No hint when it does not apply).
#[test]
fn the_hint_does_not_show_when_it_does_not_apply() {
    // The toggle on.
    let mut a = hinted_app();
    a.ai.set_toggle(ToolToggle::Github, true);
    assert_eq!(guidance(&a), None, "toggle on");
    // A grant.
    let mut a = hinted_app();
    grant(&mut a, id2());
    assert_eq!(guidance(&a), None, "grant");
    // Tools off.
    let mut a = hinted_app();
    a.ai.set_tools_enabled(false);
    assert_eq!(guidance(&a), None, "tools off");
    // No origin, a non-GitHub origin, an unreadable one, and no answer at all.
    for (name, repo) in [
        ("no origin", GhRepo::NoOrigin),
        ("gitlab", GhRepo::NotGithub),
        ("no repository", GhRepo::NoRepo),
        ("unreadable", GhRepo::Unreadable),
    ] {
        let mut a = tools_app();
        probed(&mut a, repo);
        assert_eq!(guidance(&a), None, "{name}");
    }
    assert_eq!(
        guidance(&tools_app()),
        None,
        "unknown folder: no answer yet"
    );
    // An engine that offers no tools.
    let mut a = hinted_app();
    a.tool_engines = &[];
    assert_eq!(guidance(&a), None, "engine without tools");
    // A turn in flight.
    let mut a = hinted_app();
    send(&mut a, "go");
    assert_eq!(guidance(&a), None, "busy");
}

// Spec (Model never sees the hint): not a message, not stored, not sent.
#[test]
fn the_hint_is_in_no_request_no_row_and_no_transcript() {
    let mut a = hinted_app();
    assert!(guidance(&a).is_some());
    let effects = send(&mut a, "hello");
    for effect in &effects {
        match effect {
            Effect::ChatSend { request, .. } => assert!(
                request
                    .messages
                    .iter()
                    .all(|m| !m.content.contains("/github"))
                    && !request.system.contains("Type /github"),
                "{request:?}"
            ),
            Effect::Store(cmd) => assert!(!format!("{cmd:?}").contains("/github")),
            _ => {}
        }
    }
    assert!(
        (chat(&a, id2()).entries().iter())
            .all(|e| !matches!(e, Entry::System(l) if l.contains("/github"))),
        "the hint is guidance above the composer, not a transcript line"
    );
    assert_eq!(composer(&a), "");
}

// Spec (New repository after /cd): the grant ended with the folder, and the
// hint may show once for the new repository.
#[test]
fn a_new_repository_after_cd_may_show_the_hint_once() {
    let mut a = hinted_app();
    send(&mut a, "hi");
    assert_eq!(guidance(&a), None);
    let turn = chat(&a, id2()).turn();
    a.update(AppEvent::Chat(id2(), turn, StreamEvent::Done(Finish::Stop)));
    grant(&mut a, id2());
    a.apply_folder(id2(), PathBuf::from("/tmp"));
    assert!(!granted(&a, id2()), "the grant ended with the folder");
    assert_eq!(guidance(&a), None, "no answer for the new folder yet");
    // The answer of the old folder is stale and shows nothing.
    a.update(AppEvent::OriginProbed {
        pane: id2(),
        folder: PathBuf::from("/launch"),
        dialog: false,
        repo: slug("https://github.com/o/r.git"),
    });
    assert_eq!(guidance(&a), None);
    a.update(AppEvent::OriginProbed {
        pane: id2(),
        folder: PathBuf::from("/tmp"),
        dialog: false,
        repo: slug("https://github.com/o/b.git"),
    });
    assert_eq!(
        guidance(&a).as_deref(),
        Some("Type /github to let this chat read o/b's issues and PRs")
    );
    // The new folder's prompt is ready: the next message really leaves.
    a.update(AppEvent::PromptReady(
        id2(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".to_owned(),
            notices: Vec::new(),
        },
    ));
    send(&mut a, "x");
    assert_eq!(guidance(&a), None, "once for o/b as well");
}
