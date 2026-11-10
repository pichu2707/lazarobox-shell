//! Deleting a llama.cpp model file from the disk (model-delete). The core
//! only asks; everything is checked here, on the measure and again on the
//! delete, and once more right before each removal:
//!
//! - the path is absolute and plain (no `..`), and its folder, with links
//!   followed, lies inside one of the scanned model folders, also resolved;
//! - each file is a regular `.gguf` the scan lists, with every part of a
//!   split model, or, in the Hugging Face cache, a link to a blob of that
//!   same repository's `blobs/`;
//! - no file the config uses goes, however the config spells it.
//!
//! Only files are removed, one by one, and then folders left empty. There
//! is no recursive delete. `.incomplete` and `mmproj` files are never
//! touched. A blob another snapshot still links to stays.

use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::core::ai::gguf::{is_model_file, label, split_part};
use crate::core::ai::hf::size_label;

/// How deep and how many entries of a repository's snapshots are read to
/// find the other links to a blob, as the model scan reads.
const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 20_000;

/// The file system's identity of a file, to see it did not change.
type FileId = (u64, u64);

fn id_of(meta: &Metadata) -> FileId {
    (meta.dev(), meta.ino())
}

/// One file of a model, as the scan lists it.
struct Part {
    path: PathBuf,
    id: FileId,
    /// What it holds: its own length, or its blob's.
    size: u64,
    /// The blob a Hugging Face link points to.
    blob: Option<Blob>,
}

struct Blob {
    path: PathBuf,
    id: FileId,
    /// No other link points to it: it goes with the link.
    goes: bool,
}

/// What a delete would remove, checked.
pub struct Plan {
    parts: Vec<Part>,
    /// The repository folder of a file in the Hugging Face layout.
    repo: Option<PathBuf>,
}

impl Plan {
    /// The sum of the parts, one `stat` each.
    pub fn size(&self) -> u64 {
        self.parts.iter().map(|p| p.size).sum()
    }

    pub fn parts(&self) -> usize {
        self.parts.len()
    }
}

/// Checks `target`, a model file the scan listed in `dirs`, and every part of
/// it; refuses one that any of `in_use` names. Removes nothing.
pub fn plan(target: &Path, dirs: &[PathBuf], in_use: &[PathBuf]) -> Result<Plan, String> {
    let plain = target
        .components()
        .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !target.is_absolute() || !plain {
        return Err("not a plain absolute path".to_owned());
    }
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !is_model_file(name) {
        return Err("not a model file the list shows".to_owned());
    }
    let folder = target.parent().unwrap_or(target);
    let folder = fs::canonicalize(folder).map_err(|e| format!("cannot read its folder: {e}"))?;
    let root = dirs
        .iter()
        .filter_map(|d| fs::canonicalize(d).ok())
        .filter(|d| folder.starts_with(d))
        .max_by_key(|d| d.components().count())
        .ok_or("it is outside the model folders")?;
    let repo = repo_of(&folder, &root);
    let mut parts = Vec::new();
    for (n, name) in part_names(name).iter().enumerate() {
        let path = folder.join(name);
        match fs::symlink_metadata(&path) {
            // A part never downloaded: nothing of it to remove.
            Err(e) if n > 0 && e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot read {name}: {e}")),
            Ok(meta) => parts.push(part(path, &meta, repo.as_deref())?),
        }
    }
    if let Some(repo) = &repo {
        mark_shared(&mut parts, repo)?;
    }
    let used: Vec<PathBuf> = in_use
        .iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .collect();
    for part in &parts {
        let real = part.blob.as_ref().map_or(&part.path, |b| &b.path);
        if used.contains(real) {
            return Err(format!("{} is in use by the config", label(&part.path)));
        }
    }
    Ok(Plan { parts, repo })
}

/// The repository folder of `folder` when it lies in the Hugging Face layout
/// (`models--<org>--<repo>/snapshots/<commit>/…`) under `root`.
fn repo_of(folder: &Path, root: &Path) -> Option<PathBuf> {
    let rel: Vec<Component> = folder.strip_prefix(root).ok()?.components().collect();
    let at = rel.windows(3).position(|w| {
        w[0].as_os_str()
            .to_str()
            .is_some_and(|n| n.starts_with("models--"))
            && w[1].as_os_str() == "snapshots"
    })?;
    Some(rel[..=at].iter().fold(root.to_path_buf(), |p, c| p.join(c)))
}

