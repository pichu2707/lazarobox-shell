//! The screens of the llama-server install wizard (llama-server-install,
//! design D9), drawn in the picker's popup.

use super::*;
use crate::core::ai::install::{Release, Variant};
use crate::core::ai::picker::{InstallAsk, InstallReply};

fn release() -> Release {
    Release {
        tag: "b9000".to_owned(),
        asset: "llama-b9000-bin-ubuntu-x64.tar.gz".to_owned(),
        size: 41_000_000,
        sha256: "a".repeat(64),
    }
}

/// The wizard open over llamacpp's models, with the CPU and Vulkan builds.
fn wizard() -> MenuState {
    let mut menu = picker_of("code", 0);
    for _ in 0..10 {
        let on_llamacpp = menu
            .rows(&Config::default())
            .iter()
            .any(|r| matches!(r, Row::Pick { label, selected: true, .. } if label == "llamacpp"));
        if on_llamacpp {
            break;
        }
        press(&mut menu, &[KeyCode::Char('j')]);
    }
    press(&mut menu, &[KeyCode::Enter]);
    menu.open_install_for_test(vec![Variant::Cpu, Variant::Vulkan]);
    menu
}

/// The id of the request the wizard waits on.
fn pending(menu: &MenuState) -> u64 {
    match menu.install_asking().expect("a request in flight") {
        InstallAsk::Resolve(id, _) | InstallAsk::Download(id, _) | InstallAsk::Install(id, _) => id,
        InstallAsk::Cancel => unreachable!("cancel has no id"),
    }
}

fn confirm_download() -> MenuState {
    let mut menu = wizard();
    press(&mut menu, &[KeyCode::Enter]);
    let id = pending(&menu);
    menu.install_reply(id, Ok(InstallReply::Resolved(release())));
    menu.install_arm();
    menu
}

fn downloading() -> MenuState {
    let mut menu = confirm_download();
    press(&mut menu, &[KeyCode::Char('y')]);
    let id = pending(&menu);
    menu.install_progress(id, 10_250_000);
    menu
}

fn confirm_install() -> MenuState {
    let mut menu = downloading();
    let id = pending(&menu);
    menu.install_reply(id, Ok(InstallReply::Downloaded));
    menu.install_arm();
    menu
}

fn offer() -> MenuState {
    let mut menu = confirm_install();
    press(&mut menu, &[KeyCode::Char('y')]);
    let id = pending(&menu);
    menu.install_reply(id, Ok(InstallReply::Installed));
    menu
}

fn show(menu: &MenuState) -> String {
    text(&draw(62, POPUP_H, menu))
}

fn footer_line(menu: &MenuState) -> String {
    show(menu)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty() && !line.contains('└'))
        .unwrap_or_default()
        .trim()
        .trim_matches('│')
        .trim()
        .to_owned()
}

#[test]
fn snapshot_install_plan() {
    insta::assert_snapshot!(show(&wizard()));
}

#[test]
fn snapshot_install_resolving() {
    let mut menu = wizard();
    press(&mut menu, &[KeyCode::Enter]);
    insta::assert_snapshot!(show(&menu));
}

#[test]
fn snapshot_install_confirm_download() {
    insta::assert_snapshot!(show(&confirm_download()));
}

#[test]
fn snapshot_install_downloading() {
    insta::assert_snapshot!(show(&downloading()));
}

#[test]
fn snapshot_install_cancel_prompt() {
    let mut menu = downloading();
    press(&mut menu, &[KeyCode::Esc]);
    insta::assert_snapshot!(show(&menu));
}

#[test]
fn snapshot_install_cancelling() {
    let mut menu = downloading();
    press(&mut menu, &[KeyCode::Esc, KeyCode::Char('y')]);
    insta::assert_snapshot!(show(&menu));
}

#[test]
fn snapshot_install_confirm_install() {
    insta::assert_snapshot!(show(&confirm_install()));
}

#[test]
fn snapshot_install_installing() {
    let mut menu = confirm_install();
    press(&mut menu, &[KeyCode::Char('y')]);
    insta::assert_snapshot!(show(&menu));
}

#[test]
fn snapshot_install_offer() {
    insta::assert_snapshot!(show(&offer()));
}

#[test]
fn snapshot_install_failed() {
    let mut menu = downloading();
    let id = pending(&menu);
    menu.install_reply(id, Err("download failed: checksum mismatch".to_owned()));
    insta::assert_snapshot!(show(&menu));
}

// Spec: the smallest popup still shows a wizard screen in full.
#[test]
fn snapshot_install_at_the_smallest_popup() {
    for (name, menu) in [
        ("plan", wizard()),
        ("confirm_download", confirm_download()),
        ("downloading", downloading()),
    ] {
        insta::assert_snapshot!(
            format!("install_smallest_{name}"),
            text(&draw(MIN_WIDTH, MIN_HEIGHT, &menu))
        );
    }
}

#[test]
fn the_footer_names_the_keys_of_each_screen() {
    assert_eq!(footer_line(&wizard()), "j/k move · Enter choose · Esc back");
    assert_eq!(footer_line(&confirm_download()), "y continue · n back");
    assert_eq!(footer_line(&downloading()), "Esc cancel the download");
    let mut menu = downloading();
    press(&mut menu, &[KeyCode::Esc]);
    assert_eq!(footer_line(&menu), "cancel the download? (y/n)");
    press(&mut menu, &[KeyCode::Char('y')]);
    assert_eq!(footer_line(&menu), "cancelling…");
    assert_eq!(footer_line(&offer()), "y download a model · n close");
}

#[test]
fn a_wizard_popup_never_panics_at_any_size() {
    for menu in [wizard(), confirm_download(), downloading(), offer()] {
        for w in 0..=40 {
            for h in 0..=20 {
                draw(w, h, &menu);
            }
        }
    }
}
