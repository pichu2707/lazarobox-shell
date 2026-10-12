//! The loopback HTTP client for chat turns: one POST to `127.0.0.1`, the reply
//! decoded as it arrives. Detection (`ai_detect`) stays a GET with a hard body
//! cap; nothing here is shared with it but the framing primitives in
//! `core::ai::http`.

use std::{net::SocketAddr, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    time::{Instant, sleep_until, timeout, timeout_at},
};

use crate::core::ai::{
    chat::{
        engine::{ChatError, StreamEvent},
        stream::StreamDecoder,
    },
    http::{Body, Framing, Head, Step, parse_head},
    sanitize::clean,
};

/// Size of one read from the socket.
const READ_SIZE: usize = 16 * 1024;

/// The only paths a chat may POST to.
pub const CHAT_PATHS: [&str; 2] = ["/api/chat", "/v1/chat/completions"];

/// Most bytes read of a non-200 body before the reply is given up on.
pub const ERROR_BODY_CAP: usize = 64 * 1024;

/// Longest error message sent for a non-200 reply, in characters.
pub const ERROR_MESSAGE_CAP: usize = 200;

pub const CANNOT_CONNECT: &str = "cannot connect to the engine";
pub const NO_ANSWER: &str = "no answer from the engine";
pub const STALLED: &str = "the engine stopped answering";
pub const TOO_LONG: &str = "the turn took too long";
pub const INVALID_REPLY: &str = "invalid reply from the engine";
pub const REFUSED: &str = "refused: chat requests go to the loopback engine paths only";

/// The time limits of one turn. Passed in, so tests run them on the paused
/// tokio clock instead of waiting.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    /// To open the connection.
    pub connect: Duration,
    /// From the request to the first byte of the reply.
    pub first_byte: Duration,
    /// Between reads once the reply started.
    pub idle: Duration,
    /// From the request to the end of the turn.
    pub total: Duration,
    /// Without a first byte after this long, a `Slow` event is sent once.
    pub slow: Duration,
}

impl Timeouts {
    /// What a server turn gets.
    pub const SERVER: Self = Self {
        connect: Duration::from_secs(2),
        first_byte: Duration::from_secs(120),
        idle: Duration::from_secs(60),
        total: Duration::from_secs(15 * 60),
        slow: Duration::from_secs(3),
    };
}

/// POSTs `body` to `path` on `host:port` and sends the decoded reply to `tx`.
/// Every turn ends with exactly one `Done` or `Error` event, unless the turn is
/// cancelled: dropping or aborting the future, or dropping the receiver, ends it
/// at once and closes the socket without further events.
pub async fn post_stream(
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    decoder: &mut (dyn StreamDecoder + Send),
    tx: &mpsc::Sender<StreamEvent>,
) {
    post_stream_with(&Timeouts::SERVER, host, port, path, body, decoder, tx).await
}

