//! The `AUTO` and `RUN` badges of a chat in auto or run approval mode (spec chat-pane,
//! "Approval mode badge").

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use super::*;
use crate::core::ai::chat::review::Badge;
use crate::core::ai::{
    chat::{
        SendOutcome,
        engine::{Availability, Finish, StreamEvent},
    },
    config::ApprovalMode,
};

fn theme() -> LazaroboxTheme {
    LazaroboxTheme::default()
}

fn answered() -> ChatState {
    let mut chat = ChatState::new(None);
    let ready = Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    };
    let SendOutcome::Sent { turn, .. } = chat.send("question", &ready, "") else {
        panic!("send refused");
    };
    chat.apply(turn, StreamEvent::Delta("an answer".into()));
    chat.set_usage_config(crate::core::ai::config::UsageConfig::without_line());
    chat.apply(turn, StreamEvent::Done(Finish::Stop));
    chat
}

/// A chat with tools on and in `mode`.
fn chat_in(mode: ApprovalMode) -> ChatState {
    let mut chat = answered();
    chat.set_run_hint(true);
    chat.set_approval_mode(mode);
    chat
}

fn draw(chat: &ChatState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    ChatView::new(chat, &theme()).render(area, &mut buf);
    buf
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn badge_style(bg: Color) -> Style {
    Style::default()
        .fg(theme().bg_base)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

/// The badge `text` at the top right, bold, `bg_base` on `bg`.
fn assert_badge_of(buf: &Buffer, text: &str, bg: Color) {
    let width = buf.area.width;
    let len = u16::try_from(text.len()).unwrap();
    assert!(row(buf, 0).ends_with(text), "{:?}", row(buf, 0));
    for x in width - len..width {
        let cell = &buf[(x, 0)];
        assert_eq!(cell.fg, badge_style(bg).fg.unwrap(), "fg at {x}");
        assert_eq!(cell.bg, bg, "bg at {x}");
        assert!(cell.modifier.contains(Modifier::BOLD), "bold at {x}");
    }
}

fn assert_badge(buf: &Buffer) {
    assert_badge_of(buf, " AUTO ", theme().warning_orange);
}

#[test]
fn auto_draws_the_badge_top_right_of_the_conversation() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 30, 6);
    assert_badge(&buf);
}

#[test]
fn the_badge_shows_in_the_list_and_editor_views_too() {
    let mut list = chat_in(ApprovalMode::Auto);
    list.open_list();
    assert_badge(&draw(&list, 40, 8));

    let mut editor = chat_in(ApprovalMode::Auto);
    editor.open_note_editor().expect("an answer to save");
    assert_badge(&draw(&editor, 40, 10));
}

#[test]
fn below_six_columns_the_badge_is_one_letter() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 5, 4);
    assert_eq!(buf[(4, 0)].symbol(), "A");
    assert_eq!(buf[(4, 0)].bg, theme().warning_orange);
    assert!(!row(&buf, 0).contains("AUTO"));
}

#[test]
fn run_draws_a_red_badge_in_every_view() {
    assert_badge_of(
        &draw(&chat_in(ApprovalMode::Run), 30, 6),
        " RUN ",
        theme().error_red,
    );
    let mut list = chat_in(ApprovalMode::Run);
    list.open_list();
    assert_badge_of(&draw(&list, 40, 8), " RUN ", theme().error_red);
    let mut editor = chat_in(ApprovalMode::Run);
    editor.open_note_editor().expect("an answer to save");
    assert_badge_of(&draw(&editor, 40, 10), " RUN ", theme().error_red);
}

#[test]
fn below_five_columns_the_run_badge_is_one_letter() {
    let four = draw(&chat_in(ApprovalMode::Run), 4, 4);
    assert_eq!(four[(3, 0)].symbol(), "R");
    assert_eq!(four[(3, 0)].bg, theme().error_red);
    assert!(!row(&four, 0).contains("RUN"));
    // Five columns still fit the whole word.
    let five = draw(&chat_in(ApprovalMode::Run), 5, 4);
    assert_eq!(row(&five, 0), " RUN ");
}

