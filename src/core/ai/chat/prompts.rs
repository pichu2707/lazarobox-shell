//! System prompts: the built-in text of each profile and task, and the pure
//! choice between it and the user's override files. Pure: the files are read
//! by the runtime and arrive here as `Override` values.

use crate::core::ai::{
    config::{Profile, Task},
    orchestra::rdd::Lens,
    sanitize::clean,
    tools::registry::{Family, FamilySet},
};

/// Why an override file was skipped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    NotUtf8,
    TooLarge,
    Unreadable,
    NotRegular,
}

/// What the runtime found for one override file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Override {
    /// No file, or one with nothing but whitespace.
    Missing,
    /// The valid content of the file.
    Text(String),
    /// The file exists but cannot be used.
    Skipped(Reason),
}

/// The chosen prompt and the notices of the override files skipped on the way.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    pub text: String,
    pub notices: Vec<String>,
}

/// The built-in prompt of a profile and task.
pub fn builtin(profile: Profile, task: Task) -> &'static str {
    match (profile, task) {
        (Profile::Analyst, Task::General) => include_str!("prompts/analyst/general.md"),
        (Profile::Analyst, Task::Code) => include_str!("prompts/analyst/code.md"),
        (Profile::Analyst, Task::Chat) => include_str!("prompts/analyst/chat.md"),
        (Profile::Analyst, Task::Review) => include_str!("prompts/analyst/review.md"),
        (Profile::Analyst, Task::Summarize) => include_str!("prompts/analyst/summarize.md"),
        (Profile::Programmer, Task::General) => include_str!("prompts/programmer/general.md"),
        (Profile::Programmer, Task::Code) => include_str!("prompts/programmer/code.md"),
        (Profile::Programmer, Task::Chat) => include_str!("prompts/programmer/chat.md"),
        (Profile::Programmer, Task::Review) => include_str!("prompts/programmer/review.md"),
        (Profile::Programmer, Task::Summarize) => {
            include_str!("prompts/programmer/summarize.md")
        }
        // The coordinator rules, then the ODD playbook of the steps (S5).
        (Profile::Analyst, Task::Orchestrate) => concat!(
            include_str!("prompts/analyst/orchestrate.md"),
            include_str!("prompts/analyst/orchestrate_odd.md")
        ),
        (Profile::Programmer, Task::Orchestrate) => concat!(
            include_str!("prompts/programmer/orchestrate.md"),
            include_str!("prompts/programmer/orchestrate_odd.md")
        ),
    }
}

/// The built-in prompt of a review lens (S7b): read-only, the change is
/// untrusted data, only a severe finding the change caused blocks, a fixed
/// shape of findings, never an approval. Built-in only: lens prompts have no
/// override file.
pub fn lens_builtin(profile: Profile, lens: Lens) -> &'static str {
    match (profile, lens) {
        (Profile::Analyst, Lens::Risk) => include_str!("prompts/analyst/lens_risk.md"),
        (Profile::Analyst, Lens::Resilience) => include_str!("prompts/analyst/lens_resilience.md"),
        (Profile::Analyst, Lens::Readability) => {
            include_str!("prompts/analyst/lens_readability.md")
        }
        (Profile::Analyst, Lens::Reliability) => {
            include_str!("prompts/analyst/lens_reliability.md")
        }
        (Profile::Programmer, Lens::Risk) => include_str!("prompts/programmer/lens_risk.md"),
        (Profile::Programmer, Lens::Resilience) => {
            include_str!("prompts/programmer/lens_resilience.md")
        }
        (Profile::Programmer, Lens::Readability) => {
            include_str!("prompts/programmer/lens_readability.md")
        }
        (Profile::Programmer, Lens::Reliability) => {
            include_str!("prompts/programmer/lens_reliability.md")
        }
    }
}

/// The built-in prompt of the refuter (S7b), with the same rules.
pub fn refuter_builtin(profile: Profile) -> &'static str {
    match profile {
        Profile::Analyst => include_str!("prompts/analyst/refuter.md"),
        Profile::Programmer => include_str!("prompts/programmer/refuter.md"),
    }
}

