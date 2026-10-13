//! The impure shell around `App`: terminal, PTY, timers and the event loop.
//!
//! Redraws are driven by a dirty flag and a 16 ms tick, so output floods are
//! capped at about 60 fps. Mouse capture is never enabled, so the host
//! terminal keeps its native selection.

use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    io::{self, Write, stdout},
    panic,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub mod ai_detect;
pub mod anim;
pub mod chat;
pub mod chat_store;
mod config_write;
pub mod engram;
pub mod loopback;
pub mod proc;

use ai_detect::spawn_detection;
use chat::Chats;
use chat_store::StoreHandle;
use config_write::save_config;
use engram::{Loopback, Memory};
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
        ai::{Detection, chat::store::StoreReply, config::AiConfig, memory::port::MemReply},
        config::{Config, config_path},
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
/// How long quitting waits for pending memory work (session ends).
const MEMORY_QUIT_BOUND: Duration = Duration::from_secs(1);
/// Capacity of the channel from the memory worker to the event loop.
const MEMORY_CHANNEL: usize = 64;
/// Longest a quit waits for the store thread to write what is queued.
const STORE_JOIN_BOUND: Duration = Duration::from_secs(2);
/// Capacity of the channel from the store thread to the event loop.
const STORE_CHANNEL: usize = 64;
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

/// What runs before the UI starts.
fn startup_effects(app: &mut App) -> Vec<Effect> {
    // Startup trigger: the first result arrives on `ai_rx` once the loop runs.
    app.request_detection()
}

/// Runs the terminal pane until the child exits or the user quits.
pub async fn run() -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let cwd = env::current_dir().unwrap_or_else(|_| "/".into());
    let shell = env::var("SHELL").ok();
    // Resolved once: the startup read and the menu's save use the same file.
    let config_path = config_path(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"));
    // Read once, here: the config parser itself never touches the environment.
    let engram_port = env::var("ENGRAM_PORT").ok();
    let mut app = app_from_config(cols, rows, config_path.as_deref(), engram_port.as_deref())
        .with_hostname(local_hostname().as_deref())
        .with_dir_check(|dir| dir.is_dir())
        .with_env(shell.as_deref(), env::var_os("HOME").map(Into::into), cwd);
    let (id, spec) = app.initial_spawn();

    let shutdown = Shutdown::new()?;
    let (tx, rx) = mpsc::channel(PTY_CHANNEL);
    let (ai_tx, ai_rx) = mpsc::channel(1);
    let (chat_tx, chat_rx) = mpsc::channel(chat::APP_CHANNEL);
    let (memory_tx, memory_rx) = mpsc::channel(MEMORY_CHANNEL);
    // Off unless the config activates it: then the sender is dropped, no
    // worker ever starts, nothing touches the network and the token is never
    // read from the environment.
    let memory = memory_for(
        &app.ai().memory,
        || env::var("ENGRAM_HTTP_TOKEN").ok(),
        engram::serve::Server::from_env,
        memory_tx,
    );
    let (store_tx, store_rx) = mpsc::channel(STORE_CHANNEL);
    let store_path = chat_store::resolve_path(
        env::var_os("XDG_DATA_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    );
    let mut panes = Panes::new(tx, Box::new(spawn_portable))
        .with_memory(memory)
        .with_config_path(config_path)
        .with_detector(Box::new(move || spawn_detection(ai_tx.clone())))
        .with_chats(Chats::new(chat::adapter_runner(), chat_tx))
        .with_store(chat_store::spawn_at(store_path, store_tx));
    let inbox = Inbox {
        pty: rx,
        ai: ai_rx,
        chat: chat_rx,
        memory: memory_rx,
        store: store_rx,
    };
    // The first pane is spawned before the UI starts: a failure is fatal.
    panes.spawn_pane(id, &spec)?;
    panes.apply(startup_effects(&mut app));

    let mut terminal = ratatui::init();
    install_panic_hook();
    let result = match execute!(stdout(), EnableBracketedPaste) {
        Ok(()) => event_loop(&mut terminal, &mut panes, inbox, shutdown, app).await,
        Err(error) => Err(error),
    };

    // The user gets the prompt back first; the children are killed after.
    // `rx` went away with the loop, so readers stuck on a full channel stopped.
    restore();
    // Quit order, and why:
    // 1. The loop already returned, so the final Cancel and Append effects of
    //    every open turn were applied (they must be emitted before the store
    //    is shut down, or the last messages are lost).
    // 2. The store thread drains what is queued, inside STORE_JOIN_BOUND.
    // 3. Memory flush and chat shutdown run together (network vs local
    //    processes), each inside its own bound.
    // 4. The PTY children are killed with whatever is left of the one bound.
    panes.shutdown_store(STORE_JOIN_BOUND);
    let started = Instant::now();
    panes
        .shutdown_engines(MEMORY_QUIT_BOUND, QUIT_KILL_BOUND)
        .await;
    panes.shutdown(QUIT_KILL_BOUND.saturating_sub(started.elapsed()));
    result
}

/// The memory for `config`. `read_token` is called only when the worker
/// starts, so a run without active memory never reads `ENGRAM_HTTP_TOKEN`; the
/// transport itself never touches the environment. `launcher` is built under
/// the same condition; production passes the real one (the PATH and the state
/// directory), tests a fake so no real `engram` is ever looked up.
fn memory_for<L: engram::serve::Launcher>(
    config: &crate::core::ai::config::MemoryConfig,
    read_token: impl FnOnce() -> Option<String> + 'static,
    launcher: impl FnOnce() -> L + 'static,
    replies: mpsc::Sender<crate::core::ai::memory::port::MemReply>,
) -> Memory {
    Memory::for_config(config, move |port| {
        engram::spawn_with(Loopback::new(port, read_token()), launcher(), port, replies)
    })
}

/// This machine's hostname, for OSC 7 host checks. `None` when the lookup fails.
fn local_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is a valid writable buffer of the given length.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    // The name may be unterminated when truncated: stop at the buffer end.
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..len]).into_owned();
    (!name.is_empty()).then_some(name)
}

