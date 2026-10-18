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

/// The flags lazarobox itself passes to claude: the argv of `claude_invocation`
/// plus the tool wiring (`tools::wiring::claude`). A test keeps this list equal
/// to what those builders emit.
const CLAUDE_FLAGS: &[&str] = &[
    "-p",
    "--output-format",
    "--verbose",
    "--include-partial-messages",
    "--tools",
    "--strict-mcp-config",
    "--safe-mode",
    "--disable-slash-commands",
    "--permission-prompts",
    "--no-session-persistence",
    "--system-prompt",
    "--model",
    "--mcp-config",
    "--allowedTools",
    "--setting-sources",
];

/// The flags lazarobox passes to codex (`codex_invocation`).
const CODEX_FLAGS: &[&str] = &[
    "-s",
    "--json",
    "--ephemeral",
    "--ignore-user-config",
    "--ignore-rules",
    "--skip-git-repo-check",
    "--disable",
    "-c",
    "--model",
];

/// The flags lazarobox passes to gemini (`gemini_invocation`, its policy file
/// and the tool wiring).
const GEMINI_FLAGS: &[&str] = &[
    "--skip-trust",
    "-p",
    "-o",
    "--approval-mode",
    "-e",
    "--allowed-mcp-server-names",
    "--model",
    "--policy",
    "--admin-policy",
];

/// The flags we pass to the CLI called `name`; none for a name we do not know.
fn known_flags(name: &str) -> &'static [&'static str] {
    match name {
        "claude" => CLAUDE_FLAGS,
        "codex" => CODEX_FLAGS,
        "gemini" => GEMINI_FLAGS,
        _ => &[],
    }
}

/// What commander.js (the CLIs' parser) writes for a flag it does not know,
/// quoted with `'` or `"`. Exact: anything around it is some other message.
fn unknown_option_arg(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("error: unknown option ")?;
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = rest[1..].strip_suffix(quote)?;
    (!inner.contains(quote)).then_some(inner)
}

/// The word the CLI rejected, read from the raw stderr: the argument of the
/// last non-empty line, or of the line before a closing `(Did you mean ...?)`
/// hint. No other line counts, so an older complaint never hides the real
/// final error.
fn rejected_option(stderr: &str) -> Option<&str> {
    let mut lines = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !sanitize(line).trim().is_empty())
        .rev();
    let mut line = lines.next()?;
    if line.starts_with("(Did you mean ") && line.ends_with(')') {
        line = lines.next()?;
    }
    unknown_option_arg(line)
}

/// What to tell the user when the CLI `name` rejected an option: it is older
/// than lazarobox needs. The flag is named only when it is one of ours (a
/// constant, never text from stderr); any other word is not echoed.
///
/// The fix is always to update the CLI, never to relax the call. The text-only
/// options (`--safe-mode`, `--permission-prompts none`, `--tools ""`,
/// `--strict-mcp-config`) are what keep claude from running tools when it runs
/// without the jail, so none of them is dropped to fit an old CLI.
fn outdated_cli(name: &str, stderr: &str) -> Option<String> {
    let rejected = rejected_option(stderr)?;
    let how = if name == "claude" {
        "update it (claude update)"
    } else {
        "update it"
    };
    Some(
        match known_flags(name).iter().find(|flag| **flag == rejected) {
            Some(flag) => format!(
                "{name} does not know the option {flag}: this {name} is older than lazarobox \
             needs; {how} and try again"
            ),
            None => format!(
                "this {name} is older than lazarobox needs: it rejected an option; {how} and try again"
            ),
        },
    )
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
            outdated_cli(name, stderr)
                .or_else(|| last_line(stderr).map(|line| redact_tokens(&sanitize(&line))))
                .unwrap_or(reason),
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