/// The prompt for `task`: the task's override, then the `general` override,
/// then the built-in. A skipped file adds a notice and resolution goes on.
/// A text is cut at the end only; one with nothing but whitespace counts as
/// missing. The `general` task has a single level.
pub fn resolve_prompt(
    profile: Profile,
    task: Task,
    task_file: Override,
    general_file: Override,
) -> Resolved {
    let mut notices = Vec::new();
    let levels = [(task, task_file), (Task::General, general_file)];
    for (level, file) in levels
        .into_iter()
        .take(if task == Task::General { 1 } else { 2 })
    {
        match file {
            Override::Text(text) if !text.trim().is_empty() => {
                return Resolved {
                    text: text.trim_end().to_owned(),
                    notices,
                };
            }
            Override::Text(_) | Override::Missing => {}
            Override::Skipped(reason) => notices.push(notice(profile, level, reason)),
        }
    }
    Resolved {
        text: builtin(profile, task).trim_end().to_owned(),
        notices,
    }
}

/// The sentence of the built-in chat prompts that says the model has no tools.
const NO_TOOLS: &str = "You are a text assistant. You have no access to files, a shell or the terminal: you cannot read, run or change anything. When something depends on what is on the user's machine, ask them to paste it.";

/// What a turn with tools says about them instead of [`NO_TOOLS`]. Names the
/// tools by purpose, not by the name an engine gives them.
const TOOLS: &str = include_str!("prompts/tools.md");

/// The sentence of [`TOOLS`] that says the model cannot change files.
const NO_CHANGES: &str =
    "You cannot change files yourself; any change needs a command the user reviews and runs.";

/// What a turn that offers the write, exec or git write tools says instead of
/// [`NO_CHANGES`]: it can do the work, and must finish every step (#439).
/// `{abilities}` names what the families of the turn allow.
const CODING: &str = include_str!("prompts/coding.md");

/// What each coding family lets the model do, in [`CODING`]'s words.
const ABILITIES: [(Family, &str); 3] = [
    (
        Family::Write,
        "write and edit files (they stay staged until you propose the batch, which the user reviews once)",
    ),
    (Family::Exec, "run tests and builds"),
    (Family::GitWrite, "add and commit with git"),
];

/// `system` for a turn that offers tools: the built-in "no access" sentence
/// becomes the tools block. A prompt without that sentence (the user's own
/// file) gets the block as a last bullet instead. Only a turn with tools calls
/// this: any other keeps its prompt byte for byte. A turn that offers the
/// write, exec or git write tools gets [`CODING`] instead of [`NO_CHANGES`].
pub fn with_tools(system: &str, families: FamilySet) -> String {
    let abilities: Vec<&str> = ABILITIES
        .iter()
        .filter(|(family, _)| families.contains(*family))
        .map(|(_, ability)| *ability)
        .collect();
    let block = match abilities.split_last() {
        Some((last, [])) => coding(last),
        Some((last, rest)) => coding(&format!("{} and {last}", rest.join(", "))),
        None => TOOLS.trim_end().to_owned(),
    };
    let block = block.as_str();
    if system.trim().is_empty() {
        return block.to_owned();
    }
    if system.contains(NO_TOOLS) {
        return system.replacen(NO_TOOLS, block, 1);
    }
    format!("{}\n\n- {block}", system.trim_end())
}

/// [`TOOLS`] with [`CODING`] in place of [`NO_CHANGES`].
fn coding(abilities: &str) -> String {
    let block = CODING.trim_end().replace("{abilities}", abilities);
    TOOLS.trim_end().replacen(NO_CHANGES, &block, 1)
}

/// The system line for a skipped override file.
fn notice(profile: Profile, task: Task, reason: Reason) -> String {
    let why = match reason {
        Reason::NotUtf8 => "not valid UTF-8",
        Reason::TooLarge => "larger than 16 KiB",
        Reason::Unreadable => "cannot be read",
        Reason::NotRegular => "not a regular file",
    };
    clean(&format!(
        "prompt override ignored: {}/{}.md {why}",
        profile.id(),
        task.id()
    ))
}

#[cfg(test)]
mod tests;
