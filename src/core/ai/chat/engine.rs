//! The contract between the chat core and an engine adapter: what a turn
//! sends and what a stream may say back, and whether an engine can be used.
//! Pure data and functions, no IO.

use std::{path::PathBuf, time::Duration};

use super::{
    adapters::{self, adapter_for},
    approval::ApprovalAsk,
    budget,
    usage::TurnUsage,
};
use crate::core::ai::{
    CATALOG, Detection, Kind, Status,
    config::{AiConfig, Profile, Task},
    jail::JailSupport,
    orchestra::{
        RunId,
        run::{Roster, RunLimits},
    },
    status_text,
    tools::{
        Locality, registry::FamilySet, report::RunReport, servers::Qualified, wiring::McpWire,
    },
};

/// Who wrote a message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
}

/// One message as it is sent to an engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// Everything one turn sends to an engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatRequest {
    pub engine: String,
    pub model: Option<String>,
    pub system: String,
    /// The project-memory block (data, already sanitized and delimited), when
    /// one arrived for the conversation. Apart from `system` and from the
    /// history: the adapter puts it after the system prompt (servers) or
    /// first on stdin (CLIs, never argv).
    pub memory: Option<String>,
    pub messages: Vec<ChatMessage>,
    /// Whether the turn offers the engine tools. Off unless the app asked for
    /// them for an engine that passed its check ([`tools_for`]). No adapter
    /// reads it yet: only the runtime does, to open the turn's tool session,
    /// so the bytes an engine receives do not depend on it.
    pub tools: Tools,
    /// Which tool families the turn offers, and so which built-in names a call
    /// may use. [`FamilySet::LEGACY`] until a toggle turns more on.
    pub families: FamilySet,
}

/// What an orchestrator turn hands the runtime beside its request (D5): the
/// availability of every task as the app resolved it from the menu, and what
/// the run's workers need. The runtime never reads the configuration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrchSend {
    pub roster: Roster,
    /// The conversation's profile: the workers' prompts come from it.
    pub profile: Profile,
    /// Each worker's deadline (`worker_timeout_secs`).
    pub deadline: Duration,
    /// How many workers run at once, how many a run may spawn and how long it
    /// may last.
    pub limits: RunLimits,
    /// What the user asked, verbatim: kept for the run's ledger (S4).
    pub goal: String,
    /// The open run this turn continues (S4, D8) instead of starting one.
    pub attach: Option<RunId>,
}

/// Whether a turn offers tools.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub enum Tools {
    #[default]
    Off,
    On {
        /// The cwd the chat was opened from: what a relative path is read
        /// against.
        origin: Option<PathBuf>,
        /// The session the engine connects to, set by the runtime once it
        /// has opened the turn's socket. The app never sets it. Boxed so the
        /// request stays small (`SendOutcome` carries one).
        mcp: Option<Box<McpWire>>,
        /// The tools of the user MCP servers of this chat, qualified. Empty
        /// until the runtime has started the servers for the turn.
        /// Boxed like `mcp`, so the request stays small.
        external: Box<Qualified>,
    },
}

/// The CLIs whose manual MCP check passed (D12). An engine joins in the commit
/// that follows its check; one outside the set carries `Tools::Off` and stays
/// byte-identical to a build without tools. The check passed for all four on
/// 2026-10-17.
pub const MCP_VERIFIED: &[&str] = &["claude", "codex", "gemini", "ollama"];

/// The tools of a turn on `engine`: on only when the user has tools enabled
/// and the engine is in `verified`.
pub fn tools_for(engine: &str, enabled: bool, origin: Option<PathBuf>, verified: &[&str]) -> Tools {
    if enabled && verified.contains(&engine) {
        Tools::On {
            origin,
            mcp: None,
            external: Box::default(),
        }
    } else {
        Tools::Off
    }
}

/// Why a stream finished normally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finish {
    Stop,
    Length,
}

/// Why a stream failed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChatError {
    /// The stream ended without a final event.
    Interrupted,
    /// The answer grew past the cap.
    Truncated,
    /// The engine or the transport reported a problem.
    Failed(String),
    /// The jail helper could not enforce the rules, so the CLI never ran. The
    /// text is why (sanitized, may be empty).
    Jail(String),
}

/// Identifies one tool call within a turn. The host that runs the call hands
/// them out, so a `ToolResult` can only name a call the host announced.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CallId(pub u64);

/// How a tool call ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolStatus {
    Ok,
    Error,
    /// Not run: refused or not approved. A timer never approves.
    Denied,
}

impl ToolStatus {
    /// The word a result row shows.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Denied => "denied",
        }
    }
}

