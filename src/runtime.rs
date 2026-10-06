//! The impure shell around `App`: terminal, PTY, timers and the event loop.
//!
//! Redraws are driven by a dirty flag and a 16 ms tick, so output floods are
//! capped at about 60 fps. Mouse capture is never enabled, so the host
//! terminal keeps its native selection.

use std::{
    env,
    io::{self, Write, stdout},
    panic,
    time::Duration,
};

use futures::StreamExt;
use ratatui::{
    DefaultTerminal,
    crossterm::{
        cursor::SetCursorStyle,
        event::{DisableBracketedPaste, EnableBracketedPaste, Event, EventStream},
        execute, terminal,
    },
};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use crate::{
    app::{App, AppEvent, Effect, pane_size},
    core::{
        pane::{CursorKind, CursorShape, PaneSize},
        pty::{PtyEvent, PtyHandle, PtySink, SpawnSpec, portable::spawn_portable},
    },
    ui::{self, theme::LazaroboxTheme},
};

/// Time between redraw checks.
const TICK: Duration = Duration::from_millis(16);
/// Capacity of the reader-to-loop channel; a full channel stalls the reader.
const PTY_CHANNEL: usize = 256;
/// PTY events handled per wake-up before yielding to the other branches.
const EVENT_BUDGET: usize = 256;

/// Runs the terminal pane until the child exits or the user quits.
pub async fn run() -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let size = pane_size(cols, rows);
    let cwd = env::current_dir().unwrap_or_else(|_| "/".into());
    let spec = SpawnSpec::new(env::var("SHELL").ok().as_deref(), cwd, size);

    let (tx, rx) = mpsc::channel(PTY_CHANNEL);
    let sink: PtySink = Box::new(move |event| tx.blocking_send(event).is_ok());
    let mut pty = spawn_portable(&spec, sink)?;

    let mut terminal = ratatui::init();
    install_panic_hook();
    let result = match execute!(stdout(), EnableBracketedPaste) {
        Ok(()) => event_loop(&mut terminal, pty.as_mut(), rx, size).await,
        Err(error) => Err(error),
    };

    pty.kill();
    drop(pty);
    restore();
    result
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    pty: &mut dyn PtyHandle,
    mut rx: mpsc::Receiver<PtyEvent>,
    size: PaneSize,
) -> io::Result<()> {
    let theme = LazaroboxTheme::default();
    let mut app = App::new(size);
    let mut events = EventStream::new();
    let mut tick = interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut out = stdout();
    let mut applied_cursor = CursorShape::default();

    loop {
        tokio::select! {
            event = events.next() => match event {
                Some(Ok(event)) => {
                    if let Some(event) = to_app_event(event)
                        && step(&mut app, event, pty)
                    {
                        return Ok(());
                    }
                }
                Some(Err(error)) => return Err(error),
                None => return Ok(()),
            },
            event = rx.recv() => {
                let Some(event) = event else { return Ok(()) };
                if step(&mut app, AppEvent::Pty(event), pty) {
                    return Ok(());
                }
                for _ in 1..EVENT_BUDGET {
                    let Ok(event) = rx.try_recv() else { break };
                    if step(&mut app, AppEvent::Pty(event), pty) {
                        return Ok(());
                    }
                }
            }
            _ = tick.tick() => {
                if app.dirty {
                    app.dirty = false;
                    terminal.draw(|frame| ui::render(frame, &app, &theme))?;
                }
                apply_cursor_style(&mut out, app.cursor_shape(), &mut applied_cursor)?;
            }
        }
    }
}

/// Feeds one event to the app and runs the effects it asks for.
/// Returns `true` when the app wants to quit.
fn step(app: &mut App, event: AppEvent, pty: &mut dyn PtyHandle) -> bool {
    run_effects(app.update(event), pty)
}

/// Executes effects against the PTY. Returns `true` if one of them was `Quit`.
fn run_effects(effects: Vec<Effect>, pty: &mut dyn PtyHandle) -> bool {
    let mut quit = false;
    for effect in effects {
        match effect {
            Effect::WritePty(bytes) => pty.write(bytes),
            // A failed resize leaves the child at the old size; nothing to recover.
            Effect::ResizePty(size) => {
                let _ = pty.resize(size);
            }
            Effect::Quit => quit = true,
        }
    }
    quit
}

fn to_app_event(event: Event) -> Option<AppEvent> {
    match event {
        Event::Key(key) => Some(AppEvent::Key(key)),
        Event::Paste(text) => Some(AppEvent::Paste(text)),
        Event::Resize(cols, rows) => Some(AppEvent::Resize { cols, rows }),
        _ => None,
    }
}

fn to_cursor_style(shape: CursorShape) -> SetCursorStyle {
    match (shape.kind, shape.blinking) {
        (CursorKind::Default, _) => SetCursorStyle::DefaultUserShape,
        (CursorKind::Block, true) => SetCursorStyle::BlinkingBlock,
        (CursorKind::Block, false) => SetCursorStyle::SteadyBlock,
        (CursorKind::Underline, true) => SetCursorStyle::BlinkingUnderScore,
        (CursorKind::Underline, false) => SetCursorStyle::SteadyUnderScore,
        (CursorKind::Bar, true) => SetCursorStyle::BlinkingBar,
        (CursorKind::Bar, false) => SetCursorStyle::SteadyBar,
    }
}

