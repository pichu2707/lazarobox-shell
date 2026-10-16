//! The live chat pane (ai-chat C8): `Prefix c`, the chat key table, yank,
//! scrolling and the reasoning toggle.

use super::*;
use crate::core::{
    ai::{
        EngineState, Status,
        chat::{
            Entry, ReasoningState,
            engine::{Finish, StreamEvent},
            transcript::Scroll,
        },
        config::{Assignment, Task},
    },
    copy::osc52,
};

pub(super) fn chat_id() -> PaneId {
    id2()
}

pub(super) fn code(code: KeyCode, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new(code, mods))
}

pub(super) fn ctrl(c: char) -> AppEvent {
    code(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub(super) fn plain(code_: KeyCode) -> AppEvent {
    code(code_, KeyModifiers::NONE)
}

pub(super) fn repeat(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new_with_kind(
        code,
        modifiers,
        KeyEventKind::Repeat,
    ))
}

pub(super) fn chat_of(a: &App) -> &ChatState {
    match a.pane_view(chat_id()) {
        Some(PaneView::Chat(chat)) => chat,
        _ => panic!("pane 2 is not a chat"),
    }
}

pub(super) fn composer(a: &App) -> String {
    chat_of(a).composer().text()
}

pub(super) fn type_text(a: &mut App, text: &str) {
    for c in text.chars() {
        a.update(key(c));
    }
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

fn detection(models: &[&str]) -> Detection {
    Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: models.iter().map(|m| (*m).to_owned()).collect(),
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
    }
}

/// A launched app with a ready ollama engine and a focused chat pane (id 2).
pub(super) fn ready_app() -> App {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    assert_eq!(a.focused(), chat_id());
    a.update(prompt_ready(SYSTEM, &[]));
    a.take_dirty();
    a
}

/// What the runtime would answer to the `PreparePrompt` of the chat pane.
fn prompt_ready(system: &str, notices: &[&str]) -> AppEvent {
    AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: system.to_owned(),
            notices: notices.iter().map(|n| (*n).to_owned()).collect(),
        },
    )
}

/// The system prompt `ready_app` delivers.
const SYSTEM: &str = "system prompt\n\nworkspace block";

pub(super) fn delta(turn: u64, text: &str) -> AppEvent {
    AppEvent::Chat(chat_id(), turn, StreamEvent::Delta(text.into()))
}

fn thinking(turn: u64, text: &str) -> AppEvent {
    AppEvent::Chat(chat_id(), turn, StreamEvent::ThinkingDelta(text.into()))
}

pub(super) fn finish(turn: u64) -> AppEvent {
    AppEvent::Chat(chat_id(), turn, StreamEvent::Done(Finish::Stop))
}

/// Types `question`, sends it with Enter and returns the effects.
pub(super) fn ask(a: &mut App, question: &str) -> Vec<Effect> {
    type_text(a, question);
    a.update(plain(KeyCode::Enter))
}

/// A chat that asked `q` and got `answer` back in full.
pub(super) fn answered(answer: &str) -> App {
    let mut a = ready_app();
    let turn = match ask(&mut a, "q").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    a.update(delta(turn, answer));
    a.update(finish(turn));
    a.take_dirty();
    a
}

/// A chat with a request in flight (waiting for the first byte).
fn streaming() -> (App, u64) {
    let mut a = ready_app();
    let turn = match ask(&mut a, "hi").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    };
    (a, turn)
}

// 9.1: Prefix c -------------------------------------------------------------

#[test]
fn prefix_c_opens_a_focused_chat_to_the_right_without_spawning_anything() {
    let mut a = launched(80, 25);
    let effects = press(&mut a, "c");
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(a.focused(), chat_id());
    assert!(matches!(a.pane_view(chat_id()), Some(PaneView::Chat(_))));
    assert_eq!(a.tiling().panes.len(), 2);
    let tiling = a.tiling();
    let shell = tiling.panes[0].1;
    let chat = tiling.panes[1].1;
    assert!(chat.x > shell.x, "the chat is to the right of the shell");
    assert!(
        effects.iter().all(|e| matches!(
            e,
            Effect::ResizePty(PaneId::FIRST, _)
                | Effect::DetectEngines
                | Effect::PreparePrompt { .. }
        )),
        "{effects:?}"
    );
}

#[test]
fn a_chat_opened_from_a_shell_remembers_the_shell_cwd() {
    let mut a = launched(80, 25);
    a.update(cwd_event("/srv/app"));
    press(&mut a, "c");
    assert_eq!(chat_of(&a).origin_cwd(), Some(Path::new("/srv/app")));
}

#[test]
fn opening_a_chat_in_a_pane_too_small_changes_nothing() {
    let mut a = launched(15, 5);
    let effects = press(&mut a, "c");
    assert_eq!(a.tiling().panes.len(), 1);
    assert_eq!(a.focused(), PaneId::FIRST);
    assert!(!effects.contains(&Effect::DetectEngines), "{effects:?}");
}

#[test]
fn a_chat_can_open_another_independent_chat() {
    let mut a = launched(120, 25);
    press(&mut a, "c");
    type_text(&mut a, "draft");
    press(&mut a, "c");
    let second = a.focused();
    assert_eq!(a.tiling().panes.len(), 3);
    assert_ne!(second, chat_id());
    assert_eq!(composer(&a), "draft", "the first chat keeps its composer");
    let Some(PaneView::Chat(new)) = a.pane_view(second) else {
        panic!("a second chat");
    };
    assert_eq!(new.composer().text(), "");
}

#[test]
fn opening_a_chat_requests_exactly_one_detection() {
    let mut a = launched(120, 25);
    let first = press(&mut a, "c");
    assert_eq!(
        first
            .iter()
            .filter(|e| **e == Effect::DetectEngines)
            .count(),
        1,
        "{first:?}"
    );
    let second = press(&mut a, "c");
    assert!(
        !second.contains(&Effect::DetectEngines),
        "a run is in flight: {second:?}"
    );
}

