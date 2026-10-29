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
//! the pure [`ask_for`] decides, from the call and the engine's locality. A
//! read also tells its ask where it leads (inside the chat's root or not), and
//! a local read that leads outside the root asks too; the host never applies
//! the approval mode, the app does (`approval::answers_itself`).
//! A call the turn does not offer (its role's toolset, D7) is refused first,
//! as an unknown tool, and an orchestration call goes to the turn's run
//! manager (D3). A worker's read runs unasked inside its run's read scope and
//! asks with the worker's label outside it, on every engine (D6).
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
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::mpsc;

use super::{Limits, OrchHandle, ToolEnv, WorkerScope, fs::Within, stage::Stages};
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, Reach, UNSHOWABLE, ask_for},
            engine::{Answer, CallId, Decision, StreamEvent, ToolStatus},
        },
        orchestra::tools::{OrchCall, WAIT_MAX_SECS},
        tools::{
            Call, Locality, ToolError, command,
            edit::EditError,
            label,
            mcp::refusal_text,
            paths::{REFUSED, display_target},
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
    /// The built-in families this turn offers, and so may call: the same set
    /// as its tool list (D7). A chat's is its request's; a worker's or the
    /// orchestrator's is its `Toolset`'s. The write family is checked again
    /// here: a call the parse should have refused is refused as unknown.
    pub families: FamilySet,
    /// Whose batch the write tools fill, and whose staged changes the read
    /// tools show. `None` for a turn that cannot stage.
    pub stage_key: Option<StageKey>,
    /// The paths (as shown) the user allowed a read or an edit of in this
    /// turn: a later `edit_file` of one does not ask again.
    pub allowed: Mutex<HashSet<String>>,
    /// The run manager of an orchestrator turn. Without it an orchestration
    /// call is an unknown tool (D3).
    pub orchestra: Option<OrchHandle>,
    /// Set for a worker turn only: the run whose read scope its reads are
    /// judged against, and the label of its asks.
    pub read_scope: Option<WorkerScope>,
}

