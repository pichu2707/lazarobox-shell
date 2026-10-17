//! The tool host: runs the calls a model makes during a turn and tells the
//! chat about each one.
//!
//! One [`ToolHost`] serves the app. For each turn the engine adapter takes a
//! [`Dispatch`] from [`ToolHost::dispatch`], bound to that turn's event
//! channel, engine locality and origin directory. The MCP session hands every
//! `tools/call` of a CLI engine to it; the Ollama loop will call it directly.
//!
//! For each call the host emits exactly one `ToolCall` and then exactly one
//! `ToolResult` on the turn's channel, so the transcript rows come from here
//! and never from parsing text. Free calls run at once. A call that needs the
//! user (see [`ask_for`]) goes to the turn's [`Approver`] first; nothing
//! about it runs until the answer is `Sent`. Which calls need the user is D17:
//! the pure [`ask_for`] decides, from the call and the engine's locality.
//!
//! A panic in a call is caught by the MCP session, which exempts the panic
//! hook around each poll (`mcp::scoped`); the Ollama loop will poll the
//! dispatch inside `chat::scoped`. The host catches nothing itself.

use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::mpsc;

use super::{Limits, ToolEnv, run};
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, ask_for},
            engine::{CallId, Decision, StreamEvent, ToolStatus},
        },
        tools::{Call, Locality, command, summary::clip},
    },
    runtime::mcp::{Dispatch, Outcome},
};

/// What the model is told when a call needs the user and no approver is
/// wired.
pub const NOT_APPROVED: &str = "the user did not approve it";

/// Where a call that needs the user's answer waits for it. The future is
/// dropped when the call is cancelled or its chat closes; a timer never
/// answers `Sent`.
pub trait Approver: Send + Sync {
    fn ask(&self, id: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Decision>;
}

/// The approver of a turn that has none: it denies every call. Slice 2e-2
/// replaces it with the one that waits for the chat's approval overlay (and,
/// in `auto` mode, for the app to type the command, D18); the host does not
/// change, because the mode is applied by the app when the `ToolCall` arrives.
pub struct Unwired;

impl Approver for Unwired {
    fn ask(&self, _: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Decision> {
        async { Decision::Denied(NOT_APPROVED) }.boxed()
    }
}

/// What one turn gives the host.
pub struct TurnScope {
    /// The turn's event channel: where the `ToolCall` and `ToolResult` go.
    pub events: mpsc::Sender<StreamEvent>,
    /// Where the engine of the turn sends what a tool returns.
    pub locality: Locality,
    /// The chat's origin directory, against which a relative path is read.
    pub origin: PathBuf,
    pub approver: Arc<dyn Approver>,
}

impl TurnScope {
    /// A scope whose calls that need the user are denied.
    pub fn new(events: mpsc::Sender<StreamEvent>, locality: Locality, origin: PathBuf) -> Self {
        Self {
            events,
            locality,
            origin,
            approver: Arc::new(Unwired),
        }
    }
}

/// Runs tool calls for every chat of the app.
pub struct ToolHost {
    home: Option<OsString>,
    xdg_config_home: Option<OsString>,
    xdg_data_home: Option<OsString>,
    limits: Limits,
    next: AtomicU64,
}

impl ToolHost {
    /// A host that reads the paths of the running process.
    pub fn from_env() -> Arc<Self> {
        Self::new(
            std::env::var_os("HOME"),
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("XDG_DATA_HOME"),
            Limits::default(),
        )
    }

    pub fn new(
        home: Option<OsString>,
        xdg_config_home: Option<OsString>,
        xdg_data_home: Option<OsString>,
        limits: Limits,
    ) -> Arc<Self> {
        Arc::new(Self {
            home,
            xdg_config_home,
            xdg_data_home,
            limits,
            next: AtomicU64::new(1),
        })
    }

