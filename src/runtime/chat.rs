//! The chat task: one tokio task per pane turn, between an engine adapter and
//! the app. Adapters are a seam (`Runner`); real ones arrive with the engines.

use std::{
    cell::Cell,
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    FutureExt,
    future::{BoxFuture, join_all},
};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

mod cli;
mod external;
pub mod live;
pub mod managed;

#[cfg(test)]
pub(crate) use cli::own_group_is_private;
pub(crate) use cli::{
    JailNote, alive_by_comm, codex_install, ensure_private_dir, jail_note, own_comm, read_comm,
    run_listing, run_version, trusted_cargo_path, trusted_real_path,
};
pub use live::LiveServers;
pub use managed::{Lease, Managed};

use self::external::{Externals, Start};
use super::{
    loopback::{Timeouts, post_stream},
    mcp::{Dispatch, Hub, Started, external::Slot},
    orchestra::{OrchScope, RunManager},
    tools::{Pending, ToolHost, TurnScope, Waits, pending::TURN},
};
use crate::core::{
    ai::{
        ServerPorts,
        chat::{
            ANSWER_CAP, REASONING_CAP,
            adapters::{Adapter, Invocation, Wire, adapter_for, is_cli_engine},
            claude::ClaudeDecoder,
            codex::CodexDecoder,
            engine::{
                Answer, CallId, ChatError, ChatRequest, Decision, OrchSend, StreamEvent, Tools,
                locality,
            },
            gemini::GeminiDecoder,
            review::{Badge, BatchEvent},
            stream::{ERROR_CAP, NdjsonSplitter, SseParser, StreamDecoder},
        },
        config::McpServer,
        orchestra::{RunId, SessionKey, run::UserChoice},
        sanitize::{StreamSanitizer, Utf8Stream, clean},
        tools::{
            report::RunReport,
            stage::{BatchId, StageKey},
            summary::clip,
            wiring::{McpWire, Secret},
        },
    },
    layout::PaneId,
};
pub use cli::CliEnv;

/// Capacity of the channel between an adapter and its turn task. A full
/// channel only slows the adapter down (and, through the socket, the engine),
/// so no delta is ever dropped. The one best-effort event, `Slow`, is sent
/// with `try_send` before the first byte, when nothing else is queued: it
/// always finds room as long as this is more than 1.
pub const TURN_CHANNEL: usize = 32;
/// Capacity of the channel from every turn task to the event loop: the same
/// as the PTY channel. The loop drains it in bursts, and a full channel holds
/// the turn tasks back instead of losing events.
pub const APP_CHANNEL: usize = 256;
/// Shown when an engine fails with a message that is empty once cleaned.
pub const EMPTY_ERROR: &str = "the engine reported an error";
/// What the placeholder adapter says until the real ones land.
pub const NO_ADAPTER: &str = "no engine adapter is available yet";
/// Why an orchestrator turn ends when its tool session could not open.
pub const NO_SESSION: &str =
    "the orchestrator tools could not start, so this turn cannot orchestrate: press Ctrl+O to chat";

/// An event of a turn, tagged with its pane and turn id.
pub type Tagged = (PaneId, u64, StreamEvent);

/// What an adapter runs: it sends its events to `tx` and ends. Dropping the
/// future cancels the turn: it closes the socket or kills the process group.
///
/// The future MUST stay alive until the adapter's stream ends. Once it
/// completes the turn's receiver is closed: buffered events are delivered in
/// order, anything sent afterwards (say, from a detached task holding a `tx`
/// clone) is discarded, and the turn ends as `Interrupted` unless a final
/// event already arrived. Adapters must not spawn detached tasks that outlive
/// the future.
pub type Runner =
    Arc<dyn Fn(ChatRequest, mpsc::Sender<StreamEvent>) -> BoxFuture<'static, ()> + Send + Sync>;

/// A decoder that has seen no byte yet. Every turn builds its own: a decoder
/// keeps a partial line and an `ended` flag, and neither may leak into the
/// next turn of the pane.
///
/// `wired` is true for a CLI turn that got the tool server; only claude's
/// decoder reads it.
pub fn fresh_decoder(wire: Wire, wired: bool) -> Box<dyn StreamDecoder + Send> {
    match wire {
        Wire::Ndjson => Box::new(NdjsonSplitter::server()),
        Wire::Sse => Box::new(SseParser::server()),
        Wire::ClaudeStreamJson if wired => Box::new(ClaudeDecoder::wired()),
        Wire::ClaudeStreamJson => Box::new(ClaudeDecoder::new()),
        Wire::CodexJsonl if wired => Box::new(CodexDecoder::wired()),
        Wire::CodexJsonl => Box::new(CodexDecoder::new()),
        Wire::GeminiStreamJson if wired => Box::new(GeminiDecoder::wired()),
        Wire::GeminiStreamJson => Box::new(GeminiDecoder::new()),
    }
}

/// One HTTP turn against a local server, with a decoder built for this turn.
pub async fn http_turn(
    wire: Wire,
    port: u16,
    path: &'static str,
    body: Vec<u8>,
    tx: mpsc::Sender<StreamEvent>,
) {
    let mut decoder = fresh_decoder(wire, false);
    post_stream("127.0.0.1", port, path, &body, &mut *decoder, &tx).await;
}

