//! `gh_issue_draft`: the model's draft of an issue for the Feedback form. Pure:
//! the arguments are checked here, nothing runs and nothing reaches GitHub. A
//! field that would not fit the form is refused, never cut (D4).

use super::{
    ToolError,
    registry::{ArgDef, ArgKind, ArgValue, Parsed},
};
use crate::core::{
    ai::chat::composer::Composer,
    menu::feedback::{DESCRIPTION_CAP, Kind, TITLE_CAP},
};

/// A byte bound well above the form's caps, so an over-long field reaches
/// the character check and the model is told which field to shorten.
const BYTES: ArgKind = ArgKind::Str { max: 64 * 1024 };

const fn arg(name: &'static str) -> ArgDef {
    ArgDef {
        name,
        kind: BYTES,
        required: true,
        gate: None,
    }
}

pub const ARGS: &[ArgDef] = &[arg("kind"), arg("title"), arg("description")];

/// A draft the user is asked about, as the form will hold it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IssueDraft {
    pub kind: Kind,
    pub title: String,
    pub description: String,
}

impl IssueDraft {
    /// The line of the call in the transcript.
    pub fn argument(&self) -> String {
        format!("{}: {}", self.kind.label(), self.title)
    }
}

/// The draft the arguments describe, or why the model must rewrite it.
pub fn parse(parsed: &Parsed) -> Result<IssueDraft, ToolError> {
    let text = |name| match parsed.get(name) {
        Some(ArgValue::Str(text)) => Ok(text.as_str()),
        _ => Err(ToolError::MissingArgument),
    };
    let kind = match text("kind")? {
        "idea" => Kind::Idea,
        "bug" => Kind::Bug,
        _ => return Err(ToolError::DraftKind),
    };
    let title = field(text("title")?, TITLE_CAP, ToolError::DraftTitleLong)?;
    if title.contains('\n') {
        return Err(ToolError::DraftTitleLines);
    }
    let description = field(
        text("description")?,
        DESCRIPTION_CAP,
        ToolError::DraftBodyLong,
    )?;
    Ok(IssueDraft {
        kind,
        title,
        description,
    })
}

/// `text` as the form will hold it: the composer cleans it (a tab grows into
/// spaces, a control character goes), so the cap is checked on what would be
/// pasted and a text that fits is never cut.
fn field(text: &str, cap: usize, too_long: ToolError) -> Result<String, ToolError> {
    if text.chars().count() > cap {
        return Err(too_long);
    }
    let mut held = Composer::with_cap(usize::MAX);
    held.paste(text);
    let held = held.text();
    let held = held.trim();
    if held.is_empty() {
        return Err(ToolError::DraftEmpty);
    }
    if held.chars().count() > cap {
        return Err(too_long);
    }
    Ok(held.to_owned())
}

#[cfg(test)]
mod tests;
