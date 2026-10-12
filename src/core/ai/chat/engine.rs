//! Whether the chat can use the engine assigned to it. Pure, no IO.

use crate::core::ai::{
    CATALOG, Detection, Kind, Status,
    config::{AiConfig, Task},
    jail::JailSupport,
    status_text,
};

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

#[cfg(test)]
mod availability_tests;
