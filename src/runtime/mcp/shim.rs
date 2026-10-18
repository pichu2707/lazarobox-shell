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
//! - The token is sent only to a socket that is ours. The path must be
//!   absolute; the socket and its parent are `lstat`ed first (the parent a
//!   real directory of ours, mode 0700; the socket a real socket of ours,
//!   neither a symlink); after `connect` the peer uid must be ours (`SO_PEERCRED`
//!   or `getpeereid`). The token itself must have the session's shape, 32
//!   lowercase hex digits. Any failure stops before a byte is sent.
//! - The handshake has one overall deadline, not one per read. When stdin
//!   closes first the shim half-closes the socket and keeps printing replies
//!   until the session closes or [`DRAIN_GRACE`] passes.
//! - Jail: the engine's rules for the shim are rx on the app binary (and its
//!   ELF loader) plus an `rw_file` rule on the socket. That rule binds to the
//!   socket's inode, so the socket is bound before the engine starts and a
//!   rebind invalidates it. Landlock ABI 9 adds
//!   `LANDLOCK_ACCESS_FS_RESOLVE_UNIX`; while the pinned landlock crate caps
//!   `from_all`, connect is ungated, but a crate that handles ABI 9 must grant
//!   that access on the socket rule or the shim stops connecting.
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
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::core::ai::tools::mcp::MAX_LINE_BYTES;

/// The subcommand the engine's MCP config launches.
pub const SUBCOMMAND: &str = "__mcp-serve";

/// The environment variable that carries the session token.
pub const TOKEN_ENV: &str = "LAZAROBOX_MCP_TOKEN";

/// How long the shim keeps relaying replies after its stdin closed.
pub const DRAIN_GRACE: Duration = Duration::from_secs(5);

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
    /// The token is not a session token (32 lowercase hex digits).
    BadToken,
    /// The socket is not a private socket of ours: nothing was sent.
    Untrusted,
    Refused,
    TimedOut,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Usage => write!(f, "usage: lazarobox-shell {SUBCOMMAND} <socket>"),
            Failure::NoToken => write!(f, "{TOKEN_ENV} is not set"),
            Failure::Connect(e) => write!(f, "cannot reach the lazarobox session: {e}"),
            Failure::BadToken => write!(f, "{TOKEN_ENV} is not a session token"),
            Failure::Untrusted => f.write_str("the socket is not a private socket of this user"),
            Failure::Refused => f.write_str("the lazarobox session refused the connection"),
            Failure::TimedOut => f.write_str("the lazarobox session did not answer in time"),
        }
    }
}

impl Failure {
    /// 2 for a misuse of the command, 1 when the session could not be used.
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Usage | Failure::NoToken | Failure::BadToken => 2,
            Failure::Connect(_) | Failure::Untrusted | Failure::Refused | Failure::TimedOut => 1,
        }
    }
}

/// The one argument: an absolute socket path. A relative one would depend on
/// the working directory the engine happens to have.
pub fn parse_args(args: &[OsString]) -> Option<PathBuf> {
    match args {
        [socket] if Path::new(socket).is_absolute() => Some(PathBuf::from(socket)),
        _ => None,
    }
}

/// What the preflight needs to know about one path, from `lstat`.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub is_dir: bool,
    pub is_socket: bool,
    pub uid: u32,
    pub mode: u32,
}

impl Facts {
    fn of(meta: &std::fs::Metadata) -> Facts {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        Facts {
            is_dir: meta.file_type().is_dir(),
            is_socket: meta.file_type().is_socket(),
            uid: meta.uid(),
            mode: meta.mode() & 0o777,
        }
    }
}

/// Whether `socket` and its `parent` may be trusted with the token: the
/// parent is a real directory of ours that only we can enter (0700), and the
/// socket is a socket of ours. Facts come from `lstat`, so a symlink is
/// neither a directory nor a socket here.
pub fn trusted(parent: Facts, socket: Facts, euid: u32) -> bool {
    parent.is_dir
        && parent.uid == euid
        && parent.mode == 0o700
        && socket.is_socket
        && socket.uid == euid
}

fn euid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// `lstat`s the socket and its parent and applies [`trusted`]. Nothing is
/// connected to before this passes.
fn preflight(socket: &Path) -> Result<(), Failure> {
    let parent = socket.parent().ok_or(Failure::Untrusted)?;
    let parent = std::fs::symlink_metadata(parent).map_err(Failure::Connect)?;
    let socket = std::fs::symlink_metadata(socket).map_err(Failure::Connect)?;
    if trusted(Facts::of(&parent), Facts::of(&socket), euid()) {
        Ok(())
    } else {
        Err(Failure::Untrusted)
    }
}

