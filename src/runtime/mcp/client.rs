//! The MCP client of slice 4b: lazarobox as the client of a server the user
//! declared (`[[ai.mcp.servers]]`, see `mcp-servers`).
//!
//! Trust model:
//!
//! - The user chose to trust this server, so it is spawned directly and NOT
//!   jailed. That is exactly why nothing here starts on its own: this module
//!   has no caller yet that reads config or opens a repository, and a later
//!   slice may only start a server on the demand of a chat turn with tools on.
//! - Everything a server sends is untrusted data. Tool names stay raw for
//!   routing (`servers::qualify` maps them), descriptions are sanitized and
//!   capped, a schema over its cap is replaced, and a line over
//!   [`MAX_LINE_BYTES`] is skipped without being buffered.
//! - A server never gets more than the user named: see `child_env` for the
//!   scrubbed environment. It never sees the session token or socket path.
//! - The wire is the 2c codec ([`Frame`]) in the other direction. Requests are
//!   strictly one at a time: a stale answer of a cancelled request is skipped
//!   by its id. A hung-up or dying server ends the call with an error result,
//!   a timer bounds every wait, and nothing logs arguments or results.

use std::{future::Future, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    time::timeout,
};

use super::{
    Outcome,
    session::{Line, read_line},
};
use crate::core::ai::{
    sanitize::clean,
    tools::{
        ExternalTool,
        mcp::{Frame, MAX_LINE_BYTES, PROTOCOL_VERSION},
    },
};

/// Most tools kept from one server.
const MAX_TOOLS: usize = 64;
/// Most characters kept of a tool description.
const DESCRIPTION_MAX: usize = 1024;
/// Most bytes of a tool schema; a bigger one is replaced by a bare object.
const SCHEMA_MAX: usize = 16 * 1024;
/// Most characters of a server's error message kept.
const MESSAGE_MAX: usize = 200;
/// How long a `notifications/cancelled` may take to be written.
const CANCEL_WRITE: Duration = Duration::from_secs(1);

/// How long each kind of wait may last.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// `initialize` and `tools/list`.
    pub handshake: Duration,
    /// One `tools/call`. The user may be asked for a `y` before it, but that
    /// wait is not part of it.
    pub call: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            handshake: Duration::from_secs(20),
            call: Duration::from_secs(15 * 60),
        }
    }
}

/// Why a request got no result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ClientError {
    Timeout,
    Cancelled,
    /// The server hung up or died.
    Closed,
    /// The server answered something this client cannot use.
    Protocol(&'static str),
    /// The server answered with a JSON-RPC error (message sanitized, capped).
    Remote(String),
}

impl ClientError {
    /// A short reason for the model or a notice. Server text is cleaned.
    pub fn reason(&self) -> String {
        match self {
            ClientError::Timeout => "timed out".to_owned(),
            ClientError::Cancelled => "cancelled".to_owned(),
            ClientError::Closed => "server closed the connection".to_owned(),
            ClientError::Protocol(what) => format!("bad reply: {what}"),
            ClientError::Remote(message) => message.clone(),
        }
    }
}

/// One connected server. Cancel-safe between requests: dropping a future
/// mid-request leaves the answer, if it comes, to be skipped by its id.
pub struct McpClient {
    reader: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    next_id: u64,
    timeouts: Timeouts,
}

/// How one request ended, before the cancel notice is written.
enum Ended {
    Answer(Result<Value, ClientError>),
    Timeout,
    Cancelled,
}

impl McpClient {
    /// A client over any transport. [`McpClient::handshake`] must come first.
    pub fn over(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
        timeouts: Timeouts,
    ) -> Self {
        McpClient {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            next_id: 1,
            timeouts,
        }
    }

