//! Writes the menu-owned keys back to `config.toml`.

use std::{
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write},
    path::Path,
    process,
};

use crate::core::{
    config::ConfigEdit,
    config_edit::{EditError, apply_edit},
};

/// Applies `edits` to the config file at `path`. The error is the one-line
/// footer text.
///
/// The file is read at save time, edited in place (comments and unknown keys
/// survive) and replaced atomically. A symlinked file is written through, so
/// dotfile managers keep their link.
pub(super) fn save_config(path: Option<&Path>, edits: &[ConfigEdit]) -> Result<(), String> {
    let failed = |error: io::Error| format!("save failed: {error}");
    let path = path.ok_or("save failed: no config path (HOME is not set)")?;
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let existing = match fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(failed(error)),
    };
    let content = apply_edit(existing.as_deref(), edits).map_err(|error| match error {
        EditError::Unparseable => "config.toml has errors; fix it before saving".to_owned(),
        EditError::NotATable(table) => {
            format!("config.toml: {table} is not a table; fix it before saving")
        }
        EditError::KeyIsTable { table, key } => {
            format!("config.toml: {table}.{key} is a table; fix it before saving")
        }
    })?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    write_atomic(&target, &content).map_err(failed)
}

/// Writes `content` to `target` through a temp file in the same directory,
/// so the rename is atomic. The temp file is removed on any failure after it
/// exists, and an existing file's permissions carry over.
fn write_atomic(target: &Path, content: &str) -> io::Result<()> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = target.with_file_name(format!(".{name}.{}.tmp", process::id()));
    let result = write_temp(&temp, target, content).and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_temp(temp: &Path, target: &Path, content: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    if let Ok(existing) = fs::metadata(target) {
        file.set_permissions(existing.permissions())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
    };

    use super::*;

    // Every test works inside its own `tempfile::tempdir`; none reads the
    // real HOME or XDG_CONFIG_HOME.
    const STATUSLINE_TOP: ConfigEdit = ConfigEdit {
        table: "statusline",
        key: "position",
        value: "top",
    };

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn config_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("config.toml")
    }

    // Spec: Missing file and dirs.
    #[test]
    fn a_missing_file_and_its_directories_are_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/lazarobox/config.toml");
        assert_eq!(save_config(Some(&path), &[STATUSLINE_TOP]), Ok(()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
    }

    // Spec: Atomic replace.
    #[test]
    fn a_save_replaces_the_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "# mine\n[statusline]\nposition = \"bottom\" # why\n").unwrap();
        assert_eq!(save_config(Some(&path), &[STATUSLINE_TOP]), Ok(()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# mine\n[statusline]\nposition = \"top\" # why\n"
        );
        assert_eq!(names(dir.path()), ["config.toml"]);
    }

    // Spec: Failure keeps the original.
    #[test]
    fn a_read_only_directory_keeps_the_original_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "[statusline]\nposition = \"bottom\"\n").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let writable = fs::File::create(dir.path().join("probe")).is_ok();
        let result = save_config(Some(&path), &[STATUSLINE_TOP]);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        if writable {
            eprintln!("skipped: running as root, directory permissions do not apply");
            return;
        }
        let error = result.unwrap_err();
        assert!(error.starts_with("save failed: "), "got {error:?}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[statusline]\nposition = \"bottom\"\n"
        );
        assert_eq!(names(dir.path()), ["config.toml"]);
    }

    #[test]
    fn a_failed_rename_removes_the_temp() {
        let dir = tempfile::tempdir().unwrap();
        let target = config_in(&dir);
        fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, "x = 1\n").is_err());
        assert_eq!(names(dir.path()), ["config.toml"]);
        assert!(target.is_dir());
    }

    // Spec: No config path.
    #[test]
    fn no_config_path_fails_and_writes_nothing() {
        assert_eq!(
            save_config(None, &[STATUSLINE_TOP]),
            Err("save failed: no config path (HOME is not set)".to_owned())
        );
    }

    // Spec: Unparseable file refused; Non-table collision refused; Owned key
    // holding a table refused.
    #[test]
    fn refused_files_map_to_their_message_and_stay_byte_identical() {
        let cases = [
            (
                "statusline = [\n",
                "config.toml has errors; fix it before saving",
            ),
            (
                "statusline = 3\n",
                "config.toml: statusline is not a table; fix it before saving",
            ),
            (
                "[statusline.position]\nx = 1\n",
                "config.toml: statusline.position is a table; fix it before saving",
            ),
        ];
        for (text, message) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = config_in(&dir);
            fs::write(&path, text).unwrap();
            assert_eq!(
                save_config(Some(&path), &[STATUSLINE_TOP]),
                Err(message.to_owned()),
                "for {text:?}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
            assert_eq!(names(dir.path()), ["config.toml"]);
        }
    }

    #[test]
    fn other_read_errors_say_save_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::create_dir(&path).unwrap();
        let cause = fs::read_to_string(&path).unwrap_err();
        assert_eq!(
            save_config(Some(&path), &[STATUSLINE_TOP]),
            Err(format!("save failed: {cause}"))
        );
    }

    // ADR CM-12: a symlinked config is written through.
    #[test]
    fn a_symlinked_config_is_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("dotfiles");
        let link_dir = dir.path().join("home");
        fs::create_dir(&real_dir).unwrap();
        fs::create_dir(&link_dir).unwrap();
        let real = real_dir.join("config.toml");
        let link = link_dir.join("config.toml");
        fs::write(&real, "[statusline]\nposition = \"bottom\"\n").unwrap();
        symlink(&real, &link).unwrap();
        assert_eq!(save_config(Some(&link), &[STATUSLINE_TOP]), Ok(()));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), real);
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
        assert_eq!(names(&real_dir), ["config.toml"]);
        assert_eq!(names(&link_dir), ["config.toml"]);
    }

    // ADR CM-12: the existing mode survives the replace.
    #[test]
    fn the_existing_mode_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "[statusline]\nposition = \"bottom\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(save_config(Some(&path), &[STATUSLINE_TOP]), Ok(()));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}