    /// The dispatch of one turn.
    pub fn dispatch(self: &Arc<Self>, scope: TurnScope) -> Arc<dyn Dispatch> {
        Arc::new(TurnDispatch {
            host: self.clone(),
            scope: Arc::new(scope),
        })
    }
}

struct TurnDispatch {
    host: Arc<ToolHost>,
    scope: Arc<TurnScope>,
}

impl Dispatch for TurnDispatch {
    fn call(&self, call: Call) -> BoxFuture<'static, Outcome> {
        execute(self.host.clone(), self.scope.clone(), call).boxed()
    }
}

/// What a result row says when the call was dropped before its answer.
const CANCELLED: &str = "cancelled";

/// The one `ToolResult` a call owes the chat. Dropping it unanswered (the
/// call was cancelled, or its chat closed) sends `denied` without waiting, so
/// a `ToolCall` is never left without its result.
struct Owed {
    id: CallId,
    events: mpsc::Sender<StreamEvent>,
    answered: bool,
}

impl Owed {
    async fn answer(mut self, status: ToolStatus, summary: &str) {
        self.answered = true;
        let result = StreamEvent::ToolResult {
            id: self.id,
            status,
            summary: summary.to_owned(),
        };
        // A closed channel means the turn is gone: nobody is left to tell.
        let _ = self.events.send(result).await;
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.events.try_send(StreamEvent::ToolResult {
                id: self.id,
                status: ToolStatus::Denied,
                summary: CANCELLED.to_owned(),
            });
        }
    }
}

/// A result that is the end of the call: the text for the model, and the
/// status and one line for the row.
fn ended(status: ToolStatus, text: String, summary: &str) -> (Outcome, ToolStatus, String) {
    let is_error = status != ToolStatus::Ok;
    (Outcome { text, is_error }, status, clip(summary))
}

async fn execute(host: Arc<ToolHost>, scope: Arc<TurnScope>, call: Call) -> Outcome {
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    // The command is the sanitizer's, so the ask shows what would be typed.
    let checked = match &call {
        Call::RunInPane { command } => command::validate(command)
            .map(|valid| Call::RunInPane {
                command: valid.as_str().to_owned(),
            })
            .map_err(|refusal| refusal.reason()),
        other => Ok(other.clone()),
    };
    let ask = checked
        .as_ref()
        .ok()
        .and_then(|call| ask_for(call, scope.locality));
    let announce = StreamEvent::ToolCall {
        id,
        name: call.name().to_owned(),
        summary: clip(call.argument()),
        approval: ask.clone().map(Box::new),
    };
    if scope.events.send(announce).await.is_err() {
        return Outcome {
            text: CANCELLED.to_owned(),
            is_error: true,
        };
    }
    let owed = Owed {
        id,
        events: scope.events.clone(),
        answered: false,
    };

    let (outcome, status, summary) = match checked {
        Err(reason) => ended(ToolStatus::Error, reason.to_owned(), reason),
        Ok(call) => {
            let decision = match &ask {
                Some(ask) => scope.approver.ask(id, ask).await,
                None => Decision::Sent,
            };
            match (decision, call) {
                (Decision::Denied(reason), _) => {
                    ended(ToolStatus::Denied, format!("denied: {reason}"), reason)
                }
                (Decision::Sent, Call::RunInPane { .. }) => ended(
                    ToolStatus::Ok,
                    "The command was typed into the user's pane. Enter was not pressed: \
                     the user runs it themselves."
                        .to_owned(),
                    "typed, no Enter",
                ),
                (Decision::Sent, call) => {
                    let env = ToolEnv {
                        home: host.home.clone(),
                        xdg_config_home: host.xdg_config_home.clone(),
                        xdg_data_home: host.xdg_data_home.clone(),
                        origin: scope.origin.clone(),
                    };
                    match run(call, env, host.limits.clone()).await {
                        Ok(text) => {
                            let summary = format!("{} bytes", text.len());
                            ended(ToolStatus::Ok, text, &summary)
                        }
                        Err(failure) => {
                            let reason = failure.message();
                            ended(ToolStatus::Error, reason.to_owned(), reason)
                        }
                    }
                }
            }
        }
    };
    owed.answer(status, &summary).await;
    outcome
}

#[cfg(test)]
mod tests;