/// What a failure text becomes before the app sees it: sanitized, on one
/// line, trimmed and at most `ERROR_CAP` characters. Transport, stderr and
/// HTTP errors all pass through here, whatever built them.
fn error_text(raw: &str) -> String {
    let flat = clean(raw).split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = flat.chars().take(ERROR_CAP).collect();
    let text = capped.trim_end();
    if text.is_empty() {
        EMPTY_ERROR.to_owned()
    } else {
        text.to_owned()
    }
}

/// Everything between an adapter and `ChatState` for one turn: text is decoded
/// and sanitized by stateful filters (an escape sequence can be split across
/// deltas), the answer is capped, and failure texts are made safe. One pipe
/// per turn.
pub struct Pipe {
    utf8: Utf8Stream,
    sanitizer: StreamSanitizer,
    /// Sanitized bytes forwarded so far.
    sent: usize,
    /// Reasoning has its own filter: an escape sequence left open in it must
    /// not swallow answer text, and the other way round.
    thinking: StreamSanitizer,
    /// Sanitized reasoning bytes forwarded so far.
    thought: usize,
    /// Answer text went out: later reasoning is dropped here.
    answering: bool,
    over: bool,
}

impl Pipe {
    pub fn new() -> Self {
        Self {
            utf8: Utf8Stream::new(),
            sanitizer: StreamSanitizer::new(),
            sent: 0,
            thinking: StreamSanitizer::new(),
            thought: 0,
            answering: false,
            over: false,
        }
    }

    /// What reaches the app for one event of the adapter. Once a final event
    /// went out, nothing does.
    pub fn event(&mut self, event: StreamEvent) -> Vec<StreamEvent> {
        if self.over {
            return Vec::new();
        }
        match event {
            StreamEvent::Slow => vec![StreamEvent::Slow],
            StreamEvent::Notice(text) => {
                let line = clean(&text)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if line.is_empty() {
                    Vec::new()
                } else {
                    vec![StreamEvent::Notice(line)]
                }
            }
            // Made by the host that ran the call, never decoded from text.
            // The texts still pass the output sanitizer: a path or a command
            // in them is data.
            StreamEvent::ToolCall {
                id,
                name,
                summary,
                approval,
            } => vec![StreamEvent::ToolCall {
                id,
                name: clip(&name),
                summary: clip(&summary),
                approval,
            }],
            StreamEvent::ToolResult {
                id,
                status,
                summary,
            } => vec![StreamEvent::ToolResult {
                id,
                status,
                summary: clip(&summary),
            }],
            StreamEvent::Delta(text) => self.bytes(text.as_bytes()),
            StreamEvent::ThinkingDelta(text) => self.reasoning(&text),
            StreamEvent::Usage(usage) => vec![StreamEvent::Usage(usage)],
            StreamEvent::Done(finish) => self.end(StreamEvent::Done(finish)),
            StreamEvent::Error(ChatError::Failed(text)) => {
                self.end(StreamEvent::Error(ChatError::Failed(error_text(&text))))
            }
            StreamEvent::Error(error) => self.end(StreamEvent::Error(error)),
        }
    }

    /// Forwards reasoning text, sanitized with its own state and cut at
    /// `REASONING_CAP` without an error. Once the answer began, reasoning is
    /// dropped.
    fn reasoning(&mut self, text: &str) -> Vec<StreamEvent> {
        let clean = self.thinking.push(text);
        if self.answering || clean.is_empty() {
            return Vec::new();
        }
        let mut cut = clean.len().min(REASONING_CAP - self.thought);
        while !clean.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            return Vec::new();
        }
        self.thought += cut;
        vec![StreamEvent::ThinkingDelta(clean[..cut].to_owned())]
    }

    /// A final event went out: the turn is over.
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// Raw output of an engine: a character may be cut between two calls.
    pub fn bytes(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        if self.over {
            return Vec::new();
        }
        let text = self.utf8.push(bytes);
        self.text(&text)
    }

    /// The adapter ended without a final event.
    pub fn end_without_final(&mut self) -> Vec<StreamEvent> {
        if self.over {
            return Vec::new();
        }
        self.end(StreamEvent::Error(ChatError::Interrupted))
    }

    /// Forwards decoded text, cutting the answer at `ANSWER_CAP`.
    fn text(&mut self, decoded: &str) -> Vec<StreamEvent> {
        let text = self.sanitizer.push(decoded);
        if text.is_empty() {
            return Vec::new();
        }
        let room = ANSWER_CAP - self.sent;
        if text.len() <= room {
            self.sent += text.len();
            self.answering = true;
            return vec![StreamEvent::Delta(text)];
        }
        let mut cut = room;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        self.sent += cut;
        self.over = true;
        let mut out = Vec::new();
        if cut > 0 {
            out.push(StreamEvent::Delta(text[..cut].to_owned()));
        }
        out.push(StreamEvent::Error(ChatError::Truncated));
        out
    }

    /// Flushes what the filters still hold, then ends with `last`.
    fn end(&mut self, last: StreamEvent) -> Vec<StreamEvent> {
        let tail = self.utf8.finish();
        let mut out = self.text(&tail);
        self.sanitizer.finish();
        self.thinking.finish();
        if !self.over {
            self.over = true;
            out.push(last);
        }
        out
    }
}

