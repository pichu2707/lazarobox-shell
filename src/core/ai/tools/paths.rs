//! Which paths the read tools may touch.
//!
//! The rule is a deny list under `$HOME`: another tool's credential folders,
//! `~/.ssh`, `~/.gnupg` and `.env` files at any depth, plus everything outside
//! `$HOME`. Every refusal looks the same, so a model cannot probe which files
//! exist or which are excluded.

use std::path::{Component, Path, PathBuf};

use crate::core::ai::jail::CREDENTIAL_DIRS;

/// The one text returned for an excluded, outside or missing path.
pub const REFUSED: &str = "path not available";

/// Folders under `$HOME` that are never readable, besides `CREDENTIAL_DIRS`.
const SECRET_DIRS: &[&str] = &[".ssh", ".gnupg"];

/// The verdict on a path. `Allowed` carries the real path to open.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PathVerdict {
    Allowed(PathBuf),
    Refused,
}

/// Whether a file or folder name is an environment file: `.env` or `.env.*`.
/// Case-insensitive (ASCII), so `.ENV` on a case-folding filesystem counts.
pub fn is_env_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env" || lower.starts_with(".env.")
}

/// Whether an entry name is excluded wherever it appears below `$HOME`. The
/// walking tools skip such entries; the root folders are judged by position
/// in [`check_path`].
pub fn is_excluded_name(name: &str) -> bool {
    is_env_name(name)
}

/// Whether `name` is an excluded folder when it sits directly under `$HOME`.
fn is_secret_root(name: &str) -> bool {
    SECRET_DIRS
        .iter()
        .chain(CREDENTIAL_DIRS)
        .any(|dir| dir.eq_ignore_ascii_case(name))
}

/// Judges what follows `$HOME` in a path: refused when it holds a `.` or `..`
/// part, starts with an excluded folder, or has an excluded name anywhere.
fn judge_rest(rest: &Path) -> bool {
    let mut names = Vec::new();
    for part in rest.components() {
        match part {
            Component::Normal(name) => names.push(name.to_string_lossy()),
            _ => return false,
        }
    }
    !names.first().is_some_and(|first| is_secret_root(first))
        && !names.iter().any(|name| is_excluded_name(name))
}

/// Judges an already resolved path (no symlinks, no `.` or `..`) against the
/// resolved `home`. Pure: it reads no file.
pub fn check_path(home: &Path, real: &Path) -> PathVerdict {
    match real.strip_prefix(home) {
        Ok(rest) if judge_rest(rest) => PathVerdict::Allowed(real.to_path_buf()),
        _ => PathVerdict::Refused,
    }
}

/// Removes `.` and resolves `..` by text alone, without touching the disk.
/// A `..` at the root stays at the root.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | None
                ) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Expands a leading `~` or `~/` to `home`. `~user` is not supported and
/// gives `None`.
fn expand_tilde(home: &Path, input: &str) -> Option<PathBuf> {
    match input.strip_prefix('~') {
        None => Some(PathBuf::from(input)),
        Some("") => Some(home.to_path_buf()),
        Some(rest) => rest.strip_prefix('/').map(|rest| home.join(rest)),
    }
}

