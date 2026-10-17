//! The listener and the hub: one socket per chat, one task per connection.
//!
//! Line policy. The wire carries newline-delimited JSON, and
//! `Server::handle` takes a `&str`, so the cap has to hold before the string
//! exists. [`read_line`] reads at most `max + 2` bytes of a line into a
//! buffer; a longer line is skipped to its newline without being stored, the
//! client gets the oversize error (`id: null`) and the connection goes on.
//! Resyncing rather than closing is what the spec asks for ("invalid JSON,
//! then a message over the cap, then a valid call: the third is served") and
//! costs nothing: memory stays at one line per connection either way. The
//! handshake line is held to [`HELLO_MAX`] and, being unauthenticated, to a
//! timeout; failing either closes the connection silently.
//!
//! A `tools/call` runs as its own task, so a call waiting on the user's `y`
//! does not stop the connection from reading `notifications/cancelled`.
//! Replies from every task go through one writer task.

use std::{collections::HashMap, fs, io, os::unix::fs::PermissionsExt, path::Path, path::PathBuf};
use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc,
    task::{AbortHandle, JoinHandle, JoinSet},
    time::timeout,
};

use super::{Dispatch, Token};
use crate::{
    core::{
        ai::tools::{
            ToolSpec,
            mcp::{Action, Frame, MAX_LINE_BYTES, Server, tool_result},
        },
        layout::PaneId,
    },
    runtime::chat::{alive_by_comm, ensure_private_dir, read_comm},
};

#[cfg(test)]
mod tests;

/// The longest handshake line. The real one is about 60 bytes.
const HELLO_MAX: usize = 512;

/// How long a new connection has to present its handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;

/// What the app hands to the engine adapter for a started session.
#[derive(Debug)]
pub struct Started {
    pub socket: PathBuf,
    pub token: Token,
}

struct Live {
    socket: PathBuf,
    /// The accept loop. It owns the listener and the connection tasks, so
    /// ending it ends them all.
    task: JoinHandle<()>,
}

/// The sessions of this run, one per chat pane. Needs a tokio runtime.
pub struct Hub {
    dir: PathBuf,
    sessions: HashMap<PaneId, Live>,
    handshake: Duration,
    /// The only UID whose connections are served.
    uid: u32,
}

impl Hub {
    /// Uses the first of `dirs` that is (or can be made) a private folder of
    /// ours, and sweeps the sockets of dead runs out of it.
    pub fn new(dirs: &[PathBuf]) -> io::Result<Hub> {
        let dir = dirs
            .iter()
            .find(|dir| ensure_private_dir(dir).is_ok())
            .ok_or_else(|| io::Error::other("no private folder for the sockets"))?
            .clone();
        // Without our own `comm` nobody can be told apart: sweep nothing.
        let own = read_comm("self");
        if own.is_some() {
            sweep(&dir, &|pid| {
                alive_by_comm(pid, own.as_deref(), &|p| read_comm(&p.to_string()))
            });
        }
        Ok(Hub {
            dir,
            sessions: HashMap::new(),
            handshake: HANDSHAKE_TIMEOUT,
            // SAFETY: `geteuid` has no preconditions and cannot fail.
            uid: unsafe { libc::geteuid() },
        })
    }

    /// Starts the session of `pane`, retiring any earlier one (its token stops
    /// working). `dispatch` is where this chat's calls go.
    pub fn open(
        &mut self,
        pane: PaneId,
        tools: Vec<ToolSpec>,
        dispatch: Arc<dyn Dispatch>,
    ) -> io::Result<Started> {
        self.close(pane);
        let token = Token::generate()?;
        let socket = self
            .dir
            .join(format!("s-{}-{}.sock", std::process::id(), pane.number()));
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        // The folder is ours and 0700, so nobody could connect in between.
        if let Err(e) = fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&socket);
            return Err(e);
        }
        let conn = Conn {
            token: Arc::new(token.clone()),
            tools,
            dispatch,
            uid: self.uid,
            handshake: self.handshake,
        };
        let task = tokio::spawn(accept(listener, conn));
        self.sessions.insert(
            pane,
            Live {
                socket: socket.clone(),
                task,
            },
        );
        Ok(Started { socket, token })
    }

    /// Ends the session of `pane`: its socket goes, live connections and calls
    /// in flight are dropped. Nothing happens for a pane without one.
    pub fn close(&mut self, pane: PaneId) {
        if let Some(live) = self.sessions.remove(&pane) {
            live.task.abort();
            let _ = fs::remove_file(&live.socket);
        }
    }
}

impl Drop for Hub {
    /// At exit: no socket stays behind.
    fn drop(&mut self) {
        let panes: Vec<PaneId> = self.sessions.keys().copied().collect();
        for pane in panes {
            self.close(pane);
        }
    }
}

