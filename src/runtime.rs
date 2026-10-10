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
    signal::unix::{Signal, SignalKind, signal},
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use crate::{
    app::{App, AppEvent, Effect},
    core::{
        layout::PaneId,
        pane::{CursorKind, CursorShape},
        pty::{PtyEvent, PtyHandle, PtySink, portable::spawn_portable},
    },
    ui::{self, theme::LazaroboxTheme},
};

/// Time between redraw checks.
const TICK: Duration = Duration::from_millis(16);
/// Capacity of the reader-to-loop channel; a full channel stalls the reader.
const PTY_CHANNEL: usize = 256;
/// PTY events handled per wake-up before yielding to the other branches.
const EVENT_BUDGET: usize = 256;
/// Time between reads of the child's working directory.
const CWD_POLL: Duration = Duration::from_secs(1);

/// SIGTERM and SIGHUP (the host terminal closing). Without handling them the
/// process would die with the host terminal still in raw mode.
struct Shutdown {
    term: Signal,
    hup: Signal,
}

impl Shutdown {
    fn new() -> io::Result<Self> {
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            hup: signal(SignalKind::hangup())?,
        })
    }

    /// Resolves once either signal arrives.
    async fn requested(&mut self) {
        tokio::select! {
            _ = self.term.recv() => {}
            _ = self.hup.recv() => {}
        }
    }
}

/// Runs the terminal pane until the child exits or the user quits.
pub async fn run() -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let cwd = env::current_dir().unwrap_or_else(|_| "/".into());
    let shell = env::var("SHELL").ok();
    let app =
        App::new(cols, rows).with_env(shell.as_deref(), env::var_os("HOME").map(Into::into), cwd);
    let (id, spec) = app.initial_spawn();

    let shutdown = Shutdown::new()?;
    let (tx, rx) = mpsc::channel(PTY_CHANNEL);
    let sink = tagged_sink(id, tx);
    let mut pty = spawn_portable(&spec, sink)?;

    let mut terminal = ratatui::init();
    install_panic_hook();
    let result = match execute!(stdout(), EnableBracketedPaste) {
        Ok(()) => event_loop(&mut terminal, pty.as_mut(), rx, shutdown, app).await,
        Err(error) => Err(error),
    };

    pty.kill();
    drop(pty);
    restore();
    result
}

