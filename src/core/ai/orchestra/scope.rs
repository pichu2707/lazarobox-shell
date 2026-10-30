//! The read scope of a run: root folders whose reads are pre-approved for
//! that run only. Both the requested and the real path must sit inside one
//! root, and the path deny-lists of `tools::paths` always win.

use std::path::{Path, PathBuf};

use super::RunId;
use super::run::{Run, RunState};
use crate::core::ai::jail;
use crate::core::ai::tools::paths::{PathVerdict, SecretRoots, display_target, resolve};
use crate::core::ai::tools::summary::clip;

/// Why a root is dropped: its path cannot be shown as it would be read.
pub const UNSHOWN: &str = "its path cannot be shown safely";
/// Why a root is dropped: the path policy refuses it (a secret or credential
/// folder, outside `$HOME`, missing).
pub const REFUSED_ROOT: &str = "refused by the path policy";
/// Why a root is dropped: `$HOME` itself or a broad scratch folder.
pub const TOO_BROAD: &str = "too broad: the home folder or a scratch folder";

/// A root the scope refused, for display: its name as given, sanitized and
/// capped (`clip`), and why.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dropped {
    pub root: String,
    pub reason: &'static str,
}

/// Decides whether a resolved root is a broad scratch folder.
pub type ScratchCheck<'a> = &'a dyn Fn(&Path) -> bool;

/// One approved root: as requested (folded by text) and as resolved.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Root {
    text: PathBuf,
    real: PathBuf,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReadScope {
    run: RunId,
    roots: Vec<Root>,
}

impl ReadScope {
    /// The scope of `run` over `inputs`, each given as typed (the chat origin
    /// by default, never model or worker text). A root is dropped, and
    /// returned sanitized with its reason, when it cannot be shown, does not
    /// pass the read policy, is `$HOME` itself or a broad scratch folder (by
    /// `is_scratch`, on its real path).
    pub fn new(
        run: RunId,
        secrets: &SecretRoots,
        origin: &Path,
        inputs: &[&str],
        is_scratch: ScratchCheck,
    ) -> (Self, Vec<Dropped>) {
        let mut roots = Vec::new();
        let mut dropped = Vec::new();
        for input in inputs {
            match root(secrets, origin, input, is_scratch) {
                Ok(root) => roots.push(root),
                Err(reason) => dropped.push(Dropped {
                    root: clip(input),
                    reason,
                }),
            }
        }
        (Self { run, roots }, dropped)
    }

    /// The roots kept, as requested (folded by text).
    pub fn roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|root| root.text.clone()).collect()
    }

    /// [`new`](Self::new) with the scratch folders of the jail.
    pub fn for_run(
        run: RunId,
        secrets: &SecretRoots,
        origin: &Path,
        inputs: &[&str],
    ) -> (Self, Vec<Dropped>) {
        Self::new(run, secrets, origin, inputs, &jail::is_scratch)
    }

    /// The scope that grants nothing: no tooling, or no root could be judged.
    pub fn denied(run: RunId) -> Self {
        Self {
            run,
            roots: Vec::new(),
        }
    }

    /// The real path of a read of `input` that needs no ask: `run` is the run
    /// of this scope and still going, and one root holds both the requested
    /// path and the real path, which the deny-lists must also allow. `None`
    /// means the read is out of scope and must be asked.
    ///
    /// The caller MUST open exactly the returned path, with no-follow
    /// semantics (`open_beneath` / `O_NOFOLLOW`), and re-judge the real path
    /// of the opened descriptor; resolving `input` again reopens a race.
    pub fn allows(
        &self,
        run: &Run,
        secrets: &SecretRoots,
        origin: &Path,
        input: &str,
    ) -> Option<PathBuf> {
        let live = matches!(run.state(), RunState::Open | RunState::Halted { .. });
        if run.id() != self.run || !live {
            return None;
        }
        let (Some(text), PathVerdict::Allowed(real)) = (
            display_target(secrets.home(), origin, input),
            resolve(secrets, origin, input),
        ) else {
            return None;
        };
        let text = Path::new(&text);
        let inside = self
            .roots
            .iter()
            .any(|root| text.starts_with(&root.text) && real.starts_with(&root.real));
        inside.then_some(real)
    }
}

fn root(
    secrets: &SecretRoots,
    origin: &Path,
    input: &str,
    is_scratch: ScratchCheck,
) -> Result<Root, &'static str> {
    let text = PathBuf::from(display_target(secrets.home(), origin, input).ok_or(UNSHOWN)?);
    let PathVerdict::Allowed(real) = resolve(secrets, origin, input) else {
        return Err(REFUSED_ROOT);
    };
    let broad = real == secrets.home() || is_scratch(&real) || is_scratch(&text);
    if broad {
        return Err(TOO_BROAD);
    }
    Ok(Root { text, real })
}
