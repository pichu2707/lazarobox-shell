//! The tool loop of the Ollama engine. Ollama has no CLI to run tools for it,
//! so lazarobox does: it sends the registry in `tools`, executes the calls of
//! each reply through the turn's [`Dispatch`] (the same host, approvals and
//! events as the MCP session of a CLI engine) and sends the results back, up
//! to [`max_rounds`] requests per turn.
//!
//! Tool messages live in the request bodies of one turn only: the turn's
//! events carry text and tool rows, so the history never sees them.

use std::{panic::AssertUnwindSafe, sync::Arc};

use futures::FutureExt;
use tokio::{sync::mpsc, time::timeout};

use super::TURN_CHANNEL;

use crate::{
    core::ai::{
        chat::{
            ANSWER_CAP, REASONING_CAP,
            adapters::{
                ToolExchange, ToolRound, offered_names, ollama_round_body, openai_round_body,
            },
            engine::{ChatError, ChatRequest, Finish, StreamEvent, Tools},
            leaked_thinking::{LeakedThinking, Piece},
            prompts,
            stream::{
                CallError, OllamaToolDecoder, OpenAiToolDecoder, StreamDecoder, ToolCalls,
                ToolRequest,
            },
            text_call,
        },
        tools::{
            mcp::refusal_text,
            registry::{Family, FamilySet},
            servers::Qualified,
        },
    },
    runtime::{
        loopback::{TOO_LONG, Timeouts, post_stream_with},
        mcp::Dispatch,
    },
};

/// Most requests of one turn. The calls of the last reply are not run: no
/// request would follow to use their results.
pub const MAX_ROUNDS: usize = 8;
/// Most requests of a turn once a coding-tools family is on: a coding turn
/// reads, searches and edits before it answers.
pub const CODING_ROUNDS: usize = 24;
/// The round limit of a turn offering `set`: [`CODING_ROUNDS`] when any coding
/// family is on, else [`MAX_ROUNDS`].
pub fn max_rounds(set: FamilySet) -> usize {
    let coding = [
        Family::Search,
        Family::Write,
        Family::Exec,
        Family::Git,
        Family::GitWrite,
    ];
    if coding.into_iter().any(|family| set.contains(family)) {
        CODING_ROUNDS
    } else {
        MAX_ROUNDS
    }
}
/// The system line of a turn that hit the round limit.
pub const CAP_NOTICE: &str = "tool limit reached for this turn";
/// What a reply with more calls than [`MAX_CALLS`](crate::core::ai::chat::stream::MAX_CALLS)
/// is told about the extra ones.
const TOO_MANY: &str = "too many tool calls in one reply";
/// Most bytes of tool results echoed back over one turn. Past it a result is
/// replaced by [`DROPPED`], so the request bodies stay bounded.
pub const RESULTS_CAP: usize = 256 * 1024;
/// What the model is told in place of a result over [`RESULTS_CAP`].
pub const DROPPED: &str = "result dropped: the tool output limit of this turn was reached";
/// What the model is told when a call panicked.
pub const FAILED: &str = "the tool failed";
/// How a rejected `tools` field shows in the error of an Ollama reply: any 400.
const REJECTED: &str = "HTTP 400";
/// The line a pane gets once when its llama.cpp server refuses `tools` and the
/// turn goes on as text. The chat keeps one copy of a line per pane.
pub const NO_TOOLS_NOTICE: &str =
    "llama-server refused tools (start it with --jinja): answering as plain text";

/// The wire a tool loop speaks: Ollama's own (`/api/chat`, NDJSON) or the
/// OpenAI chat-completions one (`/v1/chat/completions`, SSE) of llama.cpp.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Ollama,
    OpenAi,
}

impl Profile {
    /// The path the requests of this wire go to.
    fn path(self) -> &'static str {
        match self {
            Profile::Ollama => "/api/chat",
            Profile::OpenAi => "/v1/chat/completions",
        }
    }

    /// Whether the error text of a reply says the server refused `tools`.
    /// Ollama answers a plain 400. llama-server answers a 4xx or 5xx whose
    /// message names the `--jinja` flag (recorded: 500 `tools param requires
    /// --jinja flag`); a template exception or any other 4xx/5xx is a real
    /// error. The status prefix keeps an in-stream error from matching.
    pub(super) fn rejected(self, text: &str) -> bool {
        match self {
            Profile::Ollama => text.starts_with(REJECTED),
            Profile::OpenAi => {
                let lower = text.to_lowercase();
                (text.starts_with("HTTP 4") || text.starts_with("HTTP 5"))
                    && lower.contains("--jinja")
            }
        }
    }

    /// The line sent before the retry as text, if this wire says anything.
    pub(super) fn notice(self) -> Option<&'static str> {
        match self {
            Profile::Ollama => None,
            Profile::OpenAi => Some(NO_TOOLS_NOTICE),
        }
    }

    /// The body of one request of this wire.
    fn body(self, request: &ChatRequest, rounds: &[ToolRound], tools: bool) -> Vec<u8> {
        match self {
            Profile::Ollama => ollama_round_body(request, rounds, tools),
            Profile::OpenAi => openai_round_body(request, rounds, tools),
        }
    }
}

