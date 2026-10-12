//! The contract between the chat core and an engine adapter: what a turn
//! sends and what a stream may say back, and whether an engine can be used.
//! Pure data and functions, no IO.

use crate::core::ai::{
    CATALOG, Detection, Kind, Status,
    config::{AiConfig, Task},
    jail::JailSupport,
    status_text,
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
    availability_with(cfg, detection, jail, GATED_OFF)
}

/// The CLIs whose manual check failed. None yet: the checks run in C13-C15.
const GATED_OFF: &[&str] = &[];

/// CLIs that must never start without a working jail.
const JAIL_REQUIRED: &[&str] = &["codex", "gemini"];

/// `availability` with an explicit list of CLIs whose manual check failed.
pub fn availability_with(
    cfg: &AiConfig,
    detection: Option<&Detection>,
    jail: JailSupport,
    gated_off: &[&str],
) -> Availability {
    let (assignment, _) = cfg.resolve(Task::Chat);
    let Some(engine) = assignment.engine else {
        return Availability::Unassigned;
    };
    if !has_adapter(&engine) {
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
    if JAIL_REQUIRED.contains(&engine.as_str()) && !jail.is_available() {
        return Availability::NoJail { engine };
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

/// Whether an adapter exists for the engine. `opencode` has none.
pub fn has_adapter(id: &str) -> bool {
    id != "opencode"
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
