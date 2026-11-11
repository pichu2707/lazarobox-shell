//! The per-file line counter behind the limit of a `simple` worker (W9, D13).
//! Pure: no IO.

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

/// Whether `path` is a test file by its name or a `tests` directory: no cap.
/// Whole-name rules, so `attests.rs` and `latest.rs` are not tests.
pub fn is_test(path: &str) -> bool {
    let mut parts = path.rsplit(['/', '\\']);
    let name = parts.next().unwrap_or("");
    name.starts_with("tests.")
        || ["_test.", "_tests.", ".spec.", ".test."]
            .iter()
            .any(|m| name.contains(m))
        || parts.any(|dir| dir == "tests")
}

/// The added rows that are code, with a block comment tracked across them.
fn code_rows(lang: &Lang, diff: &Diff) -> usize {
    let mut open: Option<&str> = None;
    let mut code = 0;
    for hunk in &diff.hunks {
        for row in hunk.lines.iter().filter(|l| l.kind == Kind::Added) {
            code += usize::from(is_code(lang, row.raw_text(), &mut open));
        }
    }
    code
}

/// Whether a row is code, given the block comment still open before it.
fn is_code(lang: &Lang, row: &str, open: &mut Option<&'static str>) -> bool {
    let mut rest = row.trim();
    loop {
        if let Some(close) = *open {
            let Some(at) = rest.find(close) else {
                return false;
            };
            *open = None;
            rest = rest[at + close.len()..].trim();
        }
        if rest.is_empty() {
            return false;
        }
        let opener = lang
            .block
            .into_iter()
            .chain(lang.triple.then_some(("\"\"\"", "\"\"\"")))
            .chain(lang.triple.then_some(("'''", "'''")))
            .find(|(o, _)| rest.starts_with(o));
        if let Some((o, c)) = opener {
            *open = Some(c);
            rest = &rest[o.len()..];
            continue;
        }
        return !lang.line.iter().any(|m| rest.starts_with(m));
    }
}
