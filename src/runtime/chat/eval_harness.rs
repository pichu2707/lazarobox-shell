//! Test support of the local-model evaluation (openspec `local-multistep`).
//! It lives in the chat module because it reaches the private Ollama loop.

use super::ollama;
use crate::{
    core::ai::{
        eval::refused,
        tools::{ToolError, mcp::refusal_text, paths::REFUSED},
    },
    runtime::tools::host::NOT_APPROVED,
};

/// Every text the host or the loop gives a call that did not run must be one
/// `eval::refused` knows, or a refused step would score as completed.
#[test]
fn the_refusal_prefixes_cover_the_real_constants() {
    let errors = [
        ToolError::UnknownTool,
        ToolError::NotAnObject,
        ToolError::MissingArgument,
        ToolError::WrongType,
        ToolError::ExtraArgument,
        ToolError::TooLong,
        ToolError::PaneRange,
        ToolError::ScrollbackRange,
        ToolError::DraftKind,
        ToolError::DraftEmpty,
        ToolError::DraftTitleLong,
        ToolError::DraftTitleLines,
        ToolError::DraftBodyLong,
    ];
    for error in errors {
        assert!(refused(refusal_text(error)), "{error:?}");
    }
    for text in [
        ollama::FAILED,
        ollama::DROPPED,
        ollama::TOO_MANY,
        REFUSED,
        &format!("denied: {NOT_APPROVED}"),
    ] {
        assert!(refused(text), "{text}");
    }
}
