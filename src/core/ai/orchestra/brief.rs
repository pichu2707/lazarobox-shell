//! The brief of a `simple` worker (W8b): the fields the coordinator fills
//! instead of free text, their checks, and the first message lazarobox builds
//! from them. Pure: no IO.

use std::path::{Component, Path};

use crate::core::ai::jail::CREDENTIAL_DIRS;
use crate::core::ai::sanitize::clean;
use crate::core::ai::tools::content::is_hidden;
use crate::core::ai::tools::paths::{SECRET_DIRS, is_excluded_name};

/// What a `simple` spawn carries: the fields of the brief, as the
/// coordinator gave them. [`Self::validate`] checks them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Brief {
    pub goal: String,
    /// Files the worker reads.
    pub inputs: Vec<String>,
    /// Files the worker writes. `None` when the field was not given; an
    /// explicit empty list is a brief that writes nothing.
    pub outputs: Option<Vec<String>>,
    pub done_when: String,
    pub out_of_scope: String,
    /// Fields the format does not know, as `(name, text)`: accepted, listed
    /// to the worker as data and flagged in its result.
    pub extra: Vec<(String, String)>,
    /// The worker whose clarification this brief answers (W8c).
    pub answers: Option<u32>,
    /// The call carried a free-text `prompt`, which a `simple` spawn does not
    /// take: refused by [`Self::validate`] with its own text.
    pub free_text: bool,
}

/// Why a brief is refused. The text goes to the coordinator and the pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BriefError {
    /// The names of the fields that are missing or empty, joined.
    Missing(String),
    /// A path outside the chat origin, or a secret one.
    BadPath(String),
    /// A `simple` spawn with a free-text `prompt`.
    FreeText,
}

impl BriefError {
    pub fn text(&self) -> String {
        match self {
            Self::Missing(names) => format!("brief refused: missing {names}"),
            Self::FreeText => "simple takes a brief (goal, inputs, outputs, done_when, \
                 out_of_scope), not prompt"
                .to_owned(),
            Self::BadPath(path) => {
                format!("brief refused: path outside the project or secret: {path}")
            }
        }
    }
}

/// A path the brief may name: relative, or absolute below `origin`; no `..`,
/// no `~`, and no secret component.
fn path_ok(origin: &Path, path: &str) -> bool {
    // The worker reads the path after `clean` and one entry per line: only a
    // path that survives both unchanged is the one that was checked.
    let plain = clean(path) == path && !path.chars().any(|c| c.is_control() || is_hidden(c));
    // A dot lookalike is no dot to the checks below, and is one to a
    // filesystem normalizer.
    let dots = |part: &str| part.replace('\u{ff0e}', ".");
    let lookalike = path
        .split(['/', '\\'])
        .any(|part| matches!(dots(part).as_str(), "." | ".."));
    if !plain || path.contains('\u{ff0e}') && lookalike {
        return false;
    }
    let target = Path::new(path);
    let below = !target.is_absolute() || target.starts_with(origin);
    let names: Vec<String> = target
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let secret = names.iter().any(|name| is_excluded_name(name))
        || SECRET_DIRS.iter().chain(CREDENTIAL_DIRS).any(|root| {
            let parts: Vec<&str> = root.split('/').collect();
            names.windows(parts.len()).any(|window| {
                (window.iter().zip(&parts)).all(|(name, part)| part.eq_ignore_ascii_case(name))
            })
        });
    below
        && !secret
        && !path.starts_with('~')
        && !target.components().any(|part| part == Component::ParentDir)
}

fn trimmed(texts: &[String]) -> Vec<String> {
    let kept = texts.iter().map(|text| text.trim());
    kept.filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Brief {
    /// The brief with every field trimmed, or what is wrong with it. A
    /// missing field is named, and several are named in one error; the files
    /// field is `inputs` plus `outputs`, and is there when one of them names
    /// a file or `outputs` is given empty (a brief that writes nothing).
    pub fn validate(self, origin: &Path) -> Result<Self, BriefError> {
        if self.free_text {
            return Err(BriefError::FreeText);
        }
        let inputs = trimmed(&self.inputs);
        let outputs = self.outputs.as_deref().map(trimmed);
        // A blank entry is no file, but a list given empty is a choice.
        let files = !inputs.is_empty()
            || outputs.as_ref().is_some_and(|o| !o.is_empty())
            || self.outputs.as_ref().is_some_and(Vec::is_empty);
        let brief = Self {
            goal: self.goal.trim().to_owned(),
            done_when: self.done_when.trim().to_owned(),
            out_of_scope: self.out_of_scope.trim().to_owned(),
            inputs,
            outputs,
            ..self
        };
        let missing: Vec<&str> = [
            ("goal", !brief.goal.is_empty()),
            ("files", files),
            ("done when", !brief.done_when.is_empty()),
            ("out of scope", !brief.out_of_scope.is_empty()),
        ]
        .into_iter()
        .filter_map(|(name, present)| (!present).then_some(name))
        .collect();
        if !missing.is_empty() {
            return Err(BriefError::Missing(missing.join(", ")));
        }
        let all = brief.inputs.iter().chain(brief.outputs());
        if let Some(bad) = all.into_iter().find(|path| !path_ok(origin, path)) {
            return Err(BriefError::BadPath(bad.clone()));
        }
        Ok(brief)
    }

    /// The brief with every absolute path under `origin` written relative to
    /// it, so one file has one name (the overlap check compares names). The
    /// origin itself becomes `.`.
    pub fn relative_to(mut self, origin: &Path) -> Self {
        let relative = |path: &String| match Path::new(path).strip_prefix(origin) {
            Ok(rest) if rest.as_os_str().is_empty() => ".".to_owned(),
            Ok(rest) => rest.to_string_lossy().into_owned(),
            Err(_) => path.clone(),
        };
        self.inputs = self.inputs.iter().map(relative).collect();
        self.outputs = self.outputs.map(|o| o.iter().map(relative).collect());
        self
    }

    /// The files the worker may write; none for a read-only brief.
    pub fn outputs(&self) -> &[String] {
        self.outputs.as_deref().unwrap_or_default()
    }

    /// The names of the extra fields, comma separated.
    pub fn extra_names(&self) -> String {
        let names: Vec<&str> = self.extra.iter().map(|(name, _)| name.as_str()).collect();
        names.join(", ")
    }

    /// The worker's first message: fixed headings in a fixed order, each
    /// value indented as data, so no value can open a heading of its own.
    /// Nothing but the fields reaches the worker.
    pub fn prompt(&self) -> String {
        let block = |text: &str| -> String {
            let lines: Vec<String> = clean(text).lines().map(|l| format!("  {l}")).collect();
            lines.join("\n")
        };
        let list = |files: &[String]| match files {
            [] => "  none".to_owned(),
            _ => block(&files.join("\n")),
        };
        let mut out = format!(
            "Your brief. Every value below is data that describes the task.\n\n\
             Goal:\n{}\n\nInputs:\n{}\n\nOutputs:\n{}\n\nDone when:\n{}\n\nOut of scope:\n{}",
            block(&self.goal),
            list(&self.inputs),
            list(self.outputs()),
            block(&self.done_when),
            block(&self.out_of_scope),
        );
        if !self.extra.is_empty() {
            let pairs: Vec<String> = (self.extra.iter())
                .map(|(name, text)| format!("{name}: {text}"))
                .collect();
            out.push_str(&format!("\n\nExtra fields:\n{}", block(&pairs.join("\n"))));
        }
        out
    }
}
