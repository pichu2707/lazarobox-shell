//! The Ollama pull over the loopback client (ollama-download): one POST to
//! [`PULL_PATH`] on `127.0.0.1`, its NDJSON reply read as it arrives. Only
//! the pull uses this path; a chat never does (see [`super::CHAT_PATHS`]).

use std::{net::SocketAddr, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout, timeout_at},
};

use super::{ERROR_BODY_CAP, READ_SIZE, Timeouts};
use crate::core::ai::{
    http::{Body, Head, Step, parse_head},
    ollama_lib::pull::{PullDecoder, Pulled, status_reason},
};

/// The only path a pull may POST to.
pub const PULL_PATH: &str = "/api/pull";

pub const PULL_REFUSED: &str = "refused: a pull goes to the loopback Ollama pull path only";
pub const PULL_CANNOT_CONNECT: &str = "cannot connect to Ollama";
pub const PULL_NO_ANSWER: &str = "no answer from Ollama";
pub const PULL_STALLED: &str = "Ollama stopped answering";
pub const PULL_TOO_LONG: &str = "the pull took too long";
pub const PULL_INVALID: &str = "invalid reply from Ollama";

impl Timeouts {
    /// What a pull gets: Ollama may sit on a layer for minutes (verifying
    /// a digest of tens of GB), and a big model takes hours on a slow line.
    /// `slow` and `quiet` are not used.
    pub const PULL: Self = Self {
        connect: Duration::from_secs(2),
        first_byte: Duration::from_secs(120),
        idle: Duration::from_secs(300),
        total: Duration::from_secs(24 * 60 * 60),
        slow: Duration::from_secs(120),
        quiet: None,
    };
}

/// POSTs `body` to `path` on `host:port` and reads the pull's reply,
/// calling `progress(done, total)` as it rises. `Ok` once Ollama says
/// `success`; otherwise the reason it failed. Dropping the future closes
/// the connection: that is how a pull is cancelled.
pub async fn pull_with(
    timeouts: &Timeouts,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<(), String> {
    // Only the literal loopback address and the one static path.
    if host != "127.0.0.1" || path != PULL_PATH {
        return Err(PULL_REFUSED.to_owned());
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(Ok(stream)) = timeout(timeouts.connect, TcpStream::connect(addr)).await else {
        return Err(PULL_CANNOT_CONNECT.to_owned());
    };
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    match timeout(
        timeouts.total,
        exchange(stream, &request, timeouts, progress),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(PULL_TOO_LONG.to_owned()),
    }
}

/// The clock of one reply: the first-byte limit until a byte arrives, then
/// the idle limit between reads.
struct Clock<'a> {
    timeouts: &'a Timeouts,
    start: Instant,
    started: bool,
}

impl Clock<'_> {
    async fn read<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
        buf: &mut [u8],
    ) -> Result<usize, String> {
        let (deadline, late) = if self.started {
            (Instant::now() + self.timeouts.idle, PULL_STALLED)
        } else {
            (self.start + self.timeouts.first_byte, PULL_NO_ANSWER)
        };
        let n = match timeout_at(deadline, stream.read(buf)).await {
            Err(_) => return Err(late.to_owned()),
            Ok(Ok(n)) => n,
            // A reset after the reply began is an early end, like a close.
            Ok(Err(_)) if self.started => 0,
            Ok(Err(_)) => return Err(PULL_INVALID.to_owned()),
        };
        self.started |= n > 0;
        Ok(n)
    }
}

async fn exchange<S>(
    mut stream: S,
    request: &[u8],
    timeouts: &Timeouts,
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut clock = Clock {
        timeouts,
        start: Instant::now(),
        started: false,
    };
    match timeout_at(clock.start + timeouts.first_byte, stream.write_all(request)).await {
        Err(_) => return Err(PULL_NO_ANSWER.to_owned()),
        Ok(Err(_)) => return Err(PULL_CANNOT_CONNECT.to_owned()),
        Ok(Ok(())) => {}
    }
    let mut buf = vec![0u8; READ_SIZE];
    let mut raw = Vec::new();
    let (status, framing, head_len) = loop {
        match parse_head(&raw) {
            Head::Incomplete => {}
            Head::Invalid => return Err(PULL_INVALID.to_owned()),
            Head::Complete {
                status,
                framing,
                head_len,
            } => break (status, framing, head_len),
        }
        match clock.read(&mut stream, &mut buf).await? {
            0 => return Err(PULL_INVALID.to_owned()),
            n => raw.extend_from_slice(&buf[..n]),
        }
    };
    let mut body = Body::new(framing);
    let mut pending = raw.split_off(head_len);
    if status != 200 {
        let mut text = Vec::new();
        loop {
            let step = body.feed(&pending, &mut text);
            pending.clear();
            if text.len() >= ERROR_BODY_CAP || !matches!(step, Ok(Step::More)) {
                break;
            }
            match clock.read(&mut stream, &mut buf).await {
                Ok(n) if n > 0 => pending.extend_from_slice(&buf[..n]),
                _ => break,
            }
        }
        text.truncate(ERROR_BODY_CAP);
        return Err(status_reason(status, &text));
    }
    let mut decoder = PullDecoder::new();
    let mut data = Vec::new();
    loop {
        let step = body
            .feed(&pending, &mut data)
            .map_err(|_| PULL_INVALID.to_owned())?;
        pending.clear();
        if let Some(end) = relay(decoder.feed(&data), progress) {
            return end;
        }
        data.clear();
        if step == Step::Done {
            break;
        }
        match clock.read(&mut stream, &mut buf).await? {
            0 => break,
            n => pending.extend_from_slice(&buf[..n]),
        }
    }
    relay(decoder.finish(), progress).unwrap_or_else(|| Err(PULL_INVALID.to_owned()))
}

/// Reports the progress of `events`; how the pull ended, if one of them
/// ends it.
fn relay(
    events: Vec<Pulled>,
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Option<Result<(), String>> {
    for event in events {
        match event {
            Pulled::Progress { done, total } => progress(done, total),
            Pulled::Done => return Some(Ok(())),
            Pulled::Failed(reason) => return Some(Err(reason)),
        }
    }
    None
}
