//! How a CLI turn ends. Pure: the runtime collects the facts (the decoder's
//! final event, whether any answer text went out, how the process exited and
//! what it wrote to stderr) and this decides the one final event of the turn.

use super::engine::{ChatError, StreamEvent};
use crate::core::ai::jail::exit_error;
use crate::core::ai::parse::sanitize;
use crate::core::ai::sanitize::redact_tokens;

/// Most bytes of stderr kept for a turn: the tail, which is where the reason
/// for a failure is.
pub const STDERR_CAP: usize = 16 * 1024;

/// How the process ended, when it was reaped on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exit {
    /// `None` when a signal ended it.
    pub code: Option<i32>,
}

impl Exit {
    fn success(self) -> bool {
        self.code == Some(0)
    }
}

/// The last line of `stderr` that still has text once its escapes and
/// control characters are gone, trimmed.
pub fn last_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !sanitize(line).trim().is_empty())
        .map(str::to_owned)
}

/// The final event of a turn.
///
/// - `last`: the decoder's final event, if it produced one;
/// - `answered`: some answer text went out;
/// - `exit`: `None` when we stopped the process ourselves after the final
///   event, so its status says nothing;
/// - `stderr`: what the process wrote (the tail).
///
/// The jail marker wins over everything: a helper that could not enforce the
/// rules never ran the CLI. Otherwise a failing exit beats a `Done`, an engine
/// error is kept as the decoder said it, and an answer needs text.
pub fn verdict(
    name: &str,
    last: Option<StreamEvent>,
    answered: bool,
    exit: Option<Exit>,
    stderr: &str,
) -> StreamEvent {
    if let Some(error) = exit_error(exit.and_then(|e| e.code), stderr) {
        return StreamEvent::Error(error);
    }
    let process_failure = || {
        let reason = match exit {
            Some(Exit { code: Some(code) }) => format!("{name} exited with status {code}"),
            _ => format!("{name} was stopped by a signal"),
        };
        StreamEvent::Error(ChatError::Failed(
            last_line(stderr).map_or(reason, |line| redact_tokens(&line)),
        ))
    };
    let failed_exit = exit.is_some_and(|e| !e.success());
    match last {
        Some(StreamEvent::Done(_)) if failed_exit => process_failure(),
        Some(StreamEvent::Done(_)) if !answered => {
            StreamEvent::Error(ChatError::Failed(format!("{name} returned no answer")))
        }
        Some(event @ StreamEvent::Done(_)) => event,
        Some(StreamEvent::Error(ChatError::Interrupted)) | None if failed_exit => process_failure(),
        Some(event @ StreamEvent::Error(_)) => event,
        _ => StreamEvent::Error(ChatError::Interrupted),
    }
}

#[cfg(test)]
mod tests;