impl Default for Pipe {
    fn default() -> Self {
        Self::new()
    }
}

/// A runner with the port of server engines overridden, so tests can
/// talk to a fake server on an ephemeral port. The host stays `127.0.0.1`.
#[cfg(test)]
pub(crate) fn adapter_runner_at(port: Option<u16>) -> Runner {
    adapter_runner_with(port.map_or_else(ServerPorts::default, ServerPorts::every))
}

/// [`adapter_runner_with`] with the live server state its turns read.
#[cfg(test)]
pub(crate) fn adapter_runner_live(live: LiveServers) -> Runner {
    runner_live(
        live,
        CliEnv::from_env(crate::core::ai::jail::JailSupport::Unavailable(
            crate::core::ai::jail::JailOff::NotProbed,
        )),
        None,
    )
}

/// [`adapter_runner_at`] with one port per server engine.
#[cfg(test)]
pub(crate) fn adapter_runner_with(ports: ServerPorts) -> Runner {
    runner_with_ports(
        ports,
        CliEnv::from_env(crate::core::ai::jail::JailSupport::Unavailable(
            crate::core::ai::jail::JailOff::NotProbed,
        )),
        None,
    )
}

/// The runner over an explicit machine: the port of server engines (when
/// overridden), the environment CLI engines run in and the host for the direct
/// tool loop of a server engine (unused: that loop runs on the dispatch of the
/// turn). The tool session of a turn is not made here but by [`Chats::send`],
/// which knows the pane and turn; a turn without `Tools::On` never touches
/// either, so its requests stay as they were without tools.
pub fn runner_with(port: Option<u16>, cli: CliEnv, tools: Option<Arc<ToolHost>>) -> Runner {
    runner_with_ports(
        port.map_or_else(ServerPorts::default, ServerPorts::every),
        cli,
        tools,
    )
}

/// [`runner_with`] with the port of each server engine given: a turn goes to
/// the port its engine was detected on (`[ai.servers.<id>] port`, else the
/// catalog port).
pub fn runner_with_ports(ports: ServerPorts, cli: CliEnv, tools: Option<Arc<ToolHost>>) -> Runner {
    runner_managed(ports, None, cli, tools)
}

/// [`runner_with_ports`] with the llama-server lazarobox manages, if any: a
/// llamacpp turn starts it first, and fails with its reason when it cannot.
pub fn runner_managed(
    ports: ServerPorts,
    managed: Option<Arc<Managed>>,
    cli: CliEnv,
    tools: Option<Arc<ToolHost>>,
) -> Runner {
    runner_live(LiveServers::new(ports, managed), cli, tools)
}

/// [`runner_managed`] reading the ports and the managed llama-server when
/// each turn starts, so a server the menu turns on is used at once.
pub fn runner_live(live: LiveServers, cli: CliEnv, tools: Option<Arc<ToolHost>>) -> Runner {
    Arc::new(move |request, tx| {
        let cli = cli.clone();
        let tools = tools.clone();
        let port = live.ports().port(&request.engine);
        let managed = live.managed().filter(|_| request.engine == "llamacpp");
        Box::pin(async move {
            let _tools = tools;
            // Held until the turn ends: the idle stop never cuts it.
            let lease = match managed {
                Some(managed) => match managed.lease().await {
                    Ok(lease) => Some(lease),
                    Err(reason) => {
                        let _ = tx.send(StreamEvent::Error(ChatError::Failed(reason))).await;
                        return;
                    }
                },
                None => None,
            };
            let Some(adapter) = adapter_for(&request.engine) else {
                let _ = tx
                    .send(StreamEvent::Error(ChatError::Failed(NO_ADAPTER.into())))
                    .await;
                return;
            };
            match adapter.build(&request) {
                Invocation::Http {
                    wire,
                    port: catalog_port,
                    path,
                    body,
                } => {
                    // The managed server's port is known once it runs.
                    let port = lease
                        .as_ref()
                        .map(Lease::port)
                        .or(port)
                        .unwrap_or(catalog_port);
                    // Only a turn that offers tools has a dispatch to loop on.
                    match (adapter, &request.tools, ollama::current()) {
                        (Adapter::Ollama, Tools::On { .. }, Some(dispatch)) => {
                            ollama::tool_turn(&request, port, dispatch, &Timeouts::SERVER, &tx)
                                .await
                        }
                        (Adapter::Llamacpp, Tools::On { .. }, Some(dispatch)) => {
                            let profile = ollama::Profile::OpenAi;
                            ollama::tool_turn_with(
                                profile,
                                &request,
                                port,
                                dispatch,
                                &Timeouts::SERVER,
                                &tx,
                            )
                            .await
                        }
                        _ => http_turn(wire, port, path, body, tx).await,
                    }
                }
                Invocation::Process(job) => cli::process_turn(&cli, job, &tx).await,
                Invocation::Refused(reason) => {
                    let _ = tx
                        .send(StreamEvent::Error(ChatError::Failed(reason.into())))
                        .await;
                }
            }
        })
    })
}

thread_local! {
    /// How many adapter polls are running on this thread right now.
    static IN_ADAPTER: Cell<usize> = const { Cell::new(0) };
}

