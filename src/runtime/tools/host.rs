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
    borrow::Cow,
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::mpsc;

use super::{Limits, ToolEnv, stage::Stages};
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, ask_for},
            engine::{Answer, CallId, Decision, StreamEvent, ToolStatus},
        },
        tools::{
            Call, Locality, command,
            edit::EditError,
            label,
            paths::display_target,
            registry::FamilySet,
            report::{self, RunReport},
            stage::StageKey,
            summary::clip,
        },
    },
    runtime::mcp::{
        Dispatch, Outcome,
        external::{ExternalSet, Slot},
    },
};

/// What the model is told when a call needs the user and no approver is
/// wired.
pub const NOT_APPROVED: &str = "the user did not approve it";

/// Where a call that needs the user's answer waits for it. The future is
/// dropped when the call is cancelled or its chat closes; a timer never
/// answers `Sent`.
///
/// `ask` MUST register the wait synchronously, before it returns the future:
/// the host asks before it announces the `ToolCall`, and a decision may be
/// delivered the instant the announcement is seen. A wait registered only
/// when the future is first polled could miss it.
pub trait Approver: Send + Sync {
    fn ask(&self, id: CallId, ask: &ApprovalAsk) -> BoxFuture<'static, Answer>;
}

/// The approver of a turn that has none: it denies every call. A turn that
/// offers tools gets [`Waits::approver`](super::Waits::approver) instead, which
/// waits for the answer the chat's approval overlay reports (and, in `auto`
/// mode, for the app to type the command, D18); the host does not change,
/// because the mode is applied by the app when the `ToolCall` arrives.
pub struct Unwired;

impl Approver for Unwired {
    fn ask(&self, _: CallId, _: &ApprovalAsk) -> BoxFuture<'static, Answer> {
        async { Decision::Denied(NOT_APPROVED).into() }.boxed()
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
    /// The user's MCP servers this turn may call. Empty until the turn has
    /// started them: a server tool is then refused even after `y`.
    pub external: Slot,
    /// The families this turn was offered. The write family is checked again
    /// here: a call the parse should have refused is refused as unknown.
    pub families: FamilySet,
    /// Whose batch the write tools fill, and whose staged changes the read
    /// tools show. `None` for a turn that cannot stage.
    pub stage_key: Option<StageKey>,
    /// The paths (as shown) the user allowed a read or an edit of in this
    /// turn: a later `edit_file` of one does not ask again.
    pub allowed: Mutex<HashSet<String>>,
}

