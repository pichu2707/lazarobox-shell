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
//!
//! Drain. When the client's read side ends (the shim half-closes when its
//! stdin ends) the connection stops reading but lets the calls in flight
//! finish and their replies flush, for at most [`DRAIN_GRACE`]; then it
//! aborts what is left. A client that hung up for good (both directions) and
//! `Hub::close` end the wait at once. Nothing is read during the drain, so a
//! `notifications/cancelled` sent after EOF is not seen: that call keeps
//! running until it finishes or the grace ends. Accepted.
//!
//! Limits, per session: [`MAX_PENDING`] connections may wait for their
//! handshake (a newcomer beyond that is dropped at once, so silent clients
//! cannot starve descriptors), and [`MAX_CONNS`] may be authenticated (a
//! newcomer beyond that is closed after a valid handshake, without the `ok`).
//! Per connection, [`MAX_IN_FLIGHT`] calls may be pending: one more is
//! answered at once with error [`TOO_BUSY`] (-32000) echoing its id, and so is
//! a `tools/call` whose id is still in flight (-32600). A dispatch that panics,
//! before or inside its future, answers an `isError` result with a generic
//! text. `Hub::close` sets a flag before aborting anything, and a connection
//! checks it before every dispatch, so nothing dispatches after `close`
//! returns even though the abort itself lands a moment later.
//!
//! Sweep caveat: liveness is judged by the process `comm` (`alive_by_comm`:
//! `/proc/<pid>/comm`, or `/bin/ps` where there is no `/proc`; compared by file
//! name, and a process that cannot be checked is kept), so an instance in
//! another PID namespace that shares the folder looks dead and loses its
//! sockets. The same limit as the chat CLI's own sweep.

use std::{collections::HashMap, fs, io, os::unix::fs::PermissionsExt, path::Path, path::PathBuf};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::FutureExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Interest},
    net::{UnixListener, UnixStream},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::{AbortHandle, JoinHandle, JoinSet},
    time::timeout,
};

use super::{Dispatch, Outcome, Token, external::Slot, guarded_call};
use crate::{
    core::{
        ai::tools::{
            ToolSpec,
            mcp::{Action, Frame, MAX_LINE_BYTES, Server, tool_result},
            registry,
            registry::FamilySet,
            registry_with,
            wiring::Unwire,
        },
        layout::PaneId,
    },
    runtime::chat::{alive_by_comm, ensure_private_dir, own_comm, read_comm},
};

#[cfg(test)]
mod tests;

/// The longest handshake line. The real one is about 60 bytes.
const HELLO_MAX: usize = 512;

/// How long a new connection has to present its handshake. The shim sends
/// it as its first act, so a client that is silent for 2 s is not one.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the calls in flight may keep running after the client stopped
/// sending (its read side hit EOF) so their replies still go out. It is the
/// time the shim waits for those replies after its stdin ends. Past it the
/// calls are aborted.
const DRAIN_GRACE: Duration = super::shim::DRAIN_GRACE;

/// Connections per session that may wait for their handshake at once.
const MAX_PENDING: usize = 8;

/// Authenticated connections per session at once.
const MAX_CONNS: usize = 4;

/// `tools/call` requests in flight per connection.
const MAX_IN_FLIGHT: usize = 32;

/// The JSON-RPC error for a call over [`MAX_IN_FLIGHT`] (implementation range).
const TOO_BUSY: i64 = -32000;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;

/// What the app hands to the engine adapter for a started session.
#[derive(Debug)]
pub struct Started {
    pub socket: PathBuf,
    pub token: Token,
    /// Ends this session from outside the hub (see [`Unwire`]).
    pub end: Unwire,
}

struct Live {
    socket: PathBuf,
    /// The accept loop. It owns the listener and the connection tasks, so
    /// ending it ends them all.
    task: JoinHandle<()>,
    /// Set by `close` before anything else, so no connection dispatches after.
    closed: Arc<AtomicBool>,
}

