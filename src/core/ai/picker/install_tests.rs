//! The llama-server install wizard (llama-server-install, wizard steps).

use super::install::{
    COPY_ROW, HINT_CANCEL_INSTALL, HINT_CONFIRM_STEP, HINT_PLAN, HINT_WORKING, InstallAsk, Reply,
    Screen, Step, Wizard,
};
use super::*;
use crate::core::ai::install::{Release, Variant};

fn release() -> Release {
    Release {
        tag: "b9000".to_owned(),
        asset: "llama-b9000-bin-ubuntu-x64.tar.gz".to_owned(),
        size: 40_000_000,
        sha256: "a".repeat(64),
    }
}

fn wizard() -> Wizard {
    Wizard::new(vec![Variant::Cpu, Variant::Vulkan])
}

/// A wizard that chose the CPU build and got its release.
fn confirming_download() -> Wizard {
    let mut w = wizard();
    w.enter(0);
    w.reply(Ok(Reply::Resolved(release())));
    w
}

fn confirming_install() -> Wizard {
    let mut w = confirming_download();
    w.yes();
    w.reply(Ok(Reply::Downloaded));
    w
}

fn offering() -> Wizard {
    let mut w = confirming_install();
    w.yes();
    w.reply(Ok(Reply::Installed));
    w
}

fn labels(w: &Wizard) -> Vec<String> {
    w.rows().into_iter().map(|r| r.label).collect()
}

// Scenario: the plan lists the variants, then the copy-only row.
#[test]
fn plan_rows_are_the_variants_then_copy_only() {
    let rows = wizard().rows();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].key, PickKey::Hub(0));
    assert!(rows[0].label.contains("CPU"));
    assert!(rows[1].label.contains("Vulkan"));
    assert_eq!(rows[2].label, COPY_ROW);
    assert!(rows.iter().all(|r| r.enabled));
}

// Scenario: nothing runs without consent.
#[test]
fn a_new_wizard_asks_nothing_and_y_at_the_plan_does_nothing() {
    let mut w = wizard();
    assert_eq!(w.asking(), None);
    assert_eq!(w.yes(), Step::Stay);
    assert_eq!(w.no(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Plan));
}

#[test]
fn choosing_a_variant_asks_to_resolve_it() {
    let mut w = wizard();
    assert_eq!(w.enter(1), Step::Ask(InstallAsk::Resolve(Variant::Vulkan)));
    assert!(matches!(w.screen(), Screen::Resolving(Variant::Vulkan)));
    assert_eq!(w.asking(), Some(InstallAsk::Resolve(Variant::Vulkan)));
}

// Scenario: "I'll do it myself" never starts anything.
#[test]
fn the_copy_only_row_copies_instructions_and_never_asks() {
    let mut w = wizard();
    match w.enter(2) {
        Step::Copy(text) => assert!(text.contains("github.com/ggml-org/llama.cpp/releases")),
        other => panic!("expected a copy, got {other:?}"),
    }
    assert!(matches!(w.screen(), Screen::Plan));
    assert_eq!(w.asking(), None);
}

#[test]
fn a_row_past_the_end_does_nothing() {
    assert_eq!(wizard().enter(9), Step::Stay);
}

#[test]
fn the_release_waits_for_a_yes_before_the_download() {
    let mut w = confirming_download();
    assert!(matches!(w.screen(), Screen::ConfirmDownload(r) if *r == release()));
    assert_eq!(w.asking(), None);
    assert_eq!(w.enter(0), Step::Stay);
    assert_eq!(w.yes(), Step::Ask(InstallAsk::Download(release())));
    assert!(matches!(w.screen(), Screen::Downloading { .. }));
}

#[test]
fn n_at_the_download_prompt_goes_back_to_the_plan() {
    let mut w = confirming_download();
    assert_eq!(w.no(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Plan));
}

