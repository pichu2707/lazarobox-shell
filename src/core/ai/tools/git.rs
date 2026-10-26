//! The git verbs as fixed argv templates, and the parse of their output.
//! Pure: it spawns nothing and reads no file. The runner in the runtime owns
//! the jail and the process; this module only decides the words.
//!
//! Only status, diff, log, add and commit exist, and no verb takes a free-form
//! argument list. Every call carries the same `-c` hardening, and the secret
//! file names are excluded by pathspec so a tracked `.env` is never printed.

use std::path::{Component, Path};

use super::paths::{SECRET_FILES, is_excluded_name};
use crate::core::ai::sanitize::is_format;

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

/// The unit separator that splits the fields of a log record.
const FIELD: char = '\u{1f}';

/// The settings in front of every verb. They turn off the config keys that
/// run a program, the pager, prompts and the network.
const HARDENING: [&str; 7] = [
    "core.fsmonitor=false",
    "core.untrackedCache=false",
    "core.pager=cat",
    "diff.external=",
    "protocol.allow=never",
    "credential.helper=",
    "core.sshCommand=false",
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
}

impl GitError {
    /// The text sent back to the model.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Path => "path not available",
            Self::PathCount => "give between 1 and 64 paths",
            Self::Message => "invalid commit message",
        }
    }
}

/// The `-c` hardening, the pager switch and nothing else: the start of every argv.
fn base() -> Vec<String> {
    let mut argv = Vec::new();
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
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_EDITOR", "true"),
    ];
    if read {
        pairs.push(("GIT_OPTIONAL_LOCKS", "0"));
    }
    pairs
}

/// The `:(exclude,glob,icase)` pathspecs for the secret names: every
/// [`SECRET_FILES`] entry, `.env` and `.env.*`, at any depth. `icase` follows
/// the case-insensitive rule of the path guard.
pub fn excludes() -> Vec<String> {
    let mut specs: Vec<String> = [".env", ".env.*"]
        .iter()
        .chain(SECRET_FILES)
        .map(|name| format!(":(exclude,glob,icase)**/{name}"))
        .collect();
    specs.sort();
    specs
}

/// Checks a path given to a verb: relative, below the root, outside `.git`
/// and not a secret name. Pure: the runtime still resolves it.
fn check(path: &str) -> Result<(), GitError> {
    if path.is_empty() || path.contains('\0') {
        return Err(GitError::Path);
    }
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if name.eq_ignore_ascii_case(".git") || is_excluded_name(&name) {
                    return Err(GitError::Path);
                }
            }
            Component::CurDir => {}
            _ => return Err(GitError::Path),
        }
    }
    Ok(())
}

/// The pathspec of one path: literal, so `:` and glob characters are inert.
fn literal(path: &str) -> String {
    format!(":(literal){path}")
}

/// `git status`: parsed by [`parse_status`].
pub fn status_argv() -> Vec<String> {
    let mut argv = base();
    argv.extend(
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--ignore-submodules=all",
            "--",
            ".",
        ]
        .map(str::to_owned),
    );
    argv.extend(excludes());
    argv
}

/// `git diff` of the working tree, or of the index when `staged`.
pub fn diff_argv(staged: bool, path: Option<&str>) -> Result<Vec<String>, GitError> {
    let target = match path {
        Some(path) => {
            check(path)?;
            literal(path)
        }
        None => ".".to_owned(),
    };
    let mut argv = base();
    argv.push("diff".to_owned());
    if staged {
        argv.push("--cached".to_owned());
    }
    argv.extend(["--no-ext-diff", "--no-textconv", "--no-color", "--"].map(str::to_owned));
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
pub fn log_argv(max_count: Option<u64>, path: Option<&str>) -> Result<Vec<String>, GitError> {
    let mut argv = base();
    argv.extend([
        "log".to_owned(),
        "-n".to_owned(),
        clamp_count(max_count).to_string(),
        "--no-color".to_owned(),
        "--format=%H%x1f%an%x1f%aI%x1f%s".to_owned(),
    ]);
    if let Some(path) = path {
        check(path)?;
        argv.push("--".to_owned());
        argv.push(literal(path));
        argv.extend(excludes());
    }
    Ok(argv)
}

/// `git add` of literal paths.
pub fn add_argv(paths: &[&str]) -> Result<Vec<String>, GitError> {
    if paths.is_empty() || paths.len() > MAX_ADD_PATHS {
        return Err(GitError::PathCount);
    }
    for path in paths {
        check(path)?;
    }
    let mut argv = base();
    argv.extend(["add", "--"].map(str::to_owned));
    argv.extend(paths.iter().map(|path| literal(path)));
    Ok(argv)
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
pub fn commit_argv(message: &str) -> Result<Vec<String>, GitError> {
    check_message(message)?;
    let mut argv = base();
    argv.extend(["-c", "commit.gpgsign=false", "commit", "-m"].map(str::to_owned));
    argv.push(message.to_owned());
    Ok(argv)
}

/// How a path changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An ordinary change (porcelain `1`).
    Changed,
    /// A rename or a copy (porcelain `2`).
    Renamed,
    /// An unmerged path (porcelain `u`).
    Unmerged,
    Untracked,
}

/// One status entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub kind: Kind,
    /// The two-letter state, index then worktree (`.` for none); `??` for untracked.
    pub xy: String,
    pub path: String,
    /// The old path of a rename or copy.
    pub from: Option<String>,
}