/// The uid of the process at the other end of `sock`.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn peer_uid(sock: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writes and `len` is its size.
    let rc = unsafe {
        libc::getsockopt(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(cred.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The uid of the process at the other end of `sock`.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
pub fn peer_uid(sock: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: both out-pointers are valid for writes.
    let rc = unsafe { libc::getpeereid(sock.as_raw_fd(), &mut uid, &mut gid) };
    if rc == 0 {
        Ok(uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether `token` has the shape the session issues: 32 lowercase hex digits.
fn token_shaped(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
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
    if !token_shaped(token) {
        return Err(Failure::BadToken);
    }
    // The token goes only to a socket that is ours: it is checked before
    // connecting and again, by peer uid, before sending.
    preflight(&socket)?;
    let sock = UnixStream::connect(socket).map_err(Failure::Connect)?;
    if peer_uid(&sock).map_err(|_| Failure::Untrusted)? != euid() {
        return Err(Failure::Untrusted);
    }
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

/// The outcome of a `set_*_timeout` call on the session's socket. macOS
/// answers `EINVAL` when the peer has already closed (the session may hang up
/// right after its `ok`); a read on such a socket cannot block, it returns what
/// is buffered or the end, so that error is not a failure.
fn timeout_set(set: io::Result<()>) -> io::Result<()> {
    match set {
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => Ok(()),
        other => other,
    }
}

/// Sends the handshake and waits for `{"ok":true}`. `wait` bounds the whole
/// exchange, not each read: a peer that trickles bytes still runs out of time.
fn handshake(
    sock: &UnixStream,
    reader: &mut impl BufRead,
    token: &str,
    wait: Duration,
) -> Result<(), Failure> {
    let deadline = Instant::now() + wait;
    let hello = serde_json::json!({"lazarobox": 1, "token": token});
    let mut line = hello.to_string();
    line.push('\n');
    sock.set_write_timeout(Some(wait.max(Duration::from_millis(1))))
        .map_err(Failure::Connect)?;
    let mut writer = sock;
    writer
        .write_all(line.as_bytes())
        .map_err(|_| Failure::Refused)?;
    sock.set_write_timeout(None).map_err(Failure::Connect)?;

    // Not `read_line`: an answer past the cap is refused on the spot, not
    // skipped to its newline (which a wedged peer would never send).
    let mut answer = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Failure::TimedOut);
        }
        timeout_set(sock.set_read_timeout(Some(left))).map_err(Failure::Connect)?;
        let chunk = match reader.fill_buf() {
            Ok([]) => return Err(Failure::Refused),
            Ok(chunk) => chunk,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(Failure::TimedOut);
            }
            Err(_) => return Err(Failure::Refused),
        };
        let take = chunk
            .iter()
            .position(|&b| b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        answer.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if answer.last() == Some(&b'\n') {
            break;
        }
        if answer.len() > REPLY_MAX {
            return Err(Failure::Refused);
        }
    }
    if answer.len() > REPLY_MAX + 1 {
        return Err(Failure::Refused);
    }
    timeout_set(sock.set_read_timeout(None)).map_err(Failure::Connect)?;
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

/// Which side of the relay ended first.
enum End {
    Stdin,
    Session,
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
    serve_with(sock, token, wait, DRAIN_GRACE, input, output)
}

/// [`serve`] with the grace period after stdin closes made explicit.
pub fn serve_with<I, O>(
    sock: UnixStream,
    token: &str,
    wait: Duration,
    grace: Duration,
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
    let (done, ended) = mpsc::channel::<End>();

    let mut to_app = sock.try_clone().map_err(Failure::Connect)?;
    let (mut answer, done_in) = (out.clone(), done.clone());
    thread::spawn(move || {
        let mut input = BufReader::new(input);
        copy_lines(
            &mut input,
            |line| to_app.write_all(line).and_then(|()| to_app.flush()),
            &mut answer,
        );
        let _ = done_in.send(End::Stdin);
    });

    let (mut to_client, mut answer) = (out.clone(), out);
    thread::spawn(move || {
        copy_lines(
            &mut from_app,
            |line| to_client.write_all(line).and_then(|()| to_client.flush()),
            &mut answer,
        );
        let _ = done.send(End::Session);
    });

    if let Ok(End::Stdin) = ended.recv() {
        // The client is done talking, not listening: half-close so the session
        // sees the end of the requests, and keep printing what it still sends.
        let _ = sock.shutdown(std::net::Shutdown::Write);
        let _ = ended.recv_timeout(grace);
    }
    let _ = sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests;
