//! The per-file line counter behind the limit of a `simple` worker (W9, D13).
//! Pure: no IO.

use std::collections::BTreeMap;

use super::WorkerKey;
use super::ledger::one_line;
use crate::core::ai::chat::approval::{ApprovalAsk, LimitFile};
use crate::core::ai::tools::diff::{Diff, Kind, diff};

/// The most counted lines one brief may write to one file without an ask.
pub const LIMIT: usize = 400;

/// The counted lines of one write to one file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Count {
    pub lines: usize,
    /// Every added line was counted: the language is unknown, or the diff
    /// could not be read row by row.
    pub every_line: bool,
    /// A test file by path: it has no cap.
    pub test: bool,
}

impl Count {
    /// Whether the write goes over the limit and must ask.
    pub fn over(&self) -> bool {
        !self.test && self.lines > LIMIT
    }
}

/// The limit ask of a `simple` worker's batch, for the user's chat pane: the
/// files over the limit (`over`, with their counted lines) and the staged
/// files outside the brief, every path one clean line. It is aimed at the
/// worker's session.
pub fn limit_ask(
    worker: WorkerKey,
    label: &str,
    over: &[(String, Count)],
    outside: &[String],
) -> ApprovalAsk {
    ApprovalAsk::Limit {
        worker,
        label: one_line(label),
        over: (over.iter())
            .map(|(path, count)| LimitFile {
                path: one_line(path),
                lines: count.lines,
                every_line: count.every_line,
            })
            .collect(),
        outside: outside.iter().map(|path| one_line(path)).collect(),
    }
}

/// How one language marks what is not code.
struct Lang {
    exts: &'static [&'static str],
    /// A line starting with one of these is a comment.
    line: &'static [&'static str],
    /// A comment that may span rows.
    block: Option<(&'static str, &'static str)>,
    /// A line starting with `"""` or `'''` opens a docstring.
    triple: bool,
}

const C_LIKE: Option<(&str, &str)> = Some(("/*", "*/"));

/// The languages the counter reads, by extension. Anything else counts every
/// row.
const LANGS: &[Lang] = &[
    Lang {
        exts: &[
            "rs", "c", "h", "cpp", "cc", "hpp", "js", "jsx", "mjs", "ts", "tsx", "go", "java",
            "kt", "swift", "cs", "scala", "dart",
        ],
        line: &["//"],
        block: C_LIKE,
        triple: false,
    },
    Lang {
        exts: &["php"],
        line: &["//", "#"],
        block: C_LIKE,
        triple: false,
    },
    Lang {
        exts: &["py"],
        line: &["#"],
        block: None,
        triple: true,
    },
    Lang {
        exts: &["sh", "bash", "zsh", "rb", "toml", "yaml", "yml", "pl", "r"],
        line: &["#"],
        block: None,
        triple: false,
    },
    Lang {
        exts: &["sql"],
        line: &["--"],
        block: C_LIKE,
        triple: false,
    },
    Lang {
        exts: &["lua"],
        line: &["--"],
        block: Some(("--[[", "]]")),
        triple: false,
    },
    Lang {
        exts: &["hs"],
        line: &["--"],
        block: Some(("{-", "-}")),
        triple: false,
    },
];

/// The counted lines `text` adds to `path` over `base` (`None` for a create).
pub fn count(path: &str, base: Option<&str>, text: &str) -> Count {
    from_diff(path, &diff(base, text))
}

/// The counted lines of an existing diff of `path`. A diff that is capped or
/// not minimal does not show the whole change, so every added line counts: an
/// over-count only produces an ask.
pub fn from_diff(path: &str, diff: &Diff) -> Count {
    let test = is_test(path);
    let lang = lang_of(path);
    let (lines, every_line) = match lang {
        Some(lang) if !diff.truncated && !diff.coarse => (code_rows(lang, diff), false),
        _ => (diff.added, true),
    };
    Count {
        lines,
        every_line,
        test,
    }
}

fn lang_of(path: &str) -> Option<&'static Lang> {
    let name = path.rsplit(['/', '\\']).next()?;
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    LANGS.iter().find(|l| l.exts.contains(&ext.as_str()))
}

/// Whether `path` is a test file: no cap. `path` is relative to the project
/// root. The name rules (`tests.*`, `*_test.*`, `*_tests.*`, `*.test.*`,
/// `*.spec.*`, on the last suffix pair only) need no directory; a `tests`
/// directory counts only inside a relative path, so the components above the
/// project (an absolute path, or a leading `..`) are never read. Whole-name
/// rules, so `attests.rs` and `latest.rs` are not tests.
pub fn is_test(path: &str) -> bool {
    let key = normalize(path);
    let drive = matches!(key.as_bytes(), [d, b':', ..] if d.is_ascii_alphabetic());
    let relative = !drive && !key.starts_with('/') && !key.starts_with("..");
    let mut parts = key.rsplit('/');
    let name = parts.next().unwrap_or("");
    let stem = name.rsplit_once('.').map_or("", |(stem, _)| stem);
    stem == "tests"
        || ["_test", "_tests", ".test", ".spec"]
            .iter()
            .any(|m| stem.ends_with(m))
        || (relative && parts.any(|dir| dir == "tests"))
}