/// The user's answer to an approval, as the host receives it. The wording
/// follows the write action: `Sent` also means "allowed" for a read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Allowed. For `run_in_pane` it means the command was typed: the app
    /// answers it only once every accept-time check passed and the write was
    /// handed to the pane, so the result row and the model's text may say so.
    /// A check that fails (a full-screen app, a shell that cannot take a
    /// multi-line paste) is a `Denied` with the reason instead.
    Sent,
    /// `Sent` for a command typed without a prompt (design D18) into the pane
    /// the user knows as number `.0`; the result row names it.
    Typed(usize),
    /// Nothing was typed: pane number `.0` has a program in the foreground
    /// (or the runtime cannot tell). Carries the number so the reason can name
    /// the pane; the host words it as `pane N is running a program`.
    Busy(usize),
    /// Why nothing was done; short, and it goes back to the model.
    Denied(&'static str),
    /// Allowed by the chat's approval mode, with no prompt: a read or a search
    /// that `run` lets go (`approval::answers_itself`). Never the user's `y`:
    /// it allows nothing else in the turn, and the host takes it only where
    /// its own ask allowed it, keeping a read in the root as it opens the file.
    Unprompted,
}

/// What a waiting call is finally told: the user's `Decision`, or, for a
/// command the app ran, the report of the run. `Decision` stays `Copy`; the
/// report travels boxed so the channel stays small.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answer {
    Decided(Decision),
    Ran(Box<RunReport>),
}

impl From<Decision> for Answer {
    fn from(decision: Decision) -> Self {
        Self::Decided(decision)
    }
}

/// Whether the adapter of `engine` keeps what a tool returns on this machine.
/// `None` for an engine that is not in the catalog.
pub fn locality(engine: &str) -> Option<Locality> {
    CATALOG
        .iter()
        .find(|e| e.id == engine)
        .map(|e| e.kind.locality())
}

/// A token count as a row shows it: exact below a thousand, then `12.3k`,
/// then `1.2M` (where `k` would round up to `1000.0k`).
fn tokens(count: u64) -> String {
    match count {
        0..1000 => count.to_string(),
        1000..999_950 => format!("{:.1}k", count as f64 / 1000.0),
        _ => format!("{:.1}M", count as f64 / 1e6),
    }
}

/// What a turn used, as the CLI or server reported it. Never estimated: an
/// engine that reports nothing emits no `Usage` at all.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Usage {
    /// Prompt tokens, cached ones included.
    pub input: u64,
    pub output: u64,
    /// The cost in millionths of a US dollar, when the CLI reports one.
    pub cost_micros: Option<u64>,
}

impl Usage {
    /// The worker-row figure of a turn's report: the prompt count adds the
    /// cache reads and writes to the fresh tokens, so it means the same for
    /// every engine. `None` unless both the input and output counts were
    /// reported: a missing one is not zero.
    pub fn from_turn(turn: &TurnUsage) -> Option<Self> {
        let (input, output) = turn.input.zip(turn.output)?;
        let cached = [turn.cache_read, turn.cache_write]
            .into_iter()
            .flatten()
            .fold(0u64, u64::saturating_add);
        Some(Self {
            input: input.saturating_add(cached),
            output,
            cost_micros: turn.cost_micros,
        })
    }

    /// The sum of two reports. The cost is known only when both reports
    /// carry one: a missing cost is never counted as zero, and a partial sum
    /// would read as the whole.
    pub fn plus(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cost_micros: self
                .cost_micros
                .zip(other.cost_micros)
                .map(|(a, b)| a.saturating_add(b)),
        }
    }

    /// The report as a row shows it: `12.3k in 450 out $0.0049`. The cost is
    /// left out when the engine did not report one.
    pub fn label(&self) -> String {
        let mut text = format!("{} in {} out", tokens(self.input), tokens(self.output));
        if let Some(micros) = self.cost_micros {
            // Under half a unit of the fourth decimal it would read `$0.0000`.
            if (1..50).contains(&micros) {
                text.push_str(" <$0.0001");
            } else {
                text.push_str(&format!(" ${:.4}", micros as f64 / 1e6));
            }
        }
        text
    }

    /// `cost_micros` of a cost in dollars, when it is a usable number.
    pub fn micros(dollars: f64) -> Option<u64> {
        (dollars.is_finite() && dollars >= 0.0).then(|| (dollars * 1e6).round() as u64)
    }
}

