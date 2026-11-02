//! What `list_panes` tells a model about the user's panes. Pure: the runtime
//! reads `/proc` and hands the facts here.
//!
//! The command line of another process is data the user did not write for the
//! model, and secrets hide in it in more shapes than any list can name
//! (`-u user:pass`, `-a PASS`, a conninfo string, a header). So nothing is
//! shown unless it is known to be harmless: an allowlist, not a denylist.
//! The environment of other processes is never read.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use crate::core::ai::{
    chat::approval_rows::command_rows,
    sanitize::{clean, redact_tokens},
};

/// The longest argument written, in characters.
pub const ARG_CAP: usize = 120;
/// The longest line written for one pane, in characters.
pub const LINE_CAP: usize = 300;
/// The most bytes of `/proc/<pid>/cmdline` that are looked at.
/// The width the capped text is guaranteed to fit the approval prompt at.
pub const PROMPT_COLS: usize = 40;
/// The most panes the text names.
pub const MAX_PANES: usize = 5;
pub const CMDLINE_CAP: usize = 8192;
/// The most arguments of a pane looked up on disk.
const MAX_STATS: usize = 16;
/// The most rows the approval prompt shows of a command.
const PROMPT_ROWS: usize = 6;
/// What stands for every argument that is not known to be harmless.
const HIDDEN: &str = "…";

/// The program in the foreground of a pane.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Foreground {
    pub argv: Vec<String>,
    /// Its own working directory, which may not be the shell's.
    pub cwd: Option<PathBuf>,
}

/// The raw facts of one pane, before they are made safe to show.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Facts {
    pub number: u32,
    pub focused: bool,
    /// The shell's working directory.
    pub cwd: Option<PathBuf>,
    pub fg: Option<Foreground>,
}

/// The last reading of the panes: one already-redacted line each. Written by
/// the runtime on its poll, read by the tool host. Raw argv never gets here.
#[derive(Clone, Default)]
pub struct Roster(Arc<Mutex<Vec<String>>>);

impl Roster {
    pub fn set(&self, lines: Vec<String>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = lines;
    }

