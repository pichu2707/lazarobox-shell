//! Whether a repository's own git config holds only settings known to make
//! `git status`, `git log` and `git diff` run nothing and read nothing
//! outside the repository. Allow-list: any other key asks. Pure: the caller
//! reads the files.

/// The keys a plain repository may set: what `git init` and `git clone`
/// write, and a few settings of line endings, identity and defaults. Each is
/// (section, has a subsection, key), names in lower case.
const ALLOWED: &[(&str, bool, &str)] = &[
    ("core", false, "repositoryformatversion"),
    ("core", false, "filemode"),
    ("core", false, "logallrefupdates"),
    ("core", false, "ignorecase"),
    ("core", false, "precomposeunicode"),
    ("core", false, "symlinks"),
    ("core", false, "autocrlf"),
    ("core", false, "eol"),
    ("core", false, "safecrlf"),
    ("remote", true, "url"),
    ("remote", true, "pushurl"),
    ("remote", true, "fetch"),
    ("remote", true, "push"),
    ("remote", true, "tagopt"),
    ("remote", true, "prune"),
    ("branch", true, "remote"),
    ("branch", true, "merge"),
    ("branch", true, "rebase"),
    ("branch", true, "pushremote"),
    ("user", false, "name"),
    ("user", false, "email"),
    ("init", false, "defaultbranch"),
    ("pull", false, "rebase"),
    ("push", false, "default"),
    ("fetch", false, "prune"),
    ("extensions", false, "objectformat"),
];

/// Whether `text`, one config file, sets only [`ALLOWED`] keys, with
/// `core.bare` only false. Anything this scan cannot read for sure is not
/// plain: a byte order mark, a backslash anywhere (a continued line or an
/// escape could hide a header or a key), a control character, a key outside
/// a section, a broken header or key name. An `include` or `includeIf` is
/// no allowed key either.
pub fn is_plain(text: &str) -> bool {
    if text.starts_with('\u{feff}') || text.contains('\\') {
        return false;
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return false;
    }
    let mut section: Option<(String, bool)> = None;
    for raw in text.lines() {
        let mut line = raw.trim();
        if let Some(rest) = line.strip_prefix('[') {
            let Some((header, after)) = header(rest) else {
                return false;
            };
            section = Some(header);
            line = after.trim();
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((name, sub)) = section.as_ref() else {
            return false;
        };
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), Some(value.trim())),
            None => (line, None),
        };
        if !is_name(key, false) {
            return false;
        }
        let key = key.to_ascii_lowercase();
        let bare_false = name == "core" && !sub && key == "bare" && is_false(value);
        if !bare_false && !ALLOWED.contains(&(name.as_str(), *sub, key.as_str())) {
            return false;
        }
    }
    true
}

/// The section of a header (`rest` follows its `[`): its name in lower case
/// and whether it has a subsection, and the text after the `]`.
fn header(rest: &str) -> Option<((String, bool), &str)> {
    if let Some(quote) = rest.find('"') {
        let name = rest[..quote].trim();
        let end = quote + 1 + rest[quote + 1..].find('"')?;
        let after = rest[end + 1..].trim_start().strip_prefix(']')?;
        return is_name(name, false).then(|| ((name.to_ascii_lowercase(), true), after));
    }
    let (inner, after) = rest.split_once(']')?;
    let inner = inner.trim();
    if !is_name(inner, true) {
        return None;
    }
    let (name, sub) = match inner.split_once('.') {
        Some((name, _)) => (name, true),
        None => (inner, false),
    };
    Some(((name.to_ascii_lowercase(), sub), after))
}

/// Whether a boolean value is plainly false.
fn is_false(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.split(['#', ';']).next().unwrap_or("").trim();
        ["false", "no", "off", "0"]
            .iter()
            .any(|f| v.eq_ignore_ascii_case(f))
    })
}

/// A section or key name: ASCII letters, digits and `-`, starting with a
/// letter; `dotted` allows the `.` of a legacy subsection header.
fn is_name(name: &str, dotted: bool) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || (dotted && b == b'.'))
}

#[cfg(test)]
mod tests;
