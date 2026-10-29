//! The folder probe (chat-folder, design D2): resolves a path on disk and
//! judges it with the same secret roots a tool call builds, so the chat never
//! accepts a folder the tools would refuse.

use std::path::Path;

use crate::{
    core::ai::chat::folder::{Probed, Refusal, judge},
    runtime::tools::{ToolEnv, fs::load_roots},
};

/// Resolves `path` (symlinks followed) and judges it. A missing path, a file
/// and a refused folder each have their reason; with HOME unusable everything
/// is `not allowed`, as for the tools.
pub fn probe(env: &ToolEnv, path: &Path) -> Result<Probed, Refusal> {
    // A relative path would read against the process cwd, not the chat.
    if !path.is_absolute() {
        return Err(Refusal::NotFound);
    }
    let real = std::fs::canonicalize(path).map_err(|_| Refusal::NotFound)?;
    if !real.is_dir() {
        return Err(Refusal::NotAFolder);
    }
    // `/` is refused whatever HOME says.
    if real.parent().is_none() {
        return Err(Refusal::Root);
    }
    let roots = load_roots(env).map_err(|_| Refusal::NotAllowed)?;
    let verdict = judge(&real, &roots)?;
    Ok(Probed {
        path: real,
        verdict,
    })
}

#[cfg(test)]
mod tests;
