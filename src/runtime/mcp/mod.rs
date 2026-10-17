//! The app side of the MCP transport: one private unix socket per chat, behind
//! a session token. A CLI engine reaches the tool registry through a stdio
//! shim ([`shim`], `lazarobox-shell __mcp-serve <socket>`) that relays to the
//! socket of its chat.
//!
//! Trust model:
//!
//! - The socket lives in a directory of ours (0700, owned by us) and is 0600.
//!   Another user cannot open it; the peer UID is checked as well.
//! - Reaching the socket is not enough. The first line must be the handshake
//!   `{"lazarobox":1,"token":"<hex>"}` carrying the session token (128 random
//!   bits, compared in constant time). A wrong or missing token, a late or
//!   oversized handshake, or a foreign UID closes the connection without a
//!   byte of reply and without calling the [`Dispatch`].
//! - A session is one chat's. Its token only opens its own socket, and the
//!   [`Dispatch`] it was opened with only knows that chat. Closing the chat
//!   removes the socket, drops the live connections and every call in flight.
//! - The token is handed to the engine through its environment
//!   ([`shim::TOKEN_ENV`]), never argv; the shim reads it from there.
//!   Nothing in this module logs: it has no diagnostics that could carry it,
//!   and [`Token`] prints as `Token(..)`.
//! - After the handshake the lines are MCP (see `core::ai::tools::mcp`). The
//!   session caps the bytes it buffers per line before it builds a string:
//!   see `session` for the policy.

mod session;
pub mod shim;

use std::{
    cell::Cell,
    ffi::OsStr,
    fmt,
    fs::File,
    future::Future,
    io::{self, Read},
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use futures::future::BoxFuture;

use crate::core::ai::tools::Call;
pub use session::{Hub, Started};

/// What the host made of one tool call: the text for the model and whether it
/// is an error.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub text: String,
    pub is_error: bool,
}

/// Where a session sends the calls of its chat. One per session, so the host
/// behind it only ever sees that chat. The future is dropped when the client
/// cancels the request, the connection ends or the chat closes: that is how a
/// pending approval is withdrawn.
pub trait Dispatch: Send + Sync {
    fn call(&self, call: Call) -> BoxFuture<'static, Outcome>;
}

thread_local! {
    /// How many dispatch calls or polls are running on this thread right now.
    static IN_DISPATCH: Cell<usize> = const { Cell::new(0) };
}

/// Whether the current thread is inside a [`Dispatch`] call or a poll of its
/// future. The panic hook reads it: the session catches a panic there and
/// answers the call with an error result, so the terminal must stay intact.
/// Same mechanism as `chat::in_adapter_poll`: per poll, not per task, because
/// a task can move between threads and the hook runs where the panic happens.
pub fn in_dispatch() -> bool {
    IN_DISPATCH.with(|depth| depth.get() > 0)
}

/// Counts one dispatch scope in; the drop counts it out, also on unwind.
struct DispatchGuard;

impl DispatchGuard {
    fn enter() -> Self {
        IN_DISPATCH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        IN_DISPATCH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Wraps a dispatch future so [`in_dispatch`] is true during each poll.
pub fn scoped<F: Future>(inner: F) -> Scoped<F> {
    Scoped(Box::pin(inner))
}

pub struct Scoped<F>(Pin<Box<F>>);

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let _guard = DispatchGuard::enter();
        self.0.as_mut().poll(cx)
    }
}

/// `dispatch.call(call)` with [`in_dispatch`] set around the synchronous part
/// and around every poll of the future it returns.
fn guarded_call(dispatch: &dyn Dispatch, call: Call) -> BoxFuture<'static, Outcome> {
    let _guard = DispatchGuard::enter();
    Box::pin(scoped(dispatch.call(call)))
}

/// The secret of one session: 128 random bits as 32 lowercase hex digits.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    /// A fresh token from `/dev/urandom`. Fails closed: with no randomness
    /// there is no session.
    pub fn generate() -> io::Result<Token> {
        let mut bytes = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        Ok(Token(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// The token text, for the one place that hands it to the engine.
    pub fn reveal(&self) -> &str {
        &self.0
    }

    /// Whether `other` is this token. The time taken does not depend on where
    /// they first differ (only on the length, which is public).
    fn matches(&self, other: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), other.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

/// The folders to try for the sockets, best first: `$XDG_RUNTIME_DIR/lazarobox`
/// when that variable is an absolute path, then `<data>/run`.
pub fn candidate_dirs(xdg_runtime: Option<&OsStr>, data: Option<&Path>) -> Vec<PathBuf> {
    let runtime = xdg_runtime
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("lazarobox"));
    runtime
        .into_iter()
        .chain(data.map(|d| d.join("run")))
        .collect()
}

#[cfg(test)]
mod tests;