#[test]
fn c_inside_a_group_or_another_mode_opens_no_chat() {
    let mut a = launched(80, 25);
    press(&mut a, "w");
    a.update(key('c'));
    assert_eq!(a.tiling().panes.len(), 1);
    assert_eq!(a.input(), InputMode::Terminal);
    // COPY and RESIZE take their own keys.
    press(&mut a, "[");
    a.update(key('c'));
    assert_eq!(a.tiling().panes.len(), 1);
    a.update(key('q'));
    press(&mut a, "wr");
    a.update(key('c'));
    assert_eq!(a.tiling().panes.len(), 1);
}

// 9.2: the chat key table ---------------------------------------------------

#[test]
fn typing_edits_the_composer_and_redraws() {
    let mut a = ready_app();
    assert_eq!(a.update(key('h')), vec![]);
    assert!(a.take_dirty(), "an edit redraws the chat");
    a.update(key('i'));
    assert_eq!(a.update(plain(KeyCode::Backspace)), vec![]);
    assert_eq!(composer(&a), "h");
    a.update(plain(KeyCode::Left));
    a.update(plain(KeyCode::Delete));
    assert_eq!(composer(&a), "");
}

#[test]
fn shifted_letters_are_typed() {
    let mut a = ready_app();
    a.update(code(KeyCode::Char('H'), KeyModifiers::SHIFT));
    assert_eq!(composer(&a), "H");
}

#[test]
fn enter_sends_and_the_newline_bindings_do_not() {
    let mut a = ready_app();
    type_text(&mut a, "hi");
    assert_eq!(a.update(code(KeyCode::Enter, KeyModifiers::ALT)), vec![]);
    assert_eq!(a.update(ctrl('j')), vec![]);
    assert_eq!(composer(&a), "hi\n\n");
    let effects = a.update(plain(KeyCode::Enter));
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { pane, .. }] if *pane == chat_id()),
        "{effects:?}"
    );
    assert_eq!(composer(&a), "", "a sent message leaves the composer empty");
    assert_eq!(chat_of(&a).messages().count(), 1);
}

#[test]
fn a_send_carries_the_assigned_engine_and_model() {
    let mut a = ready_app();
    let effects = ask(&mut a, "hello");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.engine, "ollama");
    assert_eq!(request.model.as_deref(), Some("llama3:8b"));
}

#[test]
fn a_send_pins_the_prepared_system_prompt() {
    let mut a = ready_app();
    let effects = ask(&mut a, "hello");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
}

#[test]
fn a_blank_send_does_nothing() {
    let mut a = ready_app();
    type_text(&mut a, "   ");
    assert_eq!(a.update(plain(KeyCode::Enter)), vec![]);
    assert_eq!(composer(&a), "   ");
    assert_eq!(chat_of(&a).messages().count(), 0);
}

#[test]
fn a_send_while_streaming_is_refused_and_keeps_the_text() {
    let (mut a, _) = streaming();
    type_text(&mut a, "next");
    assert_eq!(a.update(plain(KeyCode::Enter)), vec![]);
    assert_eq!(composer(&a), "next");
}

#[test]
fn ctrl_u_clears_the_composer() {
    let mut a = ready_app();
    type_text(&mut a, "draft");
    assert_eq!(a.update(ctrl('u')), vec![]);
    assert_eq!(composer(&a), "");
}

#[test]
fn esc_and_ctrl_c_cancel_a_running_stream_and_keep_the_partial_answer() {
    for cancel in [plain(KeyCode::Esc), ctrl('c')] {
        let (mut a, turn) = streaming();
        a.update(delta(turn, "Hel"));
        assert_eq!(
            a.update(cancel.clone()),
            vec![Effect::ChatCancel(chat_id())]
        );
        assert_eq!(chat_of(&a).phase(), Phase::Idle);
        let last = chat_of(&a).messages().last().expect("the partial answer");
        assert_eq!(last.text, "Hel", "{cancel:?}");
    }
}

#[test]
fn esc_and_ctrl_c_on_an_idle_chat_do_nothing_and_never_quit() {
    for stop in [plain(KeyCode::Esc), ctrl('c')] {
        let mut a = ready_app();
        type_text(&mut a, "keep me");
        assert_eq!(a.update(stop.clone()), vec![], "{stop:?}");
        assert_eq!(composer(&a), "keep me");
        assert_eq!(a.input(), InputMode::Terminal);
    }
}

#[test]
fn unlisted_keys_are_swallowed() {
    let mut a = ready_app();
    type_text(&mut a, "ab");
    for event in [
        plain(KeyCode::Tab),
        plain(KeyCode::F(5)),
        ctrl('x'),
        plain(KeyCode::Up),
        plain(KeyCode::Down),
        code(KeyCode::Char('x'), KeyModifiers::ALT),
        code(KeyCode::Enter, KeyModifiers::SHIFT),
        plain(KeyCode::Insert),
    ] {
        assert_eq!(a.update(event.clone()), vec![], "{event:?}");
        assert_eq!(composer(&a), "ab", "{event:?}");
    }
}

#[test]
fn chat_keys_never_write_a_pty() {
    let mut a = ready_app();
    let events = [
        key('a'),
        plain(KeyCode::Enter),
        plain(KeyCode::Esc),
        ctrl('c'),
        plain(KeyCode::Tab),
        plain(KeyCode::Backspace),
        ctrl('y'),
        ctrl('b'),
        ctrl('r'),
        ctrl('u'),
        plain(KeyCode::PageUp),
        plain(KeyCode::PageDown),
    ];
    for event in events {
        let effects = a.update(event.clone());
        assert!(
            effects.iter().all(|e| !matches!(e, Effect::WritePty(..))),
            "{event:?} -> {effects:?}"
        );
    }
}