/// A parsed `status --porcelain=v2 -z --branch`.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Status {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<Entry>,
    /// Entries past [`MAX_STATUS_ENTRIES`], counted and not kept.
    pub omitted: usize,
}

/// Parses the porcelain v2 `-z` output. Unknown or short records are skipped;
/// ignored files (`!`) are not listed.
pub fn parse_status(raw: &str) -> Status {
    let mut status = Status::default();
    let mut records = raw.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            header_line(&mut status, header);
            continue;
        }
        let entry = match record.as_bytes().first() {
            Some(b'1') => fields(record, 9).map(|f| (Kind::Changed, f[1], f[8], None)),
            Some(b'2') => fields(record, 10).map(|f| {
                let from = records.next().map(str::to_owned);
                (Kind::Renamed, f[1], f[9], from)
            }),
            Some(b'u') => fields(record, 11).map(|f| (Kind::Unmerged, f[1], f[10], None)),
            Some(b'?') => record
                .strip_prefix("? ")
                .map(|p| (Kind::Untracked, "??", p, None)),
            _ => None,
        };
        let Some((kind, xy, path, from)) = entry else {
            continue;
        };
        if status.entries.len() >= MAX_STATUS_ENTRIES {
            status.omitted += 1;
            continue;
        }
        status.entries.push(Entry {
            kind,
            xy: xy.to_owned(),
            path: path.to_owned(),
            from,
        });
    }
    status
}

/// Splits a record into `count` fields; the last one keeps its spaces.
fn fields(record: &str, count: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = record.splitn(count, ' ').collect();
    (parts.len() == count).then_some(parts)
}

fn header_line(status: &mut Status, header: &str) {
    if let Some(head) = header.strip_prefix("branch.head ") {
        status.branch = Some(head.to_owned());
    } else if let Some(upstream) = header.strip_prefix("branch.upstream ") {
        status.upstream = Some(upstream.to_owned());
    } else if let Some(counts) = header.strip_prefix("branch.ab ") {
        let mut parts = counts.split(' ');
        let number = |part: Option<&str>, sign: char| {
            part.and_then(|p| p.strip_prefix(sign))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        status.ahead = number(parts.next(), '+');
        status.behind = number(parts.next(), '-');
    }
}

/// The text of a status for the model.
pub fn render_status(status: &Status) -> String {
    let mut out = String::new();
    match (&status.branch, &status.upstream) {
        (Some(branch), Some(up)) => out.push_str(&format!(
            "branch {branch} (upstream {up}, ahead {}, behind {})\n",
            status.ahead, status.behind
        )),
        (Some(branch), None) => out.push_str(&format!("branch {branch}\n")),
        _ => {}
    }
    if status.entries.is_empty() {
        out.push_str("clean");
        return out;
    }
    for entry in &status.entries {
        match &entry.from {
            Some(from) => out.push_str(&format!("{} {} <- {from}\n", entry.xy, entry.path)),
            None => out.push_str(&format!("{} {}\n", entry.xy, entry.path)),
        }
    }
    if status.omitted > 0 {
        out.push_str(&format!("[{} more entries not listed]\n", status.omitted));
    }
    out.trim_end().to_owned()
}

/// One log entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

/// Parses the log output: one record per line, four fields split on `0x1f`.
/// At most [`MAX_LOG_COUNT`] records are kept; a malformed line is skipped.
pub fn parse_log(raw: &str) -> Vec<Commit> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, FIELD);
            Some(Commit {
                hash: parts.next()?.to_owned(),
                author: parts.next()?.to_owned(),
                date: parts.next()?.to_owned(),
                subject: parts.next()?.to_owned(),
            })
        })
        .take(MAX_LOG_COUNT)
        .collect()
}

