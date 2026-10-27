//! The statusline and the guidance of a live chat pane, end to end through
//! `ui::render`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{
    app::AppEvent,
    core::ai::{
        Detection, EngineState, Status,
        chat::engine::{Finish, StreamEvent},
        config::{AiConfig, Assignment, Task},
    },
};

const COLS: u16 = 120;
const ROWS: u16 = 8;

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

fn ready_detection() -> Detection {
    Detection {
        engines: vec![EngineState {
            id: "ollama",
            status: Status::Available,
            models: vec!["llama3:8b".into()],
            version: None,
            model_labels: Vec::new(),
            diagnostics: Default::default(),
        }],
        ..Default::default()
    }
}

/// An app with a ready engine and a focused chat opened from `/srv/app`.
fn chat_app() -> App {
    let mut app = App::new(COLS, ROWS)
        .with_env(Some("/bin/zsh"), None, "/srv/app".into())
        .with_ai(ollama());
    app.update(AppEvent::EnginesDetected(ready_detection()));
    prefixed(&mut app, "c");
    app.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    app
}

fn chat_id() -> crate::core::layout::PaneId {
    crate::core::layout::PaneId::for_test(2)
}

/// Sends a message from the chat and returns the turn.
fn send(app: &mut App, text: &str) -> u64 {
    keys(app, text);
    let effects = app.update(AppEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    match effects.as_slice() {
        [crate::app::Effect::ChatSend { turn, .. }] => *turn,
        other => panic!("expected one ChatSend, got {other:?}"),
    }
}

fn rows(app: &App) -> Vec<String> {
    let theme = LazaroboxTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).unwrap();
    terminal.draw(|frame| render(frame, app, &theme)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..ROWS)
        .map(|y| (0..COLS).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn status(app: &App) -> String {
    rows(app).pop().unwrap()
}

#[test]
fn a_focused_chat_shows_chat_the_origin_cwd_and_the_engine() {
    let app = chat_app();
    let line = status(&app);
    assert!(
        line.contains("CHAT") && !line.contains("TERMINAL"),
        "{line}"
    );
    assert!(line.contains("/srv/app"), "{line}");
    assert!(line.contains("ollama › llama3:8b"), "{line}");
}

#[test]
fn moving_focus_to_the_shell_brings_terminal_and_the_shell_name_back() {
    let mut app = chat_app();
    prefixed(&mut app, "h");
    let line = status(&app);
    assert!(line.contains("TERMINAL") && line.contains("zsh"), "{line}");
    assert!(!line.contains("ollama"), "{line}");
}

#[test]
fn prefix_over_a_chat_reads_prefix() {
    let mut app = chat_app();
    app.update(key(' ', KeyModifiers::CONTROL));
    let line = status(&app);
    assert!(line.contains("PREFIX") && !line.contains("CHAT"), "{line}");
}

#[test]
fn the_guidance_is_drawn_in_the_chat_pane() {
    let mut app = App::new(COLS, ROWS).with_env(Some("/bin/zsh"), None, "/srv".into());
    app.update(AppEvent::EnginesDetected(Detection::default()));
    prefixed(&mut app, "c");
    let all = rows(&app).join("\n");
    assert!(all.contains("No engine assigned to chat"), "{all}");
    assert!(status(&app).contains("no engine"));
}

#[test]
fn a_ready_chat_draws_no_guidance() {
    let all = rows(&chat_app()).join("\n");
    assert!(
        !all.contains("No engine") && !all.contains("checking"),
        "{all}"
    );
}

#[test]
fn no_stream_means_no_indicator_in_any_mode() {
    let mut app = chat_app();
    for _ in 0..3 {
        let line = status(&app);
        assert!(!line.contains('▰') && !line.contains("thinking"), "{line}");
        app.update(key(' ', KeyModifiers::CONTROL));
    }
}

#[test]
fn a_running_stream_shows_the_indicator_and_the_phase_flips() {
    let mut app = chat_app();
    let turn = send(&mut app, "hi");
    let line = status(&app);
    assert!(line.contains("▰▰▰") && line.contains("thinking"), "{line}");
    assert!(line.contains("llama3:8b") && line.contains("0s"), "{line}");
    assert!(line.contains("CHAT") && line.contains("/srv/app"), "{line}");
    app.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Delta("Hello".into()),
    ));
    assert!(status(&app).contains("answering"));
    app.update(AppEvent::Chat(
        chat_id(),
        turn,
        StreamEvent::Done(Finish::Stop),
    ));
    let line = status(&app);
    assert!(!line.contains('▰') && !line.contains("answering"), "{line}");
}

#[test]
fn the_indicator_shows_in_prefix_with_a_shell_focused_and_in_the_menu() {
    let mut app = chat_app();
    send(&mut app, "hi");
    prefixed(&mut app, "h");
    assert!(status(&app).contains("thinking"), "shell focused");
    app.update(key(' ', KeyModifiers::CONTROL));
    let line = status(&app);
    assert!(
        line.contains("PREFIX") && line.contains("thinking"),
        "{line}"
    );
    app.update(key('m', KeyModifiers::NONE));
    let line = status(&app);
    assert!(line.contains("MENU") && line.contains("thinking"), "{line}");
}