#[test]
fn repeat_edits_and_scrolls_but_never_decides() {
    let mut a = answered("hello");
    type_text(&mut a, "ab");
    assert_eq!(
        a.update(repeat(KeyCode::Char('c'), KeyModifiers::NONE)),
        vec![]
    );
    assert_eq!(composer(&a), "abc");
    a.update(repeat(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(composer(&a), "ab");
    for event in [
        repeat(KeyCode::Enter, KeyModifiers::NONE),
        repeat(KeyCode::Esc, KeyModifiers::NONE),
        repeat(KeyCode::Char('c'), KeyModifiers::CONTROL),
        repeat(KeyCode::Char('y'), KeyModifiers::CONTROL),
        repeat(KeyCode::Char('b'), KeyModifiers::CONTROL),
        repeat(KeyCode::Char('r'), KeyModifiers::CONTROL),
    ] {
        assert_eq!(a.update(event.clone()), vec![], "{event:?}");
    }
    assert_eq!(composer(&a), "ab", "nothing was sent or cleared");
    assert_eq!(a.notice(), None, "a held Ctrl+Y does not yank");
}

#[test]
fn a_paste_goes_into_the_composer_and_sends_nothing() {
    let mut a = ready_app();
    assert_eq!(a.update(AppEvent::Paste("x\r\ny\u{1b}[31m".into())), vec![]);
    assert_eq!(composer(&a), "x\ny[31m");
    assert!(a.take_dirty());
    assert_eq!(chat_of(&a).messages().count(), 0);
}

#[test]
fn the_prefix_still_works_on_a_chat() {
    let mut a = ready_app();
    type_text(&mut a, "draft");
    a.update(ctrl_space());
    assert_eq!(a.input(), InputMode::Prefix);
    a.update(esc());
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(composer(&a), "draft", "a mode change keeps the composer");
    // The second Ctrl+Space is swallowed, not inserted as a NUL.
    a.update(ctrl_space());
    a.update(ctrl_space());
    assert_eq!(composer(&a), "draft");
    press(&mut a, "wv");
    assert_eq!(a.tiling().panes.len(), 3);
    assert_eq!(a.input(), InputMode::Terminal);
}

#[test]
fn the_composer_survives_the_menu_and_the_menu_does_not_feed_it() {
    let mut a = ready_app();
    type_text(&mut a, "draft");
    press(&mut a, "m");
    assert_eq!(a.input(), InputMode::Menu);
    a.update(key('x'));
    a.update(AppEvent::Paste("pasted".into()));
    a.update(esc());
    assert_eq!(a.input(), InputMode::Terminal);
    assert_eq!(composer(&a), "draft");
}

// Yank ------------------------------------------------------------------------

#[test]
fn ctrl_y_copies_the_last_answer_and_says_how_much() {
    let mut a = answered("hello");
    let effects = a.update(ctrl('y'));
    assert_eq!(effects, vec![Effect::SetClipboard(osc52("hello").unwrap())]);
    assert_eq!(a.notice(), Some("Copied 5 characters"));
    assert!(a.take_dirty());
}

#[test]
fn a_one_character_yank_is_singular() {
    let mut a = answered("x");
    a.update(ctrl('y'));
    assert_eq!(a.notice(), Some("Copied 1 character"));
}

#[test]
fn yanking_with_no_answer_says_so() {
    let mut a = ready_app();
    assert_eq!(a.update(ctrl('y')), vec![]);
    assert_eq!(a.notice(), Some("Nothing to copy"));
    assert_eq!(a.update(ctrl('b')), vec![]);
    assert_eq!(a.notice(), Some("Nothing to copy"));
}

#[test]
fn a_huge_answer_is_too_large_to_yank() {
    let mut a = answered(&"a".repeat(80_000));
    assert_eq!(a.update(ctrl('y')), vec![]);
    assert_eq!(a.notice(), Some("Selection too large to copy"));
}

#[test]
fn ctrl_b_copies_the_last_code_block_without_its_fences() {
    let mut a = answered("text\n```rust\nlet x = 1;\n```\nmore");
    let effects = a.update(ctrl('b'));
    assert_eq!(
        effects,
        vec![Effect::SetClipboard(osc52("let x = 1;").unwrap())]
    );
    assert_eq!(a.notice(), Some("Copied 10 characters"));
}

#[test]
fn ctrl_b_without_a_code_block_has_nothing_to_copy() {
    let mut a = answered("plain text");
    assert_eq!(a.update(ctrl('b')), vec![]);
    assert_eq!(a.notice(), Some("Nothing to copy"));
}

#[test]
fn a_yank_never_includes_the_reasoning_and_leaves_the_transcript_alone() {
    let mut a = ready_app();
    let turn = match ask(&mut a, "q").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(thinking(turn, "secret plan"));
    a.update(delta(turn, "hello"));
    a.update(finish(turn));
    let before = chat_of(&a).entries().to_vec();
    let effects = a.update(ctrl('y'));
    assert_eq!(effects, vec![Effect::SetClipboard(osc52("hello").unwrap())]);
    assert_eq!(chat_of(&a).entries(), before.as_slice());
}

// Scroll ----------------------------------------------------------------------

#[test]
fn page_keys_scroll_the_transcript_and_clamp_at_both_ends() {
    let mut a = answered(&"line\n".repeat(100));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
    assert_eq!(a.update(plain(KeyCode::PageUp)), vec![]);
    assert!(a.take_dirty());
    let Scroll::Pinned(first) = chat_of(&a).scroll() else {
        panic!("PageUp pins the view");
    };
    a.update(plain(KeyCode::PageUp));
    let Scroll::Pinned(second) = chat_of(&a).scroll() else {
        panic!("still pinned");
    };
    assert!(second < first, "a second page goes further up");
    for _ in 0..30 {
        a.update(plain(KeyCode::PageUp));
    }
    assert_eq!(chat_of(&a).scroll(), Scroll::Pinned(0));
    for _ in 0..30 {
        a.update(plain(KeyCode::PageDown));
    }
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

/// An answered chat whose engine then went away, so guidance shows over the
/// history. Returns the app and the rows the transcript is really drawn in.
fn guided_history() -> (App, usize, usize) {
    let mut a = answered(&"line\n".repeat(100));
    a.update(AppEvent::EnginesDetected(detection_with(
        Status::NotRunning,
        &[],
    )));
    let guidance = guidance_of(&a).expect("the engine is not running");
    let rect = a
        .tiling()
        .panes
        .iter()
        .find(|(id, _)| *id == chat_id())
        .map(|(_, rect)| *rect)
        .unwrap();
    let width = usize::from(rect.width);
    let guidance_rows = crate::core::ai::chat::wrap::wrap_words(&guidance, width).len();
    assert!(guidance_rows >= 2, "the test needs wrapped guidance");
    let composer_rows = 1;
    let drawn = usize::from(rect.height) - composer_rows - guidance_rows;
    (a, width, drawn)
}

#[test]
fn page_up_moves_by_the_rows_drawn_when_guidance_takes_some() {
    let (mut a, width, drawn) = guided_history();
    let total = chat_of(&a).lines(width).len();
    a.update(plain(KeyCode::PageUp));
    // Follow shows the last `drawn` rows; one page up leaves one row of overlap.
    assert_eq!(
        chat_of(&a).scroll(),
        Scroll::Pinned(total - drawn - (drawn - 1))
    );
}

#[test]
fn page_down_after_page_up_comes_back_by_the_rows_drawn() {
    let (mut a, width, drawn) = guided_history();
    let total = chat_of(&a).lines(width).len();
    a.update(plain(KeyCode::PageUp));
    a.update(plain(KeyCode::PageUp));
    a.update(plain(KeyCode::PageDown));
    assert_eq!(
        chat_of(&a).scroll(),
        Scroll::Pinned(total - drawn - (drawn - 1))
    );
}

#[test]
fn a_pinned_view_keeps_its_top_line_when_a_resize_changes_the_composer_rows() {
    let mut a = answered(&(0..80).map(|i| format!("row {i}\n")).collect::<String>());
    a.update(AppEvent::Resize { cols: 50, rows: 25 });
    // A long draft: several rows in the narrow pane, fewer in the wide one.
    type_text(&mut a, &"word ".repeat(30));
    for _ in 0..3 {
        a.update(plain(KeyCode::PageUp));
    }
    let top_text = |a: &App| {
        let chat = chat_of(a);
        let width = a
            .tiling()
            .panes
            .iter()
            .find(|(id, _)| *id == chat_id())
            .map(|(_, rect)| usize::from(rect.width))
            .unwrap();
        let lines = chat.lines(width);
        let rows = a.chat_viewport(chat_id());
        lines[chat.scroll().top(lines.len(), rows)].text.clone()
    };
    let before = top_text(&a);
    assert!(matches!(chat_of(&a).scroll(), Scroll::Pinned(_)));
    for cols in [120, 50, 90, 50] {
        a.update(AppEvent::Resize { cols, rows: 25 });
        assert!(matches!(chat_of(&a).scroll(), Scroll::Pinned(_)), "{cols}");
        assert_eq!(top_text(&a), before, "at {cols} columns");
    }
}

#[test]
fn a_short_transcript_does_not_scroll() {
    let mut a = answered("short");
    a.update(plain(KeyCode::PageUp));
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

#[test]
fn sending_returns_the_view_to_the_bottom() {
    let mut a = answered(&"line\n".repeat(100));
    a.update(plain(KeyCode::PageUp));
    ask(&mut a, "again");
    assert_eq!(chat_of(&a).scroll(), Scroll::Follow);
}

// 9.8: Ctrl+R -------------------------------------------------------------------

fn reasoning_states(a: &App) -> Vec<ReasoningState> {
    chat_of(a)
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Reasoning(r) => Some(r.state()),
            _ => None,
        })
        .collect()
}

/// Ctrl+R pressed a second after the last one: a new press, not a hold.
fn toggle_later(a: &mut App) {
    let later = NOW_MS.with(|n| n.get()) + 1000;
    at(a, later, ctrl('r'));
}

/// A finished turn that reasoned: a collapsed block above the answer.
fn reasoned() -> App {
    let mut a = ready_app().with_clock(mock_now);
    let turn = match ask(&mut a, "q").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(thinking(turn, "Let me check"));
    a.update(delta(turn, "Hi"));
    a.update(finish(turn));
    a.take_dirty();
    a
}

#[test]
fn ctrl_r_expands_and_collapses_the_reasoning_without_touching_the_composer() {
    let mut a = reasoned();
    type_text(&mut a, "draft");
    assert_eq!(reasoning_states(&a), [ReasoningState::Collapsed]);
    assert_eq!(a.update(ctrl('r')), vec![]);
    assert_eq!(reasoning_states(&a), [ReasoningState::Expanded]);
    assert!(a.take_dirty());
    toggle_later(&mut a);
    assert_eq!(reasoning_states(&a), [ReasoningState::Collapsed]);
    assert_eq!(composer(&a), "draft");
}

#[test]
fn ctrl_r_does_nothing_while_reasoning_streams_or_without_reasoning() {
    let (mut a, turn) = streaming();
    a.update(thinking(turn, "Let me"));
    a.take_dirty();
    assert_eq!(a.update(ctrl('r')), vec![]);
    assert_eq!(reasoning_states(&a), [ReasoningState::Streaming]);

    let mut plain_chat = answered("no reasoning here");
    assert_eq!(plain_chat.update(ctrl('r')), vec![]);
    assert!(reasoning_states(&plain_chat).is_empty());
}

#[test]
fn the_reasoning_block_through_the_key_looks_right() {
    use ratatui::{Terminal, backend::TestBackend};
    let theme = crate::ui::theme::LazaroboxTheme::default();
    let mut a = reasoned();
    let shot = |a: &App| {
        let mut terminal = Terminal::new(TestBackend::new(40, 9)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let Some(PaneView::Chat(chat)) = a.pane_view(chat_id()) else {
                    panic!("a chat");
                };
                frame.render_widget(
                    crate::ui::components::chat::ChatView::new(chat, &theme),
                    area,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
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
    };
    insta::assert_snapshot!("reasoning_collapsed_by_default", shot(&a));
    a.update(ctrl('r'));
    insta::assert_snapshot!("reasoning_expanded_by_ctrl_r", shot(&a));
    toggle_later(&mut a);
    insta::assert_snapshot!("reasoning_collapsed_again", shot(&a));
}

// W6: a pinned view stays on the same content ---------------------------------

/// A reasoned turn whose answer has 60 numbered lines, scrolled up a page.
fn pinned_reasoned() -> App {
    let mut a = ready_app();
    let turn = match ask(&mut a, "a question long enough to wrap in a narrow pane").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(thinking(turn, &"think ".repeat(60)));
    let answer: Vec<String> = (0..60).map(|i| format!("answer line {i}")).collect();
    a.update(delta(turn, &answer.join("\n")));
    a.update(finish(turn));
    a.update(plain(KeyCode::PageUp));
    assert!(matches!(chat_of(&a).scroll(), Scroll::Pinned(_)));
    assert!(top_row(&a).starts_with("answer line"), "{}", top_row(&a));
    a
}

/// The text of the first visible transcript row.
fn top_row(a: &App) -> String {
    let size = a.focused_size();
    let chat = chat_of(a);
    let rows = usize::from(chat.transcript_rows(size.cols, size.rows));
    let lines = chat.lines(usize::from(size.cols));
    lines[chat.scroll().top(lines.len(), rows)].text.clone()
}

#[test]
fn ctrl_r_keeps_a_pinned_view_on_the_same_line() {
    let mut a = pinned_reasoned();
    let before = top_row(&a);
    a.update(ctrl('r'));
    assert_eq!(top_row(&a), before);
    toggle_later(&mut a);
    assert_eq!(top_row(&a), before);
}

#[test]
fn a_resize_keeps_a_pinned_view_on_the_same_line() {
    let mut a = pinned_reasoned();
    let before = top_row(&a);
    a.update(AppEvent::Resize { cols: 50, rows: 25 });
    assert_eq!(top_row(&a), before);
    a.update(AppEvent::Resize {
        cols: 120,
        rows: 25,
    });
    assert_eq!(top_row(&a), before);
}

// 9.3: guidance and the engine check -------------------------------------------

fn detection_with(status: Status, models: &[&str]) -> Detection {
    Detection {
        engines: vec![EngineState {
            id: "ollama",
            status,
            models: models.iter().map(|m| (*m).to_owned()).collect(),
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
    }
}

/// A chat (pane 2) in an app with `ai`, before any detection result.
fn undetected_chat(ai: AiConfig) -> App {
    let mut a = launched(80, 25).with_ai(ai);
    press(&mut a, "c");
    a.update(prompt_ready(SYSTEM, &[]));
    a
}

fn guidance_of(a: &App) -> Option<String> {
    a.chat_guidance(chat_id())
}

fn assigned(engine: &str) -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set(
        Task::Chat,
        Assignment {
            engine: Some(engine.into()),
            model: None,
        },
    );
    ai
}

#[test]
fn unassigned_shows_guidance_and_a_send_keeps_the_text() {
    let mut a = undetected_chat(AiConfig::default());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    assert!(
        guidance_of(&a)
            .unwrap()
            .contains("No engine assigned to chat")
    );
    assert_eq!(ask(&mut a, "hello"), vec![]);
    assert_eq!(composer(&a), "hello");
    assert_eq!(chat_of(&a).messages().count(), 0);
}

#[test]
fn an_engine_that_is_not_running_blocks_the_send_and_is_never_replaced() {
    let mut a = undetected_chat(ollama());
    a.update(AppEvent::EnginesDetected(detection_with(
        Status::NotRunning,
        &[],
    )));
    let text = guidance_of(&a).unwrap();
    assert!(
        text.contains("ollama") && text.contains("not running"),
        "{text}"
    );
    assert_eq!(ask(&mut a, "hello"), vec![]);
    assert_eq!(composer(&a), "hello");
    assert_eq!(a.ai(), &ollama(), "the assignment is untouched");
}

#[test]
fn a_model_that_was_not_downloaded_needs_a_download() {
    let mut a = undetected_chat(ollama());
    a.update(AppEvent::EnginesDetected(detection_with(
        Status::Available,
        &["other:7b"],
    )));
    let text = guidance_of(&a).unwrap();
    assert!(
        text.contains("llama3:8b") && text.contains("download required"),
        "{text}"
    );
    assert_eq!(ask(&mut a, "hello"), vec![]);
}

#[test]
fn opencode_is_not_supported_in_chat() {
    let mut a = undetected_chat(assigned("opencode"));
    a.update(AppEvent::EnginesDetected(detection(&[])));
    assert!(
        guidance_of(&a)
            .unwrap()
            .contains("not supported in chat yet")
    );
    assert_eq!(ask(&mut a, "hi"), vec![]);
}

#[test]
fn a_cli_without_an_adapter_is_not_supported_in_chat_yet() {
    let mut a = undetected_chat(assigned("opencode"));
    a.update(AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id: "opencode",
            status: Status::Available,
            models: vec![],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
    }));
    assert_eq!(
        guidance_of(&a).as_deref(),
        Some("opencode: not supported in chat yet")
    );
    assert_eq!(ask(&mut a, "hi"), vec![]);
    assert_eq!(composer(&a), "hi");
}

fn cli_detected(id: &'static str) -> AppEvent {
    AppEvent::EnginesDetected(Detection {
        engines: vec![EngineState {
            id,
            status: Status::Available,
            models: vec![],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
    })
}

fn working_jail() -> crate::core::ai::jail::JailSupport {
    crate::core::ai::jail::JailSupport::Available {
        backend: crate::core::ai::jail::Backend::Landlock,
        abi: 7,
    }
}

#[test]
fn claude_chats_with_or_without_a_jail() {
    for jail in [
        crate::core::ai::jail::JailSupport::Unavailable(crate::core::ai::jail::JailOff::NotProbed),
        working_jail(),
    ] {
        let mut a = undetected_chat(assigned("claude")).with_jail(jail);
        a.update(cli_detected("claude"));
        assert_eq!(guidance_of(&a), None, "{jail:?}");
        let effects = ask(&mut a, "hello");
        let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
            panic!("a send expected: {effects:?}");
        };
        assert_eq!(request.engine, "claude");
        assert_eq!(request.model, None, "no model: the CLI's own default");
    }
}

#[test]
fn a_cli_without_an_adapter_stays_unsupported_even_with_a_jail() {
    let mut a = undetected_chat(assigned("opencode")).with_jail(working_jail());
    a.update(cli_detected("opencode"));
    assert_eq!(
        guidance_of(&a).as_deref(),
        Some("opencode: not supported in chat yet")
    );
    assert_eq!(ask(&mut a, "hi"), vec![]);
}

#[test]
fn gemini_chats_only_with_a_jail() {
    let mut a = undetected_chat(assigned("gemini")).with_jail(working_jail());
    a.update(cli_detected("gemini"));
    assert_eq!(guidance_of(&a), None);
    let effects = ask(&mut a, "hello");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("a send expected: {effects:?}");
    };
    assert_eq!(request.engine, "gemini");

    let mut a = undetected_chat(assigned("gemini")).with_jail(
        crate::core::ai::jail::JailSupport::Unavailable(crate::core::ai::jail::JailOff::NotProbed),
    );
    a.update(cli_detected("gemini"));
    assert_eq!(
        guidance_of(&a).as_deref(),
        Some("gemini: not available on this system yet: the jail was not probed")
    );
    assert_eq!(ask(&mut a, "hi"), vec![]);
}

#[test]
fn checking_engines_shows_until_the_result_arrives_and_then_goes_away() {
    let mut a = undetected_chat(ollama());
    assert_eq!(guidance_of(&a).as_deref(), Some("checking engines…"));
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    assert_eq!(guidance_of(&a), None);
}

#[test]
fn a_send_waits_for_the_first_detection_and_then_goes() {
    let mut a = undetected_chat(ollama());
    assert_eq!(ask(&mut a, "hello"), vec![]);
    assert_eq!(composer(&a), "hello", "waiting keeps the text");
    let effects = a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { pane, .. }] if *pane == chat_id()),
        "{effects:?}"
    );
    assert_eq!(composer(&a), "");
}

