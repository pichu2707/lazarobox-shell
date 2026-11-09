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

// The click (D1, D4) ------------------------------------------------------------

mod click {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    use super::super::chat_live::{answered, chat_id};
    use super::*;

    const LINK: &str = "http://localhost:3000/x";

    fn mouse(a: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> Vec<Effect> {
        a.update(AppEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
    }

    fn click(a: &mut App, at: (u16, u16)) -> Vec<Effect> {
        assert_eq!(mouse(a, MouseEventKind::Down(MouseButton::Left), at), []);
        mouse(a, MouseEventKind::Up(MouseButton::Left), at)
    }

    fn opened(url: &str) -> Vec<Effect> {
        vec![Effect::OpenUrl(url.to_owned())]
    }

    /// A shell that printed `text` on its first row.
    fn shell(text: &str) -> (App, Rect) {
        let mut a = launched(80, 25).with_opener(true);
        a.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(text.as_bytes().to_vec()),
        ));
        let rect = a.tiling().panes[0].1;
        (a, rect)
    }

    #[test]
    fn a_click_on_a_shell_link_opens_it() {
        let (mut a, r) = shell("see http://localhost:3000/x now");
        assert_eq!(click(&mut a, (r.x + 6, r.y)), opened(LINK));
        assert_eq!(a.notice.as_deref(), Some("Opened http://localhost:3000/x"));
    }

    #[test]
    fn the_press_alone_opens_nothing() {
        let (mut a, r) = shell(LINK);
        let at = (r.x + 3, r.y);
        assert_eq!(
            mouse(&mut a, MouseEventKind::Down(MouseButton::Left), at),
            []
        );
    }

    #[test]
    fn a_release_on_another_cell_opens_nothing() {
        let (mut a, r) = shell(LINK);
        mouse(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            (r.x + 3, r.y),
        );
        let up = mouse(
            &mut a,
            MouseEventKind::Up(MouseButton::Left),
            (r.x + 4, r.y),
        );
        assert_eq!(up, []);
    }

    #[test]
    fn a_release_with_no_press_opens_nothing() {
        let (mut a, r) = shell(LINK);
        let at = (r.x + 3, r.y);
        assert_eq!(mouse(&mut a, MouseEventKind::Up(MouseButton::Left), at), []);
    }

    #[test]
    fn a_click_beside_a_link_opens_nothing() {
        let (mut a, r) = shell("see http://localhost:3000/x now");
        assert_eq!(click(&mut a, (r.x, r.y)), []);
        assert_eq!(click(&mut a, (r.x + 27, r.y)), [], "the space after it");
        assert_eq!(click(&mut a, (r.x + 6, r.y + 1)), [], "the row below");
    }

    #[test]
    fn a_link_that_wraps_opens_whole_from_either_row() {
        let url = format!("https://a.io/{}", "x".repeat(100));
        let (mut a, r) = shell(&url);
        assert_eq!(click(&mut a, (r.x + 2, r.y)), opened(&url));
        assert_eq!(click(&mut a, (r.x + 2, r.y + 1)), opened(&url));
    }

    // Half a link must never pass for a whole one: the view ends inside it.
    #[test]
    fn a_link_taller_than_the_view_opens_whole() {
        let url = format!("https://a.io/{}", "x".repeat(80 * 30));
        let (mut a, r) = shell(&url);
        assert_eq!(click(&mut a, (r.x + 2, r.y)), opened(&url));
    }

    #[test]
    fn a_click_in_the_second_pane_reads_that_pane() {
        let (mut a, _) = shell("nothing here");
        press(&mut a, "wv");
        let (id, r) = a.tiling().panes[1];
        a.update(AppEvent::Pty(
            id,
            PtyEvent::Output(LINK.as_bytes().to_vec()),
        ));
        assert_eq!(click(&mut a, (r.x + 3, r.y)), opened(LINK));
    }

    #[test]
    fn a_link_scrolled_into_view_can_be_clicked_in_copy() {
        let (mut a, r) = shell(LINK);
        press(&mut a, "[");
        assert!(matches!(a.input(), InputMode::Copy(_)));
        assert_eq!(click(&mut a, (r.x + 3, r.y)), opened(LINK));
    }