tokio::task_local! {
    /// The dispatch of the tool session of the turn this task runs.
    pub static DISPATCH: Arc<dyn Dispatch>;
}

/// The dispatch of the turn the current task runs, if it offers tools.
pub fn current() -> Option<Arc<dyn Dispatch>> {
    DISPATCH.try_with(Arc::clone).ok()
}

/// One turn with tools: rounds of request, reply and tool calls until the
/// model answers in text, the cap is hit or the turn fails. Ends with exactly
/// one `Done` or `Error` unless the receiver is gone. The total limit covers
/// every round together.
pub async fn tool_turn(
    request: &ChatRequest,
    port: u16,
    dispatch: Arc<dyn Dispatch>,
    timeouts: &Timeouts,
    tx: &mpsc::Sender<StreamEvent>,
) {
    tool_turn_with(Profile::Ollama, request, port, dispatch, timeouts, tx).await;
}

/// [`tool_turn`] on the wire of `profile`.
pub async fn tool_turn_with(
    profile: Profile,
    request: &ChatRequest,
    port: u16,
    dispatch: Arc<dyn Dispatch>,
    timeouts: &Timeouts,
    tx: &mpsc::Sender<StreamEvent>,
) {
    let looped = rounds(profile, request, port, dispatch, timeouts, tx);
    if timeout(timeouts.total, looped).await.is_err() {
        let _ = tx
            .send(StreamEvent::Error(ChatError::Failed(TOO_LONG.into())))
            .await;
    }
}

async fn rounds(
    profile: Profile,
    request: &ChatRequest,
    port: u16,
    dispatch: Arc<dyn Dispatch>,
    timeouts: &Timeouts,
    tx: &mpsc::Sender<StreamEvent>,
) {
    // The retry without tools must not describe tools: it keeps the original.
    let plain = request;
    let external = match &request.tools {
        Tools::On { external, .. } => (**external).clone(),
        Tools::Off => Qualified::default(),
    };
    let mut tooled = request.clone();
    tooled.system = prompts::with_tools(&plain.system);
    let mut used = 0;
    // The next text follows earlier text that did not end its line.
    let mut gap = false;
    let mut history: Vec<ToolRound> = Vec::new();
    // Decided per turn: a server without `--jinja` costs one refused request
    // every turn (the notice is deduped by the chat); a per-pane memory is a
    // follow-up.
    let mut offer = true;
    // A tool call written as text gets one reminder a turn (local-model-writes
    // D3); the text is never run.
    let offered = offered_names(&tooled);
    let mut reminded = false;
    let mut requests = 0;
    let limit = max_rounds(request.families);
    while requests < limit && !tx.is_closed() {
        let body = if offer {
            profile.body(&tooled, &history, true)
        } else {
            profile.body(plain, &history, false)
        };
        let path = profile.path();
        // A round after tool results may leak reasoning into its text.
        let hold = !history.is_empty();
        let reply = match profile {
            Profile::Ollama => {
                let decoder = OllamaToolDecoder::server_with(external.clone())
                    .with_families(request.families);
                round(timeouts, port, path, &body, gap, hold, decoder, tx).await
            }
            Profile::OpenAi => {
                let decoder = OpenAiToolDecoder::server_with(external.clone())
                    .with_families(request.families);
                round(timeouts, port, path, &body, gap, hold, decoder, tx).await
            }
        };
        let finish = match reply.end {
            Some(Ok(finish)) => finish,
            // A model that rejects `tools` is asked again as plain text,
            // once, and only before it took any: later it is an error. The
            // failed request produced no event, but llama.cpp says why.
            Some(Err(ChatError::Failed(text)))
                if offer && history.is_empty() && profile.rejected(&text) =>
            {
                offer = false;
                if let Some(line) = profile.notice() {
                    let _ = tx.send(StreamEvent::Notice(line.into())).await;
                }
                continue;
            }
            Some(Err(error)) => {
                let _ = tx.send(StreamEvent::Error(error)).await;
                return;
            }
            // The receiver went away: nobody wants the rest.
            None => return,
        };
        requests += 1;
        // Without tools, whatever calls the reply holds are not run.
        if reply.calls.is_empty() || !offer {
            let written = (offer && !reminded && requests < limit)
                .then(|| text_call::written_call(&reply.text, &offered))
                .flatten();
            let Some(name) = written else {
                let _ = tx.send(StreamEvent::Done(finish)).await;
                return;
            };
            reminded = true;
            let _ = tx
                .send(StreamEvent::Notice(text_call::TEXT_CALL_NOTICE.into()))
                .await;
            if !reply.text.is_empty() {
                gap = !reply.text.ends_with('\n');
            }
            history.push(ToolRound {
                thinking: reply.thinking,
                text: reply.text,
                exchanges: Vec::new(),
                nudge: Some(text_call::reminder(&name)),
            });
            continue;
        }
        if requests == limit {
            break;
        }
        let mut exchanges = Vec::new();
        for call in reply.calls {
            exchanges.push(exchange(call, &*dispatch, &mut used).await);
        }
        if !reply.text.is_empty() {
            gap = !reply.text.ends_with('\n');
        }
        history.push(ToolRound {
            thinking: reply.thinking,
            text: reply.text,
            exchanges,
            nudge: None,
        });
    }
    if !tx.is_closed() {
        let _ = tx.send(StreamEvent::Notice(CAP_NOTICE.into())).await;
        let _ = tx.send(StreamEvent::Done(Finish::Stop)).await;
    }
}

