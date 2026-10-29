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

/// The text a `glob` call returns for the root-relative `paths` it found:
/// sorted, once each, at most [`MAX_MATCHES`], one per line. `bounded` says
/// the walk hit its budget; each cap that was hit adds its marker.
pub fn render(mut paths: Vec<String>, bounded: bool) -> String {
    paths.sort_unstable();
    paths.dedup();
    let capped = paths.len() > MAX_MATCHES;
    paths.truncate(MAX_MATCHES);
    if paths.is_empty() {
        paths.push(NO_MATCH.to_owned());
    }
    if capped {
        paths.push(CAP_MARKER.to_owned());
    }
    if bounded {
        paths.push(BOUND_MARKER.to_owned());
    }
    paths.join("\n")
}

fn translate(pattern: &str) -> Result<String, GlobError> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let last = segments.len() - 1;
    let mut out = String::from("^");
    for (at, segment) in segments.iter().enumerate() {
        if *segment == "**" {
            // Whole segments, none included; at the end, everything below.
            out.push_str(if at == last { "(?s:.+)" } else { "(?:[^/]+/)*" });
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
            if bad(end) || end < c {
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
            assert!(matches!(Glob::new(bad), Err(GlobError::BadClass)), "{bad}");
        }
        // `..` is only a problem as a whole segment.
        assert!(Glob::new("a..b").is_ok());
        assert!(Glob::new("..a/b").is_ok());
        for error in [GlobError::Empty, GlobError::TooComplex] {
            assert!(!error.message().is_empty());
        }
    }

    #[test]
    fn a_reversed_range_is_a_bad_class() {
        assert_eq!(Glob::new("[z-a]").unwrap_err(), GlobError::BadClass);
        assert_eq!(Glob::new("[!z-a]").unwrap_err(), GlobError::BadClass);
        assert!(Glob::new("[a-a]").is_ok());
    }

    // A trailing `**` is everything below, even a name with a newline.
    #[test]
    fn a_trailing_double_star_matches_a_newline_in_a_name() {
        assert!(matches("src/**", "src/a\nb.rs"));
        assert!(matches("src/**", "src/d/a\n"));
        assert!(matches("src/*", "src/a\nb.rs"));
        assert!(!matches("src/*", "src/a\n/b"));
    }

    #[test]
    fn a_pattern_that_compiles_too_large_is_refused() {
        // Within MAX_PATTERN_BYTES, long runs of `*` or `?` pass the size limit.
        for pattern in ["*".repeat(512), "?".repeat(512), "?*".repeat(256)] {
            assert_eq!(Glob::new(&pattern).unwrap_err(), GlobError::TooComplex);
        }
        // Nested `**/` stays under it.
        assert!(Glob::new(&format!("{}x", "**/".repeat(170))).is_ok());
        assert!(Glob::new(&"[a-z]".repeat(100)).is_ok());
    }

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|n| format!("f{n:03}.rs")).collect()
    }

    // Spec (code-search, glob): names only, in a stable order.
    #[test]
    fn matches_are_sorted_and_listed_one_per_line() {
        let got = render(
            vec!["src/b.rs".into(), "README.md".into(), "src/a.rs".into()],
            false,
        );
        assert_eq!(got, "README.md\nsrc/a.rs\nsrc/b.rs");
    }

    #[test]
    fn a_path_found_twice_is_listed_once() {
        let got = render(vec!["a.rs".into(), "a.rs".into()], false);
        assert_eq!(got, "a.rs");
    }

    #[test]
    fn no_match_says_so() {
        assert_eq!(render(Vec::new(), false), NO_MATCH);
        assert_eq!(
            render(Vec::new(), true),
            format!("{NO_MATCH}\n{BOUND_MARKER}")
        );
    }

    // Spec: at most 100 matches, and a result that hits a cap says so.
    #[test]
    fn the_hundredth_match_fits_and_the_next_one_is_cut_with_a_marker() {
        let exact = render(names(MAX_MATCHES), false);
        assert_eq!(exact.lines().count(), MAX_MATCHES);
        assert!(!exact.contains("truncated"), "{exact}");

        let over = render(names(MAX_MATCHES + 1), false);
        let lines: Vec<&str> = over.lines().collect();
        assert_eq!(lines.len(), MAX_MATCHES + 1);
        assert_eq!(lines[MAX_MATCHES], CAP_MARKER);
        assert_eq!(lines[MAX_MATCHES - 1], "f099.rs");
    }

    // The cap keeps the smallest names, whatever order the walk found them.
    #[test]
    fn the_cap_keeps_the_first_names_in_sorted_order() {
        let mut found = names(MAX_MATCHES + 5);
        found.reverse();
        let got = render(found, false);
        assert!(got.starts_with("f000.rs\n"));
        assert!(!got.contains("f100.rs"));
    }

    // Spec: a budget stop is reported even when few matches were found.
    #[test]
    fn a_bounded_walk_is_reported() {
        assert_eq!(
            render(vec!["a.rs".into()], true),
            format!("a.rs\n{BOUND_MARKER}")
        );
        let both = render(names(MAX_MATCHES + 1), true);
        assert!(
            both.ends_with(&format!("{CAP_MARKER}\n{BOUND_MARKER}")),
            "{both}"
        );
    }
}
