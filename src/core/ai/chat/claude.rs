//! The decoder of `claude -p --output-format stream-json` output: one JSON
//! event per line. Pure: bytes in, `StreamEvent`s out, whatever the chunking.
//!
//! What the CLI prints (claude 2.1.299, `tests/fixtures/claude/`):
//!
//! - `stream_event` lines wrap the API stream. A `text_delta` is an answer
//!   fragment and a `thinking_delta` is reasoning; `signature_delta`, block
//!   starts and stops, and `message_*` events carry nothing we show.
//! - an `assistant` line repeats the whole message once the block is complete,
//!   so it is skipped: the answer is the sum of the deltas.
//! - the `result` line ends the turn. It repeats the whole answer too, which is
//!   why CLI lines may be 2 MiB. A failure arrives as `is_error: true` (with
//!   `subtype: "success"`, as the CLI reports an API error that way).
//! - a tool block fails the turn closed unless the turn is wired and the tool
//!   is `mcp__lazarobox__*` (then it adds nothing: the host announces the
//!   call). A wired turn reads content blocks with an allow-list, a text-only
//!   one refuses tool-like types only (see `foreign_block`). The shape is the
//!   one claude 2.1.x documents; the manual check records the real one.
//! - a `rate_limit_event` that was `rejected` is kept as text (`(limit window:
//!   X)`) and added to the failure the turn ends with, or to the one a stream
//!   that ends with no result gets, so the orchestrator can classify it (S2).
//! - `system` and any unknown type are ignored, and so is
//!   any line that does not start with `{` (stray stdout text).

use serde_json::Value;

use super::engine::{ChatError, Finish, StreamEvent};
use super::stream::{
    CLI_LINE_CAP, ERROR_CAP, Lines, StreamDecoder, TooLong, emit, error_message, finish_with,
    malformed, too_long,
};
use super::usage::{CostSource, TurnUsage, count, micros, model_id};
use crate::core::ai::tools::wiring::CLAUDE_TOOL_PREFIX;

/// The failure of a turn in which claude started a tool that is not ours.
pub const TOOL_REFUSED: &str = "claude tried to use a tool";

/// Whether a type names a tool in any case: `tool_use`, `server_tool_use`,
/// `mcp_tool_result`, `computer_use`.
fn tool_like(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("tool") || kind.ends_with("_use")
}

/// Whether a content block is not allowed in this turn.
///
/// - Wired (the lazarobox tools are on): a strict allow-list. Only `text`,
///   `thinking`, `redacted_thinking` and a `tool_use` of ours (the prefix and
///   at least one more character) pass; any other type is foreign, including
///   one a new claude adds. The tools are a security boundary, so what the
///   list does not know ends the turn.
/// - Text-only (no tools were offered): any tool at all is foreign, ours
///   included, and so is any type that contains `tool` or ends in `_use` in
///   any case. Other new block types stay tolerated, so an ordinary chat does
///   not break when claude adds one.
///
/// A block with no string type is judged like an unknown type.
fn foreign_block(block: &Value, wired: bool) -> bool {
    let kind = block["type"].as_str();
    if !wired {
        return kind.is_some_and(tool_like);
    }
    match kind {
        Some("text" | "thinking" | "redacted_thinking") => false,
        Some("tool_use") => !block["name"].as_str().is_some_and(|name| {
            name.strip_prefix(CLAUDE_TOOL_PREFIX)
                .is_some_and(|rest| !rest.is_empty())
        }),
        _ => true,
    }
}

/// Whether this line carries a tool that is not allowed (see
/// [`foreign_block`]) in a content block: a block start, the first message
/// (`message_start`) or the repeated message (`assistant`), sub-agents
/// included. A delta whose type is tool-like, or that carries a block, counts
/// too (the `input_json_delta` of an allowed call does not). Usage counters
/// such as `message.usage.server_tool_use` are not blocks and are not read.
fn started_foreign_tool(value: &Value, wired: bool) -> bool {
    let event = &value["event"];
    let blocks: Vec<&Value> = match value["type"].as_str() {
        Some("stream_event") => match event["type"].as_str() {
            Some("content_block_start") => vec![&event["content_block"]],
            Some("message_start") => event["message"]["content"]
                .as_array()
                .map(|items| items.iter().collect())
                .unwrap_or_default(),
            Some("content_block_delta") => {
                let delta = &event["delta"];
                if delta["type"].as_str().is_some_and(tool_like) {
                    return true;
                }
                match &delta["content_block"] {
                    Value::Null => Vec::new(),
                    block => vec![block],
                }
            }
            _ => return false,
        },
        Some("assistant") => value["message"]["content"]
            .as_array()
            .map(|items| items.iter().collect())
            .unwrap_or_default(),
        _ => return false,
    };
    blocks.into_iter().any(|block| foreign_block(block, wired))
}

