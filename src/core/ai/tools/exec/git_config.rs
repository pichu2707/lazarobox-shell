//! What a repository's own git config sets that would make `git status`,
//! `git log` or `git diff` run a program. Pure: the caller reads the files.

/// What one config file says.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Scan {
    /// A key that makes status, log or diff run a program is set.
    pub runs_program: bool,
    /// The paths of `include.path` and `includeIf.*.path`, as written.
    pub includes: Vec<String>,
}

/// The text is not a config this scan can read: the caller asks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unparsed;

/// Reads one config file's text. The keys that make `git status`, `git log`
/// or `git diff` run a program: `core.fsmonitor`, `core.pager`, any
/// `pager.*`, `diff.external`, `diff.<driver>.textconv` and `.command`, and
/// `filter.<driver>.clean`, `.process` and `.smudge`, whatever their value.
/// Section and key names compare case-insensitively, the legacy
/// `[section.sub]` header included. A line this scan cannot read for sure
/// (a key outside a section, a broken header or key name, an escape or a
/// continued line in an include path) is [`Unparsed`]: a key could hide
/// behind it.
pub fn scan(text: &str) -> Result<Scan, Unparsed> {
    let mut out = Scan::default();
    let mut section: Option<(String, bool)> = None;
    for raw in text.lines() {
        let mut line = raw.trim();
        if let Some(rest) = line.strip_prefix('[') {
            let (header, after) = header(rest)?;
            section = Some(header);
            line = after.trim();
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let (name, sub) = section.as_ref().ok_or(Unparsed)?;
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), Some(value.trim())),
            None => (line, None),
        };
        if !is_name(key, false) {
            return Err(Unparsed);
        }
        let key = key.to_ascii_lowercase();
        out.runs_program |= matches!(
            (name.as_str(), sub, key.as_str()),
            ("core", false, "fsmonitor" | "pager")
                | ("pager", _, _)
                | ("diff", false, "external")
                | ("diff", true, "textconv" | "command")
                | ("filter", true, "clean" | "process" | "smudge")
        );
        if matches!(
            (name.as_str(), sub, key.as_str()),
            ("include", false, "path") | ("includeif", true, "path")
        ) {
            out.includes.push(path(value.unwrap_or(""))?);
        }
    }
    Ok(out)
}

/// The section of a header (`rest` follows its `[`): its name in lower case
/// and whether it has a subsection, and the text after the `]`.
fn header(rest: &str) -> Result<((String, bool), &str), Unparsed> {
    if let Some(quote) = rest.find('"') {
        let name = rest[..quote].trim();
        let mut chars = rest[quote + 1..].char_indices();
        let end = loop {
            match chars.next().ok_or(Unparsed)? {
                (_, '\\') => {
                    chars.next().ok_or(Unparsed)?;
                }
                (at, '"') => break quote + 1 + at,
                _ => {}
            }
        };
        let after = rest[end + 1..]
            .trim_start()
            .strip_prefix(']')
            .ok_or(Unparsed)?;
        if !is_name(name, false) {
            return Err(Unparsed);
        }
        return Ok(((name.to_ascii_lowercase(), true), after));
    }
    let (inner, after) = rest.split_once(']').ok_or(Unparsed)?;
    let inner = inner.trim();
    if !is_name(inner, true) {
        return Err(Unparsed);
    }
    let (name, sub) = match inner.split_once('.') {
        Some((name, _)) => (name, true),
        None => (inner, false),
    };
    Ok(((name.to_ascii_lowercase(), sub), after))
}

/// A section or key name: ASCII letters, digits and `-`, starting with a
/// letter; `dotted` allows the `.` of a legacy subsection header.
fn is_name(name: &str, dotted: bool) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || (dotted && b == b'.'))
}

/// The value of an include path: unquoted or one whole quoted string, a
/// trailing comment dropped. An escape or a continued line is [`Unparsed`].
fn path(value: &str) -> Result<String, Unparsed> {
    if value.contains('\\') {
        return Err(Unparsed);
    }
    let value = match value.strip_prefix('"') {
        Some(rest) => rest.split_once('"').ok_or(Unparsed)?.0,
        None => value.split(['#', ';']).next().unwrap_or("").trim(),
    };
    if value.is_empty() {
        return Err(Unparsed);
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;
