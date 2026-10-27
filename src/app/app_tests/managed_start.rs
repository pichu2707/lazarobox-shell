//! A save that turns lazarobox's own llama-server on starts it at once, with
//! no restart (llamacpp-pick-model S4).

use std::path::PathBuf;

use super::*;
use crate::core::ai::config::{MANAGED_PORT, ManagedServer};

fn managing(model: &str) -> AiConfig {
    let mut ai = AiConfig::default();
    ai.manage(PathBuf::from(model));
    ai
}

/// The menu saves `draft` over the app's current AI config.
fn save(a: &mut App, draft: AiConfig) -> Vec<Effect> {
    a.menu = Some(MenuState::open(&a.config).with_ai(draft, a.engines.clone()));
    a.input = InputMode::Menu;
    a.update(AppEvent::ConfigSaved)
}

fn starts(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::StartManaged { .. }))
        .collect()
}

#[test]
fn a_save_that_turns_the_managed_server_on_starts_it() {
    let mut a = launched(80, 25);
    let effects = save(&mut a, managing("/m/qwen.gguf"));
    assert_eq!(
        starts(&effects),
        [&Effect::StartManaged {
            model: PathBuf::from("/m/qwen.gguf"),
            port: MANAGED_PORT,
        }]
    );
    assert_eq!(a.notice(), Some("starting llama-server with qwen.gguf…"));
    assert_eq!(
        a.ai.managed(),
        Some(&ManagedServer {
            model: PathBuf::from("/m/qwen.gguf")
        })
    );
}

#[test]
fn another_model_restarts_it_on_its_own_port() {
    let mut a = launched(80, 25);
    let mut before = AiConfig::default()
        .with_servers(crate::core::ai::ServerPorts::default().with("llamacpp", 39080));
    before.manage(PathBuf::from("/m/a.gguf"));
    a.ai = before.clone();
    before.manage(PathBuf::from("/m/b.gguf"));
    let effects = save(&mut a, before);
    assert_eq!(
        starts(&effects),
        [&Effect::StartManaged {
            model: PathBuf::from("/m/b.gguf"),
            port: 39080,
        }]
    );
}

#[test]
fn a_save_that_leaves_the_managed_server_alone_starts_nothing() {
    let mut a = launched(80, 25);
    assert_eq!(
        starts(&save(&mut a, AiConfig::default())),
        Vec::<&Effect>::new()
    );
    a.ai = managing("/m/a.gguf");
    assert_eq!(
        starts(&save(&mut a, managing("/m/a.gguf"))),
        Vec::<&Effect>::new()
    );
    assert_eq!(a.notice(), None);
}

#[test]
fn a_started_server_is_detected_again_and_says_so() {
    let mut a = launched(80, 25);
    save(&mut a, managing("/m/qwen.gguf"));
    let effects = a.update(AppEvent::ManagedStarted(Ok(())));
    assert_eq!(effects, vec![Effect::RedetectEngines]);
    assert_eq!(a.notice(), Some("llama-server is ready"));
}

#[test]
fn a_failed_start_says_why_and_detects_nothing() {
    let mut a = launched(80, 25);
    save(&mut a, managing("/m/qwen.gguf"));
    let reason = "could not start llama-server: is it on the PATH?";
    let effects = a.update(AppEvent::ManagedStarted(Err(reason.to_owned())));
    assert_eq!(effects, vec![]);
    assert_eq!(a.notice(), Some(reason));
}

// Found by the independent verifier.

#[test]
fn a_cancelled_menu_starts_nothing_and_keeps_the_ai_config() {
    let mut a = launched(80, 25);
    a.menu = Some(MenuState::open(&a.config).with_ai(managing("/m/q.gguf"), a.engines.clone()));
    a.input = InputMode::Menu;
    let effects = a.close_menu(super::super::Outcome::Reverted);
    assert_eq!(starts(&effects), Vec::<&Effect>::new());
    assert_eq!(a.ai.managed(), None);
    assert_eq!(a.notice(), None);
}

#[test]
fn a_failed_save_starts_nothing() {
    let mut a = launched(80, 25);
    a.menu = Some(MenuState::open(&a.config).with_ai(managing("/m/q.gguf"), a.engines.clone()));
    a.input = InputMode::Menu;
    let effects = a.update(AppEvent::ConfigSaveFailed("save failed: disk full".into()));
    assert_eq!(starts(&effects), Vec::<&Effect>::new());
    assert_eq!(a.ai.managed(), None);
}

#[test]
fn a_failed_start_after_ready_does_not_redetect() {
    let mut a = launched(80, 25);
    let effects = a.update(AppEvent::ManagedStarted(Err(
        crate::runtime::chat::managed::REPLACED.to_owned(),
    )));
    assert_eq!(effects, vec![]);
}

fn idles(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::ManagedIdle(_)))
        .collect()
}

// Spec S8: a save that changes the idle time hands it to the runtime at once.
#[test]
fn a_save_that_changes_the_idle_time_hands_it_to_the_runtime() {
    let mut a = launched(80, 25);
    let mut draft = a.ai.clone();
    draft.set_idle_minutes(30);
    let effects = save(&mut a, draft);
    assert_eq!(
        idles(&effects),
        [&Effect::ManagedIdle(Some(std::time::Duration::from_secs(
            30 * 60
        )))]
    );
    assert_eq!(a.ai.managed_options().idle_minutes, 30);
    let mut never = a.ai.clone();
    never.set_idle_minutes(0);
    assert_eq!(idles(&save(&mut a, never)), [&Effect::ManagedIdle(None)]);
    // Unchanged: nothing for the runtime.
    let same = a.ai.clone();
    assert_eq!(idles(&save(&mut a, same)), Vec::<&Effect>::new());
}
