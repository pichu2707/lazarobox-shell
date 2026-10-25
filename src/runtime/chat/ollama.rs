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
            adapters::{ToolExchange, ToolRound, ollama_round_body},
            engine::{ChatError, ChatRequest, Finish, StreamEvent, Tools},
            prompts,
            stream::{CallError, OllamaToolDecoder, ToolRequest},
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
/// How a rejected `tools` field shows in the error of a reply: any 400.
const REJECTED: &str = "HTTP 400";

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
    let looped = rounds(request, port, dispatch, timeouts, tx);
    if timeout(timeouts.total, looped).await.is_err() {
        let _ = tx
            .send(StreamEvent::Error(ChatError::Failed(TOO_LONG.into())))
            .await;
    }
}

async fn rounds(
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
    let mut offer = true;
    let mut requests = 0;
    let limit = max_rounds(request.families);
    while requests < limit && !tx.is_closed() {
        let body = if offer {
            ollama_round_body(&tooled, &history, true)
        } else {
            ollama_round_body(plain, &history, false)
        };
        let reply = round(timeouts, port, &body, gap, &external, request.families, tx).await;
        let finish = match reply.end {
            Some(Ok(finish)) => finish,
            // A model that rejects `tools` is asked again as plain text,
            // once, and only before it took any: later a 400 is an error. The
            // failed request produced no event.
            Some(Err(ChatError::Failed(text)))
                if offer && history.is_empty() && text.starts_with(REJECTED) =>
            {
                offer = false;
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
            let _ = tx.send(StreamEvent::Done(finish)).await;
            return;
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
async fn round(
    timeouts: &Timeouts,
    port: u16,
    body: &[u8],
    gap: bool,
    external: &Qualified,
    families: FamilySet,
    tx: &mpsc::Sender<StreamEvent>,
) -> Reply {
    let (inner, mut events) = mpsc::channel(TURN_CHANNEL);
    let mut decoder = OllamaToolDecoder::server_with(external.clone()).with_families(families);
    let post = async {
        post_stream_with(
            timeouts,
            "127.0.0.1",
            port,
            "/api/chat",
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
        while let Some(event) = events.recv().await {
            match event {
                StreamEvent::Done(finish) => end = Some(Ok(finish)),
                StreamEvent::Error(error) => end = Some(Err(error)),
                other => {
                    let other = match other {
                        StreamEvent::Delta(t) => {
                            push_capped(&mut text, &t, ANSWER_CAP);
                            let lead = std::mem::take(&mut gap) && !t.is_empty();
                            StreamEvent::Delta(if lead { format!("\n{t}") } else { t })
                        }
                        StreamEvent::ThinkingDelta(t) => {
                            push_capped(&mut thinking, &t, REASONING_CAP);
                            StreamEvent::ThinkingDelta(t)
                        }
                        other => other,
                    };
                    if tx.send(other).await.is_err() {
                        return (text, thinking, None);
                    }
                }
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
