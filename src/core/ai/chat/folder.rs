//! Which folders a chat may work in (chat-folder, design D2/D3). Pure: the
//! disk is read by the runtime, which hands the resolved path and the secret
//! roots of one tool call to [`judge`].

use std::path::Path;

use crate::core::ai::tools::paths::SecretRoots;

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