/// Whether the current thread is inside a poll of a chat adapter future. The
/// panic hook reads it: a panic there is caught by `run_turn` and becomes
/// `Interrupted`, so it must not tear the terminal down.
pub fn in_adapter_poll() -> bool {
    IN_ADAPTER.with(|depth| depth.get() > 0)
}

/// Counts one adapter poll in; the drop counts it out, also on unwind.
struct PollGuard;

impl PollGuard {
    fn enter() -> Self {
        IN_ADAPTER.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for PollGuard {
    fn drop(&mut self) {
        IN_ADAPTER.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Wraps an adapter future so `in_adapter_poll` is true during each of its
/// polls. Set per poll, not per task: a task can move between threads between
/// polls, and the panic hook runs on the thread that panics.
///
/// Known gaps, accepted for now:
/// - A double panic (the adapter panics, then a `Drop` panics during unwind)
///   aborts the process with the terminal left raw: the hook skipped the
///   restore for both.
/// - A panic in an adapter's `Drop` during `Chats::cancel` or `shutdown`
///   happens with the future dropped outside a poll, so it runs the full hook.
/// - Panics in tasks the adapter spawns run the full hook too.
/// - A caught adapter panic is silent; logging its message is planned for C11.
pub fn scoped<F: Future>(inner: F) -> Scoped<F> {
    Scoped(Box::pin(inner))
}

pub struct Scoped<F>(Pin<Box<F>>);

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let _guard = PollGuard::enter();
        self.0.as_mut().poll(cx)
    }
}

/// Denies the calls of a turn that still wait for the user when the turn's
/// task ends, however it ends: finished, failed, timed out (the 15-minute
/// total) or aborted by a cancel or a newer turn. A timer never approves.
struct EndTurn {
    waits: Arc<Waits>,
    pane: PaneId,
    turn: u64,
}

impl Drop for EndTurn {
    fn drop(&mut self) {
        self.waits.end_turn(self.pane, self.turn);
    }
}

/// The adapter future of a turn inside the task-locals of its tool session:
/// the dispatch a server engine runs its tool loop on, and the count of calls
/// waiting for the user that the adapter's timers read. A worker turn of the
/// run manager is wrapped the same way.
pub(crate) fn in_session(
    adapter: BoxFuture<'static, ()>,
    pending: Option<Pending>,
    dispatch: Option<Arc<dyn Dispatch>>,
) -> BoxFuture<'static, ()> {
    let adapter = match dispatch {
        Some(dispatch) => Box::pin(ollama::DISPATCH.scope(dispatch, adapter)),
        None => adapter,
    };
    match pending {
        Some(pending) => Box::pin(TURN.scope(pending, adapter)),
        None => adapter,
    }
}

/// One turn: runs the adapter, filters its events and forwards them tagged.
/// The adapter future lives in this task, so dropping the task drops it.
/// `events` is the turn's channel (the sender the adapter gets is a clone, and
/// a tool session may hold others); `pending` is set when the turn offers
/// tools, so the adapter's timers can see the calls that wait for the user.
async fn run_turn(
    runner: Runner,
    app: mpsc::Sender<Tagged>,
    turn: Turn,
    request: ChatRequest,
    channel: (mpsc::Sender<StreamEvent>, mpsc::Receiver<StreamEvent>),
    pending: Option<Pending>,
    dispatch: Option<Arc<dyn Dispatch>>,
) {
    let Turn {
        pane,
        turn,
        waits,
        external,
    } = turn;
    let mut request = request;
    let _end = EndTurn { waits, pane, turn };
    let (tx, mut rx) = channel;
    // A panicking adapter must still end the turn: its sender is dropped and
    // the pane hears `Interrupted` instead of waiting forever.
    // The call itself sits inside the guarded future: a runner that panics
    // before it returns its future is caught the same way.
    let adapter = async move {
        // The user's servers first: the engine gets their tools, and the
        // notices go out before its first byte.
        if let Some(start) = external {
            start.run(pane, &mut request.tools, &tx).await;
        }
        runner(request, tx).await
    };
    let adapter = in_session(Box::pin(adapter), pending, dispatch);
    let run = AssertUnwindSafe(scoped(adapter)).catch_unwind().fuse();
    tokio::pin!(run);
    let mut running = true;
    let mut listening = true;
    let mut pipe = Pipe::new();
    loop {
        let event = tokio::select! {
            _ = &mut run, if running => {
                running = false;
                // The adapter is done (or panicked). Closing the receiver
                // keeps what is already buffered and then yields `None`, so a
                // sender clone that outlives the adapter cannot hold the turn
                // open.
                rx.close();
                continue;
            }
            event = rx.recv() => event,
        };
        let out = match event {
            Some(event) => pipe.event(event),
            None => pipe.end_without_final(),
        };
        let ended = pipe.is_over();
        // Once the app stops listening (lazarobox is quitting), the events
        // are dropped but the turn keeps running: returning would drop the
        // adapter and SIGKILL its group before the polite stop is over. Only
        // `Chats::shutdown` (or the engine ending) stops it then.
        for event in out {
            if listening && app.send((pane, turn, event)).await.is_err() {
                listening = false;
            }
        }
        if ended {
            return;
        }
    }
}

/// Which turn a task runs, and where its calls wait for the user.
struct Turn {
    pane: PaneId,
    turn: u64,
    waits: Arc<Waits>,
    /// Starts the chat's user servers ahead of the engine; `None` without any.
    external: Option<Start>,
}

/// What a chat needs to offer tools: the host that runs the calls and the hub
/// of per-chat sockets a CLI engine reaches them through. The hub is made when
/// the first turn with tools starts, so an app that never offers tools never
/// touches the folder.
struct Tooling {
    host: Arc<ToolHost>,
    /// The folders to try for the sockets, best first.
    dirs: Vec<PathBuf>,
    hub: Option<Hub>,
    /// The session of each pane that has one, for the wiring of its engine.
    started: HashMap<PaneId, Started>,
}

/// Whether a turn's engine will get its tools wired (see [`wires_tools`]).
pub type Wires = Arc<dyn Fn(&ChatRequest) -> bool + Send + Sync>;

/// Whether the tools of `request` reach its engine: a server engine (Ollama,
/// llama.cpp) runs them in-process; a CLI only when it will run jailed. The user's
/// servers are started only for such a turn: for a bare CLI the wiring is
/// dropped, so a server would run, and be announced, for nothing.
// The llamacpp arm is inert until `MCP_VERIFIED` lists it (PR3).
pub fn wires_tools(cli: &CliEnv, request: &ChatRequest) -> bool {
    match adapter_for(&request.engine) {
        Some(Adapter::Ollama | Adapter::Llamacpp) => true,
        Some(adapter) => match adapter.build(request) {
            Invocation::Process(job) => cli::runs_jailed(cli, &job),
            Invocation::Http { .. } | Invocation::Refused(_) => false,
        },
        None => false,
    }
}

/// [`wires_tools`] over the machine `cli`.
pub fn wires_for(cli: CliEnv) -> Wires {
    Arc::new(move |request| wires_tools(&cli, request))
}

/// A turn's tool session: what its adapter timers see, the dispatch of a
/// server engine (Ollama), and where its user servers will be put.
/// A turn's session: its pending asks, the dispatch of a server engine, the
/// slot of its user servers, and the run an orchestrator turn started.
type Opened = (Pending, Option<Arc<dyn Dispatch>>, Slot, Option<RunId>);

/// The running turns, by pane. Each turn is one task that owns its adapter
/// future: aborting the task drops the future, which closes the socket or
/// kills the process group, so a cancel needs no cooperation from the adapter.
///
/// A turn whose request has `Tools::On` also gets a tool session (when tooling
/// is set): a socket and a dispatch bound to the turn's event channel. The
/// calls of that session that need the user wait in `waits`, keyed by (pane,
/// turn, call); [`Chats::decide`] answers them.
pub struct Chats {
    runner: Runner,
    tx: mpsc::Sender<Tagged>,
    turns: HashMap<PaneId, JoinHandle<()>>,
    waits: Arc<Waits>,
    tooling: Option<Tooling>,
    /// The user's MCP servers, started per chat for a turn with tools on.
    externals: Externals,
    /// The servers of chats that closed, being stopped.
    retiring: Vec<JoinHandle<()>>,
    /// The llama-server lazarobox started for llamacpp turns, stopped on quit.
    live: LiveServers,
    /// The runs of orchestrating panes; `None` leaves every chat path as it
    /// was before orchestration.
    orchestra: Option<Arc<RunManager>>,
}

impl Chats {
    pub fn new(runner: Runner, tx: mpsc::Sender<Tagged>) -> Self {
        Self {
            runner,
            tx,
            turns: HashMap::new(),
            waits: Waits::new(),
            tooling: None,
            externals: Externals::default(),
            retiring: Vec::new(),
            live: LiveServers::default(),
            orchestra: None,
        }
    }