/// The first detection result: a ready ollama with the model listed.
fn ready_detection() -> AppEvent {
    AppEvent::EnginesDetected(detection(&["llama3:8b"]))
}

#[test]
fn esc_cancels_a_parked_send() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    a.update(plain(KeyCode::Esc));
    assert_eq!(a.update(ready_detection()), vec![]);
    assert_eq!(composer(&a), "hello", "the text stays for the user");
}

#[test]
fn ctrl_c_cancels_a_parked_send() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    a.update(ctrl('c'));
    assert_eq!(a.update(ready_detection()), vec![]);
}

#[test]
fn text_typed_after_the_enter_drops_the_parked_send() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    type_text(&mut a, " world");
    assert_eq!(a.update(ready_detection()), vec![]);
    assert_eq!(
        composer(&a),
        "hello world",
        "the edit is kept, nothing sent"
    );
}

#[test]
fn a_paste_after_the_enter_drops_the_parked_send() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    a.update(AppEvent::Paste(" world".into()));
    assert_eq!(a.update(ready_detection()), vec![]);
}

#[test]
fn ctrl_u_drops_the_parked_send_even_if_the_same_text_is_typed_again() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    a.update(ctrl('u'));
    type_text(&mut a, "hello");
    assert_eq!(a.update(ready_detection()), vec![]);
    assert_eq!(composer(&a), "hello");
}

