//! The llama-server install wizard (llama-server-install, wizard steps).

use super::install::{
    COPY_ROW, HINT_CANCEL_INSTALL, HINT_CANCELLING, HINT_CONFIRM_STEP, HINT_PLAN, HINT_WORKING,
    InstallAsk, Reply, Screen, Step, Wizard,
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

/// The id of the request in flight.
fn id_of(w: &Wizard) -> u64 {
    match w.asking().expect("a request in flight") {
        InstallAsk::Resolve(id, _)
        | InstallAsk::Download(id, _)
        | InstallAsk::Install(id, _)
        | InstallAsk::Remove(id, _) => id,
        InstallAsk::Cancel | InstallAsk::CheckNewer => panic!("no id"),
    }
}

/// Reply to the request in flight.
fn answer(w: &mut Wizard, result: Result<Reply, String>) -> bool {
    let id = id_of(w);
    w.reply(id, result)
}

fn preply(p: &mut Picker, result: Result<Reply, String>) {
    let id = id_of(p.install().expect("wizard"));
    p.install_reply(id, result);
}

fn wizard() -> Wizard {
    Wizard::new(vec![Variant::Cpu, Variant::Vulkan])
}

/// A wizard that chose the CPU build and got its release.
fn confirming_download() -> Wizard {
    let mut w = wizard();
    w.enter(0);
    answer(&mut w, Ok(Reply::Resolved(release())));
    w.arm();
    w
}

fn confirming_install() -> Wizard {
    let mut w = confirming_download();
    w.yes();
    answer(&mut w, Ok(Reply::Downloaded));
    w.arm();
    w
}

fn offering() -> Wizard {
    let mut w = confirming_install();
    w.yes();
    answer(&mut w, Ok(Reply::Installed));
    w.arm();
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
    let step = w.enter(1);
    let id = id_of(&w);
    assert_eq!(step, Step::Ask(InstallAsk::Resolve(id, Variant::Vulkan)));
    assert!(matches!(w.screen(), Screen::Resolving(Variant::Vulkan)));
    assert_eq!(w.asking(), Some(InstallAsk::Resolve(id, Variant::Vulkan)));
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
    let step = w.yes();
    assert_eq!(step, Step::Ask(InstallAsk::Download(id_of(&w), release())));
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
    let id = id_of(&w);
    w.back();
    assert!(!w.reply(id, Ok(Reply::Resolved(release()))));
    assert!(matches!(w.screen(), Screen::Plan));
    // A reply of the wrong kind is dropped too.
    let mut w = wizard();
    w.enter(0);
    answer(&mut w, Ok(Reply::Installed));
    assert!(matches!(w.screen(), Screen::Resolving(_)));
}

#[test]
fn the_install_waits_for_a_yes_after_the_verified_download() {
    let mut w = confirming_install();
    assert!(matches!(w.screen(), Screen::ConfirmInstall(_)));
    assert_eq!(w.asking(), None);
    let step = w.yes();
    assert_eq!(step, Step::Ask(InstallAsk::Install(id_of(&w), release())));
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
    answer(&mut w, Ok(Reply::Resolved(release())));
    w.arm();
    asks.push(w.yes());
    answer(&mut w, Ok(Reply::Downloaded));
    w.arm();
    asks.push(w.yes());
    let [
        Step::Ask(InstallAsk::Resolve(a, Variant::Cpu)),
        Step::Ask(InstallAsk::Download(b, downloaded)),
        Step::Ask(InstallAsk::Install(c, installed)),
    ] = asks.as_slice()
    else {
        panic!("not the three asks in order: {asks:?}");
    };
    assert!(a < b && b < c, "{asks:?}");
    assert_eq!((downloaded, installed), (&release(), &release()));
    answer(&mut w, Ok(Reply::Installed));
    w.arm();
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
    // The download is still stopping: "cancelling…" until its reply arrives.
    assert!(matches!(
        w.screen(),
        Screen::Downloading {
            cancelling: true,
            ..
        }
    ));
    assert!(!w.asks_cancel());
    assert!(w.asking().is_some());
    assert_eq!(w.hint(), HINT_CANCELLING);
    assert!(labels(&w).iter().any(|l| l.contains("cancelling")));
    assert_eq!(w.yes(), Step::Stay);
    assert_eq!(w.no(), Step::Stay);
    assert_eq!(w.back(), Step::Stay);
    assert!(answer(&mut w, Err("cancelled".to_owned())));
    assert!(matches!(w.screen(), Screen::Plan));
    assert_eq!(w.asking(), None);
}

#[test]
fn a_download_that_finishes_while_cancelling_still_ends_at_the_plan() {
    let mut w = confirming_download();
    w.yes();
    w.back();
    w.yes();
    assert!(answer(&mut w, Ok(Reply::Downloaded)));
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
    let id = id_of(&w);
    w.progress(id, 10_000_000);
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
    answer(
        &mut w,
        Err("GitHub rate limit reached\u{1b}[31m".to_owned()),
    );
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
    answer(&mut w, Err("llama-server --version failed".to_owned()));
    assert!(matches!(w.screen(), Screen::Failed(_)));
}

#[test]
fn every_screen_has_a_title_and_a_hint() {
    let mut w = wizard();
    assert!(!w.title().is_empty());
    assert_eq!(w.hint(), HINT_PLAN);
    w.enter(0);
    assert_eq!(w.hint(), HINT_WORKING);
    answer(&mut w, Ok(Reply::Resolved(release())));
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
    let outcome = p.enter(&draft, none);
    let id = id_of(p.install().unwrap());
    assert_eq!(
        outcome,
        Outcome::Install(InstallAsk::Resolve(id, Variant::Cpu))
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
    preply(&mut p, Ok(Reply::Resolved(release())));
    p.install_arm();
    let outcome = p.install_answer(true, &draft, None);
    let id = id_of(p.install().unwrap());
    assert_eq!(
        outcome,
        Outcome::Install(InstallAsk::Download(id, release()))
    );
    preply(&mut p, Ok(Reply::Downloaded));
    p.install_arm();
    p.install_answer(true, &draft, None);
    preply(&mut p, Ok(Reply::Installed));
    p.install_arm();
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
    preply(&mut p, Ok(Reply::Resolved(release())));
    p.install_arm();
    p.install_answer(true, &draft, None);
    preply(&mut p, Ok(Reply::Downloaded));
    p.install_arm();
    p.install_answer(true, &draft, None);
    preply(&mut p, Ok(Reply::Installed));
    p.install_arm();
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

// Review fixes: a key typed for one screen never answers the next.
#[test]
fn a_queued_y_for_the_cancel_prompt_does_not_start_the_install() {
    let mut w = confirming_download();
    w.yes();
    w.back();
    // The download ends while the cancel prompt waits; the `y` was typed for it.
    answer(&mut w, Ok(Reply::Downloaded));
    assert!(matches!(w.screen(), Screen::ConfirmInstall(_)));
    assert_eq!(w.yes(), Step::Stay);
    assert!(matches!(w.screen(), Screen::ConfirmInstall(_)));
    // Once drawn, the user's own `y` counts.
    w.arm();
    let step = w.yes();
    assert_eq!(step, Step::Ask(InstallAsk::Install(id_of(&w), release())));
}

#[test]
fn a_queued_y_does_not_confirm_the_download_the_user_never_saw() {
    let mut w = wizard();
    w.enter(0);
    answer(&mut w, Ok(Reply::Resolved(release())));
    assert_eq!(w.yes(), Step::Stay);
    assert_eq!(w.no(), Step::Stay);
    assert!(matches!(w.screen(), Screen::ConfirmDownload(_)));
    w.arm();
    assert!(matches!(w.yes(), Step::Ask(InstallAsk::Download(..))));
}

#[test]
fn a_queued_y_does_not_open_hugging_face_after_the_install() {
    let mut w = confirming_install();
    w.yes();
    answer(&mut w, Ok(Reply::Installed));
    assert!(matches!(w.screen(), Screen::OfferHf));
    assert_eq!(w.yes(), Step::Stay);
    assert_eq!(w.no(), Step::Stay);
    w.arm();
    assert_eq!(w.yes(), Step::Hub);
}

// Review fixes: a reply belongs to the request that asked for it.
#[test]
fn a_late_reply_for_a_left_lookup_is_dropped() {
    let mut w = wizard();
    w.enter(0);
    let cpu = id_of(&w);
    // Esc drops the lookup (idempotent: no cancel ask), Vulkan starts another.
    assert_eq!(w.back(), Step::Stay);
    let step = w.enter(1);
    let vulkan = id_of(&w);
    assert_eq!(
        step,
        Step::Ask(InstallAsk::Resolve(vulkan, Variant::Vulkan))
    );
    assert_ne!(cpu, vulkan);
    assert!(!w.reply(cpu, Ok(Reply::Resolved(release()))));
    assert!(matches!(w.screen(), Screen::Resolving(Variant::Vulkan)));
    assert!(w.reply(vulkan, Ok(Reply::Resolved(release()))));
    assert!(matches!(w.screen(), Screen::ConfirmDownload(_)));
}

#[test]
fn a_stale_error_does_not_fail_a_new_lookup() {
    let mut w = wizard();
    w.enter(0);
    let old = id_of(&w);
    w.back();
    w.enter(0);
    assert!(!w.reply(old, Err("cancelled".to_owned())));
    assert!(matches!(w.screen(), Screen::Resolving(_)));
}

#[test]
fn a_reply_is_taken_once() {
    let mut w = wizard();
    w.enter(0);
    let id = id_of(&w);
    assert!(w.reply(id, Ok(Reply::Resolved(release()))));
    assert!(!w.reply(id, Err("again".to_owned())));
    assert!(matches!(w.screen(), Screen::ConfirmDownload(_)));
}

#[test]
fn esc_while_resolving_drops_the_request_in_flight() {
    let mut w = wizard();
    w.enter(0);
    let id = id_of(&w);
    w.back();
    assert_eq!(w.asking(), None);
    assert!(!w.reply(id, Ok(Reply::Resolved(release()))));
    assert!(matches!(w.screen(), Screen::Plan));
}

// Review fix: the offer never opens a second download over a running one.
#[test]
fn y_on_the_offer_is_ignored_while_a_hugging_face_download_runs() {
    let mut p = picker();
    let draft = AiConfig::default();
    p.enter(&draft, None);
    preply(&mut p, Ok(Reply::Resolved(release())));
    p.install_arm();
    p.install_answer(true, &draft, None);
    preply(&mut p, Ok(Reply::Downloaded));
    p.install_arm();
    p.install_answer(true, &draft, None);
    preply(&mut p, Ok(Reply::Installed));
    p.install_arm();
    p.set_download(Some(Running {
        source: crate::core::ai::picker::Source::HuggingFace,
        file: "m.gguf".to_owned(),
        license: "mit".to_owned(),
        done: 1,
        total: 2,
    }));
    assert_eq!(p.install_answer(true, &draft, None), Outcome::Stay);
    assert!(!p.asks_cancel());
    assert!(p.hub().is_none());
    assert!(p.install().is_some());
}

// Review fix: ids are process-wide, so a closed wizard's late reply never
// matches the request of a new one.
#[test]
fn two_wizards_never_issue_the_same_id() {
    let (mut a, mut b) = (wizard(), wizard());
    a.enter(0);
    b.enter(0);
    assert_ne!(id_of(&a), id_of(&b));
}

#[test]
fn a_late_reply_from_a_closed_wizard_is_ignored_by_the_next_one() {
    let mut closed = wizard();
    closed.enter(0);
    let late = id_of(&closed);
    drop(closed);
    let mut next = wizard();
    next.enter(0);
    assert!(!next.reply(late, Ok(Reply::Resolved(release()))));
    assert!(matches!(next.screen(), Screen::Resolving(_)));
}

// Review fix: closing the wizard mid-download has something to stop.
#[test]
fn a_wizard_downloads_only_between_the_yes_and_the_reply() {
    let mut w = confirming_download();
    assert!(!w.downloading());
    w.yes();
    assert!(w.downloading());
    answer(&mut w, Ok(Reply::Downloaded));
    assert!(!w.downloading());
}

// Review fix: progress names its request; the size on screen stays the release's.
#[test]
fn progress_of_another_request_is_ignored_and_never_resizes_the_release() {
    let mut w = confirming_download();
    w.yes();
    let id = id_of(&w);
    w.progress(id + 1, 5);
    assert!(matches!(w.screen(), Screen::Downloading { done: 0, .. }));
    w.progress(id, 7);
    assert!(matches!(w.screen(), Screen::Downloading { done: 7, .. }));
    let Screen::Downloading { release: shown, .. } = w.screen() else {
        panic!("not downloading");
    };
    assert_eq!(shown.size, release().size);
}

// Remove stops a running managed server first, and the prompt says so.
#[test]
fn the_remove_prompt_says_a_running_managed_server_is_stopped_first() {
    let w = Wizard::remove(std::path::PathBuf::from(
        "/h/lazarobox/llama.cpp/current/llama-server",
    ));
    let text = labels(&w).join(" ");
    assert!(text.contains("stopped first"), "{text}");
}
