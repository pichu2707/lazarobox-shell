//! The run-in-pane confirm and the hint under the last shell block
//! (chat-pane: "Approval prompt", "Run action on code blocks").

use std::time::{Duration, Instant};

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::*;
use crate::core::ai::chat::{
    Entry, SendOutcome,
    engine::{Availability, Finish, StreamEvent},
    transcript::Scroll,
    yank,
};

const BLOCK: &str = "Try this:\n```bash\ncd ~/dev/curso-rust\n```";

fn base() -> Instant {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn at(ms: u64) -> Instant {
    base() + Duration::from_millis(ms)
}

fn ready() -> Availability {
    Availability::Ready {
        engine: "ollama".into(),
        model: Some("llama3:8b".into()),
    }
}

/// A chat with one finished exchange whose answer is `answer`.
fn exchange(answer: &str) -> ChatState {
    let mut chat = ChatState::new(None);
    let SendOutcome::Sent { .. } = chat.send_at("how?", &ready(), "", at(0)) else {
        panic!("send refused");
    };
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta(answer.into()), at(10));
    chat.apply_at(turn, StreamEvent::Done(Finish::Stop), at(20));
    chat
}

fn prompt(command: &str) -> ApprovalPrompt {
    ApprovalPrompt {
        target: "run in pane 2 (~/dev)".into(),
        command: command.into(),
    }
}

fn draw(chat: &ChatState, approval: Option<&ApprovalPrompt>, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let theme = LazaroboxTheme::default();
    ChatView::new(chat, &theme)
        .approval(approval)
        .render(area, &mut buf);
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn with_hint(answer: &str) -> ChatState {
    let mut chat = exchange(answer);
    chat.set_run_hint(true);
    chat
}

// The approval prompt -------------------------------------------------------

#[test]
fn the_pane_command_prompt_names_the_pane_the_command_and_the_keys() {
    let chat = exchange(BLOCK);
    let shot = draw(&chat, Some(&prompt("cd ~/dev/curso-rust")), 60, 16);
    insta::assert_snapshot!("approval_pane_command_60x16", shot);
    assert!(shot.contains("run in pane 2 (~/dev)"));
    assert!(shot.contains("cd ~/dev/curso-rust"));
    assert!(shot.contains("y type"));
    assert!(shot.contains("no Enter"));
    assert!(shot.contains("n cancel"));
    assert!(shot.contains("p next pane"));
}

#[test]
fn a_multi_line_command_with_a_tab_is_drawn_visibly_line_by_line() {
    let chat = exchange(BLOCK);
    let shot = draw(
        &chat,
        Some(&prompt("for f in *; do\n\techo $f\ndone")),
        60,
        16,
    );
    insta::assert_snapshot!("approval_multiline_with_tab", shot);
    assert!(shot.contains("for f in *; do"));
    assert!(
        shot.contains("→echo $f"),
        "a tab is drawn as an arrow: {shot}"
    );
    assert!(shot.contains("done"));
    assert!(!shot.contains('\t'));
}

#[test]
fn a_long_command_is_clipped_marked_and_its_line_count_stated() {
    let command = (1..=20)
        .map(|n| format!("echo line{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let chat = exchange(BLOCK);
    let shot = draw(&chat, Some(&prompt(&command)), 60, 16);
    insta::assert_snapshot!("approval_long_command_clipped", shot);
    assert!(shot.contains("echo line1"));
    assert!(!shot.contains("echo line20"));
    assert!(shot.contains("20 lines"), "{shot}");
}

#[test]
fn the_prompt_is_drawn_even_when_the_transcript_is_scrolled_up() {
    let mut chat = exchange(&format!("{}\n{}", "filler\n".repeat(40), BLOCK));
    chat.set_scroll(Scroll::Pinned(0));
    let shot = draw(&chat, Some(&prompt("cd /tmp")), 40, 12);
    assert!(shot.contains("run in pane 2 (~/dev)"), "{shot}");
    assert!(shot.contains("cd /tmp"), "{shot}");
}

#[test]
fn the_real_cursor_is_hidden_while_the_prompt_is_up() {
    let chat = exchange(BLOCK);
    let theme = LazaroboxTheme::default();
    let area = Rect::new(0, 0, 40, 12);
    assert!(ChatView::new(&chat, &theme).cursor(area).is_some());
    let p = prompt("ls");
    assert!(
        ChatView::new(&chat, &theme)
            .approval(Some(&p))
            .cursor(area)
            .is_none()
    );
}

#[test]
fn a_prompt_in_a_tiny_pane_never_panics_and_never_hides_the_command_silently() {
    let chat = exchange(BLOCK);
    let p = prompt("a\nb\nc");
    for height in 1..=12 {
        for width in 1..=24 {
            let shot = draw(&chat, Some(&p), width, height);
            if height >= 3 && width >= 24 {
                assert!(shot.contains('a'), "{width}x{height}: {shot}");
            }
        }
    }
    // Too small for three lines: the prompt says it is clipped.
    let shot = draw(&chat, Some(&p), 40, 3);
    assert!(shot.contains("3 lines"), "{shot}");
}

// The hint under the block --------------------------------------------------

#[test]
fn the_hint_is_drawn_right_under_the_last_shell_block() {
    let chat = with_hint(BLOCK);
    let shot = draw(&chat, None, 50, 12);
    insta::assert_snapshot!("hint_under_the_shell_block", shot);
    let lines: Vec<&str> = shot.lines().collect();
    let close = lines.iter().rposition(|l| l.trim() == "```").unwrap();
    assert!(lines[close + 1].contains("Ctrl+Space e"), "{shot}");
}

#[test]
fn there_is_no_hint_without_a_runnable_block_or_with_tools_off() {
    let off = exchange(BLOCK);
    let no_hint = draw(&off, None, 50, 12);
    assert!(!no_hint.contains("Ctrl+Space e"), "{no_hint}");
    for answer in [
        "```rust\nfn main() {}\n```",
        "```\nls\n```",
        "```bash\nls",
        "plain",
    ] {
        let chat = with_hint(answer);
        let shot = draw(&chat, None, 50, 12);
        assert!(!shot.contains("Ctrl+Space e"), "{answer:?}: {shot}");
        let same = draw(&exchange(answer), None, 50, 12);
        assert_eq!(shot, same, "{answer:?}");
    }
}

#[test]
fn there_is_no_hint_while_the_message_is_still_streaming() {
    let mut chat = with_hint("hello");
    let SendOutcome::Sent { .. } = chat.send_at("again", &ready(), "", at(30)) else {
        panic!("send refused");
    };
    let turn = chat.turn();
    chat.apply_at(turn, StreamEvent::Delta(BLOCK.into()), at(40));
    let shot = draw(&chat, None, 50, 12);
    assert!(!shot.contains("Ctrl+Space e"), "{shot}");
}

#[test]
fn the_hint_is_not_content() {
    let chat = with_hint(BLOCK);
    let hint = "Ctrl+Space e";
    assert!(!yank::last_answer(&chat).unwrap().contains(hint));
    assert!(!yank::last_code_block(&chat).unwrap().contains(hint));
    assert!(chat.messages().all(|m| !m.text.contains(hint)));
    assert!(
        chat.entries()
            .iter()
            .all(|e| !matches!(e, Entry::System(t) if t.contains(hint)))
    );
}
