//! The chat task: one tokio task per pane turn, between an engine adapter and
//! the app. Adapters are a seam (`Runner`); real ones arrive with the engines.

use std::{
    cell::Cell,
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
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
    alive_by_comm, ensure_private_dir, jail_note, read_comm, run_listing, trusted_real_path,
};

use super::{loopback::post_stream, tools::ToolHost};
use crate::core::{
    ai::{
        chat::{
            ANSWER_CAP, REASONING_CAP,
            adapters::{Invocation, Wire, adapter_for},
            claude::ClaudeDecoder,
            codex::CodexDecoder,
            engine::{ChatError, ChatRequest, StreamEvent},
            gemini::GeminiDecoder,
            stream::{ERROR_CAP, NdjsonSplitter, SseParser, StreamDecoder},
        },
        sanitize::{StreamSanitizer, Utf8Stream, clean},
        tools::summary::clip,
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
pub fn fresh_decoder(wire: Wire) -> Box<dyn StreamDecoder + Send> {
    match wire {
        Wire::Ndjson => Box::new(NdjsonSplitter::server()),
        Wire::Sse => Box::new(SseParser::server()),
        Wire::ClaudeStreamJson => Box::new(ClaudeDecoder::new()),
        Wire::CodexJsonl => Box::new(CodexDecoder::new()),
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
    let mut decoder = fresh_decoder(wire);
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
/// overridden), the environment CLI engines run in and the host that runs the
/// tool calls of a turn. A turn that offers tools builds its
/// [`Dispatch`](crate::runtime::mcp::Dispatch) from the host with
/// `ToolHost::dispatch`; a turn that does not (every turn today) never
/// touches it, so the requests stay as they were without tools.
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
                } => http_turn(wire, port.unwrap_or(catalog_port), path, body, tx).await,
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

/// One turn: runs the adapter, filters its events and forwards them tagged.
/// The adapter future lives in this task, so dropping the task drops it.
async fn run_turn(
    runner: Runner,
    app: mpsc::Sender<Tagged>,
    pane: PaneId,
    turn: u64,
    request: ChatRequest,
) {
    let (tx, mut rx) = mpsc::channel(TURN_CHANNEL);
    // A panicking adapter must still end the turn: its sender is dropped and
    // the pane hears `Interrupted` instead of waiting forever.
    // The call itself sits inside the guarded future: a runner that panics
    // before it returns its future is caught the same way.
    let run = AssertUnwindSafe(scoped(async move { runner(request, tx).await }))
        .catch_unwind()
        .fuse();
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

/// The running turns, by pane. Each turn is one task that owns its adapter
/// future: aborting the task drops the future, which closes the socket or
/// kills the process group, so a cancel needs no cooperation from the adapter.
pub struct Chats {
    runner: Runner,
    tx: mpsc::Sender<Tagged>,
    turns: HashMap<PaneId, JoinHandle<()>>,
}

impl Chats {
    pub fn new(runner: Runner, tx: mpsc::Sender<Tagged>) -> Self {
        Self {
            runner,
            tx,
            turns: HashMap::new(),
        }
    }

    /// Starts turn `turn` of `pane`, replacing any turn it still had.
    pub fn send(&mut self, pane: PaneId, turn: u64, request: ChatRequest) {
        self.turns.retain(|_, task| !task.is_finished());
        if let Some(old) = self.turns.remove(&pane) {
            old.abort();
        }
        let task = tokio::spawn(run_turn(
            self.runner.clone(),
            self.tx.clone(),
            pane,
            turn,
            request,
        ));
        self.turns.insert(pane, task);
    }

    /// Stops the turn of `pane`, if any. Returns at once.
    pub fn cancel(&mut self, pane: PaneId) {
        if let Some(task) = self.turns.remove(&pane) {
            task.abort();
        }
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
#[cfg(test)]
mod tests;