    pub fn lines(&self) -> Vec<String> {
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

/// The flag of an interpreter that takes its program inline (`bash -c`,
/// `python -c`, `node -e`...): the text after it is code, shown never.
fn inline_flag(program: &str, args: &[String]) -> Option<String> {
    let takes = |arg: &str| match program {
        "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" => {
            arg.len() > 1
                && arg.starts_with('-')
                && arg.ends_with('c')
                && arg[1..].bytes().all(|b| b.is_ascii_alphabetic())
        }
        "node" | "nodejs" | "perl" | "ruby" => arg == "-e",
        _ => program.starts_with("python") && arg == "-c",
    };
    args.iter().find(|arg| takes(arg)).cloned()
}

/// `-x` or `--long`, without any `=value`; any other shape (`-pSECRET`,
/// `-pass`) is not a flag.
fn flag(arg: &str) -> Option<&str> {
    let name = arg.split('=').next().unwrap_or_default();
    let long = name.strip_prefix("--").filter(|rest| !rest.is_empty());
    let ok = match long {
        Some(rest) => rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        None => {
            name.len() == 2 && name.starts_with('-') && name.as_bytes()[1].is_ascii_alphabetic()
        }
    };
    ok.then_some(name)
}

/// Whether `arg` is a path worth showing: it says it is one, or it names an
/// entry that exists in `cwd` (looked at, never followed). Text that could
/// carry a credential (`?`, `=`, `@`, `:`) never counts.
fn path_like(arg: &str, cwd: Option<&Path>, stats: &mut usize) -> bool {
    if arg.is_empty() || arg.contains(['?', '=', '@', ':', '\0']) {
        return false;
    }
    if ["/", "./", "../", "~/"].iter().any(|p| arg.starts_with(p)) {
        return true;
    }
    // A disk lookup can block (a stalled mount): only a few are made.
    let Some(dir) = cwd.filter(|_| *stats > 0) else {
        return false;
    };
    *stats -= 1;
    std::fs::symlink_metadata(dir.join(arg)).is_ok()
}

/// `argv` as it may be shown: the program's file name, then the flags and the
/// paths, and `…` for everything else (runs of it are one). Interpreters with
/// an inline program show their flag only. `cwd` is where relative names are
/// looked for.
pub fn safe_argv(argv: &[String], cwd: Option<&Path>, home: Option<&Path>) -> Vec<String> {
    let Some((first, args)) = argv.split_first() else {
        return Vec::new();
    };
    // A title a program set for itself (`nginx: master process ...`) is free
    // text, not a name.
    let program = first.rsplit('/').next().unwrap_or(first);
    let name = if first.contains([' ', ':']) {
        HIDDEN.to_owned()
    } else {
        tidy(program, home)
    };
    let mut out = vec![cap(&name, ARG_CAP)];
    if let Some(flag) = inline_flag(program, args) {
        out.push(flag);
        return out;
    }
    let mut stats = MAX_STATS;
    for arg in args {
        let shown = if let Some(name) = flag(arg) {
            cap(&tidy(name, None), ARG_CAP)
        } else if path_like(arg, cwd, &mut stats) {
            cap(&tidy(arg, home), ARG_CAP)
        } else {
            HIDDEN.to_owned()
        };
        if shown != HIDDEN || out.last().map(String::as_str) != Some(HIDDEN) {
            out.push(shown);
        }
    }
    out
}

/// `text` with every path under `home` written from `~`.
fn tilde(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str).filter(|h| h.len() > 1) else {
        return text.to_owned();
    };
    let text = text.replace(&format!("{home}/"), "~/");
    if text == home { "~".to_owned() } else { text }
}

/// `text` as one clean line, as the approval prompt writes it: escape
/// sequences, bidi and zero-width characters and line breaks are gone.
fn flat(text: &str) -> String {
    clean(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` shortened under `home`, redacted and made one clean line.
fn tidy(text: &str, home: Option<&Path>) -> String {
    flat(&redact_tokens(&tilde(text, home)))
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

/// The line of one pane. The focused pane shows its program's allowlisted
/// arguments; any other shows the program's name only. The program's own
/// folder is shown when it differs from the shell's.
pub fn line(facts: &Facts, home: Option<&Path>) -> String {
    let show = |path: &Path| tidy(&path.to_string_lossy(), home);
    let cwd = facts
        .cwd
        .as_deref()
        .map_or_else(|| "unknown".to_owned(), show);
    let (program, running) = match &facts.fg {
        None => ("unknown".to_owned(), String::new()),
        Some(fg) => {
            let work = fg.cwd.as_deref().or(facts.cwd.as_deref());
            let words = if facts.focused {
                &fg.argv[..]
            } else {
                &fg.argv[..fg.argv.len().min(1)]
            };
            let elsewhere = fg
                .cwd
                .as_deref()
                .filter(|dir| Some(*dir) != facts.cwd.as_deref());
            (
                safe_argv(words, work, home).join(" "),
                elsewhere.map_or_else(String::new, |dir| format!(" (running in {})", show(dir))),
            )
        }
    };
    let focus = if facts.focused { " (focused)" } else { "" };
    cap(
        &format!(
            "pane {}{focus} cwd: {cwd} program: {program}{running}",
            facts.number
        ),
        LINE_CAP,
    )
}

/// The text a model gets, and the user approves: the lines of the panes (the
/// focused one first), at most [`MAX_PANES`] and always few enough rows for
/// the approval prompt at [`PROMPT_COLS`] columns, shortening lines and then
/// leaving panes out (`+N more`) as needed, so the ask stays answerable.
pub fn render(lines: &[String]) -> String {
    if lines.is_empty() {
        return "no panes".to_owned();
    }
    let mut shown: Vec<String> = Vec::new();
    for (i, pane) in lines.iter().enumerate().take(MAX_PANES) {
        let tail = (i + 1 < lines.len()).then(|| format!("+{} more", lines.len() - i - 1));
        let mut cand = pane.clone();
        let fits = loop {
            let text = [
                shown.clone(),
                vec![cand.clone()],
                tail.clone().into_iter().collect(),
            ]
            .concat();
            if command_rows(&text.join("\n"), PROMPT_COLS).len() <= PROMPT_ROWS {
                break true;
            }
            let len = cand.chars().count();
            if len <= 24 {
                break false;
            }
            cand = cap(&cand, len * 3 / 4);
        };
        if !fits && i > 0 {
            break;
        }
        shown.push(cand);
    }
    if shown.len() < lines.len() {
        shown.push(format!("+{} more", lines.len() - shown.len()));
    }
    shown.join("\n")
}

#[cfg(test)]
mod tests;