/// What a `result` line reports a turn used, if anything. `modelUsage` has
/// one entry per model and counts sub-agents and side models, which the
/// top-level `usage` leaves out, so it wins; `usage` is the fallback. The
/// dollars are the CLI's own `total_cost_usd`, a list price.
fn usage(value: &Value) -> Option<TurnUsage> {
    let mut sum = models_usage(value).or_else(|| top_level_usage(value))?;
    if let Some(total) = micros(&value["total_cost_usd"]) {
        sum.cost_micros = Some(total);
    }
    sum.cost_source = sum.cost_micros.map(|_| CostSource::EngineList);
    sum.duration_ms = count(&value["duration_api_ms"]);
    Some(sum)
}

/// The sum of `modelUsage`, if any entry reports something.
fn models_usage(value: &Value) -> Option<TurnUsage> {
    let models = value["modelUsage"].as_object()?;
    let mut sum = TurnUsage::default();
    for entry in models.values() {
        sum.merge(&TurnUsage {
            input: count(&entry["inputTokens"]),
            output: count(&entry["outputTokens"]),
            cache_read: count(&entry["cacheReadInputTokens"]),
            cache_write: count(&entry["cacheCreationInputTokens"]),
            reasoning: count(&entry["thinkingTokens"]),
            cost_micros: micros(&entry["costUSD"]),
            ..TurnUsage::default()
        });
    }
    // Entries that carry no count (only a cost, or nothing) say nothing about
    // the tokens: the top-level `usage` is then the better source.
    let counts = [
        sum.input,
        sum.output,
        sum.cache_read,
        sum.cache_write,
        sum.reasoning,
    ];
    if counts.iter().all(Option::is_none) {
        return None;
    }
    // A sum of costs is whole only when every model reported one: a partial
    // sum would read as the whole, so the cost stays unknown.
    if models
        .values()
        .any(|entry| micros(&entry["costUSD"]).is_none())
    {
        sum.cost_micros = None;
    }
    if models.len() == 1 {
        sum.model = models.keys().next().and_then(|k| model_id(k));
    } else {
        sum.multi_model = true;
    }
    sum.reported()
}

/// The top-level `usage`, which leaves out sub-agents and side models.
fn top_level_usage(value: &Value) -> Option<TurnUsage> {
    let usage = &value["usage"];
    TurnUsage {
        input: count(&usage["input_tokens"]),
        output: count(&usage["output_tokens"]),
        cache_read: count(&usage["cache_read_input_tokens"]),
        cache_write: count(&usage["cache_creation_input_tokens"]),
        reasoning: count(&usage["output_tokens_details"]["thinking_tokens"]),
        ..TurnUsage::default()
    }
    .reported()
}

/// The limit a `rate_limit_event` rejected, as one safe line.
fn rejected_limit(value: &Value) -> Option<String> {
    let info = &value["rate_limit_info"];
    (info["status"].as_str() == Some("rejected")).then(|| {
        let kind = info["rateLimitType"].as_str().unwrap_or("");
        if kind.trim().is_empty() {
            "rejected".to_owned()
        } else {
            error_message(kind).chars().take(LIMIT_CAP).collect()
        }
    })
}

/// The longest limit type kept.
const LIMIT_CAP: usize = 40;

/// `message` with the rejected limit's window appended, the total within
/// `ERROR_CAP`: the message gives way, never the window.
fn with_window(message: &str, limit: &str) -> String {
    let suffix = format!(" (limit window: {limit})");
    let room = ERROR_CAP.saturating_sub(suffix.chars().count());
    let head: String = message.chars().take(room).collect();
    format!("{}{suffix}", head.trim_end())
}

/// Decodes the stream-json output of the claude CLI.
pub struct ClaudeDecoder {
    lines: Lines,
    ended: bool,
    /// An answer fragment went out: the `result` text is a repeat.
    streamed: bool,
    /// The turn has the lazarobox tools wired in (see [`started_foreign_tool`]).
    wired: bool,
    /// The kind of limit a rejected `rate_limit_event` named, cleaned.
    limit: Option<String>,
}