    /// The live server state the runner's turns read: quitting stops the
    /// llama-server it manages then.
    pub fn with_live(mut self, live: LiveServers) -> Self {
        self.live = live;
        self
    }

    /// The user's MCP servers (`[[ai.mcp.servers]]`, read once at startup),
    /// started with `home` as their working directory. Only a turn with tools
    /// on starts them.
    pub fn with_servers(mut self, servers: Vec<McpServer>, home: PathBuf, wires: Wires) -> Self {
        self.externals = Externals::new(servers, home, wires);
        self
    }

    /// Lets the panes of this app run orchestrations on `orchestra` (D1):
    /// Ctrl+C on a pane also ends its run, and quitting ends every run.
    pub fn with_orchestra(mut self, orchestra: Arc<RunManager>) -> Self {
        self.orchestra = Some(orchestra);
        self
    }

    /// Lets turns that ask for tools run them through `host`, with session
    /// sockets in the first of `dirs` that is a private folder of ours.
    pub fn with_tools(mut self, host: Arc<ToolHost>, dirs: Vec<PathBuf>) -> Self {
        self.tooling = Some(Tooling {
            host,
            dirs,
            hub: None,
            started: HashMap::new(),
        });
        self
    }

    /// Starts a background text-only run (a compaction) for `pane`. Unlike
    /// [`Chats::send`] it is not a turn of the chat: it ends no wait, stops no
    /// server and replaces no tool session, so a user's tool call or MCP
    /// server of the pane survives it. A later `send` aborts it.
    pub fn send_background(&mut self, pane: PaneId, turn: u64, request: ChatRequest) {
        self.turns.retain(|_, task| !task.is_finished());
        if let Some(old) = self.turns.remove(&pane) {
            old.abort();
        }
        let channel = mpsc::channel(TURN_CHANNEL);
        let task = tokio::spawn(run_turn(
            self.runner.clone(),
            self.tx.clone(),
            Turn {
                pane,
                turn,
                waits: self.waits.clone(),
                external: None,
            },
            request,
            channel,
            None,
            None,
        ));
        self.turns.insert(pane, task);
    }