#[test]
fn a_reply_for_a_screen_the_user_left_is_dropped() {
    let mut w = wizard();
    w.enter(0);
    w.back();
    assert!(!w.reply(Ok(Reply::Resolved(release()))));
    assert!(matches!(w.screen(), Screen::Plan));
    // A reply of the wrong kind is dropped too.
    let mut w = wizard();
    w.enter(0);
    w.reply(Ok(Reply::Installed));
    assert!(matches!(w.screen(), Screen::Resolving(_)));
}

#[test]
fn the_install_waits_for_a_yes_after_the_verified_download() {
    let mut w = confirming_install();
    assert!(matches!(w.screen(), Screen::ConfirmInstall(_)));
    assert_eq!(w.asking(), None);
    assert_eq!(w.yes(), Step::Ask(InstallAsk::Install(release())));
    assert!(matches!(w.screen(), Screen::Installing(_)));
}

#[test]
fn n_at_the_install_prompt_goes_back_to_the_plan() {
    let mut w = confirming_install();
    w.no();
    assert!(matches!(w.screen(), Screen::Plan));
}

// Scenario: Hugging Face is offered last, after the install.
#[test]
fn the_asks_come_in_order_and_hugging_face_comes_last() {
    let mut w = wizard();
    let mut asks = vec![w.enter(0)];
    w.reply(Ok(Reply::Resolved(release())));
    asks.push(w.yes());
    w.reply(Ok(Reply::Downloaded));
    asks.push(w.yes());
    assert_eq!(
        asks,
        vec![
            Step::Ask(InstallAsk::Resolve(Variant::Cpu)),
            Step::Ask(InstallAsk::Download(release())),
            Step::Ask(InstallAsk::Install(release())),
        ]
    );
    w.reply(Ok(Reply::Installed));
    assert!(matches!(w.screen(), Screen::OfferHf));
    assert_eq!(w.clone().yes(), Step::Hub);
    assert_eq!(w.no(), Step::Close);
}

#[test]
fn esc_on_the_offer_closes_the_wizard() {
    assert_eq!(offering().back(), Step::Close);
}

#[test]
fn esc_at_the_plan_closes_the_wizard() {
    assert_eq!(wizard().back(), Step::Close);
}

#[test]
fn esc_while_resolving_goes_back_to_the_plan() {
    let mut w = wizard();
    w.enter(0);
    assert_eq!(w.back(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Plan));
    assert_eq!(w.asking(), None);
}

// Scenario: cancel keeps the partial file; the user is asked first.
#[test]
fn esc_while_downloading_asks_before_cancelling() {
    let mut w = confirming_download();
    w.yes();
    assert_eq!(w.back(), Step::Stay);
    assert!(w.asks_cancel());
    assert_eq!(w.hint(), HINT_CANCEL_INSTALL);
    // Any other answer keeps the download.
    assert_eq!(w.no(), Step::Stay);
    assert!(!w.asks_cancel());
    assert!(matches!(w.screen(), Screen::Downloading { .. }));
    w.back();
    assert_eq!(w.yes(), Step::Ask(InstallAsk::Cancel));
    assert!(matches!(w.screen(), Screen::Plan));
}

#[test]
fn y_without_a_cancel_prompt_does_not_cancel_the_download() {
    let mut w = confirming_download();
    w.yes();
    assert_eq!(w.yes(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Downloading { .. }));
}

#[test]
fn the_download_row_shows_the_share_done() {
    let mut w = confirming_download();
    w.yes();
    w.progress(10_000_000, 40_000_000);
    assert!(labels(&w).iter().any(|l| l.contains("25%")));
}

#[test]
fn esc_cannot_interrupt_the_install_step() {
    let mut w = confirming_install();
    w.yes();
    assert_eq!(w.back(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Installing(_)));
}