pub(crate) async fn post_stream_with(
    timeouts: &Timeouts,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    decoder: &mut (dyn StreamDecoder + Send),
    tx: &mpsc::Sender<StreamEvent>,
) {
    // Only the literal loopback address and the static chat paths: nothing a
    // model, a config file or a redirect says can steer the request elsewhere.
    if host != "127.0.0.1" || !CHAT_PATHS.contains(&path) {
        fail(tx, REFUSED).await;
        return;
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stream = match timeout(timeouts.connect, TcpStream::connect(addr)).await {
        Ok(Ok(stream)) => stream,
        _ => {
            fail(tx, CANNOT_CONNECT).await;
            return;
        }
    };
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    drive(stream, &request, timeouts, decoder, tx).await;
}

/// Why a turn stopped before its reply was complete.
enum Stop {
    /// The receiver is gone: nobody wants the rest.
    Cancelled,
    /// The turn ends with this message.
    Failed(&'static str),
    /// The turn ends with this error, already built.
    Error(ChatError),
}

/// Sends `request` over `stream` and relays the reply to `tx`. The total
/// limit covers everything from here on.
pub(crate) async fn drive<S>(
    mut stream: S,
    request: &[u8],
    timeouts: &Timeouts,
    decoder: &mut (dyn StreamDecoder + Send),
    tx: &mpsc::Sender<StreamEvent>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let turn = relay(&mut stream, request, timeouts, decoder, tx);
    let error = match timeout(timeouts.total, turn).await {
        Ok(Ok(())) | Ok(Err(Stop::Cancelled)) => return,
        Ok(Err(Stop::Failed(message))) => ChatError::Failed(message.into()),
        Ok(Err(Stop::Error(error))) => error,
        Err(_) => ChatError::Failed(TOO_LONG.into()),
    };
    let _ = tx.send(StreamEvent::Error(error)).await;
}

async fn fail(tx: &mpsc::Sender<StreamEvent>, message: &str) {
    let _ = tx
        .send(StreamEvent::Error(ChatError::Failed(message.into())))
        .await;
}

/// Where the clock stands for one reply.
struct Wait<'a> {
    timeouts: &'a Timeouts,
    start: Instant,
    /// A byte of the reply has arrived.
    started: bool,
    slow_sent: bool,
}

impl Wait<'_> {
    /// One read, under the limit that applies now. Before the first byte that
    /// is the first-byte limit, with a single `Slow` event on the way; after
    /// it, the idle limit.
    async fn read<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
        buf: &mut [u8],
        tx: &mpsc::Sender<StreamEvent>,
    ) -> Result<usize, Stop> {
        let read = if self.started {
            tokio::select! {
                read = timeout(self.timeouts.idle, stream.read(buf)) => match read {
                    Ok(read) => read,
                    Err(_) => return Err(Stop::Failed(STALLED)),
                },
                _ = tx.closed() => return Err(Stop::Cancelled),
            }
        } else {
            let slow_at = self.start + self.timeouts.slow;
            let give_up = self.start + self.timeouts.first_byte;
            loop {
                tokio::select! {
                    read = stream.read(buf) => break read,
                    _ = sleep_until(slow_at), if !self.slow_sent => {
                        self.slow_sent = true;
                        // Best effort: a full channel must not hold back the
                        // read or the timers, so the hint is dropped instead.
                        match tx.try_send(StreamEvent::Slow) {
                            Err(mpsc::error::TrySendError::Closed(_)) => return Err(Stop::Cancelled),
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                        }
                    }
                    _ = sleep_until(give_up) => return Err(Stop::Failed(NO_ANSWER)),
                    _ = tx.closed() => return Err(Stop::Cancelled),
                }
            }
        };
        let n = match read {
            Ok(n) => n,
            // A reset after the reply began is an early end, like a close.
            Err(_) if self.started => 0,
            Err(_) => return Err(Stop::Failed(INVALID_REPLY)),
        };
        self.started |= n > 0;
        Ok(n)
    }
}

async fn relay<S>(
    stream: &mut S,
    request: &[u8],
    timeouts: &Timeouts,
    decoder: &mut (dyn StreamDecoder + Send),
    tx: &mpsc::Sender<StreamEvent>,
) -> Result<(), Stop>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wait = Wait {
        timeouts,
        start: Instant::now(),
        started: false,
        slow_sent: false,
    };
    let mut buf = vec![0u8; READ_SIZE];
    // The request goes out under the first-byte limit too: a server that never
    // reads it is as silent as one that never answers.
    let written = tokio::select! {
        written = timeout_at(wait.start + timeouts.first_byte, stream.write_all(request)) => written,
        _ = tx.closed() => return Err(Stop::Cancelled),
    };
    match written {
        Err(_) => return Err(Stop::Failed(NO_ANSWER)),
        Ok(Err(_)) => return Err(after_failed_write(&mut wait, stream, &mut buf, tx).await),
        Ok(Ok(())) => {}
    }

    // The head, until its blank line.
    let mut raw = Vec::new();
    let (status, framing, head_len) = loop {
        match parse_head(&raw) {
            Head::Incomplete => {}
            Head::Invalid => return Err(Stop::Failed(INVALID_REPLY)),
            Head::Complete {
                status,
                framing,
                head_len,
            } => break (status, framing, head_len),
        }
        match wait.read(stream, &mut buf, tx).await? {
            0 => return Err(Stop::Failed(INVALID_REPLY)),
            n => raw.extend_from_slice(&buf[..n]),
        }
    };
    let mut body = Body::new(framing);
    let mut pending = raw.split_off(head_len);

    if status != 200 {
        return Err(non_200(status, body, pending, &mut wait, stream, &mut buf, tx).await);
    }

    let mut data = Vec::new();
    loop {
        let step = body
            .feed(&pending, &mut data)
            .map_err(|_| Stop::Failed(INVALID_REPLY))?;
        pending.clear();
        if !data.is_empty() {
            let events = decoder.feed(&data);
            data.clear();
            if send_all(tx, events).await? {
                return Ok(());
            }
        }
        if step == Step::Done {
            break;
        }
        match wait.read(stream, &mut buf, tx).await? {
            0 => break,
            n => pending.extend_from_slice(&buf[..n]),
        }
    }
    // The reply ended, by its framing or by the peer: whatever the decoder
    // still holds, and an interruption if the engine never said it was done.
    let events = decoder.finish();
    send_all(tx, events).await?;
    Ok(())
}

