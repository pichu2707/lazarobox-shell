//! The git verbs as fixed argv templates, and the parse of their output.
//! Pure: it spawns nothing and reads no file. The runner in the runtime owns
//! the jail and the process; this module only decides the words.
//!
//! Only status, diff, log, add and commit exist, and no verb takes a free-form
//! argument list. Every call carries the same `-c` hardening, and the secret
//! file names are excluded by pathspec so a tracked `.env` is never printed.

use std::path::{Component, Path};

use super::paths::{SECRET_DIRS, SECRET_FILES, is_excluded_name};
use crate::core::ai::jail::CREDENTIAL_DIRS;
use crate::core::ai::sanitize::is_format;

mod parse;

use parse::tidy;
pub use parse::{Commit, Entry, Kind, Status, parse_log, parse_status, render_log, render_status};

/// The most log entries a call returns, whatever the model asks for.
pub const MAX_LOG_COUNT: usize = 50;
/// The log entries returned when the model names no count.
pub const DEFAULT_LOG_COUNT: usize = 20;
/// The most status entries kept; the rest are counted.
pub const MAX_STATUS_ENTRIES: usize = 200;
/// The longest commit message, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// Most paths one `add` takes.
pub const MAX_ADD_PATHS: usize = 64;
/// The longest commit name or email, in bytes.
pub const MAX_IDENTITY_BYTES: usize = 256;

/// The settings in front of every verb. They turn off the config keys that
/// run a program, the pager, prompts and the network.
const HARDENING: [&str; 8] = [
    "core.fsmonitor=false",
    "core.untrackedCache=false",
    "core.pager=cat",
    "diff.external=",
    "protocol.allow=never",
    "credential.helper=",
    "core.sshCommand=false",
    "gpg.program=false",
];

/// Why an argument was refused. Short and content-free: it goes to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitError {
    /// A path that is empty, absolute, escapes the root, names `.git` or a
    /// secret file, or holds a NUL.
    Path,
    /// `add` with no path, or with more than [`MAX_ADD_PATHS`].
    PathCount,
    /// A commit message that is empty, too long, or holds a control or bidi
    /// character.
    Message,
    /// A git dir that is not an absolute, clean path.
    GitDir,
    /// A commit name or email that is empty, long, or holds a control or bidi
    /// character.
    Identity,
}

impl GitError {
    /// The text sent back to the model.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Path => "path not available",
            Self::PathCount => "give between 1 and 64 paths",
            Self::Message => "invalid commit message",
            Self::GitDir => "invalid git dir",
            Self::Identity => "invalid commit identity",
        }
    }
}

/// Whether text is one clean line: no control and no format character.
fn clean_line(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || is_format(c))
}

/// The git dirs of the repository, resolved by the runtime preflight
/// (`rev-parse --absolute-git-dir` and `--git-common-dir`). Hooks are pinned
/// to the `hooks` folder of the COMMON dir: that is the repo's own
/// `.git/hooks`, which the write tools refuse to touch, so a hooks folder the
/// model could write (husky) never runs. In a linked worktree the git dir is
/// `.git/worktrees/<name>`, which holds no hooks; the common dir does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GitDir {
    git: String,
    common: String,
}

impl GitDir {
    /// Both are absolute paths with no control or format character and no
    /// `..`, other than `/`. They are equal outside a linked worktree.
    pub fn new(git_dir: &str, common_dir: &str) -> Result<Self, GitError> {
        let clean = |path: &str| {
            path.starts_with('/')
                && path.len() > 1
                && clean_line(path)
                && !path.split('/').any(|part| part == "..")
        };
        if clean(git_dir) && clean(common_dir) {
            Ok(Self {
                git: git_dir.trim_end_matches('/').to_owned(),
                common: common_dir.trim_end_matches('/').to_owned(),
            })
        } else {
            Err(GitError::GitDir)
        }
    }

    fn hooks(&self) -> String {
        format!("{}/hooks", self.common)
    }
}

/// The commit author: the repo's effective identity, that is `git config
/// user.name` and `user.email` as the repo sees them (its `.git/config` with
/// the global config in scope). The runtime reads them in the preflight,
/// outside the jail, and passes them in; the config itself is never loaded
/// in the jail.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    name: String,
    email: String,
}

impl Identity {
    /// Both must be non-empty, at most [`MAX_IDENTITY_BYTES`], one clean line;
    /// the email holds an `@` and no whitespace.
    pub fn new(name: &str, email: &str) -> Result<Self, GitError> {
        let fits =
            |text: &str| !text.is_empty() && text.len() <= MAX_IDENTITY_BYTES && clean_line(text);
        let ok = fits(name)
            && fits(email)
            && email.contains('@')
            && !email.chars().any(char::is_whitespace);
        if ok {
            Ok(Self {
                name: name.to_owned(),
                email: email.to_owned(),
            })
        } else {
            Err(GitError::Identity)
        }
    }
}

