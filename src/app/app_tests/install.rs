//! The llama-server install wizard of the llamacpp picker: its asks go out as
//! effects, the replies and the progress come back as events (design D9).

use super::hf_download::{at_llamacpp, cursor_label, enter};
use super::*;
use crate::core::ai::install::{Release, Variant};
use crate::core::ai::picker::{COPY_ROW, InstallAsk, InstallReply};

fn release() -> Release {
    Release {
        tag: "b9000".to_owned(),
        asset: "llama-b9000-bin-ubuntu-x64.tar.gz".to_owned(),
        size: 1000,
        sha256: "a".repeat(64),
    }
}

fn esc() -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
}

/// The id of the request the open wizard waits on.
fn pending(a: &App) -> u64 {
    match a
        .menu()
        .unwrap()
        .install_asking()
        .expect("a request in flight")
    {
        InstallAsk::Resolve(id, _) | InstallAsk::Download(id, _) | InstallAsk::Install(id, _) => id,
        InstallAsk::Cancel => unreachable!("cancel has no id"),
    }
}

fn installs(effects: &[Effect]) -> Vec<InstallAsk> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Install(ask) => Some(ask.clone()),
            _ => None,
        })
        .collect()
}

/// The wizard open over llamacpp's models, with the CPU and Vulkan builds.
fn wizard() -> App {
    let mut a = at_llamacpp();
    a.menu
        .as_mut()
        .unwrap()
        .open_install_for_test(vec![Variant::Cpu, Variant::Vulkan]);
    a
}

/// What the screen shows once a frame was drawn: the key a prompt waits for
/// is armed only then.
fn drawn(a: &mut App) {
    a.frame_drawn();
}

/// Enter on the CPU build and the reply with `release()`: the confirm screen.
fn at_confirm_download() -> App {
    let mut a = wizard();
    let asked = a.update(enter());
    assert_eq!(
        installs(&asked),
        vec![InstallAsk::Resolve(pending(&a), Variant::Cpu)]
    );
    a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Resolved(release())),
    ));
    drawn(&mut a);
    a
}

fn at_downloading() -> App {
    let mut a = at_confirm_download();
    let asked = a.update(key('y'));
    assert_eq!(
        installs(&asked),
        vec![InstallAsk::Download(pending(&a), release())]
    );
    a
}

fn at_confirm_install() -> App {
    let mut a = at_downloading();
    a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Downloaded),
    ));
    drawn(&mut a);
    a
}

fn footer(a: &App) -> String {
    match a.menu().unwrap().footer() {
        crate::core::menu::Footer::Hint(text) => text.into_owned(),
        crate::core::menu::Footer::Error(text) => text.to_owned(),
    }
}

// Scenario: choosing a build is the consent to look it up.
#[test]
fn enter_on_a_build_asks_the_runtime_to_resolve_it() {
    let mut a = wizard();
    let effects = a.update(enter());
    assert_eq!(
        installs(&effects),
        vec![InstallAsk::Resolve(pending(&a), Variant::Cpu)]
    );
    assert_eq!(effects.len(), 1, "{effects:?}");
}

#[test]
fn the_copy_row_copies_and_asks_nothing() {
    let mut a = wizard();
    a.update(key('j'));
    a.update(key('j'));
    assert_eq!(cursor_label(&a), COPY_ROW);
    let effects = a.update(enter());
    assert!(installs(&effects).is_empty(), "{effects:?}");
    assert!(
        effects.iter().any(|e| matches!(e, Effect::SetClipboard(_))),
        "{effects:?}"
    );
}

// Scenario: the replies walk the wizard through its steps.
#[test]
fn replies_walk_resolve_download_install_and_the_offer() {
    let mut a = at_confirm_download();
    assert_eq!(footer(&a), "y continue · n back");
    a.update(key('y'));
    assert_eq!(footer(&a), "Esc cancel the download");
    a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Downloaded),
    ));
    drawn(&mut a);
    let asked = a.update(key('y'));
    assert_eq!(
        installs(&asked),
        vec![InstallAsk::Install(pending(&a), release())]
    );
    assert_eq!(footer(&a), "installing…");
}

// Scenario: a key queued for the previous screen answers nothing.
#[test]
fn a_reply_opened_prompt_ignores_y_until_a_frame_drew_it() {
    let mut a = wizard();
    a.update(enter());
    a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Resolved(release())),
    ));
    let early = a.update(key('y'));
    assert!(installs(&early).is_empty(), "{early:?}");
    drawn(&mut a);
    let late = a.update(key('y'));
    assert_eq!(
        installs(&late),
        vec![InstallAsk::Download(pending(&a), release())]
    );
}

#[test]
fn n_on_a_confirm_screen_goes_back_to_the_plan_without_asking() {
    let mut a = at_confirm_download();
    let effects = a.update(key('n'));
    assert!(installs(&effects).is_empty(), "{effects:?}");
    assert_eq!(cursor_label(&a), "CPU build");
}

// Scenario: progress shows on the wizard's screen.
#[test]
fn progress_shows_as_a_percentage_on_the_download_screen() {
    let mut a = at_downloading();
    a.update(AppEvent::InstallProgress {
        id: pending(&a),
        done: 250,
        total: 1000,
    });
    assert_eq!(
        cursor_label_any(&a),
        "↓ 25% llama-b9000-bin-ubuntu-x64.tar.gz"
    );
}