    #[test]
    fn in_prefix_a_click_opens_nothing() {
        let (mut a, r) = shell(LINK);
        a.update(ctrl_space());
        assert_eq!(click(&mut a, (r.x + 3, r.y)), []);
    }

    #[test]
    fn with_the_mouse_off_a_click_opens_nothing() {
        let (mut a, r) = shell(LINK);
        a.config.mouse = false;
        assert_eq!(click(&mut a, (r.x + 3, r.y)), []);
    }

    #[test]
    fn with_no_opener_the_link_is_copied_at_once() {
        let (a, r) = shell(LINK);
        let mut a = a.with_opener(false);
        let effects = click(&mut a, (r.x + 3, r.y));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::SetClipboard(_))),
            "{effects:?}"
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::OpenUrl(_))));
        assert_eq!(
            a.notice.as_deref(),
            Some("The browser did not open: link copied")
        );
    }

    #[test]
    fn a_file_link_is_never_opened() {
        let (mut a, r) = shell("file:///etc/passwd");
        assert_eq!(click(&mut a, (r.x + 3, r.y)), []);
    }

    /// The screen cell of the first occurrence of `needle` in the chat.
    fn chat_cell(a: &App, needle: &str) -> (u16, u16) {
        let id = chat_id();
        let (_, rect) = a
            .tiling()
            .panes
            .iter()
            .find(|(pane, _)| *pane == id)
            .copied()
            .unwrap();
        let state = a.panes.get(&id).unwrap();
        let chat = state.chat_state().unwrap();
        let lines = chat.lines(usize::from(state.size.cols));
        let drawn = a.chat_viewport(id);
        let top = chat.scroll().top(lines.len(), drawn);
        let (index, col) = lines
            .iter()
            .enumerate()
            .skip(top)
            .find_map(|(i, line)| {
                line.text
                    .find(needle)
                    .map(|at| (i, line.text[..at].chars().count()))
            })
            .expect("the needle is drawn");
        let first = chat.transcript_first_row(state.size.cols, state.size.rows);
        (
            rect.x + u16::try_from(col).unwrap(),
            rect.y + first + u16::try_from(index - top).unwrap(),
        )
    }

    #[test]
    fn a_click_on_a_link_in_an_answer_opens_only_the_url() {
        let mut a = answered("Read [the docs](https://a.io/docs).").with_opener(true);
        let (col, row) = chat_cell(&a, "https://a.io/docs");
        assert_eq!(click(&mut a, (col + 2, row)), opened("https://a.io/docs"));
    }

    #[test]
    fn a_link_that_wraps_in_the_chat_opens_whole_from_either_row() {
        let url = format!("https://a.io/{}", "x".repeat(120));
        let mut a = answered(&format!("Link: {url} end")).with_opener(true);
        let (col, row) = chat_cell(&a, "https://a.io/");
        assert_eq!(click(&mut a, (col + 2, row)), opened(&url));
        assert_eq!(click(&mut a, (col + 2, row + 1)), opened(&url));
    }

    #[test]
    fn a_click_on_a_local_server_in_an_answer_opens_it() {
        let mut a = answered("It runs on http://localhost:5173/ now.").with_opener(true);
        let (col, row) = chat_cell(&a, "http://localhost");
        assert_eq!(
            click(&mut a, (col + 2, row)),
            opened("http://localhost:5173/")
        );
    }

    #[test]
    fn a_click_on_the_chat_text_around_a_link_opens_nothing() {
        let mut a = answered("Read https://a.io/docs now").with_opener(true);
        let (col, row) = chat_cell(&a, "Read");
        assert_eq!(click(&mut a, (col, row)), []);
    }

    #[test]
    fn the_composer_is_not_part_of_the_transcript() {
        let mut a = answered("hello").with_opener(true);
        let r = a.tiling().panes[0].1;
        assert_eq!(click(&mut a, (r.x + 2, r.y + r.height - 1)), []);
    }
}
