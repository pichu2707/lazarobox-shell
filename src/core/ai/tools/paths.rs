//! Which paths the read tools may touch.
//!
//! The rule is a deny list under `$HOME`: another tool's credential folders,
//! `~/.ssh`, `~/.gnupg`, tool configs that hold tokens, keyrings, lazarobox's
//! own store, and credential, history and `.env` files at any depth, plus
//! everything outside `$HOME`. Every refusal looks the same, so a model cannot probe which files
//! exist or which are excluded.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::core::ai::jail::CREDENTIAL_DIRS;

/// The one text returned for an excluded, outside or missing path.
pub const REFUSED: &str = "path not available";

/// Folders and files under `$HOME`, given as a path relative to it, that are
/// never readable, besides `CREDENTIAL_DIRS`. `.local/share/lazarobox` is the
/// app's own store (chat history, engram, update cache) at its default XDG
/// place; a custom `$XDG_DATA_HOME` is not followed here, nor are
/// `$XDG_CONFIG_HOME` or `$GH_CONFIG_DIR`. The list is best effort, not
/// complete.
const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".config/gh",
    ".config/gcloud",
    ".local/share/keyrings",
    ".local/share/fish/fish_history",
    ".local/share/lazarobox",
    // The app's config holds `config.toml` with MCP env tokens.
    ".config/lazarobox",
];

/// File names that are never readable wherever they sit below `$HOME`:
/// credential files, `.envrc`, `.npmrc` (denied at any depth on purpose, as a
/// project file may hold a token) and shell or REPL histories.
const SECRET_FILES: &[&str] = &[
    ".netrc",
    ".git-credentials",
    ".pgpass",
    ".npmrc",
    ".envrc",
    ".bash_history",
    ".zsh_history",
    ".zhistory",
    ".python_history",
    ".node_repl_history",
    ".psql_history",
    ".mysql_history",
    ".sqlite_history",
];

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
        || SECRET_FILES
            .iter()
            .any(|file| file.eq_ignore_ascii_case(name))
}