#[test]
fn a_parked_send_sends_the_text_it_was_parked_with() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    let effects = a.update(ready_detection());
    match effects.as_slice() {
        [Effect::ChatSend { request, .. }] => {
            assert!(format!("{request:?}").contains("hello"), "{request:?}");
        }
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

#[test]
fn a_parked_send_carries_the_system_prompt() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    let effects = a.update(ready_detection());
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
}

#[test]
fn a_waiting_send_that_finds_the_engine_unusable_sends_nothing() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    let effects = a.update(AppEvent::EnginesDetected(detection_with(
        Status::NotRunning,
        &[],
    )));
    assert_eq!(effects, vec![]);
    assert_eq!(composer(&a), "hello");
    assert!(guidance_of(&a).unwrap().contains("not running"));
}

#[test]
fn the_engine_is_checked_again_on_every_send() {
    let mut a = undetected_chat(ollama());
    a.update(AppEvent::EnginesDetected(detection_with(
        Status::NotRunning,
        &[],
    )));
    ask(&mut a, "hello");
    assert_eq!(composer(&a), "hello");
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    let effects = a.update(plain(KeyCode::Enter));
    assert!(
        matches!(effects.as_slice(), [Effect::ChatSend { .. }]),
        "{effects:?}"
    );
}

#[test]
fn there_is_no_guidance_while_a_stream_runs() {
    let (a, _) = streaming();
    assert_eq!(guidance_of(&a), None);
}

