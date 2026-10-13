//! Writes the menu-owned keys back to `config.toml`.

use std::{
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process,
};

use crate::core::{
    config::ConfigEdit,
    config_edit::{EditError, apply_edit},
};

/// Most symlinks followed when the target does not exist yet.
const MAX_LINK_HOPS: usize = 8;

/// The file a save really writes. An existing file resolves through
/// `canonicalize`. A dangling symlink is followed by hand so the save creates
/// its target instead of replacing the link.
fn resolve_target(path: &Path) -> Result<PathBuf, String> {
    if let Ok(real) = fs::canonicalize(path) {
        return Ok(real);
    }
    let mut current = path.to_owned();
    // One extra pass checks the file reached after the last allowed hop.
    for _ in 0..=MAX_LINK_HOPS {
        if !fs::symlink_metadata(&current).is_ok_and(|meta| meta.is_symlink()) {
            return Ok(current);
        }
        let link = fs::read_link(&current).map_err(|error| format!("save failed: {error}"))?;
        // `join` keeps an absolute link as is.
        current = current.parent().map_or(link.clone(), |dir| dir.join(link));
    }
    Err("save failed: too many symlink levels".to_owned())
}

/// Applies `edits` to the config file at `path`. The error is the one-line
/// footer text.
///
/// The file is read at save time, edited in place (comments and unknown keys
/// survive) and replaced atomically. A symlinked file is written through, so
/// dotfile managers keep their link.
pub(super) fn save_config(path: Option<&Path>, edits: &[ConfigEdit]) -> Result<(), String> {
    let failed = |error: io::Error| format!("save failed: {error}");
    let path = path.ok_or("save failed: no config path (HOME is not set)")?;
    let target = resolve_target(path)?;
    // Replacing by rename would ignore a read-only file; refuse it instead.
    if fs::metadata(&target).is_ok_and(|meta| meta.permissions().mode() & 0o200 == 0) {
        return Err("config.toml is read-only".to_owned());
    }
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
/// so the rename is atomic. A stale temp from a crashed run is replaced once.
/// The temp is removed on a failure only if this call created it, and an
/// existing file's permissions carry over.
fn write_atomic(target: &Path, content: &str) -> io::Result<()> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = target.with_file_name(format!(".{name}.{}.tmp", process::id()));
    let create = || OpenOptions::new().write(true).create_new(true).open(&temp);
    let file = match create() {
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            fs::remove_file(&temp)?;
            create()?
        }
        other => other?,
    };
    let result = fill_temp(file, target, content).and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn fill_temp(mut file: fs::File, target: &Path, content: &str) -> io::Result<()> {
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
        value: std::borrow::Cow::Borrowed("top"),
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
        assert_eq!(
            result,
            Err("save failed: Permission denied (os error 13)".to_owned())
        );
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

    // Spec (config-persistence): AI collisions use the same footer wording.
    // Characterization: the generic mapping already covers the `ai` keys.
    #[test]
    fn ai_collisions_map_to_their_message_and_stay_byte_identical() {
        let engine = ConfigEdit {
            table: "ai",
            key: "engine",
            value: std::borrow::Cow::Borrowed("ollama"),
        };
        let cases = [
            (
                "ai = 3\n",
                "config.toml: ai is not a table; fix it before saving",
            ),
            (
                "[ai.engine]\nx = 1\n",
                "config.toml: ai.engine is a table; fix it before saving",
            ),
            (
                "[ai]\nengine = \"x\"\n[ai.model]\nx = 1\n",
                "config.toml: ai.model is a table; fix it before saving",
            ),
        ];
        let model = ConfigEdit {
            key: "model",
            ..engine.clone()
        };
        for (text, message) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = config_in(&dir);
            fs::write(&path, text).unwrap();
            assert_eq!(
                save_config(Some(&path), &[engine.clone(), model.clone()]),
                Err(message.to_owned()),
                "for {text:?}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    // Spec (config-persistence): Profile key collisions.
    #[test]
    fn profile_collisions_map_to_their_message_and_stay_byte_identical() {
        let profile = ConfigEdit {
            table: "ai",
            key: "profile",
            value: std::borrow::Cow::Borrowed("analyst"),
        };
        let cases = [
            (
                "[ai.profile]\nx = 1\n",
                "config.toml: ai.profile is a table; fix it before saving",
            ),
            (
                "ai = 3\n",
                "config.toml: ai is not a table; fix it before saving",
            ),
        ];
        for (text, message) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = config_in(&dir);
            fs::write(&path, text).unwrap();
            assert_eq!(
                save_config(Some(&path), std::slice::from_ref(&profile)),
                Err(message.to_owned()),
                "for {text:?}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_scalar_profile_is_replaced_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "[ai]\nprofile = 3\n").unwrap();
        let profile = ConfigEdit {
            table: "ai",
            key: "profile",
            value: std::borrow::Cow::Borrowed("analyst"),
        };
        save_config(Some(&path), &[profile]).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[ai]\nprofile = \"analyst\"\n"
        );
    }

    // Spec (config-persistence): nested collisions name the full path.
    #[test]
    fn nested_ai_collisions_map_to_their_message_and_stay_byte_identical() {
        let code = |key| ConfigEdit {
            table: "ai.tasks.code",
            key,
            value: std::borrow::Cow::Borrowed("claude"),
        };
        let cases = [
            (
                "ai = 3\n",
                "config.toml: ai is not a table; fix it before saving",
            ),
            (
                "[ai]\ntasks = 3\n",
                "config.toml: ai.tasks is not a table; fix it before saving",
            ),
            (
                "[ai.tasks]\ncode = \"x\"\n",
                "config.toml: ai.tasks.code is not a table; fix it before saving",
            ),
            (
                "[ai.tasks.code.engine]\nx = 1\n",
                "config.toml: ai.tasks.code.engine is a table; fix it before saving",
            ),
        ];
        for (text, message) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = config_in(&dir);
            fs::write(&path, text).unwrap();
            assert_eq!(
                save_config(Some(&path), &[code("engine"), code("model")]),
                Err(message.to_owned()),
                "for {text:?}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
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

    // Spec: dangling symlinks are followed, never replaced.
    #[test]
    fn a_dangling_relative_link_is_followed_and_its_target_created() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("config.toml");
        symlink("dotfiles/lazarobox/config.toml", &link).unwrap();
        assert_eq!(save_config(Some(&link), &[STATUSLINE_TOP]), Ok(()));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            fs::read_to_string(dir.path().join("dotfiles/lazarobox/config.toml")).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
        assert_eq!(names(dir.path()), ["config.toml", "dotfiles"]);
    }

    #[test]
    fn a_dangling_chain_is_followed_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("config.toml");
        let middle = dir.path().join("middle");
        symlink("middle", &link).unwrap();
        symlink("store/real.toml", &middle).unwrap();
        assert_eq!(save_config(Some(&link), &[STATUSLINE_TOP]), Ok(()));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert!(fs::symlink_metadata(&middle).unwrap().is_symlink());
        assert_eq!(
            fs::read_to_string(dir.path().join("store/real.toml")).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
    }

    /// `config.toml -> l1 -> ... -> l{n-1} -> store/real.toml` (n links, the
    /// last target missing). Returns the path of the first link.
    fn dangling_chain(dir: &Path, links: usize) -> PathBuf {
        let name = |i: usize| match i {
            0 => "config.toml".to_owned(),
            i => format!("l{i}"),
        };
        for i in 0..links {
            let target = if i + 1 == links {
                "store/real.toml".to_owned()
            } else {
                name(i + 1)
            };
            symlink(target, dir.join(name(i))).unwrap();
        }
        dir.join(name(0))
    }

    // Boundary: at most 8 links are followed.
    #[test]
    fn a_dangling_chain_of_eight_links_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let first = dangling_chain(dir.path(), 8);
        assert_eq!(save_config(Some(&first), &[STATUSLINE_TOP]), Ok(()));
        assert_eq!(
            fs::read_to_string(dir.path().join("store/real.toml")).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
        for i in 1..8 {
            let link = dir.path().join(format!("l{i}"));
            assert!(fs::symlink_metadata(link).unwrap().is_symlink());
        }
        assert!(fs::symlink_metadata(&first).unwrap().is_symlink());
    }

    #[test]
    fn a_dangling_chain_of_nine_links_is_refused_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dangling_chain(dir.path(), 9);
        assert_eq!(
            save_config(Some(&first), &[STATUSLINE_TOP]),
            Err("save failed: too many symlink levels".to_owned())
        );
        assert!(!dir.path().join("store").exists());
    }

    // Characterization: an absolute dangling target is followed as is.
    #[test]
    fn an_absolute_dangling_target_is_followed_and_created() {
        let dir = tempfile::tempdir().unwrap();
        let link_dir = dir.path().join("home");
        fs::create_dir(&link_dir).unwrap();
        let link = link_dir.join("config.toml");
        let real = dir.path().join("elsewhere/config.toml");
        symlink(&real, &link).unwrap();
        assert_eq!(save_config(Some(&link), &[STATUSLINE_TOP]), Ok(()));
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
    }

    // Characterization: the read-only check applies to the resolved target.
    #[test]
    fn a_read_only_target_behind_a_symlink_is_refused_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        let link = dir.path().join("config.toml");
        fs::write(&real, "[statusline]\nposition = \"bottom\"\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o444)).unwrap();
        symlink(&real, &link).unwrap();
        assert_eq!(
            save_config(Some(&link), &[STATUSLINE_TOP]),
            Err("config.toml is read-only".to_owned())
        );
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "[statusline]\nposition = \"bottom\"\n"
        );
        assert_eq!(names(dir.path()), ["config.toml", "real.toml"]);
    }

    #[test]
    fn a_symlink_loop_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        symlink("b", &a).unwrap();
        symlink("a", &b).unwrap();
        assert_eq!(
            save_config(Some(&a), &[STATUSLINE_TOP]),
            Err("save failed: too many symlink levels".to_owned())
        );
        assert_eq!(names(dir.path()), ["a", "b"]);
    }

    // Spec: Read-only file refused.
    #[test]
    fn a_read_only_file_is_refused_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "[statusline]\nposition = \"bottom\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(
            save_config(Some(&path), &[STATUSLINE_TOP]),
            Err("config.toml is read-only".to_owned())
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[statusline]\nposition = \"bottom\"\n"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o444
        );
        assert_eq!(names(dir.path()), ["config.toml"]);
    }

    // Spec: a temp left by a crash does not block the first save.
    #[test]
    fn a_stale_temp_from_a_crash_does_not_block_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        let stale = dir
            .path()
            .join(format!(".config.toml.{}.tmp", std::process::id()));
        fs::write(&stale, "junk").unwrap();
        assert_eq!(save_config(Some(&path), &[STATUSLINE_TOP]), Ok(()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[statusline]\nposition = \"top\"\n"
        );
        assert_eq!(names(dir.path()), ["config.toml"]);
    }

    // Spec (prompt-profiles, Context toggle): Collision.
    #[test]
    fn a_context_table_maps_to_its_message_and_stays_byte_identical() {
        let context = ConfigEdit {
            table: "ai",
            key: "context",
            value: std::borrow::Cow::Borrowed("false"),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        fs::write(&path, "[ai.context]\nx = 1\n").unwrap();
        assert_eq!(
            save_config(Some(&path), std::slice::from_ref(&context)),
            Err("config.toml: ai.context is a table; fix it before saving".to_owned())
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "[ai.context]\nx = 1\n");
    }

    #[test]
    fn a_context_save_writes_a_boolean_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_in(&dir);
        let context = ConfigEdit {
            table: "ai",
            key: "context",
            value: std::borrow::Cow::Borrowed("false"),
        };
        save_config(Some(&path), &[context]).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[ai]\ncontext = false\n"
        );
    }
}