/// Whether the names below `$HOME` start with an excluded root (ASCII
/// case-insensitive, component by component).
fn starts_with_secret_root(names: &[std::borrow::Cow<'_, str>]) -> bool {
    SECRET_DIRS.iter().chain(CREDENTIAL_DIRS).any(|root| {
        let mut parts = root.split('/');
        names.len() >= root.split('/').count()
            && names
                .iter()
                .zip(&mut parts)
                .all(|(name, part)| part.eq_ignore_ascii_case(name))
    })
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
    !starts_with_secret_root(&names) && !names.iter().any(|name| is_excluded_name(name))
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

/// Top folders that are never a home: `/` and the system trees. `/root` is a
/// real home and is allowed.
const SYSTEM_DIRS: &[&str] = &[
    "/home",
    "/Users",
    "/usr",
    "/var",
    "/etc",
    "/opt",
    "/tmp",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/mnt",
    "/media",
    "/srv",
    "/boot",
    "/private",
    "/private/tmp",
    "/private/var",
    "/private/etc",
    "/System",
    "/Library",
];

/// Whether a canonical HOME is `/` or one of [`SYSTEM_DIRS`], compared
/// component by component and ASCII case-insensitive (a case-folding
/// filesystem makes `/USERS` the same folder as `/Users`).
fn is_system_dir(home: &Path) -> bool {
    let same = |dir: &str| {
        let (mut a, mut b) = (home.components(), Path::new(dir).components());
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) => {
                    let (x, y) = (x.as_os_str(), y.as_os_str());
                    if !x.eq_ignore_ascii_case(y) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    };
    same("/") || SYSTEM_DIRS.iter().any(|dir| same(dir))
}

/// The real path of `path`, `Ok(None)` when it is not there (missing, or a
/// parent is not a folder) and `Err` for anything else (permissions, loops):
/// an unreadable root must refuse, never be skipped.
fn real_or_absent(path: &Path) -> Result<Option<PathBuf>, ()> {
    match path.canonicalize() {
        Ok(real) => Ok(Some(real)),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
        Err(_) => Err(()),
    }
}

/// A validated HOME and the real paths of its secret roots: `~/.ssh`,
/// `~/.gnupg`, the other nested roots, every credential folder, and each
/// excluded file name directly in HOME. Following the links is what stops a
/// second name for the same secret (`~/foo -> ~/dotfiles/ssh`) or a link with
/// an ordinary target (`~/.env -> ~/secrets/app.env`).
///
/// Building it reads the disk, so a tool call builds it ONCE and judges every
/// path (and every listed entry) against it. Known limit: a link BELOW HOME
/// depth one, named like any excluded file (`.env`, `.netrc`, `.npmrc`, a
/// history file) with an ordinary target, is judged by its requested name
/// only, and so is any root the user moved with `XDG_CONFIG_HOME`,
/// `XDG_DATA_HOME` or `GH_CONFIG_DIR`, unless the caller passes the real
/// folders as `extra` roots (the tool runtime passes lazarobox's own config and
/// data folders, which honour `XDG_CONFIG_HOME` and `XDG_DATA_HOME`).
///
/// The roots are a SNAPSHOT taken at load: a root created after the load is
/// out of scope for that call. The tool runtime re-checks the real path of the
/// opened file descriptor against them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SecretRoots {
    /// HOME as given, folded by text.
    given: PathBuf,
    /// HOME with every link followed.
    real: PathBuf,
    roots: Vec<PathBuf>,
}

impl SecretRoots {
    /// Validates `home` and reads its secret roots. `None` (refuse everything)
    /// when `home` is relative, cannot be resolved, resolves to `/` or a system
    /// folder, or when any root or HOME itself cannot be read. A root that is
    /// simply absent (not found, or a parent that is not a folder) is skipped.
    /// `extra` are further absolute roots, resolved with the same fail-closed
    /// rules; a relative one refuses everything.
    pub fn load(home: &Path, extra: &[PathBuf]) -> Option<SecretRoots> {
        if !home.is_absolute() || extra.iter().any(|root| !root.is_absolute()) {
            return None;
        }
        let real = home.canonicalize().ok()?;
        // Judge the name as given too: `/bin` resolves to `/usr/bin` on a
        // merged-usr system.
        if is_system_dir(&real) || is_system_dir(&normalize(home)) {
            return None;
        }
        let mut roots = Vec::new();
        for dir in SECRET_DIRS.iter().chain(CREDENTIAL_DIRS) {
            roots.extend(real_or_absent(&real.join(dir)).ok()?);
        }
        for root in extra {
            roots.extend(real_or_absent(root).ok()?);
        }
        for entry in std::fs::read_dir(&real).ok()? {
            let entry = entry.ok()?;
            if is_excluded_name(&entry.file_name().to_string_lossy()) {
                roots.extend(real_or_absent(&entry.path()).ok()?);
            }
        }
        Some(SecretRoots {
            given: normalize(home),
            real,
            roots,
        })
    }

    /// The resolved HOME.
    pub fn home(&self) -> &Path {
        &self.real
    }
}

/// Resolves what a model asked for and judges it twice. A canonical-only
/// check is not enough: with a dotfile manager `~/.ssh` may be a link to
/// `~/dotfiles/ssh`, whose real path looks ordinary. So the path is judged as
/// REQUESTED (`~` expanded, relative input read against `origin`, `.` and `..`
/// folded by text) and as REAL (every symlink followed). It is allowed only
/// when both pass, and the real path is not (under) the real target of a
/// secret root in `roots`.
///
/// `origin` must be absolute, because `canonicalize` on a relative path would
/// depend on the process cwd. A path that cannot be resolved is `Refused`.
pub fn resolve(roots: &SecretRoots, origin: &Path, input: &str) -> PathVerdict {
    if !origin.is_absolute() {
        return PathVerdict::Refused;
    }
    let Some(expanded) = expand_tilde(&roots.real, input) else {
        return PathVerdict::Refused;
    };
    let full = if expanded.is_absolute() {
        expanded
    } else {
        origin.join(expanded)
    };
    let requested = normalize(&full);
    // The requested path may name HOME as given (a link) or as resolved.
    let lexical_ok = [&roots.given, &roots.real]
        .iter()
        .find_map(|h| requested.strip_prefix(h).ok())
        .is_some_and(judge_rest);
    if !lexical_ok {
        return PathVerdict::Refused;
    }
    match full.canonicalize() {
        Ok(real) if roots.roots.iter().any(|root| real.starts_with(root)) => PathVerdict::Refused,
        Ok(real) => check_path(&roots.real, &real),
        Err(_) => PathVerdict::Refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    /// Loads the roots of `home` and resolves; an unusable HOME refuses.
    fn res(home: &Path, origin: &Path, input: &str) -> PathVerdict {
        match SecretRoots::load(home, &[]) {
            Some(roots) => resolve(&roots, origin, input),
            None => PathVerdict::Refused,
        }
    }

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
            res(&f.home, &f.home.join("project"), input),
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
        for name in [".environment", "env", "x.env"] {
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
            res(&f.home, &f.home.join("project"), "link"),
            PathVerdict::Allowed(f.home.join("project/src/lib.rs"))
        );
    }

    #[test]
    fn a_relative_input_is_read_against_the_origin() {
        let f = fixture();
        let real = f.home.join("project/src/lib.rs");
        assert_eq!(
            res(&f.home, &f.home.join("project"), "src/lib.rs"),
            PathVerdict::Allowed(real.clone())
        );
        assert_eq!(
            res(&f.home, &f.home.join("project/src"), "lib.rs"),
            PathVerdict::Allowed(real.clone())
        );
        assert_eq!(
            res(&f.home, &f.home.join("deep"), "src/lib.rs"),
            PathVerdict::Refused,
            "missing against another origin"
        );
        assert_eq!(
            res(&f.home, &f.outside, real.to_str().unwrap()),
            PathVerdict::Allowed(real),
            "absolute ignores the origin"
        );
    }

    // Spec: one identical refusal for excluded, outside and missing.
    #[test]
    fn excluded_outside_and_missing_are_one_identical_verdict_and_text() {
        let f = fixture();
        let origin = f.home.join("project");
        let excluded = res(&f.home, &origin, "../.ssh/id");
        let outside = res(
            &f.home,
            &origin,
            f.outside.join("note.txt").to_str().unwrap(),
        );
        let missing = res(&f.home, &origin, "nope/nothing");
        assert_eq!(excluded, PathVerdict::Refused);
        assert_eq!(excluded, outside);
        assert_eq!(excluded, missing);
        assert_eq!(REFUSED, "path not available");
    }

    #[test]
    fn a_home_that_cannot_be_resolved_refuses_everything() {
        let f = fixture();
        assert_eq!(
            res(&f.home.join("gone"), &f.home, "project"),
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
                res(&f.home, &origin, input),
                PathVerdict::Refused,
                "{input}"
            );
        }
        // Review C1: the real folder behind a secret root is secret too.
        assert!(!allowed(&f, "../dotfiles/ssh/id_ed25519"));
        assert!(!allowed(&f, "../dotfiles/ssh"));
        assert!(!allowed(&f, "../dotfiles/claude/token"));
        assert!(allowed(&f, "../dotfiles"), "the parent stays ordinary");
    }

    // Review C1: another name for the same secret folder is refused too.
    #[test]
    fn a_second_alias_of_a_secret_folder_is_refused() {
        let f = fixture();
        let dot = f.home.join("dotfiles");
        fs::create_dir_all(&dot).unwrap();
        fs::rename(f.home.join(".ssh"), dot.join("ssh")).unwrap();
        symlink(dot.join("ssh"), f.home.join(".ssh")).unwrap();
        symlink(dot.join("ssh"), f.home.join("foo")).unwrap();
        assert!(!allowed(&f, "~/foo/id"));
        assert!(!allowed(&f, "~/foo"));
        assert!(!allowed(&f, "~/dotfiles/ssh/id"));
    }

    // Review C1: a `.env` link whose target has an ordinary name.
    #[test]
    fn an_env_link_in_home_protects_its_target() {
        let f = fixture();
        fs::remove_file(f.home.join(".env")).unwrap();
        fs::create_dir_all(f.home.join("secrets")).unwrap();
        fs::write(f.home.join("secrets/app.env"), "k").unwrap();
        symlink(f.home.join("secrets/app.env"), f.home.join(".env")).unwrap();
        assert!(!allowed(&f, "~/.env"));
        assert!(!allowed(&f, "~/secrets/app.env"));
        assert!(!allowed(&f, "../secrets/app.env"));
    }

    // Review W1: a HOME that is, or resolves to, the filesystem root.
    #[test]
    fn a_home_that_resolves_to_the_root_or_is_too_shallow_is_refused() {
        let f = fixture();
        let origin = f.home.join("project");
        symlink("/", f.home.join("rootlink")).unwrap();
        for home in [
            Path::new("x/.."),
            Path::new("/home"),
            Path::new("/tmp"),
            Path::new("/usr"),
            Path::new("/."),
            &f.home.join("rootlink"),
        ] {
            assert_eq!(
                res(home, &origin, "/etc/hostname"),
                PathVerdict::Refused,
                "{home:?}"
            );
        }
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
            res(&f.home, &f.outside, "~/project/src/lib.rs"),
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
                res(Path::new(home), &origin, "/"),
                PathVerdict::Refused,
                "{home:?}"
            );
            assert_eq!(
                res(Path::new(home), &origin, "project"),
                PathVerdict::Refused,
                "{home:?}"
            );
        }
        for origin in ["", "project", "."] {
            assert_eq!(
                res(&f.home, Path::new(origin), "project"),
                PathVerdict::Refused,
                "{origin:?}"
            );
            assert_eq!(
                res(&f.home, Path::new(origin), "~/project"),
                PathVerdict::Refused,
                "{origin:?}"
            );
        }
    }

    // User decision: wider deny list (credential files, tool configs,
    // keyrings, shell histories, lazarobox's own store).
    #[test]
    fn credential_files_at_any_depth_are_refused() {
        let f = fixture();
        for name in [
            ".netrc",
            ".git-credentials",
            ".pgpass",
            ".npmrc",
            ".envrc",
            ".bash_history",
            ".zsh_history",
            ".zhistory",
            ".python_history",
            ".node_repl_history",
            ".psql_history",
            ".mysql_history",
            ".sqlite_history",
            ".NETRC",
        ] {
            for p in [name.to_owned(), format!("project/{name}")] {
                assert_eq!(
                    check_path(&f.home, &f.home.join(&p)),
                    PathVerdict::Refused,
                    "{p}"
                );
            }
        }
        assert!(is_excluded_name(".envrc"));
    }

    #[test]
    fn nested_secret_roots_are_refused_with_their_children() {
        let f = fixture();
        for root in [
            ".config/gh",
            ".config/gcloud",
            ".local/share/keyrings",
            ".local/share/fish/fish_history",
            ".local/share/lazarobox",
            ".password-store",
            ".Config/GH",
        ] {
            for p in [root.to_owned(), format!("{root}/child")] {
                assert_eq!(
                    check_path(&f.home, &f.home.join(&p)),
                    PathVerdict::Refused,
                    "{p}"
                );
            }
        }
        for p in [
            ".config",
            ".config/nvim",
            ".config/github",
            ".local/share",
            ".local/share/fish",
            ".local/share/fish/other",
            ".local/share/lazarobox-x",
        ] {
            let path = f.home.join(p);
            assert_eq!(
                check_path(&f.home, &path),
                PathVerdict::Allowed(path),
                "{p}"
            );
        }
    }

    #[test]
    fn a_credential_file_link_in_home_protects_its_target() {
        let f = fixture();
        fs::create_dir_all(f.home.join("vault")).unwrap();
        fs::write(f.home.join("vault/n"), "k").unwrap();
        symlink(f.home.join("vault/n"), f.home.join(".netrc")).unwrap();
        assert!(!allowed(&f, "~/.netrc"));
        assert!(!allowed(&f, "~/vault/n"));
    }

    #[test]
    fn an_alias_of_a_nested_secret_root_is_refused() {
        let f = fixture();
        let dot = f.home.join("dotfiles");
        fs::create_dir_all(dot.join("gh")).unwrap();
        fs::write(dot.join("gh/hosts.yml"), "t").unwrap();
        fs::create_dir_all(f.home.join(".config")).unwrap();
        symlink(dot.join("gh"), f.home.join(".config/gh")).unwrap();
        fs::create_dir_all(f.home.join(".local/share/lazarobox")).unwrap();
        fs::write(f.home.join(".local/share/lazarobox/chat.db"), "c").unwrap();
        symlink(
            f.home.join(".local/share/lazarobox"),
            f.home.join("project/store"),
        )
        .unwrap();
        assert!(!allowed(&f, "~/.config/gh/hosts.yml"));
        assert!(!allowed(&f, "~/dotfiles/gh/hosts.yml"));
        assert!(!allowed(&f, "store/chat.db"));
        assert!(!allowed(&f, "~/.local/share/lazarobox/chat.db"));
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

    // Review W1 (round 2): `/root` is a real home, the system trees are not.
    #[test]
    fn system_folders_are_not_a_home_but_root_is() {
        for dir in [
            "/",
            "/home",
            "/Users",
            "/usr",
            "/var",
            "/etc",
            "/opt",
            "/tmp",
            "/bin",
            "/lib",
            "/proc",
            "/sys",
            "/dev",
            "/USERS",
            "/Home",
            "/private/tmp",
            "/Private/VAR",
            "/run",
            "/mnt",
            "/media",
            "/srv",
            "/sbin",
            "/boot",
            "/lib64",
            "/System",
            "/Library",
            "/private",
            "/private/etc",
        ] {
            assert!(is_system_dir(Path::new(dir)), "{dir}");
            assert!(SecretRoots::load(Path::new(dir), &[]).is_none(), "{dir}");
        }
        for dir in ["/root", "/home/u", "/srv/u"] {
            assert!(!is_system_dir(Path::new(dir)), "{dir}");
        }
    }

    /// Whether the test can make a folder unreadable (not running as root).
    fn can_deny(path: &Path) -> bool {
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        let denied = fs::read_dir(path).is_err();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        denied
    }

    // Review W2: an unreadable HOME or root fails closed.
    #[test]
    fn an_unreadable_home_refuses_everything() {
        let f = fixture();
        if !can_deny(&f.home) {
            eprintln!("SKIPPED: cannot deny read access (running as root?)");
            return;
        }
        fs::set_permissions(&f.home, fs::Permissions::from_mode(0o000)).unwrap();
        let verdict = res(&f.home, &f.outside, "~/project/src/lib.rs");
        fs::set_permissions(&f.home, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(verdict, PathVerdict::Refused);
    }

    #[test]
    fn a_root_that_cannot_be_inspected_refuses_but_an_absent_one_is_skipped() {
        let f = fixture();
        assert!(allowed(&f, "src/lib.rs"), "absent roots are fine");
        fs::create_dir_all(f.home.join(".config")).unwrap();
        if !can_deny(&f.home.join(".config")) {
            eprintln!("SKIPPED: cannot deny read access (running as root?)");
            return;
        }
        fs::set_permissions(f.home.join(".config"), fs::Permissions::from_mode(0o000)).unwrap();
        let verdict = res(&f.home, &f.home.join("project"), "src/lib.rs");
        fs::set_permissions(f.home.join(".config"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(verdict, PathVerdict::Refused);
    }

    // Review W3: one load serves many paths.
    #[test]
    fn one_load_judges_many_paths() {
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        assert_eq!(roots.home(), f.home);
        let origin = f.home.join("project");
        assert!(matches!(
            resolve(&roots, &origin, "src/lib.rs"),
            PathVerdict::Allowed(_)
        ));
        assert_eq!(resolve(&roots, &origin, "~/.ssh/id"), PathVerdict::Refused);
    }

    // Review W4: the app's own config holds MCP env tokens.
    #[test]
    fn the_app_config_folder_is_refused() {
        let f = fixture();
        fs::create_dir_all(f.home.join(".config/lazarobox")).unwrap();
        fs::write(f.home.join(".config/lazarobox/config.toml"), "t").unwrap();
        assert!(!allowed(&f, "~/.config/lazarobox/config.toml"));
        assert_eq!(
            check_path(&f.home, &f.home.join(".config/lazarobox")),
            PathVerdict::Refused
        );
    }

    // Review suggestion: a secret root that links to HOME refuses everything.
    #[test]
    fn a_secret_root_that_links_to_home_refuses_everything_under_it() {
        let f = fixture();
        fs::remove_dir_all(f.home.join(".gnupg")).unwrap();
        symlink(&f.home, f.home.join(".gnupg")).unwrap();
        assert!(!allowed(&f, "project/src/lib.rs"));
        assert!(!allowed(&f, "~"));
        assert!(!allowed(&f, "~/.gnupg/project/src/lib.rs"));
    }

    // Review W4 (round 3): a failure that is not a permission error, so it
    // also fails when the tests run as root.
    #[test]
    fn a_symlink_loop_on_a_secret_root_refuses_everything() {
        let f = fixture();
        fs::remove_dir_all(f.home.join(".gnupg")).unwrap();
        symlink(f.home.join(".gnupg"), f.home.join(".gnupg")).unwrap();
        assert!(SecretRoots::load(&f.home, &[]).is_none());
        assert!(!allowed(&f, "src/lib.rs"));
    }

    // Review W3 (round 3): extra roots are resolved like the built-in ones.
    #[test]
    fn extra_roots_are_resolved_and_fail_closed() {
        let f = fixture();
        let store = f.home.join("custom/store");
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join("chat.db"), "c").unwrap();
        symlink(&store, f.home.join("project/alias")).unwrap();
        let origin = f.home.join("project");
        let before = SecretRoots::load(&f.home, &[]).unwrap();
        assert!(matches!(
            resolve(&before, &origin, "alias/chat.db"),
            PathVerdict::Allowed(_)
        ));
        let roots = SecretRoots::load(&f.home, std::slice::from_ref(&store)).unwrap();
        for input in ["alias/chat.db", "~/custom/store/chat.db", "~/custom/store"] {
            assert_eq!(
                resolve(&roots, &origin, input),
                PathVerdict::Refused,
                "{input}"
            );
        }
        assert!(matches!(
            resolve(&roots, &origin, "src/lib.rs"),
            PathVerdict::Allowed(_)
        ));
        let missing = f.home.join("nope");
        assert!(
            SecretRoots::load(&f.home, &[missing]).is_some(),
            "absent skipped"
        );
        assert!(SecretRoots::load(&f.home, &[PathBuf::from("rel")]).is_none());
        let looped = f.home.join("loop");
        symlink(&looped, &looped).unwrap();
        assert!(SecretRoots::load(&f.home, &[looped]).is_none());
    }
}