/// One spelling of a path: `\` as `/`, no empty or `.` component, `..`
/// resolved lexically (a leading one is kept).
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let unified = path.replace('\\', "/");
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." if out.last().is_some_and(|l| *l != "..") => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    let joined = out.join("/");
    if unified.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The added rows that are code. A block comment or docstring is tracked only
/// inside a run of consecutive added rows, and only when its closer comes later
/// in that run: an unclosed opener (a string, a closer on an unchanged row)
/// leaves its row as code, so the counter never silences rows it cannot close.
fn code_rows(lang: &Lang, diff: &Diff) -> usize {
    let mut code = 0;
    for hunk in &diff.hunks {
        let mut run: Vec<&str> = Vec::new();
        for line in &hunk.lines {
            if line.kind == Kind::Added {
                run.push(line.raw_text());
            } else {
                code += run_code(lang, &run);
                run.clear();
            }
        }
        code += run_code(lang, &run);
    }
    code
}

fn run_code(lang: &Lang, run: &[&str]) -> usize {
    let mut open: Option<&str> = None;
    // The triple quote of a string a row of code opened and nothing closed.
    let mut string: Option<&str> = None;
    let mut code = 0;
    for (i, row) in run.iter().enumerate() {
        if let Some(q) = string {
            // Inside a string: code, until the row that closes it.
            code += 1;
            if row.matches(q).count() % 2 == 1 {
                string = None;
            }
            continue;
        }
        if !is_code(lang, row, &run[i + 1..], &mut open) {
            continue;
        }
        code += 1;
        if lang.triple && open.is_none() {
            string = TRIPLES
                .iter()
                .copied()
                .find(|q| row.matches(q).count() % 2 == 1);
        }
    }
    code
}

/// The triple quotes of a docstring or a multi-row string.
const TRIPLES: [&str; 2] = ["\"\"\"", "'''"];

/// Whether a row is code, given the block still open before it and the rows
/// of the run after it.
fn is_code(lang: &Lang, row: &str, later: &[&str], open: &mut Option<&'static str>) -> bool {
    let mut rest = row.trim();
    // The block was opened on this row: its first row is always the comment.
    let mut fresh = false;
    loop {
        if let Some(close) = *open {
            // A docstring hides its rows. A block comment hides only rows
            // that look like one (a leading `*`) and its closing row; any
            // other row is code and ends the block, so a marker inside a
            // string cannot silence what follows.
            let looks = fresh || TRIPLES.contains(&close) || rest.starts_with('*');
            let Some(at) = rest.find(close) else {
                if looks {
                    return false;
                }
                *open = None;
                return true;
            };
            if !(looks || rest.starts_with(close) || rest.ends_with(close)) {
                *open = None;
                return true;
            }
            *open = None;
            rest = rest[at + close.len()..].trim();
        }
        if rest.is_empty() {
            return false;
        }
        let opener = lang
            .block
            .into_iter()
            .chain(lang.triple.then_some((TRIPLES[0], TRIPLES[0])))
            .chain(lang.triple.then_some((TRIPLES[1], TRIPLES[1])))
            .find(|(o, _)| rest.starts_with(o));
        if let Some((o, c)) = opener {
            let after = &rest[o.len()..];
            if !after.contains(c) && !later.iter().any(|r| r.contains(c)) {
                return true;
            }
            *open = Some(c);
            rest = after;
            fresh = true;
            continue;
        }
        return !lang.line.iter().any(|m| rest.starts_with(m));
    }
}

/// The counted lines one worker applied for its brief, per file. A new brief
/// starts from `default`, and each worker has its own: another worker's
/// writes to the same file never add to it.
#[derive(Clone, Default, Debug)]
pub struct Written {
    files: BTreeMap<String, Count>,
}

impl Written {
    /// Records an applied write to `path`.
    pub fn add(&mut self, path: &str, count: Count) {
        let sum = self.with(path, count);
        self.files.insert(normalize(path), sum);
    }

    /// What `path` holds so far.
    pub fn total(&self, path: &str) -> Count {
        self.files.get(&normalize(path)).copied().unwrap_or(Count {
            lines: 0,
            every_line: false,
            test: is_test(path),
        })
    }

    /// What `path` would hold with `count` added, without recording it.
    pub fn with(&self, path: &str, count: Count) -> Count {
        let held = self.total(path);
        Count {
            lines: held.lines.saturating_add(count.lines),
            every_line: held.every_line || count.every_line,
            test: count.test,
        }
    }
}