/// Applies `want` to the host terminal only if it differs from `last`.
fn apply_cursor_style<W: Write>(
    out: &mut W,
    want: CursorShape,
    last: &mut CursorShape,
) -> io::Result<()> {
    if want != *last {
        execute!(out, to_cursor_style(want))?;
        *last = want;
    }
    Ok(())
}

/// Undoes what the runtime changed in the host terminal, minus raw mode and
/// the alternate screen, which `ratatui::restore` handles.
fn restore_to<W: Write>(out: &mut W) -> io::Result<()> {
    execute!(out, SetCursorStyle::DefaultUserShape, DisableBracketedPaste)
}

fn restore() {
    // Best effort: there is nowhere left to report a failure.
    let _ = restore_to(&mut stdout());
    ratatui::restore();
}

/// Restores the cursor and bracketed paste before the previous hook (which
/// `ratatui::init` set up to leave raw mode and the alternate screen) runs.
fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_to(&mut stdout());
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent,
        MouseEventKind,
    };

    use super::*;
    use crate::{
        app::{AppEvent, Effect},
        core::pane::{CursorKind, CursorShape, PaneSize},
        core::pty::fake::FakePty,
    };

    fn shape(kind: CursorKind, blinking: bool) -> CursorShape {
        CursorShape { kind, blinking }
    }

    fn applied(want: CursorShape, last: &mut CursorShape) -> Vec<u8> {
        let mut out = Vec::new();
        apply_cursor_style(&mut out, want, last).unwrap();
        out
    }

    // Spec: effects are executed against the PTY.
    #[test]
    fn write_and_resize_effects_reach_the_pty() {
        let mut pty = FakePty::default();
        let log = pty.log.clone();
        let size = PaneSize {
            rows: 39,
            cols: 100,
        };
        let quit = run_effects(
            vec![Effect::WritePty(b"ls\r".to_vec()), Effect::ResizePty(size)],
            &mut pty,
        );
        assert!(!quit);
        let log = log.lock().unwrap();
        assert_eq!(log.writes, vec![b"ls\r".to_vec()]);
        assert_eq!(log.resizes, vec![size]);
    }

    #[test]
    fn quit_effect_is_reported() {
        let mut pty = FakePty::default();
        assert!(run_effects(vec![Effect::Quit], &mut pty));
    }

    #[test]
    fn effects_are_executed_in_order() {
        let mut pty = FakePty::default();
        let log = pty.log.clone();
        run_effects(
            vec![
                Effect::WritePty(vec![1]),
                Effect::WritePty(vec![2]),
                Effect::WritePty(vec![3]),
            ],
            &mut pty,
        );
        assert_eq!(log.lock().unwrap().writes, vec![vec![1], vec![2], vec![3]]);
    }

    // Spec: cursor style is applied only when it changes.
    #[test]
    fn cursor_style_is_applied_on_change() {
        let mut last = CursorShape::default();
        assert_eq!(
            applied(shape(CursorKind::Bar, false), &mut last),
            b"\x1b[6 q"
        );
        assert_eq!(last, shape(CursorKind::Bar, false));
    }

    #[test]
    fn cursor_style_is_not_reapplied_when_unchanged() {
        let mut last = CursorShape::default();
        applied(shape(CursorKind::Bar, false), &mut last);
        assert!(applied(shape(CursorKind::Bar, false), &mut last).is_empty());
    }

    #[test]
    fn default_shape_at_start_emits_nothing() {
        let mut last = CursorShape::default();
        assert!(applied(CursorShape::default(), &mut last).is_empty());
    }

    #[test]
    fn every_shape_maps_to_its_decscusr_code() {
        let cases = [
            (shape(CursorKind::Default, false), "\x1b[0 q"),
            (shape(CursorKind::Block, true), "\x1b[1 q"),
            (shape(CursorKind::Block, false), "\x1b[2 q"),
            (shape(CursorKind::Underline, true), "\x1b[3 q"),
            (shape(CursorKind::Underline, false), "\x1b[4 q"),
            (shape(CursorKind::Bar, true), "\x1b[5 q"),
            (shape(CursorKind::Bar, false), "\x1b[6 q"),
        ];
        for (want, code) in cases {
            // Start from a different shape so the change is always emitted.
            let mut last = shape(CursorKind::Block, true);
            if want == last {
                last = CursorShape::default();
            }
            assert_eq!(applied(want, &mut last), code.as_bytes(), "{want:?}");
        }
    }

    // Spec: restore emits DefaultUserShape.
    #[test]
    fn restore_resets_the_cursor_and_bracketed_paste() {
        let mut out = Vec::new();
        restore_to(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[0 q"), "{text:?}");
        assert!(text.contains("\x1b[?2004l"), "{text:?}");
    }

    fn key_event(kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn crossterm_events_map_to_app_events() {
        let key = key_event(KeyEventKind::Press);
        assert_eq!(to_app_event(Event::Key(key)), Some(AppEvent::Key(key)));
        assert_eq!(
            to_app_event(Event::Paste("hi".into())),
            Some(AppEvent::Paste("hi".into()))
        );
        assert_eq!(
            to_app_event(Event::Resize(100, 40)),
            Some(AppEvent::Resize {
                cols: 100,
                rows: 40
            })
        );
    }

    #[test]
    fn other_crossterm_events_are_dropped() {
        assert_eq!(to_app_event(Event::FocusGained), None);
        let mouse = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(to_app_event(Event::Mouse(mouse)), None);
    }
}
