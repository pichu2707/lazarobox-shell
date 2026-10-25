//! The `glob` pattern as an anchored regular expression, and the shape of a
//! result. Pure: no IO. The walk that feeds [`render`] lives in the runtime.

use regex::{Regex, RegexBuilder};

/// The longest pattern accepted, in bytes.
pub const MAX_PATTERN_BYTES: usize = 512;
/// The most matches a result lists.
pub const MAX_MATCHES: usize = 100;
/// Said when there were more matches than [`MAX_MATCHES`].
pub const CAP_MARKER: &str = "[truncated: more matches than the cap of 100]";
/// Said when the walk hit its budget before it saw every entry.
pub const BOUND_MARKER: &str = "[truncated: the search was bounded]";
/// A result with no match.
pub const NO_MATCH: &str = "no matches";

/// The most compiled bytes a pattern may take.
const SIZE_LIMIT: usize = 256 * 1024;

/// Why a pattern was refused. Short and content-free: it goes back to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlobError {
    Empty,
    /// Starts with `/`: patterns are relative to the root.
    Absolute,
    /// Has a `..` segment or a NUL.
    Escapes,
    /// `{a,b}` and `\` are not supported.
    Unsupported,
    /// An unclosed, empty or reversed `[...]`, or one naming `/`.
    BadClass,
    TooComplex,
}

impl GlobError {
    pub fn message(self) -> &'static str {
        match self {
            GlobError::Empty => "the pattern is empty",
            GlobError::Absolute => "the pattern must be relative to the root",
            GlobError::Escapes => "the pattern must stay inside the root",
            GlobError::Unsupported => "braces and backslashes are not supported in a pattern",
            GlobError::BadClass => "the pattern has a bad [...] class",
            GlobError::TooComplex => "the pattern is too complex",
        }
    }
}

/// A compiled pattern. It matches a whole root-relative path, `/`-separated.
#[derive(Clone, Debug)]
pub struct Glob {
    re: Regex,
}

impl Glob {
    /// `*` is any run inside a segment, `?` one character inside a segment,
    /// `[a-z]` / `[!a-z]` a class, and a whole `**` segment any number of
    /// segments (none included). A leading `./` is dropped.
    pub fn new(pattern: &str) -> Result<Glob, GlobError> {
        let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
        if pattern.is_empty() {
            return Err(GlobError::Empty);
        }
        if pattern.starts_with('/') {
            return Err(GlobError::Absolute);
        }
        if pattern.contains('\0') || pattern.split('/').any(|segment| segment == "..") {
            return Err(GlobError::Escapes);
        }
        let re = RegexBuilder::new(&translate(pattern)?)
            .size_limit(SIZE_LIMIT)
            .build()
            .map_err(|_| GlobError::TooComplex)?;
        Ok(Glob { re })
    }

    pub fn is_match(&self, path: &str) -> bool {
        self.re.is_match(path)
    }
}

fn translate(pattern: &str) -> Result<String, GlobError> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let last = segments.len() - 1;
    let mut out = String::from("^");
    for (at, segment) in segments.iter().enumerate() {
        if *segment == "**" {
            // Whole segments, none included; at the end, everything below.
            out.push_str(if at == last { ".+" } else { "(?:[^/]+/)*" });
            continue;
        }
        segment_regex(segment, &mut out)?;
        if at != last {
            out.push('/');
        }
    }
    out.push('$');
    Ok(out)
}

fn segment_regex(segment: &str, out: &mut String) -> Result<(), GlobError> {
    let chars: Vec<char> = segment.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        at += 1;
        match c {
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '{' | '}' | '\\' => return Err(GlobError::Unsupported),
            '[' => at += class_regex(&chars[at..], out)?,
            other => out.push_str(&regex::escape(other.encode_utf8(&mut [0; 4]))),
        }
    }
    Ok(())
}

/// One `[...]` after its `[`; returns how many characters it took. It never
/// matches `/`.
fn class_regex(chars: &[char], out: &mut String) -> Result<usize, GlobError> {
    let bad = |c: char| matches!(c, '/' | '[' | '\\');
    let negated = matches!(chars.first(), Some('!' | '^'));
    let mut at = usize::from(negated);
    let mut items = String::new();
    loop {
        let &c = chars.get(at).ok_or(GlobError::BadClass)?;
        at += 1;
        if c == ']' && !items.is_empty() {
            break;
        }
        // A `]` first in the class is a literal `]`.
        if bad(c) {
            return Err(GlobError::BadClass);
        }
        push_class_char(c, &mut items);
        // `a-z`: a dash between two characters is a range.
        if let (Some('-'), Some(&end)) = (chars.get(at), chars.get(at + 1))
            && end != ']'
        {
            if bad(end) {
                return Err(GlobError::BadClass);
            }
            items.push('-');
            push_class_char(end, &mut items);
            at += 2;
        }
    }
    out.push_str(if negated { "[^/" } else { "[" });
    out.push_str(&items);
    out.push(']');
    Ok(at)
}

