//! System prompts: the built-in text of each profile and task, and the pure
//! choice between it and the user's override files. Pure: the files are read
//! by the runtime and arrive here as `Override` values.

use crate::core::ai::{
    config::{Profile, Task},
    sanitize::clean,
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