/// What an adapter task reports about one turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StreamEvent {
    /// No byte arrived yet and the wait is getting long.
    Slow,
    Delta(String),
    /// Something the user should know about how this turn runs, as one line
    /// (a CLI that runs without its jail). Shown once per pane.
    Notice(String),
    /// A piece of the engine's reasoning, kept apart from the answer.
    ThinkingDelta(String),
    /// A tool call the host is about to run. Produced by the host that
    /// executes it, never decoded from answer text. `approval` is set when the
    /// call waits for the user. It is boxed so the event stays small: every
    /// adapter result carries one.
    ToolCall {
        id: CallId,
        name: String,
        summary: String,
        approval: Option<Box<ApprovalAsk>>,
    },
    /// The end of the call `id`, exactly one per `ToolCall`.
    ToolResult {
        id: CallId,
        status: ToolStatus,
        summary: String,
    },
    /// What the turn used, as the engine reported it. Sent before the final
    /// event: nothing after `Done` or `Error` is delivered.
    Usage(Box<TurnUsage>),
    Done(Finish),
    Error(ChatError),
}

impl StreamEvent {
    /// The usage event of a turn. Boxed so the event stays small: every
    /// adapter result carries one.
    pub fn usage(usage: TurnUsage) -> Self {
        Self::Usage(Box::new(usage))
    }
}

/// What the chat pane may do with the engine assigned to the `chat` task.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Availability {
    Ready {
        engine: String,
        model: Option<String>,
    },
    Unassigned,
    /// No detection result has arrived yet.
    Checking,
    /// The engine is not available; carries its id and status text.
    Undetected {
        engine: String,
        status: String,
    },
    Unsupported {
        engine: String,
    },
    NoJail {
        engine: String,
        /// Why this system cannot jail it, with what to do.
        reason: String,
    },
    /// The system can jail, but the jail refuses this CLI's layout.
    Refused {
        engine: String,
        /// Which layout and what to do about it.
        reason: String,
    },
    GatedOff {
        engine: String,
    },
    /// The CLI is older than the argv chat runs it with needs (claude today):
    /// the turn never starts. `version` is the detected `x.y.z`.
    Outdated {
        engine: String,
        version: String,
    },
    NoModel {
        engine: String,
    },
    ModelMissing {
        engine: String,
        model: String,
    },
}

/// Resolves the engine of the `chat` task. Pure, never changes the
/// assignment and never picks another engine.
pub fn availability(
    cfg: &AiConfig,
    detection: Option<&Detection>,
    jail: JailSupport,
) -> Availability {
    availability_for(Task::Chat, cfg, detection, jail)
}

/// Resolves the engine of any task through its effective assignment (its
/// own, else `general`). Same rules as chat, and still pure: an unusable
/// engine is reported, never replaced by another.
pub fn availability_for(
    task: Task,
    cfg: &AiConfig,
    detection: Option<&Detection>,
    jail: JailSupport,
) -> Availability {
    availability_for_with(task, cfg, detection, jail, GATED_OFF, &has_adapter)
}

/// The CLIs whose manual check failed. None yet: the checks run in C13-C15.
const GATED_OFF: &[&str] = &[];

/// CLIs that must never start without a working jail.
pub(crate) const JAIL_REQUIRED: &[&str] = &["codex", "gemini"];

/// `availability` with an explicit list of CLIs whose manual check failed and
/// the test that says which engines have an adapter (so the jail and gate
/// rules stay testable for engines whose adapter has not landed).
pub fn availability_with(
    cfg: &AiConfig,
    detection: Option<&Detection>,
    jail: JailSupport,
    gated_off: &[&str],
    adapter_exists: &dyn Fn(&str) -> bool,
) -> Availability {
    availability_for_with(Task::Chat, cfg, detection, jail, gated_off, adapter_exists)
}