/// A write that failed (EPIPE, reset) often means the server answered early and
/// hung up, say with a 413 before reading a large body. Reads the head once more:
/// a non-200 status is reported as any other; anything else is a failed connect.
async fn after_failed_write<S: AsyncRead + Unpin>(
    wait: &mut Wait<'_>,
    stream: &mut S,
    buf: &mut [u8],
    tx: &mpsc::Sender<StreamEvent>,
) -> Stop {
    let mut raw = Vec::new();
    loop {
        match parse_head(&raw) {
            Head::Incomplete => {}
            Head::Complete {
                status,
                framing,
                head_len,
            } if status != 200 => {
                let pending = raw.split_off(head_len);
                return non_200(status, Body::new(framing), pending, wait, stream, buf, tx).await;
            }
            _ => return Stop::Failed(CANNOT_CONNECT),
        }
        match wait.read(stream, buf, tx).await {
            Ok(n) if n > 0 => raw.extend_from_slice(&buf[..n]),
            Err(Stop::Cancelled) => return Stop::Cancelled,
            _ => return Stop::Failed(CANNOT_CONNECT),
        }
    }
}

/// Sends `events` up to the first one that ends the turn; true when one did.
async fn send_all(tx: &mpsc::Sender<StreamEvent>, events: Vec<StreamEvent>) -> Result<bool, Stop> {
    for event in events {
        let ends = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
        tx.send(event).await.map_err(|_| Stop::Cancelled)?;
        if ends {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The error for a reply that is not a 200: the status and a short message
/// from the first `ERROR_BODY_CAP` bytes of the body, never all of it.
async fn non_200<S: AsyncRead + Unpin>(
    status: u16,
    mut body: Body,
    mut pending: Vec<u8>,
    wait: &mut Wait<'_>,
    stream: &mut S,
    buf: &mut [u8],
    tx: &mpsc::Sender<StreamEvent>,
) -> Stop {
    let mut text = Vec::new();
    loop {
        let step = body.feed(&pending, &mut text);
        pending.clear();
        if text.len() >= ERROR_BODY_CAP || !matches!(step, Ok(Step::More)) {
            break;
        }
        match wait.read(stream, buf, tx).await {
            Ok(0) => break,
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(Stop::Cancelled) => return Stop::Cancelled,
            Err(_) => break,
        }
    }
    text.truncate(ERROR_BODY_CAP);
    // `clean` keeps `\n` and `\t`; both would let the text fake extra lines.
    let flat = clean(&String::from_utf8_lossy(&text))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let message = if flat.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {flat}")
    };
    let capped: String = message.chars().take(ERROR_MESSAGE_CAP).collect();
    Stop::Error(ChatError::Failed(capped.trim_end().into()))
}

/// The only route prefixes an engram request may target.
pub const ENGRAM_ROUTES: [&str; 6] = [
    "/health",
    "/project/current",
    "/sessions",
    "/observations",
    "/context",
    "/search",
];

/// Largest body `request` sends or accepts, in bytes.
pub const REQUEST_BODY_CAP: usize = 256 * 1024;

/// The methods an engram request may use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Get,
    Post,
    Delete,
}

/// A complete reply: any status the caller must interpret.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Why a request produced no reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallError {
    /// Nothing accepted the connection.
    Connect,
    /// A time limit ran out.
    Timeout,
    /// The reply was malformed, ambiguous, truncated or a redirect.
    Invalid,
    /// A body is over `REQUEST_BODY_CAP`.
    TooLarge,
    /// The request itself was not allowed; nothing was sent.
    Refused,
}

/// The time limits of one request.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub connect: Duration,
    /// From the connection attempt to the end of the reply.
    pub total: Duration,
}

impl Limits {
    pub const HEALTH: Self = Self::new(300);
    pub const READ: Self = Self::new(800);
    pub const WRITE: Self = Self::new(2000);

    const fn new(total_ms: u64) -> Self {
        Self {
            connect: Duration::from_millis(200),
            total: Duration::from_millis(total_ms),
        }
    }
}

pub async fn request(
    port: u16,
    method: Method,
    target: &str,
    body: Option<&[u8]>,
    auth: Option<&str>,
    limits: Limits,
) -> Result<Reply, CallError> {
    request_to("127.0.0.1", port, method, target, body, auth, limits).await
}

