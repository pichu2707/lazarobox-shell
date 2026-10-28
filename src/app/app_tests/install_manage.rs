//! The manage rows of llamacpp in the AI picker: remove, and the newer build
//! notice that opening an owned install asks for (llama-server-install, PR7).

use std::path::PathBuf;

use super::hf_download::{enter, goto_pick};
use super::*;
use crate::core::ai::config::AiConfig;
use crate::core::ai::picker::{InstallAsk, InstallReply, REMOVE_ROW};
use crate::core::ai::{CATALOG, Detection, EngineState, Status};

const OWNED: &str = "/h/lazarobox/llama.cpp/current/llama-server";

fn llamacpp(binary: &str) -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| EngineState {
                binary: (e.id == "llamacpp").then(|| PathBuf::from(binary)),
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
        data_home: Some("/h".into()),
        ..Default::default()
    }
}

fn picker_on_engines(a: &mut App, detection: Detection) {
    a.engines = Some(detection);
    a.menu = Some(MenuState::open(&a.config).with_ai(AiConfig::default(), a.engines.clone()));
    a.input = InputMode::Menu;
    goto_row(a, "chat");
    a.update(enter());
}

/// Enter on llamacpp's engine row: the effects of opening its model level.
fn open_llamacpp(a: &mut App) -> Vec<Effect> {
    goto_pick(a, "llamacpp");
    a.update(enter())
}

fn checks(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Install(InstallAsk::CheckNewer)))
        .count()
}

// Spec (Newer tag notice): an owned install asks once per run.
#[test]
fn opening_an_owned_llamacpp_asks_for_the_newer_check_once_per_run() {
    let mut a = app();
    picker_on_engines(&mut a, llamacpp(OWNED));
    assert_eq!(checks(&open_llamacpp(&mut a)), 1);
    a.update(esc());
    assert_eq!(checks(&open_llamacpp(&mut a)), 0, "second time this run");
}

// Spec (A2): `[updates] check = false` makes no network call at all.
#[test]
fn no_newer_check_when_updates_are_off() {
    let mut a = app();
    a.config.update_check = false;
    picker_on_engines(&mut a, llamacpp(OWNED));
    assert_eq!(checks(&open_llamacpp(&mut a)), 0);
    // Turning it on later still gets its one check.
    a.update(esc());
    a.config.update_check = true;
    assert_eq!(checks(&open_llamacpp(&mut a)), 1);
}

#[test]
fn no_newer_check_for_a_system_binary() {
    let mut a = app();
    picker_on_engines(&mut a, llamacpp("/usr/bin/llama-server"));
    assert_eq!(checks(&open_llamacpp(&mut a)), 0);
}

// Spec (Remove llama-server): ask y/n, remove, then detect again.
#[test]
fn confirming_remove_asks_the_runtime_and_a_removed_reply_detects_again() {
    let mut a = app();
    picker_on_engines(&mut a, llamacpp(OWNED));
    open_llamacpp(&mut a);
    goto_pick(&mut a, REMOVE_ROW);
    assert!(a.update(enter()).is_empty());
    a.frame_drawn();
    let asked = a.update(key('y'));
    let [Effect::Install(InstallAsk::Remove(id, binary))] = asked.as_slice() else {
        panic!("expected one remove ask: {asked:?}");
    };
    assert_eq!(binary, &PathBuf::from(OWNED));
    let after = a.update(AppEvent::InstallReply(*id, Ok(InstallReply::Removed)));
    assert!(after.contains(&Effect::RedetectEngines), "{after:?}");
}

#[test]
fn declining_remove_asks_nothing() {
    let mut a = app();
    picker_on_engines(&mut a, llamacpp(OWNED));
    open_llamacpp(&mut a);
    goto_pick(&mut a, REMOVE_ROW);
    a.update(enter());
    a.frame_drawn();
    let declined = a.update(key('n'));
    assert!(declined.is_empty(), "{declined:?}");
    // Back on the model level, where the row is still offered.
    goto_pick(&mut a, REMOVE_ROW);
}

// A newer-build notice never replaces what the footer already says.
#[test]
fn a_newer_notice_never_overwrites_an_existing_notice() {
    let mut a = app();
    a.update(AppEvent::InstallNewer(
        "llama.cpp b9100 is available".to_owned(),
    ));
    assert_eq!(a.notice(), Some("llama.cpp b9100 is available"));
    a.update(AppEvent::InstallNewer(
        "llama.cpp b9200 is available".to_owned(),
    ));
    assert_eq!(a.notice(), Some("llama.cpp b9100 is available"));
    // An attestation warning is about the install in hand: it does replace.
    a.update(AppEvent::InstallWarning("gh is not installed".to_owned()));
    assert_eq!(a.notice(), Some("gh is not installed"));
}
