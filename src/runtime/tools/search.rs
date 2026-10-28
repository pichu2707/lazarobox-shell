//! `grep`: a regular expression over the text of the files under the chat's
//! root. Read only. It walks with the budget, the secret rules and the safe
//! opens of the other read tools (see `fs`), and returns redacted lines.
//!
//! The pattern is matched against the cleaned and redacted text, never the
//! raw text: a model must not learn by matching what a secret holds.

use std::{
    collections::VecDeque,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use regex::RegexBuilder;

use super::{
    fs::{
        Budget, Failure, Limits, OMITTED_MARKER, ToolEnv, entries, load_roots, one_line,
        open_checked, redaction_changes, visible,
    },
    staged::{MARKER, StagedView, plain, plain_str},
};
use crate::core::ai::{
    memory::redact::redact,
    sanitize::clean,
    tools::{
        glob::{BOUND_MARKER, Glob, MAX_MATCHES, MAX_PATTERN_BYTES, NO_MATCH, render},
        paths::{
            PathVerdict, SecretRoots, is_excluded_name, is_git_config, plain_relative, resolve,
        },
    },
};

/// The most bytes of match lines a result holds.
pub const MAX_OUTPUT_BYTES: usize = 20 * 1024;
/// The largest file searched, in bytes.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// The most characters of one matching line that are shown.
const MAX_LINE_CHARS: usize = 200;
/// The most compiled bytes a pattern may take.
const SIZE_LIMIT: usize = 256 * 1024;
/// Folders the walk never enters.
const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    ".git",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];
/// Whether `name` is a folder the walk never enters (ASCII case-insensitive).
fn skipped_dir(name: &str) -> bool {
    SKIPPED_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
}
/// Said when the matches hit the count or the size cap.
pub const CAP_MARKER: &str = "[truncated: more matches than the cap of 100 or 20 KiB]";

/// Searches the root of the chat (`env.origin`), or its sub-directory `dir`,
/// for `pattern`. One `path:line:text` per match, paths relative to the root.
pub fn grep(
    pattern: &str,
    dir: Option<&str>,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<String, Failure> {
    grep_over(pattern, dir, env, limits, cancel, None)
}

/// [`grep`] that also searches the staged text of `view`, marking those paths.
pub fn grep_over(
    pattern: &str,
    dir: Option<&str>,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
    view: Option<&StagedView>,
) -> Result<String, Failure> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(Failure::BadPattern);
    }
    let re = RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|_| Failure::BadPattern)?;
    let mut found = Found::default();
    let walked = walk(
        dir,
        env,
        limits,
        cancel,
        view,
        |roots, path, rel, staged| {
            let text = match staged {
                Some(text) => prepared(text),
                None => read_text(roots, path).map(plain),
            };
            text.is_none_or(|text| found.scan(&shown(rel, staged), &text, &re))
        },
    )?;
    Ok(found.render(walked.omitted, walked.bounded))
}