/// What one connection needs to be served.
#[derive(Clone)]
struct Conn {
    token: Arc<Token>,
    tools: Vec<ToolSpec>,
    dispatch: Arc<dyn Dispatch>,
    uid: u32,
    handshake: Duration,
}

/// Removes `s-<pid>-<pane>.sock` files whose pid `alive` rejects. Anything
/// else in the folder stays.
fn sweep(dir: &Path, alive: &dyn Fn(u32) -> bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| n.strip_prefix("s-")?.strip_suffix(".sock"))
            .and_then(|n| n.split_once('-'))
            .filter(|(_, pane)| !pane.is_empty() && pane.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|(pid, _)| pid.parse::<u32>().ok());
        if pid.is_some_and(|pid| !alive(pid)) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

async fn accept(listener: UnixListener, conn: Conn) {
    // Dropped with this task: ends every connection of the session.
    let mut conns = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    conns.spawn(serve(stream, conn.clone()));
                }
                // Out of descriptors and the like: wait, do not end the session.
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            },
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
    }
}

/// One line off the wire.
#[derive(Debug, PartialEq)]
enum Line {
    Text(String),
    /// Longer than the cap; already skipped to its newline.
    Oversize,
    /// Not valid UTF-8.
    Invalid,
    Eof,
}

/// Reads one line of at most `max` bytes (newline excluded). Buffers at most
/// `max + 2` bytes whatever the peer sends. An unterminated tail at EOF is
/// dropped: nobody is left to answer it.
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R, max: usize) -> io::Result<Line> {
    let mut buf = Vec::new();
    let read = (&mut *reader)
        .take(max as u64 + 2)
        .read_until(b'\n', &mut buf)
        .await?;
    if read == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.len() > max {
            return Ok(Line::Oversize);
        }
        return Ok(String::from_utf8(buf).map_or(Line::Invalid, Line::Text));
    }
    if read < max + 2 {
        return Ok(Line::Eof);
    }
    // Over the cap with no newline yet: skip to it without keeping anything.
    loop {
        let chunk = reader.fill_buf().await?;
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

/// Whether `line` is the handshake of this session.
fn hello_ok(token: &Token, line: &str) -> bool {
    let Ok(Value::Object(hello)) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    hello.get("lazarobox").and_then(Value::as_u64) == Some(1)
        && hello
            .get("token")
            .and_then(Value::as_str)
            .is_some_and(|t| token.matches(t))
}

fn wire(value: &Value) -> String {
    format!("{value}\n")
}

fn refusal(code: i64, message: &str) -> String {
    wire(
        &Frame::Error {
            id: Value::Null,
            code,
            message: message.to_owned(),
        }
        .to_value(),
    )
}

/// Serves one connection. Every way out just returns: the stream closes, and
/// the tasks of the connection (the writer, the calls in flight) are aborted
/// with the set that owns them.
async fn serve(stream: UnixStream, conn: Conn) {
    if stream.peer_cred().map(|c| c.uid()).ok() != Some(conn.uid) {
        return;
    }
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let first = timeout(conn.handshake, read_line(&mut reader, HELLO_MAX)).await;
    match first {
        Ok(Ok(Line::Text(line))) if hello_ok(&conn.token, &line) => {}
        _ => return,
    }
    if write
        .write_all(wire(&json!({"ok": true})).as_bytes())
        .await
        .is_err()
    {
        return;
    }

    let mut tasks = JoinSet::new();
    let (out, mut replies) = mpsc::channel::<String>(16);
    tasks.spawn(async move {
        while let Some(line) = replies.recv().await {
            if write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut server = Server::new(conn.tools);
    let mut in_flight: HashMap<String, AbortHandle> = HashMap::new();
    loop {
        while tasks.try_join_next().is_some() {}
        let reply = match read_line(&mut reader, MAX_LINE_BYTES).await {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Oversize) => Some(refusal(INVALID_REQUEST, "line too long")),
            Ok(Line::Invalid) => Some(refusal(PARSE_ERROR, "invalid UTF-8")),
            Ok(Line::Text(line)) => match server.handle(&line) {
                Action::None => None,
                Action::Reply(value) => Some(wire(&value)),
                Action::Call { id, call } => {
                    in_flight.retain(|_, handle| !handle.is_finished());
                    let key = id.to_string();
                    let work = conn.dispatch.call(call);
                    let out = out.clone();
                    let handle = tasks.spawn(async move {
                        let done = work.await;
                        let _ = out
                            .send(wire(&tool_result(&id, &done.text, done.is_error)))
                            .await;
                    });
                    in_flight.insert(key, handle);
                    None
                }
                Action::Cancel { id } => {
                    if let Some(handle) = in_flight.remove(&id.to_string()) {
                        handle.abort();
                    }
                    None
                }
            },
        };
        if let Some(reply) = reply
            && out.send(reply).await.is_err()
        {
            break;
        }
    }
}