/// The file names of every part of the model file `name`: itself, or all
/// `-NNNNN-of-NNNNN` parts of a split model.
fn part_names(name: &str) -> Vec<String> {
    // `name` ends in `.gguf` in any case: ASCII, so the byte split is safe.
    let (stem, ext) = name.split_at(name.len() - ".gguf".len());
    let lower = stem.to_ascii_lowercase();
    let (Some(1), Some(of)) = (split_part(&lower), lower.rfind("-of-")) else {
        return vec![name.to_owned()];
    };
    let total: u32 = lower[of + 4..].parse().unwrap_or(1);
    // `-00001` sits right before `-of-`.
    let (prefix, tail) = (&stem[..of - 6], &stem[of..]);
    (1..=total)
        .map(|k| format!("{prefix}-{k:05}{tail}{ext}"))
        .collect()
}

fn part(path: PathBuf, meta: &Metadata, repo: Option<&Path>) -> Result<Part, String> {
    let name = label(&path);
    if meta.file_type().is_file() {
        return Ok(Part {
            id: id_of(meta),
            size: meta.len(),
            blob: None,
            path,
        });
    }
    if !meta.file_type().is_symlink() {
        return Err(format!("{name} is not a regular file"));
    }
    let Some(repo) = repo else {
        return Err(format!("{name} is a link outside a Hugging Face cache"));
    };
    let blob = fs::canonicalize(&path).map_err(|e| format!("cannot follow {name}: {e}"))?;
    let blob_meta = fs::symlink_metadata(&blob).map_err(|e| format!("cannot read {name}: {e}"))?;
    let in_blobs = blob.parent() == Some(repo.join("blobs").as_path());
    let incomplete = blob.extension().is_some_and(|e| e == "incomplete");
    if !in_blobs || incomplete || !blob_meta.file_type().is_file() {
        return Err(format!("{name} links outside its repository's blobs"));
    }
    Ok(Part {
        id: id_of(meta),
        size: blob_meta.len(),
        blob: Some(Blob {
            path: blob,
            id: id_of(&blob_meta),
            goes: true,
        }),
        path,
    })
}

/// Keeps every blob another link of `repo`'s snapshots still points to, and
/// a blob two parts share is removed once.
fn mark_shared(parts: &mut [Part], repo: &Path) -> Result<(), String> {
    let mut links = Vec::new();
    let mut budget = MAX_ENTRIES;
    // A snapshot that cannot be read may still link to the blob.
    links_in(&repo.join("snapshots"), 0, &mut budget, &mut links)
        .map_err(|e| format!("cannot check the other snapshots of its repository: {e}"))?;
    if budget == 0 {
        return Err("too many files in its repository to check".to_owned());
    }
    let ours: Vec<PathBuf> = parts.iter().map(|p| p.path.clone()).collect();
    let others: Vec<PathBuf> = links
        .into_iter()
        .filter(|l| !ours.contains(l))
        .filter_map(|l| fs::canonicalize(l).ok())
        .collect();
    let mut seen: Vec<PathBuf> = Vec::new();
    for part in parts.iter_mut() {
        if let Some(blob) = &mut part.blob {
            blob.goes = !others.contains(&blob.path) && !seen.contains(&blob.path);
            seen.push(blob.path.clone());
        }
    }
    Ok(())
}

/// The links under `dir`, without following links to folders. A folder or
/// an entry that cannot be read, or a folder deeper than the scan reads, is
/// an error.
fn links_in(
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    found: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        if *budget == 0 {
            return Ok(());
        }
        *budget -= 1;
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            // A folder the scan does not read may hold a link to the blob.
            if depth >= MAX_DEPTH {
                return Err(io::Error::other("its folders are nested too deep"));
            }
            links_in(&entry.path(), depth + 1, budget, found)?;
        } else if kind.is_symlink() {
            found.push(entry.path());
        }
    }
    Ok(())
}