/// Lists the files under the root (or its sub-directory `dir`) whose
/// root-relative path matches the glob `pattern`: names only, never content.
pub fn glob(
    pattern: &str,
    dir: Option<&str>,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<String, Failure> {
    glob_over(pattern, dir, env, limits, cancel, None)
}

/// [`glob`] that also lists the staged files of `view`, marking those paths.
pub fn glob_over(
    pattern: &str,
    dir: Option<&str>,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
    view: Option<&StagedView>,
) -> Result<String, Failure> {
    let matcher = Glob::new(pattern).map_err(|_| Failure::BadPattern)?;
    let mut paths = Vec::new();
    // The walk may collect one name past the cap, so `render` can say it cut.
    let walked = walk(dir, env, limits, cancel, view, |_, _, rel, staged| {
        if matcher.is_match(rel) {
            paths.push(shown(rel, staged));
        }
        paths.len() <= MAX_MATCHES
    })?;
    let mut out = render(paths, walked.bounded);
    if walked.omitted {
        out.push('\n');
        out.push_str(OMITTED_MARKER);
    }
    Ok(out)
}

/// The path as the result shows it: marked when its text is staged, and with
/// the marker words defanged otherwise.
fn shown(rel: &str, staged: Option<&str>) -> String {
    match staged {
        Some(_) => format!("{rel} {MARKER}"),
        None => plain_str(rel).into_owned(),
    }
}

/// `dir` joined to `root` by text alone, when it is a plain root-relative
/// path that the approval label shows exactly as it is searched (see
/// [`plain_relative`]).
fn shown_dir(root: &Path, dir: &str) -> Option<PathBuf> {
    plain_relative(dir).map(|rel| root.join(rel))
}

/// What a [`walk`] left out.
struct Walked {
    /// Names hidden because they look like secrets.
    omitted: bool,
    /// A budget (time, entries, depth) or a cancel stopped the walk.
    bounded: bool,
}

/// The budgeted walk of both search tools: breadth first from `dir` (the
/// root when `None`), never following a link, never entering a skipped
/// folder, never showing a secret name or a git setting. `visit` gets the
/// secret roots, the real path and the root-relative one-line path of each
/// regular file, and the staged text of it when `view` holds one (a file the
/// batch creates is visited after the disk, under the same rules); it returns
/// `false` to stop.
fn walk(
    dir: Option<&str>,
    env: &ToolEnv,
    limits: &Limits,
    cancel: &AtomicBool,
    view: Option<&StagedView>,
    mut visit: impl FnMut(&SecretRoots, &Path, &str, Option<&str>) -> bool,
) -> Result<Walked, Failure> {
    let roots = load_roots(env)?;
    let budget = Budget::new(limits, cancel);
    let root = env.origin.canonicalize().map_err(|_| Failure::Refused)?;
    let dir = dir.unwrap_or(".");
    let Some(shown) = shown_dir(&root, dir) else {
        return Err(Failure::Refused);
    };
    let PathVerdict::Allowed(start) = resolve(&roots, &root, dir) else {
        return Err(Failure::Refused);
    };
    // A link on the way would search a folder other than the one shown.
    if start != shown {
        return Err(Failure::Refused);
    }
    // The sub-directory may not leave the root, by `..` or by a link.
    if !start.starts_with(&root) {
        return Err(Failure::Refused);
    }
    // A walk may not start inside a skipped folder either (`path=".git"`).
    if start.strip_prefix(&root).is_ok_and(|rel| {
        rel.components()
            .any(|c| skipped_dir(&c.as_os_str().to_string_lossy()))
    }) {
        return Err(Failure::Refused);
    }
    let mut queue = VecDeque::from([(start.clone(), 0usize)]);
    let (mut visited, mut bounded, mut omitted) = (0usize, false, false);
    let mut seen: Vec<PathBuf> = Vec::new();
    'walk: while let Some((folder, level)) = queue.pop_front() {
        if budget.spent() {
            bounded = true;
            break;
        }
        let Ok(fd) = open_checked(&roots, &folder, true) else {
            continue;
        };
        let Ok((mut children, more)) = entries(&fd, limits.visited, &budget) else {
            continue;
        };
        bounded |= more;
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in children {
            visited += 1;
            if budget.spent() || visited > limits.visited {
                bounded = true;
                break 'walk;
            }
            // Links are never followed: one could lead out of the root.
            if kind.is_symlink() || !(kind.is_dir() || kind.is_file()) {
                continue;
            }
            let text_name = name.to_string_lossy();
            if kind.is_dir() && skipped_dir(&text_name) {
                continue;
            }
            if is_excluded_name(&text_name) || redaction_changes(&name) {
                omitted = true;
                continue;
            }
            if !visible(&roots, &budget, &folder, &name, kind) {
                continue;
            }
            let path = folder.join(&name);
            // Git settings can hold a credential: never read, wherever they sit.
            if kind.is_file() && is_git_config(&path) {
                continue;
            }
            let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            if redaction_changes(rel.as_os_str()) {
                omitted = true;
                continue;
            }
            if kind.is_dir() {
                if level >= limits.depth {
                    bounded = true;
                } else {
                    queue.push_back((path, level + 1));
                }
            } else {
                let staged = view.and_then(|v| v.text(&path));
                seen.extend(staged.map(|_| path.clone()));
                if !visit(&roots, &path, &one_line(rel.as_os_str()), staged) {
                    return Ok(Walked { omitted, bounded });
                }
            }
        }
    }
    // The files only the batch makes, under the rules the walk applies.
    for (path, text) in view.iter().flat_map(|v| v.files_below(&start)) {
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let within = path
            .strip_prefix(&start)
            .unwrap_or(path)
            .components()
            .count();
        if seen.iter().any(|p| p == path) || within > limits.depth + 1 {
            continue;
        }
        match staged_only(&roots, path, rel) {
            Some(true) => omitted = true,
            Some(false) if !visit(&roots, path, &one_line(rel.as_os_str()), Some(text)) => break,
            _ => {}
        }
    }
    Ok(Walked { omitted, bounded })
}

