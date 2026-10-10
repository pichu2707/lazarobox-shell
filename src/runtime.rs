//! The impure shell around `App`: terminal, PTY, timers and the event loop.
//!
//! Redraws are driven by a dirty flag and a 16 ms tick, so output floods are
//! capped at about 60 fps. Mouse capture is never enabled, so the host
//! terminal keeps its native selection.

use std::{
    collections::{HashMap, VecDeque},
    env,
    io::{self, Write, stdout},
    panic,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
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
/// Time between reads of the child's working directory.
const CWD_POLL: Duration = Duration::from_secs(1);
/// Longest a quit waits for the children to die: HUP grace (300 ms) plus the
/// reap wait (2 s) plus margin.
const QUIT_KILL_BOUND: Duration = Duration::from_secs(3);
/// How often `Panes::shutdown` checks whether the kills are done.
const SHUTDOWN_POLL: Duration = Duration::from_millis(5);
/// Most events one `drive` call processes (the first one plus its feedback).
/// Feedback comes from reactions run on the UI thread; a future
/// `SpawnFailed` -> `SpawnPane` reaction on a persistently failing spawn would
/// otherwise spin there forever and freeze the UI.
const MAX_DRIVE_STEPS: usize = 64;

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
    let mut panes = Panes::new(tx, Box::new(spawn_portable));
    // The first pane is spawned before the UI starts: a failure is fatal.
    panes.spawn_pane(id, &spec)?;

    let mut terminal = ratatui::init();
    install_panic_hook();
    let result = match execute!(stdout(), EnableBracketedPaste) {
        Ok(()) => event_loop(&mut terminal, &mut panes, rx, shutdown, app).await,
        Err(error) => Err(error),
    };

    // The user gets the prompt back first; the children are killed after.
    // `rx` went away with the loop, so readers stuck on a full channel stopped.
    restore();
    panes.shutdown(QUIT_KILL_BOUND);
    result
}

