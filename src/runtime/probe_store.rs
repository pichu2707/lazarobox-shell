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
};

use crate::core::probe::{Outcome, parse, render};
use crate::runtime::{bounded_read, engram::serve::state_dir};

/// The file inside the state folder.
pub const FILE: &str = "terminals.json";

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
        Err(bounded_read::ReadError::NotRegular) => return Err(why("is not a regular file")),
        Err(bounded_read::ReadError::TooLarge) => return Err(why("is too large")),
        Err(bounded_read::ReadError::Unreadable) => return Err(why("cannot be read")),
    };
    let text = String::from_utf8(bytes).map_err(|_| why("is not valid UTF-8"))?;
    parse(&text).map_err(|error| why(&error.to_string()))
}

fn why(reason: &str) -> String {
    format!("{FILE}: {reason}")
}

fn no_dir() -> String {
    why("no state folder (HOME is not set)")
}

/// Stores `outcome` for `key`, keeping the other terminals' rows. The file is
/// written to a temporary name and renamed over the old one.
pub fn save(dir: Option<&Path>, key: &str, outcome: Outcome) -> Result<(), String> {
    // Read first: a file that cannot be read is left alone, not replaced.
    let mut results = load(dir)?;
    let dir = dir.ok_or_else(no_dir)?;
    results.insert(key.to_owned(), outcome);
    let failed = |error: io::Error| why(&format!("cannot be written ({error})"));
    fs::create_dir_all(dir).map_err(failed)?;
    let temp = dir.join(format!("{FILE}.{}.tmp", std::process::id()));
    let written = fs::File::create(&temp)
        .and_then(|mut file| file.write_all(render(&results).as_bytes()))
        .and_then(|()| fs::rename(&temp, dir.join(FILE)));
    if let Err(error) = written {
        // Best effort: the temporary name is ours alone.
        let _ = fs::remove_file(&temp);
        return Err(failed(error));
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
            assert!(err.starts_with(FILE), "{err}");
            assert!(save(Some(dir.path()), "kitty", Outcome::Pass).is_err());
            assert_eq!(fs::read_to_string(dir.path().join(FILE)).unwrap(), bad);
        }
    }

    #[test]
    fn io_errors_are_reasons_not_panics() {
        // No state folder at all.
        assert!(load(None).unwrap_err().contains("HOME"));
        assert!(save(None, "k", Outcome::Pass).unwrap_err().contains("HOME"));
        // The file is a folder.
        let dir = tmp();
        fs::create_dir(dir.path().join(FILE)).unwrap();
        assert!(load(Some(dir.path())).unwrap_err().starts_with(FILE));
        // The state folder is a file: it cannot be created or written.
        let blocked = tmp();
        let file = blocked.path().join("lazarobox");
        fs::write(&file, "x").unwrap();
        let err = save(Some(&file), "k", Outcome::Pass).unwrap_err();
        assert!(err.starts_with(FILE), "{err}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "x");
    }

    #[test]
    fn an_oversize_file_is_refused() {
        let dir = tmp();
        fs::write(dir.path().join(FILE), vec![b' '; CAP as usize + 1]).unwrap();
        assert!(load(Some(dir.path())).unwrap_err().contains("large"));
    }
}
