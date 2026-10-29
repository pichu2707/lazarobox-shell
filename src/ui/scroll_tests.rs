//! The scroll badge, the COPY key hint and the scroll position, end to end
//! through `ui::render` (pane-scroll S1).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{
    app::AppEvent,
    core::{
        ai::{
            Detection, EngineState, Status,
            chat::engine::{Finish, StreamEvent},
            config::{AiConfig, Assignment, Task},
        },
        layout::PaneId,
        pty::PtyEvent,
    },
};

const COLS: u16 = 100;
const ROWS: u16 = 12;

fn key(c: char, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new(KeyCode::Char(c), mods))
}

fn keys(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(key(c, KeyModifiers::NONE));
    }
}

fn prefixed(app: &mut App, text: &str) {
    app.update(key(' ', KeyModifiers::CONTROL));
    keys(app, text);
}

fn code(app: &mut App, code: KeyCode) {
    app.update(AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn frame(app: &App, cols: u16, rows: u16) -> Vec<String> {
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    terminal.draw(|f| render(f, app, &theme)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..rows)
        .map(|y| (0..cols).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn rows(app: &App) -> Vec<String> {
    frame(app, COLS, ROWS)
}

fn status(app: &App) -> String {
    rows(app).pop().unwrap()
}

fn text(app: &App) -> String {
    rows(app)
        .iter()
        .map(|r| r.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn with_history(app: &mut App, lines: usize) {
    for i in 0..lines {
        app.update(AppEvent::Pty(
            PaneId::FIRST,
            PtyEvent::Output(format!("line{i}\r\n").into_bytes()),
        ));
    }
}

fn shell_app() -> App {
    let mut app = App::new(COLS, ROWS).with_env(Some("/bin/zsh"), None, "/work".into());
    with_history(&mut app, 80);
    app
}

fn ollama() -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Chat,
        Assignment {
            engine: Some("ollama".into()),
            model: Some("llama3:8b".into()),
        },
    );
    ai
}

fn chat_id() -> PaneId {
    PaneId::for_test(2)
}

/// A shell on the left and a focused chat on the right with a long answer.
fn chat_app() -> App {
    let mut app = App::new(COLS, ROWS)
        .with_env(Some("/bin/zsh"), None, "/srv/app".into())
        .with_ai(ollama());
    app.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            binary: None,
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".into()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }));
    prefixed(&mut app, "c");
    app.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    keys(&mut app, "q");
    let effects = app.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    let turn = match effects.as_slice() {
        [crate::app::Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    let answer: String = (0..80).map(|i| format!("answer {i}\n")).collect();
    app.update(AppEvent::Chat(chat_id(), turn, StreamEvent::Delta(answer)));
    app.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    app
}

fn first_row(app: &App) -> String {
    rows(app).remove(0).trim_end().to_owned()
}

// Badge ---------------------------------------------------------------------------

#[test]
fn a_wheel_scrolled_shell_pane_shows_the_badge_top_right() {
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    assert!(first_row(&app).ends_with("[+12]"), "{}", first_row(&app));
    insta::assert_snapshot!("badge_shell_wheel_scrolled", text(&app));
}

#[test]
fn the_badge_changes_with_the_offset() {
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 7);
    assert!(first_row(&app).ends_with("[+7]"), "{}", first_row(&app));
    app.scroll_shell_for_test(PaneId::FIRST, 15);
    assert!(first_row(&app).ends_with("[+15]"), "{}", first_row(&app));
}

#[test]
fn a_live_pane_shows_no_badge() {
    let app = shell_app();
    assert!(!text(&app).contains("[+"), "{}", text(&app));
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 5);
    app.scroll_shell_for_test(PaneId::FIRST, 0);
    assert!(!text(&app).contains("[+"));
    assert!(!text(&app).contains("[+0]"));
}

#[test]
fn a_shell_pane_in_copy_shows_the_badge() {
    let mut app = shell_app();
    prefixed(&mut app, "[");
    keys(&mut app, &"k".repeat(15));
    let n = app.scrolled_by(PaneId::FIRST).expect("scrolled");
    assert!(n > 0);
    assert!(
        first_row(&app).ends_with(&format!("[+{n}]")),
        "{}",
        first_row(&app)
    );
    insta::assert_snapshot!("badge_shell_copy", text(&app));
}

#[test]
fn a_scrolled_chat_shows_the_badge_and_a_chat_at_the_bottom_does_not() {
    let mut app = chat_app();
    assert!(!text(&app).contains("[+"), "{}", text(&app));
    code(&mut app, KeyCode::PageUp);
    let n = app.scrolled_by(chat_id()).expect("scrolled");
    assert!(
        first_row(&app).ends_with(&format!("[+{n}]")),
        "{}",
        first_row(&app)
    );
    insta::assert_snapshot!("badge_chat_scrolled", text(&app));
}

#[test]
fn an_unfocused_scrolled_pane_keeps_its_badge() {
    let mut app = chat_app();
    code(&mut app, KeyCode::PageUp);
    let n = app.scrolled_by(chat_id()).expect("scrolled");
    prefixed(&mut app, "h");
    assert_eq!(app.focused(), PaneId::FIRST);
    assert!(
        first_row(&app).ends_with(&format!("[+{n}]")),
        "{}",
        first_row(&app)
    );
    assert!(!status(&app).contains('\u{2191}'), "{}", status(&app));
}

#[test]
fn the_alternate_screen_shows_no_badge() {
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    app.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(b"\x1b[?1049h".to_vec()),
    ));
    assert!(!text(&app).contains("[+"), "{}", text(&app));
}

#[test]
fn a_tiny_scrolled_pane_shows_no_badge_and_nothing_panics() {
    for cols in 1..=8 {
        let mut app = App::new(cols, 6);
        with_history(&mut app, 30);
        app.scroll_shell_for_test(PaneId::FIRST, 12);
        let shown = frame(&app, cols, 6);
        if cols < 5 {
            assert!(!shown.join("\n").contains('+'), "{cols}: {shown:?}");
        }
    }
    let mut app = App::new(3, 6);
    with_history(&mut app, 30);
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    insta::assert_snapshot!("badge_tiny_pane", frame(&app, 3, 6).join("\n"));
}

#[test]
fn the_badge_does_not_resize_the_pane() {
    let mut app = shell_app();
    let before = app.focused_size();
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    assert_eq!(app.focused_size(), before);
    // The content under the badge is still the pane's own text.
    let first = frame(&app, COLS, ROWS)[0].clone();
    assert!(first.starts_with("line"), "{first}");
}

// Statusline position -------------------------------------------------------------

#[test]
fn a_scrolled_focused_shell_reads_terminal_with_its_position() {
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    let (offset, total) = app.scroll_position().expect("scrolled");
    assert_eq!(offset, 12);
    let line = status(&app);
    assert!(
        line.contains(&format!("TERMINAL \u{2191}12/{total}")),
        "{line}"
    );
    insta::assert_snapshot!("position_terminal_scrolled", line.trim_end());
}

#[test]
fn a_scrolled_focused_chat_reads_chat_with_its_position() {
    let mut app = chat_app();
    code(&mut app, KeyCode::PageUp);
    let (offset, total) = app.scroll_position().expect("scrolled");
    let line = status(&app);
    assert!(
        line.contains(&format!("CHAT \u{2191}{offset}/{total}")),
        "{line}"
    );
}

#[test]
fn a_live_focused_pane_reads_the_bare_label() {
    assert!(status(&shell_app()).contains("TERMINAL "));
    assert!(!status(&shell_app()).contains('\u{2191}'));
    assert!(!status(&chat_app()).contains('\u{2191}'));
}

#[test]
fn chat_copy_always_shows_the_position_even_at_zero() {
    let mut app = chat_app();
    prefixed(&mut app, "[");
    let (_, total) = app.scroll_position().expect("always");
    let line = status(&app);
    assert!(line.contains(&format!("COPY \u{2191}0/{total}")), "{line}");
    app.update(key('u', KeyModifiers::CONTROL));
    let (offset, _) = app.scroll_position().expect("always");
    assert!(offset > 0);
    assert!(
        status(&app).contains(&format!("COPY \u{2191}{offset}/{total}")),
        "{}",
        status(&app)
    );
}

#[test]
fn the_zoom_marker_comes_after_the_position() {
    let mut app = shell_app();
    prefixed(&mut app, "wz");
    assert!(app.zoomed());
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    let (_, total) = app.scroll_position().expect("scrolled");
    let line = status(&app);
    assert!(
        line.contains(&format!("TERMINAL \u{2191}12/{total} [Z]")),
        "{line}"
    );
}

#[test]
fn the_alternate_screen_shows_no_position() {
    let mut app = shell_app();
    app.scroll_shell_for_test(PaneId::FIRST, 12);
    app.update(AppEvent::Pty(
        PaneId::FIRST,
        PtyEvent::Output(b"\x1b[?1049h".to_vec()),
    ));
    assert!(!status(&app).contains('\u{2191}'), "{}", status(&app));
}

#[test]
fn a_scrolled_pane_never_panics_at_any_width() {
    for cols in 0..=40 {
        let mut app = App::new(cols.max(1), 6);
        with_history(&mut app, 30);
        app.scroll_shell_for_test(PaneId::FIRST, 12);
        let _ = frame(&app, cols, 6);
    }
}

// COPY hint -----------------------------------------------------------------------

const SHELL_HINT: &str = "j/k scroll \u{b7} v select \u{b7} y copy \u{b7} Esc exit";
const CHAT_HINT: &str =
    "j/k move \u{b7} v select \u{b7} y copy \u{b7} ^Y answer \u{b7} ^B code \u{b7} Esc exit";

#[test]
fn shell_copy_shows_the_shell_hint_in_the_path_segment() {
    let mut app = shell_app();
    prefixed(&mut app, "[");
    let line = status(&app);
    assert!(line.contains("COPY"), "{line}");
    assert!(line.contains(SHELL_HINT), "{line}");
    assert!(!line.contains("/work"), "the hint replaces the cwd: {line}");
    insta::assert_snapshot!("hint_shell_copy", line.trim_end());
}

#[test]
fn chat_copy_shows_its_hint_with_the_chat_copies() {
    let mut app = chat_app();
    prefixed(&mut app, "[");
    let line = status(&app);
    assert!(line.contains(CHAT_HINT), "{line}");
    insta::assert_snapshot!("hint_chat_copy", line.trim_end());
}

#[test]
fn a_selection_keeps_the_shell_hint() {
    let mut app = shell_app();
    prefixed(&mut app, "[");
    keys(&mut app, "v");
    let line = status(&app);
    assert!(line.contains("VISUAL"), "{line}");
    assert!(line.contains(SHELL_HINT), "{line}");
}

#[test]
fn a_notice_replaces_the_hint_until_the_next_key() {
    let mut app = App::new(COLS, ROWS).with_env(Some("/bin/zsh"), None, "/work".into());
    prefixed(&mut app, "[");
    keys(&mut app, "vy");
    let line = status(&app);
    assert!(line.contains("Nothing to copy"), "{line}");
    assert!(!line.contains(SHELL_HINT), "{line}");
    keys(&mut app, "l");
    assert!(status(&app).contains(SHELL_HINT), "{}", status(&app));
}

#[test]
fn leaving_copy_brings_the_cwd_back() {
    let mut app = shell_app();
    prefixed(&mut app, "[");
    code(&mut app, KeyCode::Esc);
    let line = status(&app);
    assert!(
        line.contains("TERMINAL") && line.contains("/work"),
        "{line}"
    );
    assert!(!line.contains("scroll"), "{line}");
    let mut app = chat_app();
    prefixed(&mut app, "[");
    code(&mut app, KeyCode::Esc);
    let line = status(&app);
    assert!(line.contains("CHAT") && line.contains("/srv/app"), "{line}");
}

#[test]
fn a_narrow_path_segment_clips_the_hint_by_whole_entries() {
    let mut app = App::new(56, 4).with_env(Some("/bin/zsh"), None, "/work".into());
    prefixed(&mut app, "[");
    let line = frame(&app, 56, 4).pop().unwrap();
    assert!(line.contains('\u{2026}'), "{line}");
    assert!(!line.contains("Esc exit"), "{line}");
    let hint = line
        .split("j/k")
        .nth(1)
        .map(|rest| format!("j/k{rest}"))
        .unwrap_or_default();
    // Whole entries only: every entry kept is one of the four.
    let kept: Vec<&str> = hint
        .trim_end()
        .split(" \u{b7} ")
        .map(|e| e.trim().trim_end_matches('\u{2026}').trim())
        .filter(|e| !e.is_empty() && !e.starts_with("sh"))
        .collect();
    for entry in &kept[..kept.len().saturating_sub(1)] {
        assert!(SHELL_HINT.split(" \u{b7} ").any(|h| h == *entry), "{entry}");
    }
    insta::assert_snapshot!("hint_clipped", line.trim_end());
}

#[test]
fn copy_never_panics_at_any_width() {
    for cols in 0..=40 {
        let mut app = App::new(cols.max(1), 4);
        with_history(&mut app, 10);
        prefixed(&mut app, "[");
        let _ = frame(&app, cols, 4);
        let mut chat = App::new(cols.max(1), 4).with_ai(ollama());
        prefixed(&mut chat, "c");
        prefixed(&mut chat, "[");
        let _ = frame(&chat, cols, 4);
    }
}

// The COPY cursor and selection in a chat ------------------------------------------

/// The buffer and the real cursor of one frame.
fn drawn(app: &App) -> (ratatui::buffer::Buffer, ratatui::layout::Position) {
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
    terminal.draw(|f| render(f, app, &theme)).unwrap();
    let cursor = terminal.get_cursor_position().unwrap();
    (terminal.backend().buffer().clone(), cursor)
}

/// The screen cell where the chat pane draws the text `needle`.
fn cell_of(app: &App, needle: &str) -> (u16, u16) {
    let x = app
        .tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == chat_id())
        .map(|(_, rect)| rect.x)
        .expect("the chat pane");
    let y = rows(app)
        .iter()
        .position(|row| {
            row.chars()
                .skip(usize::from(x))
                .collect::<String>()
                .starts_with(needle)
        })
        .expect("drawn");
    (x, u16::try_from(y).unwrap())
}

#[test]
fn chat_copy_puts_the_cursor_on_the_transcript_instead_of_the_composer() {
    let mut app = chat_app();
    keys(&mut app, "hi");
    let (_, typing) = drawn(&app);
    prefixed(&mut app, "[");
    assert!(matches!(app.input(), crate::app::InputMode::ChatCopy(_)));
    let (x, y) = cell_of(&app, "answer 79");
    let (_, copying) = drawn(&app);
    assert_ne!(typing, copying, "the composer cursor is not shown in COPY");
    // The answer ends in a newline: its last row is the blank one below.
    assert_eq!(
        (copying.x, copying.y),
        (x, y + 1),
        "on the last visible row"
    );
    keys(&mut app, "kl");
    let (_, moved) = drawn(&app);
    assert_eq!((moved.x, moved.y), (x + 1, y));
}

#[test]
fn a_chat_selection_is_drawn_reversed() {
    use ratatui::style::Modifier;
    let mut app = chat_app();
    prefixed(&mut app, "[");
    keys(&mut app, "kvlllll");
    let (x, y) = cell_of(&app, "answer 79");
    let (buf, _) = drawn(&app);
    let reversed = |col: u16| buf[(col, y)].modifier.contains(Modifier::REVERSED);
    assert!((x..x + 6).all(reversed), "`answer` is selected");
    assert!(!reversed(x + 6));
    assert!(!buf[(x, y - 1)].modifier.contains(Modifier::REVERSED));
    // Line selection: the whole row.
    keys(&mut app, "V");
    let (buf, _) = drawn(&app);
    assert!(buf[(x + 20, y)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn with_a_batch_badge_the_chat_copy_cursor_stays_inside_the_transcript() {
    use crate::{app::PaneView, core::ai::chat::review::Badge};
    let mut app = chat_app();
    app.sync_batches(&std::collections::HashMap::from([(
        chat_id(),
        Badge::Staged(3),
    )]));
    prefixed(&mut app, "[");
    app.update(key('u', KeyModifiers::CONTROL));
    let marks = app.chat_copy_marks().expect("chat COPY");
    let Some(PaneView::Chat(chat)) = app.pane_view(chat_id()) else {
        panic!("the chat pane");
    };
    let (x, _) = cell_of(&app, "answer");
    let lines = chat.lines(usize::from(COLS - x));
    let text = lines[marks.cursor.line].text.clone();
    let (_, y) = cell_of(&app, &text);
    let (_, cursor) = drawn(&app);
    assert_eq!(
        (cursor.x, cursor.y),
        (x + marks.cursor.col, y),
        "on {text:?}"
    );
}
