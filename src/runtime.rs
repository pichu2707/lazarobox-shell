//! The impure shell around `App`: terminal, PTY, timers and the event loop.
//!
//! Redraws are driven by a dirty flag and a 16 ms tick, so output floods are
//! capped at about 60 fps. Mouse capture reports only the wheel (button
//! reporting with SGR encoding, no motion modes) and is on by default; a
//! terminal's own selection then needs Shift. `mouse = false` never enables it.
//! `restore_to` always disables it, so every exit path leaves the terminal clean.

use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    io::{self, Write, stdout},
    panic,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub mod ai_detect;
pub mod anim;
mod auto_type;
pub mod bounded_read;
pub mod chat;
pub mod chat_store;
mod config_write;
pub mod desktop;
pub mod engram;
pub mod gguf;
pub mod hf;
pub mod jail;
pub mod jobs;
pub mod login_path;
pub mod loopback;
pub mod mcp;
pub mod notify;
pub mod proc;
pub mod prompts;
#[cfg(test)]
pub(crate) mod refused_port;
mod run_check;
pub mod tools;
pub mod update;
pub mod watch;
pub mod workspace;

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
        event::{
            DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
            Event, EventStream, MouseEventKind,
        },
        execute, terminal,
    },
};
use tokio::{
    signal::unix::{Signal, SignalKind, signal},
    sync::{mpsc, oneshot},
    time::{MissedTickBehavior, interval, timeout},
};

use crate::{
    app::{App, AppEvent, Effect, RunEvent},
    core::{
        ai::{
            Detection,
            chat::{
                approval,
                notify::{bell_after_failed_desktop, rings_at_once},
                prompts::Resolved,
                store::StoreReply,
            },
            config::{AiConfig, NotifyMode, Profile},
            memory::port::MemReply,
        },
        config::{Config, config_path},
        layout::PaneId,
        mouse::{CAPTURE_OFF, CAPTURE_ON},
        pane::{CursorKind, CursorShape},
        pty::{PtyEvent, PtyHandle, PtySink, SpawnSpec, portable::spawn_portable},
    },
    ui::{self, theme::LazaroboxTheme},
};

/// The terminal bell character.
const BEL: u8 = 0x07;
/// Time between redraw checks.
const TICK: Duration = Duration::from_millis(16);
/// Capacity of the reader-to-loop channel; a full channel stalls the reader.
const PTY_CHANNEL: usize = 256;
/// PTY events handled per wake-up before yielding to the other branches.
const EVENT_BUDGET: usize = 256;
/// Time between reads of the child's working directory.
const CWD_POLL: Duration = Duration::from_secs(1);
/// How often the managed llama-server is checked for its idle stop.
const IDLE_CHECK: Duration = Duration::from_secs(30);
/// Longest a quit waits for the children to die: HUP grace (300 ms) plus the
/// reap wait (2 s) plus margin.
const QUIT_KILL_BOUND: Duration = Duration::from_secs(3);
/// How long quitting waits for pending memory work (session ends).
const MEMORY_QUIT_BOUND: Duration = Duration::from_secs(1);
/// Capacity of the channel from the memory worker to the event loop.
const MEMORY_CHANNEL: usize = 64;
/// Longest a quit waits for the store thread to write what is queued.
const STORE_JOIN_BOUND: Duration = Duration::from_secs(2);
/// Capacity of the channel from the path checks of runs to the event loop.
const RUN_CHANNEL: usize = 16;
/// Capacity of the channel from the store thread to the event loop.
const STORE_CHANNEL: usize = 64;
/// Longest the app waits for a prompt job before it uses the built-in prompt.
const PREPARE_BOUND: Duration = Duration::from_secs(2);
/// Capacity of the channel from the prompt jobs to the event loop.
const PROMPT_CHANNEL: usize = 16;

/// Capacity of the Hugging Face channel: the replies, and a download's
/// progress about every 250 ms (dropped while the loop is behind).
const HUB_CHANNEL: usize = 16;
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
    // The capture bytes wait in `host_out` until `run` has entered the
    // terminal modes; `mouse = false` yields no effect at all.
    let mut effects: Vec<Effect> = app.sync_mouse().into_iter().collect();
    // Startup trigger: the first result arrives on `ai_rx` once the loop runs.
    effects.extend(app.request_detection());
    effects
}

/// How long the runtime waits for blocking threads when it is dropped at
/// exit. All the quit work (store, memory, chats, children) is awaited inside
/// [`run`]; what can still be running here is a wedged probe or a `waitid` on
/// a child that is being killed, and neither may hang the quit.
pub const RUNTIME_QUIT_BOUND: Duration = Duration::from_secs(1);

/// Owns the runtime and bounds its shutdown on every way out, a panic
/// included: dropping it waits at most `bound` for the blocking threads.
pub struct QuitGuard {
    rt: Option<tokio::runtime::Runtime>,
    bound: Duration,
}

impl QuitGuard {
    pub fn new(rt: tokio::runtime::Runtime, bound: Duration) -> Self {
        Self {
            rt: Some(rt),
            bound,
        }
    }

    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.rt
            .as_ref()
            .expect("the runtime lives until the guard drops")
            .block_on(future)
    }
}

impl Drop for QuitGuard {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(self.bound);
        }
    }
}