/// A sink that tags each event with the pane it came from.
fn tagged_sink(id: PaneId, tx: mpsc::Sender<(PaneId, PtyEvent)>) -> PtySink {
    Box::new(move |event| tx.blocking_send((id, event)).is_ok())
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    panes: &mut Panes,
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
                        && step(&mut app, event, panes)
                    {
                        return Ok(());
                    }
                }
                Some(Err(error)) => return Err(error),
                None => return Ok(()),
            },
            event = rx.recv() => {
                let Some((id, event)) = event else { return Ok(()) };
                if step(&mut app, AppEvent::Pty(id, event), panes) {
                    return Ok(());
                }
                for _ in 1..EVENT_BUDGET {
                    let Ok((id, event)) = rx.try_recv() else { break };
                    if step(&mut app, AppEvent::Pty(id, event), panes) {
                        return Ok(());
                    }
                }
            }
            // Same exit path as Quit: the caller restores and kills the children.
            () = shutdown.requested() => return Ok(()),
            _ = cwd_poll.tick() => {
                for event in panes.poll_cwds() {
                    step(&mut app, event, panes);
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

/// Builds the child of a pane; the production one is `spawn_portable`.
type SpawnFn = Box<dyn FnMut(&SpawnSpec, PtySink) -> io::Result<Box<dyn PtyHandle>>>;

/// Whether the loop keeps running after a batch of effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Continue,
    Quit,
}

/// The live children, by pane.
struct Panes {
    handles: HashMap<PaneId, Box<dyn PtyHandle>>,
    tx: mpsc::Sender<(PaneId, PtyEvent)>,
    spawn: SpawnFn,
    reapers: Vec<JoinHandle<()>>,
}

impl Panes {
    fn new(tx: mpsc::Sender<(PaneId, PtyEvent)>, spawn: SpawnFn) -> Self {
        Self {
            handles: HashMap::new(),
            tx,
            spawn,
            reapers: Vec::new(),
        }
    }

    /// Starts the child of pane `id`, tagging everything it emits with `id`.
    fn spawn_pane(&mut self, id: PaneId, spec: &SpawnSpec) -> io::Result<()> {
        let handle = (self.spawn)(spec, tagged_sink(id, self.tx.clone()))?;
        self.handles.insert(id, handle);
        Ok(())
    }

    /// Executes `effects` in order, ignoring ids with no live child. Failed
    /// spawns come back as events for the app.
    fn apply(&mut self, effects: Vec<Effect>) -> (Outcome, Vec<AppEvent>) {
        let mut outcome = Outcome::Continue;
        let mut feedback = Vec::new();
        for effect in effects {
            match effect {
                Effect::WritePty(id, bytes) => {
                    if let Some(handle) = self.handles.get_mut(&id) {
                        handle.write(bytes);
                    }
                }
                // A failed resize leaves the child at the old size; nothing to recover.
                Effect::ResizePty(id, size) => {
                    if let Some(handle) = self.handles.get_mut(&id) {
                        let _ = handle.resize(size);
                    }
                }
                Effect::SpawnPane(id, spec) => {
                    if let Err(error) = self.spawn_pane(id, &spec) {
                        feedback.push(AppEvent::SpawnFailed(id, error.to_string()));
                    }
                }
                Effect::ClosePane(id) => self.close(id),
                Effect::Quit => outcome = Outcome::Quit,
            }
        }
        (outcome, feedback)
    }

    /// Takes the child of `id` out of the registry and kills it on a detached
    /// thread: the SIGHUP / grace / SIGKILL teardown can take seconds.
    fn close(&mut self, id: PaneId) {
        let Some(mut handle) = self.handles.remove(&id) else {
            return;
        };
        self.reapers.retain(|reaper| !reaper.is_finished());
        self.reapers.push(thread::spawn(move || {
            handle.kill();
            drop(handle);
        }));
    }

    /// The working directory of every child whose lookup succeeds.
    fn poll_cwds(&self) -> Vec<AppEvent> {
        self.handles
            .iter()
            .filter_map(|(id, handle)| poll_cwd(*id, handle.pid()))
            .collect()
    }

    /// Kills every child in parallel and waits for the reapers, giving up at
    /// `bound`. Threads still running then are left detached: the process is
    /// about to exit.
    fn shutdown(mut self, bound: Duration) {
        for (_, mut handle) in self.handles.drain() {
            self.reapers.push(thread::spawn(move || {
                handle.kill();
                drop(handle);
            }));
        }
        let deadline = Instant::now() + bound;
        while !self.reapers.iter().all(JoinHandle::is_finished) && Instant::now() < deadline {
            thread::sleep(SHUTDOWN_POLL);
        }
    }
}

/// Runs `event` and the events its effects feed back until none is left.
/// Returns `true` as soon as a quit is requested. Gives up (dropping the
/// pending feedback) after `MAX_DRIVE_STEPS` events.
fn drive(
    mut update: impl FnMut(AppEvent) -> Vec<Effect>,
    event: AppEvent,
    panes: &mut Panes,
) -> bool {
    let mut queue = VecDeque::from([event]);
    for _ in 0..MAX_DRIVE_STEPS {
        let Some(event) = queue.pop_front() else {
            break;
        };
        let (outcome, feedback) = panes.apply(update(event));
        if outcome == Outcome::Quit {
            return true;
        }
        queue.extend(feedback);
    }
    false
}

/// Feeds one event to the app and runs the effects it asks for.
/// Returns `true` when the app wants to quit.
fn step(app: &mut App, event: AppEvent, panes: &mut Panes) -> bool {
    drive(|event| app.update(event), event, panes)
}

/// The child's current working directory as an event, read from
/// `/proc/<pid>/cwd`. `None` when there is no pid or the lookup fails (not
/// Linux, process gone): the app then keeps its previous value. The link is
/// resolved by the kernel without touching the disk, so it cannot stall the loop.
fn poll_cwd(id: PaneId, pid: Option<u32>) -> Option<AppEvent> {
    let pid = pid?;
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|cwd| AppEvent::Cwd(id, cwd))
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
        assert_eq!(poll_cwd(PaneId::FIRST, None), None);
        // pid_t::MAX is never a live process.
        assert_eq!(poll_cwd(PaneId::FIRST, Some(i32::MAX as u32)), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cwd_poll_reads_the_cwd_of_a_live_process() {
        let expected = env::current_dir().unwrap();
        assert_eq!(
            poll_cwd(PaneId::FIRST, Some(std::process::id())),
            Some(AppEvent::Cwd(PaneId::FIRST, expected))
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
            if let Some(event) = poll_cwd(PaneId::FIRST, pty.pid()) {
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

    // --- Panes registry, against a fake SpawnFn ---

    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex, mpsc as std_mpsc},
        time::Instant,
    };

    use crate::core::pty::{SpawnSpec, fake::FakeLog};

    const SIZE: PaneSize = PaneSize { rows: 24, cols: 80 };

    fn id(n: u32) -> PaneId {
        PaneId::for_test(n)
    }

    fn spec() -> SpawnSpec {
        SpawnSpec::new(Some("/bin/sh"), "/".into(), SIZE)
    }

    /// What the fake `SpawnFn` saw, in spawn order.
    #[derive(Default)]
    struct Rec {
        specs: Vec<SpawnSpec>,
        logs: Vec<Arc<Mutex<FakeLog>>>,
        sinks: Vec<PtySink>,
    }

    fn fake_panes(
        pid: Option<u32>,
    ) -> (Panes, mpsc::Receiver<(PaneId, PtyEvent)>, Rc<RefCell<Rec>>) {
        let rec = Rc::new(RefCell::new(Rec::default()));
        let seen = Rc::clone(&rec);
        let spawn: SpawnFn = Box::new(move |spec, sink| {
            let pty = FakePty {
                pid,
                ..FakePty::default()
            };
            let mut rec = seen.borrow_mut();
            rec.specs.push(spec.clone());
            rec.logs.push(pty.log.clone());
            rec.sinks.push(sink);
            Ok(Box::new(pty))
        });
        let (tx, rx) = mpsc::channel(PTY_CHANNEL);
        (Panes::new(tx, spawn), rx, rec)
    }

    fn spawn_two(panes: &mut Panes) {
        panes.apply(vec![
            Effect::SpawnPane(id(1), spec()),
            Effect::SpawnPane(id(2), spec()),
        ]);
    }

    // Spec: output routed by id.
    #[test]
    fn spawned_panes_tag_their_events_with_their_id() {
        let (mut panes, mut rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        assert!((rec.borrow_mut().sinks[1])(PtyEvent::Output(
            b"two".to_vec()
        )));
        assert_eq!(
            rx.try_recv().unwrap(),
            (id(2), PtyEvent::Output(b"two".to_vec()))
        );
    }

    #[test]
    fn write_and_resize_are_routed_by_id() {
        let (mut panes, _rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        let size = PaneSize { rows: 9, cols: 7 };
        let (outcome, _) = panes.apply(vec![
            Effect::WritePty(id(2), b"ls\r".to_vec()),
            Effect::ResizePty(id(2), size),
        ]);
        assert_eq!(outcome, Outcome::Continue);
        let rec = rec.borrow();
        let second = rec.logs[1].lock().unwrap();
        assert_eq!(second.writes, vec![b"ls\r".to_vec()]);
        assert_eq!(second.resizes, vec![size]);
        let first = rec.logs[0].lock().unwrap();
        assert!(first.writes.is_empty() && first.resizes.is_empty());
    }

    // Spec: stale events dropped (effects for unknown ids are ignored).
    #[test]
    fn effects_for_unknown_ids_are_ignored() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let (outcome, feedback) = panes.apply(vec![
            Effect::WritePty(id(9), vec![1]),
            Effect::ResizePty(id(9), SIZE),
            Effect::ClosePane(id(9)),
        ]);
        assert_eq!(outcome, Outcome::Continue);
        assert!(feedback.is_empty());
    }

    #[test]
    fn quit_effect_is_reported() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        assert_eq!(panes.apply(vec![Effect::Quit]).0, Outcome::Quit);
    }

    // Spec: spawn failure is fed back, not fatal.
    #[test]
    fn a_failed_spawn_is_fed_back_as_an_event() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let spawn: SpawnFn = Box::new(|_, _| Err(io::Error::other("boom")));
        let mut panes = Panes::new(tx, spawn);
        let (outcome, feedback) = panes.apply(vec![Effect::SpawnPane(id(2), spec())]);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(feedback, vec![AppEvent::SpawnFailed(id(2), "boom".into())]);
        assert!(panes.handles.is_empty());
    }

    #[test]
    fn step_loops_feedback_until_it_is_empty() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let spawn: SpawnFn = Box::new(|_, _| Err(io::Error::other("boom")));
        let mut panes = Panes::new(tx, spawn);
        let mut seen = Vec::new();
        let quit = drive(
            |event| {
                seen.push(event.clone());
                match event {
                    AppEvent::Resize { .. } => vec![Effect::SpawnPane(id(2), spec())],
                    AppEvent::SpawnFailed(..) => vec![Effect::Quit],
                    _ => Vec::new(),
                }
            },
            AppEvent::Resize { cols: 1, rows: 1 },
            &mut panes,
        );
        assert!(quit, "the effect from the feedback event is honoured");
        assert_eq!(
            seen,
            vec![
                AppEvent::Resize { cols: 1, rows: 1 },
                AppEvent::SpawnFailed(id(2), "boom".into())
            ]
        );
    }

    /// A child whose kill waits for a gate (or `wait`), like a shell that
    /// ignores SIGHUP.
    struct SlowPty {
        log: Arc<Mutex<FakeLog>>,
        gate: std_mpsc::Receiver<()>,
        wait: Duration,
    }

    impl PtyHandle for SlowPty {
        fn write(&mut self, _bytes: Vec<u8>) {}
        fn resize(&mut self, _size: PaneSize) -> io::Result<()> {
            Ok(())
        }
        fn pid(&self) -> Option<u32> {
            None
        }
        fn kill(&mut self) {
            let _ = self.gate.recv_timeout(self.wait);
            self.log.lock().unwrap().kills += 1;
        }
    }

    /// Registers a slow child; dropping the returned sender releases its kill.
    fn add_slow(
        panes: &mut Panes,
        n: u32,
        wait: Duration,
    ) -> (Arc<Mutex<FakeLog>>, std_mpsc::Sender<()>) {
        let log = Arc::new(Mutex::new(FakeLog::default()));
        let (gate_tx, gate) = std_mpsc::channel();
        let pty = SlowPty {
            log: log.clone(),
            gate,
            wait,
        };
        panes.handles.insert(id(n), Box::new(pty));
        (log, gate_tx)
    }

    // Spec: the ClosePane effect returns immediately; the child is killed off-loop.
    #[test]
    fn close_kills_off_the_loop() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let (log, gate) = add_slow(&mut panes, 1, Duration::from_secs(30));
        let started = Instant::now();
        panes.apply(vec![Effect::ClosePane(id(1))]);
        assert!(started.elapsed() < Duration::from_secs(5), "apply blocked");
        assert!(panes.handles.is_empty());
        assert_eq!(panes.reapers.len(), 1);
        assert_eq!(log.lock().unwrap().kills, 0, "the kill is still blocked");
        drop(gate);
        for reaper in panes.reapers.drain(..) {
            reaper.join().unwrap();
        }
        assert_eq!(log.lock().unwrap().kills, 1);
    }

    #[test]
    fn finished_reapers_are_pruned() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        spawn_two(&mut panes);
        panes.close(id(1));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !panes.reapers.iter().all(JoinHandle::is_finished) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        panes.close(id(2));
        assert_eq!(panes.reapers.len(), 1, "the finished reaper was dropped");
    }

    // Spec: quit leaves no orphan; the kills run in parallel.
    #[test]
    fn shutdown_kills_every_child_in_parallel() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let held: Vec<_> = (1..=3)
            .map(|n| add_slow(&mut panes, n, Duration::from_millis(400)))
            .collect();
        let started = Instant::now();
        panes.shutdown(Duration::from_secs(5));
        let elapsed = started.elapsed();
        // Sequential kills would take at least 3 x 400 ms = 1200 ms.
        assert!(elapsed < Duration::from_millis(1000), "serial: {elapsed:?}");
        for (log, _gate) in &held {
            assert_eq!(log.lock().unwrap().kills, 1);
        }
    }

    #[test]
    fn shutdown_gives_up_at_the_bound() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let (log, gate) = add_slow(&mut panes, 1, Duration::from_secs(30));
        let started = Instant::now();
        panes.shutdown(Duration::from_millis(150));
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(150), "early: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(2), "unbounded: {elapsed:?}");
        assert_eq!(log.lock().unwrap().kills, 0);
        drop(gate);
    }

    // Spec: fallback poll, one event per live pid.
    #[cfg(target_os = "linux")]
    #[test]
    fn poll_cwds_reports_each_live_pane() {
        let (mut panes, _rx, _rec) = fake_panes(Some(std::process::id()));
        spawn_two(&mut panes);
        let mut events = panes.poll_cwds();
        events.sort_by_key(|event| match event {
            AppEvent::Cwd(id, _) => Some(*id),
            _ => None,
        });
        let cwd = env::current_dir().unwrap();
        assert_eq!(
            events,
            vec![AppEvent::Cwd(id(1), cwd.clone()), AppEvent::Cwd(id(2), cwd)]
        );
    }

    // Spec: per-pane order is preserved through the shared channel.
    #[test]
    fn events_from_one_sink_arrive_in_order_even_when_interleaved() {
        let (mut panes, mut rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        for n in 0..5u8 {
            assert!((rec.borrow_mut().sinks[0])(PtyEvent::Output(vec![n])));
            assert!((rec.borrow_mut().sinks[1])(PtyEvent::Output(vec![100 + n])));
        }
        let mut per_pane: HashMap<PaneId, Vec<u8>> = HashMap::new();
        while let Ok((pane, event)) = rx.try_recv() {
            let PtyEvent::Output(bytes) = event else {
                panic!("unexpected {event:?}");
            };
            per_pane.entry(pane).or_default().extend(bytes);
        }
        assert_eq!(per_pane[&id(1)], vec![0, 1, 2, 3, 4]);
        assert_eq!(per_pane[&id(2)], vec![100, 101, 102, 103, 104]);
    }

    // Spec: stale events dropped (late output / exit of a closed pane).
    #[test]
    fn late_events_from_a_closed_pane_have_no_effect() {
        let (mut panes, mut rx, rec) = fake_panes(None);
        let mut app = App::new(80, 24);
        spawn_two(&mut panes);
        panes.apply(vec![Effect::ClosePane(id(2))]);
        app.take_dirty();
        assert!((rec.borrow_mut().sinks[1])(PtyEvent::Output(
            b"late".to_vec()
        )));
        assert!((rec.borrow_mut().sinks[1])(PtyEvent::Exited));
        while let Ok((pane, event)) = rx.try_recv() {
            assert!(!step(&mut app, AppEvent::Pty(pane, event), &mut panes));
        }
        assert!(!app.take_dirty(), "the app state is untouched");
    }

    #[test]
    fn drive_stops_at_the_step_cap() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let spawn: SpawnFn = Box::new(|_, _| Err(io::Error::other("boom")));
        let mut panes = Panes::new(tx, spawn);
        let mut calls = 0;
        let quit = drive(
            |_| {
                calls += 1;
                if calls >= 1000 {
                    return vec![Effect::Quit];
                }
                vec![Effect::SpawnPane(id(2), spec())]
            },
            AppEvent::Resize { cols: 1, rows: 1 },
            &mut panes,
        );
        assert!(!quit);
        assert_eq!(calls, MAX_DRIVE_STEPS);
    }

    // Spec: a pane without a pid, or whose lookup fails, is skipped.
    #[cfg(target_os = "linux")]
    #[test]
    fn poll_cwds_skips_panes_without_a_usable_pid() {
        for pid in [None, Some(i32::MAX as u32)] {
            let (mut panes, _rx, _rec) = fake_panes(pid);
            panes.apply(vec![Effect::SpawnPane(id(1), spec())]);
            // The second pane reports normally.
            let live: SpawnFn = Box::new(|_, _| {
                Ok(Box::new(FakePty {
                    pid: Some(std::process::id()),
                    ..FakePty::default()
                }))
            });
            panes.spawn = live;
            panes.apply(vec![Effect::SpawnPane(id(2), spec())]);
            assert_eq!(
                panes.poll_cwds(),
                vec![AppEvent::Cwd(id(2), env::current_dir().unwrap())]
            );
        }
    }
}
