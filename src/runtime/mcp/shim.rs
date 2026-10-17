//! The stdio shim of the MCP transport: `lazarobox-shell __mcp-serve <socket>`.
//!
//! An engine (a CLI that speaks MCP over stdio) launches this process. It is a
//! dumb relay between the engine's stdin/stdout and the unix socket of its
//! chat's session (see `runtime::mcp`), written with plain `std` threads: no
//! tokio, no tool code. The registry and every decision stay in the app.
//!
//! Trust model:
//!
//! - The socket path is the one argument. It is not a secret.
//! - The token comes from the environment ([`TOKEN_ENV`]), never argv, which
//!   any local user can read in `ps`. The shim sends it as the first line,
//!   `{"lazarobox":1,"token":"<hex>"}`, and relays only after `{"ok":true}`.
//!   No answer, another answer, EOF or silence for [`HANDSHAKE_WAIT`] is a
//!   refusal: it exits non-zero with a short message on stderr. No message
//!   ever contains the token.
//! - It opens no file and starts no process. Under the jail that is the
//!   point: it needs only its own executable, its loader and the socket.
//! - Each line is capped at the same 1 MiB as the session, before it is
//!   buffered. A longer line is not forwarded: the client gets a -32600 error
//!   with `id: null` and the relay goes on. Malformed lines pass untouched
//!   (the session answers them), so a bad line never ends the relay.
//! - Stdout carries only protocol lines. It never writes terminal escape
//!   codes and never installs the TUI or its panic hook: `main` dispatches
//!   here before any of that exists.

use std::{
    ffi::OsString,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use serde_json::Value;

use crate::core::ai::tools::mcp::MAX_LINE_BYTES;

/// The subcommand the engine's MCP config launches.
pub const SUBCOMMAND: &str = "__mcp-serve";

/// The environment variable that carries the session token.
pub const TOKEN_ENV: &str = "LAZAROBOX_MCP_TOKEN";

/// How long the session has to answer the handshake. It answers at once, so
/// this only bounds a wedged peer.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);

/// The longest handshake answer. The real one is 12 bytes.
const REPLY_MAX: usize = 512;

/// Sent to the client, `id: null`, in place of a line over the cap.
const TOO_LONG: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"line too long\"}}\n";

/// Why the shim stopped before it could relay.
#[derive(Debug)]
pub enum Failure {
    Usage,
    NoToken,
    Connect(io::Error),
    Refused,
    TimedOut,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Usage => write!(f, "usage: lazarobox-shell {SUBCOMMAND} <socket>"),
            Failure::NoToken => write!(f, "{TOKEN_ENV} is not set"),
            Failure::Connect(e) => write!(f, "cannot reach the lazarobox session: {e}"),
            Failure::Refused => f.write_str("the lazarobox session refused the connection"),
            Failure::TimedOut => f.write_str("the lazarobox session did not answer in time"),
        }
    }
}

impl Failure {
    /// 2 for a misuse of the command, 1 when the session could not be used.
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Usage | Failure::NoToken => 2,
            Failure::Connect(_) | Failure::Refused | Failure::TimedOut => 1,
        }
    }
}

/// The one argument: a non-empty socket path.
pub fn parse_args(args: &[OsString]) -> Option<PathBuf> {
    match args {
        [socket] if !socket.is_empty() => Some(PathBuf::from(socket)),
        _ => None,
    }
}

/// Entry point of `__mcp-serve`: the exit status of the process.
pub fn main(args: &[OsString]) -> i32 {
    let token = std::env::var(TOKEN_ENV).ok();
    match run(args, token.as_deref()) {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("lazarobox mcp: {failure}");
            failure.exit_code()
        }
    }
}

/// Connects to the socket in `args` and relays stdin/stdout through it.
pub fn run(args: &[OsString], token: Option<&str>) -> Result<(), Failure> {
    let socket = parse_args(args).ok_or(Failure::Usage)?;
    let token = token.filter(|t| !t.is_empty()).ok_or(Failure::NoToken)?;
    let sock = UnixStream::connect(socket).map_err(Failure::Connect)?;
    serve(sock, token, HANDSHAKE_WAIT, io::stdin(), io::stdout())
}

/// One line read with a cap, as bytes: the shim forwards without judging.
#[derive(Debug, PartialEq)]
enum Line {
    Bytes(Vec<u8>),
    /// Longer than the cap; already skipped to its newline.
    Oversize,
    Eof,
}