/// The notice for the commit review when the repo sets `core.hooksPath`
/// elsewhere than its own hooks folder, or `None` when there is nothing to
/// say. The runtime (G-b) reads the configured value.
pub fn hooks_notice(dir: &GitDir, configured: &str) -> Option<String> {
    let configured = configured.trim_end_matches('/');
    if configured.is_empty() || configured == dir.hooks() {
        return None;
    }
    Some(format!(
        "this repo sets core.hooksPath to {}; lazarobox runs only the repo hooks folder",
        tidy(configured)
    ))
}

/// The `-c` hardening, the hooks pin and the pager switch: the start of every argv.
fn base(dir: &GitDir) -> Vec<String> {
    let mut argv = vec!["-c".to_owned(), format!("core.hooksPath={}", dir.hooks())];
    for setting in HARDENING {
        argv.push("-c".to_owned());
        argv.push(setting.to_owned());
    }
    argv.push("--no-pager".to_owned());
    argv
}

/// The environment of a call, as pairs. `read` adds `GIT_OPTIONAL_LOCKS=0`.
pub fn env(read: bool) -> Vec<(&'static str, &'static str)> {
    let mut pairs = vec![
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_SYSTEM", "/dev/null"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_ATTR_NOSYSTEM", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_EDITOR", "true"),
    ];
    if read {
        pairs.push(("GIT_OPTIONAL_LOCKS", "0"));
    }
    pairs
}

/// The `:(exclude,glob,icase)` pathspecs for the secret names: every
/// [`SECRET_FILES`] entry, `.env`, `.env.*` and the temp files, plus the
/// contents of every secret folder, at any depth. `icase` follows
/// the case-insensitive rule of the path guard.
pub fn excludes() -> Vec<String> {
    let mut specs: Vec<String> = [".env", ".env.*", ".lbx-*.tmp"]
        .iter()
        .chain(SECRET_FILES)
        .map(|name| format!(":(exclude,glob,icase)**/{name}"))
        .collect();
    specs.extend(
        SECRET_DIRS
            .iter()
            .chain(CREDENTIAL_DIRS)
            .map(|dir| format!(":(exclude,glob,icase)**/{dir}/**")),
    );
    specs.sort();
    specs
}

/// Checks a path given to a verb: relative, below the root, outside `.git`
/// and not a secret name. Pure: the runtime still resolves it.
pub(crate) fn check(path: &str) -> Result<(), GitError> {
    if path.is_empty() || !clean_line(path) {
        return Err(GitError::Path);
    }
    let mut names = Vec::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => names.push(name.to_string_lossy()),
            Component::CurDir => {}
            _ => return Err(GitError::Path),
        }
    }
    if names.iter().any(|name| name.eq_ignore_ascii_case(".git")) || secret_names(&names) {
        return Err(GitError::Path);
    }
    Ok(())
}

/// Whether the names hold a secret file name or a secret folder (ASCII
/// case-insensitive, component by component, at any depth).
fn secret_names<S: AsRef<str>>(names: &[S]) -> bool {
    names.iter().any(|name| is_excluded_name(name.as_ref()))
        || SECRET_DIRS.iter().chain(CREDENTIAL_DIRS).any(|root| {
            let parts: Vec<&str> = root.split('/').collect();
            names.windows(parts.len()).any(|window| {
                window
                    .iter()
                    .zip(&parts)
                    .all(|(name, part)| part.eq_ignore_ascii_case(name.as_ref()))
            })
        })
}

/// Whether a path from git output names a secret.
fn secret_path(path: &str) -> bool {
    let names: Vec<&str> = path.split('/').collect();
    secret_names(&names)
}

/// The pathspec of one path: literal, so `:` and glob characters are inert.
fn literal(path: &str) -> String {
    format!(":(literal){path}")
}

/// `git status`: parsed by [`parse_status`].
pub fn status_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--ignore-submodules=all",
            "--untracked-files=normal",
            "--renames",
            "--",
            ".",
        ]
        .map(str::to_owned),
    );
    argv.extend(excludes());
    argv
}

/// `git diff` of the working tree, or of the index when `staged`.
pub fn diff_argv(dir: &GitDir, staged: bool, path: Option<&str>) -> Result<Vec<String>, GitError> {
    let target = match path {
        Some(path) => {
            check(path)?;
            literal(path)
        }
        None => ".".to_owned(),
    };
    let mut argv = base(dir);
    argv.push("diff".to_owned());
    if staged {
        argv.push("--cached".to_owned());
    }
    argv.extend(
        [
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            "--no-color",
            "--",
        ]
        .map(str::to_owned),
    );
    argv.push(target);
    argv.extend(excludes());
    Ok(argv)
}