/// Sends one request to `127.0.0.1:port` and returns the reply, whatever its
/// status (except a redirect or an informational one, which are `Invalid`).
/// Nothing leaves the machine and nothing is followed: the host is checked, the
/// target must sit under `ENGRAM_ROUTES`, the token goes on `DELETE` only, and
/// the reply body is capped at `REQUEST_BODY_CAP`. The token is never put in an
/// error.
pub(crate) async fn request_to(
    host: &str,
    port: u16,
    method: Method,
    target: &str,
    body: Option<&[u8]>,
    auth: Option<&str>,
    limits: Limits,
) -> Result<Reply, CallError> {
    if host != "127.0.0.1" || !allowed_target(target) {
        return Err(CallError::Refused);
    }
    match (method, body, auth) {
        (Method::Post, Some(body), _) if body.len() > REQUEST_BODY_CAP => {
            return Err(CallError::TooLarge);
        }
        (Method::Get | Method::Delete, Some(_), _) => return Err(CallError::Refused),
        (Method::Get | Method::Post, _, Some(_)) => return Err(CallError::Refused),
        (_, _, Some(token)) if !header_value(token) => return Err(CallError::Refused),
        _ => {}
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let start = Instant::now();
    let stream = match timeout(limits.connect, TcpStream::connect(addr)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) => return Err(CallError::Connect),
        Err(_) => return Err(CallError::Timeout),
    };
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
    };
    let mut head = format!(
        "{verb} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\n"
    );
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    if let Some(token) = auth {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body.unwrap_or(&[]));
    match timeout_at(start + limits.total, exchange(stream, &bytes)).await {
        Ok(result) => result,
        Err(_) => Err(CallError::Timeout),
    }
}

/// A target is allowed when it is printable ASCII (no space, CR, LF, tab or
/// other control byte), has no `#`, and its path is an engram route or below
/// one. In the path (before `?`) a `.` or `..` segment is refused even when
/// percent-encoded (`%2e`), and so are an encoded slash (`%2f`), an encoded
/// backslash (`%5c`) and a raw backslash: a server may decode them into a
/// different route. The query is only checked for bytes, so an encoded cwd such
/// as `%2Ftmp%2F..%2Fx` still passes.
fn allowed_target(target: &str) -> bool {
    if !target.bytes().all(|b| b.is_ascii_graphic()) || target.contains('#') {
        return false;
    }
    let path = target.split('?').next().unwrap_or("");
    if path.contains('\\') || path.split('/').any(dot_segment) {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c") {
        return false;
    }
    ENGRAM_ROUTES.iter().any(|route| {
        path.strip_prefix(route)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Whether a path segment is `.` or `..` once `%2e` is decoded.
fn dot_segment(segment: &str) -> bool {
    let decoded = segment.to_ascii_lowercase().replace("%2e", ".");
    decoded == "." || decoded == ".."
}

/// A bearer value that cannot break the header: non-empty printable ASCII, no
/// space.
fn header_value(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| b.is_ascii_graphic())
}

async fn exchange(mut stream: TcpStream, request: &[u8]) -> Result<Reply, CallError> {
    stream
        .write_all(request)
        .await
        .map_err(|_| CallError::Invalid)?;
    let mut buf = vec![0u8; READ_SIZE];
    let mut raw = Vec::new();
    let (status, framing, head_len) = loop {
        match parse_head(&raw) {
            Head::Incomplete => {}
            Head::Invalid => return Err(CallError::Invalid),
            Head::Complete {
                status,
                framing,
                head_len,
            } => break (status, framing, head_len),
        }
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => return Err(CallError::Invalid),
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    };
    // Redirects are never followed and 1xx is not a final answer.
    if !matches!(status, 200..=299 | 400..=599) {
        return Err(CallError::Invalid);
    }
    if matches!(framing, Framing::Length(n) if n > REQUEST_BODY_CAP) {
        return Err(CallError::TooLarge);
    }
    let mut framed = Body::new(framing);
    let mut out = Vec::new();
    let mut pending = raw.split_off(head_len);
    loop {
        let step = framed.feed(&pending, &mut out);
        if out.len() > REQUEST_BODY_CAP {
            return Err(CallError::TooLarge);
        }
        match step {
            Ok(Step::Done) => return Ok(Reply { status, body: out }),
            Ok(Step::More) => {}
            Err(_) => return Err(CallError::Invalid),
        }
        match stream.read(&mut buf).await {
            // The peer closed: that ends a close-delimited body and truncates
            // any other.
            Ok(0) if matches!(framing, Framing::Close) => {
                return Ok(Reply { status, body: out });
            }
            Ok(0) | Err(_) => return Err(CallError::Invalid),
            Ok(n) => {
                pending.clear();
                pending.extend_from_slice(&buf[..n]);
            }
        }
    }
}

#[cfg(test)]
mod request_tests;
#[cfg(test)]
mod tests;