    /// Starts turn `turn` of `pane`, replacing any turn it still had.
    pub fn send(&mut self, pane: PaneId, turn: u64, request: ChatRequest) {
        self.start_turn(pane, turn, request, None);
    }

    /// Starts orchestrator turn `turn` of `pane` (agent-orchestrator D5): a
    /// turn like [`Chats::send`] whose session also reaches the run it
    /// starts, which replaces any run of the pane. Nothing is asked: the
    /// run's read scope is the chat origin, when the path policy allows it.
    /// Without a run manager it runs as a plain turn; when the run cannot
    /// start (quitting) the turn ends at once with the reason.
    pub fn send_orchestrated(
        &mut self,
        pane: PaneId,
        turn: u64,
        request: ChatRequest,
        orch: OrchSend,
    ) {
        self.start_turn(pane, turn, request, Some(orch));
    }

    fn start_turn(
        &mut self,
        pane: PaneId,
        turn: u64,
        mut request: ChatRequest,
        orch: Option<OrchSend>,
    ) {
        self.turns.retain(|_, task| !task.is_finished());
        if let Some(old) = self.turns.remove(&pane) {
            old.abort();
        }
        // At once, not when the aborted task is dropped: an answer for the
        // old turn must find nothing from here on.
        // A continuation keeps its workers' waits: the run goes on.
        if orch.as_ref().is_none_or(|orch| orch.attach.is_none()) {
            self.waits.end_pane(pane);
        }
        // A chat whose turn has no tools no longer uses its servers.
        if request.tools == Tools::Off {
            self.stop_servers(pane);
        }
        let channel = mpsc::channel(TURN_CHANNEL);
        let opened = match self.open_session(pane, turn, &request, &channel.0, orch) {
            Ok(opened) => opened,
            Err(reason) => {
                let app = self.tx.clone();
                let error = StreamEvent::Error(ChatError::Failed(reason));
                tokio::spawn(async move { app.send((pane, turn, error)).await });
                return;
            }
        };
        let (pending, dispatch, external, run) = match opened {
            Some((pending, dispatch, slot, run)) => (
                Some(pending),
                dispatch,
                self.externals.start_for(slot, &request),
                run,
            ),
            None => (None, None, None, None),
        };
        // The run of an orchestrator turn hears when the turn is over: it then
        // ends by itself once it has nothing left to do. An aborted turn says
        // nothing: Ctrl+C ended its run, a new turn replaced it.
        let ended = run.zip(self.orchestra.clone());
        // Tell the engine's adapter which session is this turn's. Without a
        // session the request keeps `mcp: None` and the turn runs as text.
        if let (Tools::On { mcp, .. }, Some(started), Some(_)) =
            (&mut request.tools, self.session(pane), &pending)
        {
            *mcp = Some(Box::new(McpWire {
                socket: started.socket.clone(),
                token: Secret::new(started.token.reveal()),
                unwire: started.end.clone(),
            }));
        }
        let turn = run_turn(
            self.runner.clone(),
            self.tx.clone(),
            Turn {
                pane,
                turn,
                waits: self.waits.clone(),
                external,
            },
            request,
            channel,
            pending,
            dispatch,
        );
        let task = tokio::spawn(async move {
            turn.await;
            if let Some((run, manager)) = ended {
                manager.turn_ended(pane, run);
            }
        });
        self.turns.insert(pane, task);
    }