#[test]
fn the_badge_follows_the_mode_and_the_tools_flag() {
    let mut chat = answered();
    assert_eq!(chat.mode_badge(), None);
    chat.set_approval_mode(ApprovalMode::Auto);
    assert_eq!(chat.mode_badge(), None, "tools off");
    chat.set_run_hint(true);
    assert_eq!(chat.mode_badge(), Some(ApprovalMode::Auto));
    chat.set_approval_mode(ApprovalMode::Run);
    assert_eq!(chat.mode_badge(), Some(ApprovalMode::Run));
    chat.set_approval_mode(ApprovalMode::Ask);
    assert_eq!(chat.mode_badge(), None);
}

#[test]
fn ask_mode_and_tools_off_draw_no_marker() {
    let ask = draw(&chat_in(ApprovalMode::Ask), 30, 6);
    let mut off = chat_in(ApprovalMode::Auto);
    off.set_run_hint(false);
    let off = draw(&off, 30, 6);
    for buf in [ask, off] {
        for y in 0..buf.area.height {
            assert!(!row(&buf, y).contains("AUTO"), "{:?}", row(&buf, y));
            assert!((0..buf.area.width).all(|x| buf[(x, y)].bg != theme().warning_orange));
            assert!((0..buf.area.width).all(|x| buf[(x, y)].bg != theme().error_red));
        }
    }
}

#[test]
fn tiny_areas_do_not_panic() {
    for mode in [ApprovalMode::Auto, ApprovalMode::Run] {
        let chat = chat_in(mode);
        for width in 1..=10 {
            for height in 1..=2 {
                draw(&chat, width, height);
            }
        }
    }
}

