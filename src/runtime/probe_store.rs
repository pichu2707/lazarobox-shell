//! Keeps the prefix probe results in `$XDG_STATE_HOME/lazarobox/terminals.json`
//! (see [`crate::core::probe`] for the format). State, not config. Every
//! error is a one-line reason for a notice; a file that cannot be read is
//! never overwritten.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use crate::core::probe::{Outcome, parse, render};
use crate::runtime::{bounded_read, engram::serve::state_dir};

/// The file inside the state folder.
pub const FILE: &str = "terminals.json";

/// Makes each save's temporary name its own.
static SAVES: AtomicU32 = AtomicU32::new(0);

/// The most bytes read: a file of a few terminals is far below it.
const CAP: u64 = 64 * 1024;

/// The state folder for this process: `$XDG_STATE_HOME/lazarobox`, else
/// `$HOME/.local/state/lazarobox`.
pub fn dir_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    state_dir(xdg, home)
}

/// Every stored result. A missing file is an empty map (probe once).
pub fn load(dir: Option<&Path>) -> Result<BTreeMap<String, Outcome>, String> {
    let path = dir.ok_or_else(no_dir)?.join(FILE);
    let bytes = match bounded_read::read_regular(&path, CAP) {
        Ok(bytes) => bytes,
        Err(bounded_read::ReadError::Missing) => return Ok(BTreeMap::new()),
        Err(bounded_read::ReadError::NotRegular) => {
            return Err(why(&path, "is not a regular file"));
        }
        Err(bounded_read::ReadError::TooLarge) => return Err(why(&path, "is too large")),
        Err(bounded_read::ReadError::Unreadable) => return Err(why(&path, "cannot be read")),
    };
    let text = String::from_utf8(bytes).map_err(|_| why(&path, "is not valid UTF-8"))?;
    parse(&text).map_err(|error| why(&path, &error.to_string()))
}

/// A reason that names the file, so the user can find and delete it.
fn why(path: &Path, reason: &str) -> String {
    format!("{}: {reason}", path.display())
}

fn no_dir() -> String {
    format!("{FILE}: no usable state folder")
}

/// Stores `outcome` for `key`, keeping the other terminals' rows. The file is
/// written to a temporary name and renamed over the old one.
pub fn save(dir: Option<&Path>, key: &str, outcome: Outcome) -> Result<(), String> {
    save_with(dir, key, outcome, || {})
}

