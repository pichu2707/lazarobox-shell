//! A Hugging Face download started from the llamacpp picker: the requests,
//! the progress in the status bar and the end (config-menu, Download in the
//! status bar).

use std::path::PathBuf;

use super::*;
use crate::core::ai::config::AiConfig;
use crate::core::ai::hf::{Answer, Ask, End, HubFile, Listing, Repo, RepoFiles};
use crate::core::ai::picker::HF_ROW;
use crate::core::ai::{CATALOG, Detection, EngineState, Status};
use crate::core::menu::Footer;

const GB: u64 = 1_000_000_000;

pub(super) fn enter() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn stopped_llamacpp() -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: None,
                id: e.id,
                status: if e.id == "llamacpp" {
                    Status::NotRunning
                } else {
                    Status::NotInstalled
                },
                models: Vec::new(),
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            })
            .collect(),
        local_models: vec![PathBuf::from("/m/b.gguf")],
        data_home: None,
        vulkan_icd: false,
        ..Default::default()
    }
}

pub(super) fn cursor_label(a: &App) -> String {
    a.menu()
        .unwrap()
        .rows(&a.config)
        .into_iter()
        .find_map(|row| match row {
            Row::Pick {
                label,
                selected: true,
                ..
            } => Some(label),
            _ => None,
        })
        .expect("a picker cursor")
}

pub(super) fn goto_pick(a: &mut App, label: &str) {
    for _ in 0..30 {
        if cursor_label(a) == label {
            return;
        }
        a.update(key('j'));
    }
    panic!("no pick row {label}");
}

/// The menu open on the chat picker, at llamacpp's model level.
pub(super) fn at_llamacpp() -> App {
    let mut a = app();
    a.engines = Some(stopped_llamacpp());
    a.menu = Some(MenuState::open(&a.config).with_ai(AiConfig::default(), a.engines.clone()));
    a.input = InputMode::Menu;
    goto_row(&mut a, "chat");
    a.update(enter());
    goto_pick(&mut a, "llamacpp");
    a.update(enter());
    a
}

fn listing() -> Listing {
    listing_named("a-Q4_K_M.gguf")
}

fn listing_named(path: &str) -> Listing {
    let repo = Repo {
        id: "o/a".to_owned(),
        commit: "c1".to_owned(),
        license_id: Some("apache-2.0".to_owned()),
        license_link: None,
        files: Vec::new(),
    };
    let file = HubFile {
        path: path.to_owned(),
        size: 4 * GB,
        oid: "a".repeat(64),
    };
    Listing {
        files: RepoFiles::new(repo, &[file]),
        hub: Some(PathBuf::from("/hub")),
        free: Some(50 * GB),
        missing: vec![4 * GB],
        ram: None,
    }
}

/// Enter on the entry row, which asks for the popular models, an empty
/// answer, then Esc: the search field.
fn to_query(a: &mut App) {
    goto_pick(a, HF_ROW);
    assert_eq!(a.update(enter()), vec![Effect::HubAsk(Ask::Popular)]);
    a.update(AppEvent::HubReply(
        Ask::Popular,
        Ok(Answer::Repos {
            hits: Vec::new(),
            ram: None,
        }),
    ));
    a.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    )));
}

/// Pastes `o/a`, gets its files and presses `y` on its only choice: the
/// effects of every step, in order.
fn start_download(a: &mut App) -> Vec<Effect> {
    start_download_of(a, listing())
}

fn start_download_of(a: &mut App, listing: Listing) -> Vec<Effect> {
    to_query(a);
    let mut effects = a.update(AppEvent::Paste("o/a".to_owned()));
    let asked = a.update(enter());
    assert_eq!(asked, vec![Effect::HubAsk(Ask::Files("o/a".to_owned()))]);
    effects.extend(asked);
    effects.extend(a.update(AppEvent::HubReply(
        Ask::Files("o/a".to_owned()),
        Ok(Answer::Files(Box::new(listing))),
    )));
    effects.extend(a.update(enter()));
    effects.extend(a.update(key('y')));
    effects
}

fn downloads(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Download(_)))
        .count()
}

fn redetects(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::RedetectEngines))
        .count()
}

#[test]
fn y_on_the_confirmation_starts_one_download() {
    let mut a = at_llamacpp();
    let effects = start_download(&mut a);
    assert_eq!(downloads(&effects), 1, "{effects:?}");
    let Some(Effect::Download(job)) = effects.last() else {
        panic!("the last effect is not the download: {effects:?}");
    };
    assert_eq!(job.id, "o/a");
    assert_eq!(job.choice.name(), "a-Q4_K_M.gguf");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::SaveConfig(_))),
        "{effects:?}"
    );
    assert_eq!(a.status_download().as_deref(), Some("↓ 0%"));
    assert_eq!(cursor_label(&a), "↓ 0% a-Q4_K_M.gguf · apache-2.0");
}

