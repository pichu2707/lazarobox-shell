//! Finds the llama.cpp model files in the folders [`model_dirs`] names.
//!
//! [`model_dirs`]: crate::core::ai::gguf::model_dirs

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::ai::gguf::{is_model_file, order};

/// How many folders below a model folder are read: enough for the Hugging
/// Face cache (`models--org--repo/snapshots/<rev>/` plus a subfolder).
const MAX_DEPTH: usize = 6;

/// How many entries one search reads at most, so a huge folder (a cache
/// shared with other tools) cannot stall it.
const MAX_ENTRIES: usize = 20_000;

/// The model files under `dirs`, each once, sorted by file name. A folder
/// that is missing or unreadable is skipped. Links to files are followed (the
/// Hugging Face cache links each snapshot file into `blobs/`) and listed by
/// the link's path; links to folders are not, so a loop cannot hang it.
pub fn find(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut budget = MAX_ENTRIES;
    for dir in dirs {
        walk(dir, 0, &mut budget, &mut found);
    }
    order(found)
}

fn walk(dir: &Path, depth: usize, budget: &mut usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, budget, found);
            }
            continue;
        }
        let is_file = kind.is_file() || (kind.is_symlink() && path.is_file());
        if is_file && is_model_file(&entry.file_name().to_string_lossy()) {
            found.push(path);
        }
    }
}

#[cfg(test)]
mod tests;