#[test]
fn a_shell_pane_has_no_guidance() {
    let a = launched(80, 25);
    assert_eq!(a.chat_guidance(PaneId::FIRST), None);
}

// 9.4: the engine segment -----------------------------------------------------

fn general(engine: &str, model: &str) -> AiConfig {
    let mut ai = AiConfig::default();
    ai.set(
        Task::General,
        Assignment {
            engine: Some(engine.into()),
            model: Some(model.into()),
        },
    );
    ai
}

#[test]
fn the_right_segment_shows_the_chat_engine_and_model() {
    let a = ready_app();
    assert_eq!(a.status_right(), "ollama › llama3:8b");
}

#[test]
fn a_chat_with_nothing_assigned_says_no_engine() {
    let mut a = launched(80, 25);
    press(&mut a, "c");
    assert_eq!(a.status_right(), "no engine");
}

#[test]
fn chat_falls_back_to_the_general_assignment() {
    let mut a = launched(80, 25).with_ai(general("claude", "sonnet"));
    press(&mut a, "c");
    assert_eq!(a.status_right(), "claude › sonnet");
}

#[test]
fn an_engine_without_a_model_shows_only_the_engine() {
    let mut a = launched(80, 25).with_ai(assigned("claude"));
    press(&mut a, "c");
    assert_eq!(a.status_right(), "claude");
}

