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

/// Whether a name is an environment template (`.env.example`,
/// `.env.local.sample`): an env name that ends in `.example`, `.sample`,
/// `.template` or `.dist` (ASCII case-insensitive). It holds no secret by
/// convention, so a write may create it; a read still refuses it.
fn is_env_template(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_env_name(name)
        && [".example", ".sample", ".template", ".dist"]
            .iter()
            .any(|ext| lower.ends_with(ext))
}

/// Whether a name is a temp file of the write primitives:
/// `.lbx-<32 lowercase hex digits>.tmp`. A crash can leave one behind; it is
/// hidden from every tool and refused as a read or write target.
pub fn is_temp_name(name: &str) -> bool {
    name.strip_prefix(".lbx-")
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|hex| {
            hex.len() == 32 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
}

/// Whether an entry name is excluded wherever it appears below `$HOME`. The
/// walking tools skip such entries; the root folders are judged by position
/// in [`check_path`].
pub fn is_excluded_name(name: &str) -> bool {
    is_env_name(name)
        || is_temp_name(name)
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

/// Whether `real` is a git configuration file: `config` or `config.worktree`
/// (ASCII case-insensitive) somewhere below a `.git` folder. It can hold a
/// remote URL with a credential and hooks-like settings, so reads refuse it
/// like a secret. Pure: it reads no file.
pub fn is_git_config(real: &Path) -> bool {
    let named = real.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        name.eq_ignore_ascii_case("config") || name.eq_ignore_ascii_case("config.worktree")
    });
    named
        && real.parent().is_some_and(|dir| {
            dir.components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
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

/// The path a read ask shows: `input` resolved against `origin` and `home` by
/// text alone, the same way [`resolve`] does before it checks the disk. `None`
/// when the result would not read the same once it is sanitized for display
/// (too long, control, bidi or zero-width characters, a secret-looking run,
/// not UTF-8): the user must never approve a path that differs from the one
/// that is opened, so such a call is refused instead of asked.
pub fn display_target(home: &Path, origin: &Path, input: &str) -> Option<String> {
    let expanded = expand_tilde(home, input)?;
    let full = if expanded.is_absolute() {
        expanded
    } else {
        origin.join(expanded)
    };
    let text = normalize(&full).to_str()?.to_owned();
    let plain = crate::core::ai::tools::summary::clip(&text) == text;
    let secret = crate::core::ai::memory::redact::redact(&text).hits > 0;
    (plain && !secret).then_some(text)
}

/// The root-relative path `input` names by text alone, when the approval label
/// shows it exactly as it is used. `None` for an empty, absolute or `~` path,
/// any `..`, and any text the label would change (controls, zero-width, bidi
/// or other invisible characters, runs of whitespace, a secret-looking run). A `.` part is
/// dropped, so `.` gives an empty path.
pub fn plain_relative(input: &str) -> Option<PathBuf> {
    let label = crate::core::ai::sanitize::clean(input)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if input.is_empty()
        || input.starts_with('~')
        || label != input
        || input.chars().any(super::content::is_hidden)
    {
        return None;
    }
    let mut out = PathBuf::new();
    for part in Path::new(input).components() {
        match part {
            Component::CurDir => {}
            Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
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

    /// Whether an already resolved path (no symlinks, no `.` or `..`) may be
    /// opened or listed: inside HOME, not excluded by name or position, and
    /// not (under) the real target of a secret root of this snapshot. Pure.
    /// The runtime runs it on the real path of an opened file descriptor.
    pub fn judge_real(&self, real: &Path) -> bool {
        matches!(check_path(&self.real, real), PathVerdict::Allowed(_)) && !self.under_root(real)
    }

    fn under_root(&self, real: &Path) -> bool {
        self.roots.iter().any(|root| real.starts_with(root))
    }

    /// [`judge_real`](Self::judge_real) for the safe commands: outside HOME is
    /// fine for an existing target as long as it is not (under) a secret root.
    fn judge_exec(&self, real: &Path, want: Want) -> bool {
        if real.starts_with(&self.real) {
            self.judge_real(real)
        } else {
            want == Want::Existing && !self.under_root(real)
        }
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
    let Some(full) = locate(roots, origin, input) else {
        return PathVerdict::Refused;
    };
    if !home_rest(roots, &normalize(&full)).is_some_and(judge_rest) {
        return PathVerdict::Refused;
    }
    match full.canonicalize() {
        Ok(real) if roots.judge_real(&real) => PathVerdict::Allowed(real),
        _ => PathVerdict::Refused,
    }
}

/// `input` with `~` expanded and a relative path joined to `origin`. `None`
/// for a relative `origin` (`canonicalize` would depend on the process cwd)
/// or an unsupported `~user`.
fn locate(roots: &SecretRoots, origin: &Path, input: &str) -> Option<PathBuf> {
    if !origin.is_absolute() {
        return None;
    }
    let expanded = expand_tilde(&roots.real, input)?;
    Some(if expanded.is_absolute() {
        expanded
    } else {
        origin.join(expanded)
    })
}

/// The part of a requested path below HOME, whether the path names HOME as
/// given (a link) or as resolved. `None` when it is not under HOME.
fn home_rest<'a>(roots: &SecretRoots, requested: &'a Path) -> Option<&'a Path> {
    [&roots.given, &roots.real]
        .iter()
        .find_map(|h| requested.strip_prefix(h).ok())
}

/// What a path handed to [`resolve_exec`] is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// It must exist (`cd`, `ls`, `pwd`); outside HOME is fine.
    Existing,
    /// It must exist and be inside HOME (`tree`, which reads names below).
    Home,
    /// It will be created (`mkdir`): it must be inside HOME.
    New,
}

/// The path policy of the safe commands that may run unprompted. Like
/// [`resolve`] it judges the requested and the real path, but paths outside
/// HOME are allowed for [`Want::Existing`] unless they are (under) a secret
/// root. A [`Want::Home`] path gets the read rules plus "inside HOME". A
/// [`Want::New`] path is judged by its deepest existing ancestor,
/// canonicalized, plus the names that do not exist yet, and must be inside
/// HOME. A dangling link is refused. `cwd` must be the shell's real directory.
pub fn resolve_exec(roots: &SecretRoots, cwd: &Path, input: &str, want: Want) -> PathVerdict {
    let Some(full) = locate(roots, cwd, input) else {
        return PathVerdict::Refused;
    };
    let requested = normalize(&full);
    let lexical = match home_rest(roots, &requested) {
        Some(rest) => judge_rest(rest),
        None => want == Want::Existing && !roots.under_root(&requested),
    };
    let real = match want {
        Want::Existing | Want::Home => full.canonicalize().ok(),
        Want::New => canonical_new(&full),
    };
    match real {
        Some(real) if lexical && roots.judge_exec(&real, want) => PathVerdict::Allowed(real),
        _ => PathVerdict::Refused,
    }
}

/// `path` with its deepest existing ancestor canonicalized and the missing
/// names appended. `None` when something exists but cannot be resolved (a
/// dangling link, a permission error) or a missing name is not a plain name.
fn canonical_new(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match current.canonicalize() {
            Ok(real) => return Some(missing.iter().rev().fold(real, |p, name| p.join(name))),
            Err(e) if e.kind() == ErrorKind::NotFound && current.symlink_metadata().is_err() => {
                missing.push(current.file_name()?);
                current = current.parent()?;
            }
            Err(_) => return None,
        }
    }
}

/// Folder names a write never enters, at any depth below HOME.
const WRITE_DENIED_DIRS: &[&str] = &[".git", "node_modules"];

/// File names that run code when the project is built or tested: a change
/// carries a warning in the review. Lower case, compared ASCII
/// case-insensitively.
const BUILD_FILES: &[&str] = &[
    "makefile",
    "build.rs",
    "cargo.toml",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "conftest.py",
    "go.mod",
    "justfile",
    "rakefile",
    "tox.ini",
    "noxfile.py",
    "cmakelists.txt",
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    ".pre-commit-config.yaml",
    ".gitlab-ci.yml",
    ".gitattributes",
    ".gitmodules",
    // Read by the user's shell at login.
    ".bashrc",
    ".zshrc",
    ".profile",
    ".bash_profile",
];

/// Folders whose every file runs on a git event or when the project opens.
const BUILD_DIRS: &[&str] = &[".husky", ".githooks", ".devcontainer"];

/// Where a write may land: the real path, and whether it is a build file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WriteTarget {
    pub real: PathBuf,
    pub build_file: bool,
}

/// Whether the root-relative path `rel` is a build file (tier B): one of
/// [`BUILD_FILES`], `Dockerfile.*`, `settings.gradle*`, `*.sh`, `*.ps1`,
/// `*.bat`, `*.cmd`, a file below one of [`BUILD_DIRS`], `.vscode/tasks.json`,
/// `.vscode/launch.json`, `.cargo/config*` or `.github/workflows/*`.
fn is_build_file(rel: &Path) -> bool {
    let names: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    let Some((name, dirs)) = names.split_last() else {
        return false;
    };
    BUILD_FILES.contains(&name.as_str())
        || name.starts_with("dockerfile.")
        || name.starts_with("settings.gradle")
        || [".sh", ".ps1", ".bat", ".cmd"]
            .iter()
            .any(|ext| name.ends_with(ext))
        || dirs.iter().any(|d| BUILD_DIRS.contains(&d.as_str()))
        || (dirs.last().is_some_and(|d| d == ".vscode")
            && (name == "tasks.json" || name == "launch.json"))
        || (dirs.last().is_some_and(|d| d == ".cargo") && name.starts_with("config"))
        || dirs.ends_with(&[".github".to_owned(), "workflows".to_owned()])
}

/// The path policy of the write tools. `origin` is the root; `input` is a
/// path below it. Allowed only when:
/// - the root is a real folder strictly inside HOME and not secret (HOME, a
///   parent of HOME, a system folder, a missing or outside root all refuse);
/// - `input` is a plain relative path that the review shows exactly as it is
///   written (see [`plain_relative`]), in its canonical text form: no `.`
///   part, empty part or trailing slash, and no secret-looking run;
/// - the real target is the root joined to that text, so no link sits on the
///   way, in or out of the root, and nothing below HOME is excluded by name,
///   position or secret root, nor is a `.git` or `node_modules` folder (an
///   env template such as `.env.example` is the one name let through);
/// - the target itself, and any folder above it, may not exist yet.
///
/// Every refusal is the same `None` (the text is [`REFUSED`]). It reads the
/// disk but writes nothing.
pub fn resolve_write(roots: &SecretRoots, origin: &Path, input: &str) -> Option<WriteTarget> {
    if !origin.is_absolute() {
        return None;
    }
    let root = origin.canonicalize().ok()?;
    if root == roots.real || !root.is_dir() || !roots.judge_real(&root) {
        return None;
    }
    let rel = plain_relative(input)?;
    // A secret-looking run would show redacted in the review.
    if rel.to_str() != Some(input) || crate::core::ai::memory::redact::redact(input).hits > 0 {
        return None;
    }
    let shown = root.join(&rel);
    let real = canonical_new(&shown)?;
    // An env template is judged by the folder it sits in: only its own name
    // is let through.
    let template = real
        .file_name()
        .is_some_and(|name| is_env_template(&name.to_string_lossy()));
    let judged = if template { real.parent()? } else { &real };
    if real != shown || !roots.judge_real(judged) {
        return None;
    }
    let below_home = real.strip_prefix(&roots.real).ok()?;
    // A dot folder of HOME (`.config`, `.local`) or `dotfiles` holds files
    // that run at login or on start: the review carries the tier-B warning.
    let dotfiles = below_home.components().next().is_some_and(|c| {
        let name = c.as_os_str().to_string_lossy();
        name.starts_with('.') || name.eq_ignore_ascii_case("dotfiles")
    });
    let denied = below_home.components().any(|c| {
        WRITE_DENIED_DIRS
            .iter()
            .any(|dir| c.as_os_str().eq_ignore_ascii_case(dir))
    });
    (!denied).then(|| WriteTarget {
        build_file: dotfiles || is_build_file(&rel),
        real,
    })
}

#[cfg(test)]
mod display_tests {
    use super::*;

    const HOME: &str = "/home/u";
    const ORIGIN: &str = "/home/u/dev/app";

    fn shown(input: &str) -> Option<String> {
        display_target(Path::new(HOME), Path::new(ORIGIN), input)
    }

    #[test]
    fn a_relative_path_is_shown_resolved_against_the_origin() {
        assert_eq!(
            shown("src/lib.rs").as_deref(),
            Some("/home/u/dev/app/src/lib.rs")
        );
        assert_eq!(shown("../x/./y").as_deref(), Some("/home/u/dev/x/y"));
    }

    #[test]
    fn tilde_and_absolute_paths_are_shown_expanded() {
        assert_eq!(shown("~/a.txt").as_deref(), Some("/home/u/a.txt"));
        assert_eq!(shown("~").as_deref(), Some("/home/u"));
        assert_eq!(shown("/home/u/b").as_deref(), Some("/home/u/b"));
    }

    #[test]
    fn a_path_that_display_would_change_is_not_shown() {
        let long = format!("~/{}", "a".repeat(300));
        for input in [
            long.as_str(),
            "~/a\u{202e}txt.exe",
            "~/a\u{200b}b",
            "~/line\nbreak",
            "~/tab\there",
            "~/esc\u{1b}[31mx",
            "~/sk-abcdefghijklmnopqrstuvwxyz0123456789ABCD",
            "~user/x",
        ] {
            assert_eq!(shown(input), None, "{input:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn git_config_is_a_config_file_below_a_dot_git_folder() {
        for yes in [
            "/h/p/.git/config",
            "/h/p/.git/config.worktree",
            "/h/p/.git/modules/sub/config",
            "/h/p/.GIT/CONFIG",
        ] {
            assert!(is_git_config(Path::new(yes)), "{yes}");
        }
        for no in [
            "/h/p/config",
            "/h/p/.git/HEAD",
            "/h/p/.git",
            "/h/p/.git/config.bak",
            "/h/p/.github/config",
            "/h/p/.git/config/x",
        ] {
            assert!(!is_git_config(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn a_plain_relative_path_keeps_its_parts_and_drops_dots() {
        assert_eq!(plain_relative("src/a.rs"), Some(PathBuf::from("src/a.rs")));
        assert_eq!(plain_relative("./src/./a"), Some(PathBuf::from("src/a")));
        assert_eq!(plain_relative("."), Some(PathBuf::new()));
    }

    #[test]
    fn invisible_filler_and_blank_characters_are_not_plain() {
        for code in [
            0x034fu32, 0x115f, 0x1160, 0x180e, 0x2800, 0x3164, 0xfe00, 0xfe0f, 0xffa0, 0xfff9,
            0xfffb, 0xe0100, 0xe01ef,
        ] {
            let input = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(plain_relative(&input), None, "U+{code:04X}");
        }
        assert_eq!(
            plain_relative("a\u{3165}b"),
            Some(PathBuf::from("a\u{3165}b"))
        );
    }

    #[test]
    fn a_path_the_label_would_change_is_not_plain() {
        for bad in [
            "",
            "~",
            "~/x",
            "/etc/x",
            "../x",
            "a/../b",
            "a\nb",
            "a  b",
            "a\u{200b}b",
            "a\u{202e}b",
            "a\u{1b}[0mb",
        ] {
            assert_eq!(plain_relative(bad), None, "{bad:?}");
        }
    }

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
    fn a_leftover_write_temp_name_is_excluded_and_nothing_else_is() {
        let temp = ".lbx-0123456789abcdef0123456789abcdef.tmp";
        assert!(is_temp_name(temp) && is_excluded_name(temp));
        for other in [
            ".lbx-0123456789abcdef0123456789abcde.tmp",
            ".lbx-0123456789ABCDEF0123456789abcdef.tmp",
            ".lbx-0123456789abcdef0123456789abcdeg.tmp",
            "lbx-0123456789abcdef0123456789abcdef.tmp",
            ".lbx-0123456789abcdef0123456789abcdef.tmp2",
            "a.tmp",
        ] {
            assert!(!is_temp_name(other), "{other}");
        }
        let f = fixture();
        assert_eq!(
            check_path(&f.home, &f.home.join("project").join(temp)),
            PathVerdict::Refused
        );
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

    #[test]
    fn judge_real_applies_name_position_and_root_targets() {
        let f = fixture();
        let store = f.home.join("custom/store");
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join("chat.db"), "x").unwrap();
        let roots = SecretRoots::load(&f.home, std::slice::from_ref(&store)).unwrap();
        assert!(roots.judge_real(&f.home.join("project/src/lib.rs")));
        assert!(!roots.judge_real(&f.home.join(".ssh/id")));
        assert!(!roots.judge_real(&f.home.join("deep/er/.env")));
        assert!(!roots.judge_real(&store.join("chat.db")));
        assert!(!roots.judge_real(&f.outside.join("note.txt")));
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        outside: PathBuf,
        roots: SecretRoots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (home, outside) = (root.join("home"), root.join("srv"));
        for sub in [
            "home/project/src",
            "home/dotfiles/ssh",
            "home/.ssh",
            "home/.aws",
            "home/.config/gcloud",
            "srv/data",
        ] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        symlink(home.join("dotfiles/ssh"), home.join(".ssh/keys")).unwrap();
        fs::write(home.join(".ssh/id"), "x").unwrap();
        symlink(home.join(".ssh"), home.join("project/s")).unwrap();
        symlink(&outside, home.join("project/out")).unwrap();
        symlink(home.join(".ssh/nope"), home.join("project/dangling")).unwrap();
        let roots = SecretRoots::load(&home, &[]).unwrap();
        Fixture {
            _dir: dir,
            home,
            outside,
            roots,
        }
    }

    fn exec(f: &Fixture, cwd: &str, input: &str, want: Want) -> PathVerdict {
        resolve_exec(&f.roots, &f.home.join(cwd), input, want)
    }

    fn ok(f: &Fixture, cwd: &str, input: &str, want: Want) -> bool {
        matches!(exec(f, cwd, input, want), PathVerdict::Allowed(_))
    }

    #[test]
    fn secret_roots_are_refused_for_every_input_shape() {
        let f = fixture();
        for input in ["~/.ssh", "~/.aws", ".ssh", "../.ssh", "~/.config/gcloud"] {
            assert!(!ok(&f, "project", input, Want::Existing), "{input}");
        }
        assert!(!ok(&f, "project", "~/.aws/x", Want::New));
        assert!(!ok(&f, ".", ".ssh", Want::Existing));
    }

    #[test]
    fn a_link_into_a_secret_root_is_refused_on_the_real_path() {
        let f = fixture();
        assert!(!ok(&f, "project", "s", Want::Existing));
        assert!(!ok(&f, "project", "s/id", Want::Existing));
        assert!(!ok(&f, "project", "s/x", Want::New));
    }

    #[test]
    fn dot_dot_is_resolved_and_ordinary_paths_pass() {
        let f = fixture();
        assert_eq!(
            exec(&f, "project/src", "..", Want::Existing),
            PathVerdict::Allowed(f.home.join("project"))
        );
        assert!(ok(&f, "project", "~", Want::Existing));
        assert!(ok(&f, "project", ".", Want::Existing));
        assert!(ok(&f, "project", "src", Want::Existing));
    }

    #[test]
    fn outside_home_is_allowed_for_existing_unless_it_is_missing() {
        let f = fixture();
        let data = f.outside.join("data");
        assert!(ok(&f, "project", data.to_str().unwrap(), Want::Existing));
        assert!(ok(&f, "project", "out/data", Want::Existing));
        assert!(!ok(&f, "project", "/srv/missing-for-sure", Want::Existing));
        assert!(!ok(&f, "project", "nope", Want::Existing));
    }

    #[test]
    fn home_is_existing_but_never_outside_home() {
        let f = fixture();
        let data = f.outside.join("data");
        assert!(!ok(&f, "project", data.to_str().unwrap(), Want::Home));
        assert!(!ok(&f, "project", "out/data", Want::Home));
        assert!(!ok(&f, "project", "/nope-for-sure", Want::Home));
        assert!(!ok(&f, "project", "nope", Want::Home));
        assert!(!ok(&f, "project", "~/.ssh", Want::Home));
        assert!(!ok(&f, "project", "s", Want::Home));
        assert_eq!(
            exec(&f, "project", "src", Want::Home),
            PathVerdict::Allowed(f.home.join("project/src"))
        );
        assert!(ok(&f, "project", "~", Want::Home));
        // A cwd outside HOME is outside HOME for the verbs that act on it.
        assert_eq!(
            resolve_exec(&f.roots, &data, ".", Want::Home),
            PathVerdict::Refused
        );
    }

    #[test]
    fn a_link_outside_home_into_a_secret_root_is_refused_for_existing() {
        let f = fixture();
        let link = f.outside.join("to-ssh");
        symlink(f.home.join(".ssh"), &link).unwrap();
        let (cwd, link) = (f.home.join("project"), link.to_str().unwrap());
        for input in [link, &format!("{link}/id")] {
            let verdict = resolve_exec(&f.roots, &cwd, input, Want::Existing);
            assert_eq!(verdict, PathVerdict::Refused, "{input}");
        }
    }

    #[test]
    fn new_must_stay_inside_home_and_resolves_the_deepest_ancestor() {
        let f = fixture();
        assert_eq!(
            exec(&f, "project", "a/b/c", Want::New),
            PathVerdict::Allowed(f.home.join("project/a/b/c"))
        );
        assert_eq!(
            exec(&f, "project", "src", Want::New),
            PathVerdict::Allowed(f.home.join("project/src"))
        );
        let tmp = f.outside.join("x");
        assert!(!ok(&f, "project", tmp.to_str().unwrap(), Want::New));
        assert!(!ok(&f, "project", "out/x", Want::New));
        assert!(!ok(&f, "project", "dangling/x", Want::New));
        assert!(!ok(&f, "project", "dangling", Want::New));
    }

    #[test]
    fn a_secret_root_outside_home_refuses_what_is_under_it() {
        let f = fixture();
        let secret = f.outside.join("data");
        let roots = SecretRoots::load(&f.home, std::slice::from_ref(&secret)).unwrap();
        let cwd = f.home.join("project");
        for input in [secret.to_str().unwrap(), "out/data"] {
            let verdict = resolve_exec(&roots, &cwd, input, Want::Existing);
            assert_eq!(verdict, PathVerdict::Refused, "{input}");
        }
        let sibling = f.outside.to_str().unwrap();
        assert!(matches!(
            resolve_exec(&roots, &cwd, sibling, Want::Existing),
            PathVerdict::Allowed(_)
        ));
    }

    #[test]
    fn a_home_reached_through_a_link_is_judged_by_both_names() {
        let f = fixture();
        let link = f.home.parent().unwrap().join("homelink");
        symlink(&f.home, &link).unwrap();
        let roots = SecretRoots::load(&link, &[]).unwrap();
        assert_ne!(roots.given, roots.real);
        let go = |cwd: &str, input: &str, want| resolve_exec(&roots, &link.join(cwd), input, want);
        assert_eq!(
            go("project", "src", Want::Existing),
            PathVerdict::Allowed(f.home.join("project/src"))
        );
        assert_eq!(
            go("project", "a/b", Want::New),
            PathVerdict::Allowed(f.home.join("project/a/b"))
        );
        assert_eq!(go("project", ".ssh", Want::Existing), PathVerdict::Refused);
        assert_eq!(
            go("project", "../.ssh", Want::Existing),
            PathVerdict::Refused
        );
        assert_eq!(go(".ssh", ".", Want::Existing), PathVerdict::Refused);
        assert_eq!(go("project", "~/.aws/x", Want::New), PathVerdict::Refused);
    }

    #[test]
    fn a_relative_cwd_or_an_unreadable_home_form_refuses() {
        let f = fixture();
        let verdict = resolve_exec(&f.roots, Path::new("project"), ".", Want::Existing);
        assert_eq!(verdict, PathVerdict::Refused);
        assert!(!ok(&f, "project", "~root", Want::Existing));
        assert_eq!(SecretRoots::load(Path::new("relative"), &[]), None);
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    /// A temp `$HOME` holding a project, canonical like the real one.
    struct Fixture {
        _dir: tempfile::TempDir,
        top: PathBuf,
        home: PathBuf,
        origin: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().canonicalize().unwrap();
        let home = top.join("home");
        let origin = home.join("project");
        let outside = top.join("outside");
        for sub in ["home/project/src", "home/.ssh", "outside"] {
            fs::create_dir_all(top.join(sub)).unwrap();
        }
        fs::write(origin.join("src/lib.rs"), "x").unwrap();
        fs::write(origin.join("notes.txt"), "x").unwrap();
        fs::write(outside.join("note.txt"), "x").unwrap();
        symlink(&outside, origin.join("out_link")).unwrap();
        symlink(origin.join("src"), origin.join("in_link")).unwrap();
        symlink(origin.join("notes.txt"), origin.join("file_link")).unwrap();
        symlink(origin.join("missing"), origin.join("dangling")).unwrap();
        Fixture {
            _dir: dir,
            top,
            home,
            origin,
            outside,
        }
    }

    fn write(f: &Fixture, origin: &Path, input: &str) -> Option<WriteTarget> {
        let roots = SecretRoots::load(&f.home, &[])?;
        resolve_write(&roots, origin, input)
    }

    fn ok(f: &Fixture, input: &str) -> Option<WriteTarget> {
        write(f, &f.origin, input)
    }

    #[test]
    fn a_file_in_a_folder_that_may_not_exist_yet_resolves_below_the_root() {
        let f = fixture();
        for (input, want) in [
            ("src/lib.rs", "src/lib.rs"),
            ("notes.txt", "notes.txt"),
            ("new.rs", "new.rs"),
            ("src/util/deep/mod.rs", "src/util/deep/mod.rs"),
            ("fresh/dir", "fresh/dir"),
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert_eq!(target.real, f.origin.join(want), "{input}");
        }
    }

    #[test]
    fn nothing_is_created_by_resolving() {
        let f = fixture();
        assert!(ok(&f, "src/util/new.rs").is_some());
        assert!(!f.origin.join("src/util").exists());
    }

    #[test]
    fn the_root_may_not_be_home_a_parent_of_home_or_a_system_folder() {
        let f = fixture();
        let missing = f.origin.join("missing");
        for origin in [
            f.home.clone(),
            f.top.clone(),
            f.outside.clone(),
            PathBuf::from("/"),
            PathBuf::from("/home"),
            PathBuf::from("/usr"),
            PathBuf::from("/etc"),
            missing,
            f.origin.join("notes.txt"),
            f.home.join(".ssh"),
            PathBuf::from("project"),
        ] {
            assert_eq!(write(&f, &origin, "x.txt"), None, "{origin:?}");
        }
        assert!(write(&f, &f.origin.join("src"), "x.txt").is_some());
    }

    #[test]
    fn a_target_outside_the_root_is_refused_however_it_gets_there() {
        let f = fixture();
        for input in [
            "../x",
            "src/../../x",
            "/etc/x",
            "/tmp/x",
            "~/x",
            "~",
            &format!("{}/x", f.origin.display()),
            "out_link/x",
            "out_link/note.txt",
            "out_link",
        ] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
    }

    #[test]
    fn a_link_anywhere_on_the_way_is_refused_even_when_it_stays_inside() {
        let f = fixture();
        for input in ["in_link/new.rs", "in_link/lib.rs", "file_link", "dangling"] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
    }

    #[test]
    fn a_path_the_label_cannot_show_exactly_is_refused() {
        let f = fixture();
        for input in [
            "",
            ".",
            "./",
            "./new.rs",
            "src/",
            "src//new.rs",
            "src/./new.rs",
            "a\nb",
            "a\tb",
            "a  b",
            " a",
            "a\u{200b}b",
            "a\u{202e}b",
            "a\u{1b}[31mb",
            "a\u{0}b",
            "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
        ] {
            assert_eq!(ok(&f, input), None, "{input:?}");
        }
    }

    #[test]
    fn git_and_node_modules_are_refused_at_any_depth_and_case() {
        let f = fixture();
        for input in [
            ".git/hooks/pre-commit",
            ".git/config",
            "src/.git/x",
            ".GIT/x",
            ".git",
            "node_modules/x/index.js",
            "web/node_modules/x.js",
            "Node_Modules/x.js",
            "node_modules",
        ] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
        // Look-alikes are ordinary names.
        for input in [
            ".github/ci.yml",
            ".gitignore",
            "my.git/x",
            "node_modules2/x",
        ] {
            assert!(ok(&f, input).is_some(), "{input}");
        }
    }

    #[test]
    fn secret_names_are_refused_at_any_depth() {
        let f = fixture();
        for input in [
            ".env",
            ".env.local",
            "src/.env",
            ".ENV",
            "a/.env.d/x",
            ".npmrc",
            "web/.npmrc",
            ".netrc",
            ".git-credentials",
            ".envrc",
            ".bash_history",
            "docs/.pgpass",
        ] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
    }

    #[test]
    fn a_root_inside_a_secret_folder_writes_nothing() {
        let f = fixture();
        let nested = f.home.join(".ssh/keys");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(write(&f, &nested, "x.txt"), None);
    }

    #[test]
    fn every_refusal_is_none_for_its_own_cause() {
        let f = fixture();
        fs::write(f.origin.join(".env"), "x").unwrap();
        // An existing secret name, a missing secret name and an escaping path.
        for input in [
            ".env",
            "missing/.env",
            "../missing/x",
            "../../outside/note.txt",
        ] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
        assert_eq!(REFUSED, "path not available");
    }

    #[test]
    fn only_a_home_that_is_not_a_system_folder_can_judge_a_write() {
        // The system-folder rule lives in `SecretRoots::load`; `resolve_write`
        // holds no copy of it, and the private fields cannot build a roots
        // value for `/usr` that `load` would refuse. So the rule is tested
        // where it sits: no roots, no write, whatever the root folder is.
        for home in ["/usr", "/tmp", "/etc", "/home", "/"] {
            assert_eq!(SecretRoots::load(Path::new(home), &[]), None, "{home}");
        }
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        for origin in ["/usr/bin", "/tmp", "/etc"] {
            assert_eq!(
                resolve_write(&roots, Path::new(origin), "x.txt"),
                None,
                "{origin}"
            );
        }
    }

    #[test]
    fn invisible_characters_in_a_name_are_refused() {
        let f = fixture();
        for code in [
            0x034fu32, 0x115f, 0x1160, 0x180e, 0x2800, 0x3164, 0xfe0f, 0xffa0, 0xfff9, 0xe0100,
        ] {
            let input = format!("src/a{}b.rs", char::from_u32(code).unwrap());
            assert_eq!(ok(&f, &input), None, "U+{code:04X}");
        }
    }

    /// A fixture with a root folder `rel` directly below HOME.
    fn root_in_home(f: &Fixture, rel: &str) -> PathBuf {
        let root = f.home.join(rel);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_root_under_a_dot_folder_of_home_or_dotfiles_carries_the_warning() {
        let f = fixture();
        for rel in [
            ".config/nvim",
            ".config/autostart",
            ".config/systemd/user",
            ".local/bin",
            ".config/fish",
            // Not `.CONFIG`: on a case-insensitive file system (macOS) that
            // is the `.config` folder above, and its real path differs.
            ".CACHE/x",
            "dotfiles",
        ] {
            let root = root_in_home(&f, rel);
            let target = write(&f, &root, "init.lua").unwrap_or_else(|| panic!("{rel} refused"));
            assert!(target.build_file, "{rel}");
            assert_eq!(target.real, root.join("init.lua"), "{rel}");
        }
        // The letter case of `dotfiles` does not matter. Its own fixture:
        // on a case-insensitive file system it is the `dotfiles` folder above.
        let g = fixture();
        let root = root_in_home(&g, "Dotfiles/zsh");
        let target = write(&g, &root, "init.lua").expect("Dotfiles/zsh refused");
        assert!(target.build_file);
        assert_eq!(target.real, root.join("init.lua"));
        // A root that is not in a dot folder or `dotfiles` is not flagged.
        for rel in ["dev/.config/app", "dotfiles2", "my.dotfiles"] {
            let root = root_in_home(&f, rel);
            assert!(!write(&f, &root, "init.lua").unwrap().build_file, "{rel}");
        }
        assert!(!ok(&f, "src/new.rs").unwrap().build_file);
    }

    #[test]
    fn env_templates_are_writable_but_real_env_files_are_not() {
        let f = fixture();
        for input in [
            ".env.example",
            ".env.local.sample",
            "web/.env.template",
            ".ENV.DIST",
            ".env.production.example",
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert!(!target.build_file, "{input}");
        }
        for input in [
            ".env",
            ".env.local",
            ".env.example.local",
            ".env.examples",
            ".env.example/x",
            ".env.d/x.example",
            "web/.env",
            ".envrc",
        ] {
            assert_eq!(ok(&f, input), None, "{input}");
        }
    }

    #[test]
    fn the_tier_b_list_covers_hooks_ci_containers_and_build_tools() {
        let f = fixture();
        for input in [
            ".husky/pre-commit",
            ".husky/_/husky.sh",
            ".githooks/pre-push",
            ".pre-commit-config.yaml",
            ".vscode/tasks.json",
            ".vscode/launch.json",
            ".devcontainer/devcontainer.json",
            ".devcontainer/sub/Dockerfile",
            "Dockerfile",
            "docker/Dockerfile.dev",
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
            "justfile",
            "Justfile",
            "Rakefile",
            "tox.ini",
            "noxfile.py",
            "CMakeLists.txt",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "pom.xml",
            ".gitlab-ci.yml",
            ".gitattributes",
            ".gitmodules",
            ".bashrc",
            ".zshrc",
            ".profile",
            ".bash_profile",
            "tools/setup.ps1",
            "run.bat",
            "RUN.CMD",
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert!(target.build_file, "{input}");
        }
        for input in [
            ".vscode/settings.json",
            ".vscode/extensions.json",
            "tasks.json",
            ".husky",
            "Dockerfile2",
            "dockerfile_notes.md",
            "compose.txt",
            "pom.xml.bak",
            "tox.ini.md",
            "build.gradle.md",
            "settings.grad",
            "run.bat.txt",
            "notes.cmdx",
            "profile",
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert!(!target.build_file, "{input}");
        }
    }

    #[test]
    fn build_files_carry_a_warning_and_other_files_do_not() {
        let f = fixture();
        for input in [
            "Makefile",
            "sub/Makefile",
            "build.rs",
            "crates/a/build.rs",
            "Cargo.toml",
            "crates/a/Cargo.toml",
            ".cargo/config",
            ".cargo/config.toml",
            "package.json",
            "pyproject.toml",
            "setup.py",
            "tests/conftest.py",
            "go.mod",
            "run.sh",
            "scripts/deploy.sh",
            ".github/workflows/ci.yml",
            ".GitHub/Workflows/ci.yml",
            "MAKEFILE",
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert!(target.build_file, "{input}");
        }
        for input in [
            "src/main.rs",
            "Cargo.lock",
            "README.md",
            ".github/CODEOWNERS",
            "workflows/ci.yml",
            ".github/workflows",
            ".cargo/other",
            "cargo.toml.bak",
            "notes.shx",
            "build.rs.txt",
            "sh",
        ] {
            let target = ok(&f, input).unwrap_or_else(|| panic!("{input} refused"));
            assert!(!target.build_file, "{input}");
        }
    }
}