#[test]
fn a_failure_shows_the_message_and_esc_returns_to_the_plan() {
    let mut w = wizard();
    w.enter(0);
    w.reply(Err("GitHub rate limit reached\u{1b}[31m".to_owned()));
    assert!(matches!(w.screen(), Screen::Failed(_)));
    let shown = labels(&w).join(" ");
    assert!(shown.contains("GitHub rate limit reached"));
    assert!(!shown.contains('\u{1b}'));
    assert_eq!(w.asking(), None);
    assert_eq!(w.yes(), Step::Stay);
    assert_eq!(w.back(), Step::Stay);
    assert!(matches!(w.screen(), Screen::Plan));
}

#[test]
fn a_failure_while_installing_is_reported_too() {
    let mut w = confirming_install();
    w.yes();
    w.reply(Err("llama-server --version failed".to_owned()));
    assert!(matches!(w.screen(), Screen::Failed(_)));
}

#[test]
fn every_screen_has_a_title_and_a_hint() {
    let mut w = wizard();
    assert!(!w.title().is_empty());
    assert_eq!(w.hint(), HINT_PLAN);
    w.enter(0);
    assert_eq!(w.hint(), HINT_WORKING);
    w.reply(Ok(Reply::Resolved(release())));
    assert_eq!(w.hint(), HINT_CONFIRM_STEP);
}

#[test]
fn an_empty_plan_still_offers_the_copy_only_row() {
    let w = Wizard::new(Vec::new());
    assert_eq!(labels(&w), vec![COPY_ROW.to_owned()]);
}

// Picker integration.
fn picker() -> Picker {
    let mut p = Picker::open(Target::Task(Task::Chat), &AiConfig::default(), None);
    p.open_install(vec![Variant::Cpu]);
    p
}

#[test]
fn the_picker_turns_a_wizard_ask_into_an_outcome() {
    let mut p = picker();
    let (draft, none) = (AiConfig::default(), None);
    assert!(p.install().is_some());
    assert_eq!(
        p.enter(&draft, none),
        Outcome::Install(InstallAsk::Resolve(Variant::Cpu))
    );
}

#[test]
fn the_picker_copy_row_gives_a_copy_outcome() {
    let mut p = picker();
    let draft = AiConfig::default();
    p.step(true, &draft, None);
    assert!(matches!(p.enter(&draft, None), Outcome::Copy(_)));
}

#[test]
fn esc_at_the_plan_leaves_the_picker_on_llamacpp_models() {
    let mut p = picker();
    let draft = AiConfig::default();
    assert_eq!(p.back(&draft, None), Outcome::Stay);
    assert!(matches!(&p.level, Level::Models { engine } if engine == "llamacpp"));
}

#[test]
fn the_picker_routes_replies_and_answers_to_the_wizard() {
    let mut p = picker();
    let draft = AiConfig::default();
    p.enter(&draft, None);
    p.install_reply(Ok(Reply::Resolved(release())));
    assert_eq!(
        p.install_answer(true, &draft, None),
        Outcome::Install(InstallAsk::Download(release()))
    );
    p.install_reply(Ok(Reply::Downloaded));
    p.install_answer(true, &draft, None);
    p.install_reply(Ok(Reply::Installed));
    // `y` on the offer opens the Hugging Face screens.
    assert_eq!(
        p.install_answer(true, &draft, None),
        Outcome::Ask(hf::Ask::Popular)
    );
    assert!(p.hub().is_some());
}

#[test]
fn n_on_the_offer_closes_the_wizard() {
    let mut p = picker();
    let draft = AiConfig::default();
    p.enter(&draft, None);
    p.install_reply(Ok(Reply::Resolved(release())));
    p.install_answer(true, &draft, None);
    p.install_reply(Ok(Reply::Downloaded));
    p.install_answer(true, &draft, None);
    p.install_reply(Ok(Reply::Installed));
    assert_eq!(p.install_answer(false, &draft, None), Outcome::Stay);
    assert!(p.install().is_none());
}

#[test]
fn the_wizard_takes_no_text_and_no_refresh() {
    let mut p = picker();
    p.type_char('x');
    assert_eq!(p.text(), None);
    assert_eq!(p.refresh(), Outcome::Stay);
}
