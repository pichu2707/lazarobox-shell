//! The read scope of a run: root folders whose reads are pre-approved for
//! that run only. Both the requested and the real path must sit inside one
//! root, and the path deny-lists of `tools::paths` always win.

use std::path::{Path, PathBuf};

use super::RunId;
use super::run::{Run, RunState};
use crate::core::ai::tools::paths::{PathVerdict, SecretRoots, display_target, resolve};

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
    /// The scope of `run` over the roots the user proposed, each given as the
    /// model or the user would type it. A root is dropped, and returned by
    /// name, when it does not pass the read policy, is `$HOME` itself or a
    /// broad scratch folder (by `is_scratch`, on its real path).
    pub fn new(
        run: RunId,
        secrets: &SecretRoots,
        origin: &Path,
        inputs: &[&str],
        is_scratch: ScratchCheck,
    ) -> (Self, Vec<String>) {
        let mut roots = Vec::new();
        let mut dropped = Vec::new();
        for input in inputs {
            match root(secrets, origin, input, is_scratch) {
                Some(root) => roots.push(root),
                None => dropped.push((*input).to_owned()),
            }
        }
        (Self { run, roots }, dropped)
    }

    /// The scope after the user denies: it grants nothing.
    pub fn denied(run: RunId) -> Self {
        Self {
            run,
            roots: Vec::new(),
        }
    }

    /// Whether a read of `input` needs no ask: `run` is the run of this scope
    /// and still going, and one root holds both the requested path and the
    /// real path, which the deny-lists must also allow.
    pub fn allows(&self, run: &Run, secrets: &SecretRoots, origin: &Path, input: &str) -> bool {
        let live = matches!(run.state(), RunState::Open | RunState::Halted { .. });
        if run.id() != self.run || !live {
            return false;
        }
        let (Some(text), PathVerdict::Allowed(real)) = (
            display_target(secrets.home(), origin, input),
            resolve(secrets, origin, input),
        ) else {
            return false;
        };
        let text = Path::new(&text);
        self.roots
            .iter()
            .any(|root| text.starts_with(&root.text) && real.starts_with(&root.real))
    }
}

fn root(
    secrets: &SecretRoots,
    origin: &Path,
    input: &str,
    is_scratch: ScratchCheck,
) -> Option<Root> {
    let text = PathBuf::from(display_target(secrets.home(), origin, input)?);
    let PathVerdict::Allowed(real) = resolve(secrets, origin, input) else {
        return None;
    };
    let broad = real == secrets.home() || is_scratch(&real) || is_scratch(&text);
    (!broad).then_some(Root { text, real })
}