/// Clamps the requested log count to `1..=`[`MAX_LOG_COUNT`].
pub fn clamp_count(requested: Option<u64>) -> usize {
    match requested {
        None => DEFAULT_LOG_COUNT,
        Some(n) => usize::try_from(n)
            .unwrap_or(MAX_LOG_COUNT)
            .clamp(1, MAX_LOG_COUNT),
    }
}

/// `git log`: a fixed format, never a patch.
pub fn log_argv(
    dir: &GitDir,
    max_count: Option<u64>,
    path: Option<&str>,
) -> Result<Vec<String>, GitError> {
    let mut argv = base(dir);
    argv.extend([
        "log".to_owned(),
        "-n".to_owned(),
        clamp_count(max_count).to_string(),
        "--no-color".to_owned(),
        "--no-show-signature".to_owned(),
        "--format=%H%x1f%an%x1f%aI%x1f%s".to_owned(),
        "--".to_owned(),
    ]);
    if let Some(path) = path {
        check(path)?;
        argv.push(literal(path));
    }
    argv.extend(excludes());
    Ok(argv)
}

/// `git add` of literal paths.
pub fn add_argv(dir: &GitDir, paths: &[&str]) -> Result<Vec<String>, GitError> {
    add_with(dir, paths, false)
}

/// The same add as a dry run: it lists what would be staged and stages nothing.
pub fn add_dry_argv(dir: &GitDir, paths: &[&str]) -> Result<Vec<String>, GitError> {
    add_with(dir, paths, true)
}

/// Whether the paths of an add are allowed: between 1 and
/// [`MAX_ADD_PATHS`], each one inside the root and not a secret or `.git`.
pub fn check_add(paths: &[&str]) -> Result<(), GitError> {
    if paths.is_empty() || paths.len() > MAX_ADD_PATHS {
        return Err(GitError::PathCount);
    }
    paths.iter().try_for_each(|path| check(path))
}

fn add_with(dir: &GitDir, paths: &[&str], dry: bool) -> Result<Vec<String>, GitError> {
    check_add(paths)?;
    let mut argv = base(dir);
    argv.push("add".to_owned());
    if dry {
        argv.push("--dry-run".to_owned());
    }
    argv.push("--".to_owned());
    argv.extend(paths.iter().map(|path| literal(path)));
    // A folder or `.` would stage the secrets below it: exclude them too.
    argv.extend(excludes());
    Ok(argv)
}

/// The names the index would commit, NUL separated, with no secret exclude
/// and no rename pairing: a renamed secret shows under both names.
pub fn index_names_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(
        [
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            "--",
            ".",
        ]
        .map(str::to_owned),
    );
    argv
}

/// Whether the `-z` listing of [`index_names_argv`] holds a secret name or a
/// path inside a secret folder: the commit is then refused, so the review
/// (which cannot show a secret) stays the whole truth.
pub fn staged_has_secret(listing: &str) -> bool {
    listing
        .split('\0')
        .filter(|name| !name.is_empty())
        .any(secret_path)
}

/// The diff the commit review shows: the whole index against `HEAD`, with
/// nothing left out (no secret exclude, because [`staged_has_secret`] has
/// refused a secret already; `--ignore-submodules=none` beats a `.gitmodules`
/// `ignore = all`, and `--text` beats a `-diff` attribute, so neither hides a
/// change the commit records).
pub fn review_diff_argv(dir: &GitDir) -> Vec<String> {
    let mut argv = base(dir);
    argv.extend(
        [
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--text",
            "--ignore-submodules=none",
            "--no-color",
            "--",
            ".",
        ]
        .map(str::to_owned),
    );
    argv
}

/// Whether a commit message is allowed: not empty, bounded, and free of
/// control (a newline and a tab excepted) and bidi characters.
pub fn check_message(message: &str) -> Result<(), GitError> {
    let bad = message.trim().is_empty()
        || message.len() > MAX_MESSAGE_BYTES
        || message
            .chars()
            .any(|c| (c.is_control() && c != '\n' && c != '\t') || is_format(c));
    if bad { Err(GitError::Message) } else { Ok(()) }
}