#[test]
fn the_badge_snapshot() {
    let buf = draw(&chat_in(ApprovalMode::Auto), 30, 5);
    let shot = (0..buf.area.height)
        .map(|y| row(&buf, y).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("auto_badge_30x5", shot);
}

#[test]
fn the_run_badge_snapshot() {
    let buf = draw(&chat_in(ApprovalMode::Run), 30, 5);
    let shot = (0..buf.area.height)
        .map(|y| row(&buf, y).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("run_badge_30x5", shot);
}

// The batch badge (spec chat-pane, "Staged batch indicator"): top left, so it
// never meets the mode badge or the scroll badge on the right.

fn with_batch(mode: ApprovalMode, badge: Badge) -> ChatState {
    let mut chat = chat_in(mode);
    chat.set_batch_badge(Some(badge));
    chat
}

fn shot_of(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| row(buf, y).trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_staged_batch_shows_its_count_top_left() {
    let buf = draw(&with_batch(ApprovalMode::Ask, Badge::Staged(3)), 40, 5);
    assert!(row(&buf, 0).starts_with(" staged 3 "), "{:?}", row(&buf, 0));
    assert_eq!(buf[(1, 0)].bg, theme().info_blue);
    assert!(buf[(1, 0)].modifier.contains(Modifier::BOLD));
    insta::assert_snapshot!("staged_badge_40x5", shot_of(&buf));
}

#[test]
fn an_orphan_wears_a_distinct_badge_with_its_review_key() {
    let buf = draw(&with_batch(ApprovalMode::Ask, Badge::Orphan(2)), 40, 5);
    assert!(
        row(&buf, 0).starts_with(" orphan 2 · Ctrl+G "),
        "{:?}",
        row(&buf, 0)
    );
    assert_eq!(buf[(1, 0)].bg, theme().warning_orange);
    insta::assert_snapshot!("orphan_badge_40x5", shot_of(&buf));
}

#[test]
fn the_batch_badge_and_the_mode_badge_share_the_row_without_touching() {
    let buf = draw(&with_batch(ApprovalMode::Auto, Badge::Orphan(12)), 40, 5);
    let top = row(&buf, 0);
    assert!(top.starts_with(" orphan 12 · Ctrl+G "), "{top:?}");
    assert!(top.ends_with(" AUTO "), "{top:?}");
    insta::assert_snapshot!("orphan_and_auto_badges_40x5", shot_of(&buf));
}

#[test]
fn the_batch_badge_gets_shorter_before_it_meets_the_mode_badge() {
    let at = |width| {
        row(
            &draw(&with_batch(ApprovalMode::Run, Badge::Orphan(2)), width, 4),
            0,
        )
    };
    // Wide enough for the whole text, then for the count only, then for a letter.
    assert!(at(30).starts_with(" orphan 2 · Ctrl+G "), "{:?}", at(30));
    assert!(at(22).starts_with(" orphan 2 "), "{:?}", at(22));
    assert!(!at(22).contains("Ctrl+G"), "{:?}", at(22));
    assert!(at(15).starts_with(" O2 "), "{:?}", at(15));
    assert!(at(15).ends_with(" RUN "), "{:?}", at(15));
    // No room left: only the mode badge.
    let narrow = draw(&with_batch(ApprovalMode::Run, Badge::Orphan(2)), 9, 4);
    assert!(row(&narrow, 0).ends_with(" RUN "));
    assert!((0..4).all(|x| narrow[(x, 0)].bg != theme().warning_orange));
}

#[test]
fn the_batch_badge_shows_in_every_view_but_the_review() {
    let badge = Badge::Staged(1);
    let mut list = with_batch(ApprovalMode::Ask, badge);
    list.open_list();
    assert!(row(&draw(&list, 40, 8), 0).starts_with(" staged 1 "));
    let mut editor = with_batch(ApprovalMode::Ask, badge);
    editor.open_note_editor().expect("an answer to save");
    assert!(row(&draw(&editor, 40, 10), 0).starts_with(" staged 1 "));
    let mut none = chat_in(ApprovalMode::Ask);
    none.set_batch_badge(None);
    assert!(!row(&draw(&none, 40, 5), 0).contains("staged"));
}

#[test]
fn the_batch_badge_degrades_on_tiny_areas() {
    for badge in [Badge::Staged(7), Badge::Orphan(120)] {
        let chat = with_batch(ApprovalMode::Auto, badge);
        for width in 1..=24 {
            for height in 1..=2 {
                draw(&chat, width, height);
            }
        }
    }
}

#[test]
fn the_review_owns_the_pane_and_hides_the_batch_badge() {
    use crate::core::ai::chat::{
        approval::{ApprovalAsk, Origin},
        review_view::{ReviewView, testing::numbered},
    };
    let mut chat = with_batch(ApprovalMode::Ask, Badge::Orphan(3));
    let ask = ApprovalAsk::Batch(numbered(3));
    chat.approvals_mut().request(ask, Origin::Block).unwrap();
    chat.approvals_mut()
        .set_review(ReviewView::new(numbered(3), (40, 8)));
    let buf = draw(&chat, 40, 8);
    assert!(!row(&buf, 0).contains("orphan"), "{:?}", row(&buf, 0));
}

// The badge owns a row of its own: it never covers the transcript.

#[test]
fn the_batch_badge_keeps_the_first_transcript_row_visible() {
    for badge in [Badge::Staged(3), Badge::Orphan(2)] {
        let buf = draw(&with_batch(ApprovalMode::Ask, badge), 40, 6);
        assert!(row(&buf, 0).contains(if matches!(badge, Badge::Staged(_)) {
            "staged"
        } else {
            "orphan"
        }));
        assert!(row(&buf, 1).starts_with("› question"), "{:?}", row(&buf, 1));
        let bare = draw(&chat_in(ApprovalMode::Ask), 40, 6);
        assert!(
            row(&bare, 0).starts_with("› question"),
            "{:?}",
            row(&bare, 0)
        );
    }
}

#[test]
fn a_pane_too_small_to_spare_a_row_shows_the_transcript_and_no_badge() {
    // Transcript of one row: it keeps it, and the badge is not drawn.
    let chat = with_batch(ApprovalMode::Ask, Badge::Orphan(2));
    let buf = draw(&chat, 40, 2);
    assert!(!row(&buf, 0).contains("orphan"), "{:?}", row(&buf, 0));
    assert!(!row(&buf, 0).is_empty());
    let layout = ChatView::new(&chat, &theme()).layout(Rect::new(0, 0, 40, 2));
    assert_eq!(layout.transcript.height, 1);
}

#[test]
fn the_layout_gives_up_a_row_only_while_a_badge_is_drawn() {
    let area = Rect::new(0, 0, 40, 8);
    let bare = ChatView::new(&chat_in(ApprovalMode::Ask), &theme()).layout(area);
    let chat = with_batch(ApprovalMode::Ask, Badge::Staged(1));
    let badged = ChatView::new(&chat, &theme()).layout(area);
    assert_eq!(badged.transcript.y, bare.transcript.y + 1);
    assert_eq!(badged.transcript.height + 1, bare.transcript.height);
    assert_eq!(badged.composer, bare.composer);
}
