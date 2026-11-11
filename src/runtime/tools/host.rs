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
//! manager (D3). A worker's read runs unasked inside its run's read scope;
//! outside it, it asks as the same chat read would, with the worker's label
//! (D6).
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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::mpsc;

use super::{
    Limits, OrchHandle, ToolEnv, WorkerScope, approver::CANCELLED, fs::Within, stage::Stages,
};
use crate::core::ai::tools::windows::WinDrives;
use crate::{
    core::ai::{
        chat::{
            approval::{ApprovalAsk, Reach, UNSHOWABLE, ask_for},
            engine::{Answer, CallId, Decision, ScreenText, StreamEvent, ToolStatus},
        },
        orchestra::{
            WorkerKey,
            commands::{Ran, RanEnd},
            tools::{OrchCall, WAIT_MAX_SECS},
        },
        tools::{
            Call, Locality, ToolError, command,
            edit::EditError,
            label,
            mcp::refusal_text,
            panes::{self, Roster},
            paths::{REFUSED, display_target, is_env_template},
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
/// What the model is told on the user's `y`: the form is theirs (D2).
pub(super) const READY: &str = "The user accepted: the Feedback form will open with this draft at the end of this turn for the user to review and send. You do not send it.";
/// What a later `gh_issue_draft` of the turn answers after a denial.
pub(super) const DRAFT_DECLINED: &str = "a draft issue was already denied in this turn";
/// The key of a denied draft in the turn's `allowed` set.
const DRAFT_DENIED_KEY: &str = "issue-draft-denied";

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

    /// The approver of one worker of this turn's run, whose asks wait under
    /// the worker's own session. `None` when the approver keeps no sessions:
    /// the worker then asks through this one.
    fn for_worker(&self, _key: WorkerKey) -> Option<Arc<dyn Approver>> {
        None
    }

    /// The worker ended or was cancelled: its asks are denied, the other
    /// workers' are not touched.
    fn end_worker(&self, _key: WorkerKey) {}

    /// Whether this approver's worker session is over (cancelled, or replaced
    /// by a retry): its asks are denied unseen and must not be announced.
    fn is_over(&self) -> bool {
        false
    }
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
    /// The Windows drive folder the user let this chat write in (openspec
    /// `wsl-drive-access` D7): set from the chat's grant when the turn starts.
    /// `None` for a worker, and for a chat without a grant.
    pub drive_grant: Option<PathBuf>,
    /// The paths (as shown) the user allowed a read or an edit of in this
    /// turn: a later `edit_file` of one does not ask again.
    pub allowed: Mutex<HashSet<String>>,
    /// The run manager of an orchestrator turn. Without it an orchestration
    /// call is an unknown tool (D3).
    pub orchestra: Option<OrchHandle>,
    /// Set for a worker turn only: the run whose read scope its reads are
    /// judged against, and the label of its asks.
    pub read_scope: Option<WorkerScope>,
    /// The engine of the turn is a CLI. It runs in a private cwd and may
    /// join a relative path to it before it calls a tool; a server engine
    /// never does, so the path rewrite is for CLI turns only.
    pub cli: bool,
    /// The engine is an Ollama model served by Ollama's cloud (the name says
    /// so, or there is none): what a tool returns leaves the machine although
    /// the adapter is local. Only the GitHub tools and the reads of a blocked
    /// conversation act on it.
    pub ollama_cloud: bool,
    /// The conversation read GitHub text: set from the chat at the start of
    /// the turn and again by the host when it starts `gh`, so it applies to
    /// the next call of the same turn.
    pub github_read: AtomicBool,
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
            drive_grant: None,
            allowed: Mutex::default(),
            orchestra: None,
            read_scope: None,
            cli: false,
            ollama_cloud: false,
            github_read: AtomicBool::new(false),
        }
    }

    /// Whether a read of the user's files asks as on a cloud engine: it does
    /// there, and on an Ollama `*-cloud` model once the conversation read
    /// GitHub text (other reads of that model stay free).
    fn reads_ask(&self, locality: Locality) -> bool {
        locality == Locality::Cloud
            || (self.ollama_cloud && self.github_read.load(Ordering::Relaxed))
    }

    /// Whether this is a read-only review worker with a batch to look at: it
    /// reads the staged view, not the disk alone (S7b).
    fn sees_stage(&self) -> bool {
        self.read_scope.is_some()
            && self.stage_key.is_some()
            && !self
                .families
                .contains(crate::core::ai::tools::registry::Family::Write)
    }

    /// Whether this turn may call `call` (D7, the filter's second place after
    /// the tool list): an orchestration call has a run manager to go to (D3);
    /// a worker's or the orchestrator's call has its family in the turn's
    /// families, and a user server tool is a chat's or the orchestrator's
    /// (asked `y` as in a chat, user decision 2026-10-29), never a worker's.
    /// A chat turn's families are checked where they always were: the write
    /// and exec gate (`files::gate`).
    fn offers(&self, call: &Call) -> bool {
        let orchestrated = self.orchestra.is_some() || self.read_scope.is_some();
        match (call, call.family()) {
            (Call::Orchestra(_), _) if self.orchestra.is_none() => false,
            (_, Some(family)) => !orchestrated || self.families.contains(family),
            (_, None) => self.read_scope.is_none(),
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
    /// The private cwds of CLI turns, found on first use.
    cli_cwds: std::sync::OnceLock<Arc<[PathBuf]>>,
    /// The last reading of the panes, filled by the runtime's poll.
    roster: Roster,
    /// The login PATH and the fork cache of the GitHub tools.
    gh: github::Gh,
    /// Windows drives set by a test; otherwise the ones the runtime found.
    drives: std::sync::OnceLock<WinDrives>,
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
            cli_cwds: std::sync::OnceLock::new(),
            roster: Roster::default(),
            gh: github::Gh::default(),
            drives: std::sync::OnceLock::new(),
        })
    }

    /// Makes every call of this host see `drives` as the WSL drives.
    #[cfg(test)]
    pub(crate) fn set_drives(&self, drives: WinDrives) {
        let _ = self.drives.set(drives);
    }

    /// The login PATH, where `gh` is looked up and what it runs with. Set
    /// once, at start; a second call changes nothing.
    pub fn set_gh_path(&self, path: OsString) {
        self.gh.set_path(path);
    }

    /// The job variables of `[build] jobs` for the next `run_command`s.
    pub fn set_build_env(&self, env: [(String, String); 2]) {
        self.exec.set_build_env(env);
    }

    /// Where the runtime puts what the panes run, for `list_panes`.
    pub fn roster(&self) -> Roster {
        self.roster.clone()
    }

    /// What a call may see, for the chat whose origin directory is `origin`.
    pub(crate) fn env(&self, origin: &Path) -> ToolEnv {
        ToolEnv {
            home: self.home.clone(),
            xdg_config_home: self.xdg_config_home.clone(),
            xdg_data_home: self.xdg_data_home.clone(),
            origin: origin.to_path_buf(),
            drives: self.drives.get().cloned(),
            drive_grant: None,
        }
    }

    /// What a call of the file tools may see in `scope`: [`env`](Self::env)
    /// with the Windows drive folder the user let the turn's chat write in.
    pub(crate) fn scope_env(&self, scope: &TurnScope) -> ToolEnv {
        ToolEnv {
            drive_grant: scope.drive_grant.clone(),
            ..self.env(&scope.origin)
        }
    }

    /// The change set staged in the batch of `key`, as a worker's ledger
    /// names it (S7b: the run manager re-reads it after a correction). `None`
    /// when the read itself failed.
    pub(crate) async fn staged_changes(
        &self,
        key: StageKey,
        origin: &Path,
    ) -> Option<Vec<crate::core::ai::orchestra::changes::Change>> {
        let (stages, env) = (self.stages.clone(), self.env(origin));
        let taken = tokio::task::spawn_blocking(move || files::staged_changes(&stages, key, &env));
        review::joined(taken.await)
    }

    /// What a review may be shown of the batch of `key`; `allow` says which
    /// staged targets it may read. `None` when the read itself failed.
    pub(crate) async fn staged_diff(
        &self,
        key: StageKey,
        origin: &Path,
        allow: impl Fn(&Path) -> bool + Send + 'static,
    ) -> Option<crate::core::ai::orchestra::changes::StagedDiff> {
        let (stages, env) = (self.stages.clone(), self.env(origin));
        let taken =
            tokio::task::spawn_blocking(move || files::staged_diff(&stages, key, &env, &allow));
        taken.await.ok()
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

/// A run answered a call that is not a command: nothing ran.
const NOT_COMMAND: &str = "that call is not a command";
/// No nonce could be made, so the rows cannot be wrapped as data.
const NO_PANES: &str = "the panes cannot be listed now";
const NO_SCREEN: &str = "the screen cannot be sent now";
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

/// Whether an `edit_file` asks like a read before it runs: its answers tell
/// whether a text occurs in the file. On a cloud engine every edit does; on a
/// local one only that of an env template, which no read may show.
fn edit_asks(locality: Locality, path: &str) -> bool {
    locality == Locality::Cloud
        || Path::new(path)
            .file_name()
            .is_some_and(|name| is_env_template(&name.to_string_lossy()))
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

/// What a review worker is told when it reads outside the run's scope.
const OUT_OF_REVIEW_SCOPE: &str = "that path is outside the run's scope: the review cannot read it";

/// What a worker is told when it writes or runs after `needs_specification`.
const CLARIFYING: &str =
    "you asked for a clearer brief: write and run nothing more; end with a short text";

/// What a worker is told when it writes or runs after it chose to split.
const SPLIT_CLOSED: &str =
    "you chose to split the work: write and run nothing more; end with a short text";

/// What refuses `call` when it is a write or a command of a worker that
/// already ended its brief, by `needs_specification` or by a split: it is
/// refused, before it asks and after it waited, with the text of its ending.
fn closed(scope: &TurnScope, call: &Call) -> Option<&'static str> {
    if !(call.is_write() || matches!(call, Call::RunCommand { .. })) {
        return None;
    }
    let w = scope.read_scope.as_ref()?;
    if !w.handle.clarifying(w.run, w.worker) {
        return None;
    }
    Some(if w.handle.split_chosen(w.run, w.worker) {
        SPLIT_CLOSED
    } else {
        CLARIFYING
    })
}

/// What a worker is told when `needs_specification` recorded nothing.
const NOT_RECORDED: &str = "your question was not recorded: a first one stands";

/// What a worker is told once its question is recorded.
const RECORDED: &str =
    "recorded: the coordinator gets your question; write nothing more and end with a short text";

/// What a correction worker is told when it proposes the batch.
const CORRECTION_PROPOSES: &str = "a correction only stages its fixes: the worker whose change is under review proposes the batch";

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

/// The text of `list_panes`, wrapped as data: the command line of a program is
/// text the model must not take as instructions. `text` is what the user was
/// shown. The row shows what was sent.
fn list_panes(text: String) -> (Outcome, ToolStatus, String) {
    let Some(nonce) = super::new_nonce() else {
        return ended(ToolStatus::Error, NO_PANES.to_owned(), NO_PANES);
    };
    ended(ToolStatus::Ok, label::wrap(&nonce, &text), &text)
}

/// The result of an approved screen: the frozen text, wrapped as data.
fn screen_result(pane: u32, text: &ScreenText) -> (Outcome, ToolStatus, String) {
    let Some(nonce) = super::new_nonce() else {
        return ended(ToolStatus::Error, NO_SCREEN.to_owned(), NO_SCREEN);
    };
    let summary = format!("{} lines from pane {pane}", text.0.lines().count());
    ended(ToolStatus::Ok, label::wrap(&nonce, &text.0), &summary)
}

/// The private cwds of a CLI turn (`<data>/lazarobox/cli-cwd`), as typed and
/// canonical: a CLI joins a relative path to its own cwd before it calls a
/// host tool, so a path under it names a file of the project.
fn cli_cwds(host: &ToolHost) -> Vec<PathBuf> {
    let home = host.home.as_deref();
    let Ok(data) = crate::core::desktop::data_home(host.xdg_data_home.as_deref(), home) else {
        return Vec::new();
    };
    let cwd = data.join("lazarobox").join("cli-cwd");
    let mut all = vec![cwd.clone()];
    if let Ok(real) = std::fs::canonicalize(&cwd)
        && real != cwd
    {
        all.push(real);
    }
    all
}

async fn execute(host: Arc<ToolHost>, scope: Arc<TurnScope>, call: Call) -> Outcome {
    let rehomed = std::cell::Cell::new(false);
    let call = if scope.cli {
        let cwds = match host.cli_cwds.get() {
            Some(known) => known.clone(),
            // The folders are looked up off the async path, once.
            None => {
                let finder = host.clone();
                let found = tokio::task::spawn_blocking(move || {
                    finder
                        .cli_cwds
                        .get_or_init(|| cli_cwds(&finder).into())
                        .clone()
                });
                found.await.unwrap_or_default()
            }
        };
        call.map_paths(|path| {
            let home = crate::core::ai::tools::paths::rehome(&cwds, path);
            rehomed.set(rehomed.get() || home.is_some());
            home
        })
    } else {
        call
    };
    let id = CallId(host.next.fetch_add(1, Ordering::Relaxed));
    // The command is the sanitizer's, so the ask shows what would be typed.
    let mut checked: Result<Call, Cow<'static, str>> = match &call {
        // What the turn does not offer is refused as the parse refuses it.
        call if !scope.offers(call) => Err(refusal_text(ToolError::UnknownTool).into()),
        // A worker that asked for a clearer brief writes and runs nothing.
        call if let Some(text) = closed(&scope, call) => Err(text.into()),
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
    // The text of `list_panes`, as the ask showed it.
    let mut listed = None;
    // How an answer of the approval mode is taken: only as the facts this
    // ask carried allow.
    let mut unprompted = Unprompted::Never;
    // A GitHub read: what the checks before the prompt found and the keys
    // the answer is remembered under. A GitHub read or a draft: the reason
    // of a decline made earlier in this turn.
    let mut github_plan = None;
    let mut github_keys = None;
    let mut declined = None;
    // A write over a template that is on disk tells what it holds, so it asks
    // like an edit of it. Whether the file exists is read before the match.
    let overwrites = match &checked {
        Ok(call) => files::overwrites_template(&host, &scope, call).await,
        Err(_) => false,
    };
    if let Ok(valid) = &checked {
        let home = host.home.as_deref().map(Path::new);
        let shown = |path: &str| home.and_then(|home| display_target(home, &scope.origin, path));
        match (valid, scope.locality, &scope.read_scope) {
            // A worker read runs unasked inside its run's scope (D6). Outside
            // it, or when the scope grants nothing, it asks as the same chat
            // read would (D6, user decision 2026-10-29), labelled with the
            // worker, so the approval mode answers it alike. A read asks with
            // the path as it will be opened: resolved, and only when the
            // display shows it unchanged. The ask carries where the read
            // leads, for the approval mode. A cloud read always asks; a local
            // one only when it leads outside the root.
            // A review worker never asks: its reads stay inside the run's
            // scope, staged-only files included, or are refused and told to
            // its review (S7b).
            (Call::ReadFile { path, .. }, _, Some(worker)) if scope.sees_stage() => {
                match files::review_reach(&host, &scope, path).await {
                    files::ReviewRead::Allowed => {}
                    // A probe is refused with its reason and counts for nothing.
                    files::ReviewRead::Refused => checked = Err(OUT_OF_REVIEW_SCOPE.into()),
                    files::ReviewRead::RefusedStaged => {
                        worker.handle.unread(worker.run, worker.worker, path);
                        checked = Err(OUT_OF_REVIEW_SCOPE.into());
                    }
                }
            }
            (Call::ReadFile { path, .. }, locality, worker) => {
                if let Some(worker) = worker {
                    let env = host.env(&scope.origin);
                    scoped = super::in_scope(worker, env, path, &host.limits).await;
                }
                let label = worker
                    .as_ref()
                    .map(|worker| (worker.ask_label(), worker.key()));
                let read = |path: String, reach: Reach, cloud: bool| match &label {
                    Some((label, key)) => ApprovalAsk::WorkerRead {
                        path,
                        label: label.clone(),
                        worker: *key,
                        reach,
                        cloud,
                    },
                    None => ApprovalAsk::Read { path, reach, cloud },
                };
                let reach = if scoped.is_some() {
                    Reach::Root
                } else {
                    files::reach(&host, &scope, path).await
                };
                let cloud = scope.reads_ask(locality);
                if scoped.is_none() && (cloud || reach == Reach::Other) {
                    match shown(path) {
                        Some(shown) => {
                            if cloud {
                                approves = Some(shown.clone());
                            }
                            unprompted = Unprompted::of_read(reach, cloud);
                            ask = Some(read(shown, reach, cloud));
                        }
                        // A local read ran free before `run` asked for it:
                        // the app decides, with nothing shown.
                        None if !cloud => {
                            unprompted = Unprompted::of_read(reach, cloud);
                            ask = Some(read(String::new(), Reach::Unshowable, cloud));
                        }
                        None => checked = Err(UNSHOWABLE.into()),
                    }
                }
            }
            // A worker's staging call asks nothing (S7): staging keeps it in
            // its run's read scope, where its reads run unasked, and refuses
            // it outside; only the user's batch review writes it.
            (
                Call::CreateDir { .. } | Call::WriteFile { .. } | Call::EditFile { .. },
                _,
                Some(_),
            ) => {}
            // An edit that can only fail is refused before it asks. A missing
            // or ambiguous `old` still asks: it too tells whether a text occurs.
            (Call::EditFile { old, path, .. }, locality, _)
                if edit_asks(locality, path) && old.is_empty() =>
            {
                checked = Err(EditError::EmptyOld.to_string().into());
            }
            // An edit of a file asks like a read of it, once per file per turn,
            // and so does a write over a template that exists.
            (Call::EditFile { path, .. } | Call::WriteFile { path, .. }, locality, _)
                if overwrites
                    || (matches!(valid, Call::EditFile { .. }) && edit_asks(locality, path)) =>
            {
                match shown(path) {
                    Some(shown) => {
                        if !files::lock_set(&scope.allowed).contains(&shown) {
                            ask = Some(ApprovalAsk::read(shown.clone()));
                        }
                        approves = Some(shown);
                    }
                    None => checked = Err(UNSHOWABLE.into()),
                }
            }
            // A command is judged before it asks: a refusal never opens a prompt.
            // A worker's ask names its session and label (D19): only the scope
            // knows them, not the plan.
            (Call::RunCommand { .. }, locality, read_scope) => {
                match exec::plan(&host, &scope, valid).await {
                    Ok(plan) => {
                        ask = ask_for(valid, locality);
                        if let Some(ask) = ask.as_mut() {
                            exec::fill(&host, &plan, ask);
                            if let ApprovalAsk::Exec { worker, .. } = ask {
                                *worker = read_scope.as_ref().map(|w| (w.key(), w.ask_label()));
                            }
                        }
                        pinned = plan.pin;
                    }
                    // A worker's refusal is also told in the pane, naming
                    // the worker (W6): its row is in the worker's view.
                    Err(why) => {
                        if let Some(worker) = read_scope {
                            let line = format!(
                                "worker {}: run_command refused: {why}",
                                worker.ask_label()
                            );
                            let _ = scope.events.send(StreamEvent::Notice(line)).await;
                        }
                        checked = Err(why.into());
                    }
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
            // The correction of a review only stages: the worker whose change
            // is under review proposes the batch.
            (Call::ProposeBatch, _, Some(worker))
                if worker.handle.corrects(worker.run, worker.worker) =>
            {
                checked = Err(CORRECTION_PROPOSES.into());
            }
            // A batch is sealed before the ask: the user reviews what was
            // sealed. A `simple` worker's file over the line limit asks first
            // (W9b), then a high-risk change is reviewed first, with the user's
            // consent (S7b); the batch review follows whatever it found.
            (Call::ProposeBatch, _, _) => match limit::before_batch(&host, &scope).await {
                limit::Limited::Go => {
                    review::before_batch(&host, &scope).await;
                    match files::seal(&host, &scope).await {
                        Ok((batch, review)) => {
                            ask = Some(ApprovalAsk::Batch(review));
                            sealed = Some(batch);
                        }
                        Err(why) => checked = Err(why.into()),
                    }
                }
                limit::Limited::Stop(why) => checked = Err(why.into()),
            },
            // A search asks with how concrete the root it walks is: the
            // approval mode lets only the search of a concrete root go.
            (Call::Grep { .. }, locality, _) if scope.reads_ask(locality) => {
                let found = files::search_reach(&host, &scope).await;
                ask = ask_for(valid, Locality::Cloud);
                if let Some(ApprovalAsk::Search { reach, .. }) = ask.as_mut() {
                    *reach = found;
                }
                if found == Reach::Root {
                    unprompted = Unprompted::AsIs;
                }
            }
            // The text is rendered once: what the user reads is what is sent.
            // A local engine keeps it on the machine and is not asked.
            (Call::ListPanes, locality, _) => {
                let text = panes::render_aged(&host.roster.lines(), host.roster.age());
                ask = ask_for(valid, locality).map(|_| ApprovalAsk::Panes { text: text.clone() });
                listed = Some(text);
            }
            // The repository, `gh` and the folder are judged before anything
            // is asked: a refusal opens no prompt and starts nothing.
            (Call::Github(gh), _, _) => match github::plan(&host, &scope).await {
                Ok(planned) => {
                    match github::consent(&scope, gh, &planned.slug) {
                        github::Consent::Free => {}
                        github::Consent::Declined => declined = Some(github::DECLINED),
                        github::Consent::Ask(question, yes, no) => {
                            ask = Some(*question);
                            github_keys = Some((yes, no));
                        }
                    }
                    github_plan = Some(planned);
                }
                Err(why) => checked = Err(why.into()),
            },
            // One decline per turn, as the GitHub consent has it; every
            // other call asks.
            (Call::IssueDraft(_), locality, _) => {
                if files::lock_set(&scope.allowed).contains(DRAFT_DENIED_KEY) {
                    declined = Some(DRAFT_DECLINED);
                } else {
                    ask = ask_for(valid, locality);
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
    // A closed channel: nobody will ever see the batch, so it is not left
    // sealed. A mere drop while waiting for room keeps it (see `Sealed`).
    let unseen = |sealed: &mut Option<files::Sealed>| {
        if let Some(batch) = sealed.as_mut() {
            batch.unseen();
        }
    };
    let gone = || Outcome {
        text: CANCELLED.to_owned(),
        is_error: true,
    };
    // The slot for the result comes first: waiting for it happens before
    // anything is announced.
    let Ok(slot) = scope.events.clone().reserve_owned().await else {
        unseen(&mut sealed);
        return gone();
    };
    // The worker's session ended while the slot was awaited (a cancel): the
    // wait is already denied, so nothing is announced that the user could be
    // shown and never answer. The slot drops unused.
    if pending.is_some() && scope.approver.is_over() {
        return gone();
    }
    let announce = StreamEvent::ToolCall {
        id,
        name: call.name().to_owned(),
        summary: if rehomed.get() {
            // Clipped first, so the note is never the part that is cut.
            format!("{} (from cli-cwd path)", clip(&call.argument()))
        } else {
            clip(&call.argument())
        },
        approval: ask.map(Box::new),
    };
    if scope.events.send(announce).await.is_err() {
        unseen(&mut sealed);
        return gone();
    }
    // Only an announced call owes a result.
    let owed = Owed {
        id,
        slot: Some(slot),
    };

    // A worker's command the user did not approve, whatever the answer, is
    // recorded for the coordinator as denied (W6); one that ran is recorded
    // by `exec::run`, which takes it from here.
    let mut denied_command = match (&checked, &scope.read_scope) {
        (Ok(call @ Call::RunCommand { .. }), Some(worker)) => Some((worker, exec::shown(call))),
        _ => None,
    };
    let (outcome, status, summary) = match checked {
        Err(reason) => ended(ToolStatus::Error, reason.to_string(), &reason),
        Ok(_) if declined.is_some() => {
            let reason = declined.unwrap_or_default();
            ended(ToolStatus::Denied, format!("denied: {reason}"), reason)
        }
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
            // A parallel call of the same turn may have recorded the
            // worker's `needs_specification` while this one waited: it is
            // judged again, and a batch is denied and discarded as the
            // user's `n` would.
            if let Some(closing) = closed(&scope, &call) {
                record_denied(denied_command.take());
                let (outcome, status, summary) = match call {
                    Call::ProposeBatch => {
                        let no = Decision::Denied(closing);
                        files::answered(&host, &scope, sealed.take(), no).await
                    }
                    _ => ended(ToolStatus::Denied, format!("denied: {closing}"), closing),
                };
                owed.answer(status, &summary);
                return outcome;
            }
            let decision = match answer {
                Answer::Decided(decision) => {
                    if let (Some(shown), Decision::Sent) = (approves, decision) {
                        files::lock_set(&scope.allowed).insert(shown);
                    }
                    // A decline holds for the rest of the turn, also against a
                    // parallel call asked before it: the key is read again.
                    let mut decision = decision;
                    if let Some((yes, no)) = github_keys {
                        let mut allowed = files::lock_set(&scope.allowed);
                        match decision {
                            Decision::Sent if allowed.contains(&no) => {
                                decision = Decision::Denied(github::DECLINED);
                            }
                            Decision::Sent => {
                                allowed.insert(yes);
                            }
                            Decision::Denied(_) => {
                                allowed.insert(no);
                            }
                            _ => {}
                        }
                    }
                    // Unlike the GitHub keys, a `y` already given is never
                    // taken back, and only the user's no counts.
                    if matches!(call, Call::IssueDraft(_))
                        && matches!(decision, Decision::Denied(why) if ends_draft_asks(why))
                    {
                        files::lock_set(&scope.allowed).insert(DRAFT_DENIED_KEY.to_owned());
                    }
                    decision
                }
                // Only a run is ever given a report.
                Answer::Ran(_) if !matches!(call, Call::RunInPane { .. }) => {
                    record_denied(denied_command.take());
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
                // The frozen text the user read: it answers `read_pane` only.
                Answer::Screen(text) => {
                    record_denied(denied_command.take());
                    let (outcome, status, summary) = match &call {
                        Call::ReadPane { pane, .. } => screen_result(*pane, &text),
                        _ => ended(
                            ToolStatus::Denied,
                            format!("denied: {NOT_APPROVED}"),
                            NOT_APPROVED,
                        ),
                    };
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
                // Only the frozen text releases a screen, never a plain `y` or
                // a mode's answer.
                (
                    Decision::Sent | Decision::Typed(_) | Decision::Unprompted,
                    Call::ReadPane { .. },
                ) => ended(
                    ToolStatus::Denied,
                    format!("denied: {NOT_APPROVED}"),
                    NOT_APPROVED,
                ),
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
                // The worker's question goes to its run, which records it.
                (Decision::Sent, Call::NeedsSpecification { what }) => match &scope.read_scope {
                    Some(worker) if worker.handle.clarify(worker.run, worker.worker, &what) => {
                        ended(ToolStatus::Ok, RECORDED.to_owned(), "needs specification")
                    }
                    Some(_) => ended(ToolStatus::Error, NOT_RECORDED.to_owned(), NOT_RECORDED),
                    None => {
                        let reason = refusal_text(ToolError::UnknownTool);
                        ended(ToolStatus::Error, reason.to_owned(), reason)
                    }
                },
                // After the consent, if the engine needed one: `gh` starts here.
                (Decision::Sent, Call::Github(gh)) => match github_plan.take() {
                    Some(planned) => github::run(&host, &scope, &gh, planned).await,
                    None => ended(ToolStatus::Error, CANCELLED.to_owned(), CANCELLED),
                },
                // The `y` was the user's; the app holds the draft and opens
                // the form when the turn ends. No `gh` runs for it.
                (Decision::Sent, Call::IssueDraft(_)) => {
                    ended(ToolStatus::Ok, READY.to_owned(), "draft accepted")
                }
                // Read-only metadata of the panes: it never asked.
                (Decision::Sent, Call::ListPanes) => list_panes(listed.take().unwrap_or_default()),
                // Only after `y`: the sandbox starts here and nowhere else.
                (Decision::Sent, call @ Call::RunCommand { .. }) => {
                    denied_command = None;
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
                    //
                    // A review worker (read-only, with its proposer's batch to
                    // look at, S7b) reads through the staged view instead,
                    // kept in the root: the staged text is not on disk.
                    (_, Call::ReadFile { .. }) if scope.sees_stage() => {
                        files::run_within(&host, &scope, call, Within::Root).await
                    }
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
    record_denied(denied_command);
    owed.answer(status, &summary);
    outcome
}

/// Records a worker's command that was asked and did not run (W6).
fn record_denied(command: Option<(&WorkerScope, (String, String))>) {
    if let Some((worker, (program, command))) = command {
        let end = RanEnd::Denied;
        let ran = Ran {
            program,
            command,
            end,
        };
        (worker.handle).ran(worker.run, worker.worker, ran);
    }
}

/// Whether a denied draft is the turn's decline: the user said no, or the
/// wait ended (a cancel, a closed chat, a quit, a timeout). The app's own
/// refusals (another approval pending, an open form) asked the user nothing.
fn ends_draft_asks(reason: &str) -> bool {
    matches!(
        reason,
        crate::app::DECLINED | CANCELLED | super::approver::ENDED
    )
}

mod exec;
mod files;
mod gate;
mod git;
mod github;
mod limit;
pub(crate) use git::locate_git;
mod orphan;
mod review;

#[cfg(test)]
mod exec_tests;
#[cfg(all(test, target_os = "linux"))]
pub(crate) use exec_tests::fake_toolchain;
#[cfg(test)]
mod external_tests;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod github_run_tests;
#[cfg(test)]
mod github_tests;
#[cfg(test)]
mod issue_draft_tests;
#[cfg(test)]
mod orchestra_tests;
#[cfg(test)]
mod panes_tests;
#[cfg(test)]
mod tests;