    /// The session of this turn, if it offers tools: any earlier session of
    /// the pane is replaced (its token stops working), or closed when this
    /// turn has no tools. `None` when the turn runs without a session. An
    /// orchestrator turn (`orch`) starts its run here, its handle in the
    /// session's scope; `Err` is why the run could not start.
    ///
    /// A server engine (Ollama) gets no socket: its dispatch comes back for
    /// the turn's tool loop, which reaches the same host and approvals.
    fn open_session(
        &mut self,
        pane: PaneId,
        turn: u64,
        request: &ChatRequest,
        events: &mpsc::Sender<StreamEvent>,
        orch: Option<OrchSend>,
    ) -> Result<Option<Opened>, String> {
        let Some(tooling) = self.tooling.as_mut() else {
            return Ok(None);
        };
        tooling.started.remove(&pane);
        let (Tools::On { origin, .. }, Some(locality)) =
            (&request.tools, locality(&request.engine))
        else {
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            return Ok(None);
        };
        let pending = Pending::new();
        let slot = Slot::default();
        // A chat of unknown origin reads relative paths against `/`, which
        // the path policy then refuses: the ask shows `/name`.
        let origin = origin.clone().unwrap_or_else(|| PathBuf::from("/"));
        let approver = self.waits.approver(pane, turn, pending.clone());
        let orchestra = match (orch, &self.orchestra) {
            (Some(orch), Some(manager)) => {
                let scope = OrchScope {
                    roster: orch.roster,
                    origin: origin.clone(),
                    // The run's reads run unasked in the chat origin, which
                    // must pass the path policy; nothing is asked (D6). An
                    // origin that is not UTF-8 gives no root.
                    roots: origin.to_str().map(str::to_owned).into_iter().collect(),
                    goal: orch.goal,
                    deadline: orch.deadline,
                    limits: orch.limits,
                    profile: orch.profile,
                    events: events.clone(),
                    approver: approver.clone(),
                    pending: pending.clone(),
                };
                let opened = match orch.attach {
                    Some(run) => manager.attach(pane, run, scope),
                    None => manager.start(pane, scope),
                };
                match opened {
                    Ok((run, handle)) => Some((run, handle)),
                    Err(reason) => {
                        // The pane's earlier session must not stay open.
                        if let Some(hub) = tooling.hub.as_mut() {
                            hub.close(pane);
                        }
                        return Err(reason);
                    }
                }
            }
            _ => None,
        };
        let (run, orchestra) = orchestra.unzip();
        let orchestrated = orchestra.is_some();
        let scope = TurnScope {
            events: events.clone(),
            locality,
            origin,
            approver,
            // Filled by the turn's task once its servers run.
            external: slot.clone(),
            // What the session lists is what the host accepts (D7).
            families: request.families,
            stage_key: Some(StageKey::Pane(pane)),
            allowed: Default::default(),
            // Only an orchestrator turn reaches a run; the read scope is the
            // workers'.
            orchestra,
            read_scope: None,
            cli: is_cli_engine(&request.engine),
        };
        let dispatch = tooling.host.dispatch(scope);
        if matches!(
            adapter_for(&request.engine),
            Some(Adapter::Ollama | Adapter::Llamacpp)
        ) {
            // The pane may have had a CLI session: it must not stay open.
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            return Ok(Some((pending, Some(dispatch), slot, run)));
        }
        if tooling.hub.is_none() {
            tooling.hub = Hub::new(&tooling.dirs).ok();
        }
        // No session means no tools for this turn: a chat turn runs as text,
        // an orchestrator turn cannot orchestrate, so its run ends with it.
        let started = tooling.hub.as_mut().and_then(|hub| {
            hub.open(pane, dispatch, slot.clone(), request.families)
                .ok()
        });
        let Some(started) = started else {
            if orchestrated && let Some(manager) = &self.orchestra {
                manager.cancel(pane);
                return Err(NO_SESSION.to_owned());
            }
            return Ok(None);
        };
        tooling.started.insert(pane, started);
        Ok(Some((pending, None, slot, run)))
    }

    /// The user's answer to a call that waits: the app's `ToolDecision`.
    /// `false` when no call waits under that (pane, turn, call), say because
    /// it ended meanwhile; the answer is then dropped.
    pub fn decide(
        &self,
        session: impl Into<SessionKey>,
        turn: u64,
        call: CallId,
        decision: Decision,
    ) -> bool {
        self.waits.decide(session, turn, call, decision)
    }

    /// Claims the answer of a call that waits (see [`Waits::take`]).
    pub fn take(
        &self,
        pane: PaneId,
        turn: u64,
        call: CallId,
    ) -> Option<tokio::sync::oneshot::Sender<Answer>> {
        self.waits.take(pane, turn, call)
    }

    /// Puts a claimed wait back once its command was written (see
    /// [`Waits::park`]).
    pub fn park(
        &self,
        pane: PaneId,
        turn: u64,
        call: CallId,
        wait: tokio::sync::oneshot::Sender<Answer>,
    ) -> bool {
        self.waits.park(pane, turn, call, wait)
    }

    /// Ends a parked call with the report of its run (see [`Waits::finish`]).
    pub fn finish(&self, pane: PaneId, turn: u64, call: CallId, report: Box<RunReport>) -> bool {
        self.waits.finish(pane, turn, call, report)
    }

    /// The waits, so a test can register one without a model turn.
    #[cfg(test)]
    pub fn waits(&self) -> &Arc<Waits> {
        &self.waits
    }

    /// Ends the tool session of `pane`: its socket goes and every call it
    /// holds, answered or not, is denied.
    pub fn end_session(&mut self, pane: PaneId) {
        self.waits.end_pane(pane);
        self.stop_servers(pane);
        if let Some(tooling) = self.tooling.as_mut() {
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            tooling.started.remove(&pane);
        }
    }

    /// Stops the user's servers of `pane`, if it has any, in the background:
    /// quitting waits for it (`shutdown`), and the drop of the set is the
    /// backstop that kills what a stop misses.
    fn stop_servers(&mut self, pane: PaneId) {
        self.retiring.retain(|task| !task.is_finished());
        if let Some(set) = self.externals.retire(pane) {
            self.retiring
                .push(tokio::spawn(async move { set.shutdown().await }));
        }
    }

    /// The session of `pane`, while it has one.
    pub fn session(&self, pane: PaneId) -> Option<&Started> {
        self.tooling.as_ref()?.started.get(&pane)
    }