/// Runs the terminal pane until the child exits or the user quits.
pub async fn run() -> io::Result<()> {
    // First, before any child or pane exists: from here on every process
    // group lazarobox starts is stopped once it ends, however it ends.
    watch::start();
    // Probed off the async threads: macOS spawns `sandbox-exec` for it.
    let jail_probe = tokio::task::spawn_blocking(jail::probe);
    // Asked first so the shell runs while the rest of the start-up goes on.
    let login_path = tokio::spawn(login_path::resolve(
        env::var_os("PATH").unwrap_or_default(),
        env::var_os("SHELL"),
        login_path::LOGIN_TIMEOUT,
    ));
    let (cols, rows) = terminal::size()?;
    let cwd = env::current_dir().unwrap_or_else(|_| "/".into());
    let shell = env::var("SHELL").ok();
    // Resolved once: the startup read and the menu's save use the same file.
    let config_path = config_path(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"));
    // Read once, here: the config parser itself never touches the environment.
    let engram_port = env::var("ENGRAM_PORT").ok();
    // Probed once: whether the CLI engines can be confined (Landlock or
    // Seatbelt). A probe that panicked counts as not probed.
    let jail = jail_probe
        .await
        .unwrap_or(crate::core::ai::jail::JailSupport::Unavailable(
            crate::core::ai::jail::JailOff::NotProbed,
        ));
    let mut app = app_from_config(cols, rows, config_path.as_deref(), engram_port.as_deref())
        .with_jail(jail)
        .with_hostname(local_hostname().as_deref())
        .with_dir_check(|dir| dir.is_dir())
        .with_utf8_locale(approval::names_utf8(
            env::var("LC_ALL").ok().as_deref(),
            env::var("LC_CTYPE").ok().as_deref(),
            env::var("LANG").ok().as_deref(),
        ))
        .with_session_ids(engram::worker::new_session_id)
        .with_token_check(token_in_env)
        .with_hf_tokens(hf::token_facts)
        .with_env(shell.as_deref(), env::var_os("HOME").map(Into::into), cwd);
    let (id, spec) = app.initial_spawn();

    let shutdown = Shutdown::new()?;
    let (tx, rx) = mpsc::channel(PTY_CHANNEL);
    let (ai_tx, ai_rx) = mpsc::channel(1);
    // One machine for detection and chat turns: the login shell's `PATH`
    // first, then the process entries it lacks (a launcher gives the app a
    // bare one, and the user's terminal resolves programs by the login
    // order). Never written back to the process environment. A failed or
    // panicked lookup keeps the process `PATH` but only its absolute entries
    // (relative and empty ones are dropped); with none left it is a fixed
    // default.
    let process_path = env::var_os("PATH").unwrap_or_default();
    let path = login_path
        .await
        .unwrap_or_else(|_| login_path::sanitize(&process_path));
    let cli_env = chat::CliEnv::from_env(jail).with_path(path);
    // Detection reads codex's model list through the same jail as its chat.
    let detect_env = cli_env.clone();
    // The codex model list is reused for 10 minutes across runs (`r` re-lists).
    let listing_cache = Arc::new(ai_detect::ListingCache::new(Arc::new(Instant::now)));
    let (chat_tx, chat_rx) = mpsc::channel(chat::APP_CHANNEL);
    let (memory_tx, memory_rx) = mpsc::channel(MEMORY_CHANNEL);
    // Off unless the config activates it: then no worker ever starts, nothing
    // touches the network and the token is never read from the environment.
    // A pane's command still gets `memory applies after a restart` back, for
    // memory turned on in the menu during this run.
    let memory = memory_for(
        &app.ai().memory,
        || env::var("ENGRAM_HTTP_TOKEN").ok(),
        {
            let path = cli_env.path.clone();
            move || engram::serve::Server::from_env(path)
        },
        memory_tx,
    );
    // Notify only: a cached or fetched newer version becomes a text mark.
    // Off (`[updates] check = false`) nothing is read, asked or shown.
    let (update_tx, update_rx) = mpsc::channel(1);
    let (notified_tx, notified_rx) = mpsc::channel(4);
    let (managed_tx, managed_rx) = mpsc::channel(4);
    let (shared_tx, shared_rx) = mpsc::channel(1);
    let (hub_tx, hub_rx) = mpsc::channel(HUB_CHANNEL);
    let update_cache = update::cache_path(
        env::var_os("XDG_DATA_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    );
    // The menu's `r` asks again on the same channel (the menu only offers it
    // with the check on).
    let check_update = {
        let (cache, tx, path) = (
            update_cache.clone(),
            update_tx.clone(),
            cli_env.path.clone(),
        );
        move || {
            update::check_now(
                cache.clone(),
                update::now_secs(),
                env!("CARGO_PKG_VERSION"),
                update::CurlFetcher::crates_io(path.clone()),
                tx.clone(),
            );
        }
    };
    let announced = update::begin(
        app.config().update_check,
        update_cache,
        update::now_secs(),
        env!("CARGO_PKG_VERSION"),
        update::CurlFetcher::crates_io(cli_env.path.clone()),
        update_tx,
    );
    app = app
        .with_update(announced)
        .with_install_env(Some(update::install_env(&cli_env.path)))
        .with_cargo_check(|cargo| chat::trusted_cargo_path(cargo).is_some());
    let (run_tx, run_rx) = mpsc::channel(RUN_CHANNEL);
    let (store_tx, store_rx) = mpsc::channel(STORE_CHANNEL);
    let (prompt_tx, prompt_rx) = mpsc::channel(PROMPT_CHANNEL);
    let config_dir = config_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_owned);
    let store_path = chat_store::resolve_path(
        env::var_os("XDG_DATA_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    );
    let store_dir = store_path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_owned);
    let tool_host = tools::ToolHost::from_env();
    let servers_home = env::var_os("HOME").map(PathBuf::from);
    // Read at startup, like the memory port: detection and chat both use
    // these, so an engine is chatted with where it was detected. Only a menu
    // save that turns the managed llama-server on changes them while running.
    let server_ports = app.ai().servers().clone();
    let managed_log = engram::serve::state_dir(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))
        .map(|dir| dir.join("llama-server.log"));
    // The llama-server lazarobox starts itself, when `managed = true`: only a
    // llamacpp turn starts it, on llamacpp's port, and quitting stops it.
    // Its extra arguments, read at startup like the ports: the menu does not
    // write them.
    let managed_args = app.ai().managed_options().args.clone();
    let managed = app.ai().managed().map(|server| {
        Arc::new(
            chat::Managed::new(
                cli_env.path.clone(),
                server.model.clone(),
                server_ports
                    .port("llamacpp")
                    .expect("llamacpp is a catalog server"),
                managed_log.clone(),
            )
            .with_args(managed_args.clone()),
        )
    });
    let live = chat::LiveServers::new(server_ports, managed);
    // The idle stop: checked now and then, with the time the menu set last.
    live.set_idle(app.ai().managed_options().idle());
    tokio::spawn({
        let live = live.clone();
        async move {
            let mut check = tokio::time::interval(IDLE_CHECK);
            loop {
                check.tick().await;
                live.stop_if_idle(Instant::now()).await;
            }
        }
    });
    // What the machine offers for notifications (display, bus, notify-send,
    // sound players), probed once now in the background: a slow lookup never
    // delays a send, it just fills the slot later. The `t` test refreshes it.
    let plan = notify::PlanSlot::default();
    notify::probe_in_background(&plan, notify::Session::current, cli_env.path.clone());
    let mut panes = Panes::new(tx, Box::new(spawn_portable))
        .with_memory(memory)
        .with_run_checks(run_check::Checker::new(run_check::process_env(), run_tx))
        .with_config_path(config_path)
        .with_update_checker(Box::new(check_update))
        .with_detector(Box::new({
            let live = live.clone();
            let model_dirs = model_dirs();
            move |force| {
                spawn_detection(
                    ai_tx.clone(),
                    detect_env.clone(),
                    Arc::clone(&listing_cache),
                    force,
                    live.ports(),
                    Arc::clone(&model_dirs),
                );
            }
        }))
        .with_idle(Box::new({
            let live = live.clone();
            move |idle| live.set_idle(idle)
        }))
        // The quit prompt names the llama-server when quitting would cut a
        // reply to another lazarobox that uses it.
        .with_shared_check(Box::new({
            let live = live.clone();
            move || {
                let (live, tx) = (live.clone(), shared_tx.clone());
                tokio::spawn(async move {
                    if let Some(managed) = live.managed()
                        && managed.answering_others().await
                    {
                        let _ = tx.send(()).await;
                    }
                });
            }
        }))
        // A save that turns the managed llama-server on: the next turns and
        // detections use it at once; the one it replaces stops for good.
        .with_manager(Box::new({
            let live = live.clone();
            let path = cli_env.path.clone();
            move |model, port| {
                let server = Arc::new(
                    chat::Managed::new(path.clone(), model, port, managed_log.clone())
                        .with_args(managed_args.clone()),
                );
                let old = live.manage(Arc::clone(&server));
                let tx = managed_tx.clone();
                tokio::spawn(async move {
                    if let Some(old) = old {
                        old.retire().await;
                    }
                    let _ = tx.send(server.ensure().await).await;
                });
            }
        }))
        .with_desktop(Box::new({
            let path = cli_env.path.clone();
            let busy = Arc::new(AtomicBool::new(false));
            let plan = plan.clone();
            move |body, fallback| {
                notify::spawn_send(notify::Job {
                    path: path.clone(),
                    body,
                    fallback,
                    busy: Arc::clone(&busy),
                    plan: plan.clone(),
                });
            }
        }))
        .with_sound(Box::new({
            let path = cli_env.path.clone();
            let busy = Arc::new(AtomicBool::new(false));
            let plan = plan.clone();
            move || {
                notify::spawn_sound(notify::SoundJob {
                    path: path.clone(),
                    busy: Arc::clone(&busy),
                    plan: plan.clone(),
                });
            }
        }))
        .with_notify_test(Box::new({
            let path = cli_env.path.clone();
            let busy = Arc::new(AtomicBool::new(false));
            let plan = plan.clone();
            move |mode, sound, fallback| {
                notify::spawn_test(notify::TestJob {
                    path: path.clone(),
                    mode,
                    sound,
                    fallback,
                    report: notified_tx.clone(),
                    session: notify::Session::current,
                    busy: Arc::clone(&busy),
                    plan: plan.clone(),
                });
            }
        }))
        .with_preparer(Box::new(move |pane, profile, origin, context| {
            spawn_prepare(
                config_dir.clone(),
                (pane, profile, origin, context),
                prompt_tx.clone(),
            );
        }))
        .with_chats(
            Chats::new(
                chat::runner_live(live.clone(), cli_env.clone(), Some(tool_host.clone())),
                chat_tx,
            )
            .with_live(live)
            // The sockets of tool sessions: `$XDG_RUNTIME_DIR/lazarobox`, else
            // `<data>/run`. Without a private folder every turn runs as text.
            .with_tools(
                tool_host,
                mcp::candidate_dirs(
                    env::var_os("XDG_RUNTIME_DIR").as_deref(),
                    store_dir.as_deref(),
                ),
            )
            // The user's servers, started only by a turn with tools on. They
            // need a home to run in: without `$HOME` none is started.
            .with_servers(
                servers_home
                    .as_ref()
                    .map_or_else(Vec::new, |_| app.ai().mcp_servers().to_vec()),
                servers_home.clone().unwrap_or_default(),
                chat::wires_for(cli_env.clone()),
            ),
        )
        .with_store(chat_store::spawn_at(store_path, store_tx))
        // The picker's Hugging Face screens: curl from the same PATH as the
        // update check, into the Hugging Face cache.
        .with_hub(hf::Hub::new(
            cli_env.path.clone(),
            hub_dir(),
            hf::token_env,
            hub_tx,
        ));
    let inbox = Inbox {
        pty: rx,
        ai: ai_rx,
        chat: chat_rx,
        memory: memory_rx,
        store: store_rx,
        prompt: prompt_rx,
        update: update_rx,
        run: run_rx,
        notified: notified_rx,
        managed: managed_rx,
        shared: shared_rx,
        hub: hub_rx,
    };
    // The first pane is spawned before the UI starts: a failure is fatal.
    panes.spawn_pane(id, &spec)?;
    panes.apply(startup_effects(&mut app));

    let mut terminal = ratatui::init();
    install_panic_hook();
    // The pending start-up capture leaves right after the modes, before the
    // first frame.
    let entered =
        enter_modes(&mut stdout()).and_then(|()| flush_host(&mut stdout(), &mut panes.host_out));
    let result = match entered {
        Ok(()) => event_loop(&mut terminal, &mut panes, inbox, shutdown, app).await,
        Err(error) => Err(error),
    };

    // The user gets the prompt back first; the children are stopped after.
    // `rx` went away with the loop, so readers stuck on a full channel stopped.
    restore();
    // Quit order, and why:
    // 1. Everything lazarobox started (turns, MCP servers, the managed
    //    llama-server) is asked to end with SIGHUP and SIGTERM at once, on a
    //    blocking thread; only what still runs after QUIT_GRACE gets SIGKILL.
    //    The steps below run meanwhile. If lazarobox is killed now (a terminal
    //    closed in a hurry), the watcher repeats the same polite stop.
    // 2. The loop already returned, so the final Cancel and Append effects of
    //    every open turn were applied (they must be emitted before the store
    //    is shut down, or the last messages are lost).
    // 3. The store thread drains what is queued, inside STORE_JOIN_BOUND.
    // 4. Memory flush and chat shutdown run together (network vs local
    //    processes), each inside its own bound; the chats only once step 1
    //    is over, so nothing is killed before its grace.
    // 5. The PTY children are stopped with whatever is left of the one bound.
    // 6. The jobs the panes leave running (a pane is a terminal: lazarobox
    //    never stops them) are named on stderr.
    let polite = tokio::task::spawn_blocking(|| watch::stop_listed(watch::QUIT_GRACE));
    panes.shutdown_store(STORE_JOIN_BOUND);
    let started = Instant::now();
    panes
        .shutdown_engines_after(
            async {
                let _ = polite.await;
            },
            MEMORY_QUIT_BOUND,
            QUIT_KILL_BOUND,
        )
        .await;
    let sessions = std::mem::take(&mut panes.sessions);
    panes.shutdown(QUIT_KILL_BOUND.saturating_sub(started.elapsed()));
    let left =
        tokio::task::spawn_blocking(move || jobs::left_running(&sessions, jobs::HANGUP_SETTLE))
            .await
            .unwrap_or_default();
    if let Some(report) = jobs::report(&left) {
        eprint!("{report}");
    }
    result
}

/// The folders the local llama.cpp models are looked for in, from this
/// process's environment.
fn model_dirs() -> Arc<[PathBuf]> {
    let var = |name: &str| env::var_os(name).map(PathBuf::from);
    let found = crate::core::ai::gguf::model_dirs(&crate::core::ai::gguf::DirsEnv {
        home: var("HOME"),
        xdg_data_home: var("XDG_DATA_HOME"),
        xdg_cache_home: var("XDG_CACHE_HOME"),
        llama_cache: var("LLAMA_CACHE"),
        hf_hub_cache: var("HF_HUB_CACHE"),
        hf_home: var("HF_HOME"),
        macos: cfg!(target_os = "macos"),
    });
    found.into()
}

/// The Hugging Face cache folder downloads go to, from this process's
/// environment, as the model scan looks for it.
fn hub_dir() -> Option<PathBuf> {
    let var = |name: &str| env::var_os(name).map(PathBuf::from);
    crate::core::ai::gguf::hub_dir(&crate::core::ai::gguf::DirsEnv {
        home: var("HOME"),
        xdg_data_home: var("XDG_DATA_HOME"),
        xdg_cache_home: var("XDG_CACHE_HOME"),
        llama_cache: var("LLAMA_CACHE"),
        hf_hub_cache: var("HF_HUB_CACHE"),
        hf_home: var("HF_HOME"),
        macos: cfg!(target_os = "macos"),
    })
}

/// Whether `ENGRAM_HTTP_TOKEN` is set and not empty: all the app learns of
/// the token, which decides if the delete prompt can offer to delete notes.
/// The app asks only when memory is on and a conversation has notes.
fn token_in_env() -> bool {
    token_is_set(env::var("ENGRAM_HTTP_TOKEN").ok().as_deref())
}

/// The rule behind [`token_in_env`], the same one the loopback client applies
/// before it sends a bearer header: an empty token is no token.
fn token_is_set(value: Option<&str>) -> bool {
    value.is_some_and(|token| !token.is_empty())
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
    let off_answers = replies.clone();
    Memory::for_config(config, move |port| {
        engram::spawn_with(Loopback::new(port, read_token()), launcher(), port, replies)
    })
    .answering_when_off(off_answers)
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
        .with_chat_history()
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
    prompt: mpsc::Receiver<(PaneId, Resolved)>,
    update: mpsc::Receiver<Option<String>>,
    /// What the path checks of runs found.
    run: mpsc::Receiver<(PaneId, RunEvent)>,
    /// What the notification test did.
    notified: mpsc::Receiver<String>,
    /// How the start of a llama-server the menu turned on went.
    managed: mpsc::Receiver<Result<(), String>>,
    /// The llama-server lazarobox started is answering another lazarobox.
    shared: mpsc::Receiver<()>,
    /// The Hub's replies and the progress and end of a download.
    hub: mpsc::Receiver<AppEvent>,
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
        prompt: mut prompt_rx,
        update: mut update_rx,
        run: mut run_rx,
        notified: mut notified_rx,
        managed: mut managed_rx,
        shared: mut shared_rx,
        hub: mut hub_rx,
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
    // Armed only while a chat awaits a memory reply: no wait lasts for ever.
    let mut memory_watch = anim::Animation::every(anim::MEMORY_PERIOD);

    loop {
        anim.sync(app.animating());
        memory_watch.sync(app.memory_waiting());
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
            // The memory keeps a sender for its whole life, so this branch
            // waits (it never reads a closed channel before shutdown).
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
            // Each job holds a sender only while it runs, and `run` keeps
            // the preparer (and so a sender) alive, so this never reads a
            // closed channel.
            Some((pane, resolved)) = prompt_rx.recv() => {
                if step(&mut app, AppEvent::PromptReady(pane, resolved), panes) {
                    return Ok(());
                }
            }
            // The check thread holds the only sender and may be gone already,
            // so a closed channel just ends this arm's pattern.
            Some(update) = update_rx.recv() => {
                if step(&mut app, AppEvent::UpdateChecked(update), panes) {
                    return Ok(());
                }
            }
            // `panes` keeps the sender of the checks for the whole loop, so
            // this never reads a closed channel.
            Some((pane, event)) = run_rx.recv() => {
                if step(&mut app, AppEvent::Run(pane, event), panes) {
                    return Ok(());
                }
            }
            Some(report) = notified_rx.recv() => {
                if step(&mut app, AppEvent::NotifyTested(report), panes) {
                    return Ok(());
                }
            }
            Some(started) = managed_rx.recv() => {
                if step(&mut app, AppEvent::ManagedStarted(started), panes) {
                    return Ok(());
                }
            }
            Some(()) = shared_rx.recv() => {
                if step(&mut app, AppEvent::ServerAnswersOthers, panes) {
                    return Ok(());
                }
            }
            // `panes` keeps the worker, and so a sender, for the whole loop.
            Some(event) = hub_rx.recv() => {
                if step(&mut app, event, panes) {
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
            () = memory_watch.tick() => {
                if step(&mut app, AppEvent::MemoryTick, panes) {
                    return Ok(());
                }
            }
            _ = tick.tick() => {
                // `select!` picks a ready branch at random, so output that is
                // queued could lose to this tick and be probed around: while
                // a written run settles, the queued pty output is fed first
                // (within the budget; the rest is fed before the next probe
                // of a later tick, and a settle needs 250 ms of quiet anyway).
                if !app.run_probes().is_empty() {
                    for _ in 0..EVENT_BUDGET {
                        let Ok((id, event)) = rx.try_recv() else { break };
                        if step(&mut app, AppEvent::Pty(id, event), panes) {
                            return Ok(());
                        }
                    }
                }
                // Only while a written run settles: an idle app reads nothing.
                for event in panes.probes(&app.run_probes()) {
                    if step(&mut app, event, panes) {
                        return Ok(());
                    }
                }
                if app.take_dirty() {
                    terminal.draw(|frame| ui::render(frame, &app, &theme))?;
                }
                apply_cursor_style(out, app.cursor_shape(), &mut applied_cursor)?;
                panes.stage_bell();
                flush_host(out, &mut panes.host_out)?;
            }
        }
    }
}

/// Starts one desktop notification: the body and the failure fallback flag.
type DesktopSendFn = dyn FnMut(String, Option<Arc<AtomicBool>>);

/// Starts the notification test: mode, sound flag, fallback flag.
type TestSendFn = dyn FnMut(NotifyMode, bool, Option<Arc<AtomicBool>>);

/// Starts the managed llama-server with a model on a port.
type ManageFn = dyn FnMut(PathBuf, u16);
type IdleFn = dyn FnMut(Option<Duration>);

/// Starts a prompt preparation: pane, profile, origin cwd, context flag.
type PrepareFn = dyn FnMut(PaneId, Profile, Option<PathBuf>, bool);

/// Prepares the system prompt of the conversation started in a pane on a
/// thread of its own (the override files and the repository are read from
/// the disk) and sends it to the event loop.
fn spawn_prepare(
    config_dir: Option<PathBuf>,
    (pane, profile, origin, context): (PaneId, Profile, Option<PathBuf>, bool),
    tx: mpsc::Sender<(PaneId, Resolved)>,
) {
    spawn_prepare_with(pane, profile, PREPARE_BOUND, tx, move || {
        let shell = env::var("SHELL").ok();
        let home = env::var_os("HOME").map(PathBuf::from);
        prompts::prepare_prompt(
            config_dir.as_deref(),
            profile,
            origin.as_deref(),
            home.as_deref(),
            context,
            shell.as_deref(),
            workspace::local_date(),
        )
    });
}

/// Runs `work` on a thread of its own and sends exactly one answer for
/// `pane`: its result, or the built-in prompt with a notice when it panics,
/// the thread cannot be spawned or it takes longer than `bound`. A result
/// that arrives after the bound has nobody to receive it. Needs a tokio
/// runtime for the wait.
fn spawn_prepare_with(
    pane: PaneId,
    profile: Profile,
    bound: Duration,
    tx: mpsc::Sender<(PaneId, Resolved)>,
    work: impl FnOnce() -> Resolved + Send + 'static,
) {
    let (done_tx, done_rx) = oneshot::channel();
    let started = thread::Builder::new()
        .name(prompts::THREAD_NAME.into())
        .spawn(move || {
            let result = panic::catch_unwind(panic::AssertUnwindSafe(work));
            // The receiver is gone after the bound: the late result is dropped.
            let _ = done_tx.send(result.ok());
        });
    tokio::spawn(async move {
        let resolved = if started.is_err() {
            prompts::fallback_prompt(profile, prompts::FAILED)
        } else {
            match timeout(bound, done_rx).await {
                Ok(Ok(Some(resolved))) => resolved,
                Ok(_) => prompts::fallback_prompt(profile, prompts::FAILED),
                Err(_) => prompts::fallback_prompt(profile, prompts::TIMED_OUT),
            }
        };
        // A closed channel means the app is quitting.
        let _ = tx.send((pane, resolved)).await;
    });
}

/// Builds the child of a pane; the production one is `spawn_portable`.
type SpawnFn = Box<dyn FnMut(&SpawnSpec, PtySink) -> io::Result<Box<dyn PtyHandle>>>;
type ListFn = dyn Fn(&[jobs::Session]) -> Vec<jobs::Job>;

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
    /// A `Bell` effect arrived since the last flush (chat-notify). Several in
    /// the same tick make one BEL; see [`Panes::stage_bell`].
    bell: bool,
    /// Raised by a desktop send that failed in `desktop` mode: the next
    /// flush rings the bell for that event.
    late_bell: Arc<AtomicBool>,
    /// Starts one desktop notification in the background: the body and, when
    /// an unavailable desktop or a failure must ring the bell, the flag to
    /// raise. The availability is probed there, off the UI thread. Does
    /// nothing unless `with_desktop` installs the production one.
    desktop_send: Box<DesktopSendFn>,
    /// Starts lazarobox's own sound in the background. Does nothing unless
    /// `with_sound` installs the production one.
    sound_send: Box<dyn FnMut()>,
    /// Starts the test action in the background: mode, sound, and the
    /// fallback flag for `desktop`. The report comes back on its channel.
    test_send: Box<TestSendFn>,
    /// Starts the managed llama-server the menu turned on; the outcome comes
    /// back on its channel. A no-op unless `with_manager` installs one.
    manage: Box<ManageFn>,
    /// Sets how long the managed llama-server may sit unused. A no-op unless
    /// `with_idle` installs one.
    set_idle: Box<IdleFn>,
    /// Asks, in the background, whether the llama-server lazarobox started is
    /// answering another lazarobox; a yes comes back on its channel. A no-op
    /// unless `with_shared_check` installs one.
    check_shared: Box<dyn FnMut()>,
    /// Starts one AI engine detection; the answer arrives on the AI channel.
    /// A no-op unless `with_detector` installs the production one.
    detect: Box<dyn FnMut(bool)>,
    /// Starts one update check now; the answer arrives on the update
    /// channel. A no-op unless `with_update_checker` installs the real one.
    check_update: Box<dyn FnMut()>,
    /// Starts the preparation of a conversation's system prompt: pane,
    /// profile, origin cwd and the context flag. The answer arrives on the
    /// prompt channel. A no-op unless `with_preparer` installs the
    /// production one.
    prepare: Box<PrepareFn>,
    /// The chat turns. `None` ignores the chat effects (unit tests).
    chats: Option<Chats>,
    /// Starts the path checks of runs. `None` refuses every run (unit tests).
    run_checks: Option<run_check::Checker>,
    /// The engram memory worker; off unless the config activates it.
    memory: Memory,
    /// The chat history thread. `None` ignores the store effects (unit tests).
    store: Option<StoreHandle>,
    /// The session of every pane spawned, closed ones included: their jobs
    /// are what the quit prompt names.
    sessions: Vec<jobs::Session>,
    /// Names the jobs of `sessions`; [`jobs::running`] unless a test
    /// installs another one with `with_lister`.
    list_running: Box<ListFn>,
    /// The Hugging Face requests and download. `None` ignores the hub
    /// effects (unit tests).
    hub: Option<hf::Hub>,
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
            bell: false,
            late_bell: Arc::new(AtomicBool::new(false)),
            desktop_send: Box::new(|_, _| {}),
            sound_send: Box::new(|| {}),
            test_send: Box::new(|_, _, _| {}),
            manage: Box::new(|_, _| {}),
            set_idle: Box::new(|_| {}),
            check_shared: Box::new(|| {}),
            detect: Box::new(|_| {}),
            check_update: Box::new(|| {}),
            prepare: Box::new(|_, _, _, _| {}),
            chats: None,
            run_checks: None,
            memory: Memory::off(),
            store: None,
            sessions: Vec::new(),
            list_running: Box::new(jobs::running),
            hub: None,
        }
    }

    /// Sets what names the jobs still running in the panes.
    #[cfg(test)]
    fn with_lister(mut self, list: impl Fn(&[jobs::Session]) -> Vec<jobs::Job> + 'static) -> Self {
        self.list_running = Box::new(list);
        self
    }

    /// Moves a pending bell into the host bytes, after whatever is already
    /// there (an OSC 52 yank). The loop calls it after the draw and right
    /// before `flush_host`, so the BEL leaves in the same write as the other
    /// host bytes, through the writer that draws the frame. One BEL covers all
    /// the bells asked for since the last flush.
    fn stage_bell(&mut self) {
        let late = self.late_bell.swap(false, Ordering::AcqRel);
        if std::mem::take(&mut self.bell) || late {
            self.host_out.push(BEL);
        }
    }

    /// Sets the memory the `Memory` effects go to.
    fn with_memory(mut self, memory: Memory) -> Self {
        self.memory = memory;
        self
    }

    /// [`Self::shutdown_engines_after`] with nothing to wait for first.
    #[cfg(test)]
    async fn shutdown_engines(&mut self, memory_bound: Duration, chats_bound: Duration) {
        self.shutdown_engines_after(async {}, memory_bound, chats_bound)
            .await;
    }

    /// Flushes the memory and stops the chat turns at the same time: the flush
    /// waits on the network, the chat shutdown on local processes, and neither
    /// needs the other. Each keeps its own bound. The chats are stopped only
    /// once `polite` is over (the SIGHUP and SIGTERM of everything lazarobox
    /// started, then its grace), inside `chats_bound` all the same.
    async fn shutdown_engines_after(
        &mut self,
        polite: impl std::future::Future<Output = ()>,
        memory_bound: Duration,
        chats_bound: Duration,
    ) {
        let chats = async {
            // The tokio clock, so a paused test sees the polite stop's time.
            let started = tokio::time::Instant::now();
            let _ = timeout(chats_bound, polite).await;
            if let Some(chats) = self.chats.as_mut() {
                chats
                    .shutdown(chats_bound.saturating_sub(started.elapsed()))
                    .await;
            }
        };
        tokio::join!(self.memory.shutdown(memory_bound), chats);
    }

    /// Sets the task that runs chat turns.
    fn with_chats(mut self, chats: Chats) -> Self {
        self.chats = Some(chats);
        self
    }

    /// Sets what checks the runs of `Effect::CheckRun`.
    fn with_run_checks(mut self, checker: run_check::Checker) -> Self {
        self.run_checks = Some(checker);
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

    /// Sets how `Effect::Notify` starts a desktop notification.
    fn with_desktop(mut self, send: Box<DesktopSendFn>) -> Self {
        self.desktop_send = send;
        self
    }

    /// Sets what plays the notification sound.
    fn with_sound(mut self, sound: Box<dyn FnMut()>) -> Self {
        self.sound_send = sound;
        self
    }

    /// Sets what runs the test action of the Notify row.
    fn with_notify_test(mut self, test: Box<TestSendFn>) -> Self {
        self.test_send = test;
        self
    }

    /// Sets what starts the managed llama-server the menu turns on.
    fn with_manager(mut self, manage: Box<ManageFn>) -> Self {
        self.manage = manage;
        self
    }

    /// Sets what changes the idle time of the managed llama-server.
    fn with_idle(mut self, set_idle: Box<IdleFn>) -> Self {
        self.set_idle = set_idle;
        self
    }

    /// Sets what asks whether the managed llama-server answers another
    /// lazarobox.
    fn with_shared_check(mut self, check_shared: Box<dyn FnMut()>) -> Self {
        self.check_shared = check_shared;
        self
    }

    /// Sets what `Effect::DetectEngines` runs.
    fn with_detector(mut self, detect: Box<dyn FnMut(bool)>) -> Self {
        self.detect = detect;
        self
    }

    /// Sets what runs the Hugging Face effects.
    fn with_hub(mut self, hub: hf::Hub) -> Self {
        self.hub = Some(hub);
        self
    }

    /// Sets what `Effect::CheckUpdate` runs.
    fn with_update_checker(mut self, check: Box<dyn FnMut()>) -> Self {
        self.check_update = check;
        self
    }

    /// Sets what `Effect::PreparePrompt` runs.
    fn with_preparer(mut self, prepare: Box<PrepareFn>) -> Self {
        self.prepare = prepare;
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
        // The shell leads its own session; its start time tells it from a
        // later process given the same pid.
        if let Some(sid) = handle.pid().and_then(|pid| libc::pid_t::try_from(pid).ok())
            && let Some(start) = proc::start_time(sid)
        {
            self.sessions.push(jobs::Session { sid, start });
        }
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
                Effect::DetectEngines => (self.detect)(false),
                Effect::RedetectEngines => (self.detect)(true),
                // Returns at once: the request runs on its own thread.
                Effect::CheckUpdate => (self.check_update)(),
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
                // Both return at once; the answer lands on a waiting call, or
                // on nothing when the call is over.
                Effect::ToolDecision {
                    pane,
                    turn,
                    call,
                    decision,
                } => {
                    if let Some(chats) = self.chats.as_ref() {
                        chats.decide(pane, turn, call, decision);
                    }
                }
                // The write and the answer are one step: the answer is claimed
                // first (a call that is over is owed none, so nothing is
                // typed), and `Sent` is only said after the bytes were handed
                // to a pane at its prompt.
                Effect::AutoType {
                    chat,
                    target,
                    bytes,
                    number,
                    turn,
                    call,
                } => {
                    let answer = self
                        .chats
                        .as_ref()
                        .and_then(|chats| chats.take(chat, turn, call))
                        .filter(|answer| !answer.is_closed());
                    if let Some(answer) = answer {
                        let decision =
                            auto_type::type_if_idle(self.handles.get_mut(&target), number, bytes);
                        let _ = answer.send(decision.into());
                    }
                }
                // The claim and the write are one step, as for `AutoType`, but
                // the pane must also still be in the directory that was
                // checked. The answer is not sent: the claimed wait is parked
                // after the write, so a cancel or a closed chat also ends the
                // running call, and `RunDone` finishes it. A refusal writes
                // nothing and claims nothing; the app is told, and asks the
                // user.
                Effect::AutoRun {
                    chat,
                    target,
                    bytes,
                    turn,
                    call,
                    cwd,
                } => {
                    let chats = self.chats.as_ref();
                    let claim = || {
                        let wait = chats?.take(chat, turn, call);
                        wait.filter(|answer| !answer.is_closed())
                    };
                    let park = |wait| {
                        if let Some(chats) = chats {
                            chats.park(chat, turn, call, wait);
                        }
                    };
                    let pane = self.handles.get_mut(&target);
                    if let Err(reason) = auto_type::run_if_idle(pane, &cwd, bytes, claim, park) {
                        feedback.push(AppEvent::Run(target, RunEvent::NotRun { call, reason }));
                    }
                }
                // The run settled: the parked call ends with its report. A
                // call that is gone (the turn or the chat ended) is not
                // answered.
                Effect::RunDone {
                    chat,
                    turn,
                    call,
                    report,
                } => {
                    if let Some(chats) = self.chats.as_ref() {
                        chats.finish(chat, turn, call, report);
                    }
                }
                // The pane's real directory is read here (the kernel resolves the
                // link, no disk is touched); the paths are checked on the
                // blocking pool and the answer comes back on the run channel.
                // Without a directory or a checker the run is refused, which
                // opens the prompt.
                Effect::CheckRun {
                    target,
                    call,
                    command,
                    ..
                } => {
                    let cwd = self.handles.get(&target).and_then(|pty| pty.cwd());
                    match (cwd, self.run_checks.as_ref()) {
                        (Some(cwd), Some(checks)) => checks.start(target, call, cwd, command),
                        _ => feedback.push(AppEvent::Run(
                            target,
                            RunEvent::Refused {
                                call,
                                reason: approval::NO_CWD,
                            },
                        )),
                    }
                }
                Effect::EndToolSession(pane) => {
                    if let Some(chats) = self.chats.as_mut() {
                        chats.end_session(pane);
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
                // Returns at once: the files are read on their own thread.
                Effect::PreparePrompt {
                    pane,
                    profile,
                    origin,
                    context,
                } => (self.prepare)(pane, profile, origin, context),
                // Only flagged here: the BEL joins the host bytes after the
                // draw (see `stage_bell`), never in the middle of a frame.
                Effect::Bell => self.bell = true,
                // The core table decides; the send returns at once (its own
                // task owns the timeout and the reaping).
                Effect::Notify { mode, body, sound } => {
                    self.bell |= rings_at_once(mode);
                    let fallback =
                        bell_after_failed_desktop(mode).then(|| Arc::clone(&self.late_bell));
                    (self.desktop_send)(body, fallback);
                    if sound {
                        (self.sound_send)();
                    }
                }
                // The key press that asks for it: the bell now (when the mode
                // has one), the rest in the background with a report.
                // Returns at once: the start runs on its own task.
                Effect::StartManaged { model, port } => (self.manage)(model, port),
                Effect::ManagedIdle(idle) => (self.set_idle)(idle),
                Effect::HubAsk(ask) => {
                    if let Some(hub) = &self.hub {
                        hub.ask(ask);
                    }
                }
                Effect::Download(job) => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.start(*job);
                    }
                }
                Effect::CancelDownload => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.cancel();
                    }
                }
                Effect::HfAccount(ask) => {
                    if let Some(hub) = self.hub.as_mut() {
                        hub.account(ask);
                    }
                }
                Effect::NotifyTest { mode, sound } => {
                    self.bell |= rings_at_once(mode);
                    let fallback =
                        bell_after_failed_desktop(mode).then(|| Arc::clone(&self.late_bell));
                    (self.test_send)(mode, sound, fallback);
                }
                // Staged like the other host bytes: written after the draw by
                // `flush_host`, never in the middle of a frame.
                Effect::MouseCapture(on) => {
                    self.host_out
                        .extend_from_slice(if on { CAPTURE_ON } else { CAPTURE_OFF });
                }
                Effect::ListRunning => {
                    (self.check_shared)();
                    let jobs = (self.list_running)(&self.sessions);
                    feedback.push(AppEvent::StillRunning(
                        jobs.into_iter().map(|job| job.command).collect(),
                    ));
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

    /// One reading of each pane in `ids`, whose run is being settled (design
    /// D22): the foreground check and the directory are one syscall and one
    /// link read, with no thread or timer of their own. A pane that is gone
    /// yields none.
    fn probes(&self, ids: &[PaneId]) -> Vec<AppEvent> {
        (ids.iter())
            .filter_map(|id| Some((*id, self.handles.get(id)?)))
            .map(|(id, handle)| {
                let (foreground, cwd) = (handle.shell_in_foreground(), handle.cwd());
                AppEvent::Run(id, RunEvent::Probe { foreground, cwd })
            })
            .collect()
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
        // A terminal without focus reporting never sends these, and the app
        // then keeps the window as focused.
        Event::FocusGained => Some(AppEvent::WindowFocus(true)),
        Event::FocusLost => Some(AppEvent::WindowFocus(false)),
        // Only the vertical wheel: clicks, drags, motion and the horizontal
        // wheel are not the app's.
        Event::Mouse(mouse)
            if matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            ) =>
        {
            Some(AppEvent::Mouse(mouse))
        }
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
/// the alternate screen, which `ratatui::restore` handles. The mouse capture
/// is disabled whether or not it was on: the sequence is harmless when the
/// capture is off, and the panic hook cannot know the state.
fn restore_to<W: Write>(out: &mut W) -> io::Result<()> {
    let mouse = out.write_all(CAPTURE_OFF);
    let rest = execute!(
        out,
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        DisableFocusChange
    );
    mouse.and(rest)
}

/// Enters the terminal modes the runtime needs on top of raw mode: bracketed
/// paste and focus reporting. [`restore_to`] undoes them.
fn enter_modes<W: Write>(out: &mut W) -> io::Result<()> {
    execute!(out, EnableBracketedPaste, EnableFocusChange)
}

fn restore() {
    // Best effort: there is nowhere left to report a failure. Not
    // `ratatui::restore`, which reports one with `eprintln!`: once the
    // terminal is gone (its tab was closed) that write fails and panics, the
    // panic hook restores again and panics inside the panic, and the process
    // aborts before the quit stops the chat servers it started.
    let _ = restore_to(&mut stdout());
    let _ = ratatui::try_restore();
}

/// Whether a panic on this thread should tear the terminal down. A panic
/// inside a chat adapter poll is caught by its turn task and becomes an
/// `Interrupted` turn, and one inside an MCP tool dispatch becomes an error
/// result: the app keeps running, so the terminal must stay intact.
fn should_restore() -> bool {
    // The store thread catches its own panics and reports them as replies.
    let on_store_thread = thread::current().name() == Some(chat_store::worker::THREAD_NAME);
    // The prompt job catches its panic and falls back to the built-in prompt.
    let on_prepare_thread = thread::current().name() == Some(prompts::THREAD_NAME);
    // The notification probe catches its own panic and reports it as failed.
    // An MCP dispatch panic is caught by its session and answered as an error.
    !chat::in_adapter_poll()
        && !on_store_thread
        && !on_prepare_thread
        && !notify::in_probe()
        && !mcp::in_dispatch()
}

/// Restores the cursor, bracketed paste, focus reporting and mouse capture before the previous hook (which
/// `ratatui::init` set up to leave raw mode and the alternate screen) runs.
/// Panics inside a chat adapter or an MCP dispatch are caught elsewhere and skip all of it,
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
        Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
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
        assert_eq!(
            startup_effects(&mut app),
            vec![Effect::MouseCapture(true), Effect::DetectEngines]
        );
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
        let rows = app.menu().unwrap().all_rows(&Config::default());
        let general = rows
            .iter()
            .find_map(|row| match row {
                crate::core::menu::Row::Item {
                    label: "general",
                    value,
                    ..
                } => value.as_deref().map(str::to_owned),
                _ => None,
            })
            .expect("the general row");
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
    #[cfg(target_os = "linux")]
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

    // Spec (chat-notify): Restore disables focus reporting.
    #[test]
    fn restore_disables_focus_reporting() {
        let mut out = Vec::new();
        restore_to(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[?1004l"), "{text:?}");
    }

    // The modes are entered with one write and undone by `restore_to`.
    #[test]
    fn startup_enables_focus_reporting_next_to_bracketed_paste() {
        let mut out = Vec::new();
        enter_modes(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[?2004h"), "{text:?}");
        assert!(text.contains("\x1b[?1004h"), "{text:?}");
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

    // Spec (chat-notify): Focus events map.
    #[test]
    fn focus_events_map_to_the_window_focus() {
        assert_eq!(
            to_app_event(Event::FocusGained),
            Some(AppEvent::WindowFocus(true))
        );
        assert_eq!(
            to_app_event(Event::FocusLost),
            Some(AppEvent::WindowFocus(false))
        );
    }

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 4,
            row: 7,
            modifiers: KeyModifiers::SHIFT,
        }
    }

    // Spec (mouse-input): only the vertical wheel reaches the app.
    #[test]
    fn the_vertical_wheel_is_forwarded_whole() {
        for kind in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown] {
            let event = mouse(kind);
            assert_eq!(
                to_app_event(Event::Mouse(event)),
                Some(AppEvent::Mouse(event)),
                "{kind:?}"
            );
        }
    }

    // Spec (mouse-input): clicks, drags, motion and the horizontal wheel are dropped.
    #[test]
    fn other_mouse_events_are_dropped() {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert_eq!(to_app_event(Event::Mouse(mouse(kind))), None, "{kind:?}");
        }
    }

    #[test]
    fn other_crossterm_events_are_dropped() {
        assert_eq!(
            to_app_event(Event::Mouse(mouse(MouseEventKind::Moved))),
            None
        );
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

    // Spec (mouse-input): the capture sequences are exact, nothing else is enabled.
    #[test]
    fn mouse_capture_writes_exactly_the_wheel_sequences_to_the_host() {
        let (mut panes, _rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        let (outcome, feedback) = panes.apply(vec![Effect::MouseCapture(true)]);
        assert_eq!(outcome, Outcome::Continue);
        assert!(feedback.is_empty());
        assert_eq!(panes.host_out, b"\x1b[?1000h\x1b[?1006h".to_vec());
        panes.host_out.clear();
        panes.apply(vec![Effect::MouseCapture(false)]);
        assert_eq!(panes.host_out, b"\x1b[?1006l\x1b[?1000l".to_vec());
        for log in &rec.borrow().logs {
            let log = log.lock().unwrap();
            assert!(log.writes.is_empty() && log.resizes.is_empty());
        }
    }

    #[test]
    fn mouse_capture_never_enables_motion_or_urxvt_reporting() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(vec![
            Effect::MouseCapture(true),
            Effect::MouseCapture(false),
        ]);
        let text = String::from_utf8(panes.host_out).unwrap();
        for forbidden in ["?1002", "?1003", "?1015"] {
            assert!(!text.contains(forbidden), "{text:?}");
        }
    }

    // Spec (terminal-session): Mouse disabled during restore.
    #[test]
    fn restore_always_disables_the_mouse_capture() {
        let mut out = Vec::new();
        restore_to(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\x1b[?1006l\x1b[?1000l"), "{text:?}");
    }

    // The panic hook restores through `restore_to`, so one check covers both exits.
    #[test]
    fn restore_writes_the_mouse_disable_in_one_piece_before_it_ends() {
        let mut out = Vec::new();
        restore_to(&mut out).unwrap();
        assert!(
            out.windows(CAPTURE_OFF.len()).any(|w| w == CAPTURE_OFF),
            "{out:?}"
        );
        assert_eq!(CAPTURE_OFF, b"\x1b[?1006l\x1b[?1000l");
    }

    // A run with `mouse = false` never writes a capture sequence.
    #[test]
    fn a_config_with_the_mouse_off_starts_without_any_mouse_bytes() {
        let mut app = App::new(80, 24).with_config(&Config {
            mouse: false,
            ..Config::default()
        });
        let effects = startup_effects(&mut app);
        assert_eq!(effects, vec![Effect::DetectEngines]);
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(effects);
        assert!(panes.host_out.is_empty());
    }

    // The start-up capture waits in the host bytes until the modes are entered.
    #[test]
    fn the_startup_capture_is_pending_host_bytes() {
        let mut app = App::new(80, 24);
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(startup_effects(&mut app));
        assert_eq!(panes.host_out, b"\x1b[?1000h\x1b[?1006h".to_vec());
    }

    #[test]
    fn a_wedged_blocking_thread_cannot_hang_the_runtime_shutdown() {
        let started = Instant::now();
        {
            let guard = QuitGuard::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
                Duration::from_millis(200),
            );
            guard.block_on(async {
                tokio::task::spawn_blocking(|| thread::sleep(Duration::from_secs(5)));
                tokio::task::yield_now().await;
            });
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn the_panic_path_is_bounded_too() {
        let started = Instant::now();
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let guard = QuitGuard::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
                Duration::from_millis(200),
            );
            guard.block_on(async {
                tokio::task::spawn_blocking(|| thread::sleep(Duration::from_secs(5)));
                tokio::task::yield_now().await;
            });
            panic!("boom");
        }));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    // chat-notify: Bell is staged for the flush, never written at once.
    #[test]
    fn a_bell_effect_is_flagged_and_writes_nothing_by_itself() {
        let (mut panes, _rx, rec) = fake_panes(None);
        spawn_two(&mut panes);
        let (outcome, feedback) = panes.apply(vec![Effect::Bell]);
        assert_eq!(outcome, Outcome::Continue);
        assert!(feedback.is_empty());
        assert!(panes.host_out.is_empty(), "nothing before the flush");
        for log in &rec.borrow().logs {
            let log = log.lock().unwrap();
            assert!(
                log.writes.is_empty() && log.resizes.is_empty(),
                "no PTY write"
            );
        }
        panes.stage_bell();
        assert_eq!(panes.host_out, vec![0x07]);
    }

    // chat-notify: Coalescing.
    #[test]
    fn several_bells_of_one_tick_are_one_bel() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(vec![Effect::Bell, Effect::Bell]);
        panes.apply(vec![Effect::Bell]);
        panes.stage_bell();
        assert_eq!(panes.host_out, b"\x07".to_vec());
        panes.stage_bell();
        assert_eq!(panes.host_out, b"\x07".to_vec(), "the flag was consumed");
    }

    type Sent = Rc<RefCell<Vec<(String, bool)>>>;

    /// Panes whose desktop sends are recorded (body, whether a failure or an
    /// unavailable desktop would ring the bell).
    fn desktop_panes() -> (Panes, Sent) {
        let (panes, _rx, _rec) = fake_panes(None);
        let sent: Sent = Rc::default();
        let log = Rc::clone(&sent);
        let panes = panes.with_desktop(Box::new(move |body, fallback| {
            log.borrow_mut().push((body, fallback.is_some()))
        }));
        (panes, sent)
    }

    fn notify(mode: NotifyMode) -> Effect {
        Effect::Notify {
            mode,
            body: "Answer ready in pane 2".to_owned(),
            sound: false,
        }
    }

    #[test]
    fn the_sound_starts_only_when_the_effect_asks_for_it() {
        let (panes, _rx, _rec) = fake_panes(None);
        let plays = Rc::new(RefCell::new(0));
        let count = Rc::clone(&plays);
        let mut panes = panes.with_sound(Box::new(move || *count.borrow_mut() += 1));
        panes.apply(vec![notify(NotifyMode::Auto)]);
        assert_eq!(*plays.borrow(), 0);
        panes.apply(vec![Effect::Notify {
            mode: NotifyMode::Auto,
            body: "x".to_owned(),
            sound: true,
        }]);
        assert_eq!(*plays.borrow(), 1);
    }

    #[test]
    fn a_test_effect_rings_at_once_per_mode_and_arms_the_fallback_for_desktop() {
        // (mode, sound, bell now, fallback armed)
        let table = [
            (NotifyMode::Auto, true, true, false),
            (NotifyMode::Desktop, true, false, true),
            (NotifyMode::Bell, false, true, false),
        ];
        for (mode, sound, bell, fallback) in table {
            let (panes, _rx, _rec) = fake_panes(None);
            let seen: Rc<RefCell<Vec<(NotifyMode, bool, bool)>>> = Rc::default();
            let log = Rc::clone(&seen);
            let mut panes = panes.with_notify_test(Box::new(move |mode, sound, fallback| {
                log.borrow_mut().push((mode, sound, fallback.is_some()))
            }));
            panes.apply(vec![Effect::NotifyTest { mode, sound }]);
            panes.stage_bell();
            assert_eq!(panes.host_out == vec![0x07], bell, "{mode:?}");
            assert_eq!(*seen.borrow(), vec![(mode, sound, fallback)], "{mode:?}");
        }
    }

    // Decision table, executed: the availability is probed off the UI thread,
    // so `apply` only rings what is certain and hands the rest to the send.
    #[test]
    fn a_notify_effect_rings_at_once_only_in_auto_and_always_hands_off_the_send() {
        // (mode, bell now, fallback armed)
        let table = [
            (NotifyMode::Auto, true, false),
            (NotifyMode::Desktop, false, true),
        ];
        for (mode, bell, fallback) in table {
            let (mut panes, sent) = desktop_panes();
            panes.apply(vec![notify(mode)]);
            panes.stage_bell();
            assert_eq!(panes.host_out == vec![0x07], bell, "{mode:?}");
            assert_eq!(
                *sent.borrow(),
                vec![("Answer ready in pane 2".to_owned(), fallback)],
                "{mode:?}"
            );
        }
    }

    #[test]
    fn a_failed_desktop_send_rings_on_the_next_flush_only_once() {
        let (mut panes, _sent) = desktop_panes();
        panes.late_bell.store(true, Ordering::Release);
        panes.stage_bell();
        assert_eq!(panes.host_out, vec![0x07]);
        panes.stage_bell();
        assert_eq!(panes.host_out, vec![0x07], "consumed");
    }

    #[test]
    fn the_fallback_flag_handed_to_the_send_is_the_one_stage_bell_reads() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let seen: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::default();
        let slot = Rc::clone(&seen);
        panes = panes.with_desktop(Box::new(move |_, fallback| *slot.borrow_mut() = fallback));
        panes.apply(vec![notify(NotifyMode::Desktop)]);
        seen.borrow()
            .as_ref()
            .unwrap()
            .store(true, Ordering::Release);
        panes.stage_bell();
        assert_eq!(panes.host_out, vec![0x07]);
    }

    #[test]
    fn staging_without_a_bell_adds_nothing() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.stage_bell();
        assert!(panes.host_out.is_empty());
    }

    // chat-notify: Ordered after other host bytes, one write.
    #[test]
    fn the_bel_follows_the_osc_52_bytes_in_a_single_write() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(vec![
            Effect::Bell,
            Effect::SetClipboard(b"\x1b]52;c;aGk=\x07".to_vec()),
        ]);
        panes.stage_bell();
        let mut out = SpyOut::default();
        flush_host(&mut out, &mut panes.host_out).unwrap();
        assert_eq!(out.bytes, b"\x1b]52;c;aGk=\x07\x07".to_vec());
        assert_eq!((out.writes, out.flushes), (1, 1));
        assert!(panes.host_out.is_empty());
    }

    // A bell asked for next to nothing else is the only byte written.
    #[test]
    fn a_lone_bell_flushes_exactly_one_bel() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.apply(vec![Effect::Bell]);
        panes.stage_bell();
        let mut out = SpyOut::default();
        flush_host(&mut out, &mut panes.host_out).unwrap();
        assert_eq!(out.bytes, vec![0x07]);
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

    // Spec S8: a menu save hands the new idle time to the runtime.
    #[test]
    fn a_new_idle_time_reaches_the_runtime() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let set = Arc::new(Mutex::new(Vec::new()));
        let mut panes = Panes::new(tx, Box::new(|_, _| Err(io::Error::other("no spawn"))))
            .with_idle(Box::new({
                let set = Arc::clone(&set);
                move |idle| set.lock().unwrap().push(idle)
            }));
        let thirty = Some(Duration::from_secs(30 * 60));
        let (outcome, feedback) =
            panes.apply(vec![Effect::ManagedIdle(thirty), Effect::ManagedIdle(None)]);
        assert_eq!((outcome, feedback), (Outcome::Continue, vec![]));
        assert_eq!(*set.lock().unwrap(), [thirty, None]);
    }

    // Spec S13 (managed-llama-server-followups): asking to quit also asks
    // whether the managed llama-server answers another lazarobox.
    #[test]
    fn listing_what_runs_also_asks_about_the_shared_llama_server() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut panes = Panes::new(tx, Box::new(|_, _| Err(io::Error::other("no spawn"))))
            .with_lister(|_: &[jobs::Session]| Vec::new())
            .with_shared_check(Box::new({
                let asked = Arc::clone(&asked);
                move || {
                    asked.fetch_add(1, Ordering::SeqCst);
                }
            }));
        let (outcome, feedback) = panes.apply(vec![Effect::ListRunning]);
        assert_eq!(
            (outcome, feedback),
            (Outcome::Continue, vec![AppEvent::StillRunning(Vec::new())])
        );
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn quit_effect_is_reported() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        assert_eq!(panes.apply(vec![Effect::Quit]).0, Outcome::Quit);
    }

    // Spec S7: the quit prompt names the jobs of every pane's session.
    #[test]
    fn listing_what_runs_names_the_jobs_of_the_spawned_panes() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let spawn: SpawnFn = Box::new(|_, _| {
            Ok(Box::new(FakePty {
                pid: Some(std::process::id()),
                ..FakePty::default()
            }))
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut panes = Panes::new(tx, spawn).with_lister({
            let seen = Arc::clone(&seen);
            move |sessions: &[jobs::Session]| {
                seen.lock().unwrap().extend_from_slice(sessions);
                vec![jobs::Job {
                    pgid: 77,
                    command: "sleep 77".into(),
                }]
            }
        });
        panes.apply(vec![Effect::SpawnPane(id(1), spec())]);
        let (outcome, feedback) = panes.apply(vec![Effect::ListRunning]);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(
            feedback,
            vec![AppEvent::StillRunning(vec!["sleep 77".into()])]
        );
        let me = std::process::id() as libc::pid_t;
        assert_eq!(
            *seen.lock().unwrap(),
            [jobs::Session {
                sid: me,
                start: proc::start_time(me).unwrap(),
            }]
        );
    }

    // A pane closed before the quit still had a session: its jobs count.
    #[test]
    fn a_closed_pane_keeps_its_session_listed() {
        let (tx, _rx) = mpsc::channel(PTY_CHANNEL);
        let spawn: SpawnFn = Box::new(|_, _| {
            Ok(Box::new(FakePty {
                pid: Some(std::process::id()),
                ..FakePty::default()
            }))
        });
        let mut panes = Panes::new(tx, spawn);
        panes.apply(vec![
            Effect::SpawnPane(id(1), spec()),
            Effect::ClosePane(id(1)),
        ]);
        assert_eq!(panes.sessions.len(), 1);
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
        let mut panes = panes.with_detector(Box::new(move |_| *seen.borrow_mut() += 1));
        let (outcome, feedback) = panes.apply(vec![Effect::DetectEngines]);
        assert_eq!(
            (outcome, feedback, *calls.borrow()),
            (Outcome::Continue, Vec::new(), 1)
        );
    }

    #[test]
    fn check_update_calls_the_injected_checker_once() {
        let (panes, _rx, _rec) = fake_panes(None);
        let calls = Rc::new(RefCell::new(0));
        let seen = Rc::clone(&calls);
        let mut panes = panes.with_update_checker(Box::new(move || *seen.borrow_mut() += 1));
        let (outcome, feedback) = panes.apply(vec![Effect::CheckUpdate]);
        assert_eq!(
            (outcome, feedback, *calls.borrow()),
            (Outcome::Continue, Vec::new(), 1)
        );
    }

    #[test]
    fn only_a_redetect_tells_the_detector_to_force() {
        let (panes, _rx, _rec) = fake_panes(None);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&calls);
        let mut panes = panes.with_detector(Box::new(move |force| seen.borrow_mut().push(force)));
        panes.apply(vec![Effect::DetectEngines, Effect::RedetectEngines]);
        assert_eq!(*calls.borrow(), [false, true]);
    }

    #[test]
    fn prepare_prompt_calls_the_injected_preparer_with_the_request() {
        let (panes, _rx, _rec) = fake_panes(None);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&calls);
        let mut panes = panes.with_preparer(Box::new(move |pane, profile, origin, context| {
            seen.borrow_mut().push((pane, profile, origin, context));
        }));
        let (outcome, feedback) = panes.apply(vec![Effect::PreparePrompt {
            pane: id(2),
            profile: Profile::Analyst,
            origin: Some("/srv".into()),
            context: false,
        }]);
        assert_eq!((outcome, feedback), (Outcome::Continue, Vec::new()));
        assert_eq!(
            *calls.borrow(),
            [(id(2), Profile::Analyst, Some(PathBuf::from("/srv")), false)]
        );
    }

    #[tokio::test]
    async fn spawn_prepare_answers_on_the_channel_with_the_resolved_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prompts/analyst/chat.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "Be brief.").unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        spawn_prepare(
            Some(dir.path().to_owned()),
            (id(2), Profile::Analyst, None, false),
            tx,
        );
        let (pane, resolved) = rx.recv().await.unwrap();
        assert_eq!(pane, id(2));
        assert_eq!(resolved.text, "Be brief.");
        assert!(resolved.notices.is_empty());
    }

    // Without a chat task (unit tests) the chat effects touch no pane.
    #[test]
    fn chat_effects_are_ignored_without_a_chat_task() {
        use crate::core::ai::chat::engine::{ChatRequest, Tools};
        let (mut panes, _rx, rec) = fake_panes(None);
        let request = ChatRequest {
            families: crate::core::ai::tools::registry::FamilySet::LEGACY,
            engine: "ollama".into(),
            model: Some("llama3:8b".into()),
            system: String::new(),
            memory: None,
            tools: Tools::Off,
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
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            }],
            ..Default::default()
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
            .map(|n| add_slow(&mut panes, n, Duration::from_millis(2000)))
            .collect();
        let started = Instant::now();
        // The shutdown bound sits above the serial total, so it cannot hide a
        // serial regression by cutting it short.
        panes.shutdown(Duration::from_secs(10));
        let elapsed = started.elapsed();
        // Sequential kills would take at least 3 x 2000 ms = 6000 ms; parallel
        // ones take about 2000 ms, which leaves 2.5 s of slack for a slow runner.
        assert!(elapsed < Duration::from_millis(4500), "serial: {elapsed:?}");
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
        // The slow kill blocks for 30 s; any bound well below that proves it.
        assert!(elapsed < Duration::from_secs(10), "unbounded: {elapsed:?}");
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
            extras: vec![],
            first: stored(Role::User, text, MsgStatus::Complete),
            origin: None,
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
                    conversation: Some(conversation),
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

    // ai-chat 10.5: a tag edit goes through the real store thread and file.
    #[tokio::test]
    async fn a_tag_edit_is_stored_in_the_real_file_and_listed() {
        use crate::core::ai::chat::tags::Tags;
        let dir = tempfile::tempdir().unwrap();
        let db = store_db(&dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        panes.apply(vec![Effect::Store(store_create("hi"))]);
        let Some(StoreReply::Created { conversation, .. }) = recv_store(&mut rx).await else {
            panic!("no Created reply");
        };
        let tags = Tags::parse("client-a, sql, SQL, etl");
        panes.apply(vec![Effect::Store(StoreCmd::SetTags {
            pane: id(1),
            conversation: Some(conversation),
            tags,
        })]);
        let reply = recv_store(&mut rx).await;
        let Some(StoreReply::TagsSaved { tags: saved, .. }) = reply else {
            panic!("expected TagsSaved, got {reply:?}");
        };
        assert_eq!(saved, Tags::parse("client-a, sql, etl"));
        panes.apply(vec![Effect::Store(StoreCmd::List {
            pane: id(1),
            origin: None,
        })]);
        let Some(StoreReply::Listed { rows, .. }) = recv_store(&mut rx).await else {
            panic!("no Listed reply");
        };
        assert_eq!(rows[0].tag.as_deref(), Some("client-a"));
        assert_eq!(rows[0].extras, ["etl", "sql"]);
        panes.shutdown_store(STORE_JOIN_BOUND);

        let conn = rusqlite::Connection::open(&db).unwrap();
        let stored: Vec<(String, i64)> = conn
            .prepare("SELECT tag, is_project FROM tags ORDER BY is_project DESC, tag")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            stored,
            vec![
                ("client-a".to_string(), 1),
                ("etl".to_string(), 0),
                ("sql".to_string(), 0),
            ]
        );
    }

    // A tag edit queued right before quit is flushed within the join bound.
    #[tokio::test]
    async fn quit_flushes_a_tag_edit_within_the_join_bound() {
        use crate::core::ai::chat::tags::Tags;
        let dir = tempfile::tempdir().unwrap();
        let db = store_db(&dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        panes.apply(vec![Effect::Store(store_create("q"))]);
        let Some(StoreReply::Created { conversation, .. }) = recv_store(&mut rx).await else {
            panic!("no Created reply");
        };
        panes.apply(vec![Effect::Store(StoreCmd::SetTags {
            pane: id(1),
            conversation: Some(conversation),
            tags: Tags::parse("late"),
        })]);
        let started = Instant::now();
        panes.shutdown_store(STORE_JOIN_BOUND);
        assert!(started.elapsed() < STORE_JOIN_BOUND);
        let conn = rusqlite::Connection::open(&db).unwrap();
        let tag: String = conn
            .query_row("SELECT tag FROM tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag, "late");
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
                conversation: Some(conversation),
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

    // --- Chat history through the real store (ai-chat C9) ---

    use crate::core::ai::{
        EngineState, Status,
        chat::engine::{ChatError, Finish as ChatFinish},
    };

    /// The app as `run` builds it, with a ready ollama engine, a shell pane
    /// started in `work`, and the saved AI choice of `SAVED_AI`.
    fn history_app(dir: &tempfile::TempDir) -> App {
        let config = dir.path().join("config.toml");
        fs::write(&config, SAVED_AI).unwrap();
        let work = dir.path().join("work");
        fs::create_dir_all(&work).unwrap();
        let mut app =
            app_from_config(120, 30, Some(&config), None).with_env(Some("/bin/zsh"), None, work);
        app.update(AppEvent::EnginesDetected(Detection {
            engines: vec![EngineState {
                id: "ollama",
                status: Status::Available,
                models: vec!["llama3:8b".to_owned()],
                version: None,
                model_labels: Vec::new(),
                diagnostics: Default::default(),
            }],
            ..Default::default()
        }));
        app
    }

    fn hkey(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
        AppEvent::Key(KeyEvent::new(code, modifiers))
    }

    /// Hands the store effects among `effects` to the store thread and feeds
    /// the replies they cause back to the app, until the queue is quiet.
    async fn through_store(
        app: &mut App,
        panes: &mut Panes,
        rx: &mut mpsc::Receiver<StoreReply>,
        effects: Vec<Effect>,
        replies: usize,
    ) {
        let store: Vec<Effect> = effects
            .into_iter()
            .filter(|e| matches!(e, Effect::Store(_)))
            .collect();
        panes.apply(store);
        let mut pending = Vec::new();
        for _ in 0..replies {
            let reply = recv_store(rx).await.expect("a store reply");
            pending.extend(app.update(AppEvent::Store(reply)));
        }
        panes.apply(
            pending
                .into_iter()
                .filter(|e| matches!(e, Effect::Store(_)))
                .collect(),
        );
    }

    /// One whole turn in a new conversation, driven through the real store:
    /// open a chat, `n`, send `q`, then `events` of the turn, then Esc when
    /// `cancel`. Returns the database path after the store thread stopped.
    async fn chat_turn(dir: &tempfile::TempDir, events: Vec<StreamEvent>, cancel: bool) -> PathBuf {
        let db = store_db(dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        let mut app = history_app(dir);
        let chat = id(2);

        let mut opened = app.update(hkey(KeyCode::Char(' '), KeyModifiers::CONTROL));
        opened.extend(app.update(hkey(KeyCode::Char('c'), KeyModifiers::NONE)));
        // The list is answered by the real store: empty.
        through_store(&mut app, &mut panes, &mut rx, opened, 1).await;
        app.update(AppEvent::PromptReady(
            chat,
            crate::core::ai::chat::prompts::Resolved {
                text: "system".into(),
                notices: Vec::new(),
            },
        ));
        app.update(hkey(KeyCode::Char('n'), KeyModifiers::NONE));
        app.update(hkey(KeyCode::Char('q'), KeyModifiers::NONE));
        let sent = app.update(hkey(KeyCode::Enter, KeyModifiers::NONE));
        let turn = sent
            .iter()
            .find_map(|e| match e {
                Effect::ChatSend { turn, .. } => Some(*turn),
                _ => None,
            })
            .expect("the message was sent");
        through_store(&mut app, &mut panes, &mut rx, sent, 1).await;
        for event in events {
            let effects = app.update(AppEvent::Chat(chat, turn, event));
            through_store(&mut app, &mut panes, &mut rx, effects, 0).await;
        }
        if cancel {
            let effects = app.update(hkey(KeyCode::Esc, KeyModifiers::NONE));
            through_store(&mut app, &mut panes, &mut rx, effects, 0).await;
        }
        panes.shutdown_store(STORE_JOIN_BOUND);
        db
    }

    /// `(role, content, status)` of every stored message.
    fn stored_messages(db: &Path) -> Vec<(String, String, String)> {
        let conn = rusqlite::Connection::open(db).unwrap();
        let mut stmt = conn
            .prepare("SELECT role, content, status FROM messages ORDER BY conversation_id, seq")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn file_holds(db: &Path, needle: &[u8]) -> bool {
        ["", "-journal", "-wal", "-shm"].iter().any(|suffix| {
            let mut name = db.as_os_str().to_owned();
            name.push(suffix);
            fs::read(PathBuf::from(name))
                .unwrap_or_default()
                .windows(needle.len())
                .any(|w| w == needle)
        })
    }

    #[test]
    fn the_app_from_config_keeps_a_chat_history() {
        let mut app = app_from_config(80, 25, None, None);
        let mut effects = app.update(hkey(KeyCode::Char(' '), KeyModifiers::CONTROL));
        effects.extend(app.update(hkey(KeyCode::Char('c'), KeyModifiers::NONE)));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Store(StoreCmd::List { .. }))),
            "{effects:?}"
        );
    }

    // Spec (Reasoning is not stored): Reasoning is absent from the file.
    #[tokio::test]
    async fn reasoning_is_absent_from_the_real_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = chat_turn(
            &dir,
            vec![
                StreamEvent::ThinkingDelta("secret plan".into()),
                StreamEvent::Delta("Hi".into()),
                StreamEvent::Done(ChatFinish::Stop),
            ],
            false,
        )
        .await;
        assert_eq!(
            stored_messages(&db),
            vec![
                ("user".into(), "q".into(), "complete".into()),
                ("assistant".into(), "Hi".into(), "complete".into()),
            ]
        );
        assert!(!file_holds(&db, b"secret plan"));
        assert!(file_holds(&db, b"Hi"), "the check looks at the right file");
    }

    // Spec: Reasoning only, then cancel, error or cut.
    #[tokio::test]
    async fn a_reasoning_only_turn_stores_no_assistant_row() {
        let thinking = || StreamEvent::ThinkingDelta("secret plan".into());
        let runs: Vec<(Vec<StreamEvent>, bool)> = vec![
            (vec![thinking()], true),
            (
                vec![
                    thinking(),
                    StreamEvent::Error(ChatError::Failed("boom".into())),
                ],
                false,
            ),
            (
                vec![thinking(), StreamEvent::Error(ChatError::Interrupted)],
                false,
            ),
        ];
        for (events, cancel) in runs {
            let dir = tempfile::tempdir().unwrap();
            let db = chat_turn(&dir, events, cancel).await;
            assert_eq!(
                stored_messages(&db),
                vec![("user".into(), "q".into(), "complete".into())]
            );
            assert!(!file_holds(&db, b"secret plan"));
            assert!(!file_holds(&db, b"boom"), "the error text is not stored");
        }
    }

    // Spec: Partial answer keeps no reasoning.
    #[tokio::test]
    async fn a_cancelled_partial_answer_is_stored_without_its_reasoning() {
        let dir = tempfile::tempdir().unwrap();
        let db = chat_turn(
            &dir,
            vec![
                StreamEvent::ThinkingDelta("secret plan".into()),
                StreamEvent::Delta("Hel".into()),
            ],
            true,
        )
        .await;
        assert_eq!(
            stored_messages(&db),
            vec![
                ("user".into(), "q".into(), "complete".into()),
                ("assistant".into(), "Hel".into(), "cancelled".into()),
            ]
        );
        assert!(!file_holds(&db, b"secret plan"));
    }

    /// A turn whose pane goes away before the store answered the create: the
    /// partial answer is appended by pane and lands in the real db.
    async fn closed_before_created(quit: bool) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db = store_db(&dir);
        let (mut panes, mut rx) = panes_with_store(db.clone());
        let mut app = history_app(&dir);
        let mut opened = app.update(hkey(KeyCode::Char(' '), KeyModifiers::CONTROL));
        opened.extend(app.update(hkey(KeyCode::Char('c'), KeyModifiers::NONE)));
        through_store(&mut app, &mut panes, &mut rx, opened, 1).await;
        app.update(AppEvent::PromptReady(
            id(2),
            crate::core::ai::chat::prompts::Resolved {
                text: "system".into(),
                notices: Vec::new(),
            },
        ));
        app.update(hkey(KeyCode::Char('n'), KeyModifiers::NONE));
        app.update(hkey(KeyCode::Char('q'), KeyModifiers::NONE));
        let sent = app.update(hkey(KeyCode::Enter, KeyModifiers::NONE));
        let turn = sent
            .iter()
            .find_map(|e| match e {
                Effect::ChatSend { turn, .. } => Some(*turn),
                _ => None,
            })
            .expect("the message was sent");
        // The create is queued; its reply is never read by the app.
        panes.apply(
            sent.into_iter()
                .filter(|e| matches!(e, Effect::Store(_)))
                .collect(),
        );
        app.update(AppEvent::Chat(
            id(2),
            turn,
            StreamEvent::Delta("par".into()),
        ));
        let keys = ['q', 'y'];
        let mut closing = Vec::new();
        closing.extend(app.update(hkey(KeyCode::Char(' '), KeyModifiers::CONTROL)));
        if !quit {
            closing.extend(app.update(hkey(KeyCode::Char('w'), KeyModifiers::NONE)));
        }
        for c in keys {
            closing.extend(app.update(hkey(KeyCode::Char(c), KeyModifiers::NONE)));
        }
        assert!(
            closing.iter().any(|e| matches!(
                e,
                Effect::Store(StoreCmd::Append {
                    conversation: None,
                    ..
                })
            )),
            "{closing:?}"
        );
        panes.apply(
            closing
                .into_iter()
                .filter(|e| matches!(e, Effect::Store(_)))
                .collect(),
        );
        panes.shutdown_store(STORE_JOIN_BOUND);
        drop(rx);
        (dir, db)
    }

    // Close and quit before `Created` arrives: the answer is in the real db.
    #[tokio::test]
    async fn closing_a_pane_before_created_still_stores_the_partial_answer() {
        let (_dir, db) = closed_before_created(false).await;
        assert_eq!(
            stored_messages(&db),
            vec![
                ("user".into(), "q".into(), "complete".into()),
                ("assistant".into(), "par".into(), "cancelled".into()),
            ]
        );
    }

    #[tokio::test]
    async fn quitting_before_created_still_stores_the_partial_answer() {
        let (_dir, db) = closed_before_created(true).await;
        assert_eq!(stored_messages(&db).len(), 2);
    }

    /// A chat opened in a new session on the database `dir` holds, its list
    /// answered by the real store.
    async fn reopened(dir: &tempfile::TempDir) -> (App, Panes, mpsc::Receiver<StoreReply>) {
        let (mut panes, mut rx) = panes_with_store(store_db(dir));
        let mut app = history_app(dir);
        let mut opened = app.update(hkey(KeyCode::Char(' '), KeyModifiers::CONTROL));
        opened.extend(app.update(hkey(KeyCode::Char('c'), KeyModifiers::NONE)));
        through_store(&mut app, &mut panes, &mut rx, opened, 1).await;
        app.update(AppEvent::PromptReady(
            id(2),
            crate::core::ai::chat::prompts::Resolved {
                text: "system".into(),
                notices: Vec::new(),
            },
        ));
        (app, panes, rx)
    }

    // Spec (Resume): Appends to the same conversation.
    #[tokio::test]
    async fn a_resumed_conversation_gets_its_new_messages_in_the_same_conversation() {
        let dir = tempfile::tempdir().unwrap();
        chat_turn(
            &dir,
            vec![
                StreamEvent::Delta("A1".into()),
                StreamEvent::Done(ChatFinish::Stop),
            ],
            false,
        )
        .await;
        let (mut app, mut panes, mut rx) = reopened(&dir).await;
        // The one stored conversation was opened from the same directory.
        let resume = app.update(hkey(KeyCode::Enter, KeyModifiers::NONE));
        through_store(&mut app, &mut panes, &mut rx, resume, 1).await;
        app.update(hkey(KeyCode::Char('x'), KeyModifiers::NONE));
        let sent = app.update(hkey(KeyCode::Enter, KeyModifiers::NONE));
        let turn = sent
            .iter()
            .find_map(|e| match e {
                Effect::ChatSend { turn, .. } => Some(*turn),
                _ => None,
            })
            .expect("the message was sent");
        through_store(&mut app, &mut panes, &mut rx, sent, 0).await;
        for event in [
            StreamEvent::Delta("A2".into()),
            StreamEvent::Done(ChatFinish::Stop),
        ] {
            let effects = app.update(AppEvent::Chat(id(2), turn, event));
            through_store(&mut app, &mut panes, &mut rx, effects, 0).await;
        }
        panes.shutdown_store(STORE_JOIN_BOUND);
        let db = store_db(&dir);
        let conn = rusqlite::Connection::open(&db).unwrap();
        let conversations: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(conversations, 1, "no new conversation");
        assert_eq!(
            stored_messages(&db)
                .into_iter()
                .map(|(_, text, _)| text)
                .collect::<Vec<_>>(),
            vec!["q", "A1", "x", "A2"]
        );
    }

    // Spec (Delete): the text is gone from the file.
    #[tokio::test]
    async fn a_deleted_conversation_is_gone_from_the_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = chat_turn(
            &dir,
            vec![
                StreamEvent::Delta("ZZ-MARKER-ZZ".into()),
                StreamEvent::Done(ChatFinish::Stop),
            ],
            false,
        )
        .await;
        assert!(file_holds(&db, b"ZZ-MARKER-ZZ"));
        let (mut app, mut panes, mut rx) = reopened(&dir).await;
        app.update(hkey(KeyCode::Char('d'), KeyModifiers::NONE));
        let delete = app.update(hkey(KeyCode::Char('y'), KeyModifiers::NONE));
        through_store(&mut app, &mut panes, &mut rx, delete, 1).await;
        panes.shutdown_store(STORE_JOIN_BOUND);
        assert_eq!(stored_messages(&db), Vec::new());
        assert!(!file_holds(&db, b"ZZ-MARKER-ZZ"));
    }

    // --- Chat effects ---

    use crate::core::ai::chat::engine::{ChatRequest, Decision, StreamEvent, Tools};

    fn chat_request() -> ChatRequest {
        ChatRequest {
            families: crate::core::ai::tools::registry::FamilySet::LEGACY,
            engine: "ollama".into(),
            model: Some("llama3:8b".into()),
            system: String::new(),
            memory: None,
            tools: Tools::Off,
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
            .with_detector(Box::new(move |_| *seen.borrow_mut() += 1));
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

    // The tool effects reach the chats: a turn with tools gets a session, a
    // decision for a call that is not waiting is dropped, and ending the
    // session closes it. None of them touches a PTY.
    #[tokio::test]
    async fn tool_effects_reach_the_chats_and_never_a_pty() {
        use crate::core::ai::chat::engine::{CallId, Decision};
        let dir = tempfile::tempdir().unwrap();
        let host = tools::ToolHost::new(None, None, None, tools::Limits::default());
        let (panes, _pty_rx, rec) = fake_panes(None);
        let (tx, _rx) = mpsc::channel(chat::APP_CHANNEL);
        let runner: chat::Runner = Arc::new(|_request, _tx| Box::pin(std::future::pending()));
        let mut panes =
            panes.with_chats(Chats::new(runner, tx).with_tools(host, vec![dir.path().join("run")]));
        spawn_two(&mut panes);
        let mut request = chat_request();
        request.engine = "claude".into();
        request.tools = Tools::On {
            origin: None,
            external: Default::default(),
            mcp: None,
        };
        panes.apply(vec![Effect::ChatSend {
            pane: id(1),
            turn: 1,
            request,
        }]);
        let session = |panes: &Panes| panes.chats.as_ref().unwrap().session(id(1)).is_some();
        assert!(session(&panes), "a turn with tools has a session");
        panes.apply(vec![Effect::ToolDecision {
            pane: id(1),
            turn: 1,
            call: CallId(99),
            decision: Decision::Sent,
        }]);
        assert!(session(&panes), "a stale decision changes nothing");
        panes.apply(vec![Effect::EndToolSession(id(1))]);
        assert!(!session(&panes));
        let rec = rec.borrow();
        for log in &rec.logs {
            let log = log.lock().unwrap();
            assert!(log.writes.is_empty() && log.resizes.is_empty() && log.kills == 0);
        }
    }

    /// A call of chat 9 turn 1 that waits for its answer, as a model's does.
    fn waiting(panes: &Panes, call: u64) -> futures::future::BoxFuture<'static, Decision> {
        use crate::core::ai::chat::engine::Answer;
        use futures::FutureExt;
        waiting_answer(panes, call)
            .map(|answer| match answer {
                Answer::Decided(decision) => decision,
                Answer::Ran(_) => unreachable!("no run is parked here"),
            })
            .boxed()
    }

    fn waiting_answer(
        panes: &Panes,
        call: u64,
    ) -> futures::future::BoxFuture<'static, crate::core::ai::chat::engine::Answer> {
        use crate::core::ai::chat::{approval::ApprovalAsk, engine::CallId};
        let ask = ApprovalAsk::Read {
            path: String::new(),
        };
        let waits = panes.chats.as_ref().unwrap().waits();
        waits
            .approver(id(9), 1, tools::Pending::new())
            .ask(CallId(call), &ask)
    }

    fn auto_type_effect(target: u32, call: u64) -> Effect {
        Effect::AutoType {
            chat: id(9),
            target: id(target),
            bytes: format!("cmd{target}").into_bytes(),
            number: target as usize,
            turn: 1,
            call: crate::core::ai::chat::engine::CallId(call),
        }
    }

    // The effect writes to the target, not to the chat, and only at a prompt.
    #[tokio::test]
    async fn an_auto_type_writes_only_to_a_pane_whose_shell_is_in_the_foreground() {
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let mut logs = Vec::new();
        let mut waits = Vec::new();
        for (n, foreground) in [(1, Some(true)), (2, Some(false)), (3, None)] {
            let pty = FakePty {
                foreground,
                ..FakePty::default()
            };
            logs.push(pty.log.clone());
            panes.handles.insert(id(n), Box::new(pty));
            waits.push(waiting(&panes, n.into()));
        }
        waits.push(waiting(&panes, 4));
        panes.apply((1..=4).map(|n| auto_type_effect(n, n.into())).collect());
        assert_eq!(logs[0].lock().unwrap().writes, vec![b"cmd1".to_vec()]);
        assert!(logs[1].lock().unwrap().writes.is_empty());
        assert!(logs[2].lock().unwrap().writes.is_empty());
        let mut answers = Vec::new();
        for wait in waits {
            answers.push(wait.await);
        }
        assert_eq!(answers[0], Decision::Typed(1));
        assert_eq!(answers[1], Decision::Busy(2), "a child in the foreground");
        assert_eq!(answers[2], Decision::Busy(3), "an unknown foreground");
        assert!(matches!(answers[3], Decision::Denied(_)), "no such pane");
    }

    // One answer per call, and bytes only when that answer is `Sent`: when
    // the wait is gone before the effect runs, nothing is typed.
    #[tokio::test]
    async fn an_auto_type_whose_call_is_over_writes_nothing() {
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let pty = FakePty {
            foreground: Some(true),
            ..FakePty::default()
        };
        let log = pty.log.clone();
        panes.handles.insert(id(1), Box::new(pty));
        let ended = waiting(&panes, 1);
        let dropped = waiting(&panes, 2);
        panes.chats.as_ref().unwrap().waits().end_turn(id(9), 1);
        drop(dropped);
        panes.apply(vec![auto_type_effect(1, 1), auto_type_effect(1, 2)]);
        assert!(log.lock().unwrap().writes.is_empty(), "no wait, no bytes");
        assert!(matches!(ended.await, Decision::Denied(_)));
    }

    fn check_effect(target: u32, call: u64, command: &str) -> Effect {
        Effect::CheckRun {
            chat: id(9),
            target: id(target),
            turn: 1,
            call: crate::core::ai::chat::engine::CallId(call),
            command: command.to_owned(),
        }
    }

    // S3.8: the cwd is read from the pane, the paths are checked off the loop
    // and the answer comes back on the run channel.
    #[tokio::test]
    async fn a_check_run_reads_the_pane_cwd_and_answers_on_the_run_channel() {
        use crate::app::RunEvent;
        use crate::core::ai::chat::engine::CallId;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap().join("home");
        std::fs::create_dir_all(home.join("dev")).unwrap();
        let env = run_check::process_env();
        let env = crate::runtime::tools::ToolEnv {
            home: Some(home.clone().into()),
            ..env
        };
        let (tx, mut rx) = mpsc::channel(4);
        let (panes, _rx, _rec) = fake_panes(None);
        let mut panes = panes.with_run_checks(run_check::Checker::new(env, tx));
        let pty = FakePty {
            cwd: Some(home.join("dev")),
            ..FakePty::default()
        };
        panes.handles.insert(id(1), Box::new(pty));
        let (_, feedback) = panes.apply(vec![check_effect(1, 7, "ls")]);
        assert_eq!(feedback, Vec::new(), "the answer is not synchronous");
        assert_eq!(
            rx.recv().await,
            Some((
                id(1),
                RunEvent::Checked {
                    call: CallId(7),
                    cwd: home.join("dev"),
                    cd_target: None,
                }
            ))
        );
    }

    // No cwd to check against, or nothing to check with: the prompt, at once.
    #[test]
    fn a_check_run_that_cannot_read_the_cwd_is_refused_at_once() {
        use crate::app::RunEvent;
        use crate::core::ai::chat::engine::CallId;
        let refused = |pane, call| {
            AppEvent::Run(
                id(pane),
                RunEvent::Refused {
                    call: CallId(call),
                    reason: approval::NO_CWD,
                },
            )
        };
        let (mut panes, _rx, _rec) = fake_panes(None);
        panes.handles.insert(id(1), Box::new(FakePty::default()));
        let (_, feedback) = panes.apply(vec![check_effect(1, 7, "ls")]);
        assert_eq!(feedback, vec![refused(1, 7)], "no checker");

        let (tx, _rx) = mpsc::channel(1);
        let (panes, _rx, _rec) = fake_panes(None);
        let mut panes =
            panes.with_run_checks(run_check::Checker::new(run_check::process_env(), tx));
        panes.handles.insert(id(1), Box::new(FakePty::default()));
        let (_, feedback) = panes.apply(vec![check_effect(1, 8, "ls"), check_effect(2, 9, "ls")]);
        assert_eq!(
            feedback,
            vec![refused(1, 8), refused(2, 9)],
            "an unreadable cwd, and a pane that is gone"
        );
    }

    fn auto_run_effect(target: u32, call: u64, cwd: &str) -> Effect {
        Effect::AutoRun {
            chat: id(9),
            target: id(target),
            bytes: format!("cmd{target}\r").into_bytes(),
            turn: 1,
            call: crate::core::ai::chat::engine::CallId(call),
            cwd: cwd.into(),
        }
    }

    // The write step: the pane must still be where the check was done and the
    // shell must own the terminal; otherwise nothing is written, the answer is
    // not claimed, and the app is told so it can ask the user.
    #[tokio::test]
    async fn an_auto_run_writes_only_where_it_was_checked_and_at_a_prompt() {
        use crate::app::RunEvent;
        use crate::core::ai::chat::engine::CallId;
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let mut logs = Vec::new();
        let mut waits = Vec::new();
        let panes_under_test = [
            (1, Some("/srv/a"), Some(true)),
            (2, Some("/srv/other"), Some(true)),
            (3, Some("/srv/a"), Some(false)),
            (4, None, Some(true)),
        ];
        for (n, cwd, foreground) in panes_under_test {
            let pty = FakePty {
                foreground,
                cwd: cwd.map(Into::into),
                ..FakePty::default()
            };
            logs.push(pty.log.clone());
            panes.handles.insert(id(n), Box::new(pty));
            waits.push(waiting_answer(&panes, n.into()));
        }
        let effects = (1..=4)
            .map(|n| auto_run_effect(n, n.into(), "/srv/a"))
            .collect();
        let (_, feedback) = panes.apply(effects);
        assert_eq!(logs[0].lock().unwrap().writes, vec![b"cmd1\r".to_vec()]);
        for log in &logs[1..] {
            assert!(log.lock().unwrap().writes.is_empty());
        }
        let not_run = |pane, call, reason| {
            AppEvent::Run(
                id(pane),
                RunEvent::NotRun {
                    call: CallId(call),
                    reason,
                },
            )
        };
        assert_eq!(
            feedback,
            vec![
                not_run(2, 2, auto_type::MOVED),
                not_run(3, 3, auto_type::BUSY),
                not_run(4, 4, auto_type::MOVED),
            ]
        );
        // The write is not the answer: the claimed wait is parked again, so
        // the report of the run can finish it (see `a_run_done_*`).
        let waits_map = panes.chats.as_ref().unwrap().waits();
        for call in 1..=4 {
            assert!(
                waits_map.take(id(9), 1, CallId(call)).is_some(),
                "call {call}"
            );
        }
    }

    // Only one write per run: a call that is over is owed no bytes.
    #[tokio::test]
    async fn an_auto_run_whose_call_is_over_writes_nothing() {
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let pty = FakePty {
            foreground: Some(true),
            cwd: Some("/srv/a".into()),
            ..FakePty::default()
        };
        let log = pty.log.clone();
        panes.handles.insert(id(1), Box::new(pty));
        let ended = waiting(&panes, 1);
        panes.chats.as_ref().unwrap().waits().end_turn(id(9), 1);
        let (_, feedback) = panes.apply(vec![auto_run_effect(1, 1, "/srv/a")]);
        assert!(log.lock().unwrap().writes.is_empty());
        // The app drops its slot instead of probing a run that never was.
        assert_eq!(
            feedback,
            vec![AppEvent::Run(
                id(1),
                crate::app::RunEvent::NotRun {
                    call: crate::core::ai::chat::engine::CallId(1),
                    reason: crate::app::CALL_OVER,
                }
            )]
        );
        assert!(matches!(ended.await, Decision::Denied(_)));
    }

    // No chat runtime at all: the same.
    #[test]
    fn an_auto_run_without_chats_writes_nothing_and_tells_the_app() {
        let (mut panes, _rx, _rec) = fake_panes(None);
        let pty = FakePty {
            foreground: Some(true),
            cwd: Some("/srv/a".into()),
            ..FakePty::default()
        };
        let log = pty.log.clone();
        panes.handles.insert(id(1), Box::new(pty));
        let (_, feedback) = panes.apply(vec![auto_run_effect(1, 1, "/srv/a")]);
        assert!(log.lock().unwrap().writes.is_empty());
        assert!(matches!(
            feedback[..],
            [AppEvent::Run(_, crate::app::RunEvent::NotRun { .. })]
        ));
    }

    fn run_report() -> Box<crate::core::ai::tools::report::RunReport> {
        use crate::core::ai::tools::{report::RunReport, settle::End};
        Box::new(RunReport {
            number: 1,
            end: End::Settled,
            cwd: None,
            cd_target: None,
            output: None,
        })
    }

    fn run_done(call: u64) -> Effect {
        Effect::RunDone {
            chat: id(9),
            turn: 1,
            call: crate::core::ai::chat::engine::CallId(call),
            report: run_report(),
        }
    }

    // Design D22: the write parks the claimed wait; the report finishes it,
    // once. A second report finds nothing and answers nothing.
    #[tokio::test]
    async fn a_written_run_stays_parked_until_its_report_finishes_it_once() {
        use crate::core::ai::chat::engine::Answer;
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let pty = FakePty {
            foreground: Some(true),
            cwd: Some("/srv/a".into()),
            ..FakePty::default()
        };
        panes.handles.insert(id(1), Box::new(pty));
        let mut wait = waiting_answer(&panes, 1);
        panes.apply(vec![auto_run_effect(1, 1, "/srv/a")]);
        assert!(
            futures::poll!(&mut wait).is_pending(),
            "the write is not the answer"
        );
        panes.apply(vec![run_done(1), run_done(1)]);
        assert_eq!(wait.await, Answer::Ran(run_report()));
    }

    // The turn ended while the run settled: the report finds no wait.
    #[tokio::test]
    async fn a_report_for_a_run_whose_turn_ended_answers_nothing() {
        let (mut panes, _rx) = panes_with_chats(Arc::new(|_, _| Box::pin(std::future::pending())));
        let pty = FakePty {
            foreground: Some(true),
            cwd: Some("/srv/a".into()),
            ..FakePty::default()
        };
        panes.handles.insert(id(1), Box::new(pty));
        let wait = waiting(&panes, 1);
        panes.apply(vec![auto_run_effect(1, 1, "/srv/a")]);
        panes.chats.as_ref().unwrap().waits().end_turn(id(9), 1);
        panes.apply(vec![run_done(1)]);
        assert!(matches!(wait.await, Decision::Denied(_)));
    }

    // Design D22: one probe per named pane, read from the pane's handle; a
    // pane that is gone yields none, and none named yields none.
    #[test]
    fn a_probe_reads_the_foreground_and_cwd_of_the_named_panes_only() {
        use crate::app::RunEvent;
        let (mut panes, _rx, _rec) = fake_panes(None);
        for (n, foreground, cwd) in [(1, Some(true), Some("/srv/a")), (2, None, None)] {
            let pty = FakePty {
                foreground,
                cwd: cwd.map(Into::into),
                ..FakePty::default()
            };
            panes.handles.insert(id(n), Box::new(pty));
        }
        assert_eq!(panes.probes(&[]), Vec::new());
        assert_eq!(
            panes.probes(&[id(1), id(3)]),
            vec![AppEvent::Run(
                id(1),
                RunEvent::Probe {
                    foreground: Some(true),
                    cwd: Some("/srv/a".into()),
                }
            )]
        );
        assert_eq!(
            panes.probes(&[id(2)]),
            vec![AppEvent::Run(
                id(2),
                RunEvent::Probe {
                    foreground: None,
                    cwd: None,
                }
            )]
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

    #[tokio::test]
    async fn a_panic_hook_leaves_the_terminal_alone_inside_an_mcp_dispatch() {
        assert!(should_restore());
        let inside = mcp::scoped(async { should_restore() }).await;
        assert!(!inside, "a dispatch panic is caught and answered");
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

    #[test]
    fn a_panic_on_the_prepare_thread_leaves_the_terminal_alone() {
        let inside = thread::Builder::new()
            .name(prompts::THREAD_NAME.into())
            .spawn(should_restore)
            .unwrap()
            .join()
            .unwrap();
        assert!(!inside, "spawn_prepare catches its own panics");
    }

    // Spec (prompt-profiles): a prepare job that panics still answers, with
    // the built-in prompt, no block and a notice.
    #[tokio::test]
    async fn a_panicking_prepare_job_answers_with_the_built_in_prompt() {
        let (tx, mut rx) = mpsc::channel(1);
        spawn_prepare_with(id(2), Profile::Analyst, Duration::from_secs(5), tx, || {
            panic!("boom")
        });
        let (pane, resolved) = rx.recv().await.unwrap();
        assert_eq!(pane, id(2));
        assert_eq!(
            resolved,
            prompts::fallback_prompt(Profile::Analyst, prompts::FAILED)
        );
        assert!(!resolved.text.contains("Workspace"));
        assert_eq!(resolved.notices.len(), 1);
        assert!(resolved.notices[0].contains("workspace context unavailable"));
    }

    // Spec: a prepare job that does not answer in time is replaced by the
    // built-in prompt, and its late result is dropped.
    #[tokio::test(start_paused = true)]
    async fn a_stuck_prepare_job_times_out_to_the_built_in_prompt() {
        let (tx, mut rx) = mpsc::channel(2);
        let (release, wait) = std::sync::mpsc::channel::<()>();
        spawn_prepare_with(id(2), Profile::Programmer, PREPARE_BOUND, tx, move || {
            let _ = wait.recv();
            Resolved {
                text: "late".into(),
                notices: Vec::new(),
            }
        });
        let (pane, resolved) = rx.recv().await.unwrap();
        assert_eq!(pane, id(2));
        assert_eq!(
            resolved,
            prompts::fallback_prompt(Profile::Programmer, prompts::TIMED_OUT)
        );
        assert!(resolved.notices[0].contains("workspace context unavailable"));
        // The stuck job finishes late: nothing more is sent.
        release.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(rx.try_recv().is_err(), "the late result is dropped");
    }

    #[test]
    fn the_prepare_bound_is_two_seconds() {
        assert_eq!(PREPARE_BOUND, Duration::from_secs(2));
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

    // Spec S6: the turns are stopped only once the polite stop (SIGHUP and
    // SIGTERM, then the grace) is over, and the memory flushes meanwhile.
    #[tokio::test(start_paused = true)]
    async fn quit_stops_the_chats_only_after_the_polite_stop() {
        use std::sync::Mutex;
        use tokio::time::Instant as Clock;

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
                let (_guard, _tx) = (guard, tx);
                std::future::pending::<()>().await;
            })
        });
        let (mut panes, _chat_rx) = panes_with_chats(runner);
        panes.apply(vec![chat_send(id(1), 1)]);
        tokio::task::yield_now().await;

        let polite = Duration::from_millis(500);
        let start = Clock::now();
        panes
            .shutdown_engines_after(
                tokio::time::sleep(polite),
                MEMORY_QUIT_BOUND,
                QUIT_KILL_BOUND,
            )
            .await;
        let dropped = dropped_at.lock().unwrap().expect("the turn was stopped");
        assert_eq!(
            dropped,
            start + polite,
            "the turn was killed before the grace"
        );
    }

    // Spec S6 (verifier B1): a turn still streaming when the app stops
    // listening is not dropped (its group killed) before the polite stop.
    #[tokio::test(start_paused = true)]
    async fn quit_lets_a_streaming_turn_run_until_the_polite_stop_is_over() {
        use std::sync::Mutex;
        use tokio::time::Instant as Clock;

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
                let _guard = guard;
                loop {
                    if tx.send(StreamEvent::Slow).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
        });
        let (mut panes, mut chat_rx) = panes_with_chats(runner);
        panes.apply(vec![chat_send(id(1), 1)]);
        recv(&mut chat_rx).await.unwrap();
        // The loop is gone: nobody reads the turn's events any more.
        drop(chat_rx);

        let polite = Duration::from_millis(500);
        let start = Clock::now();
        panes
            .shutdown_engines_after(
                tokio::time::sleep(polite),
                MEMORY_QUIT_BOUND,
                QUIT_KILL_BOUND,
            )
            .await;
        let dropped = dropped_at.lock().unwrap().expect("the turn was stopped");
        assert_eq!(
            dropped,
            start + polite,
            "the turn was killed before the grace"
        );
    }

    // Memory turned on in the menu during a run has no worker: the pane that
    // asks is told at once, nothing starts and nothing is read or called.
    #[tokio::test]
    async fn a_pane_command_to_an_inactive_memory_is_answered_with_a_restart_notice() {
        use crate::core::ai::{
            config::MemoryConfig,
            memory::port::{MemCmd, MemReply, Reason},
        };
        let (tx, mut rx) = mpsc::channel(8);
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let mut memory = memory_for(
            &MemoryConfig::default(),
            move || {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                None
            },
            || engram::serve::NoServer,
            tx,
        );
        memory.send(MemCmd::Resolve {
            pane: PaneId::FIRST,
            cwd: "/x".into(),
        });
        assert_eq!(
            rx.try_recv().expect("answered at once"),
            MemReply::Unavailable {
                pane: Some(PaneId::FIRST),
                reason: Reason::RestartNeeded,
            }
        );
        memory.shutdown(Duration::from_secs(1)).await;
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // Spec (chat-memory): the token counts as present when set and non-empty.
    #[test]
    fn a_token_counts_as_present_only_when_set_and_not_empty() {
        assert!(token_is_set(Some("tok-123")));
        assert!(!token_is_set(Some("")));
        assert!(!token_is_set(None));
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
        let refused = refused_port::RefusedPort::new();
        let port = refused.port();
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
