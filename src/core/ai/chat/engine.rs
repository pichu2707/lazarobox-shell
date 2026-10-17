//! The contract between the chat core and an engine adapter: what a turn
//! sends and what a stream may say back, and whether an engine can be used.
//! Pure data and functions, no IO.

use super::{adapters::adapter_for, approval::ApprovalAsk};
use crate::core::ai::{
    CATALOG, Detection, Kind, Status,
    config::{AiConfig, Task},
    jail::JailSupport,
    status_text,
    tools::Locality,
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
    Sent,
    /// Why nothing was done; short, and it goes back to the model.
    Denied(&'static str),
}

/// Whether the adapter of `engine` keeps what a tool returns on this machine.
/// `None` for an engine that is not in the catalog.
pub fn locality(engine: &str) -> Option<Locality> {
    CATALOG
        .iter()
        .find(|e| e.id == engine)
        .map(|e| e.kind.locality())
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
    Done(Finish),
    Error(ChatError),
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
    GatedOff {
        engine: String,
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
    availability_with(cfg, detection, jail, GATED_OFF, &has_adapter)
}

/// The CLIs whose manual check failed. None yet: the checks run in C13-C15.
const GATED_OFF: &[&str] = &[];

/// CLIs that must never start without a working jail.
const JAIL_REQUIRED: &[&str] = &["codex", "gemini"];

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
    let (assignment, _) = cfg.resolve(Task::Chat);
    let Some(engine) = assignment.engine else {
        return Availability::Unassigned;
    };
    if !adapter_exists(&engine) {
        return Availability::Unsupported { engine };
    }
    let Some(detection) = detection else {
        return Availability::Checking;
    };
    let available = detection
        .engines
        .iter()
        .find(|s| s.id == engine)
        .filter(|s| s.status == Status::Available);
    let Some(state) = available else {
        let status = status_text(Some(detection), &engine);
        return Availability::Undetected { engine, status };
    };
    if let JailSupport::Unavailable(off) = jail
        && JAIL_REQUIRED.contains(&engine.as_str())
    {
        return Availability::NoJail {
            engine,
            reason: off.text(),
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
/// has landed (ollama, claude) can chat; the others are unsupported.
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
    let mut used = 0;
    let mut start = history.len();
    for (i, message) in history.iter().enumerate().rev() {
        let chars = message.content.chars().count();
        let newest = i + 1 == history.len();
        let fits = history.len() - i <= MAX_HISTORY_MESSAGES && used + chars <= max_chars;
        if !newest && !fits {
            break;
        }
        used += chars;
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
