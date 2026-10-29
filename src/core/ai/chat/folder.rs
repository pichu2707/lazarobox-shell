//! Which folders a chat may work in (chat-folder, design D2/D3). Pure: the
//! disk is read by the runtime, which hands the resolved path and the secret
//! roots of one tool call to [`judge`].

use std::path::{Path, PathBuf};

use super::{ChatState, Phase, store::StoreCmd};
use crate::core::ai::{
    sanitize::clean,
    tools::paths::{SecretRoots, normalize},
};
use crate::core::layout::PaneId;

/// A folder the runtime resolved and judged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Probed {
    /// The resolved absolute path: the one stored, shown and used.
    pub path: PathBuf,
    pub verdict: Verdict,
}

/// Why a folder is being probed, so the answer reaches the right flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    /// The user typed `/cd`.
    Cd,
    /// A conversation was resumed with a stored folder.
    Resume,
}

/// The argument of `/cd`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CdArg {
    /// `/cd` alone.
    Missing,
    /// The rest of the line, trimmed, unquoted.
    Path(String),
}

/// What a composer text is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// A normal message, sent as typed.
    Plain,
    /// `//cd …`: a message that starts with `/cd`, sent with one slash less.
    Escaped(String),
    /// The user-only `/cd` command.
    Cd(CdArg),
}

/// Reads a composer text. Only the exact token `/cd` is a command (the text
/// is `/cd` or `/cd` followed by whitespace); `//cd` escapes it.
pub fn parse_command(text: &str) -> Command {
    let text = text.trim();
    let (token, rest) = match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim()),
        None => (text, ""),
    };
    match token {
        "/cd" if rest.is_empty() => Command::Cd(CdArg::Missing),
        "/cd" => Command::Cd(CdArg::Path(
            rest.lines().next().unwrap_or("").trim().to_owned(),
        )),
        "//cd" => Command::Escaped(text[1..].to_owned()),
        _ => Command::Plain,
    }
}

/// The folder a typed path names, by text alone: `~` and `~/x` from `home`,
/// a relative path from `current` (`None` when there is no folder to read it
/// against), `.` and `..` folded.
pub fn expand(arg: &str, home: &Path, current: Option<&Path>) -> Option<PathBuf> {
    let path = if arg == "~" {
        home.to_path_buf()
    } else if let Some(rest) = arg.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(arg).is_absolute() {
        PathBuf::from(arg)
    } else {
        current?.join(arg)
    };
    Some(normalize(&path))
}

/// What a resolved, existing directory is for a chat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Inside HOME and allowed: the tools accept it as a root.
    Concrete,
    /// HOME itself: only an explicit choice.
    Home,
    /// A folder that holds HOME (`/home`): only an explicit choice.
    AboveHome,
    /// Outside HOME and not a secret root: the tools refuse it as a root.
    Outside,
}

/// Why a folder cannot be a chat folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    NotFound,
    NotAFolder,
    NotAllowed,
    Root,
}

impl Refusal {
    /// The short reason shown to the user.
    pub fn text(self) -> &'static str {
        match self {
            Self::NotFound => "folder not found",
            Self::NotAFolder => "not a folder",
            Self::NotAllowed => "not allowed",
            Self::Root => "/ is not a valid chat folder",
        }
    }
}

/// Judges an already resolved directory (no symlinks, no `..`) against the
/// roots the tools use. `/` is refused outright.
pub fn judge(real: &Path, roots: &SecretRoots) -> Result<Verdict, Refusal> {
    if real.parent().is_none() {
        return Err(Refusal::Root);
    }
    if real == roots.home() {
        return Ok(Verdict::Home);
    }
    if roots.home().starts_with(real) {
        return Ok(Verdict::AboveHome);
    }
    if real.starts_with(roots.home()) {
        return if roots.judge_real(real) {
            Ok(Verdict::Concrete)
        } else {
            Err(Refusal::NotAllowed)
        };
    }
    if roots.holds_secret(real) {
        return Err(Refusal::NotAllowed);
    }
    Ok(Verdict::Outside)
}

/// What a folder change left for the caller to do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FolderChange {
    pub old: Option<PathBuf>,
    /// The prompt was prepared and is dropped: prepare it again now.
    pub prompt_now: bool,
}

impl ChatState {
    /// Makes `new` (a probed, resolved folder) the chat's folder and
    /// invalidates what was pinned to the old one: the system prompt, and a
    /// tag proposed for a conversation nothing was saved for. Adds the
    /// visible line and the once-only notice for the model.
    pub fn set_folder(&mut self, new: PathBuf) -> FolderChange {
        let old = self.origin_cwd.replace(new.clone());
        let show = |path: Option<&Path>| {
            path.map_or_else(|| "unknown".to_owned(), |p| clean(&p.to_string_lossy()))
        };
        let (from, to) = (show(old.as_deref()), show(Some(&new)));
        let prompt_now = self.invalidate_prompt();
        self.reset_memory_for_folder();
        if !self.tags_edited && self.store.is_fresh() {
            self.tag = None;
        }
        self.push_system_line(format!("folder changed: {from} -> {to}"));
        self.tell(format!(
            "The chat's folder changed from {from} to {to}. Paths from earlier \
             messages may not exist; tool paths are now relative to {to}."
        ));
        FolderChange { old, prompt_now }
    }

    /// The stored conversation this chat saves to, once it exists.
    pub fn stored_conversation(&self) -> Option<i64> {
        self.store.conversation()
    }

    /// The command that keeps `folder` with the conversation, when it is
    /// stored already (a fresh one carries the folder in its `Create`).
    pub fn store_set_folder(&self, pane: PaneId, folder: PathBuf) -> Option<StoreCmd> {
        self.store.set_folder(pane, folder)
    }

    /// Why the folder cannot change now, in the order the user is told (design
    /// D4). `running` says a command typed for this chat runs in a pane: the
    /// app owns the run slots.
    pub fn folder_busy(&self, running: bool) -> Option<&'static str> {
        if self.approvals.pending().is_some() {
            Some("an approval is pending")
        } else if running {
            Some("a command is running")
        } else if self.phase != Phase::Idle || self.compaction.running() {
            Some("the model is answering")
        } else if self.batch_badge.is_some() || self.orphan.is_some() {
            Some("writes are staged or proposed")
        } else if !self.pending_save_ids().is_empty() {
            Some("a note is still being saved")
        } else if self.note_editor().is_some() || self.summary_draft().is_some() {
            Some("a note or summary is open")
        } else {
            None
        }
    }
}

/// Whether a verdict opens the picker: never a candidate row, never silently
/// chosen.
pub fn is_broad(verdict: Verdict) -> bool {
    !matches!(verdict, Verdict::Concrete)
}

/// The verdict of a folder the user typed (picker field or `/cd`): the tools
/// refuse an outside folder as a root, so a typed one is `not allowed`.
pub fn typed(result: Result<Verdict, Refusal>) -> Result<Verdict, Refusal> {
    match result {
        Ok(Verdict::Outside) => Err(Refusal::NotAllowed),
        other => other,
    }
}