/// The sessions of this run, one per chat pane. Needs a tokio runtime.
pub struct Hub {
    dir: PathBuf,
    sessions: HashMap<PaneId, Live>,
    handshake: Duration,
    drain: Duration,
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
        let own = own_comm();
        if own.is_some() {
            sweep(&dir, &|pid| {
                alive_by_comm(pid, own.as_deref(), &|p| read_comm(&p.to_string()))
            });
        }
        Ok(Hub {
            dir,
            sessions: HashMap::new(),
            handshake: HANDSHAKE_TIMEOUT,
            drain: DRAIN_GRACE,
            // SAFETY: `geteuid` has no preconditions and cannot fail.
            uid: unsafe { libc::geteuid() },
        })
    }

    /// Starts the session of `pane`, retiring any earlier one (its token stops
    /// working). `dispatch` is where this chat's calls go.
    ///
    /// `external` is where the turn puts its user servers once they run: each
    /// connection offers (and routes) their tools besides the built-ins.
    ///
    /// The built-ins offered are `registry_with(&[], families)` and the same
    /// set gates the parse, so what is listed and what is accepted agree.
    pub fn open(
        &mut self,
        pane: PaneId,
        dispatch: Arc<dyn Dispatch>,
        external: Slot,
        families: FamilySet,
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
        let mut conn = Conn::new(
            token.clone(),
            registry_with(&[], families),
            dispatch,
            external,
            self.uid,
            self.handshake,
            self.drain,
        );
        // Not a `Conn::new` argument: it has seven already.
        conn.families = families;
        let closed = conn.closed.clone();
        let task = tokio::spawn(accept(listener, conn));
        // The same steps as `close`, from a handle: the flag first, so it is
        // a no-op once the session was closed or replaced (the next session
        // of the pane binds the same path, and must keep it).
        let end = {
            let (closed, abort, socket) = (closed.clone(), task.abort_handle(), socket.clone());
            Unwire::new(move || {
                if !closed.swap(true, Ordering::SeqCst) {
                    abort.abort();
                    let _ = fs::remove_file(&socket);
                }
            })
        };
        self.sessions.insert(
            pane,
            Live {
                socket: socket.clone(),
                task,
                closed,
            },
        );
        Ok(Started { socket, token, end })
    }

    /// Ends the session of `pane`: its socket goes, live connections and calls
    /// in flight are dropped. Nothing happens for a pane without one.
    pub fn close(&mut self, pane: PaneId) {
        if let Some(live) = self.sessions.remove(&pane) {
            live.closed.store(true, Ordering::SeqCst);
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
    external: Slot,
    families: FamilySet,
    uid: u32,
    handshake: Duration,
    drain: Duration,
    closed: Arc<AtomicBool>,
    pending: Arc<Semaphore>,
    live: Arc<Semaphore>,
}

impl Conn {
    fn new(
        token: Token,
        tools: Vec<ToolSpec>,
        dispatch: Arc<dyn Dispatch>,
        external: Slot,
        uid: u32,
        handshake: Duration,
        drain: Duration,
    ) -> Conn {
        Conn {
            token: Arc::new(token),
            tools,
            dispatch,
            external,
            families: FamilySet::LEGACY,
            uid,
            handshake,
            drain,
            closed: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Semaphore::new(MAX_PENDING)),
            live: Arc::new(Semaphore::new(MAX_CONNS)),
        }
    }
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
                    if let Ok(permit) = conn.pending.clone().try_acquire_owned() {
                        conns.spawn(serve(stream, conn.clone(), permit));
                    }
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
pub(super) enum Line {
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
pub(super) async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> io::Result<Line> {
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

/// What a call that panicked answers: nothing of the panic leaks.
fn failed() -> Outcome {
    Outcome {
        text: "the tool failed".to_owned(),
        is_error: true,
    }
}

fn wire(value: &Value) -> String {
    format!("{value}\n")
}

fn refusal(code: i64, message: &str) -> String {
    refusal_for(Value::Null, code, message)
}

fn refusal_for(id: Value, code: i64, message: &str) -> String {
    wire(
        &Frame::Error {
            id,
            code,
            message: message.to_owned(),
        }
        .to_value(),
    )
}

/// Serves one connection. Every way out just returns: the stream closes, and
/// the tasks of the connection (the writer, the calls in flight) are aborted
/// with the set that owns them.
async fn serve(stream: UnixStream, conn: Conn, pending: OwnedSemaphorePermit) {
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
    drop(pending);
    // Over the cap: closed without the `ok`, so the shim sees a refusal.
    let Ok(_slot) = conn.live.clone().try_acquire_owned() else {
        return;
    };
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
    // The user servers of the turn, if they are up by now.
    let external = conn
        .external
        .get()
        .map(|set| set.qualified().clone())
        .unwrap_or_default();
    let mut tools = conn.tools;
    tools.extend(
        registry(&external.external())
            .into_iter()
            .filter(|tool| external.raw_of(&tool.name).is_some()),
    );
    let mut server = Server::with_external(tools, external).with_families(conn.families);
    let mut in_flight: HashMap<String, AbortHandle> = HashMap::new();
    let mut drain = false;
    loop {
        while tasks.try_join_next().is_some() {}
        let reply = match read_line(&mut reader, MAX_LINE_BYTES).await {
            Ok(Line::Eof) => {
                drain = true;
                break;
            }
            Err(_) => break,
            Ok(Line::Oversize) => Some(refusal(INVALID_REQUEST, "line too long")),
            Ok(Line::Invalid) => Some(refusal(PARSE_ERROR, "invalid UTF-8")),
            Ok(Line::Text(line)) => match server.handle(&line) {
                Action::None => None,
                Action::Reply(value) => Some(wire(&value)),
                Action::Call { id, call } => {
                    in_flight.retain(|_, handle| !handle.is_finished());
                    let key = id.to_string();
                    if conn.closed.load(Ordering::SeqCst) {
                        break;
                    } else if in_flight.contains_key(&key) {
                        Some(refusal_for(
                            id,
                            INVALID_REQUEST,
                            "request id already in flight",
                        ))
                    } else if in_flight.len() >= MAX_IN_FLIGHT {
                        Some(refusal_for(id, TOO_BUSY, "too many calls in flight"))
                    } else {
                        // A panic in the dispatch must still answer the call.
                        let work =
                            catch_unwind(AssertUnwindSafe(|| guarded_call(&*conn.dispatch, call)))
                                .unwrap_or_else(|_| async { failed() }.boxed());
                        let out = out.clone();
                        let handle = tasks.spawn(async move {
                            let done = AssertUnwindSafe(work)
                                .catch_unwind()
                                .await
                                .unwrap_or_else(|_| failed());
                            let _ = out
                                .send(wire(&tool_result(&id, &done.text, done.is_error)))
                                .await;
                        });
                        in_flight.insert(key, handle);
                        None
                    }
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
    if drain {
        drain_calls(tasks, out, reader.get_ref().as_ref(), conn.drain).await;
    }
}

/// How often a drain looks for a client that hung up.
const HANGUP_POLL: Duration = Duration::from_millis(100);

/// The client stopped sending but may still be reading (the shim half-closes
/// when its stdin ends). Lets the calls in flight finish and the writer flush
/// their replies, for at most `grace`; the writer ends once the last call has
/// dropped its sender. A hangup, `Hub::close` (which aborts this whole task)
/// or the end of the grace stops the wait, and returning drops `tasks`, which
/// aborts whatever is left.
async fn drain_calls(
    mut tasks: JoinSet<()>,
    out: mpsc::Sender<String>,
    stream: &UnixStream,
    grace: Duration,
) {
    drop(out);
    let finished = async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        _ = timeout(grace, finished) => {}
        _ = hung_up(stream) => {}
    }
}

/// Resolves once the peer has closed both directions, not just its writing
/// one. Tokio reports that as a write-closed readiness (`EPOLLHUP`), which
/// has no waiter to wake, so it is looked at every [`HANGUP_POLL`].
async fn hung_up(stream: &UnixStream) {
    loop {
        match stream.ready(Interest::READABLE | Interest::WRITABLE).await {
            Ok(ready) if !ready.is_write_closed() => {}
            _ => return,
        }
        tokio::time::sleep(HANGUP_POLL).await;
    }
}
