//! Parsing and rendering of the git output: porcelain v2 status and the log.
//! Pure, like its parent: it reads text and returns values.

use super::{MAX_LOG_COUNT, MAX_STATUS_ENTRIES, secret_path};
use crate::core::ai::sanitize::is_format;

/// The unit separator that splits the fields of a log record.
const FIELD: char = '\u{1f}';

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
        // A rename is followed by its old path, whether or not it parsed:
        // the old path must never be read as a record of its own.
        let from = if record.starts_with('2') {
            records.next().map(str::to_owned)
        } else {
            None
        };
        let entry = match record.as_bytes().first() {
            Some(b'1') => fields(record, 9).map(|f| (Kind::Changed, f[1], f[8], None)),
            Some(b'2') => fields(record, 10).map(|f| (Kind::Renamed, f[1], f[9], from)),
            Some(b'u') => fields(record, 11).map(|f| (Kind::Unmerged, f[1], f[10], None)),
            Some(b'?') => record
                .strip_prefix("? ")
                .map(|p| (Kind::Untracked, "??", p, None)),
            _ => None,
        };
        let Some((kind, xy, path, from)) = entry else {
            continue;
        };
        if secret_path(path) || from.as_deref().is_some_and(secret_path) {
            continue;
        }
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

/// Text from git as one safe line: control, format and bidi characters
/// become `?`, so a name cannot forge a line or hide text.
pub(super) fn tidy(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || is_format(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// The text of a status for the model.
pub fn render_status(status: &Status) -> String {
    let mut out = String::new();
    match (&status.branch, &status.upstream) {
        (Some(branch), Some(up)) => out.push_str(&format!(
            "branch {} (upstream {}, ahead {}, behind {})\n",
            tidy(branch),
            tidy(up),
            status.ahead,
            status.behind
        )),
        (Some(branch), None) => out.push_str(&format!("branch {}\n", tidy(branch))),
        _ => {}
    }
    if status.entries.is_empty() {
        out.push_str("clean");
        return out;
    }
    for entry in &status.entries {
        match &entry.from {
            Some(from) => out.push_str(&format!(
                "{} {} <- {}\n",
                entry.xy,
                tidy(&entry.path),
                tidy(from)
            )),
            None => out.push_str(&format!("{} {}\n", entry.xy, tidy(&entry.path))),
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
            let commit = Commit {
                hash: parts.next()?.to_owned(),
                author: parts.next()?.to_owned(),
                date: parts.next()?.to_owned(),
                subject: parts.next()?.to_owned(),
            };
            // A field that holds the separator shifts the rest: drop it.
            (hex_hash(&commit.hash) && iso_date(&commit.date)).then_some(commit)
        })
        .take(MAX_LOG_COUNT)
        .collect()
}

/// Whether text is a full object name: 40 or 64 hex digits.
fn hex_hash(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether text is a strict ISO 8601 date (`%aI`): `YYYY-MM-DDTHH:MM:SS`
/// then `Z` or `+HH:MM` / `-HH:MM`.
fn iso_date(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        b.get(range)
            .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
    };
    let shape = digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && b.get(10) == Some(&b'T')
        && digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16)
        && b.get(16) == Some(&b':')
        && digits(17..19);
    shape
        && match &b[19..] {
            b"Z" => true,
            zone => {
                zone.len() == 6
                    && matches!(zone[0], b'+' | b'-')
                    && zone[1..3].iter().all(u8::is_ascii_digit)
                    && zone[3] == b':'
                    && zone[4..].iter().all(u8::is_ascii_digit)
            }
        }
}

/// The text of a log for the model: `short-hash date author subject`.
pub fn render_log(commits: &[Commit]) -> String {
    if commits.is_empty() {
        return "no commits".to_owned();
    }
    commits
        .iter()
        .map(|c| {
            let short: String = tidy(&c.hash).chars().take(12).collect();
            format!(
                "{short} {} {} {}",
                tidy(&c.date),
                tidy(&c.author),
                tidy(&c.subject)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