/// Resolves what a model asked for and judges it twice. A canonical-only
/// check is not enough: with a dotfile manager `~/.ssh` may be a link to
/// `~/dotfiles/ssh`, whose real path looks ordinary. So the path is judged as
/// REQUESTED (`~` expanded, relative input read against `origin`, `.` and `..`
/// folded by text) and as REAL (every symlink followed). It is allowed only
/// when both pass.
///
/// `home` and `origin` must be absolute and `home` must not be `/`; anything
/// else is refused, because `canonicalize` on a relative path would depend on
/// the process cwd. A path that cannot be resolved is `Refused`.
pub fn resolve(home: &Path, origin: &Path, input: &str) -> PathVerdict {
    if !home.is_absolute() || home == Path::new("/") || !origin.is_absolute() {
        return PathVerdict::Refused;
    }
    let Ok(real_home) = home.canonicalize() else {
        return PathVerdict::Refused;
    };
    let Some(expanded) = expand_tilde(&real_home, input) else {
        return PathVerdict::Refused;
    };
    let full = if expanded.is_absolute() {
        expanded
    } else {
        origin.join(expanded)
    };
    let requested = normalize(&full);
    // The requested path may name HOME as given (a link) or as resolved.
    let lexical_ok = [normalize(home), real_home.clone()]
        .iter()
        .find_map(|h| requested.strip_prefix(h).ok())
        .is_some_and(judge_rest);
    if !lexical_ok {
        return PathVerdict::Refused;
    }
    match full.canonicalize() {
        Ok(real) => check_path(&real_home, &real),
        Err(_) => PathVerdict::Refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    /// A temp `$HOME` with a few files, canonical like the real one.
    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        let outside = root.join("outside");
        for sub in [
            "home/project/src",
            "home/.ssh",
            "home/.gnupg",
            "home/.claude/sub",
            "home/deep/er/.env.d",
            "outside",
        ] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        for file in [
            "home/project/src/lib.rs",
            "home/.ssh/id",
            "home/.env",
            "home/.env.local",
            "home/deep/er/.env",
            "home/deep/er/.env.d/x",
            "home/.claude/sub/token",
            "outside/note.txt",
        ] {
            fs::write(root.join(file), "x").unwrap();
        }
        Fixture {
            _dir: dir,
            home,
            outside,
        }
    }

    fn allowed(f: &Fixture, input: &str) -> bool {
        matches!(
            resolve(&f.home, &f.home.join("project"), input),
            PathVerdict::Allowed(_)
        )
    }

    // Spec (agent-tools, path policy): excluded folders and children.
    #[test]
    fn every_credential_dir_and_a_child_are_refused() {
        let f = fixture();
        for dir in CREDENTIAL_DIRS {
            let path = f.home.join(dir);
            fs::create_dir_all(path.join("child")).unwrap();
            assert_eq!(check_path(&f.home, &path), PathVerdict::Refused, "{dir}");
            assert_eq!(
                check_path(&f.home, &path.join("child")),
                PathVerdict::Refused,
                "{dir}/child"
            );
        }
    }

    #[test]
    fn ssh_and_gnupg_are_refused() {
        let f = fixture();
        for p in [".ssh", ".ssh/id", ".gnupg"] {
            assert_eq!(
                check_path(&f.home, &f.home.join(p)),
                PathVerdict::Refused,
                "{p}"
            );
        }
    }

    #[test]
    fn env_files_are_refused_at_any_depth() {
        let f = fixture();
        for p in [
            ".env",
            ".env.local",
            "deep/er/.env",
            "deep/er/.env.d",
            "deep/er/.env.d/x",
        ] {
            assert_eq!(
                check_path(&f.home, &f.home.join(p)),
                PathVerdict::Refused,
                "{p}"
            );
        }
        for name in [".envrc", ".environment", "env", "x.env"] {
            let p = f.home.join(name);
            assert_eq!(check_path(&f.home, &p), PathVerdict::Allowed(p), "{name}");
        }
    }

    #[test]
    fn home_and_ordinary_paths_are_allowed() {
        let f = fixture();
        for p in ["", "project", "project/src/lib.rs", ".config", "deep/er"] {
            let path = f.home.join(p);
            assert_eq!(
                check_path(&f.home, &path),
                PathVerdict::Allowed(path.clone()),
                "{p}"
            );
        }
    }

    #[test]
    fn outside_home_is_refused() {
        let f = fixture();
        assert_eq!(
            check_path(&f.home, &f.outside.join("note.txt")),
            PathVerdict::Refused
        );
        assert_eq!(check_path(&f.home, Path::new("/")), PathVerdict::Refused);
        // A sibling that only shares the textual prefix.
        let sibling = PathBuf::from(format!("{}-evil/x", f.home.display()));
        assert_eq!(check_path(&f.home, &sibling), PathVerdict::Refused);
    }

    #[test]
    fn traversal_is_judged_by_where_it_lands() {
        let f = fixture();
        assert!(!allowed(&f, "../.ssh/id"));
        assert!(!allowed(&f, "../../outside/note.txt"));
        assert!(!allowed(&f, "src/../../.env"));
        assert!(allowed(&f, "src/../src/lib.rs"));
    }

    #[test]
    fn a_symlink_to_an_excluded_file_is_refused() {
        let f = fixture();
        symlink(f.home.join(".ssh/id"), f.home.join("project/innocent")).unwrap();
        symlink(f.home.join(".env"), f.home.join("project/config")).unwrap();
        symlink(f.home.join(".claude"), f.home.join("project/dir")).unwrap();
        assert!(!allowed(&f, "innocent"));
        assert!(!allowed(&f, "config"));
        assert!(!allowed(&f, "dir/sub/token"));
    }

    #[test]
    fn a_symlinked_directory_out_of_home_is_refused() {
        let f = fixture();
        symlink(&f.outside, f.home.join("project/escape")).unwrap();
        assert!(!allowed(&f, "escape"));
        assert!(!allowed(&f, "escape/note.txt"));
    }

    #[test]
    fn a_symlink_inside_home_resolves_to_the_real_path() {
        let f = fixture();
        symlink(
            f.home.join("project/src/lib.rs"),
            f.home.join("project/link"),
        )
        .unwrap();
        assert_eq!(
            resolve(&f.home, &f.home.join("project"), "link"),
            PathVerdict::Allowed(f.home.join("project/src/lib.rs"))
        );
    }

    #[test]
    fn a_relative_input_is_read_against_the_origin() {
        let f = fixture();
        let real = f.home.join("project/src/lib.rs");
        assert_eq!(
            resolve(&f.home, &f.home.join("project"), "src/lib.rs"),
            PathVerdict::Allowed(real.clone())
        );
        assert_eq!(
            resolve(&f.home, &f.home.join("project/src"), "lib.rs"),
            PathVerdict::Allowed(real.clone())
        );
        assert_eq!(
            resolve(&f.home, &f.home.join("deep"), "src/lib.rs"),
            PathVerdict::Refused,
            "missing against another origin"
        );
        assert_eq!(
            resolve(&f.home, &f.outside, real.to_str().unwrap()),
            PathVerdict::Allowed(real),
            "absolute ignores the origin"
        );
    }

    // Spec: one identical refusal for excluded, outside and missing.
    #[test]
    fn excluded_outside_and_missing_are_one_identical_verdict_and_text() {
        let f = fixture();
        let origin = f.home.join("project");
        let excluded = resolve(&f.home, &origin, "../.ssh/id");
        let outside = resolve(
            &f.home,
            &origin,
            f.outside.join("note.txt").to_str().unwrap(),
        );
        let missing = resolve(&f.home, &origin, "nope/nothing");
        assert_eq!(excluded, PathVerdict::Refused);
        assert_eq!(excluded, outside);
        assert_eq!(excluded, missing);
        assert_eq!(REFUSED, "path not available");
    }

    #[test]
    fn a_home_that_cannot_be_resolved_refuses_everything() {
        let f = fixture();
        assert_eq!(
            resolve(&f.home.join("gone"), &f.home, "project"),
            PathVerdict::Refused
        );
    }

    // Review C1: a canonical-only check is bypassed by a symlinked dotfile.
    #[test]
    fn a_secret_name_that_links_to_an_ordinary_place_is_refused() {
        let f = fixture();
        let dot = f.home.join("dotfiles");
        fs::create_dir_all(dot.join("ssh/sub")).unwrap();
        fs::write(dot.join("ssh/id_ed25519"), "k").unwrap();
        fs::write(dot.join("ssh/sub/f"), "k").unwrap();
        fs::create_dir_all(dot.join("claude")).unwrap();
        fs::write(dot.join("claude/token"), "k").unwrap();
        fs::write(dot.join("app.env"), "k").unwrap();
        fs::remove_dir_all(f.home.join(".ssh")).unwrap();
        fs::remove_dir_all(f.home.join(".claude")).unwrap();
        symlink(dot.join("ssh"), f.home.join(".ssh")).unwrap();
        symlink(dot.join("claude"), f.home.join(".claude")).unwrap();
        symlink(dot.join("app.env"), f.home.join("project/.env")).unwrap();
        let origin = f.home.join("project");
        for input in [
            "~/.ssh/id_ed25519",
            "~/.ssh/sub/f",
            "~/.ssh",
            "~/.claude/token",
            ".env",
            "./.env",
        ] {
            assert_eq!(
                resolve(&f.home, &origin, input),
                PathVerdict::Refused,
                "{input}"
            );
        }
        // The real files are ordinary and stay readable by their real name.
        assert!(allowed(&f, "../dotfiles/ssh/id_ed25519"));
    }

    #[test]
    fn an_intermediate_symlinked_directory_is_judged_by_its_name_too() {
        let f = fixture();
        fs::create_dir_all(f.home.join("store/inner")).unwrap();
        fs::write(f.home.join("store/inner/file"), "x").unwrap();
        symlink(f.home.join("store"), f.home.join(".gnupg-moved")).unwrap();
        symlink(f.home.join("store/inner"), f.home.join("project/.env.d")).unwrap();
        assert!(allowed(&f, "../.gnupg-moved/inner/file"));
        assert!(!allowed(&f, ".env.d/file"), "an .env.* directory name");
        symlink(f.home.join("store"), f.home.join(".aws")).unwrap();
        assert!(!allowed(&f, "~/.aws/inner/file"));
    }

    // Review W1.
    #[test]
    fn a_leading_tilde_means_home() {
        let f = fixture();
        assert!(allowed(&f, "~"));
        assert!(allowed(&f, "~/"));
        assert!(allowed(&f, "~/project/src/lib.rs"));
        assert_eq!(
            resolve(&f.home, &f.outside, "~/project/src/lib.rs"),
            PathVerdict::Allowed(f.home.join("project/src/lib.rs"))
        );
        for input in [
            "~/.ssh/id",
            "~/.env",
            "~root/x",
            "~other",
            "~/../outside/note.txt",
        ] {
            assert!(!allowed(&f, input), "{input}");
        }
        assert!(!allowed(&f, "x/~/y"), "only a leading tilde expands");
    }

    #[test]
    fn a_symlink_under_tilde_is_followed_and_judged() {
        let f = fixture();
        symlink(&f.outside, f.home.join("project/link")).unwrap();
        symlink(f.home.join("project/src"), f.home.join("project/ok")).unwrap();
        assert!(!allowed(&f, "~/project/link/note.txt"));
        assert!(allowed(&f, "~/project/ok/lib.rs"));
    }

    // Review W3.
    #[test]
    fn an_unusable_home_or_origin_refuses_everything() {
        let f = fixture();
        let origin = f.home.join("project");
        for home in ["", "/", "relative/home", "."] {
            assert_eq!(
                resolve(Path::new(home), &origin, "/"),
                PathVerdict::Refused,
                "{home:?}"
            );
            assert_eq!(
                resolve(Path::new(home), &origin, "project"),
                PathVerdict::Refused,
                "{home:?}"
            );
        }
        for origin in ["", "project", "."] {
            assert_eq!(
                resolve(&f.home, Path::new(origin), "project"),
                PathVerdict::Refused,
                "{origin:?}"
            );
            assert_eq!(
                resolve(&f.home, Path::new(origin), "~/project"),
                PathVerdict::Refused,
                "{origin:?}"
            );
        }
    }

    // Review W4.
    #[test]
    fn names_match_without_regard_to_ascii_case() {
        let f = fixture();
        for p in [
            ".SSH",
            ".Gnupg/x",
            ".Claude",
            ".AWS/a",
            ".ENV",
            ".Env.Local",
            "deep/.EnV",
        ] {
            assert_eq!(
                check_path(&f.home, &f.home.join(p)),
                PathVerdict::Refused,
                "{p}"
            );
        }
        assert!(is_env_name(".ENV.prod"));
        assert!(!is_env_name(".ENVRC"));
    }
}
