//! A link the browser did not open is copied instead, also when no menu is
//! open to say so (pane-links D5).

use super::*;
use crate::core::clipboard::Tool;

const URL: &str = "http://localhost:3000/x";

fn notice(a: &App) -> Option<&str> {
    a.notice.as_deref()
}

#[test]
fn outside_the_menu_a_link_that_did_not_open_is_copied() {
    let mut a = launched(80, 25);
    let effects = a.update(AppEvent::OpenFailed(URL.to_owned()));
    assert!(
        effects.iter().any(|e| matches!(e, Effect::SetClipboard(_))),
        "{effects:?}"
    );
    assert_eq!(notice(&a), Some("The browser did not open: link copied"));
}

#[test]
fn outside_the_menu_the_notice_follows_the_local_tool() {
    let mut a = launched(80, 25).with_clipboard(Some(Tool::WlCopy), false);
    let effects = a.update(AppEvent::OpenFailed(URL.to_owned()));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::CopyLocal { text, .. } if text == URL)),
        "{effects:?}"
    );
    assert_eq!(notice(&a), Some("The browser did not open: copying link"));
    a.update(AppEvent::Clipboard {
        seq: 1,
        result: Ok(()),
    });
    assert_eq!(
        notice(&a),
        Some("The browser did not open: Copied (wl-copy)")
    );
}

#[test]
fn a_link_too_large_to_copy_says_so() {
    let mut a = launched(80, 25);
    let huge = format!("https://a.io/{}", "x".repeat(4 << 20));
    let effects = a.update(AppEvent::OpenFailed(huge));
    assert_eq!(effects, []);
    assert_eq!(notice(&a), Some("The browser did not open"));
}

// The runtime's rule takes `http` for the panes: the menu keeps its own.
#[test]
fn the_menu_does_not_hand_an_http_link_to_the_opener() {
    let mut a = launched(80, 25);
    let effects = a.on_menu_command(MenuCommand::Open("http://a.io".to_owned()));
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::OpenUrl(_))),
        "{effects:?}"
    );
}

#[test]
fn the_menu_still_opens_an_https_link() {
    let mut a = launched(80, 25);
    let effects = a.on_menu_command(MenuCommand::Open("https://a.io".to_owned()));
    assert_eq!(effects, [Effect::OpenUrl("https://a.io".to_owned())]);
}