/// Reads one line of at most `max` bytes (newline excluded), buffering at most
/// `max + 2` whatever the peer sends. An unterminated tail at EOF is dropped.
fn read_line<R: BufRead>(reader: &mut R, max: usize) -> io::Result<Line> {
    let mut buf = Vec::new();
    let read = (&mut *reader)
        .take(max as u64 + 2)
        .read_until(b'\n', &mut buf)?;
    if read == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(if buf.len() > max {
            Line::Oversize
        } else {
            Line::Bytes(buf)
        });
    }
    if read < max + 2 {
        return Ok(Line::Eof);
    }
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(Line::Eof);
        }
        match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => {
                reader.consume(i + 1);
                return Ok(Line::Oversize);
            }
            None => {
                let all = chunk.len();
                reader.consume(all);
            }
        }
    }
}

/// Sends the handshake and waits for `{"ok":true}`.
fn handshake(
    sock: &UnixStream,
    reader: &mut impl BufRead,
    token: &str,
    wait: Duration,
) -> Result<(), Failure> {
    let hello = serde_json::json!({"lazarobox": 1, "token": token});
    let mut line = hello.to_string();
    line.push('\n');
    let mut writer = sock;
    writer
        .write_all(line.as_bytes())
        .map_err(|_| Failure::Refused)?;
    sock.set_read_timeout(Some(wait))
        .map_err(Failure::Connect)?;
    // Not `read_line`: an answer past the cap is refused on the spot, not
    // skipped to its newline (which a wedged peer would never send).
    let mut answer = Vec::new();
    match reader
        .take(REPLY_MAX as u64 + 1)
        .read_until(b'\n', &mut answer)
    {
        Ok(_) if answer.last() == Some(&b'\n') => {}
        Ok(_) => return Err(Failure::Refused),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return Err(Failure::TimedOut);
        }
        Err(_) => return Err(Failure::Refused),
    }
    sock.set_read_timeout(None).map_err(Failure::Connect)?;
    match serde_json::from_slice::<Value>(&answer) {
        Ok(Value::Object(reply)) if reply.get("ok") == Some(&Value::Bool(true)) => Ok(()),
        _ => Err(Failure::Refused),
    }
}

/// Copies lines from `from` to `to` until either ends. A line over the cap is
/// answered to the client through `reply` instead of being forwarded.
fn copy_lines<R: BufRead, W: Write>(
    from: &mut R,
    mut to: impl FnMut(&[u8]) -> io::Result<()>,
    answer: &mut W,
) {
    while let Ok(line) = read_line(from, MAX_LINE_BYTES) {
        let sent = match line {
            Line::Bytes(mut bytes) => {
                bytes.push(b'\n');
                to(&bytes)
            }
            Line::Oversize => answer.write_all(TOO_LONG).and_then(|()| answer.flush()),
            Line::Eof => return,
        };
        if sent.is_err() {
            return;
        }
    }
}

/// A shared handle on the client's stdout: both directions write to it.
struct Shared<O>(Arc<Mutex<O>>);

impl<O> Clone for Shared<O> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

impl<O: Write> Write for Shared<O> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_all(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

/// The handshake, then the relay, until stdin or the session ends. Returns
/// once the first direction is done: the other may be blocked in a read that
/// only process exit can interrupt.
pub fn serve<I, O>(
    sock: UnixStream,
    token: &str,
    wait: Duration,
    input: I,
    output: O,
) -> Result<(), Failure>
where
    I: Read + Send + 'static,
    O: Write + Send + 'static,
{
    let mut from_app = BufReader::new(sock.try_clone().map_err(Failure::Connect)?);
    handshake(&sock, &mut from_app, token, wait)?;

    let out = Shared(Arc::new(Mutex::new(output)));
    let (done, ended) = mpsc::channel::<()>();

    let mut to_app = sock.try_clone().map_err(Failure::Connect)?;
    let (mut answer, done_in) = (out.clone(), done.clone());
    thread::spawn(move || {
        let mut input = BufReader::new(input);
        copy_lines(
            &mut input,
            |line| to_app.write_all(line).and_then(|()| to_app.flush()),
            &mut answer,
        );
        let _ = done_in.send(());
    });

    let (mut to_client, mut answer) = (out.clone(), out);
    thread::spawn(move || {
        copy_lines(
            &mut from_app,
            |line| to_client.write_all(line).and_then(|()| to_client.flush()),
            &mut answer,
        );
        let _ = done.send(());
    });

    let _ = ended.recv();
    let _ = sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests;