    /// Discards the batch `pane` holds because the chat closed: how many
    /// entries were lost, `None` when there was nothing worth a notice. Unlike
    /// [`Chats::cancel`], which leaves a batch as an orphan for the user.
    pub fn close_batch(&mut self, pane: PaneId) -> Option<usize> {
        self.tooling
            .as_ref()?
            .host
            .close_batch(StageKey::Pane(pane))
    }

    /// The batch a turn of `pane` left behind (cancelled, timed out, or never
    /// proposed), sealed for the user to review. `None` while a turn runs,
    /// because then the batch is the model's.
    pub fn review_batch(&self, pane: PaneId) -> Option<BatchEvent> {
        let live = self.turns.get(&pane).is_some_and(|t| !t.is_finished());
        let host = &self.tooling.as_ref()?.host;
        let (id, review) = host.review_batch(StageKey::Pane(pane), live)?;
        Some(BatchEvent::Review(id, review))
    }

    /// What each chat's pane shows of its batch: staged while a turn of the
    /// pane runs, an orphan once none does. Empty without tooling, or while
    /// the store is busy (the next poll shows it).
    pub fn batch_badges(&self) -> Option<HashMap<PaneId, Badge>> {
        let tooling = self.tooling.as_ref()?;
        let live = |pane| self.turns.get(&pane).is_some_and(|t| !t.is_finished());
        tooling.host.badges(live)
    }

    /// Applies an orphan the user approved; the host refuses it while a turn
    /// of the pane runs, as it does a review.
    pub fn apply_batch(&self, id: BatchId) -> Option<BatchEvent> {
        let StageKey::Pane(pane) = id.key;
        let live = self.turns.get(&pane).is_some_and(|t| !t.is_finished());
        Some(self.tooling.as_ref()?.host.apply_batch(id, live))
    }

    /// Discards an orphan the user rejected.
    pub fn discard_batch(&self, id: BatchId) -> Option<BatchEvent> {
        Some(self.tooling.as_ref()?.host.discard_batch(id))
    }

    /// Stops the turn of `pane`, if any. Returns at once.
    /// Stops the background run of `pane` and nothing else (see
    /// [`Chats::send_background`]).
    pub fn cancel_background(&mut self, pane: PaneId) {
        if let Some(task) = self.turns.remove(&pane) {
            task.abort();
        }
    }

    /// Ctrl+C: stops the turn of `pane` and ends its run, if any. Returns at
    /// once. The run goes first, so its workers are stopped by the time the
    /// pane's waits, theirs included, are denied.
    pub fn cancel(&mut self, pane: PaneId) {
        if let Some(orchestra) = &self.orchestra {
            orchestra.cancel(pane);
        }
        if let Some(task) = self.turns.remove(&pane) {
            task.abort();
        }
        self.waits.end_pane(pane);
    }

    /// Whether a run manager is attached: without one no answer can be applied.
    pub fn orchestrates(&self) -> bool {
        self.orchestra.is_some()
    }

    /// The user's answer to the failure that halted run `run` of `pane`
    /// (`RunManager::decide`). `false` when it was not applied (the run
    /// ended or was replaced, no failure waits, or no run manager is
    /// attached): nothing changes.
    pub fn decide_run(&self, pane: PaneId, run: RunId, choice: UserChoice) -> bool {
        self.orchestra
            .as_ref()
            .is_some_and(|orchestra| orchestra.decide(pane, run, choice).is_ok())
    }

    /// Cancels worker `worker` of run `run` of `pane` (the cancel tool's
    /// path, `RunManager::cancel_worker`); a run or a worker that is gone is
    /// a no-op, its rows having said so.
    pub fn cancel_worker(
        &self,
        pane: PaneId,
        run: RunId,
        worker: crate::core::ai::orchestra::WorkerId,
    ) {
        if let Some(orchestra) = &self.orchestra {
            orchestra.cancel_worker(pane, run, worker);
        }
    }

    /// Stops every turn and waits, up to `bound`, until each adapter has been
    /// dropped (its sockets closed, its process groups killed).
    pub async fn shutdown(&mut self, bound: Duration) {
        let tasks: Vec<_> = self.turns.drain().map(|(_, task)| task).collect();
        for task in &tasks {
            task.abort();
        }
        let sets = self.externals.drain();
        let stops = join_all(sets.iter().map(|set| set.shutdown()));
        let retiring = join_all(self.retiring.drain(..));
        let managed = self.live.managed();
        // Every run ends now, whatever the bound; its future waits for the
        // worker turns to be gone.
        let runs = self
            .orchestra
            .as_ref()
            .map(|orchestra| orchestra.shutdown());
        // After the turns: an aborted start drops its server on its own.
        let turns = async move {
            join_all(tasks).await;
            if let Some(managed) = managed {
                managed.shutdown().await;
            }
        };
        let runs = async move {
            if let Some(runs) = runs {
                runs.await;
            }
        };
        let _ = timeout(bound, async { tokio::join!(turns, stops, retiring, runs) }).await;
    }
}

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod external_tests;
#[cfg(test)]
mod llamacpp_tests;
mod ollama;
#[cfg(test)]
mod ollama_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;