/// A sink that tags each event with the pane it came from.
fn tagged_sink(id: PaneId, tx: mpsc::Sender<(PaneId, PtyEvent)>) -> PtySink {
    Box::new(move |event| tx.blocking_send((id, event)).is_ok())
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    pty: &mut dyn PtyHandle,
    mut rx: mpsc::Receiver<(PaneId, PtyEvent)>,
    mut shutdown: Shutdown,
    mut app: App,
) -> io::Result<()> {
    let theme = LazaroboxTheme::default();
    let mut events = EventStream::new();
    let mut tick = interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut cwd_poll = interval(CWD_POLL);
    cwd_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
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
                let Some((id, event)) = event else { return Ok(()) };
                if step(&mut app, AppEvent::Pty(id, event), pty) {
                    return Ok(());
                }
                for _ in 1..EVENT_BUDGET {
                    let Ok((id, event)) = rx.try_recv() else { break };
                    if step(&mut app, AppEvent::Pty(id, event), pty) {
                        return Ok(());
                    }
                }
            }
            // Same exit path as Quit: the caller kills the child and restores.
            () = shutdown.requested() => return Ok(()),
            _ = cwd_poll.tick() => {
                if let Some(event) = poll_cwd(pty.pid()) {
                    step(&mut app, event, pty);
                }
            }
            _ = tick.tick() => {
                if app.take_dirty() {
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

/// Executes effects against the PTY (the only pane there is for now, so the
/// pane id they carry is not routed on). Returns `true` if one of them was `Quit`.
fn run_effects(effects: Vec<Effect>, pty: &mut dyn PtyHandle) -> bool {
    let mut quit = false;
    for effect in effects {
        match effect {
            Effect::WritePty(_, bytes) => pty.write(bytes),
            // A failed resize leaves the child at the old size; nothing to recover.
            Effect::ResizePty(_, size) => {
                let _ = pty.resize(size);
            }
            Effect::Quit => quit = true,
        }
    }
    quit
}

/// The child's current working directory as an event, read from
/// `/proc/<pid>/cwd`. `None` when there is no pid or the lookup fails (not
/// Linux, process gone): the app then keeps its previous value. The link is
/// resolved by the kernel without touching the disk, so it cannot stall the loop.
fn poll_cwd(pid: Option<u32>) -> Option<AppEvent> {
    let pid = pid?;
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(AppEvent::Cwd)
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

    // Signals are process-wide, so both are raised from one test.
    #[tokio::test]
    async fn sigterm_and_sighup_request_shutdown() {
        use std::time::Duration;

        let mut shutdown = Shutdown::new().unwrap();
        for signum in [libc::SIGTERM, libc::SIGHUP] {
            // SAFETY: raising a signal that now has a handler installed.
            unsafe { libc::raise(signum) };
            tokio::time::timeout(Duration::from_secs(2), shutdown.requested())
                .await
                .unwrap_or_else(|_| panic!("signal {signum} did not request shutdown"));
        }
    }

    // Spec: a failed cwd lookup keeps the previous value (no event is sent).
    #[test]
    fn cwd_poll_yields_nothing_without_a_pid_or_when_the_lookup_fails() {
        assert_eq!(poll_cwd(None), None);
        // pid_t::MAX is never a live process.
        assert_eq!(poll_cwd(Some(i32::MAX as u32)), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cwd_poll_reads_the_cwd_of_a_live_process() {
        let expected = env::current_dir().unwrap();
        assert_eq!(
            poll_cwd(Some(std::process::id())),
            Some(AppEvent::Cwd(expected))
        );
    }

    // Spec: `cd /tmp` in the shell is reflected by the next poll [unix PTY].
    #[cfg(target_os = "linux")]
    #[test]
    fn cd_in_a_real_shell_is_seen_by_the_poll() {
        use std::time::Instant;

        use crate::core::pty::{PtySink, SpawnSpec, portable::spawn_portable};

        let (tx, _rx) = std::sync::mpsc::channel();
        let sink: PtySink = Box::new(move |event| tx.send(event).is_ok());
        let size = PaneSize { rows: 24, cols: 80 };
        let spec = SpawnSpec::new(Some("/bin/sh"), "/".into(), size);
        let mut pty = spawn_portable(&spec, sink).expect("spawn");

        let mut app = App::new(80, 24);
        let tmp = std::fs::canonicalize("/tmp").unwrap();
        pty.write(b"cd /tmp\n".to_vec());
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.cwd_label() != tmp.display().to_string() && Instant::now() < deadline {
            if let Some(event) = poll_cwd(pty.pid()) {
                app.update(event);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(app.cwd_label(), tmp.display().to_string());
    }

    // Spec: the reader's events reach the loop tagged with their pane.
    #[test]
    fn the_sink_tags_events_with_the_pane_id() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut sink = tagged_sink(PaneId::FIRST, tx);
        assert!(sink(PtyEvent::Output(b"hi".to_vec())));
        assert!(sink(PtyEvent::Exited));
        assert_eq!(
            rx.try_recv().unwrap(),
            (PaneId::FIRST, PtyEvent::Output(b"hi".to_vec()))
        );
        assert_eq!(rx.try_recv().unwrap(), (PaneId::FIRST, PtyEvent::Exited));
        drop(rx);
        assert!(!sink(PtyEvent::Exited), "a closed channel stops the reader");
    }

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
            vec![
                Effect::WritePty(PaneId::FIRST, b"ls\r".to_vec()),
                Effect::ResizePty(PaneId::FIRST, size),
            ],
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
                Effect::WritePty(PaneId::FIRST, vec![1]),
                Effect::WritePty(PaneId::FIRST, vec![2]),
                Effect::WritePty(PaneId::FIRST, vec![3]),
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