impl TurnScope {
    /// A chat scope whose calls that need the user are denied: the 0.18.0
    /// tools, no run manager and no read scope.
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
            orchestra: None,
            read_scope: None,
        }
    }

    /// Whether this turn may call `call` (D7, the filter's second place after
    /// the tool list): an orchestration call has a run manager to go to (D3);
    /// a worker's or the orchestrator's call has its family in the turn's
    /// families, and a user server tool is only ever a chat's. A chat turn's
    /// families are checked where they always were: the write and exec gate
    /// (`files::gate`).
    fn offers(&self, call: &Call) -> bool {
        let orchestrated = self.orchestra.is_some() || self.read_scope.is_some();
        match (call, call.family()) {
            (Call::Orchestra(_), _) if self.orchestra.is_none() => false,
            (_, Some(family)) => !orchestrated || self.families.contains(family),
            (_, None) => !orchestrated,
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

/// How the host takes an answer of [`Decision::Unprompted`]: only as the facts
/// of its own ask allow, whatever the app replies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unprompted {
    /// No ask the host made allows it: it is denied and nothing runs.
    Never,
    /// A cloud read whose ask said it stays in a concrete root: kept in that
    /// root by the read itself.
    InRoot,
    /// A cloud read whose ask said its path is refused: it returns the
    /// refusal and opens nothing, even if the path appeared since.
    Refused,
    /// A local read outside the root, which runs free in `ask` and `auto`
    /// as it always has, or the search of a concrete root: run as it is.
    AsIs,
}

impl Unprompted {
    fn of_read(reach: Reach, cloud: bool) -> Self {
        match (reach, cloud) {
            (Reach::Root, true) => Self::InRoot,
            (Reach::Refused, true) => Self::Refused,
            (Reach::Other | Reach::Unshowable, false) => Self::AsIs,
            _ => Self::Never,
        }
    }
}

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

/// What a `wait` the run manager holds past [`WAIT_MAX_SECS`] answers: the
/// run is waiting for the user (an unresolved failure).
const AWAITING_USER: &str = "awaiting_user";

/// Runs an orchestration call on the run manager: its text for the model,
/// and the row. A `wait` ends within [`WAIT_MAX_SECS`] even when the run
/// manager holds it, so no call nears the MCP timeout.
async fn orchestrate(handle: &OrchHandle, call: OrchCall) -> (Outcome, ToolStatus, String) {
    let bounded = matches!(call, OrchCall::Wait { .. });
    let answer = handle.call(call);
    let outcome = if bounded {
        let bound = Duration::from_secs(u64::from(WAIT_MAX_SECS));
        tokio::time::timeout(bound, answer)
            .await
            .unwrap_or_else(|_| Outcome {
                text: AWAITING_USER.to_owned(),
                is_error: false,
            })
    } else {
        answer.await
    };
    let (status, summary) = if outcome.is_error {
        (ToolStatus::Error, clip(&outcome.text))
    } else {
        (ToolStatus::Ok, format!("{} bytes", outcome.text.len()))
    };
    (outcome, status, summary)
}

async fn execute(host: Arc<ToolHost>, scope: Arc<TurnScope>, call: Call) -> Outcome {
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    // The command is the sanitizer's, so the ask shows what would be typed.
    let mut checked: Result<Call, Cow<'static, str>> = match &call {
        // What the turn does not offer is refused as the parse refuses it.
        call if !scope.offers(call) => Err(refusal_text(ToolError::UnknownTool).into()),
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
    // A worker read inside its run's scope: the real path to open and the
    // secret roots that judged it.
    let mut scoped = None;
    let mut sealed = None;
    let mut approves = None;
    let mut pinned = None;
    let mut reviewed = None;
    // How an answer of the approval mode is taken: only as the facts this
    // ask carried allow.
    let mut unprompted = Unprompted::Never;
    if let Ok(valid) = &checked {
        let home = host.home.as_deref().map(Path::new);
        let shown = |path: &str| home.and_then(|home| display_target(home, &scope.origin, path));
        match (valid, scope.locality, &scope.read_scope) {
            // A worker read runs unasked inside its run's scope (D6). Outside
            // it, or when the scope grants nothing, it asks with the worker's
            // label on every engine, local ones too: never allowed and never
            // refused in silence. The approval mode never answers it.
            (Call::ReadFile { path, .. }, _, Some(worker)) => {
                let env = host.env(&scope.origin);
                scoped = super::in_scope(worker, env, path, &host.limits).await;
                if scoped.is_none() {
                    match shown(path) {
                        Some(shown) => {
                            ask = Some(ApprovalAsk::WorkerRead {
                                path: shown,
                                label: worker.label.clone(),
                            })
                        }
                        None => checked = Err(UNSHOWABLE.into()),
                    }
                }
            }
            // A read asks with the path as it will be opened: resolved, and
            // only when the display shows it unchanged. The ask carries where
            // the read leads, for the approval mode. A cloud read always asks;
            // a local one only when it leads outside the root.
            (Call::ReadFile { path, .. }, locality, None) => {
                let reach = files::reach(&host, &scope, path).await;
                let cloud = locality == Locality::Cloud;
                if cloud || reach == Reach::Other {
                    match shown(path) {
                        Some(shown) => {
                            if cloud {
                                approves = Some(shown.clone());
                            }
                            unprompted = Unprompted::of_read(reach, cloud);
                            ask = Some(ApprovalAsk::Read {
                                path: shown,
                                reach,
                                cloud,
                            });
                        }
                        // A local read ran free before `run` asked for it:
                        // the app decides, with nothing shown.
                        None if !cloud => {
                            unprompted = Unprompted::of_read(reach, cloud);
                            ask = Some(ApprovalAsk::Read {
                                path: String::new(),
                                reach: Reach::Unshowable,
                                cloud,
                            });
                        }
                        None => checked = Err(UNSHOWABLE.into()),
                    }
                }
            }
            // An edit that can only fail is refused before it asks. A missing
            // or ambiguous `old` still asks: it too tells whether a text occurs.
            (Call::EditFile { old, .. }, Locality::Cloud, _) if old.is_empty() => {
                checked = Err(EditError::EmptyOld.to_string().into());
            }
            // An edit of a file asks like a read of it, once per file per turn.
            (Call::EditFile { path, .. }, Locality::Cloud, _) => match shown(path) {
                Some(shown) => {
                    if !files::lock_set(&scope.allowed).contains(&shown) {
                        ask = Some(ApprovalAsk::read(shown.clone()));
                    }
                    approves = Some(shown);
                }
                None => checked = Err(UNSHOWABLE.into()),
            },
            // A command is judged before it asks: a refusal never opens a prompt.
            (Call::RunCommand { .. }, locality, _) => {
                match exec::plan(&host, &scope, valid).await {
                    Ok(plan) => {
                        ask = ask_for(valid, locality);
                        if let Some(ask) = ask.as_mut() {
                            exec::fill(&host, &plan, ask);
                        }
                        pinned = plan.pin;
                    }
                    Err(why) => checked = Err(why.into()),
                }
            }
            // A git verb is judged before it asks: a refusal opens no prompt. An
            // add or a commit builds its own ask (a commit keeps it to compare).
            (call, locality, _) if call.uses_git() => match git::plan(&host, &scope, call).await {
                Ok(planned) => {
                    reviewed = planned.clone();
                    ask = planned.or_else(|| ask_for(call, locality));
                }
                Err(why) => checked = Err(why.into()),
            },
            // A batch is sealed before the ask: the user reviews what was sealed.
            (Call::ProposeBatch, _, _) => match files::seal(&host, &scope).await {
                Ok((batch, review)) => {
                    ask = Some(ApprovalAsk::Batch(review));
                    sealed = Some(batch);
                }
                Err(why) => checked = Err(why.into()),
            },
            // A search asks with how concrete the root it walks is: the
            // approval mode lets only the search of a concrete root go.
            (Call::Grep { .. }, Locality::Cloud, _) => {
                let found = files::search_reach(&host, &scope).await;
                ask = ask_for(valid, Locality::Cloud);
                if let Some(ApprovalAsk::Search { reach, .. }) = ask.as_mut() {
                    *reach = found;
                }
                if found == Reach::Root {
                    unprompted = Unprompted::AsIs;
                }
            }
            (valid, locality, _) => ask = ask_for(valid, locality),
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
                // `offers` let it through only with a run manager to go to.
                (Decision::Sent, Call::Orchestra(call)) => match &scope.orchestra {
                    Some(handle) => orchestrate(handle, call).await,
                    None => {
                        let reason = refusal_text(ToolError::UnknownTool);
                        ended(ToolStatus::Error, reason.to_owned(), reason)
                    }
                },
                // Only after `y`: the sandbox starts here and nowhere else.
                (Decision::Sent, call @ Call::RunCommand { .. }) => {
                    exec::run(&host, &scope, &call, pinned.take()).await
                }
                // Only after `y` on a cloud engine, at once on a local one.
                (Decision::Sent, call) if call.uses_git() => {
                    git::run(&host, &scope, &call, reviewed.take()).await
                }
                // The approval mode answers only a read or a search, never
                // for the user: nothing is added to the reads the user
                // allowed, and only what the ask allowed runs. A read it let
                // go because it stayed in the root is kept there by the read
                // itself, judged on the file it opens.
                (Decision::Unprompted, call @ Call::ReadFile { .. })
                    if unprompted == Unprompted::InRoot =>
                {
                    files::run_within(&host, &scope, call, Within::Root).await
                }
                (Decision::Unprompted, Call::ReadFile { .. })
                    if unprompted == Unprompted::Refused =>
                {
                    ended(ToolStatus::Error, REFUSED.to_owned(), REFUSED)
                }
                (Decision::Unprompted, call @ (Call::ReadFile { .. } | Call::Grep { .. }))
                    if unprompted == Unprompted::AsIs =>
                {
                    files::run(&host, &scope, call).await
                }
                (Decision::Unprompted, _) => ended(
                    ToolStatus::Denied,
                    format!("denied: {NOT_APPROVED}"),
                    NOT_APPROVED,
                ),
                (Decision::Sent | Decision::Typed(_), call) => match (scoped, &call) {
                    // Exactly what the scope allowed, paged like any read:
                    // resolving the request again would reopen the race it
                    // closed.
                    (Some((roots, real)), Call::ReadFile { lines, .. }) => {
                        let limits = host.limits.clone();
                        match super::read_in_scope(roots, real, *lines, limits).await {
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
                    _ => files::run(&host, &scope, call).await,
                },
            }
        }
    };
    owed.answer(status, &summary);
    outcome
}

mod exec;
mod files;
mod gate;
mod git;
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
mod git_tests;
#[cfg(test)]
mod orchestra_tests;
#[cfg(test)]
mod tests;