/// `availability_for` with the explicit gate list and adapter test.
pub fn availability_for_with(
    task: Task,
    cfg: &AiConfig,
    detection: Option<&Detection>,
    jail: JailSupport,
    gated_off: &[&str],
    adapter_exists: &dyn Fn(&str) -> bool,
) -> Availability {
    let (assignment, _) = cfg.resolve(task);
    let Some(engine) = assignment.engine else {
        return Availability::Unassigned;
    };
    if !adapter_exists(&engine) {
        return Availability::Unsupported { engine };
    }
    let Some(detection) = detection else {
        return Availability::Checking;
    };
    // A llama-server that lazarobox starts itself only needs its binary: the
    // turn starts it. It ignores the model name, so none is looked up. It
    // returns before the jail and gated-off checks, which never apply to
    // llamacpp (pinned by a test).
    if engine == "llamacpp"
        && let Some(server) = cfg.managed()
        && detection
            .engines
            .iter()
            .any(|s| s.id == engine && matches!(s.status, Status::Available | Status::NotRunning))
    {
        let model = assignment
            .model
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| server.model.display().to_string());
        return Availability::Ready {
            engine,
            model: Some(model),
        };
    }
    let available = detection
        .engines
        .iter()
        .find(|s| s.id == engine)
        .filter(|s| s.status == Status::Available);
    let Some(state) = available else {
        let status = status_text(Some(detection), &engine);
        return Availability::Undetected { engine, status };
    };
    if engine == "claude"
        && let Some(version) = adapters::claude_old_version(state.version.as_deref())
    {
        return Availability::Outdated { engine, version };
    }
    if let JailSupport::Unavailable(off) = jail
        && JAIL_REQUIRED.contains(&engine.as_str())
    {
        return Availability::NoJail {
            engine,
            reason: off.text_required(),
        };
    }
    // The system can jail, but this CLI's layout is refused: it would not
    // start. The reason names the layout and what to do.
    if JAIL_REQUIRED.contains(&engine.as_str())
        && let Some(reason) = &state.diagnostics.jail
    {
        return Availability::Refused {
            engine,
            reason: state
                .diagnostics
                .jail_full
                .clone()
                .unwrap_or_else(|| reason.clone()),
        };
    }
    if gated_off.contains(&engine.as_str()) {
        return Availability::GatedOff { engine };
    }
    let is_server = CATALOG
        .iter()
        .any(|e| e.id == engine && matches!(e.kind, Kind::Server { .. }));
    let model = assignment.model.filter(|m| !m.is_empty());
    if is_server {
        let Some(model) = model else {
            return Availability::NoModel { engine };
        };
        if !state.models.iter().any(|m| same_model(m, &model)) {
            return Availability::ModelMissing { engine, model };
        }
        return Availability::Ready {
            engine,
            model: Some(model),
        };
    }
    Availability::Ready { engine, model }
}

/// A bare model name and the same name tagged `:latest` are one model.
fn same_model(a: &str, b: &str) -> bool {
    let bare = |m: &str| m.strip_suffix(":latest").unwrap_or(m).to_owned();
    bare(a) == bare(b)
}

/// Whether an adapter exists for the engine. Only the engines whose adapter
/// has landed can chat; the others (opencode) are unsupported.
pub fn has_adapter(id: &str) -> bool {
    adapter_for(id).is_some()
}

/// The most messages one turn sends.
pub const MAX_HISTORY_MESSAGES: usize = 40;
/// Character cap of the history for a local server (small default context).
pub const SERVER_HISTORY_CHARS: usize = 12_000;
/// Character cap of the history for a CLI.
pub const CLI_HISTORY_CHARS: usize = 100_000;

/// The history cap in characters for an engine: CLIs get the large one.
pub fn history_chars(engine: &str) -> usize {
    match CATALOG.iter().find(|e| e.id == engine).map(|e| e.kind) {
        Some(Kind::Cli) => CLI_HISTORY_CHARS,
        _ => SERVER_HISTORY_CHARS,
    }
}

/// The messages of one turn and whether older ones were left out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Context {
    pub messages: Vec<ChatMessage>,
    pub omitted: bool,
}

/// The most recent messages that fit `MAX_HISTORY_MESSAGES` and `max_chars`,
/// oldest dropped first. The newest message is always kept whole. The window
/// starts on a user message, because chat APIs reject a leading assistant one;
/// any assistant messages that would lead it count as omitted.
pub fn build_context(history: &[ChatMessage], max_chars: usize) -> Context {
    fit(history, max_chars, |m| m.content.chars().count())
}

/// [`build_context`] counting estimated tokens instead of characters.
pub fn build_context_tokens(history: &[ChatMessage], max_tokens: usize) -> Context {
    fit(history, max_tokens, |m| budget::estimate_tokens(&m.content))
}

/// The window of [`build_context`], with each message weighed by `cost`.
fn fit(history: &[ChatMessage], max: usize, cost: impl Fn(&ChatMessage) -> usize) -> Context {
    let mut used = 0;
    let mut start = history.len();
    for (i, message) in history.iter().enumerate().rev() {
        let size = cost(message);
        let newest = i + 1 == history.len();
        let fits = history.len() - i <= MAX_HISTORY_MESSAGES && used + size <= max;
        if !newest && !fits {
            break;
        }
        used += size;
        start = i;
    }
    while start + 1 < history.len() && history[start].role == Role::Assistant {
        start += 1;
    }
    Context {
        messages: history[start..].to_vec(),
        omitted: start > 0,
    }
}

#[cfg(test)]
mod availability_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod usage_tests;