/// Runs one call of a reply, or says why it was refused.
async fn exchange(call: ToolRequest, dispatch: &dyn Dispatch, used: &mut usize) -> ToolExchange {
    let mut result = match call.call {
        // A panic answers that call alone; the turn's task would end it.
        Ok(valid) => AssertUnwindSafe(async { dispatch.call(valid).await })
            .catch_unwind()
            .await
            .map_or_else(|_| FAILED.to_owned(), |outcome| outcome.text),
        Err(CallError::Invalid(error)) => refusal_text(error).to_owned(),
        Err(CallError::TooMany) => TOO_MANY.to_owned(),
    };
    if *used + result.len() > RESULTS_CAP {
        result = DROPPED.to_owned();
    } else {
        *used += result.len();
    }
    ToolExchange {
        name: call.name,
        id: call.id,
        index: call.index,
        arguments: call.arguments,
        result,
    }
}

/// What one request brought back.
struct Reply {
    text: String,
    thinking: Option<String>,
    calls: Vec<ToolRequest>,
    /// How the stream ended; `None` when the receiver went away first.
    end: Option<Result<Finish, ChatError>>,
}

/// One request. Text, reasoning and hints go on to `tx` as they arrive; the
/// end of the stream is held back, because a reply with tool calls is not the
/// end of the turn. `gap` puts a line break before the first text, to part it
/// from the text of the round before. What is kept to echo back is capped.
/// `hold` sorts reasoning leaked into the text ([`LeakedThinking`]).
#[allow(clippy::too_many_arguments)]
async fn round<D: StreamDecoder + ToolCalls + Send>(
    timeouts: &Timeouts,
    port: u16,
    path: &str,
    body: &[u8],
    gap: bool,
    hold: bool,
    mut decoder: D,
    tx: &mpsc::Sender<StreamEvent>,
) -> Reply {
    let (inner, mut events) = mpsc::channel(TURN_CHANNEL);
    let post = async {
        post_stream_with(
            timeouts,
            "127.0.0.1",
            port,
            path,
            body,
            &mut decoder,
            &inner,
        )
        .await;
        // The relay ends when the sender does.
        drop(inner);
    };
    let relay = async {
        let (mut text, mut thinking) = (String::new(), String::new());
        let mut end = None;
        let mut gap = gap;
        let mut split = LeakedThinking::new(hold, ANSWER_CAP);
        let mut sort = |piece: Piece, text: &mut String, thinking: &mut String| match piece {
            Piece::Answer(t) => {
                push_capped(text, &t, ANSWER_CAP);
                let lead = std::mem::take(&mut gap) && !t.is_empty();
                StreamEvent::Delta(if lead { format!("\n{t}") } else { t })
            }
            Piece::Reasoning(t) => {
                push_capped(thinking, &t, REASONING_CAP);
                StreamEvent::ThinkingDelta(t)
            }
        };
        while let Some(event) = events.recv().await {
            let out = match event {
                StreamEvent::Done(finish) => {
                    end = Some(Ok(finish));
                    continue;
                }
                StreamEvent::Error(error) => {
                    end = Some(Err(error));
                    continue;
                }
                StreamEvent::Delta(t) => split.push(&t),
                StreamEvent::ThinkingDelta(t) => vec![Piece::Reasoning(t)],
                other => {
                    if tx.send(other).await.is_err() {
                        return (text, thinking, None);
                    }
                    continue;
                }
            };
            for piece in out {
                let event = sort(piece, &mut text, &mut thinking);
                if tx.send(event).await.is_err() {
                    return (text, thinking, None);
                }
            }
        }
        for piece in split.finish() {
            let event = sort(piece, &mut text, &mut thinking);
            if tx.send(event).await.is_err() {
                return (text, thinking, None);
            }
        }
        (text, thinking, end)
    };
    let ((), (text, thinking, end)) = tokio::join!(post, relay);
    Reply {
        text,
        thinking: (!thinking.is_empty()).then_some(thinking),
        calls: decoder.take_calls(),
        end,
    }
}

/// Appends as much of `part` as fits under `cap` bytes, cut on a character.
fn push_capped(buf: &mut String, part: &str, cap: usize) {
    let mut cut = part.len().min(cap.saturating_sub(buf.len()));
    while !part.is_char_boundary(cut) {
        cut -= 1;
    }
    buf.push_str(&part[..cut]);
}