impl ClaudeDecoder {
    /// For a turn with no tools offered.
    pub fn new() -> Self {
        Self {
            lines: Lines::new(CLI_LINE_CAP),
            ended: false,
            streamed: false,
            wired: false,
            limit: None,
        }
    }

    /// For a turn whose claude got the lazarobox MCP server.
    pub fn wired() -> Self {
        Self {
            wired: true,
            ..Self::new()
        }
    }

    /// Events of one line.
    fn line(&mut self, line: &[u8]) -> Vec<StreamEvent> {
        // Only a line that starts like a JSON object is an event. Anything
        // else (a warning a wrapper or runtime prints on stdout) is noise.
        if line.trim_ascii_start().first() != Some(&b'{') {
            return vec![];
        }
        let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(line)) else {
            return vec![malformed()];
        };
        if started_foreign_tool(&value, self.wired) {
            return vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))];
        }
        match value["type"].as_str() {
            Some("stream_event") => self.stream_event(&value),
            Some("result") => self.result(&value),
            Some("rate_limit_event") => {
                // An event of another shape does not clear a rejected limit.
                self.limit = rejected_limit(&value).or(self.limit.take());
                vec![]
            }
            _ => vec![],
        }
    }

    fn stream_event(&mut self, value: &Value) -> Vec<StreamEvent> {
        // A sub-agent's stream is not this chat's answer.
        if !value["parent_tool_use_id"].is_null() {
            return vec![];
        }
        let event = &value["event"];
        if event["type"].as_str() != Some("content_block_delta") {
            return vec![];
        }
        let delta = &event["delta"];
        match delta["type"].as_str() {
            Some("text_delta") => {
                let text = delta["text"].as_str().filter(|t| !t.is_empty());
                self.streamed |= text.is_some();
                text.map(|t| StreamEvent::Delta(t.into()))
                    .into_iter()
                    .collect()
            }
            Some("thinking_delta") => delta["thinking"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| StreamEvent::ThinkingDelta(t.into()))
                .into_iter()
                .collect(),
            _ => vec![],
        }
    }

    fn result(&self, value: &Value) -> Vec<StreamEvent> {
        let text = value["result"].as_str().unwrap_or("");
        let failed = value["is_error"].as_bool() == Some(true)
            || value["subtype"]
                .as_str()
                .is_some_and(|s| s.starts_with("error"));
        let usage = usage(value);
        if failed {
            let mut message = error_message(text);
            if let Some(limit) = &self.limit {
                message = with_window(&message, limit);
            }
            let error = StreamEvent::Error(ChatError::Failed(message));
            let spent = usage.filter(|u| !u.idle()).map(StreamEvent::usage);
            return spent.into_iter().chain([error]).collect();
        }
        let usage = usage.map(StreamEvent::usage);
        let mut events = Vec::new();
        // Without partial messages the result is the only copy of the answer.
        if !self.streamed && !text.is_empty() {
            events.push(StreamEvent::Delta(text.into()));
        }
        let finish = if value["stop_reason"].as_str() == Some("max_tokens") {
            Finish::Length
        } else {
            Finish::Stop
        };
        events.extend(usage);
        events.push(StreamEvent::Done(finish));
        events
    }

    fn lines_events(&mut self, lines: Vec<Result<Vec<u8>, TooLong>>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        for line in lines {
            if self.ended {
                break;
            }
            let events = line.map_or_else(|TooLong| vec![too_long()], |l| self.line(&l));
            emit(&mut self.ended, &mut out, events);
        }
        out
    }
}

impl Default for ClaudeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder for ClaudeDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        if self.ended {
            return Vec::new();
        }
        let lines = self.lines.feed(bytes);
        self.lines_events(lines)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if !self.ended {
            let rest = self.lines.rest();
            out = self.lines_events(vec![rest]);
        }
        // A rejected limit explains a stream that ends with no result.
        if let (false, Some(limit)) = (self.ended, &self.limit) {
            self.ended = true;
            let message = with_window("the engine's stream ended without a result", limit);
            out.push(StreamEvent::Error(ChatError::Failed(message)));
        }
        finish_with(&mut self.ended, out)
    }
}

#[cfg(test)]
mod tests;