#[test]
fn focus_switches_between_the_engine_and_the_shell_name() {
    let mut a = ready_app();
    assert_eq!(a.status_right(), "ollama › llama3:8b");
    press(&mut a, "h");
    assert_eq!(a.status_right(), "zsh");
    press(&mut a, "l");
    assert_eq!(a.status_right(), "ollama › llama3:8b");
}

// 9.7: the animation clock input -------------------------------------------------

#[test]
fn idle_chats_do_not_animate() {
    let a = ready_app();
    assert!(!a.animating());
    assert!(a.streams().is_empty());
}

#[test]
fn a_running_stream_animates_until_it_ends_whatever_way() {
    let (mut a, turn) = streaming();
    assert!(a.animating());
    assert_eq!(a.streams().len(), 1);
    a.update(delta(turn, "x"));
    assert!(a.animating(), "answering still animates");
    a.update(finish(turn));
    assert!(!a.animating());

    let (mut a, turn) = streaming();
    a.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Error(crate::core::ai::chat::engine::ChatError::Interrupted),
    ));
    assert!(!a.animating(), "a cut stream");

    let (mut a, _) = streaming();
    a.update(plain(KeyCode::Esc));
    assert!(!a.animating(), "a cancel");

    let (mut a, _) = streaming();
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(!a.animating(), "a closed pane");
}

#[test]
fn a_stream_in_a_background_tab_still_animates_and_closing_its_tab_stops_it() {
    let (mut a, _) = streaming();
    press(&mut a, "tn");
    assert_eq!(a.active_tab(), 1);
    assert!(a.animating());
    press(&mut a, "gB");
    press(&mut a, "tc");
    a.update(key('y'));
    assert!(!a.animating());
}

#[test]
fn one_tick_advances_the_frame_by_one_and_asks_for_one_redraw() {
    let (mut a, _) = streaming();
    a.take_dirty();
    let before = a.anim_frame();
    assert_eq!(a.update(AppEvent::AnimTick), vec![]);
    assert_eq!(a.anim_frame(), before + 1);
    assert!(a.take_dirty());
    assert!(!a.take_dirty(), "exactly one redraw request");
}

#[test]
fn a_late_tick_with_no_stream_changes_nothing() {
    let mut a = ready_app();
    let before = a.anim_frame();
    assert_eq!(a.update(AppEvent::AnimTick), vec![]);
    assert_eq!(a.anim_frame(), before);
    assert!(!a.take_dirty());
}

#[test]
fn streams_carry_the_phase_the_model_and_the_start() {
    use crate::core::ai::chat::StreamPhase;
    let (mut a, turn) = streaming();
    let first = a.streams();
    assert_eq!(first[0].pane, chat_id());
    assert_eq!(first[0].model, "llama3:8b");
    assert_eq!(first[0].phase, StreamPhase::Thinking);
    a.update(delta(turn, "x"));
    assert_eq!(a.streams()[0].phase, StreamPhase::Answering);
}

// Held one-shot keys without the Kitty protocol --------------------------------

fn release(c: char, mods: KeyModifiers) -> AppEvent {
    AppEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char(c),
        mods,
        KeyEventKind::Release,
    ))
}

#[test]
fn a_held_yank_sent_as_presses_copies_once() {
    let mut a = answered("hello").with_clock(mock_now);
    assert_eq!(at(&mut a, 0, ctrl('y')).len(), 1);
    for ms in [100, 200, 300, 600, 900] {
        assert_eq!(at(&mut a, ms, ctrl('y')), vec![], "Ctrl+Y at {ms}");
    }
    assert_eq!(
        at(&mut a, 1500, ctrl('y')).len(),
        1,
        "a new press, after the pause"
    );
}

#[test]
fn a_held_reasoning_toggle_sent_as_presses_toggles_once() {
    let mut a = ready_app().with_clock(mock_now);
    let turn = match ask(&mut a, "q").as_slice() {
        [Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("{other:?}"),
    };
    a.update(thinking(turn, "pondering"));
    a.update(delta(turn, "hello"));
    a.update(finish(turn));
    let state = |a: &App| chat_of(a).entries().to_vec();
    let before = state(&a);
    at(&mut a, 10, ctrl('r'));
    let toggled = state(&a);
    assert_ne!(toggled, before);
    for ms in [110, 210, 310] {
        at(&mut a, ms, ctrl('r'));
        assert_eq!(state(&a), toggled, "Ctrl+R at {ms}");
    }
}

#[test]
fn a_release_or_another_key_ends_the_hold() {
    let mut a = answered("hello").with_clock(mock_now);
    assert_eq!(at(&mut a, 0, ctrl('y')).len(), 1);
    at(&mut a, 50, release('y', KeyModifiers::CONTROL));
    assert_eq!(at(&mut a, 100, ctrl('y')).len(), 1, "after the release");
    at(&mut a, 150, key('x'));
    assert_eq!(at(&mut a, 200, ctrl('y')).len(), 1, "after another key");
}

#[test]
fn a_held_cancel_sent_as_presses_cancels_once() {
    let mut a = ready_app().with_clock(mock_now);
    assert_eq!(ask(&mut a, "one").len(), 1);
    let cancel = at(&mut a, 0, plain(KeyCode::Esc));
    assert_eq!(cancel, vec![Effect::ChatCancel(chat_id())]);
    for ms in [100, 200, 300] {
        assert_eq!(at(&mut a, ms, plain(KeyCode::Esc)), vec![], "Esc at {ms}");
    }
    // A later press, on a new stream, cancels again.
    assert_eq!(ask(&mut a, "two").len(), 1);
    let again = at(&mut a, 2000, plain(KeyCode::Esc));
    assert_eq!(again, vec![Effect::ChatCancel(chat_id())]);
}

// PR12: the system prompt is prepared per conversation ------------------------

fn prepare_effect(effects: &[Effect]) -> Option<&Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::PreparePrompt { .. }))
}