#[test]
fn a_stream_in_another_tab_is_shown() {
    let mut app = chat_app();
    send(&mut app, "hi");
    prefixed(&mut app, "tn");
    let line = status(&app);
    assert!(
        line.contains("TERMINAL") && line.contains("thinking"),
        "{line}"
    );
}

#[test]
fn several_streams_show_the_focused_one_and_the_count() {
    let mut app = chat_app();
    send(&mut app, "first");
    prefixed(&mut app, "c");
    let second = app.focused();
    assert_ne!(second, chat_id());
    app.update(AppEvent::PromptReady(
        second,
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    send(&mut app, "second");
    assert!(status(&app).contains("+1"), "{}", status(&app));
    prefixed(&mut app, "wq");
    app.update(key('y', KeyModifiers::NONE));
    assert!(!status(&app).contains("+1"), "{}", status(&app));
}

#[test]
fn the_frame_moves_the_lit_segment() {
    let mut app = chat_app();
    send(&mut app, "hi");
    let first = status(&app);
    app.update(AppEvent::AnimTick);
    let second = status(&app);
    assert_ne!(first, second);
    assert!(second.contains("━▰▰▰━━━━"), "{second}");
}

// 10.7: the memory segment through `ui::render` -----------------------------------

fn memory_ai() -> AiConfig {
    let mut ai = ollama();
    ai.memory = crate::core::ai::config::MemoryConfig {
        enabled: true,
        acknowledged: true,
        ..Default::default()
    };
    ai
}

fn memory_app() -> App {
    let mut app = App::new(COLS, ROWS)
        .with_env(Some("/bin/zsh"), None, "/srv/app".into())
        .with_ai(memory_ai());
    app.update(AppEvent::EnginesDetected(ready_detection()));
    prefixed(&mut app, "c");
    app
}

#[test]
fn the_memory_segment_follows_the_state_of_the_chat() {
    use crate::core::ai::memory::{
        port::{MemReply, Reason},
        reply::ProjectError,
    };
    let mut app = memory_app();
    assert!(
        status(&app).contains("ollama › llama3:8b · mem: checking"),
        "{}",
        status(&app)
    );
    app.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Ok("lazarobox-shell".into()),
    }));
    assert!(status(&app).contains("· mem: ready"), "{}", status(&app));

    let mut app = memory_app();
    app.update(AppEvent::Memory(MemReply::Unavailable {
        pane: Some(chat_id()),
        reason: Reason::NotRunning,
    }));
    assert!(
        status(&app).contains("· mem: unavailable"),
        "{}",
        status(&app)
    );

    let mut app = memory_app();
    app.update(AppEvent::Memory(MemReply::Project {
        pane: chat_id(),
        result: Err(ProjectError::NotGit),
    }));
    assert!(
        status(&app).contains("· mem: unavailable"),
        "{}",
        status(&app)
    );
}

#[test]
fn memory_off_and_a_shell_pane_show_no_memory_text() {
    let line = status(&chat_app());
    assert!(!line.contains("mem:"), "memory off: {line}");
    let mut app = memory_app();
    prefixed(&mut app, "h");
    let line = status(&app);
    assert!(
        line.contains("zsh") && !line.contains("mem:"),
        "shell: {line}"
    );
}

// chat-memory S1 / S2: the context fill ----------------------------------------

/// A chat whose engine reports a context window of `window` tokens.
fn windowed_app(window: Option<usize>) -> App {
    let mut app = App::new(COLS, ROWS)
        .with_env(Some("/bin/zsh"), None, "/srv/app".into())
        .with_ai(ollama());
    app.update(AppEvent::EnginesDetected(Detection {
        windows: window.map(|w| ("ollama", w)).into_iter().collect(),
        ..ready_detection()
    }));
    prefixed(&mut app, "c");
    app.update(AppEvent::PromptReady(
        chat_id(),
        crate::core::ai::chat::prompts::Resolved {
            text: "system".into(),
            notices: Vec::new(),
        },
    ));
    app
}

#[test]
fn a_chat_shows_no_fill_before_its_first_send() {
    assert!(!status(&windowed_app(Some(10_000))).contains("context"));
}

#[test]
fn the_fill_is_the_share_of_the_engines_window_after_a_send() {
    let mut app = windowed_app(Some(10_000));
    send(&mut app, "hi");
    // 1 history + 2 system + 1500 tools reserve = 1503 of 10000.
    let line = status(&app);
    assert!(line.contains("ollama › llama3:8b · context 15%"), "{line}");
}

#[test]
fn without_a_window_the_fill_is_against_the_character_cap() {
    let mut app = windowed_app(None);
    send(&mut app, "hi");
    let line = status(&app);
    assert!(line.contains("context 0%"), "{line}");
}

#[test]
fn a_smaller_window_fills_faster() {
    let mut app = windowed_app(Some(3_000));
    send(&mut app, "hi");
    let line = status(&app);
    assert!(line.contains("context 50%"), "{line}");
}

#[test]
fn a_shell_pane_shows_no_fill() {
    let mut app = windowed_app(Some(10_000));
    send(&mut app, "hi");
    prefixed(&mut app, "h");
    assert!(!status(&app).contains("context"));
}

#[test]
fn snapshot_a_chat_status_with_the_context_fill() {
    let mut app = windowed_app(Some(10_000));
    send(&mut app, "hi");
    insta::assert_snapshot!("chat_status_context_fill", status(&app).trim_end());
}
