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
pub(crate) use cli::{
    alive_by_comm, ensure_private_dir, jail_note, read_comm, run_listing, trusted_cargo_path,
    trusted_real_path,
};

use super::{
    loopback::{Timeouts, post_stream},
    mcp::{Dispatch, Hub, Started},
    tools::{Pending, ToolHost, TurnScope, Waits, pending::TURN},
};
use crate::core::{
    ai::{
        chat::{
            ANSWER_CAP, REASONING_CAP,
            adapters::{Adapter, Invocation, Wire, adapter_for},
            claude::ClaudeDecoder,
            codex::CodexDecoder,
            engine::{
                Answer, CallId, ChatError, ChatRequest, Decision, StreamEvent, Tools, locality,
            },
            gemini::GeminiDecoder,
            stream::{ERROR_CAP, NdjsonSplitter, SseParser, StreamDecoder},
        },
        sanitize::{StreamSanitizer, Utf8Stream, clean},
        tools::{
            registry,
            report::RunReport,
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
    runner_with(
        port,
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
    Arc::new(move |request, tx| {
        let cli = cli.clone();
        let tools = tools.clone();
        Box::pin(async move {
            let _tools = tools;
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
                    let port = port.unwrap_or(catalog_port);
                    // Only a turn that offers tools has a dispatch to loop on.
                    match (adapter, &request.tools, ollama::current()) {
                        (Adapter::Ollama, Tools::On { .. }, Some(dispatch)) => {
                            ollama::tool_turn(&request, port, dispatch, &Timeouts::SERVER, &tx)
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
    let Turn { pane, turn, waits } = turn;
    let _end = EndTurn { waits, pane, turn };
    let (tx, mut rx) = channel;
    // A panicking adapter must still end the turn: its sender is dropped and
    // the pane hears `Interrupted` instead of waiting forever.
    // The call itself sits inside the guarded future: a runner that panics
    // before it returns its future is caught the same way.
    let adapter = async move { runner(request, tx).await };
    // A server engine runs its tool loop in the adapter, on this dispatch.
    let adapter: BoxFuture<'static, ()> = match dispatch {
        Some(dispatch) => Box::pin(ollama::DISPATCH.scope(dispatch, adapter)),
        None => Box::pin(adapter),
    };
    let adapter: BoxFuture<'static, ()> = match pending {
        Some(pending) => Box::pin(TURN.scope(pending, adapter)),
        None => Box::pin(adapter),
    };
    let run = AssertUnwindSafe(scoped(adapter)).catch_unwind().fuse();
    tokio::pin!(run);
    let mut running = true;
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
        for event in out {
            if app.send((pane, turn, event)).await.is_err() {
                return;
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
}

impl Chats {
    pub fn new(runner: Runner, tx: mpsc::Sender<Tagged>) -> Self {
        Self {
            runner,
            tx,
            turns: HashMap::new(),
            waits: Waits::new(),
            tooling: None,
        }
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

    /// Starts turn `turn` of `pane`, replacing any turn it still had.
    pub fn send(&mut self, pane: PaneId, turn: u64, mut request: ChatRequest) {
        self.turns.retain(|_, task| !task.is_finished());
        if let Some(old) = self.turns.remove(&pane) {
            old.abort();
        }
        // At once, not when the aborted task is dropped: an answer for the
        // old turn must find nothing from here on.
        self.waits.end_pane(pane);
        let channel = mpsc::channel(TURN_CHANNEL);
        let (pending, dispatch) = match self.open_session(pane, turn, &request, &channel.0) {
            Some((pending, dispatch)) => (Some(pending), dispatch),
            None => (None, None),
        };
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
        let task = tokio::spawn(run_turn(
            self.runner.clone(),
            self.tx.clone(),
            Turn {
                pane,
                turn,
                waits: self.waits.clone(),
            },
            request,
            channel,
            pending,
            dispatch,
        ));
        self.turns.insert(pane, task);
    }

    /// The session of this turn, if it offers tools: any earlier session of
    /// the pane is replaced (its token stops working), or closed when this
    /// turn has no tools. `None` when the turn runs without a session.
    ///
    /// A server engine (Ollama) gets no socket: its dispatch comes back for
    /// the turn's tool loop, which reaches the same host and approvals.
    fn open_session(
        &mut self,
        pane: PaneId,
        turn: u64,
        request: &ChatRequest,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Option<(Pending, Option<Arc<dyn Dispatch>>)> {
        let tooling = self.tooling.as_mut()?;
        tooling.started.remove(&pane);
        let (Tools::On { origin, .. }, Some(locality)) =
            (&request.tools, locality(&request.engine))
        else {
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            return None;
        };
        let pending = Pending::new();
        let scope = TurnScope {
            events: events.clone(),
            locality,
            // A chat of unknown origin reads relative paths against `/`,
            // which the path policy then refuses: the ask shows `/name`.
            origin: origin.clone().unwrap_or_else(|| PathBuf::from("/")),
            approver: self.waits.approver(pane, turn, pending.clone()),
        };
        let dispatch = tooling.host.dispatch(scope);
        if adapter_for(&request.engine) == Some(Adapter::Ollama) {
            // The pane may have had a CLI session: it must not stay open.
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            return Some((pending, Some(dispatch)));
        }
        if tooling.hub.is_none() {
            tooling.hub = Hub::new(&tooling.dirs).ok();
        }
        let hub = tooling.hub.as_mut()?;
        // No session means no tools for this turn: it runs as text.
        let started = hub.open(pane, registry(&[]), dispatch).ok()?;
        tooling.started.insert(pane, started);
        Some((pending, None))
    }

    /// The user's answer to a call that waits: the app's `ToolDecision`.
    /// `false` when no call waits under that (pane, turn, call), say because
    /// it ended meanwhile; the answer is then dropped.
    pub fn decide(&self, pane: PaneId, turn: u64, call: CallId, decision: Decision) -> bool {
        self.waits.decide(pane, turn, call, decision)
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
        if let Some(tooling) = self.tooling.as_mut() {
            if let Some(hub) = tooling.hub.as_mut() {
                hub.close(pane);
            }
            tooling.started.remove(&pane);
        }
    }

    /// The session of `pane`, while it has one.
    pub fn session(&self, pane: PaneId) -> Option<&Started> {
        self.tooling.as_ref()?.started.get(&pane)
    }

    /// Stops the turn of `pane`, if any. Returns at once.
    pub fn cancel(&mut self, pane: PaneId) {
        if let Some(task) = self.turns.remove(&pane) {
            task.abort();
        }
        self.waits.end_pane(pane);
    }

    /// Stops every turn and waits, up to `bound`, until each adapter has been
    /// dropped (its sockets closed, its process groups killed).
    pub async fn shutdown(&mut self, bound: Duration) {
        let tasks: Vec<_> = self.turns.drain().map(|(_, task)| task).collect();
        for task in &tasks {
            task.abort();
        }
        let _ = timeout(bound, join_all(tasks)).await;
    }
}

#[cfg(test)]
mod adapter_tests;
mod ollama;
#[cfg(test)]
mod ollama_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_tests;