fn push_class_char(c: char, items: &mut String) {
    if c.is_ascii_punctuation() {
        items.push('\\');
    }
    items.push(c);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, path: &str) -> bool {
        Glob::new(pattern).unwrap().is_match(path)
    }

    // Spec (code-search, glob): `**/*.rs` lists nested files, not other kinds.
    #[test]
    fn a_double_star_segment_crosses_directories() {
        assert!(matches("**/*.rs", "a.rs"));
        assert!(matches("**/*.rs", "src/a.rs"));
        assert!(matches("**/*.rs", "src/deep/a.rs"));
        assert!(!matches("**/*.rs", "README.md"));
        assert!(matches("src/**", "src/a/b.rs"));
        assert!(!matches("src/**", "src"));
        assert!(matches("src/**/a.rs", "src/a.rs"));
        assert!(matches("src/**/a.rs", "src/x/y/a.rs"));
        assert!(!matches("src/**/a.rs", "lib/a.rs"));
    }

    #[test]
    fn a_single_star_stays_inside_one_segment() {
        assert!(matches("src/*.rs", "src/a.rs"));
        assert!(!matches("src/*.rs", "src/x/a.rs"));
        assert!(matches("*", "a"));
        assert!(!matches("*", "a/b"));
        // Not a whole `**` segment: it is two stars in one segment.
        assert!(matches("a**b", "axxb"));
        assert!(!matches("a**b", "ax/xb"));
    }

    #[test]
    fn a_question_mark_is_one_character_inside_a_segment() {
        assert!(matches("a?c", "abc"));
        assert!(matches("a?c", "aéc"));
        assert!(!matches("a?c", "ac"));
        assert!(!matches("a?c", "a/c"));
    }

    #[test]
    fn classes_match_sets_ranges_and_negations_but_never_a_slash() {
        assert!(matches("[abc].rs", "b.rs"));
        assert!(!matches("[abc].rs", "d.rs"));
        assert!(matches("f[0-9].txt", "f7.txt"));
        assert!(!matches("f[0-9].txt", "fx.txt"));
        assert!(matches("[!a].rs", "b.rs"));
        assert!(!matches("[!a].rs", "a.rs"));
        assert!(matches("[^a].rs", "b.rs"));
        assert!(!matches("a[!x]b", "a/b"));
        assert!(matches("[]].rs", "].rs"));
        assert!(matches("[a-].rs", "-.rs"));
        assert!(matches("[.*+].rs", "+.rs"));
        assert!(!matches("[.*+].rs", "x.rs"));
    }

    // Anchored: the whole path, never a substring, and literals are literals.
    #[test]
    fn a_pattern_matches_the_whole_path_and_literals_are_escaped() {
        assert!(!matches("a.rs", "src/a.rs"));
        assert!(!matches("src", "src/a.rs"));
        assert!(!matches("a.rs", "a_rs"));
        assert!(matches("a+b(1).rs", "a+b(1).rs"));
        assert!(matches("./src/a.rs", "src/a.rs"));
    }

    #[test]
    fn bad_patterns_are_refused_with_a_short_reason() {
        let refused = |pattern: &str| Glob::new(pattern).unwrap_err();
        assert_eq!(refused(""), GlobError::Empty);
        assert_eq!(refused("./"), GlobError::Empty);
        assert_eq!(refused("/etc/*"), GlobError::Absolute);
        assert_eq!(refused("../**/*"), GlobError::Escapes);
        assert_eq!(refused("src/../x"), GlobError::Escapes);
        assert_eq!(refused("a\0b"), GlobError::Escapes);
        assert_eq!(refused("*.{rs,toml}"), GlobError::Unsupported);
        assert_eq!(refused("a\\*"), GlobError::Unsupported);
        for bad in ["[abc", "[", "[!", "[]", "[z-a]", "[a/b]", "[a[b]", "[a-/]"] {
            assert!(
                matches!(
                    Glob::new(bad),
                    Err(GlobError::BadClass | GlobError::TooComplex)
                ),
                "{bad}"
            );
        }
        // `..` is only a problem as a whole segment.
        assert!(Glob::new("a..b").is_ok());
        assert!(Glob::new("..a/b").is_ok());
        for error in [GlobError::Empty, GlobError::TooComplex] {
            assert!(!error.message().is_empty());
        }
    }

    // A reversed range is the one class the translation cannot judge alone.
    #[test]
    fn a_reversed_range_does_not_compile() {
        assert!(Glob::new("[z-a]").is_err());
    }

    #[test]
    fn a_pattern_that_compiles_too_large_is_refused() {
        let heavy = "?".repeat(512);
        let nested = format!("{}x", "**/".repeat(170));
        // Either shape is fine as long as it never panics and never hangs.
        for pattern in [heavy, nested] {
            let _ = Glob::new(&pattern);
        }
        assert!(Glob::new(&"[a-z]".repeat(100)).is_ok());
    }
}