/// [`save`] with a hook that runs after the temporary file exists and before
/// the rows are read, so a test can play another lazarobox writing in that gap.
fn save_with(
    dir: Option<&Path>,
    key: &str,
    outcome: Outcome,
    between: impl FnOnce(),
) -> Result<(), String> {
    let dir = dir.ok_or_else(no_dir)?;
    let failed = |error: io::Error| why(&dir.join(FILE), &format!("cannot be written: {error}"));
    fs::create_dir_all(dir).map_err(failed)?;
    let temp = dir.join(format!(
        "{FILE}.{}.{}.tmp",
        std::process::id(),
        SAVES.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::File::create(&temp).map_err(failed)?;
    between();
    // The rows are read as late as possible, right before the rename: another
    // lazarobox saving at the same moment can still lose its row. The race is
    // accepted (one tiny write per terminal, ever; the loser probes again).
    // A file that cannot be read is left alone, not replaced.
    let written = load(Some(dir))
        .map_err(|reason| (None, reason))
        .and_then(|mut results| {
            results.insert(key.to_owned(), outcome);
            file.write_all(render(&results).as_bytes())
                .and_then(|()| fs::rename(&temp, dir.join(FILE)))
                .map_err(|error| (Some(error), String::new()))
        });
    if let Err((error, reason)) = written {
        // Best effort: the temporary name is ours alone.
        let _ = fs::remove_file(&temp);
        return Err(error.map_or(reason, failed));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn the_state_dir_follows_xdg_then_home() {
        assert_eq!(
            dir_from(Some("/x".into()), Some("/h".into())),
            Some(PathBuf::from("/x/lazarobox"))
        );
        assert_eq!(
            dir_from(None, Some("/h".into())),
            Some(PathBuf::from("/h/.local/state/lazarobox"))
        );
        assert_eq!(dir_from(None, None), None);
    }

    #[test]
    fn a_missing_file_means_probe_once() {
        let dir = tmp();
        assert_eq!(load(Some(dir.path())), Ok(BTreeMap::new()));
        assert_eq!(load(Some(&dir.path().join("nope"))), Ok(BTreeMap::new()));
    }

    #[test]
    fn a_saved_result_is_read_back_and_keeps_the_other_rows() {
        let dir = tmp();
        let state = dir.path().join("deep/lazarobox");
        save(Some(&state), "kitty", Outcome::Pass).unwrap();
        save(Some(&state), "xterm", Outcome::Missed).unwrap();
        save(Some(&state), "kitty", Outcome::Alias).unwrap();
        let got = load(Some(&state)).unwrap();
        // Another terminal has no row: its probe runs.
        assert_eq!(got.get("alacritty"), None);
        assert_eq!(
            got,
            BTreeMap::from([
                ("kitty".to_owned(), Outcome::Alias),
                ("xterm".to_owned(), Outcome::Missed),
            ])
        );
    }

    #[test]
    fn a_write_is_temp_plus_rename_and_leaves_no_temp_file() {
        let dir = tmp();
        save(Some(dir.path()), "kitty", Outcome::Pass).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [FILE]);
    }

    #[test]
    fn a_bad_file_is_an_error_and_is_never_overwritten() {
        for bad in ["", "{nope", "[]", r#"{"kitty":"maybe"}"#] {
            let dir = tmp();
            fs::write(dir.path().join(FILE), bad).unwrap();
            let err = load(Some(dir.path())).unwrap_err();
            assert!(err.contains(FILE), "{err}");
            assert!(save(Some(dir.path()), "kitty", Outcome::Pass).is_err());
            assert_eq!(fs::read_to_string(dir.path().join(FILE)).unwrap(), bad);
        }
    }

    #[test]
    fn io_errors_are_reasons_not_panics() {
        // No state folder at all.
        assert!(load(None).unwrap_err().contains("no usable state folder"));
        assert!(
            save(None, "k", Outcome::Pass)
                .unwrap_err()
                .contains("no usable state folder")
        );
        // The file is a folder.
        let dir = tmp();
        fs::create_dir(dir.path().join(FILE)).unwrap();
        let err = load(Some(dir.path())).unwrap_err();
        // The message names the whole path, so the user can delete the file.
        assert!(
            err.starts_with(dir.path().join(FILE).to_str().unwrap()),
            "{err}"
        );
        // The state folder is a file: it cannot be created or written.
        let blocked = tmp();
        let file = blocked.path().join("lazarobox");
        fs::write(&file, "x").unwrap();
        let err = save(Some(&file), "k", Outcome::Pass).unwrap_err();
        assert!(err.contains(FILE), "{err}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "x");
    }

    // The rows of another lazarobox written while this one saves are kept.
    #[test]
    fn a_row_written_during_the_save_is_not_lost() {
        let dir = tmp();
        let other = dir.path().to_owned();
        save_with(Some(dir.path()), "kitty", Outcome::Pass, || {
            save(Some(&other), "xterm", Outcome::Alias).unwrap();
        })
        .unwrap();
        let got = load(Some(dir.path())).unwrap();
        assert_eq!(got.get("kitty"), Some(&Outcome::Pass));
        assert_eq!(got.get("xterm"), Some(&Outcome::Alias));
    }

    #[test]
    fn an_oversize_file_is_refused() {
        let dir = tmp();
        fs::write(dir.path().join(FILE), vec![b' '; CAP as usize + 1]).unwrap();
        assert!(load(Some(dir.path())).unwrap_err().contains("large"));
    }
}