/// Checks `target` again and deletes it with all its parts: the bytes freed,
/// or why it failed and, part way, what went and what did not. A failure
/// removes nothing more.
pub fn delete(target: &Path, dirs: &[PathBuf], in_use: &[PathBuf]) -> Result<u64, String> {
    let plan = plan(target, dirs, in_use)?;
    let mut done: Vec<String> = Vec::new();
    let mut freed = 0;
    for (n, part) in plan.parts.iter().enumerate() {
        let name = label(&part.path);
        let link_to = part.blob.as_ref().map(|b| b.path.as_path());
        if let Err(why) = remove(&part.path, part.id, link_to) {
            let left: Vec<String> = plan.parts[n..].iter().map(|p| label(&p.path)).collect();
            return Err(partial(&done, freed, &left, &name, &why));
        }
        match &part.blob {
            Some(blob) if blob.goes => {
                if let Err(why) = remove(&blob.path, blob.id, None) {
                    done.push(name.clone());
                    let mut left = vec![format!("the blob of {name}")];
                    left.extend(plan.parts[n + 1..].iter().map(|p| label(&p.path)));
                    return Err(partial(&done, freed, &left, &name, &why));
                }
                freed += part.size;
            }
            // Another snapshot still reads the blob: the link frees nothing.
            Some(_) => {}
            None => freed += part.size,
        }
        done.push(name);
    }
    if let Some(repo) = &plan.repo {
        tidy(&plan.parts, repo);
    }
    Ok(freed)
}

/// Removes the file or link at `path` if it is still the one checked: same
/// identity, same kind, its folder unchanged, and a link still pointing to
/// `link_to`.
fn remove(path: &Path, id: FileId, link_to: Option<&Path>) -> Result<(), String> {
    let changed = || format!("{} changed since it was checked", label(path));
    let folder = path.parent().ok_or_else(changed)?;
    if fs::canonicalize(folder).ok().as_deref() != Some(folder) {
        return Err(changed());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let kind = meta.file_type();
    let same_kind = match link_to {
        Some(blob) => kind.is_symlink() && fs::canonicalize(path).ok().as_deref() == Some(blob),
        None => kind.is_file(),
    };
    if id_of(&meta) != id || !same_kind {
        return Err(changed());
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}

/// The reason of a failure on `name`, with what went before it.
fn partial(done: &[String], freed: u64, left: &[String], name: &str, why: &str) -> String {
    if done.is_empty() {
        return format!("cannot remove {name}: {why}");
    }
    format!(
        "removed {} (freed {}); not removed {}: {why}",
        done.join(", "),
        size_label(freed),
        left.join(", ")
    )
}

/// Removes the folders of `repo` the delete left empty, from each part's
/// folder up to `snapshots/`. With no snapshot and no blob left, the
/// repository goes with its refs and its empty `.no_exist` markers, but only
/// when nothing else is in it. Best effort: a folder that cannot go stays.
fn tidy(parts: &[Part], repo: &Path) {
    let snapshots = repo.join("snapshots");
    for part in parts {
        let mut dir = part.path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(&snapshots) && *d != snapshots.as_path()) {
            if fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    if !gone(&snapshots) || !gone(&repo.join("blobs")) {
        return;
    }
    let (refs, markers) = (repo.join("refs"), repo.join(".no_exist"));
    let only_metadata = entries(repo).iter().all(|e| *e == refs || *e == markers);
    let commits = entries(&markers);
    let empty =
        |file: &PathBuf| fs::symlink_metadata(file).is_ok_and(|m| m.is_file() && m.len() == 0);
    // Every folder is read only when it is a real folder, not a link: a link
    // anywhere here keeps the whole repository.
    let prunable = only_metadata
        && own_dir(&refs)
        && own_dir(&markers)
        && entries(&refs).iter().all(|f| is_ref(f))
        && commits
            .iter()
            .all(|c| own_dir(c) && entries(c).iter().all(empty));
    if !prunable {
        return;
    }
    for file in entries(&refs) {
        let _ = fs::remove_file(file);
    }
    for commit in &commits {
        for file in entries(commit) {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_dir(commit);
    }
    if gone(&refs) && gone(&markers) {
        let _ = fs::remove_dir(repo);
    }
}

/// Removes the folder `dir` if it is empty: whether it is gone now.
fn gone(dir: &Path) -> bool {
    match fs::remove_dir(dir) {
        Ok(()) => true,
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}

/// Whether `dir` is missing or a folder itself, not a link to one.
fn own_dir(dir: &Path) -> bool {
    match fs::symlink_metadata(dir) {
        Ok(meta) => meta.file_type().is_dir(),
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Whether `path` is a ref of the cache: a small regular file that holds a
/// commit id.
fn is_ref(path: &Path) -> bool {
    let small = fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() <= 64);
    small
        && fs::read_to_string(path).is_ok_and(|text| {
            let id = text.trim();
            id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests;