/// `git commit` with signing off. No hook is skipped: they run in the jail.
pub fn commit_argv(dir: &GitDir, who: &Identity, message: &str) -> Result<Vec<String>, GitError> {
    check_message(message)?;
    let mut argv = base(dir);
    for setting in [
        "commit.gpgsign=false".to_owned(),
        format!("user.name={}", who.name),
        format!("user.email={}", who.email),
    ] {
        argv.push("-c".to_owned());
        argv.push(setting);
    }
    argv.extend(["commit", "--cleanup=whitespace", "-m"].map(str::to_owned));
    argv.push(message.to_owned());
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gd() -> GitDir {
        GitDir::new("/home/u/repo/.git", "/home/u/repo/.git").unwrap()
    }

    fn who() -> Identity {
        Identity::new("Ana Pérez", "ana@example.com").unwrap()
    }

    fn has(argv: &[String], word: &str) -> bool {
        argv.iter().any(|a| a == word)
    }

    fn pair(argv: &[String], key: &str) -> bool {
        argv.windows(2).any(|w| w[0] == "-c" && w[1] == key)
    }

    // Spec (git-tools, commit): the index is checked for secret paths, not
    // only the add arguments; a rename counts under both of its names.
    #[test]
    fn a_staged_secret_path_is_found_in_the_index_listing() {
        assert!(staged_has_secret(".env\0a.rs\0"));
        assert!(staged_has_secret("a.rs\0config/.ENV.local\0"));
        assert!(staged_has_secret("a.rs\0.ssh/id_rsa\0"));
        assert!(!staged_has_secret("a.rs\0src/env.rs\0"));
        assert!(!staged_has_secret(""));
        let argv = index_names_argv(&gd());
        for word in ["--cached", "--name-only", "-z", "--no-renames"] {
            assert!(has(&argv, word), "{word}");
        }
        // No exclude: the listing must show the secret it looks for.
        assert!(!argv.iter().any(|a| a.starts_with(":(exclude")), "{argv:?}");
    }

    // The review argv hardening: no external program, no textconv, no color.
    // What it must not hide is proved on a real repo (host git tests).
    #[test]
    fn the_commit_diff_has_no_program_and_every_verb_has_the_hardening() {
        let argv = review_diff_argv(&gd());
        for word in ["--cached", "--no-ext-diff", "--no-textconv", "--no-color"] {
            assert!(has(&argv, word), "{word}");
        }
        assert!(!argv.iter().any(|a| a.starts_with(":(exclude")), "{argv:?}");
        for setting in HARDENING {
            assert!(pair(&argv, setting), "{setting}");
        }
    }

    #[test]
    fn the_add_preview_is_the_add_with_a_dry_run() {
        let real = add_argv(&gd(), &["src", "a.rs"]).unwrap();
        let dry = add_dry_argv(&gd(), &["src", "a.rs"]).unwrap();
        assert!(!has(&real, "--dry-run") && has(&dry, "--dry-run"));
        let without: Vec<&String> = dry.iter().filter(|a| *a != "--dry-run").collect();
        assert_eq!(without, real.iter().collect::<Vec<_>>());
        assert_eq!(add_dry_argv(&gd(), &[".env"]), Err(GitError::Path));
    }

    #[test]
    fn every_verb_carries_the_hardening() {
        let all = [
            status_argv(&gd()),
            diff_argv(&gd(), false, None).unwrap(),
            log_argv(&gd(), None, None).unwrap(),
            add_argv(&gd(), &["a.rs"]).unwrap(),
            commit_argv(&gd(), &who(), "msg").unwrap(),
        ];
        for argv in &all {
            for setting in HARDENING {
                assert!(pair(argv, setting), "{setting} missing in {argv:?}");
            }
            assert!(has(argv, "--no-pager"));
            let verb = argv.iter().position(|a| a == "--no-pager").unwrap();
            assert!(argv[verb + 1..].iter().any(|a| !a.starts_with('-')));
        }
    }

    #[test]
    fn status_template() {
        let argv = status_argv(&gd());
        let at = argv.iter().position(|a| a == "status").unwrap();
        assert_eq!(
            argv[at..at + 9],
            [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--ignore-submodules=all",
                "--untracked-files=normal",
                "--renames",
                "--",
                "."
            ]
        );
        assert!(argv.iter().any(|a| a == ":(exclude,glob,icase)**/.env"));
    }

    #[test]
    fn excludes_cover_every_secret_name() {
        let specs = excludes();
        for name in SECRET_FILES {
            assert!(specs.contains(&format!(":(exclude,glob,icase)**/{name}")));
        }
        assert!(specs.contains(&":(exclude,glob,icase)**/.env.*".to_owned()));
        assert!(specs.iter().all(|s| s.starts_with(":(exclude,glob")));
    }

    #[test]
    fn diff_templates() {
        let wt = diff_argv(&gd(), false, None).unwrap();
        assert!(!has(&wt, "--cached"));
        assert!(has(&wt, "--no-ext-diff") && has(&wt, "--no-textconv") && has(&wt, "--no-color"));
        let sep = wt.iter().position(|a| a == "--").unwrap();
        assert_eq!(wt[sep + 1], ".");
        assert!(wt[sep + 2..].iter().all(|a| a.starts_with(":(exclude")));

        let staged = diff_argv(&gd(), true, Some("src/a.rs")).unwrap();
        assert!(has(&staged, "--cached"));
        assert!(has(&staged, ":(literal)src/a.rs"));
    }

    #[test]
    fn log_template_and_clamp() {
        let argv = log_argv(&gd(), Some(500), None).unwrap();
        let n = argv.iter().position(|a| a == "-n").unwrap();
        assert_eq!(argv[n + 1], "50");
        assert!(has(&argv, "--format=%H%x1f%an%x1f%aI%x1f%s"));
        assert!(!argv.iter().any(|a| a == "-p" || a == "--patch"));
        assert_eq!(clamp_count(None), 20);
        assert_eq!(clamp_count(Some(0)), 1);
        assert_eq!(clamp_count(Some(7)), 7);
        assert_eq!(clamp_count(Some(u64::MAX)), 50);
    }

    #[test]
    fn log_path_is_literal_and_excluded() {
        let argv = log_argv(&gd(), Some(3), Some("src")).unwrap();
        assert!(has(&argv, ":(literal)src"));
        assert!(has(&argv, ":(exclude,glob,icase)**/.env"));
    }

    #[test]
    fn add_is_literal_and_refuses_secrets() {
        let argv = add_argv(&gd(), &["a.rs", ":(top)x", "*.rs"]).unwrap();
        let sep = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[sep - 1], "add");
        assert_eq!(
            argv[sep + 1..sep + 4],
            [":(literal)a.rs", ":(literal):(top)x", ":(literal)*.rs"]
        );
        for bad in [
            ".env",
            "sub/.env.local",
            ".git/config",
            "a/.NPMRC",
            "",
            "/etc/x",
            "../x",
            "a\0b",
        ] {
            assert_eq!(
                add_argv(&gd(), &["ok", bad]),
                Err(GitError::Path),
                "{bad:?}"
            );
        }
        assert_eq!(add_argv(&gd(), &[]), Err(GitError::PathCount));
        let many = vec!["a"; MAX_ADD_PATHS + 1];
        assert_eq!(add_argv(&gd(), &many), Err(GitError::PathCount));
    }

    #[test]
    fn commit_disables_signing() {
        let argv = commit_argv(&gd(), &who(), "fix: a\n\nbody\twith tab").unwrap();
        assert!(pair(&argv, "commit.gpgsign=false"));
        let at = argv.iter().position(|a| a == "commit").unwrap();
        assert_eq!(argv[at + 1], "--cleanup=whitespace");
        assert_eq!(argv[at + 2], "-m");
        assert_eq!(argv[at + 3], "fix: a\n\nbody\twith tab");
        assert_eq!(argv.len(), at + 4);
        assert!(!has(&argv, "--no-verify"));
    }

    #[test]
    fn commit_message_rules() {
        for bad in ["", "  \n", "a\u{1b}[31m", "a\u{202e}b", "a\0b", "a\rb"] {
            assert_eq!(
                commit_argv(&gd(), &who(), bad),
                Err(GitError::Message),
                "{bad:?}"
            );
        }
        assert!(commit_argv(&gd(), &who(), &"x".repeat(MAX_MESSAGE_BYTES)).is_ok());
        assert!(commit_argv(&gd(), &who(), &"x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        assert!(commit_argv(&gd(), &who(), "-leading dash").is_ok());
    }

    #[test]
    fn env_for_read_and_write() {
        assert!(env(true).contains(&("GIT_OPTIONAL_LOCKS", "0")));
        assert!(!env(false).iter().any(|(k, _)| *k == "GIT_OPTIONAL_LOCKS"));
        for pair in [
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_EDITOR", "true"),
        ] {
            assert!(env(false).contains(&pair));
        }
    }

    #[test]
    fn no_other_verb_exists() {
        let verbs = [
            status_argv(&gd()),
            diff_argv(&gd(), true, None).unwrap(),
            log_argv(&gd(), None, None).unwrap(),
            add_argv(&gd(), &["a"]).unwrap(),
            commit_argv(&gd(), &who(), "m").unwrap(),
        ]
        .map(|argv| {
            let at = argv.iter().position(|a| a == "--no-pager").unwrap();
            let mut verb = at + 1;
            while argv[verb] == "-c" {
                verb += 2;
            }
            argv[verb].clone()
        });
        assert_eq!(verbs, ["status", "diff", "log", "add", "commit"]);
    }

    #[test]
    fn status_parses_porcelain_v2() {
        let raw = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/a b.rs\0\
1 A. N... 000000 100644 100644 000 ccc new.rs\0\
2 R. N... 100644 100644 100644 aaa bbb R100 moved.rs\0old.rs\0\
u UU N... 1 2 3 4 aaa bbb ccc conflict.rs\0\
? untracked.txt\0\
! ignored.log\0";
        let s = parse_status(raw);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        let got: Vec<_> = s
            .entries
            .iter()
            .map(|e| (e.kind, e.xy.as_str(), e.path.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Kind::Changed, ".M", "src/a b.rs"),
                (Kind::Changed, "A.", "new.rs"),
                (Kind::Renamed, "R.", "moved.rs"),
                (Kind::Unmerged, "UU", "conflict.rs"),
                (Kind::Untracked, "??", "untracked.txt"),
            ]
        );
        assert_eq!(s.entries[2].from.as_deref(), Some("old.rs"));
    }

    #[test]
    fn status_edge_cases() {
        assert_eq!(parse_status(""), Status::default());
        let s = parse_status("# branch.head (detached)\0garbage\x001 short\0");
        assert_eq!(s.branch.as_deref(), Some("(detached)"));
        assert!(s.entries.is_empty() && s.upstream.is_none());
        assert!(render_status(&s).ends_with("clean"));
    }

    #[test]
    fn status_is_bounded() {
        let raw: String = (0..MAX_STATUS_ENTRIES + 5)
            .map(|i| format!("? f{i}\0"))
            .collect();
        let s = parse_status(&raw);
        assert_eq!(s.entries.len(), MAX_STATUS_ENTRIES);
        assert_eq!(s.omitted, 5);
        assert!(render_status(&s).ends_with("[5 more entries not listed]"));
    }

    #[test]
    fn status_render() {
        let s = parse_status("# branch.head main\0? a\x002 R. N... 1 1 1 a b R100 n\0o\0");
        assert_eq!(render_status(&s), "branch main\n?? a\nR. n <- o");
    }

    #[test]
    fn log_parses_and_bounds() {
        let line = |i: usize| {
            format!(
                "{:040}\u{1f}Ana\u{1f}2026-01-01T00:00:00+00:00\u{1f}msg {i}\n",
                i
            )
        };
        let raw: String = (0..80).map(line).collect();
        let commits = parse_log(&raw);
        assert_eq!(commits.len(), MAX_LOG_COUNT);
        assert_eq!(commits[1].author, "Ana");
        assert_eq!(commits[1].subject, "msg 1");
        assert_eq!(commits[1].hash.len(), 40);
    }

    #[test]
    fn log_keeps_unit_separator_in_subject_and_skips_bad_lines() {
        let (h1, h2) = ("a".repeat(40), "B".repeat(64));
        let d = "2026-01-01T00:00:00+00:00";
        let raw = format!(
            "{h1}\u{1f}a\u{1f}{d}\u{1f}one\u{1f}two\nbroken line\n\n{h2}\u{1f}a\u{1f}{d}\u{1f}s\n"
        );
        let commits = parse_log(&raw);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "one\u{1f}two");
        assert_eq!(parse_log(""), []);
        assert_eq!(render_log(&[]), "no commits");
    }

    #[test]
    fn log_render() {
        let commits = parse_log(&format!(
            "{:040}\u{1f}Ana\u{1f}2026-01-01T00:00:00+00:00\u{1f}hi\n",
            1
        ));
        assert_eq!(
            render_log(&commits),
            "000000000000 2026-01-01T00:00:00+00:00 Ana hi"
        );
    }

    #[test]
    fn add_appends_the_secret_excludes_after_the_paths() {
        let argv = add_argv(&gd(), &["."]).unwrap();
        let ex = excludes();
        assert_eq!(argv[argv.len() - ex.len()..], ex[..]);
        assert_eq!(argv[argv.len() - ex.len() - 1], ":(literal).");
    }

    /// Runs git in `dir` with the argv of a verb; `None` when git is missing.
    fn git(dir: &std::path::Path, argv: &[String]) -> Option<String> {
        let out = std::process::Command::new("git")
            .args(argv)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn add_of_a_folder_stages_no_secret() {
        let dir = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        if git(dir.path(), &args(&["init", "-q"])).is_none() {
            return;
        }
        for file in [
            "ok.txt",
            ".env",
            "src/.ENV.local",
            "n/.env.prod",
            "x/.aws/creds",
            "y/.config/gh/hosts.yml",
            "z/.npmrc",
            "src/keep.rs",
        ] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        git(dir.path(), &add_argv(&gd(), &["."]).unwrap());
        let staged = git(dir.path(), &args(&["diff", "--cached", "--name-only"])).unwrap();
        let mut names: Vec<_> = staged.lines().collect();
        names.sort();
        assert_eq!(names, ["ok.txt", "src/keep.rs"]);
        git(dir.path(), &add_argv(&gd(), &["src"]).unwrap());
        let staged = git(dir.path(), &args(&["diff", "--cached", "--name-only"])).unwrap();
        assert!(!staged.contains(".ENV"));
    }

    #[test]
    fn extra_flags_and_environment() {
        let log = log_argv(&gd(), None, None).unwrap();
        assert!(has(&log, "--no-show-signature"));
        let sep = log
            .iter()
            .position(|a| a == "--")
            .expect("log always has --");
        assert_eq!(sep, log.len() - excludes().len() - 1);
        assert!(log[sep + 1..].iter().all(|a| a.starts_with(":(exclude")));
        assert!(has(
            &diff_argv(&gd(), false, None).unwrap(),
            "--ignore-submodules=all"
        ));
        assert!(has(
            &commit_argv(&gd(), &who(), "m").unwrap(),
            "--cleanup=whitespace"
        ));
        assert!(pair(
            &commit_argv(&gd(), &who(), "m").unwrap(),
            "gpg.program=false"
        ));
        for p in [
            ("GIT_ATTR_NOSYSTEM", "1"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
        ] {
            assert!(env(true).contains(&p) && env(false).contains(&p));
        }
        assert!(env(false).contains(&("GIT_CONFIG_GLOBAL", "/dev/null")));
    }

    #[test]
    fn rendering_cannot_forge_lines_or_hide_text() {
        let raw = "# branch.head ma\x1b[31min\0? a\nb\u{202e}c\0\
2 R. N... 1 1 1 a b R100 new\rname\0old\nforged\0";
        let out = render_status(&parse_status(raw));
        assert_eq!(out.lines().count(), 3, "{out:?}");
        assert!(!out.contains(['\r', '\x1b', '\u{202e}']));
        let commits = parse_log(&format!(
            "{:040}\u{1f}An\na\u{1f}2026-01-01T00:00:00Z\u{1f}sub\x1b[2Jject\u{202e}\n",
            1
        ));
        let out = render_log(&commits);
        assert_eq!(out.lines().count(), 1);
        assert!(!out.contains(['\x1b', '\u{202e}']));
    }

    #[test]
    fn a_hostile_orig_path_cannot_forge_an_entry() {
        let raw = "2 short\x001 .M N... 1 1 1 a b forged.rs\0? real\0";
        let s = parse_status(raw);
        let paths: Vec<_> = s.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["real"]);
    }

    #[test]
    fn status_drops_secret_names_old_and_new() {
        let raw = "1 .M N... 1 1 1 a b .env\0\
2 R. N... 1 1 1 a b R100 ok.rs\0.env\0\
2 R. N... 1 1 1 a b R100 .npmrc\0old.rs\0\
? sub/.ENV.local\0? x/.aws/c\0? fine\0";
        let s = parse_status(raw);
        let paths: Vec<_> = s.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["fine"]);
    }

    #[test]
    fn secret_directories_are_excluded_and_refused() {
        let specs = excludes();
        for dir in [".ssh", ".gnupg", ".config/gh", ".aws", ".claude"] {
            assert!(
                specs.contains(&format!(":(exclude,glob,icase)**/{dir}/**")),
                "{dir}"
            );
        }
        assert!(specs.contains(&":(exclude,glob,icase)**/.lbx-*.tmp".to_owned()));
        for bad in ["a/.ssh/id_rsa", ".SSH/x", "q/.Config/GH/hosts", ".aws"] {
            assert_eq!(add_argv(&gd(), &[bad]), Err(GitError::Path), "{bad}");
        }
        assert!(add_argv(&gd(), &[".config/other/x", "gh/x"]).is_ok());
    }

    #[test]
    fn hooks_are_pinned_to_the_repo_git_dir() {
        let all = [
            status_argv(&gd()),
            diff_argv(&gd(), false, None).unwrap(),
            log_argv(&gd(), None, None).unwrap(),
            add_argv(&gd(), &["a"]).unwrap(),
            commit_argv(&gd(), &who(), "m").unwrap(),
        ];
        for argv in &all {
            assert!(
                pair(argv, "core.hooksPath=/home/u/repo/.git/hooks"),
                "{argv:?}"
            );
        }
        for bad in ["", "rel/.git", "/a\0b", "/a\n/.git", "/a/\u{202e}"] {
            assert_eq!(
                GitDir::new(bad, "/a/.git"),
                Err(GitError::GitDir),
                "{bad:?}"
            );
            assert_eq!(
                GitDir::new("/a/.git", bad),
                Err(GitError::GitDir),
                "{bad:?}"
            );
        }
        assert!(GitDir::new("/", "/").is_err());
    }

    #[test]
    fn hooks_notice_names_a_foreign_hooks_path() {
        let dir = gd();
        assert_eq!(hooks_notice(&dir, "/home/u/repo/.git/hooks"), None);
        assert_eq!(hooks_notice(&dir, ""), None);
        let text = hooks_notice(&dir, ".husky/_\nforged").unwrap();
        assert_eq!(
            text,
            "this repo sets core.hooksPath to .husky/_?forged; lazarobox runs only the repo hooks folder"
        );
    }

    #[test]
    fn commit_carries_the_identity() {
        let argv = commit_argv(&gd(), &who(), "m").unwrap();
        assert!(pair(&argv, "user.name=Ana Pérez"));
        assert!(pair(&argv, "user.email=ana@example.com"));
        let commit = argv.iter().position(|a| a == "commit").unwrap();
        assert!(
            argv.iter()
                .position(|a| a == "user.name=Ana Pérez")
                .unwrap()
                < commit
        );
    }

    #[test]
    fn identity_rules() {
        assert!(Identity::new("A", "a@b").is_ok());
        let long = "x".repeat(MAX_IDENTITY_BYTES + 1);
        for (name, email) in [
            ("", "a@b"),
            ("A", ""),
            ("A\nB", "a@b"),
            ("A\u{202e}", "a@b"),
            ("A", "a@b\x1b"),
            ("A", "a @b"),
            ("A", "ab"),
            (long.as_str(), "a@b"),
            ("A", long.as_str()),
        ] {
            assert_eq!(
                Identity::new(name, email),
                Err(GitError::Identity),
                "{name:?} {email:?}"
            );
        }
    }

    #[test]
    fn log_skips_records_with_a_bad_hash_or_date() {
        let ok = format!("{:040}", 7);
        let d = "2026-01-01T00:00:00+02:00";
        // A unit separator inside the author shifts the fields: the date
        // slot then holds text, and the record is dropped.
        let shifted = format!("{ok}\u{1f}An\u{1f}a\u{1f}{d}\u{1f}subject\n");
        let bad_hash = format!("{}\u{1f}A\u{1f}{d}\u{1f}s\n", "z".repeat(40));
        let short_hash = format!("abc\u{1f}A\u{1f}{d}\u{1f}s\n");
        let bad_date = format!("{ok}\u{1f}A\u{1f}yesterday\u{1f}s\n");
        let good = format!("{ok}\u{1f}A\u{1f}{d}\u{1f}s\n");
        for bad in [&shifted, &bad_hash, &short_hash, &bad_date] {
            assert_eq!(parse_log(bad), [], "{bad:?}");
        }
        assert_eq!(parse_log(&good).len(), 1);
        assert_eq!(
            parse_log(&format!("{ok}\u{1f}A\u{1f}2026-01-01T00:00:00Z\u{1f}s")).len(),
            1
        );
    }

    #[test]
    fn log_keeps_history_simplification() {
        // Commits that touch only secret files stay hidden: the excludes
        // narrow the history. `--full-history` would show them.
        let argv = log_argv(&gd(), None, Some("src")).unwrap();
        assert!(!argv.iter().any(|a| a.starts_with("--full-history")));
        assert!(
            !argv
                .iter()
                .any(|a| a == "--simplify-merges" || a == "--sparse")
        );
    }

    #[test]
    fn paths_with_control_or_format_characters_are_refused() {
        for bad in [
            "a\nb",
            "a\x1b[2Jb",
            "a\u{202e}b",
            "a\rb",
            "a\tb",
            "a\u{200b}b",
        ] {
            assert_eq!(add_argv(&gd(), &[bad]), Err(GitError::Path), "{bad:?}");
            assert_eq!(diff_argv(&gd(), false, Some(bad)), Err(GitError::Path));
            assert_eq!(log_argv(&gd(), None, Some(bad)), Err(GitError::Path));
        }
    }

    #[test]
    fn hooks_follow_the_common_dir_in_a_linked_worktree() {
        let linked = GitDir::new("/home/u/repo/.git/worktrees/w1", "/home/u/repo/.git/").unwrap();
        for argv in [
            status_argv(&linked),
            add_argv(&linked, &["a"]).unwrap(),
            commit_argv(&linked, &who(), "m").unwrap(),
        ] {
            assert!(pair(&argv, "core.hooksPath=/home/u/repo/.git/hooks"));
            assert!(!argv.iter().any(|a| a.contains("worktrees")));
        }
        assert_eq!(hooks_notice(&linked, "/home/u/repo/.git/hooks"), None);
        assert!(hooks_notice(&linked, "/home/u/repo/.git/worktrees/w1/hooks").is_some());
        assert_eq!(GitDir::new("/a/.git", "rel/.git"), Err(GitError::GitDir));
    }

    #[test]
    fn add_dot_stages_everything_but_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        if git(dir.path(), &args(&["init", "-q"])).is_none() {
            return;
        }
        for file in ["a.txt", "d/b.txt", ".hidden", "d/.env", "e/.ssh/k"] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        let add = add_argv(&gd(), &["."]).unwrap();
        assert!(has(&add, ":(literal)."));
        git(dir.path(), &add);
        let staged = git(dir.path(), &args(&["diff", "--cached", "--name-only"])).unwrap();
        let mut names: Vec<_> = staged.lines().collect();
        names.sort();
        assert_eq!(names, [".hidden", "a.txt", "d/b.txt"]);
    }
}