// Scenario: Progress then done.
#[test]
fn progress_then_done_shows_in_the_status_bar_and_redetects_once() {
    let mut a = at_llamacpp();
    start_download(&mut a);
    let mut effects = Vec::new();
    effects.extend(a.update(AppEvent::DownloadProgress {
        done: 4 * GB / 10,
        total: 4 * GB,
    }));
    assert_eq!(a.status_download().as_deref(), Some("↓ 10%"));
    effects.extend(a.update(AppEvent::DownloadProgress {
        done: 4 * GB * 55 / 100,
        total: 4 * GB,
    }));
    assert_eq!(a.status_download().as_deref(), Some("↓ 55%"));
    assert_eq!(cursor_label(&a), "↓ 55% a-Q4_K_M.gguf · apache-2.0");
    effects.extend(a.update(AppEvent::DownloadEnd(End::Done(PathBuf::from(
        "/hub/models--o--a/snapshots/c1/a-Q4_K_M.gguf",
    )))));
    assert_eq!(a.status_download(), None);
    assert_eq!(a.notice(), Some("downloaded a-Q4_K_M.gguf"));
    assert_eq!(cursor_label(&a), HF_ROW);
    // A late end or progress changes nothing.
    effects.extend(a.update(AppEvent::DownloadEnd(End::Done(PathBuf::from("/x")))));
    effects.extend(a.update(AppEvent::DownloadProgress { done: 1, total: 2 }));
    assert_eq!(a.status_download(), None);
    assert_eq!(redetects(&effects), 1, "{effects:?}");
}

#[test]
fn a_failure_shows_its_message_and_redetects_nothing() {
    let mut a = at_llamacpp();
    start_download(&mut a);
    let message = "download failed: timeout (run it again to resume)";
    let effects = a.update(AppEvent::DownloadEnd(End::Failed(message.to_owned())));
    assert_eq!(a.notice(), Some(message));
    assert_eq!(a.status_download(), None);
    assert_eq!(redetects(&effects), 0, "{effects:?}");
}

#[test]
fn the_cancel_prompt_cancels_and_the_end_says_how_to_resume() {
    let mut a = at_llamacpp();
    start_download(&mut a);
    a.update(enter());
    let Footer::Hint(hint) = a.menu().unwrap().footer() else {
        panic!("not a hint");
    };
    assert_eq!(hint, "cancel the download? (y/n)");
    assert_eq!(a.update(key('y')), vec![Effect::CancelDownload]);
    let effects = a.update(AppEvent::DownloadEnd(End::Cancelled));
    assert_eq!(
        a.notice(),
        Some("download cancelled · run it again to resume")
    );
    assert_eq!(redetects(&effects), 0);
}

#[test]
fn a_failed_request_is_the_footer_error() {
    let mut a = at_llamacpp();
    to_query(&mut a);
    a.update(AppEvent::Paste("gated/model".to_owned()));
    a.update(enter());
    let message = "gated/model needs a Hugging Face token: set HF_TOKEN";
    a.update(AppEvent::HubReply(
        Ask::Files("gated/model".to_owned()),
        Err(message.to_owned()),
    ));
    assert_eq!(a.menu().unwrap().footer(), Footer::Error(message));
}

#[test]
fn a_reply_with_the_menu_closed_is_dropped() {
    let mut a = launched(80, 25);
    let effects = a.update(AppEvent::HubReply(
        Ask::Search("x".to_owned()),
        Err("boom".to_owned()),
    ));
    assert_eq!(effects, Vec::new());
    assert_eq!(a.notice(), None);
}

#[test]
fn a_menu_opened_during_a_download_shows_its_progress() {
    let mut a = at_llamacpp();
    start_download(&mut a);
    a.update(AppEvent::DownloadProgress {
        done: GB,
        total: 4 * GB,
    });
    // Close the menu and open it again.
    a.update(esc());
    a.update(esc());
    a.update(esc());
    assert!(a.menu().is_none());
    a.update(ctrl_space());
    a.update(key('m'));
    goto_row(&mut a, "chat");
    a.update(enter());
    goto_pick(&mut a, "llamacpp");
    a.update(enter());
    goto_pick(&mut a, "↓ 25% a-Q4_K_M.gguf · apache-2.0");
}

// A file name from the Hub is drawn safe in the end notice too: a format
// character (here a right-to-left override) shows as `?`.
#[test]
fn the_done_notice_draws_the_file_name_safe() {
    let mut a = at_llamacpp();
    let effects = start_download_of(&mut a, listing_named("a\u{202e}b-Q4_K_M.gguf"));
    assert_eq!(downloads(&effects), 1);
    a.update(AppEvent::DownloadEnd(End::Done(PathBuf::from(
        "/hub/x.gguf",
    ))));
    assert_eq!(a.notice(), Some("downloaded a?b-Q4_K_M.gguf"));
}