impl TurnScope {
    /// A scope whose calls that need the user are denied.
    pub fn new(events: mpsc::Sender<StreamEvent>, locality: Locality, origin: PathBuf) -> Self {
        Self {
            events,
            locality,
            origin,
            approver: Arc::new(Unwired),
            external: Slot::default(),
            families: FamilySet::LEGACY,
            stage_key: None,
            allowed: Mutex::default(),
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
    /// The staged batches of every chat: in memory, never saved.
    stages: Arc<Mutex<Stages>>,
    /// Who may run a command now, and what the sandbox is.
    exec: exec::Exec,
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
            stages: Arc::default(),
            exec: exec::Exec::system(),
        })
    }

    /// What a call may see, for the chat whose origin directory is `origin`.
    fn env(&self, origin: &Path) -> ToolEnv {
        ToolEnv {
            home: self.home.clone(),
            xdg_config_home: self.xdg_config_home.clone(),
            xdg_data_home: self.xdg_data_home.clone(),
            origin: origin.to_path_buf(),
        }
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
/// A run answered a call that is not a command: nothing ran.
const NOT_COMMAND: &str = "that call is not a command";
/// A server tool was approved but the turn has no running server for it.
const NO_SERVER: &str = "that tool is not available";

/// The one `ToolResult` a call owes the chat, with the channel slot for it
/// already reserved. The slot is taken before the `ToolCall` is announced, so
/// handing the result over never waits and never fails for lack of room: not
/// when the call finishes, and not when it is dropped (cancelled, or its chat
/// closed), which sends `denied` from the drop. A `ToolCall` is never left
/// without its result.
struct Owed {
    id: CallId,
    slot: Option<mpsc::OwnedPermit<StreamEvent>>,
}

impl Owed {
    fn answer(mut self, status: ToolStatus, summary: &str) {
        self.send(status, summary.to_owned());
    }

    fn send(&mut self, status: ToolStatus, summary: String) {
        // A closed channel means the turn is gone: nobody is left to tell.
        if let Some(slot) = self.slot.take() {
            slot.send(StreamEvent::ToolResult {
                id: self.id,
                status,
                summary,
            });
        }
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        self.send(ToolStatus::Denied, CANCELLED.to_owned());
    }
}

/// A result that is the end of the call: the text for the model, and the
/// status and one line for the row.
fn ended(status: ToolStatus, text: String, summary: &str) -> (Outcome, ToolStatus, String) {
    let is_error = status != ToolStatus::Ok;
    (Outcome { text, is_error }, status, clip(summary))
}

/// What a call is told when the path it reads cannot be shown to the user
/// exactly as it would be opened.
const UNSHOWABLE: &str = "path cannot be shown safely";

/// What the model is told when its command was typed.
const TYPED_TEXT: &str = "The command was typed into the user's pane. Enter was not pressed: \
                          the user runs it themselves.";

/// The result of a command the app ran in the pane: the words of the report,
/// with the output cleaned, redacted and wrapped here, off the UI thread.
///
/// The fourth element is the system line of a `cd` (`pane N: <cmd> → <cwd>`);
/// the app adds that line itself when the run ends (`report::system_line`).
fn ran(call: &Call, report: &RunReport) -> (Outcome, ToolStatus, String, Option<String>) {
    let command = match call {
        Call::RunInPane { command } => command.as_str(),
        _ => "",
    };
    let words = report::words(command, report, super::new_nonce().as_deref());
    let (outcome, status, summary) = ended(ToolStatus::Ok, words.text, &words.row);
    (outcome, status, summary, words.system)
}

/// Runs an approved server tool and returns what the model and the row see:
/// the result wrapped as untrusted data under `nonce` (D14), which also caps
/// it. The nonce is needed BEFORE the call: with none there is no wrapper, and
/// a tool must not run for a result that would be withheld.
async fn call_external(
    set: &ExternalSet,
    name: &str,
    args: serde_json::Value,
    nonce: Option<String>,
) -> (Outcome, ToolStatus, String) {
    let Some(nonce) = nonce else {
        return ended(ToolStatus::Error, NO_SERVER.to_owned(), NO_SERVER);
    };
    // No turn cancel reaches here, and none can without changing how a turn
    // ends: `Chats::cancel` aborts the turn's task at once, which drops this
    // future and with it the request in flight (its late answer is skipped by
    // id). The server is not sent `notifications/cancelled`; a call that was
    // running is simply abandoned, and the chat is idle.
    let outcome = set.call(name, args, std::future::pending()).await;
    let status = if outcome.is_error {
        ToolStatus::Error
    } else {
        ToolStatus::Ok
    };
    let summary = if outcome.is_error {
        clip(&outcome.text)
    } else {
        format!("{} bytes", outcome.text.len())
    };
    let text = label::wrap(&nonce, &outcome.text);
    (
        Outcome {
            text,
            is_error: outcome.is_error,
        },
        status,
        summary,
    )
}

async fn execute(host: Arc<ToolHost>, scope: Arc<TurnScope>, call: Call) -> Outcome {
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    // The command is the sanitizer's, so the ask shows what would be typed.
    let mut checked: Result<Call, Cow<'static, str>> = match &call {
        Call::RunInPane { command } => command::validate(command)
            .map(|valid| Call::RunInPane {
                command: valid.as_str().to_owned(),
            })
            .map_err(|refusal| refusal.reason().into()),
        other => files::gate(&scope, other)
            .map(|()| other.clone())
            .map_err(Cow::from),
    };
    let mut ask = None;
    let mut sealed = None;
    let mut approves = None;
    if let Ok(valid) = &checked {
        match (valid, scope.locality) {
            // A cloud read asks with the path as it will be opened: resolved,
            // and only when the display shows it unchanged.
            (Call::ReadFile { path, lines }, Locality::Cloud) => {
                let home = host.home.as_deref().map(Path::new);
                match home.and_then(|home| display_target(home, &scope.origin, path)) {
                    Some(shown) => {
                        approves = Some(shown.clone());
                        ask = ask_for(
                            &Call::ReadFile {
                                path: shown,
                                lines: *lines,
                            },
                            Locality::Cloud,
                        )
                    }
                    None => checked = Err(UNSHOWABLE.into()),
                }
            }
            // An edit that can only fail is refused before it asks. A missing
            // or ambiguous `old` still asks: it too tells whether a text occurs.
            (Call::EditFile { old, .. }, Locality::Cloud) if old.is_empty() => {
                checked = Err(EditError::EmptyOld.to_string().into());
            }
            // An edit of a file asks like a read of it, once per file per turn.
            (Call::EditFile { path, .. }, Locality::Cloud) => {
                let home = host.home.as_deref().map(Path::new);
                match home.and_then(|home| display_target(home, &scope.origin, path)) {
                    Some(shown) => {
                        if !files::lock_set(&scope.allowed).contains(&shown) {
                            ask = Some(ApprovalAsk::Read {
                                path: shown.clone(),
                            });
                        }
                        approves = Some(shown);
                    }
                    None => checked = Err(UNSHOWABLE.into()),
                }
            }
            // A command is judged before it asks: a refusal never opens a prompt.
            (Call::RunCommand { .. }, locality) => match exec::plan(&host, &scope, valid).await {
                Ok(_) => ask = ask_for(valid, locality),
                Err(why) => checked = Err(why.into()),
            },
            // A batch is sealed before the ask: the user reviews what was sealed.
            (Call::ProposeBatch, _) => match files::seal(&host, &scope).await {
                Ok((batch, review)) => {
                    ask = Some(ApprovalAsk::Batch(review));
                    sealed = Some(batch);
                }
                Err(why) => checked = Err(why.into()),
            },
            (valid, locality) => ask = ask_for(valid, locality),
        }
    }
    // Asked before the announcement: `Approver::ask` registers the wait at
    // once, so an answer that comes the instant the `ToolCall` is seen (D18
    // auto-type) finds it.
    //
    // Tradeoff, on purpose: registering also counts the call as pending (the
    // turn's timers pause) while `reserve_owned` below may still be waiting for
    // room in the channel, so for that moment the call is not yet visible to
    // the user. Registering after the announcement would let a fast answer
    // miss its wait, which loses a decision; a short pause of the timers
    // while the channel drains costs nothing, and the total limit still runs.
    let pending = ask.as_ref().map(|ask| scope.approver.ask(id, ask));
    let gone = || Outcome {
        text: CANCELLED.to_owned(),
        is_error: true,
    };
    // The slot for the result comes first: waiting for it happens before
    // anything is announced.
    let Ok(slot) = scope.events.clone().reserve_owned().await else {
        return gone();
    };
    let announce = StreamEvent::ToolCall {
        id,
        name: call.name().to_owned(),
        summary: clip(&call.argument()),
        approval: ask.map(Box::new),
    };
    if scope.events.send(announce).await.is_err() {
        return gone();
    }
    // From here the user may see the ask: a batch dropped now is an orphan.
    if let Some(batch) = sealed.as_mut() {
        batch.announced();
    }
    // Only an announced call owes a result.
    let owed = Owed {
        id,
        slot: Some(slot),
    };

    let (outcome, status, summary) = match checked {
        Err(reason) => ended(ToolStatus::Error, reason.to_string(), &reason),
        Ok(call) => {
            let answer = match pending {
                Some(pending) => pending.await,
                None => Decision::Sent.into(),
            };
            // Resumed, so not dropped by a cancel: from here the batch is
            // kept only where `answered` says so, and no early return below
            // can leave an orphan.
            if let Some(batch) = sealed.as_mut() {
                batch.resumed();
            }
            let decision = match answer {
                Answer::Decided(decision) => {
                    if let (Some(shown), Decision::Sent) = (approves, decision) {
                        files::lock_set(&scope.allowed).insert(shown);
                    }
                    decision
                }
                // Only a run is ever given a report.
                Answer::Ran(_) if !matches!(call, Call::RunInPane { .. }) => {
                    let (outcome, status, summary) = ended(
                        ToolStatus::Denied,
                        format!("denied: {NOT_COMMAND}"),
                        NOT_COMMAND,
                    );
                    owed.answer(status, &summary);
                    return outcome;
                }
                Answer::Ran(report) => {
                    let (outcome, status, summary, _system) = ran(&call, &report);
                    owed.answer(status, &summary);
                    return outcome;
                }
            };
            match (decision, call) {
                (decision, Call::ProposeBatch) => {
                    files::answered(&host, &scope, sealed.take(), decision).await
                }
                (Decision::Denied(reason), _) => {
                    ended(ToolStatus::Denied, format!("denied: {reason}"), reason)
                }
                (Decision::Busy(number), _) => {
                    let reason = format!("pane {number} is running a program");
                    ended(ToolStatus::Denied, format!("denied: {reason}"), &reason)
                }
                // `Typed` answers a run in a pane. A user tool is only ever
                // sent (after `y`) or denied, never routed by another answer.
                (Decision::Typed(_), Call::External { .. }) => ended(
                    ToolStatus::Denied,
                    format!("denied: {NOT_COMMAND}"),
                    NOT_COMMAND,
                ),
                (Decision::Typed(number), Call::RunInPane { .. }) => ended(
                    ToolStatus::Ok,
                    TYPED_TEXT.to_owned(),
                    &format!("typed into pane {number} \u{2014} no Enter"),
                ),
                (Decision::Sent, Call::RunInPane { .. }) => {
                    ended(ToolStatus::Ok, TYPED_TEXT.to_owned(), "typed, no Enter")
                }
                // Only after `y`: nothing is sent to the server before it.
                (
                    Decision::Sent,
                    Call::External {
                        name,
                        server,
                        tool,
                        args,
                    },
                ) => match scope.external.get() {
                    // What the user approved must be what is routed.
                    Some(set) if set.qualified().raw_of(&name) == Some((&server, &tool)) => {
                        call_external(set, &name, args, super::new_nonce()).await
                    }
                    _ => ended(ToolStatus::Error, NO_SERVER.to_owned(), NO_SERVER),
                },
                (Decision::Sent | Decision::Typed(_), call) => {
                    files::run(&host, &scope, call).await
                }
            }
        }
    };
    owed.answer(status, &summary);
    outcome
}

mod exec;
mod files;
mod gate;
mod orphan;

#[cfg(test)]
mod exec_tests;
#[cfg(test)]
mod external_tests;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod tests;