// Spec (prompt-profiles): Resolved once per conversation, at start.
#[test]
fn opening_a_chat_asks_for_its_prompt_with_the_profile_origin_and_flag() {
    use crate::core::ai::config::Profile;
    let mut ai = AiConfig::default();
    ai.set_profile(Profile::Analyst);
    ai.set_context(false);
    let mut a = launched(80, 25).with_ai(ai);
    a.update(cwd_event("/srv/app"));
    let effects = press(&mut a, "c");
    assert_eq!(
        prepare_effect(&effects),
        Some(&Effect::PreparePrompt {
            pane: chat_id(),
            profile: Profile::Analyst,
            origin: Some("/srv/app".into()),
            context: false,
        })
    );
}

#[test]
fn a_refused_split_asks_for_no_prompt() {
    let mut a = launched(15, 5);
    let effects = press(&mut a, "c");
    assert_eq!(prepare_effect(&effects), None, "{effects:?}");
}

// Spec: a send waits for the prompt, then goes with it.
#[test]
fn a_send_before_the_prompt_arrives_waits_and_then_carries_it() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    assert_eq!(ask(&mut a, "hello"), vec![], "parked, nothing is sent yet");
    assert_eq!(composer(&a), "hello", "the text stays in the composer");
    let effects = a.update(prompt_ready(SYSTEM, &[]));
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
    assert_eq!(composer(&a), "");
}

#[test]
fn editing_the_composer_drops_a_send_waiting_for_the_prompt() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    ask(&mut a, "hello");
    type_text(&mut a, "!");
    assert_eq!(a.update(prompt_ready(SYSTEM, &[])), vec![]);
    assert_eq!(composer(&a), "hello!");
}

// A parked send dies with its pane: nothing is kept for a pane that is gone.
#[test]
fn closing_the_pane_drops_its_parked_send() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    ask(&mut a, "hello");
    assert_eq!(a.waiting_sends.len(), 1);
    press(&mut a, "wq");
    a.update(key('y'));
    assert!(a.pane_view(chat_id()).is_none(), "the chat pane is closed");
    assert!(a.waiting_sends.is_empty(), "{:?}", a.waiting_sends);
    assert_eq!(a.update(prompt_ready(SYSTEM, &[])), vec![]);
}

#[test]
fn closing_a_pane_drops_only_its_own_parked_send() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    ask(&mut a, "hello");
    a.waiting_sends.push((PaneId::FIRST, "other".into()));
    a.update(AppEvent::Pty(PaneId::FIRST, PtyEvent::Exited));
    assert_eq!(a.waiting_sends, [(chat_id(), "hello".to_owned())]);
}

#[test]
fn a_waiting_send_of_a_missing_pane_is_dropped_when_the_waits_end() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.waiting_sends.push((chat_id(), "ghost".into()));
    assert_eq!(a.update(ready_detection()), vec![]);
    assert!(a.waiting_sends.is_empty(), "{:?}", a.waiting_sends);
}

// Both waits end together: the engine check and the prompt.
#[test]
fn a_send_waits_for_both_the_detection_and_the_prompt() {
    let mut a = undetected_chat(ollama());
    ask(&mut a, "hello");
    assert_eq!(
        a.update(prompt_ready(SYSTEM, &[])),
        vec![],
        "still detecting"
    );
    let effects = a.update(ready_detection());
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
}

// Spec (prompt-profiles): a skipped override file is a system line.
#[test]
fn the_notices_of_skipped_overrides_become_system_lines() {
    let mut a = launched(80, 25).with_ai(ollama());
    press(&mut a, "c");
    a.update(prompt_ready(
        SYSTEM,
        &["prompt override ignored: analyst/chat.md not valid UTF-8"],
    ));
    let lines: Vec<&Entry> = chat_of(&a).entries().iter().collect();
    assert_eq!(
        lines,
        [&Entry::System(
            "prompt override ignored: analyst/chat.md not valid UTF-8".into()
        )]
    );
    assert_eq!(chat_of(&a).messages().count(), 0, "never a message");
}

// Spec: Resolved once per conversation; the later turns reuse it.
#[test]
fn every_turn_of_the_conversation_carries_the_same_prompt() {
    let mut a = answered("first");
    let effects = ask(&mut a, "second");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
}

// Spec (A switch applies to new conversations only): a profile switch while
// the chat is open does not touch the prompt it already has.
#[test]
fn a_profile_change_after_the_start_keeps_the_prompt_of_the_open_chat() {
    use crate::core::ai::config::Profile;
    let mut a = ready_app();
    let mut ai = ollama();
    ai.set_profile(Profile::Analyst);
    a = a.with_ai(ai);
    let effects = ask(&mut a, "hello");
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, SYSTEM);
}

// Spec: the fallback prompt unblocks a waiting send; the real one that comes
// late is ignored (the prompt is pinned once).
#[test]
fn a_late_prompt_after_the_fallback_is_ignored() {
    let mut a = launched(80, 25).with_ai(ollama());
    a.update(AppEvent::EnginesDetected(detection(&["llama3:8b"])));
    press(&mut a, "c");
    ask(&mut a, "hello");
    let effects = a.update(prompt_ready(
        "built-in",
        &["prompt preparation timed out: workspace context unavailable"],
    ));
    let [Effect::ChatSend { request, .. }] = effects.as_slice() else {
        panic!("one ChatSend: {effects:?}");
    };
    assert_eq!(request.system, "built-in");
    assert_eq!(a.update(prompt_ready(SYSTEM, &["late"])), vec![]);
    let notices = chat_of(&a)
        .entries()
        .iter()
        .filter(|e| matches!(e, Entry::System(_)))
        .count();
    assert_eq!(notices, 1, "the late notices are not added");
}

#[test]
fn a_prompt_for_a_closed_pane_is_ignored() {
    let mut a = launched(80, 25).with_ai(ollama());
    assert_eq!(a.update(prompt_ready(SYSTEM, &["x"])), vec![]);
}

// The block is system text only: never a message of the transcript.
#[test]
fn the_prompt_is_not_part_of_the_transcript_or_the_history() {
    let (a, _) = streaming();
    assert!(
        chat_of(&a)
            .messages()
            .all(|m| !m.text.contains("workspace block")),
    );
}
