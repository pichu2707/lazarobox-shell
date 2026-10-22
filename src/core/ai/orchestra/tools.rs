//! The four orchestration tools: the validation of a call. Their schemas are
//! rows of the tool table (`tools::BUILT_INS`).

use std::borrow::Cow;

use serde_json::{Map, Value};

use super::WorkerId;
use crate::core::ai::config::Task;
use crate::core::ai::tools::ToolError;

/// The longest `wait`, in seconds: a request for more is clamped to it.
pub const WAIT_MAX_SECS: u16 = 120;

/// The longest worker prompt accepted, in bytes.
pub const MAX_PROMPT_BYTES: usize = 16 * 1024;

/// The most ids one `wait` may name.
pub const MAX_WAIT_IDS: usize = 32;

/// A validated orchestration call.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OrchCall {
    Spawn {
        task: Task,
        prompt: String,
    },
    /// `timeout` is in seconds, already clamped to [`WAIT_MAX_SECS`].
    Wait {
        ids: Vec<WorkerId>,
        timeout: u16,
    },
    Collect {
        id: WorkerId,
    },
    Cancel {
        id: WorkerId,
    },
}

impl OrchCall {
    /// The tool's name, as the registry offers it.
    pub fn name(&self) -> &'static str {
        match self {
            OrchCall::Spawn { .. } => "spawn_agent",
            OrchCall::Wait { .. } => "wait",
            OrchCall::Collect { .. } => "collect",
            OrchCall::Cancel { .. } => "cancel",
        }
    }

    /// What the call is about, for a one-line display: the prompt, the ids
    /// (comma-separated) or the id.
    pub fn argument(&self) -> Cow<'_, str> {
        match self {
            OrchCall::Spawn { prompt, .. } => Cow::Borrowed(prompt),
            OrchCall::Wait { ids, .. } => Cow::Owned(
                ids.iter()
                    .map(|id| id.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            OrchCall::Collect { id } | OrchCall::Cancel { id } => Cow::Owned(id.0.to_string()),
        }
    }
}

/// Why a call was refused. Short and content-free: it may go back to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrchError {
    UnknownTool,
    NotAnObject,
    MissingArgument,
    WrongType,
    ExtraArgument,
    TooLong,
    /// Not one of the five worker kinds. `orchestrate` is not spawnable.
    BadKind,
    EmptyPrompt,
}

impl From<OrchError> for ToolError {
    /// The model-facing refusal. A bad kind is a wrong value; an empty
    /// prompt is a missing one.
    fn from(error: OrchError) -> Self {
        match error {
            OrchError::UnknownTool => ToolError::UnknownTool,
            OrchError::NotAnObject => ToolError::NotAnObject,
            OrchError::MissingArgument | OrchError::EmptyPrompt => ToolError::MissingArgument,
            OrchError::WrongType | OrchError::BadKind => ToolError::WrongType,
            OrchError::ExtraArgument => ToolError::ExtraArgument,
            OrchError::TooLong => ToolError::TooLong,
        }
    }
}

fn spawnable() -> [Task; 5] {
    Task::SET_ALL
}

/// Checks an orchestration call by name against its schema.
pub fn parse(name: &str, args: &Value) -> Result<OrchCall, OrchError> {
    if !matches!(name, "spawn_agent" | "wait" | "collect" | "cancel") {
        return Err(OrchError::UnknownTool);
    }
    let object = args.as_object().ok_or(OrchError::NotAnObject)?;
    match name {
        "spawn_agent" => spawn(object),
        "wait" => wait(object),
        "collect" => only_id(object).map(|id| OrchCall::Collect { id }),
        _ => only_id(object).map(|id| OrchCall::Cancel { id }),
    }
}

fn spawn(object: &Map<String, Value>) -> Result<OrchCall, OrchError> {
    let kind = text(object, "kind")?;
    let prompt = text(object, "prompt")?;
    no_extra(object, 2)?;
    let task = spawnable()
        .into_iter()
        .find(|task| task.id() == kind)
        .ok_or(OrchError::BadKind)?;
    if prompt.is_empty() {
        return Err(OrchError::EmptyPrompt);
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(OrchError::TooLong);
    }
    Ok(OrchCall::Spawn {
        task,
        prompt: prompt.to_owned(),
    })
}

fn wait(object: &Map<String, Value>) -> Result<OrchCall, OrchError> {
    let list = object
        .get("ids")
        .ok_or(OrchError::MissingArgument)?
        .as_array()
        .ok_or(OrchError::WrongType)?;
    let timeout = match object.get("timeout") {
        None => WAIT_MAX_SECS,
        Some(value) => {
            let secs = value.as_u64().ok_or(OrchError::WrongType)?;
            u16::try_from(secs.min(u64::from(WAIT_MAX_SECS))).unwrap_or(WAIT_MAX_SECS)
        }
    };
    no_extra(object, 1 + usize::from(object.contains_key("timeout")))?;
    if list.is_empty() {
        return Err(OrchError::MissingArgument);
    }
    if list.len() > MAX_WAIT_IDS {
        return Err(OrchError::TooLong);
    }
    let ids = list.iter().map(worker_id).collect::<Result<_, _>>()?;
    Ok(OrchCall::Wait { ids, timeout })
}

fn only_id(object: &Map<String, Value>) -> Result<WorkerId, OrchError> {
    let id = worker_id(object.get("id").ok_or(OrchError::MissingArgument)?)?;
    no_extra(object, 1)?;
    Ok(id)
}

fn worker_id(value: &Value) -> Result<WorkerId, OrchError> {
    let id = value.as_u64().ok_or(OrchError::WrongType)?;
    u32::try_from(id)
        .map(WorkerId)
        .map_err(|_| OrchError::WrongType)
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, OrchError> {
    object
        .get(key)
        .ok_or(OrchError::MissingArgument)?
        .as_str()
        .ok_or(OrchError::WrongType)
}

fn no_extra(object: &Map<String, Value>, known: usize) -> Result<(), OrchError> {
    if object.len() > known {
        return Err(OrchError::ExtraArgument);
    }
    Ok(())
}