/// Whether the walk would show the staged file `path` (`rel` below the root):
/// `Some(false)` shown, `Some(true)` left out as a secret-looking name (said so
/// in the result), `None` left out silently (a skipped folder, a git setting).
fn staged_only(roots: &SecretRoots, path: &Path, rel: &Path) -> Option<bool> {
    let names: Vec<_> = rel.components().map(|c| c.as_os_str()).collect();
    let (_, folders) = names.split_last()?;
    if folders
        .iter()
        .any(|name| skipped_dir(&name.to_string_lossy()))
        || is_git_config(path)
    {
        return None;
    }
    let secret = names
        .iter()
        .any(|name| is_excluded_name(&name.to_string_lossy()) || redaction_changes(name));
    if secret || redaction_changes(rel.as_os_str()) {
        return Some(true);
    }
    roots.judge_real(path).then_some(false)
}

/// The cleaned and redacted text of a regular file of at most
/// [`MAX_FILE_BYTES`], or `None` for anything else: a binary, a file that is
/// not UTF-8, a hard-linked one or one that cannot be opened safely.
fn read_text(roots: &SecretRoots, path: &Path) -> Option<String> {
    let file = std::fs::File::from(open_checked(roots, path, false).ok()?);
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.nlink() > 1 || meta.len() > MAX_FILE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_FILE_BYTES || bytes.contains(&0) {
        return None;
    }
    prepared(&String::from_utf8(bytes).ok()?)
}

/// The cleaned and redacted `text`, unless it is over [`MAX_FILE_BYTES`].
fn prepared(text: &str) -> Option<String> {
    (text.len() <= MAX_FILE_BYTES).then(|| redact(&clean(text)).text)
}

/// The match lines so far and whether a cap stopped the search.
#[derive(Default)]
struct Found {
    lines: Vec<String>,
    bytes: usize,
    capped: bool,
}

impl Found {
    /// Adds the matching lines of `text`. `false` once a cap is hit.
    fn scan(&mut self, rel: &str, text: &str, re: &regex::Regex) -> bool {
        for (at, line) in text.lines().enumerate().filter(|(_, l)| re.is_match(l)) {
            let shown: String = line.chars().take(MAX_LINE_CHARS).collect();
            let entry = format!("{rel}:{}:{shown}", at + 1);
            if self.lines.len() == MAX_MATCHES || self.bytes + entry.len() + 1 > MAX_OUTPUT_BYTES {
                self.capped = true;
                return false;
            }
            self.bytes += entry.len() + 1;
            self.lines.push(entry);
        }
        true
    }

    fn render(mut self, omitted: bool, bounded: bool) -> String {
        if self.lines.is_empty() {
            self.lines.push(NO_MATCH.to_owned());
        }
        if omitted {
            self.lines.push(OMITTED_MARKER.to_owned());
        }
        if self.capped {
            self.lines.push(CAP_MARKER.to_owned());
        }
        if bounded {
            self.lines.push(BOUND_MARKER.to_owned());
        }
        self.lines.join("\n")
    }
}

