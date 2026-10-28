//! The manage rows of llamacpp's model level: install, remove and the newer
//! build check (llama-server-install, PR7).

use super::install::{InstallAsk, Reply, Screen};
use super::*;
use crate::core::ai::EngineState;
use crate::core::ai::install::Variant;

const OWNED: &str = "/h/lazarobox/llama.cpp/current/llama-server";

fn llamacpp(status: Status, binary: Option<&str>) -> Detection {
    Detection {
        engines: vec![EngineState {
            binary: binary.map(Into::into),
            id: "llamacpp",
            status,
            models: Vec::new(),
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        data_home: Some("/h".into()),
        ..Default::default()
    }
}

fn labels(d: &Detection) -> Vec<PickKey> {
    model_rows(
        "llamacpp",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(d),
    )
    .into_iter()
    .map(|r| r.key)
    .filter(|k| matches!(k, PickKey::InstallLlama | PickKey::RemoveLlama))
    .collect()
}

fn at_models(d: &Detection) -> Picker {
    let draft = AiConfig::default();
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(d));
    p.level = Level::Models {
        engine: "llamacpp".to_owned(),
    };
    p
}

fn cursor_on(p: &mut Picker, d: &Detection, key: PickKey) {
    let draft = AiConfig::default();
    let at = p
        .rows(&draft, Some(d))
        .iter()
        .position(|r| r.key == key)
        .expect("row");
    while p.cursor() != at {
        p.step(true, &draft, Some(d));
    }
}

#[test]
fn install_is_offered_when_llama_server_is_not_installed() {
    let d = llamacpp(Status::NotInstalled, None);
    assert_eq!(labels(&d), [PickKey::InstallLlama]);
    let rows = model_rows(
        "llamacpp",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(&d),
    );
    let row = rows
        .iter()
        .find(|r| r.key == PickKey::InstallLlama)
        .unwrap();
    assert_eq!(row.label, INSTALL_ROW);
    assert!(row.enabled);
}

#[test]
fn remove_is_offered_for_an_owned_install_only() {
    let d = llamacpp(Status::NotRunning, Some(OWNED));
    assert_eq!(labels(&d), [PickKey::RemoveLlama]);
}

#[test]
fn a_system_binary_gets_neither_entry() {
    for status in [Status::NotRunning, Status::Available] {
        let d = llamacpp(status, Some("/usr/bin/llama-server"));
        assert_eq!(labels(&d), [], "{status:?}");
    }
}

#[test]
fn no_entry_before_detection_or_without_a_data_home() {
    let rows = model_rows(
        "llamacpp",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        None,
    );
    assert!(rows.iter().all(|r| r.key != PickKey::InstallLlama));
    let mut d = llamacpp(Status::NotRunning, Some(OWNED));
    d.data_home = None;
    assert_eq!(labels(&d), []);
}

#[test]
fn other_engines_have_no_manage_rows() {
    let d = llamacpp(Status::NotInstalled, None);
    let rows = model_rows(
        "ollama",
        Target::Task(Task::Chat),
        &AiConfig::default(),
        Some(&d),
    );
    assert!(
        rows.iter()
            .all(|r| !matches!(r.key, PickKey::InstallLlama | PickKey::RemoveLlama))
    );
}

#[test]
fn the_install_row_opens_the_wizard_with_the_variants_of_this_machine() {
    let mut d = llamacpp(Status::NotInstalled, None);
    d.vulkan_icd = true;
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::InstallLlama);
    assert_eq!(p.enter(&AiConfig::default(), Some(&d)), Outcome::Stay);
    let wizard = p.install().expect("wizard open");
    assert_eq!(wizard.screen(), &Screen::Plan);
    let expected =
        crate::core::ai::install::variants(crate::core::ai::install::Platform::CURRENT, true);
    assert_eq!(wizard.variants(), expected.as_slice());
    // Vulkan only with an ICD.
    d.vulkan_icd = false;
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::InstallLlama);
    p.enter(&AiConfig::default(), Some(&d));
    assert!(!p.install().unwrap().variants().contains(&Variant::Vulkan));
}