/// The app as the saved config configures it: layout, AI choice and notice.
fn app_from_config(
    cols: u16,
    rows: u16,
    config_path: Option<&Path>,
    port_env: Option<&str>,
) -> App {
    let (config, ai, notice) = load_config(config_path, port_env);
    App::new(cols, rows)
        .with_config(&config)
        .with_ai(ai)
        .with_notice(notice)
}

/// Reads the config once at startup; a missing file is silent defaults.
fn load_config(path: Option<&Path>, port_env: Option<&str>) -> (Config, AiConfig, Option<String>) {
    match path {
        Some(path) => Config::load_with_ai(fs::read_to_string(path), port_env),
        None => (Config::default(), AiConfig::default(), None),
    }
}

/// A sink that tags each event with the pane it came from.
fn tagged_sink(id: PaneId, tx: mpsc::Sender<(PaneId, PtyEvent)>) -> PtySink {
    Box::new(move |event| tx.blocking_send((id, event)).is_ok())
}

/// Everything the loop waits on besides the keyboard and the timers.
struct Inbox {
    pty: mpsc::Receiver<(PaneId, PtyEvent)>,
    ai: mpsc::Receiver<Detection>,
    chat: mpsc::Receiver<chat::Tagged>,
    memory: mpsc::Receiver<MemReply>,
    store: mpsc::Receiver<StoreReply>,
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    panes: &mut Panes,
    inbox: Inbox,
    shutdown: Shutdown,
    app: App,
) -> io::Result<()> {
    let mut out = stdout();
    let result = run_loop(terminal, panes, inbox, shutdown, app, &mut out).await;
    flush_on_exit(&mut out, &mut panes.host_out, result)
}

/// Flushes the pending host bytes whatever way the loop ended (Quit, shutdown,
/// closed channel, error), so a yank right before quitting is not lost. The
/// loop's own error wins over a flush error.
fn flush_on_exit<W: Write>(
    out: &mut W,
    pending: &mut Vec<u8>,
    result: io::Result<()>,
) -> io::Result<()> {
    let flushed = flush_host(out, pending);
    result.and(flushed)
}