fn cursor_label_any(a: &App) -> String {
    a.menu()
        .unwrap()
        .rows(&a.config)
        .into_iter()
        .find_map(|row| match row {
            Row::Pick { label, .. } => Some(label),
            _ => None,
        })
        .expect("a wizard row")
}

// Scenario: Esc asks, y cancels, and the cancel ends with a reply.
#[test]
fn esc_asks_and_y_cancels_the_download_then_its_reply_returns_to_the_plan() {
    let mut a = at_downloading();
    let asked = a.update(esc());
    assert!(installs(&asked).is_empty(), "{asked:?}");
    assert_eq!(footer(&a), "cancel the download? (y/n)");
    let cancel = a.update(key('y'));
    assert_eq!(installs(&cancel), vec![InstallAsk::Cancel]);
    assert_eq!(footer(&a), "cancelling…");
    a.update(AppEvent::InstallReply(
        pending(&a),
        Err("cancelled".to_owned()),
    ));
    assert_eq!(cursor_label(&a), "CPU build");
    assert!(a.notice().is_none(), "{:?}", a.notice());
}

#[test]
fn n_keeps_the_download() {
    let mut a = at_downloading();
    a.update(esc());
    let effects = a.update(key('n'));
    assert!(installs(&effects).is_empty(), "{effects:?}");
    assert_eq!(footer(&a), "Esc cancel the download");
}

// Scenario: Esc on the plan leaves the wizard for the models level.
#[test]
fn esc_on_the_plan_closes_the_wizard() {
    let mut a = wizard();
    a.update(esc());
    assert!(!a.menu().unwrap().install_open());
}

// Scenario: Installed makes the engines be detected again, once.
#[test]
fn installed_redetects_the_engines_once() {
    let mut a = at_confirm_install();
    a.update(key('y'));
    let effects = a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Installed),
    ));
    assert_eq!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::RedetectEngines))
            .count(),
        1,
        "{effects:?}"
    );
}

#[test]
fn installed_redetects_even_when_the_menu_was_closed() {
    let mut a = at_confirm_install();
    a.update(key('y'));
    let id = pending(&a);
    a.menu = None;
    a.input = InputMode::Terminal;
    let effects = a.update(AppEvent::InstallReply(id, Ok(InstallReply::Installed)));
    assert!(
        effects.iter().any(|e| matches!(e, Effect::RedetectEngines)),
        "{effects:?}"
    );
}

#[test]
fn a_failure_shows_its_text_and_a_stale_reply_changes_nothing() {
    let mut a = at_downloading();
    let failed = a.update(AppEvent::InstallReply(pending(&a), Err("boom".to_owned())));
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(cursor_label_any(&a), "boom");
    // A reply that is not the one in flight changes nothing on the screen.
    a.update(AppEvent::InstallReply(
        u64::MAX,
        Ok(InstallReply::Downloaded),
    ));
    assert_eq!(cursor_label_any(&a), "boom");
}

// Scenario: the attestation warning is shown, not swallowed.
#[test]
fn a_warning_becomes_the_notice() {
    let mut a = at_downloading();
    a.update(AppEvent::InstallWarning("gh is not installed".to_owned()));
    assert_eq!(a.notice(), Some("gh is not installed"));
}

// Scenario: after the install, `y` on the offer opens the Hugging Face screens.
#[test]
fn y_on_the_offer_asks_for_the_popular_models() {
    let mut a = at_confirm_install();
    a.update(key('y'));
    a.update(AppEvent::InstallReply(
        pending(&a),
        Ok(InstallReply::Installed),
    ));
    drawn(&mut a);
    let effects = a.update(key('y'));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::HubAsk(crate::core::ai::hf::Ask::Popular))),
        "{effects:?}"
    );
}

// Review fix: closing the menu while the archive downloads stops the download.
#[test]
fn closing_the_menu_while_downloading_cancels_the_download() {
    let mut a = at_downloading();
    let effects = a.close_menu(Outcome::Reverted);
    assert_eq!(installs(&effects), vec![InstallAsk::Cancel]);
}

#[test]
fn closing_the_menu_with_nothing_downloading_asks_nothing() {
    let mut a = at_confirm_download();
    let effects = a.close_menu(Outcome::Reverted);
    assert!(installs(&effects).is_empty(), "{effects:?}");
}

// Review fix: progress of another request does not move the bar.
#[test]
fn progress_of_a_request_that_is_not_in_flight_is_ignored() {
    let mut a = at_downloading();
    let id = pending(&a);
    a.update(AppEvent::InstallProgress {
        id: id + 1000,
        done: 900,
        total: 1000,
    });
    assert_eq!(
        cursor_label_any(&a),
        "↓ 0% llama-b9000-bin-ubuntu-x64.tar.gz"
    );
    a.update(AppEvent::InstallProgress {
        id,
        done: 500,
        total: 1000,
    });
    assert_eq!(
        cursor_label_any(&a),
        "↓ 50% llama-b9000-bin-ubuntu-x64.tar.gz"
    );
}