#[test]
fn the_remove_row_asks_before_it_removes() {
    let d = llamacpp(Status::NotRunning, Some(OWNED));
    let draft = AiConfig::default();
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::RemoveLlama);
    assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    assert_eq!(p.install().unwrap().screen(), &Screen::ConfirmRemove);
    // n backs out without asking anything, once the prompt was drawn.
    assert_eq!(p.install_answer(false, &draft, Some(&d)), Outcome::Stay);
    assert!(p.install().is_some(), "an undrawn prompt ignores n");
    p.install_arm();
    assert_eq!(p.install_answer(false, &draft, Some(&d)), Outcome::Stay);
    assert!(p.install().is_none());
    // y asks the runtime.
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::RemoveLlama);
    p.enter(&draft, Some(&d));
    p.install_arm();
    let Outcome::Install(InstallAsk::Remove(id, binary)) = p.install_answer(true, &draft, Some(&d))
    else {
        panic!("expected a remove ask");
    };
    assert_eq!(binary, std::path::PathBuf::from(OWNED));
    assert_eq!(p.install().unwrap().screen(), &Screen::Removing);
    p.install_reply(id, Ok(Reply::Removed));
    assert_eq!(p.install().unwrap().screen(), &Screen::Removed);
}

#[test]
fn a_failed_remove_shows_why() {
    let d = llamacpp(Status::NotRunning, Some(OWNED));
    let draft = AiConfig::default();
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::RemoveLlama);
    p.enter(&draft, Some(&d));
    p.install_arm();
    let Outcome::Install(InstallAsk::Remove(id, _)) = p.install_answer(true, &draft, Some(&d))
    else {
        panic!("expected a remove ask");
    };
    p.install_reply(id, Err("cannot remove /x: busy".to_owned()));
    assert_eq!(
        p.install().unwrap().screen(),
        &Screen::Failed("cannot remove /x: busy".to_owned())
    );
}

#[test]
fn opening_llamacpp_with_an_owned_install_asks_for_the_newer_check() {
    let draft = AiConfig::default();
    let d = llamacpp(Status::NotRunning, Some(OWNED));
    let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(&d));
    cursor_on_engine(&mut p, &d);
    assert_eq!(
        p.enter(&draft, Some(&d)),
        Outcome::Install(InstallAsk::CheckNewer)
    );
    assert!(matches!(p.level, Level::Models { .. }));
}

#[test]
fn opening_llamacpp_without_an_owned_install_checks_nothing() {
    let draft = AiConfig::default();
    for d in [
        llamacpp(Status::NotInstalled, None),
        llamacpp(Status::NotRunning, Some("/usr/bin/llama-server")),
    ] {
        let mut p = Picker::open(Target::Task(Task::Chat), &draft, Some(&d));
        cursor_on_engine(&mut p, &d);
        assert_eq!(p.enter(&draft, Some(&d)), Outcome::Stay);
    }
}

fn cursor_on_engine(p: &mut Picker, d: &Detection) {
    cursor_on(p, d, PickKey::Engine("llamacpp".to_owned()));
}

#[test]
fn the_remove_screens_say_what_goes_and_esc_leaves_them() {
    let d = llamacpp(Status::NotRunning, Some(OWNED));
    let draft = AiConfig::default();
    let mut p = at_models(&d);
    cursor_on(&mut p, &d, PickKey::RemoveLlama);
    p.enter(&draft, Some(&d));
    let text: String = p
        .rows(&draft, Some(&d))
        .iter()
        .map(|r| r.label.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.contains("remove the llama.cpp install") && text.contains("not touched"),
        "{text}"
    );
    p.install_arm();
    let Outcome::Install(InstallAsk::Remove(id, _)) = p.install_answer(true, &draft, Some(&d))
    else {
        panic!("expected a remove ask");
    };
    p.install_reply(id, Err("busy".to_owned()));
    // A failed remove has no plan to go back to: Esc leaves the wizard.
    p.back(&draft, Some(&d));
    assert!(p.install().is_none());
    assert!(matches!(p.level, Level::Models { .. }));
}
