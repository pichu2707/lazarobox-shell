//! The loopback HTTP client for chat turns: one POST to `127.0.0.1`, the reply
//! decoded as it arrives. Detection (`ai_detect`) stays a GET with a hard body
//! cap; nothing here is shared with it but the framing primitives in
//! `core::ai::http`.

use std::{net::SocketAddr, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    time::{Instant, sleep_until, timeout},
};

use crate::core::ai::{
    chat::{
        engine::{ChatError, StreamEvent},
        stream::StreamDecoder,
    },
    http::{Dechunker, Framing, Head, Malformed, Step, parse_head},
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
/// Every turn ends with exactly one `Done` or `Error` event, unless `tx` is
/// closed or the future is dropped: both end the turn and close the socket.
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
            match timeout(self.timeouts.idle, stream.read(buf)).await {
                Ok(read) => read,
                Err(_) => return Err(Stop::Failed(STALLED)),
            }
        } else {
            let slow_at = self.start + self.timeouts.slow;
            let give_up = self.start + self.timeouts.first_byte;
            loop {
                tokio::select! {
                    read = stream.read(buf) => break read,
                    _ = sleep_until(slow_at), if !self.slow_sent => {
                        self.slow_sent = true;
                        tx.send(StreamEvent::Slow).await.map_err(|_| Stop::Cancelled)?;
                    }
                    _ = sleep_until(give_up) => return Err(Stop::Failed(NO_ANSWER)),
                }
            }
        };
        let n = read.map_err(|_| Stop::Failed(INVALID_REPLY))?;
        self.started |= n > 0;
        Ok(n)
    }
}

/// The body framing of a reply, applied to each read.
enum Body {
    Length(usize),
    Chunked(Dechunker),
    Close,
}

impl Body {
    fn new(framing: Framing) -> Self {
        match framing {
            Framing::Length(n) => Body::Length(n),
            Framing::Chunked => Body::Chunked(Dechunker::new()),
            Framing::Close => Body::Close,
        }
    }

    /// Appends the body bytes of `bytes` to `out`; `Done` when the body ended.
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<Step, Malformed> {
        match self {
            Body::Length(left) => {
                let n = (*left).min(bytes.len());
                out.extend_from_slice(&bytes[..n]);
                *left -= n;
                Ok(if *left == 0 { Step::Done } else { Step::More })
            }
            Body::Chunked(dechunker) => dechunker.feed(bytes, out),
            Body::Close => {
                out.extend_from_slice(bytes);
                Ok(Step::More)
            }
        }
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
    stream
        .write_all(request)
        .await
        .map_err(|_| Stop::Failed(CANNOT_CONNECT))?;
    let mut buf = vec![0u8; READ_SIZE];

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

#[cfg(test)]
mod tests;
