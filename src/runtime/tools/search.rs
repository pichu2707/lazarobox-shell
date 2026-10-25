//! `grep`: a regular expression over the text of the files under the chat's
//! root. Read only. It walks with the budget, the secret rules and the safe
//! opens of the other read tools (see `fs`), and returns redacted lines.
//!
//! The pattern is matched against the cleaned and redacted text, never the
//! raw text: a model must not learn by matching what a secret holds.

use std::{
    collections::VecDeque, io::Read, os::unix::fs::MetadataExt, path::Path,
    sync::atomic::AtomicBool,
};

use regex::RegexBuilder;

use super::fs::{
    Budget, Failure, Limits, OMITTED_MARKER, ToolEnv, entries, load_roots, one_line, open_checked,
    redaction_changes, visible,
};
use crate::core::ai::{
    memory::redact::redact,
    sanitize::clean,
    tools::{
        glob::{BOUND_MARKER, MAX_MATCHES, MAX_PATTERN_BYTES, NO_MATCH},
        paths::{PathVerdict, SecretRoots, is_excluded_name, is_git_config, resolve},
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
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(Failure::BadPattern);
    }
    let re = RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|_| Failure::BadPattern)?;
    let roots = load_roots(env)?;
    let budget = Budget::new(limits, cancel);
    let root = env.origin.canonicalize().map_err(|_| Failure::Refused)?;
    let PathVerdict::Allowed(start) = resolve(&roots, &root, dir.unwrap_or(".")) else {
        return Err(Failure::Refused);
    };
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
    let mut found = Found::default();
    let mut queue = VecDeque::from([(start, 0usize)]);
    let (mut visited, mut bounded, mut omitted) = (0usize, false, false);
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
            } else if let Some(text) = read_text(&roots, &path)
                && !found.scan(&one_line(rel.as_os_str()), &text, &re)
            {
                break 'walk;
            }
        }
    }
    Ok(found.render(omitted, bounded))
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
    let text = String::from_utf8(bytes).ok()?;
    Some(redact(&clean(&text)).text)
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

// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
// `cfg(test)` stays on its own line: the audit drops exactly that attribute.
#[cfg(test)]
#[cfg(target_os = "linux")]
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

    #[test]
    fn home_unset_refuses() {
        let mut f = fixture();
        f.env.home = Some(OsString::new());
        assert_eq!(run(&f, "x", None), Err(Failure::Refused));
    }
}