async fn run_loop(
    terminal: &mut DefaultTerminal,
    panes: &mut Panes,
    inbox: Inbox,
    mut shutdown: Shutdown,
    mut app: App,
    out: &mut impl Write,
) -> io::Result<()> {
    let Inbox {
        pty: mut rx,
        ai: mut ai_rx,
        chat: mut chat_rx,
        memory: mut memory_rx,
        store: mut store_rx,
    } = inbox;
    let theme = LazaroboxTheme::default();
    let mut events = EventStream::new();
    let mut tick = interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut cwd_poll = interval(CWD_POLL);
    cwd_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut applied_cursor = CursorShape::default();
    // Armed only while a chat stream runs: an idle app never wakes for it.
    let mut anim = anim::Animation::new();

    loop {
        anim.sync(app.animating());
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
            // The detector keeps a sender alive, so this never reads a closed channel.
            Some(detection) = ai_rx.recv() => {
                if step(&mut app, AppEvent::EnginesDetected(detection), panes) {
                    return Ok(());
                }
            }
            // The turn tasks keep no sender once `panes` is gone, but `panes`
            // outlives the loop, so this never reads a closed channel.
            Some((id, turn, event)) = chat_rx.recv() => {
                if step(&mut app, AppEvent::Chat(id, turn, event), panes) {
                    return Ok(());
                }
                for _ in 1..EVENT_BUDGET {
                    let Ok((id, turn, event)) = chat_rx.try_recv() else { break };
                    if step(&mut app, AppEvent::Chat(id, turn, event), panes) {
                        return Ok(());
                    }
                }
            }
            // The worker keeps a sender while it runs; with memory off the
            // channel is closed and this branch never matches.
            Some(reply) = memory_rx.recv() => {
                if step(&mut app, AppEvent::Memory(reply), panes) {
                    return Ok(());
                }
            }
            // The store thread's sender lives in `panes.store`, so this arm
            // never reads a closed channel before shutdown. At quit the loop
            // has returned; dropping `store_rx` then makes the thread's
            // `blocking_send` fail fast instead of blocking on a full channel.
            Some(reply) = store_rx.recv() => {
                if step(&mut app, AppEvent::Store(reply), panes) {
                    return Ok(());
                }
            }
            // Same exit path as Quit: the caller restores and kills the children.
            () = shutdown.requested() => return Ok(()),
            _ = cwd_poll.tick() => {
                for event in panes.poll_cwds() {
                    step(&mut app, event, panes);
                }
            }
            () = anim.tick() => {
                if step(&mut app, AppEvent::AnimTick, panes) {
                    return Ok(());
                }
            }
            _ = tick.tick() => {
                if app.take_dirty() {
                    terminal.draw(|frame| ui::render(frame, &app, &theme))?;
                }
                apply_cursor_style(out, app.cursor_shape(), &mut applied_cursor)?;
                flush_host(out, &mut panes.host_out)?;
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
    /// Where `SaveConfig` writes; `None` when no config path could be resolved.
    config_path: Option<PathBuf>,
    /// Bytes for the host terminal (OSC 52), written by `flush_host` after
    /// the draw so nothing lands in the middle of a frame.
    host_out: Vec<u8>,
    /// Starts one AI engine detection; the answer arrives on the AI channel.
    /// A no-op unless `with_detector` installs the production one.
    detect: Box<dyn FnMut()>,
    /// The chat turns. `None` ignores the chat effects (unit tests).
    chats: Option<Chats>,
    /// The engram memory worker; off unless the config activates it.
    memory: Memory,
    /// The chat history thread. `None` ignores the store effects (unit tests).
    store: Option<StoreHandle>,
}

impl Panes {
    fn new(tx: mpsc::Sender<(PaneId, PtyEvent)>, spawn: SpawnFn) -> Self {
        Self {
            handles: HashMap::new(),
            tx,
            spawn,
            reapers: Vec::new(),
            config_path: None,
            host_out: Vec::new(),
            detect: Box::new(|| {}),
            chats: None,
            memory: Memory::off(),
            store: None,
        }
    }

    /// Sets the memory the `Memory` effects go to.
    fn with_memory(mut self, memory: Memory) -> Self {
        self.memory = memory;
        self
    }

    /// Flushes the memory and stops the chat turns at the same time: the flush
    /// waits on the network, the chat shutdown on local processes, and neither
    /// needs the other. Each keeps its own bound.
    async fn shutdown_engines(&mut self, memory_bound: Duration, chats_bound: Duration) {
        let chats = async {
            if let Some(chats) = self.chats.as_mut() {
                chats.shutdown(chats_bound).await;
            }
        };
        tokio::join!(self.memory.shutdown(memory_bound), chats);
    }

    /// Sets the task that runs chat turns.
    fn with_chats(mut self, chats: Chats) -> Self {
        self.chats = Some(chats);
        self
    }

    /// Sets the thread that saves chat history.
    fn with_store(mut self, store: StoreHandle) -> Self {
        self.store = Some(store);
        self
    }

    /// Lets the store thread write what is queued, up to `bound`.
    // Blocking the runtime thread for up to 2 s is acceptable here: it only
    // happens on the quit path, where nothing else is left to serve.
    fn shutdown_store(&mut self, bound: Duration) {
        if let Some(store) = self.store.take() {
            store.shutdown(bound);
        }
    }

    /// Sets what `Effect::DetectEngines` runs.
    fn with_detector(mut self, detect: Box<dyn FnMut()>) -> Self {
        self.detect = detect;
        self
    }

    /// Sets the file `SaveConfig` writes to.
    fn with_config_path(mut self, config_path: Option<PathBuf>) -> Self {
        self.config_path = config_path;
        self
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
                // Synchronous: the file is tiny, and `drive` hands the answer
                // to the app before the next input is polled. The app relies
                // on it: save results carry no session or draft id. An async
                // save would need a generation/pending guard in `MenuState`.
                Effect::SaveConfig(edits) => {
                    feedback.push(match save_config(self.config_path.as_deref(), &edits) {
                        Ok(()) => AppEvent::ConfigSaved,
                        Err(message) => AppEvent::ConfigSaveFailed(message),
                    });
                }
                Effect::SetClipboard(bytes) => self.host_out.extend_from_slice(&bytes),
                // Returns at once: the probes run on their own tasks.
                Effect::DetectEngines => (self.detect)(),
                // Both return at once: the turn runs on its own task, and a
                // cancel only aborts it (the adapter's drop does the killing).
                Effect::ChatSend {
                    pane,
                    turn,
                    request,
                } => {
                    if let Some(chats) = self.chats.as_mut() {
                        chats.send(pane, turn, request);
                    }
                }
                Effect::ChatCancel(pane) => {
                    if let Some(chats) = self.chats.as_mut() {
                        chats.cancel(pane);
                    }
                }
                // Returns at once: the worker owns the queue and the timeouts.
                Effect::Memory(cmd) => self.memory.send(cmd),
                // Returns at once: the thread owns the database, and a slow
                // write only delays its own queue.
                Effect::Store(cmd) => {
                    if let Some(store) = self.store.as_ref() {
                        store.send(cmd);
                    }
                }
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

/// Writes the pending host bytes, flushes, and empties the buffer. An empty
/// buffer writes nothing.
fn flush_host<W: Write>(out: &mut W, pending: &mut Vec<u8>) -> io::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let result = out.write_all(pending).and_then(|()| out.flush());
    pending.clear();
    result
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

/// Whether a panic on this thread should tear the terminal down. A panic
/// inside a chat adapter poll is caught by its turn task and becomes an
/// `Interrupted` turn: the app keeps running, so the terminal must stay intact.
fn should_restore() -> bool {
    // The store thread catches its own panics and reports them as replies.
    let on_store_thread = thread::current().name() == Some(chat_store::worker::THREAD_NAME);
    !chat::in_adapter_poll() && !on_store_thread
}

/// Restores the cursor and bracketed paste before the previous hook (which
/// `ratatui::init` set up to leave raw mode and the alternate screen) runs.
/// Panics inside a chat adapter are caught elsewhere and skip all of it,
/// including the previous hook's output, which would scribble over the TUI.
fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !should_restore() {
            return;
        }
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
    use crate::core::ai::chat::engine::Finish;
    use crate::{
        app::{AppEvent, Effect},
        core::pane::{CursorKind, CursorShape, PaneSize},
        core::pty::fake::FakePty,
    };

    #[test]
    fn startup_requests_exactly_one_engine_detection() {
        let mut app = App::new(80, 24);
        assert_eq!(startup_effects(&mut app), vec![Effect::DetectEngines]);
        // Single flight: a second request while one is pending is empty.
        assert_eq!(app.request_detection(), Vec::new());
    }

    const SAVED_AI: &str = "[ai]\nengine = \"ollama\"\nmodel = \"llama3:8b\"\n";

    // Spec: the saved AI choice reaches the app at startup.
    #[test]
    fn startup_carries_the_saved_ai_choice_into_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, SAVED_AI).unwrap();
        let app = app_from_config(80, 25, Some(&path), None);
        assert_eq!(
            app.ai(),
            &AiConfig::general(Some("ollama"), Some("llama3:8b"))
        );
    }

    #[test]
    fn the_first_menu_open_shows_the_saved_assignment_unmarked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, SAVED_AI).unwrap();
        let mut app = app_from_config(80, 25, Some(&path), None);
        let mut panes = panes_saving_to(None);
        for event in [
            menu_key(' ', KeyModifiers::CONTROL),
            menu_key('m', KeyModifiers::NONE),
        ] {
            step(&mut app, event, &mut panes);
        }
        let rows = app.menu().unwrap().rows(&Config::default());
        let general = match &rows[6] {
            crate::core::menu::Row::Item { label, value, .. } => {
                assert_eq!(*label, "general");
                value.as_deref().unwrap().to_owned()
            }
            other => panic!("not an item: {other:?}"),
        };
        assert_eq!(general, "ollama › llama3:8b", "nothing is marked yet");
    }

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

    // Spec: OSC 7 accepts this machine's hostname, so the lookup must work.
    #[test]
    fn local_hostname_matches_the_kernel_hostname() {
        let expected = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .expect("linux hostname")
            .trim()
            .to_owned();
        assert_eq!(local_hostname(), Some(expected));
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

    /// Records writes and the number of flushes.
    #[derive(Default)]
    struct SpyOut {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
    }

    impl Write for SpyOut {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            self.writes += 1;
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    // copy-selection: Yank emits OSC 52 (runtime part).
    #[test]
    fn set_clipboard_is_buffered_and_touches_nothing_else() {
        let (mut panes, _rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        let (outcome, feedback) = panes.apply(vec![
            Effect::SetClipboard(b"\x1b]52;c;aGk=\x07".to_vec()),
            Effect::SetClipboard(b"more".to_vec()),
        ]);
        assert_eq!(outcome, Outcome::Continue);
        assert!(feedback.is_empty());
        assert_eq!(panes.host_out, b"\x1b]52;c;aGk=\x07more".to_vec());
        let rec = rec.borrow();
        for log in &rec.logs {
            let log = log.lock().unwrap();
            assert!(log.writes.is_empty() && log.resizes.is_empty());
        }
    }

    #[test]
    fn flush_host_writes_the_exact_bytes_flushes_and_clears() {
        let mut out = SpyOut::default();
        let mut pending = b"\x1b]52;c;aGk=\x07".to_vec();
        flush_host(&mut out, &mut pending).unwrap();
        assert_eq!(out.bytes, b"\x1b]52;c;aGk=\x07".to_vec());
        assert_eq!(out.flushes, 1);
        assert_eq!(pending, Vec::<u8>::new());
    }

    #[test]
    fn flush_on_exit_writes_pending_bytes_on_every_outcome() {
        for result in [Ok(()), Err(io::Error::other("boom"))] {
            let failed = result.is_err();
            let mut out = Vec::new();
            let mut pending = b"\x1b]52;c;aGk=\x07".to_vec();
            let got = flush_on_exit(&mut out, &mut pending, result);
            assert_eq!(out, b"\x1b]52;c;aGk=\x07".to_vec());
            assert!(pending.is_empty());
            assert_eq!(got.is_err(), failed);
        }
    }

    #[test]
    fn flush_host_with_nothing_pending_writes_nothing() {
        let mut out = SpyOut::default();
        let mut pending = Vec::new();
        flush_host(&mut out, &mut pending).unwrap();
        assert_eq!((out.bytes, out.writes, out.flushes), (vec![], 0, 0));
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

    fn statusline_top() -> Effect {
        Effect::SaveConfig(vec![crate::core::config::ConfigEdit {
            table: "statusline",
            key: "position",
            value: "top".into(),
        }])
    }

    fn panes_saving_to(path: Option<PathBuf>) -> Panes {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        Panes::new(tx, Box::new(|_, _| Err(io::Error::other("no spawn")))).with_config_path(path)
    }

    // Spec: Failure feedback, exactly one result.
    #[test]
    fn save_config_writes_and_answers_one_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lazarobox/config.toml");
        let mut panes = panes_saving_to(Some(path.clone()));
        let (outcome, feedback) = panes.apply(vec![statusline_top()]);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(feedback, vec![AppEvent::ConfigSaved]);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
    }

    #[test]
    fn save_config_without_a_path_answers_one_failure() {
        let mut panes = panes_saving_to(None);
        let (_, feedback) = panes.apply(vec![statusline_top()]);
        assert_eq!(
            feedback,
            vec![AppEvent::ConfigSaveFailed(
                "save failed: no config path (HOME is not set)".into()
            )]
        );
    }

    fn menu_key(c: char, modifiers: KeyModifiers) -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(c), modifiers))
    }

    // Spec: the answer reaches the app before the next input.
    #[test]
    fn s_in_the_menu_saves_and_the_result_closes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut panes = panes_saving_to(Some(path.clone()));
        let mut app = App::new(80, 25);
        for event in [
            menu_key(' ', KeyModifiers::CONTROL),
            menu_key('m', KeyModifiers::NONE),
            menu_key('l', KeyModifiers::NONE),
            menu_key('s', KeyModifiers::NONE),
        ] {
            assert!(!step(&mut app, event, &mut panes));
        }
        assert!(app.menu().is_none());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
    }

    // Characterization (passes on arrival): pins the ordering the app relies
    // on. The save result is processed before the next input, so a later key
    // never meets a menu that is about to close.
    #[test]
    fn the_save_result_is_processed_before_the_next_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut panes = panes_saving_to(Some(dir.path().join("config.toml")));
        let mut app = App::new(80, 25);
        for event in [
            menu_key(' ', KeyModifiers::CONTROL),
            menu_key('m', KeyModifiers::NONE),
            menu_key('l', KeyModifiers::NONE),
        ] {
            step(&mut app, event, &mut panes);
        }
        let save = menu_key('s', KeyModifiers::NONE);
        let next = menu_key('j', KeyModifiers::NONE);
        let mut seen = Vec::new();
        for event in [save.clone(), next.clone()] {
            drive(
                |event| {
                    seen.push(event.clone());
                    app.update(event)
                },
                event,
                &mut panes,
            );
        }
        assert_eq!(seen, vec![save, AppEvent::ConfigSaved, next]);
        assert!(app.menu().is_none());
        assert_eq!(app.input(), crate::app::InputMode::Terminal);
    }

    #[test]
    fn a_failed_save_leaves_the_menu_open_with_the_error() {
        let mut panes = panes_saving_to(None);
        let mut app = App::new(80, 25);
        for event in [
            menu_key(' ', KeyModifiers::CONTROL),
            menu_key('m', KeyModifiers::NONE),
            menu_key('l', KeyModifiers::NONE),
            menu_key('s', KeyModifiers::NONE),
        ] {
            assert!(!step(&mut app, event, &mut panes));
        }
        assert_eq!(
            app.menu().and_then(|menu| menu.error()),
            Some("save failed: no config path (HOME is not set)")
        );
    }

    #[test]
    fn detect_engines_calls_the_injected_detector_once() {
        let (panes, _rx, _rec) = fake_panes(None);
        let calls = Rc::new(RefCell::new(0));
        let seen = Rc::clone(&calls);
        let mut panes = panes.with_detector(Box::new(move || *seen.borrow_mut() += 1));
        let (outcome, feedback) = panes.apply(vec![Effect::DetectEngines]);
        assert_eq!(
            (outcome, feedback, *calls.borrow()),
            (Outcome::Continue, Vec::new(), 1)
        );
    }

    // Without a chat task (unit tests) the chat effects touch no pane.
    #[test]
    fn chat_effects_are_ignored_without_a_chat_task() {
        use crate::core::ai::chat::engine::ChatRequest;
        let (mut panes, _rx, rec) = fake_panes(None);
        let request = ChatRequest {
            engine: "ollama".into(),
            model: Some("llama3:8b".into()),
            system: String::new(),
            messages: Vec::new(),
        };
        let effects = vec![
            Effect::ChatSend {
                pane: PaneId::FIRST,
                turn: 1,
                request,
            },
            Effect::ChatCancel(PaneId::FIRST),
        ];
        let (outcome, feedback) = panes.apply(effects);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        assert!(rec.borrow().specs.is_empty(), "nothing was spawned");
    }

    /// A memory whose worker start is recorded, with a transport that counts
    /// its calls and answers none.
    fn recording_memory(
        config: crate::core::ai::config::MemoryConfig,
    ) -> (
        engram::Memory,
        Rc<RefCell<u32>>,
        Arc<std::sync::atomic::AtomicUsize>,
        mpsc::Receiver<crate::core::ai::memory::port::MemReply>,
    ) {
        use crate::core::ai::memory::route::Call;
        use loopback::{CallError, Limits, Reply};
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Counting(Arc<AtomicUsize>);
        impl engram::Transport for Counting {
            async fn call(&self, _: &Call, _: Limits) -> Result<Reply, CallError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(CallError::Connect)
            }
        }
        let starts = Rc::new(RefCell::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel(8);
        let (seen, counted) = (Rc::clone(&starts), Arc::clone(&calls));
        let memory = engram::Memory::for_config(&config, move |port| {
            *seen.borrow_mut() += 1;
            engram::spawn(Counting(counted), port, tx)
        });
        (memory, starts, calls, rx)
    }

    fn memory_effect() -> Effect {
        Effect::Memory(crate::core::ai::memory::port::MemCmd::Resolve {
            pane: PaneId::FIRST,
            cwd: "/x".into(),
        })
    }

    // Spec (engram-memory): when memory is not active nothing starts and the
    // network is never touched; a memory effect still returns at once.
    #[tokio::test]
    async fn memory_effects_do_nothing_when_memory_is_not_active() {
        use crate::core::ai::config::MemoryConfig;
        let configs = [
            MemoryConfig::default(),
            MemoryConfig {
                enabled: true,
                ..MemoryConfig::default()
            },
        ];
        for config in configs {
            let (memory, starts, calls, mut rx) = recording_memory(config);
            let (panes, _rx, _rec) = fake_panes(None);
            let mut panes = panes.with_memory(memory);
            let (outcome, feedback) = panes.apply(vec![memory_effect()]);
            assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
            tokio::task::yield_now().await;
            assert_eq!(*starts.borrow(), 0);
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert!(rx.try_recv().is_err());
            panes
                .shutdown_engines(Duration::from_secs(1), Duration::from_secs(3))
                .await;
        }
    }

    // The effect returns at once; the worker answers on its own channel.
    #[tokio::test]
    async fn memory_effects_start_the_worker_lazily_when_memory_is_active() {
        use crate::core::ai::{config::MemoryConfig, memory::port::MemReply};
        let config = MemoryConfig {
            enabled: true,
            acknowledged: true,
            port: 9123,
        };
        let (memory, starts, calls, mut rx) = recording_memory(config);
        let (panes, _rx, _rec) = fake_panes(None);
        let mut panes = panes.with_memory(memory);
        assert_eq!(*starts.borrow(), 0, "nothing starts with the app");
        let (outcome, feedback) = panes.apply(vec![memory_effect()]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        assert_eq!(*starts.borrow(), 1);
        let first = rx.recv().await.expect("the worker answers");
        assert!(matches!(first, MemReply::Health { .. }), "{first:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        panes
            .shutdown_engines(Duration::from_secs(1), Duration::from_secs(3))
            .await;
    }

    #[test]
    fn memory_effects_are_ignored_without_a_memory() {
        let (mut panes, _rx, rec) = fake_panes(None);
        let (outcome, feedback) = panes.apply(vec![memory_effect()]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        assert!(rec.borrow().specs.is_empty());
    }

    #[test]
    fn the_default_detector_does_nothing() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let (outcome, feedback) = panes.apply(vec![Effect::DetectEngines]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
    }

    #[test]
    fn step_applies_a_detection_to_the_app_without_writing_to_a_pty() {
        use crate::core::ai::{Detection, EngineState, Status};
        let (mut panes, _rx, rec) = fake_panes(None);
        let mut app = App::new(80, 25);
        let (first, first_spec) = app.initial_spawn();
        panes.spawn_pane(first, &first_spec).unwrap();
        let found = Detection {
            engines: vec![EngineState {
                id: "codex",
                status: Status::Available,
                models: vec!["default".to_string()],
                version: None,
            }],
        };
        let quit = step(
            &mut app,
            AppEvent::EnginesDetected(found.clone()),
            &mut panes,
        );
        assert_eq!(
            (quit, app.engines().cloned(), rec.borrow().logs.len()),
            (false, Some(found), 1)
        );
        assert_eq!(
            rec.borrow().logs[0].lock().unwrap().writes,
            Vec::<Vec<u8>>::new()
        );
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

    // --- Store effects ---

    use crate::core::ai::chat::{
        MsgStatus,
        engine::Role,
        store::{StoreCmd, StoreReply, StoredMessage},
    };

    fn stored(role: Role, text: &str, status: MsgStatus) -> StoredMessage {
        StoredMessage {
            role,
            text: text.into(),
            status,
            engine: "ollama".into(),
            model: None,
        }
    }

    fn store_create(text: &str) -> StoreCmd {
        StoreCmd::Create {
            pane: id(1),
            profile: "programmer".into(),
            tag: None,
            first: stored(Role::User, text, MsgStatus::Complete),
            open: Vec::new(),
        }
    }

    fn store_db(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("lazarobox").join("chat.db")
    }

    /// The next store reply, or `None` if none comes within 5 s.
    async fn recv_store(rx: &mut mpsc::Receiver<StoreReply>) -> Option<StoreReply> {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .ok()
            .flatten()
    }

    fn panes_with_store(db: PathBuf) -> (Panes, mpsc::Receiver<StoreReply>) {
        let (panes, _rx, _rec) = fake_panes(None);
        let (tx, rx) = mpsc::channel(STORE_CHANNEL);
        (panes.with_store(chat_store::spawn_at(Some(db), tx)), rx)
    }

    #[tokio::test]
    async fn a_store_effect_reaches_the_store_thread_and_its_reply_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut panes, mut rx) = panes_with_store(store_db(&dir));
        panes.apply(vec![Effect::Store(store_create("hi"))]);
        let reply = recv_store(&mut rx).await;
        assert!(
            matches!(reply, Some(StoreReply::Created { .. })),
            "{reply:?}"
        );
        panes.shutdown_store(STORE_JOIN_BOUND);
    }

    #[test]
    fn a_store_effect_without_a_store_thread_is_ignored() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let (outcome, feedback) = panes.apply(vec![Effect::Store(store_create("hi"))]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        panes.shutdown_store(STORE_JOIN_BOUND);
    }

    #[tokio::test]
    async fn a_slow_store_does_not_delay_keys_or_frames() {
        let dir = tempfile::tempdir().unwrap();
        let db = store_db(&dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        panes.apply(vec![Effect::Store(store_create("hi"))]);
        let Some(StoreReply::Created { conversation, .. }) = recv_store(&mut rx).await else {
            panic!("no Created reply");
        };

        // Another process holds the write lock for 1 s, under the 2 s busy
        // timeout: every write waits, then lands.
        let holder = rusqlite::Connection::open(&db).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_secs(1));
            holder.execute_batch("ROLLBACK").unwrap();
        });

        let mut app = App::new(80, 24);
        let started = Instant::now();
        let appends = (0..20)
            .map(|i| {
                Effect::Store(StoreCmd::Append {
                    pane: id(1),
                    conversation,
                    message: stored(Role::User, &format!("m{i}"), MsgStatus::Complete),
                })
            })
            .collect();
        panes.apply(appends);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let effects = app.update(AppEvent::Key(key));
        panes.apply(effects);
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| ui::render(frame, &app, &LazaroboxTheme::default()))
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "keys and frames waited for the store: {:?}",
            started.elapsed()
        );
        release.join().unwrap();
        panes.shutdown_store(Duration::from_secs(10));
        let check = rusqlite::Connection::open(&db).unwrap();
        let stored: i64 = check
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
                [conversation],
                |r| r.get(0),
            )
            .unwrap();
        // The first message plus the 20 appends: the wait cost nothing.
        assert_eq!(stored, 21, "the appends held back by the lock were lost");
    }

    #[tokio::test]
    async fn quit_flushes_a_cancelled_append_within_the_join_bound() {
        let dir = tempfile::tempdir().unwrap();
        let db = store_db(&dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        panes.apply(vec![Effect::Store(store_create("q"))]);
        let Some(StoreReply::Created { conversation, .. }) = recv_store(&mut rx).await else {
            panic!("no Created reply");
        };
        let append = |text: &str, status| {
            Effect::Store(StoreCmd::Append {
                pane: id(1),
                conversation,
                message: stored(Role::Assistant, text, status),
            })
        };
        let started = Instant::now();
        panes.apply(vec![
            append("Hel", MsgStatus::Cancelled),
            append("done", MsgStatus::Complete),
        ]);
        panes.shutdown_store(STORE_JOIN_BOUND);
        assert!(started.elapsed() < STORE_JOIN_BOUND);

        let conn = rusqlite::Connection::open(&db).unwrap();
        let saved: Vec<(String, String)> = conn
            .prepare("SELECT content, status FROM messages WHERE seq > 1 ORDER BY seq")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            saved,
            vec![
                ("Hel".to_string(), "cancelled".to_string()),
                ("done".to_string(), "complete".to_string())
            ]
        );
    }

    #[test]
    fn a_store_reply_is_accepted_by_the_app() {
        let mut app = App::new(80, 24);
        let reply = StoreReply::Failed {
            pane: id(1),
            notify: true,
        };
        assert_eq!(app.update(AppEvent::Store(reply)), Vec::new());
    }

    // --- Chat effects ---

    use crate::core::ai::chat::engine::{ChatRequest, StreamEvent};

    fn chat_request() -> ChatRequest {
        ChatRequest {
            engine: "ollama".into(),
            model: Some("llama3:8b".into()),
            system: String::new(),
            messages: Vec::new(),
        }
    }

    /// The next event, or `None` if none comes within 5 s.
    async fn recv(rx: &mut mpsc::Receiver<chat::Tagged>) -> Option<chat::Tagged> {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .ok()
            .flatten()
    }

    fn chat_send(pane: PaneId, turn: u64) -> Effect {
        Effect::ChatSend {
            pane,
            turn,
            request: chat_request(),
        }
    }

    fn scripted(events: Vec<StreamEvent>) -> chat::Runner {
        Arc::new(move |_request, tx| {
            let events = events.clone();
            Box::pin(async move {
                for event in events {
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
            })
        })
    }

    fn panes_with_chats(runner: chat::Runner) -> (Panes, mpsc::Receiver<chat::Tagged>) {
        let (panes, _rx, _rec) = fake_panes(None);
        let (tx, rx) = mpsc::channel(chat::APP_CHANNEL);
        (panes.with_chats(Chats::new(runner, tx)), rx)
    }

    #[tokio::test(start_paused = true)]
    async fn chat_send_starts_a_turn_whose_events_come_back_tagged() {
        let events = vec![
            StreamEvent::Delta("hi".into()),
            StreamEvent::Done(Finish::Stop),
        ];
        let (mut panes, mut rx) = panes_with_chats(scripted(events));
        let (outcome, feedback) = panes.apply(vec![chat_send(id(3), 5)]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        assert_eq!(
            recv(&mut rx).await,
            Some((id(3), 5, StreamEvent::Delta("hi".into())))
        );
        assert_eq!(
            recv(&mut rx).await,
            Some((id(3), 5, StreamEvent::Done(Finish::Stop)))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn chat_cancel_stops_the_turn_of_that_pane_only() {
        let runner: chat::Runner = Arc::new(|request, tx| {
            Box::pin(async move {
                loop {
                    let tag = request.system.clone();
                    if tx.send(StreamEvent::Delta(tag)).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
        });
        let (mut panes, mut rx) = panes_with_chats(runner);
        let request = |system: &str| {
            let mut r = chat_request();
            r.system = system.into();
            r
        };
        panes.apply(vec![
            Effect::ChatSend {
                pane: id(1),
                turn: 1,
                request: request("one"),
            },
            Effect::ChatSend {
                pane: id(2),
                turn: 1,
                request: request("two"),
            },
        ]);
        recv(&mut rx).await.unwrap();
        recv(&mut rx).await.unwrap();
        panes.apply(vec![Effect::ChatCancel(id(1))]);
        tokio::time::sleep(Duration::from_secs(3)).await;
        let mut seen = Vec::new();
        while let Ok((pane, _, _)) = rx.try_recv() {
            seen.push(pane);
        }
        assert!(
            !seen.is_empty() && seen.iter().all(|p| *p == id(2)),
            "{seen:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn chat_effects_never_touch_ptys_or_detection() {
        let calls = Rc::new(RefCell::new(0));
        let seen = Rc::clone(&calls);
        let (panes, _pty_rx, rec) = fake_panes(None);
        let (tx, mut rx) = mpsc::channel(chat::APP_CHANNEL);
        let runner = scripted(vec![StreamEvent::Done(Finish::Stop)]);
        let mut panes = panes
            .with_chats(Chats::new(runner, tx))
            .with_detector(Box::new(move || *seen.borrow_mut() += 1));
        // Two live PTYs, one on the very id the chat uses.
        spawn_two(&mut panes);
        panes.apply(vec![
            chat_send(id(1), 1),
            Effect::ChatCancel(id(1)),
            chat_send(id(1), 2),
        ]);
        tokio::time::sleep(Duration::from_secs(1)).await;
        while rx.try_recv().is_ok() {}
        {
            let rec = rec.borrow();
            assert_eq!(rec.specs.len(), 2, "a chat spawns no PTY");
            for log in &rec.logs {
                let log = log.lock().unwrap();
                assert!(log.writes.is_empty(), "a chat writes to no PTY");
                assert!(log.resizes.is_empty(), "a chat resizes no PTY");
                assert_eq!(log.kills, 0, "a chat kills no PTY");
            }
        }
        assert_eq!(*calls.borrow(), 0, "a chat never starts a detection run");
        panes.apply(vec![Effect::DetectEngines]);
        assert_eq!(
            *calls.borrow(),
            1,
            "and a detection run is not held back by it"
        );
    }

    #[tokio::test]
    async fn a_panic_hook_restores_the_terminal_except_inside_a_chat_adapter() {
        assert!(
            should_restore(),
            "outside an adapter the terminal is restored"
        );
        let inside = chat::scoped(async { should_restore() }).await;
        assert!(
            !inside,
            "inside an adapter poll the panic is caught: leave the TUI alone"
        );
        assert!(should_restore(), "and it is back to normal afterwards");
    }

    #[test]
    fn a_panic_on_the_store_thread_leaves_the_terminal_alone() {
        let inside = thread::Builder::new()
            .name(chat_store::worker::THREAD_NAME.into())
            .spawn(should_restore)
            .unwrap()
            .join()
            .unwrap();
        assert!(!inside, "the store thread recovers from its own panics");
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_chats_stops_every_turn() {
        let runner: chat::Runner = Arc::new(|_request, tx| {
            Box::pin(async move {
                loop {
                    if tx.send(StreamEvent::Slow).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
        });
        let (mut panes, mut rx) = panes_with_chats(runner);
        panes.apply(vec![chat_send(id(1), 1), chat_send(id(2), 1)]);
        recv(&mut rx).await.unwrap();
        panes
            .shutdown_engines(Duration::from_secs(1), Duration::from_secs(3))
            .await;
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(rx.try_recv().is_err(), "no turn survives the shutdown");
    }

    // W5: the memory flush runs while the chats shut down, so quitting waits
    // for the slower of the two, not for both.
    #[tokio::test(start_paused = true)]
    async fn quit_flushes_the_memory_while_the_chats_shut_down() {
        use crate::core::ai::{config::MemoryConfig, memory::route::Call};
        use loopback::{CallError, Limits, Reply};
        use std::sync::Mutex;
        use tokio::time::Instant as Clock;

        /// A server that answers slowly: the probe takes 290 ms, the project
        /// lookup 790 ms, so the worker is still busy when the bound ends.
        struct Slow;
        impl engram::Transport for Slow {
            async fn call(&self, call: &Call, _: Limits) -> Result<Reply, CallError> {
                if call.target.starts_with("/health") {
                    tokio::time::sleep(Duration::from_millis(290)).await;
                    let (status, body) = crate::core::ai::memory::testing::split(include_bytes!(
                        "core/ai/memory/fixtures/3.4.0/health_3_4_0.http"
                    ));
                    return Ok(Reply {
                        status,
                        body: body.to_vec(),
                    });
                }
                tokio::time::sleep(Duration::from_millis(790)).await;
                Err(CallError::Connect)
            }
        }
        /// Notes when its turn is dropped, i.e. when the chat shuts down.
        struct Dropped(Arc<Mutex<Option<Clock>>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = Some(Clock::now());
            }
        }

        let dropped_at = Arc::new(Mutex::new(None));
        let seen = Arc::clone(&dropped_at);
        let runner: chat::Runner = Arc::new(move |_request, tx| {
            let guard = Dropped(Arc::clone(&seen));
            Box::pin(async move {
                // Holding `tx` keeps the turn open until it is aborted.
                let (_guard, _tx) = (guard, tx);
                std::future::pending::<()>().await;
            })
        });
        let config = MemoryConfig {
            enabled: true,
            acknowledged: true,
            port: 9123,
        };
        let (tx, _rx) = mpsc::channel(8);
        let memory = Memory::for_config(&config, move |port| engram::spawn(Slow, port, tx));
        let (mut panes, _chat_rx) = panes_with_chats(runner);
        panes = panes.with_memory(memory);
        panes.apply(vec![chat_send(id(1), 1), memory_effect()]);
        tokio::task::yield_now().await;

        let start = Clock::now();
        panes
            .shutdown_engines(MEMORY_QUIT_BOUND, QUIT_KILL_BOUND)
            .await;
        assert_eq!(
            Clock::now() - start,
            MEMORY_QUIT_BOUND,
            "as long as the slower of the two, not the sum"
        );
        let dropped = dropped_at.lock().unwrap().expect("the turn was stopped");
        assert_eq!(dropped, start, "the chat stopped at once, not after memory");
    }

    // S1: the token is read only when memory is active, and only when the
    // worker starts.
    #[tokio::test]
    async fn the_token_is_read_only_when_an_active_memory_starts() {
        use crate::core::ai::config::MemoryConfig;
        let inactive = [
            MemoryConfig::default(),
            MemoryConfig {
                enabled: true,
                ..MemoryConfig::default()
            },
        ];
        for config in inactive {
            let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counted = Arc::clone(&reads);
            let (tx, _rx) = mpsc::channel(8);
            let mut memory = memory_for(
                &config,
                move || {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    None
                },
                || engram::serve::NoServer,
                tx,
            );
            memory.send(crate::core::ai::memory::port::MemCmd::Resolve {
                pane: PaneId::FIRST,
                cwd: "/x".into(),
            });
            memory.shutdown(Duration::from_secs(1)).await;
            assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
        }

        // Active: nothing is read before the first command, one read at start.
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let port = socket.local_addr().unwrap().port();
        let config = MemoryConfig {
            enabled: true,
            acknowledged: true,
            port,
        };
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let (tx, mut rx) = mpsc::channel(8);
        let looks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut memory = memory_for(
            &config,
            move || {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                None
            },
            {
                let looks = Arc::clone(&looks);
                move || LooksOnly(looks)
            },
            tx,
        );
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
        memory.send(crate::core::ai::memory::port::MemCmd::Resolve {
            pane: PaneId::FIRST,
            cwd: "/x".into(),
        });
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Nothing listens on the port, so the worker looks for a binary: it
        // asks the injected launcher, never the real PATH.
        let told = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(crate::core::ai::memory::port::MemReply::Unavailable {
                    pane: None,
                    ..
                }) = rx.recv().await
                {
                    break;
                }
            }
        })
        .await;
        assert!(told.is_ok(), "the worker never weighed the offer");
        assert_eq!(looks.load(std::sync::atomic::Ordering::SeqCst), 1);
        memory.shutdown(Duration::from_secs(1)).await;
    }

    /// A launcher that finds no binary and counts how often it was asked.
    struct LooksOnly(Arc<std::sync::atomic::AtomicUsize>);

    impl engram::serve::Launcher for LooksOnly {
        async fn find(&self) -> crate::core::ai::memory::offer::Binary {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::core::ai::memory::offer::Binary::Missing
        }

        async fn spawn(
            &self,
            _binary: &Path,
            _port: u16,
        ) -> std::io::Result<engram::serve::Running> {
            Err(std::io::Error::other("not available"))
        }

        async fn log_line(&self, _from: u64) -> Option<String> {
            None
        }
    }

    // The runtime stops an endless answer at 1 MiB; the app has nothing left
    // to cancel, so the cap is reported once.
    #[tokio::test(start_paused = true)]
    async fn an_endless_answer_ends_truncated_at_1_mib_without_a_second_cancel() {
        use crate::{
            app::PaneView,
            core::ai::chat::{ANSWER_CAP, MsgStatus},
        };
        let runner: chat::Runner = Arc::new(|_request, tx| {
            Box::pin(async move {
                let block = "a".repeat(64 * 1024);
                while tx.send(StreamEvent::Delta(block.clone())).await.is_ok() {}
            })
        });
        let (mut panes, mut rx) = panes_with_chats(runner);
        let mut app = App::new(80, 25);
        app.open_chat(None);
        let chat = app.focused();
        panes.apply(app.chat_send(chat, "hi"));
        let mut cancels = 0;
        while let Some((pane, turn, event)) = recv(&mut rx).await {
            let effects = app.update(AppEvent::Chat(pane, turn, event));
            cancels += effects
                .iter()
                .filter(|e| matches!(e, Effect::ChatCancel(_)))
                .count();
            if app.take_dirty()
                && matches!(app.pane_view(chat), Some(PaneView::Chat(c)) if c.phase() == crate::core::ai::chat::Phase::Idle)
            {
                break;
            }
        }
        let Some(PaneView::Chat(state)) = app.pane_view(chat) else {
            panic!("no chat")
        };
        let answer = state.messages().last().unwrap();
        assert_eq!(
            (answer.text.len(), answer.status),
            (ANSWER_CAP, MsgStatus::Truncated)
        );
        assert_eq!(cancels, 0);
        assert!(recv(&mut rx).await.is_none(), "nothing follows the cap");
    }

    // Task 5.2 across app and runtime: confirming `w q` on a streaming chat
    // aborts its turn at once, and the engine stops sending.
    #[tokio::test(start_paused = true)]
    async fn closing_a_streaming_chat_stops_its_turn() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Flag(Arc<AtomicBool>);
        impl Drop for Flag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let runner: chat::Runner = Arc::new(move |_request, tx| {
            let flag = Flag(flag.clone());
            Box::pin(async move {
                let _flag = flag;
                loop {
                    if tx.send(StreamEvent::Delta("x".into())).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
        });
        let (mut panes, mut rx) = panes_with_chats(runner);
        let mut app = App::new(80, 25);
        app.open_chat(None);
        let chat = app.focused();
        panes.apply(app.chat_send(chat, "hi"));
        let (pane, turn, event) = recv(&mut rx).await.unwrap();
        app.update(AppEvent::Chat(pane, turn, event));
        let key = |c: char| AppEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        app.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char(' '),
            KeyModifiers::CONTROL,
        )));
        app.update(key('w'));
        app.update(key('q'));
        let (outcome, _) = panes.apply(app.update(key('y')));
        assert_eq!(outcome, Outcome::Continue);
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(dropped.load(Ordering::SeqCst), "the adapter was dropped");
        let mut late = Vec::new();
        while let Ok(event) = rx.try_recv() {
            late.push(event);
        }
        for (pane, turn, event) in late {
            assert_eq!(app.update(AppEvent::Chat(pane, turn, event)), vec![]);
        }
    }
}