// Needs the file tools: they open files below HOME without a link, Linux and macOS.
// `cfg(test)` stays on its own line: the audit drops exactly that attribute.
#[cfg(test)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod tests {
    use super::*;
    use crate::core::ai::tools::glob::{BOUND_MARKER, NO_MATCH};
    use std::{ffi::OsString, fs, os::unix::fs::symlink, path::PathBuf, sync::atomic::AtomicBool};

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        env: ToolEnv,
    }

    /// `home/proj` is the root of the chat; `home/outside` holds a match
    /// the root must never reach.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap().join("home");
        let root = home.join("proj");
        for sub in [
            "proj/src",
            "proj/target",
            "proj/node_modules/x",
            "proj/.git",
            "outside",
        ] {
            fs::create_dir_all(home.join(sub)).unwrap();
        }
        fs::write(home.join("outside/leak.txt"), "fn main outside\n").unwrap();
        let env = ToolEnv {
            home: Some(home.clone().into_os_string()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: root.clone(),
        };
        Fixture {
            _dir: dir,
            root,
            env,
        }
    }

    fn run(f: &Fixture, pattern: &str, dir: Option<&str>) -> Result<String, Failure> {
        grep(
            pattern,
            dir,
            &f.env,
            &Limits::default(),
            &AtomicBool::new(false),
        )
    }

    // Spec (code-search, grep): Matches with line numbers.
    #[test]
    fn matches_come_with_path_and_line_number() {
        let f = fixture();
        fs::write(f.root.join("src/a.rs"), "use x;\n\nfn main() {}\n").unwrap();
        fs::write(f.root.join("src/b.rs"), "fn other() {}\nfn main2() {}\n").unwrap();
        assert_eq!(
            run(&f, "fn main", None).unwrap(),
            "src/a.rs:3:fn main() {}\nsrc/b.rs:2:fn main2() {}"
        );
        assert_eq!(run(&f, "^use", None).unwrap(), "src/a.rs:1:use x;");
        assert_eq!(run(&f, "nothing here", None).unwrap(), NO_MATCH);
    }

    /// Both search tools, run on `dir`, must give the same verdict.
    fn both(f: &Fixture, dir: &str) -> [Result<String, Failure>; 2] {
        let cancel = AtomicBool::new(false);
        [
            run(f, "fn main", Some(dir)),
            glob("**/*.rs", Some(dir), &f.env, &Limits::default(), &cancel),
        ]
    }

    // The ask names `dir` as the user reads it: a folder the label cannot
    // show exactly is refused, so what is approved is what is searched.
    #[test]
    fn a_folder_the_ask_cannot_show_exactly_is_refused() {
        let f = fixture();
        fs::create_dir_all(f.root.join("private")).unwrap();
        fs::write(
            f.root.join("private/p.rs"),
            "fn main secret
",
        )
        .unwrap();
        symlink(f.root.join("private"), f.root.join("docs")).unwrap();
        fs::create_dir_all(f.root.join("a  b")).unwrap();
        fs::create_dir_all(f.root.join("z\u{200b}w")).unwrap();
        let inside = f.root.join("src").to_str().unwrap().to_owned();
        for dir in [
            "docs",
            "docs/",
            "",
            "~/x",
            "~",
            inside.as_str(),
            "a  b",
            "z\u{200b}w",
            "src/../src",
            "src/\nx",
        ] {
            for out in both(&f, dir) {
                assert!(matches!(out, Err(Failure::Refused)), "{dir:?}: {out:?}");
            }
        }
    }

    #[test]
    fn a_plain_root_relative_folder_still_works() {
        let f = fixture();
        fs::create_dir_all(f.root.join("src/sub")).unwrap();
        fs::write(f.root.join("src/sub/a.rs"), "fn main() {}\n").unwrap();
        for dir in ["src/sub", "./src/sub", "src/sub/", "src//sub", "."] {
            let [grepped, globbed] = both(&f, dir);
            assert!(grepped.unwrap().contains("a.rs:1:fn main"), "{dir}");
            assert!(globbed.unwrap().contains("a.rs"), "{dir}");
        }
    }

    #[test]
    fn a_sub_directory_narrows_the_search_and_cannot_leave_the_root() {
        let f = fixture();
        fs::write(f.root.join("src/a.rs"), "needle\n").unwrap();
        fs::write(f.root.join("top.txt"), "needle\n").unwrap();
        assert_eq!(run(&f, "needle", Some("src")).unwrap(), "src/a.rs:1:needle");
        for escape in ["..", "../outside", "/etc"] {
            assert_eq!(
                run(&f, "needle", Some(escape)),
                Err(Failure::Refused),
                "{escape}"
            );
        }
    }

    // Spec (code-search, grep): Caps.
    #[test]
    fn at_most_100_matches_and_the_result_says_it_was_cut() {
        let f = fixture();
        fs::write(f.root.join("many.txt"), "hit\n".repeat(500)).unwrap();
        let out = run(&f, "hit", None).unwrap();
        assert_eq!(out.lines().filter(|l| l.contains(":hit")).count(), 100);
        assert!(out.ends_with(CAP_MARKER), "{out}");
        // Exactly 100 matches is not a cut.
        fs::write(f.root.join("many.txt"), "hit\n".repeat(100)).unwrap();
        let out = run(&f, "hit", None).unwrap();
        assert_eq!(out.lines().count(), 100);
        assert!(!out.contains(CAP_MARKER), "{out}");
    }

    #[test]
    fn the_output_is_bounded_to_20_kib() {
        let f = fixture();
        fs::write(
            f.root.join("wide.txt"),
            format!("{}\n", "x".repeat(400)).repeat(300),
        )
        .unwrap();
        let out = run(&f, "x", None).unwrap();
        let body = out
            .strip_suffix(CAP_MARKER)
            .expect("cut by size")
            .trim_end();
        assert!(body.len() <= MAX_OUTPUT_BYTES, "{}", body.len());
        assert!(body.lines().count() < 100, "{}", body.lines().count());
    }

    #[test]
    fn a_long_line_is_shortened() {
        let f = fixture();
        fs::write(
            f.root.join("long.txt"),
            format!("needle {}\n", "y".repeat(1000)),
        )
        .unwrap();
        let out = run(&f, "needle", None).unwrap();
        assert!(out.starts_with("long.txt:1:needle yyy"), "{out}");
        assert!(out.len() < 300, "{}", out.len());
    }

    // Spec (code-search, grep): Pattern too long or invalid.
    #[test]
    fn a_pattern_that_is_too_long_or_not_valid_is_refused() {
        let f = fixture();
        fs::write(f.root.join("a.txt"), "a\n").unwrap();
        assert_eq!(run(&f, &"a".repeat(513), None), Err(Failure::BadPattern));
        assert_eq!(run(&f, "(unclosed", None), Err(Failure::BadPattern));
        assert_eq!(run(&f, "(?=look)ahead", None), Err(Failure::BadPattern));
        assert_eq!(run(&f, &"a".repeat(512), None).unwrap(), NO_MATCH);
        assert_eq!(
            Failure::BadPattern.message(),
            "the pattern is not a valid expression"
        );
    }

    // Spec (code-search, grep): Secrets and binaries.
    #[test]
    fn secrets_binaries_and_skipped_folders_are_not_searched() {
        let f = fixture();
        fs::write(f.root.join(".env"), "needle=1\n").unwrap();
        fs::write(f.root.join("bin.dat"), b"needle\0\n").unwrap();
        fs::write(f.root.join("latin1.txt"), b"needle \xe9\n").unwrap();
        fs::write(f.root.join("target/t.rs"), "needle\n").unwrap();
        fs::write(f.root.join("node_modules/x/i.js"), "needle\n").unwrap();
        fs::write(f.root.join(".git/HEAD"), "needle\n").unwrap();
        fs::write(f.root.join("src/ok.rs"), "needle\n").unwrap();
        let out = run(&f, "needle", None).unwrap();
        assert_eq!(out, format!("src/ok.rs:1:needle\n{OMITTED_MARKER}"));
    }

    // Spec (code-search, grep) and (code-write, Deny rules): git settings.
    #[test]
    fn git_settings_are_never_read_even_through_the_start_folder() {
        let f = fixture();
        let secret = "url = https://host/x";
        fs::write(f.root.join(".git/config"), secret).unwrap();
        fs::create_dir_all(f.root.join(".git/modules/x")).unwrap();
        fs::write(f.root.join(".git/modules/x/config"), secret).unwrap();
        fs::create_dir_all(f.root.join(".GIT")).unwrap();
        fs::write(f.root.join(".GIT/config"), secret).unwrap();
        fs::create_dir_all(f.root.join("src/wt/.git/worktrees/w")).unwrap();
        fs::write(
            f.root.join("src/wt/.git/worktrees/w/config.worktree"),
            secret,
        )
        .unwrap();
        for start in [
            ".git",
            ".git/modules/x",
            ".GIT",
            "src/wt/.git",
            "src/wt/.git/worktrees/w",
        ] {
            assert_eq!(
                run(&f, "url", Some(start)),
                Err(Failure::Refused),
                "{start}"
            );
        }
        // Walking from the root never reaches them either.
        assert_eq!(run(&f, "url", None).unwrap(), NO_MATCH);
        // A stray `config` outside a git folder is plain text.
        fs::write(f.root.join("src/config"), secret).unwrap();
        assert_eq!(
            run(&f, "url", None).unwrap(),
            format!("src/config:1:{secret}")
        );
    }

    #[test]
    fn a_file_over_1_mib_is_skipped() {
        let f = fixture();
        fs::write(
            f.root.join("big.txt"),
            format!("needle\n{}", "z".repeat(MAX_FILE_BYTES)),
        )
        .unwrap();
        fs::write(
            f.root.join("edge.txt"),
            format!("needle\n{}", "z".repeat(MAX_FILE_BYTES - 7)),
        )
        .unwrap();
        assert_eq!(run(&f, "needle", None).unwrap(), "edge.txt:1:needle");
    }

    #[test]
    fn a_recognizable_token_is_redacted_and_cannot_be_matched() {
        let f = fixture();
        fs::write(f.root.join("conf.txt"), "key = sk-abcDEF1234567890 here\n").unwrap();
        let out = run(&f, "key", None).unwrap();
        assert!(out.starts_with("conf.txt:1:key = "), "{out}");
        assert!(!out.contains("sk-abcDEF1234567890"), "{out}");
        // The pattern is matched against the redacted text: no oracle.
        assert_eq!(run(&f, "abcDEF", None).unwrap(), NO_MATCH);
    }

    #[test]
    fn links_are_not_followed_out_of_the_root() {
        let f = fixture();
        let home = f.root.parent().unwrap();
        symlink(home.join("outside"), f.root.join("link")).unwrap();
        symlink(home.join("outside/leak.txt"), f.root.join("leak-link.txt")).unwrap();
        assert_eq!(run(&f, "fn main", None).unwrap(), NO_MATCH);
        assert_eq!(run(&f, "fn main", Some("link")), Err(Failure::Refused));
    }

    // Spec (code-search, grep): Pathological pattern.
    #[test]
    fn a_pathological_pattern_ends_within_the_time_budget() {
        let f = fixture();
        fs::write(f.root.join("a.txt"), format!("{}b\n", "a".repeat(200_000))).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(run(&f, "(a+)+$", None).unwrap(), NO_MATCH);
        assert!(started.elapsed() < Limits::default().time);
    }

    #[test]
    fn a_cancelled_search_stops_and_says_it_was_bounded() {
        let f = fixture();
        fs::write(f.root.join("a.txt"), "needle\n").unwrap();
        let cancel = AtomicBool::new(true);
        let out = grep("needle", None, &f.env, &Limits::default(), &cancel).unwrap();
        assert_eq!(out, format!("{NO_MATCH}\n{BOUND_MARKER}"));
    }

    fn find(f: &Fixture, pattern: &str, dir: Option<&str>) -> Result<String, Failure> {
        glob(
            pattern,
            dir,
            &f.env,
            &Limits::default(),
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn a_leftover_write_temp_is_neither_searched_nor_globbed() {
        let f = fixture();
        let temp = ".lbx-0123456789abcdef0123456789abcdef.tmp";
        fs::write(f.root.join("src").join(temp), "needle\n").unwrap();
        fs::write(f.root.join("src/a.rs"), "needle\n").unwrap();
        for out in [run(&f, "needle", None), find(&f, "**/*", None)] {
            let out = out.unwrap();
            assert!(out.starts_with("src/a.rs"), "{out}");
            assert!(!out.contains(".lbx-"), "{out}");
        }
    }

    // Spec (code-search, glob): Matches under the root.    // Spec (code-search, glob): Matches under the root.
    #[test]
    fn glob_lists_matching_files_relative_to_the_root_and_never_content() {
        let f = fixture();
        fs::write(f.root.join("src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(f.root.join("src/b.rs"), "fn b() {}\n").unwrap();
        fs::write(f.root.join("README.md"), "docs\n").unwrap();
        assert_eq!(find(&f, "**/*.rs", None).unwrap(), "src/a.rs\nsrc/b.rs");
        assert_eq!(find(&f, "*.md", None).unwrap(), "README.md");
        assert_eq!(find(&f, "src/*.rs", None).unwrap(), "src/a.rs\nsrc/b.rs");
        assert_eq!(find(&f, "**/*.py", None).unwrap(), NO_MATCH);
    }

    // The sub-directory narrows the walk; the pattern still sees root-relative paths.
    #[test]
    fn glob_dir_narrows_the_walk_and_cannot_leave_the_root() {
        let f = fixture();
        fs::write(f.root.join("src/a.rs"), "x\n").unwrap();
        fs::write(f.root.join("top.rs"), "x\n").unwrap();
        assert_eq!(find(&f, "**/*.rs", Some("src")).unwrap(), "src/a.rs");
        assert_eq!(find(&f, "src/*.rs", Some("src")).unwrap(), "src/a.rs");
        assert_eq!(find(&f, "*.rs", Some("src")).unwrap(), NO_MATCH);
        for escape in ["..", "../outside", "/etc"] {
            assert_eq!(find(&f, "**/*", Some(escape)), Err(Failure::Refused));
        }
    }

    // Spec (code-search, glob): Skip list and secrets.
    #[test]
    fn glob_skips_the_skip_list_and_says_secrets_were_left_out() {
        let f = fixture();
        fs::write(f.root.join("target/x.rs"), "x\n").unwrap();
        fs::write(f.root.join("node_modules/y.js"), "x\n").unwrap();
        fs::write(f.root.join(".git/HEAD"), "x\n").unwrap();
        fs::write(f.root.join(".env"), "k=1\n").unwrap();
        fs::write(f.root.join("src/ok.rs"), "x\n").unwrap();
        assert_eq!(
            find(&f, "**/*", None).unwrap(),
            format!("src/ok.rs\n{OMITTED_MARKER}")
        );
        assert_eq!(find(&f, "**/*", Some("target")), Err(Failure::Refused));
        fs::create_dir_all(f.root.join("src/Node_Modules/deep")).unwrap();
        assert_eq!(
            find(&f, "**/*", Some("src/Node_Modules/deep")),
            Err(Failure::Refused)
        );
    }

    // Spec (code-search, glob): Root escape refused.
    #[test]
    fn glob_never_follows_links_or_takes_a_pattern_out_of_the_root() {
        let f = fixture();
        let home = f.root.parent().unwrap();
        symlink(home.join("outside"), f.root.join("link")).unwrap();
        symlink(home.join("outside/leak.txt"), f.root.join("leak.txt")).unwrap();
        assert_eq!(find(&f, "**/*", None).unwrap(), NO_MATCH);
        assert_eq!(find(&f, "../**/*", None), Err(Failure::BadPattern));
        assert_eq!(find(&f, "", None), Err(Failure::BadPattern));
    }

    // Spec (code-search, glob): Budget is reported.
    #[test]
    fn glob_reports_a_walk_cut_by_the_budget_or_a_cap() {
        let f = fixture();
        for n in 0..8 {
            fs::write(f.root.join(format!("f{n}.txt")), "x\n").unwrap();
        }
        let few = Limits {
            visited: 3,
            ..Limits::default()
        };
        let out = glob("*.txt", None, &f.env, &few, &AtomicBool::new(false)).unwrap();
        assert!(out.ends_with(BOUND_MARKER), "{out}");
        let cancel = AtomicBool::new(true);
        let out = glob("*", None, &f.env, &Limits::default(), &cancel).unwrap();
        assert_eq!(out, format!("{NO_MATCH}\n{BOUND_MARKER}"));
        for n in 0..150 {
            fs::write(f.root.join(format!("g{n:03}.log")), "x\n").unwrap();
        }
        let out = find(&f, "*.log", None).unwrap();
        assert_eq!(out.lines().count(), 101, "{out}");
        assert!(out.ends_with(crate::core::ai::tools::glob::CAP_MARKER));
    }

    #[test]
    fn glob_with_home_unset_refuses() {
        let mut f = fixture();
        f.env.home = Some(OsString::new());
        assert_eq!(find(&f, "*", None), Err(Failure::Refused));
    }

    #[test]
    fn home_unset_refuses() {
        let mut f = fixture();
        f.env.home = Some(OsString::new());
        assert_eq!(run(&f, "x", None), Err(Failure::Refused));
    }
}