    /// `initialize` with the pinned revision, then `notifications/initialized`.
    pub async fn handshake(&mut self) -> Result<(), ClientError> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "lazarobox", "version": env!("CARGO_PKG_VERSION")},
        });
        let limit = self.timeouts.handshake;
        let result = self
            .request("initialize", params, limit, std::future::pending())
            .await?;
        if !result.get("protocolVersion").is_some_and(Value::is_string) {
            return Err(ClientError::Protocol("no protocolVersion"));
        }
        self.send(Frame::Notification {
            method: "notifications/initialized".to_owned(),
            params: Value::Null,
        })
        .await
    }

    /// The tools of the server, as it names them: at most [`MAX_TOOLS`], an
    /// entry without a string name skipped, text cleaned and capped.
    pub async fn list_tools(&mut self) -> Result<Vec<ExternalTool>, ClientError> {
        let limit = self.timeouts.handshake;
        let result = self
            .request("tools/list", Value::Null, limit, std::future::pending())
            .await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or(ClientError::Protocol("no tools array"))?;
        Ok(tools.iter().filter_map(read_tool).take(MAX_TOOLS).collect())
    }

    /// Calls the tool `raw` (the server's own name for it). Never fails: a
    /// timeout, a cancel, a dead server or an error from the server is an
    /// error result. `cancel` ending cancels the call in flight.
    pub async fn call(
        &mut self,
        raw: &str,
        arguments: Value,
        cancel: impl Future<Output = ()>,
    ) -> Outcome {
        let limit = self.timeouts.call;
        let params = json!({"name": raw, "arguments": arguments});
        match self.request("tools/call", params, limit, cancel).await {
            Ok(result) => Outcome {
                is_error: result.get("isError") == Some(&Value::Bool(true)),
                text: content_text(&result),
            },
            Err(error) => Outcome {
                text: format!("mcp error: {}", error.reason()),
                is_error: true,
            },
        }
    }

    async fn send(&mut self, frame: Frame) -> Result<(), ClientError> {
        let line = format!("{}\n", frame.to_line());
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|_| ClientError::Closed)?;
        self.writer.flush().await.map_err(|_| ClientError::Closed)
    }

    /// One request and its answer, bounded by `limit` and by `cancel`. On a
    /// timeout or a cancel the server is told with `notifications/cancelled`.
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        limit: Duration,
        cancel: impl Future<Output = ()>,
    ) -> Result<Value, ClientError> {
        let id = json!(self.next_id);
        self.next_id += 1;
        self.send(Frame::Request {
            id: id.clone(),
            method: method.to_owned(),
            params,
        })
        .await?;
        let ended = {
            let wait = timeout(limit, self.answer(&id));
            tokio::select! {
                answer = wait => answer.map_or(Ended::Timeout, Ended::Answer),
                () = cancel => Ended::Cancelled,
            }
        };
        let (error, reason) = match ended {
            Ended::Answer(answer) => return answer,
            Ended::Timeout => (ClientError::Timeout, "timed out"),
            Ended::Cancelled => (ClientError::Cancelled, "cancelled"),
        };
        let notice = Frame::Notification {
            method: "notifications/cancelled".to_owned(),
            params: json!({"requestId": id, "reason": reason}),
        };
        // Best effort: a server that does not read must not hold the turn.
        let _ = timeout(CANCEL_WRITE, self.send(notice)).await;
        Err(error)
    }

    /// Reads frames until the answer to `id`. Other answers (a cancelled
    /// request's), notifications and unusable lines are skipped; a request of
    /// the server gets `ping` answered and everything else refused.
    async fn answer(&mut self, id: &Value) -> Result<Value, ClientError> {
        loop {
            let line = match read_line(&mut self.reader, MAX_LINE_BYTES).await {
                Ok(Line::Text(line)) => line,
                Ok(Line::Eof) | Err(_) => return Err(ClientError::Closed),
                Ok(Line::Oversize | Line::Invalid) => continue,
            };
            match Frame::parse(&line) {
                Ok(Frame::Response { id: got, result }) if got == *id => return Ok(result),
                Ok(Frame::Error {
                    id: got, message, ..
                }) if got == *id => {
                    return Err(ClientError::Remote(
                        clean(&message).chars().take(MESSAGE_MAX).collect(),
                    ));
                }
                Ok(Frame::Request { id, method, .. }) => {
                    let reply = if method == "ping" {
                        Frame::Response {
                            id,
                            result: json!({}),
                        }
                    } else {
                        Frame::Error {
                            id,
                            code: -32601,
                            message: "method not found".to_owned(),
                        }
                    };
                    self.send(reply).await?;
                }
                _ => {}
            }
        }
    }
}

/// One entry of `tools/list`; `None` without a usable name.
fn read_tool(entry: &Value) -> Option<ExternalTool> {
    let name = entry.get("name")?.as_str().filter(|n| !n.is_empty())?;
    let description = entry
        .get("description")
        .and_then(Value::as_str)
        .map(|d| clean(d).chars().take(DESCRIPTION_MAX).collect())
        .unwrap_or_default();
    let schema = entry
        .get("inputSchema")
        .filter(|s| s.is_object() && s.to_string().len() <= SCHEMA_MAX)
        .cloned()
        .unwrap_or_else(|| json!({"type": "object"}));
    Some(ExternalTool {
        name: name.to_owned(),
        description,
        schema,
    })
}

/// The text of a call result: its text blocks, one per line, and a marker for
/// any other kind. Sanitizing, capping and labelling are the host's.
fn content_text(result: &Value) -> String {
    let blocks = result.get("content").and_then(Value::as_array);
    blocks
        .into_iter()
        .flatten()
        .map(|block| {
            match (
                block.get("type").and_then(Value::as_str),
                block.get("text").and_then(Value::as_str),
            ) {
                (Some("text"), Some(text)) => text.to_owned(),
                (Some(kind), _) => format!(
                    "[{} content omitted]",
                    clean(kind).chars().take(24).collect::<String>()
                ),
                _ => "[content omitted]".to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
