//! What `list_panes` tells a model about the user's panes. Pure: the runtime
//! reads `/proc` and hands the facts here as rows.
//!
//! The command line of another process is data the user did not write for the
//! model, and may hold secrets, so every argument is masked, redacted and cut
//! before it is written. The environment of other processes is never read.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use crate::core::ai::sanitize::redact_tokens;

/// The longest argument written, in characters.
pub const ARG_CAP: usize = 120;
/// The longest line written for one pane, in characters.
pub const LINE_CAP: usize = 300;
/// The most bytes of `/proc/<pid>/cmdline` that are looked at.
pub const CMDLINE_CAP: usize = 8192;
/// What a masked value is shown as.
const MASK: &str = "***";

/// One pane as the runtime read it. A fact that could not be read is `None`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PaneRow {
    pub number: u32,
    pub focused: bool,
    pub cwd: Option<PathBuf>,
    /// The argv of the foreground process group leader.
    pub argv: Option<Vec<String>>,
}

/// The last reading of the panes: written by the runtime on its poll, read by
/// the tool host. Shared, never persisted.
#[derive(Clone, Default)]
pub struct Roster(Arc<Mutex<Vec<PaneRow>>>);

impl Roster {
    pub fn set(&self, rows: Vec<PaneRow>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = rows;
    }

    pub fn rows(&self) -> Vec<PaneRow> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The argv held by the bytes of a `cmdline` file: NUL-separated words. `None`
/// when no word is left (a kernel thread, a zombie).
pub fn parse_cmdline(raw: &[u8]) -> Option<Vec<String>> {
    let words: Vec<String> = raw
        .split(|byte| *byte == 0)
        .filter(|word| !word.is_empty())
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect();
    (!words.is_empty()).then_some(words)
}

/// Whether the key of a `KEY=value` word looks like it names a secret.
fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASS"]
        .iter()
        .any(|needle| key.contains(needle))
}

/// `argv` with the secrets masked, redacted, shortened and cut. The first
/// word is the program, shown by its file name.
pub fn safe_argv(argv: &[String], home: Option<&Path>) -> Vec<String> {
    let mut mask_next = false;
    argv.iter()
        .enumerate()
        .map(|(index, word)| {
            let shown = if index == 0 {
                word.rsplit('/').next().unwrap_or(word).to_owned()
            } else if std::mem::take(&mut mask_next) {
                MASK.to_owned()
            } else if matches!(word.as_str(), "--password" | "--token" | "-p") {
                mask_next = true;
                word.clone()
            } else if let Some((key, _)) = word.split_once('=').filter(|(key, _)| secret_key(key)) {
                format!("{key}={MASK}")
            } else if word.len() > 2 && word.starts_with("-p") && !word.starts_with("--") {
                format!("-p{MASK}")
            } else {
                redact_tokens(word)
            };
            cap(&flat(&tilde(&shown, home)), ARG_CAP)
        })
        .collect()
}

/// `text` with every path under `home` written from `~`.
fn tilde(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str).filter(|h| h.len() > 1) else {
        return text.to_owned();
    };
    let under = format!("{home}/");
    let text = text.replace(&under, "~/");
    if text == home { "~".to_owned() } else { text }
}

/// `text` without control characters.
fn flat(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `text` cut at `max` characters, with an ellipsis when it was cut.
fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

/// The text a model gets: one line per pane.
pub fn render(rows: &[PaneRow], home: Option<&Path>) -> String {
    if rows.is_empty() {
        return "no panes".to_owned();
    }
    rows.iter()
        .map(|row| {
            let cwd = row
                .cwd
                .as_deref()
                .and_then(Path::to_str)
                .map_or_else(|| "unknown".to_owned(), |cwd| flat(&tilde(cwd, home)));
            let program = match &row.argv {
                Some(argv) => safe_argv(argv, home).join(" "),
                None => "unknown".to_owned(),
            };
            let focus = if row.focused { " (focused)" } else { "" };
            cap(
                &format!("pane {}{focus} cwd: {cwd} program: {program}", row.number),
                LINE_CAP,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