/// The text of a log for the model: `short-hash date author subject`.
pub fn render_log(commits: &[Commit]) -> String {
    if commits.is_empty() {
        return "no commits".to_owned();
    }
    commits
        .iter()
        .map(|c| {
            let short: String = c.hash.chars().take(12).collect();
            format!("{short} {} {} {}", c.date, c.author, c.subject)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(argv: &[String], word: &str) -> bool {
        argv.iter().any(|a| a == word)
    }

    fn pair(argv: &[String], key: &str) -> bool {
        argv.windows(2).any(|w| w[0] == "-c" && w[1] == key)
    }

    #[test]
    fn every_verb_carries_the_hardening() {
        let all = [
            status_argv(),
            diff_argv(false, None).unwrap(),
            log_argv(None, None).unwrap(),
            add_argv(&["a.rs"]).unwrap(),
            commit_argv("msg").unwrap(),
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
        let argv = status_argv();
        let at = argv.iter().position(|a| a == "status").unwrap();
        assert_eq!(
            argv[at..at + 7],
            [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--ignore-submodules=all",
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
        let wt = diff_argv(false, None).unwrap();
        assert!(!has(&wt, "--cached"));
        assert!(has(&wt, "--no-ext-diff") && has(&wt, "--no-textconv") && has(&wt, "--no-color"));
        let sep = wt.iter().position(|a| a == "--").unwrap();
        assert_eq!(wt[sep + 1], ".");
        assert!(wt[sep + 2..].iter().all(|a| a.starts_with(":(exclude")));

        let staged = diff_argv(true, Some("src/a.rs")).unwrap();
        assert!(has(&staged, "--cached"));
        assert!(has(&staged, ":(literal)src/a.rs"));
    }

    #[test]
    fn log_template_and_clamp() {
        let argv = log_argv(Some(500), None).unwrap();
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
        let argv = log_argv(Some(3), Some("src")).unwrap();
        assert!(has(&argv, ":(literal)src"));
        assert!(has(&argv, ":(exclude,glob,icase)**/.env"));
    }

    #[test]
    fn add_is_literal_and_refuses_secrets() {
        let argv = add_argv(&["a.rs", ":(top)x", "*.rs"]).unwrap();
        let sep = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[sep - 1], "add");
        assert_eq!(
            argv[sep + 1..],
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
            assert_eq!(add_argv(&["ok", bad]), Err(GitError::Path), "{bad:?}");
        }
        assert_eq!(add_argv(&[]), Err(GitError::PathCount));
        let many = vec!["a"; MAX_ADD_PATHS + 1];
        assert_eq!(add_argv(&many), Err(GitError::PathCount));
    }

    #[test]
    fn commit_disables_signing() {
        let argv = commit_argv("fix: a\n\nbody\twith tab").unwrap();
        assert!(pair(&argv, "commit.gpgsign=false"));
        let at = argv.iter().position(|a| a == "commit").unwrap();
        assert_eq!(argv[at + 1], "-m");
        assert_eq!(argv[at + 2], "fix: a\n\nbody\twith tab");
        assert_eq!(argv.len(), at + 3);
        assert!(!has(&argv, "--no-verify"));
    }

    #[test]
    fn commit_message_rules() {
        for bad in ["", "  \n", "a\u{1b}[31m", "a\u{202e}b", "a\0b", "a\rb"] {
            assert_eq!(commit_argv(bad), Err(GitError::Message), "{bad:?}");
        }
        assert!(commit_argv(&"x".repeat(MAX_MESSAGE_BYTES)).is_ok());
        assert!(commit_argv(&"x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        assert!(commit_argv("-leading dash").is_ok());
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
            status_argv(),
            diff_argv(true, None).unwrap(),
            log_argv(None, None).unwrap(),
            add_argv(&["a"]).unwrap(),
            commit_argv("m").unwrap(),
        ]
        .map(|argv| {
            let at = argv.iter().position(|a| a == "--no-pager").unwrap();
            let skip = if argv[at + 1] == "-c" { 3 } else { 1 };
            argv[at + skip].clone()
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
        let raw = "h\u{1f}a\u{1f}d\u{1f}one\u{1f}two\nbroken line\n\nh2\u{1f}a\u{1f}d\u{1f}s\n";
        let commits = parse_log(raw);
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
}
