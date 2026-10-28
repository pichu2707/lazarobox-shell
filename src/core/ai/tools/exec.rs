//! The grammar of the commands that `run` mode may type AND execute.
//!
//! Allow-list, no deny-list: navigation (`cd`, `pwd`, `ls`, `tree`) and
//! search (`grep`, `rg`, `find`, `git status|log|diff`), each with only the
//! flags of [`SPECS`] or [`GIT_SPECS`], and at most [`MAX_WORDS`] words. No
//! verb writes, and no allowed flag runs another program or writes a file
//! (`find -exec`/`-delete`/`-fprint`, `rg --pre`/`-z`, `git -c`/`--output`/
//! `--ext-diff` are not in the lists). A word is unquoted
//! (ASCII letters and digits, Latin letters U+00C0..=U+024F without the
//! multiplication and division signs, and ``. _ / @ % + : , = ~ -``; a `~`
//! only leads a word) or a whole
//! single-quoted `'...'` or double-quoted `"..."` word holding only those
//! characters and the space, so a quoted word is inert even when the shell is
//! inside an open quote or `$(` (a continuation prompt); words are separated by exactly one ASCII space.
//! Anything else, shell metacharacters, controls, format characters, other
//! whitespace and combining marks included, is a [`Fallback`] to the prompt.
//!
//! Invariant: checked bytes == written bytes. [`parse`] never rewrites, it
//! borrows slices of its input, and every accepted text is a fixed point of
//! [`validate`](super::command::validate). The decision is pure: it reads no
//! environment, and the parser allocates nothing.

use std::ops::Range;

use super::command::{MAX_COMMAND_BYTES, is_invisible};
use super::paths::Want;
use crate::core::ai::sanitize::is_format;

/// Why a git argument does not run on its own: it names a revision, not a
/// path. The model did nothing wrong, so it never counts toward the pause.
pub const REVISION: &str = "a git argument is a revision, not an existing path";

/// The most words (the verb included) in a runnable command.
pub const MAX_WORDS: usize = 8;

/// Bit set in [`CLASS`] for a byte allowed in an unquoted word.
const WORD: u8 = 1;
/// Bit set in [`CLASS`] for a byte allowed inside single or double quotes.
const QUOTED: u8 = 2;

/// Classifies the ASCII bytes. Non-ASCII bytes never index it: they start a
/// UTF-8 decode that accepts only Latin letters, so no mark, joiner, filler,
/// other script or symbol can reach a shell.
const CLASS: [u8; 128] = {
    let mut table = [0; 128];
    let mut b = 0;
    while b < 128 {
        let c = b as u8;
        if c.is_ascii_alphanumeric()
            || matches!(c, b'.' | b'_' | b'/' | b'@' | b'%' | b'+')
            || matches!(c, b':' | b',' | b'=' | b'~' | b'-')
        {
            table[b] |= WORD | QUOTED;
        }
        // The space is the only extra byte a quote holds: a quoted word is
        // inert text, even inside an open quote, `$(` or heredoc.
        if c == b' ' {
            table[b] |= QUOTED;
        }
        b += 1;
    }
    table
};

/// The verbs of the safe list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verb {
    Cd,
    Pwd,
    Ls,
    Tree,
    Grep,
    Rg,
    Find,
    /// `git status`, `git log` or `git diff`.
    Git,
}

/// What the word after a flag of [`Spec::valued`] must be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Value {
    /// One or two digits (`tree -L`).
    Depth,
    /// One to three digits.
    Number,
    /// Any word that does not start with a dash.
    Name,
    /// `f`, `d` or `l` (`find -type`).
    Kind,
}

/// What a verb accepts.
struct Spec {
    name: &'static str,
    verb: Verb,
    /// Short flags that may be bundled (`-la`).
    flags: &'static [u8],
    /// Whole flag words (`--oneline`).
    long: &'static [&'static str],
    /// Flags that take the next word as their value.
    valued: &'static [(&'static str, Value)],
    /// The first word that is not a flag is a pattern, not a path.
    pattern: bool,
    /// How many path arguments.
    paths: Range<usize>,
}

const NONE: Spec = Spec {
    name: "",
    verb: Verb::Pwd,
    flags: b"",
    long: &[],
    valued: &[],
    pattern: false,
    paths: 0..MAX_WORDS,
};

/// The context flags of `grep` and `rg`, and the match cap.
const CONTEXT: [(&str, Value); 4] = [
    ("-A", Value::Number),
    ("-B", Value::Number),
    ("-C", Value::Number),
    ("-m", Value::Number),
];

const SPECS: [Spec; 7] = [
    Spec {
        name: "cd",
        verb: Verb::Cd,
        paths: 0..2,
        ..NONE
    },
    Spec {
        name: "pwd",
        verb: Verb::Pwd,
        paths: 0..1,
        ..NONE
    },
    Spec {
        name: "ls",
        verb: Verb::Ls,
        flags: b"lahtr1",
        paths: 0..5,
        ..NONE
    },
    Spec {
        name: "tree",
        verb: Verb::Tree,
        flags: b"d",
        valued: &[("-L", Value::Depth)],
        paths: 0..2,
        ..NONE
    },
    // No `-R` (it follows links below) and no `-f` (patterns from a file).
    // A path is required: without one, grep reads the terminal and waits.
    Spec {
        name: "grep",
        verb: Verb::Grep,
        flags: b"inlcwvFEHhsoxr",
        valued: &CONTEXT,
        pattern: true,
        paths: 1..MAX_WORDS,
        ..NONE
    },
    // No `--pre`, `-z` (both run programs), `-L`, `-u`, `--hidden` or
    // `--no-ignore` (they widen what is read), `-f` or `-r`.
    Spec {
        name: "rg",
        verb: Verb::Rg,
        flags: b"isSnNlcwvFxHo",
        long: &[
            "--ignore-case",
            "--smart-case",
            "--line-number",
            "--count",
            "--files-with-matches",
            "--fixed-strings",
            "--word-regexp",
            "--no-heading",
        ],
        valued: &[
            CONTEXT[0],
            CONTEXT[1],
            CONTEXT[2],
            CONTEXT[3],
            ("-t", Value::Name),
            ("-T", Value::Name),
            ("--max-depth", Value::Number),
        ],
        pattern: true,
        ..NONE
    },
    // Tests and depth only: no action (`-exec`, `-delete`, `-fprint`, `-ok`)
    // and no `-L`.
    Spec {
        name: "find",
        verb: Verb::Find,
        valued: &[
            ("-name", Value::Name),
            ("-iname", Value::Name),
            ("-type", Value::Kind),
            ("-maxdepth", Value::Number),
            ("-mindepth", Value::Number),
        ],
        ..NONE
    },
];

/// The git subcommands, after `git` and an optional `--no-pager`. No global
/// option other than that one: `-c`, `-C`, `--exec-path`, `--git-dir` and
/// `--paginate` are refused as the subcommand.
const GIT_SPECS: [Spec; 3] = [
    Spec {
        name: "status",
        verb: Verb::Git,
        flags: b"sb",
        long: &["--short", "--branch", "--porcelain"],
        ..NONE
    },
    Spec {
        name: "log",
        verb: Verb::Git,
        flags: b"p",
        long: &[
            "--oneline",
            "--stat",
            "--graph",
            "--decorate",
            "--all",
            "--patch",
            "--name-only",
            "--name-status",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
        ],
        valued: &[("-n", Value::Number)],
        ..NONE
    },
    Spec {
        name: "diff",
        verb: Verb::Git,
        long: &[
            "--stat",
            "--shortstat",
            "--cached",
            "--staged",
            "--name-only",
            "--name-status",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
        ],
        ..NONE
    },
];

/// What a word of an accepted command is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    /// The verb, `--no-pager` and a git subcommand.
    Verb,
    Flag,
    /// The word after a valued flag.
    Value,
    Pattern,
    Path,
}

/// Why a command is not run directly: the byte span of the offending token
/// and a short reason. The reason never carries the text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fallback {
    pub at: Range<usize>,
    pub why: &'static str,
}

/// A command that passed the grammar: the exact text to show and to write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SafeCommand<'a> {
    text: &'a str,
    verb: Verb,
    /// Every word; a quoted one is held without its quotes.
    words: [&'a str; MAX_WORDS],
    roles: [Role; MAX_WORDS],
    len: usize,
}

impl<'a> SafeCommand<'a> {
    pub fn as_str(&self) -> &'a str {
        self.text
    }

    pub fn verb(&self) -> Verb {
        self.verb
    }

    /// Every path the command touches and how: the shell's cwd first (every
    /// verb but `cd` resolves against it), then the path arguments, or `~`
    /// for a bare `cd`. The pattern of a search and the values of flags are
    /// no paths.
    pub fn targets(&self) -> impl Iterator<Item = Target<'a>> + use<'a> {
        let (roles, words) = (self.roles, self.words);
        let paths = (0..self.len).filter(move |&k| roles[k] == Role::Path);
        let none = paths.clone().next().is_none();
        let recursive = (1..self.len).any(|k| {
            self.roles[k] == Role::Flag && !self.words[k].starts_with("--") && {
                self.words[k].contains('r')
            }
        });
        // What the arguments are for: names only, or the text of files.
        let (want, searched) = match self.verb {
            Verb::Cd | Verb::Pwd | Verb::Ls | Verb::Find => (Want::Existing, false),
            Verb::Tree | Verb::Git => (Want::Home, false),
            Verb::Grep => (Want::Home, recursive),
            Verb::Rg => (Want::Home, true),
        };
        // The pane's directory: what a command with no path acts on. Git
        // always reads the repository it is in; `pwd` reads no file and goes
        // nowhere, so it runs wherever the pane is.
        let acts = (none && self.verb != Verb::Pwd) || self.verb == Verb::Git;
        let cwd = match (self.verb, none) {
            (Verb::Cd, true) => Some(Target::new("~", want, true, false)),
            (Verb::Cd, false) => None,
            (Verb::Tree, _) => Some(Target::new(".", want, none, false)),
            _ if acts => Some(Target::new(".", want, true, none && searched)),
            _ => Some(Target::new(".", Want::Existing, false, false)),
        };
        cwd.into_iter()
            .chain(paths.map(move |k| Target::new(words[k], want, true, searched)))
    }
}

/// One path a [`SafeCommand`] touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target<'a> {
    /// As written, without quotes.
    pub input: &'a str,
    pub want: Want,
    /// The command acts on it: it must stay in the chat's root.
    pub confined: bool,
    /// A search reads the files below it (a folder is walked).
    pub searched: bool,
}

impl<'a> Target<'a> {
    fn new(input: &'a str, want: Want, confined: bool, searched: bool) -> Self {
        Self {
            input,
            want,
            confined,
            searched,
        }
    }
}

/// Decides whether `text` may run without a prompt, in one pass.
pub fn parse(text: &str) -> Result<SafeCommand<'_>, Fallback> {
    read(text).map_err(|fallback| revision(text, fallback))
}

/// A git argument the grammar refuses for its `~` or `^` (`HEAD~1`,
/// `main^`) names a revision: it falls back with [`REVISION`], which is no
/// fault of the model. Any other fallback is kept as it is.
fn revision(text: &str, fallback: Fallback) -> Fallback {
    let token = text.get(fallback.at.clone()).unwrap_or("");
    let rejected = matches!(
        fallback.why,
        "~ only starts a word" | "character not allowed"
    );
    if text.starts_with("git ") && rejected && token.contains(['~', '^']) {
        Fallback {
            why: REVISION,
            ..fallback
        }
    } else {
        fallback
    }
}

fn read(text: &str) -> Result<SafeCommand<'_>, Fallback> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(fallback(MAX_COMMAND_BYTES..bytes.len(), "command too long"));
    }
    let mut words = [""; MAX_WORDS];
    let (mut len, mut i) = (0, 0);
    loop {
        let start = i;
        match bytes.get(i) {
            None if i == 0 => return Err(fallback(0..0, "empty command")),
            None => return Err(fallback(i - 1..i, "trailing space")),
            Some(b' ') => return Err(fallback(i..i + 1, "extra space")),
            _ => {}
        }
        let quote = if matches!(bytes[i], b'\'' | b'"') {
            bytes[i]
        } else {
            0
        };
        let quoted = quote != 0;
        if len == MAX_WORDS || (quoted && len == 0) {
            let why = if len == MAX_WORDS {
                "too many words"
            } else {
                "the command is quoted"
            };
            return Err(fallback(i..token_end(bytes, i), why));
        }
        i += usize::from(quoted);
        while let Some(&c) = bytes.get(i) {
            if c == if quoted { quote } else { b' ' } {
                break;
            }
            i += step(text, i, quoted)
                .map_err(|why| fallback(start..token_end(bytes, start), why))?;
        }
        words[len] = &text[start + usize::from(quoted)..i];
        let why = if quoted {
            let closed = bytes.get(i) == Some(&quote);
            i += 1;
            check_quoted(words[len], closed).or_else(|| {
                let after = bytes.get(i).filter(|&&c| c != b' ');
                after.map(|_| "text after a quote")
            })
        } else {
            check_word(words[len])
        };
        if let Some(why) = why {
            let end = if quoted {
                token_end(bytes, start)
            } else {
                i.min(bytes.len())
            };
            return Err(fallback(start..end, why));
        }
        len += 1;
        if i >= bytes.len() {
            return finish(text, words, len);
        }
        i += 1;
    }
}

fn fallback(at: Range<usize>, why: &'static str) -> Fallback {
    Fallback { at, why }
}

/// The end of the token that starts at `from`: a quoted word through its
/// closing quote and any text stuck to it, else up to the next ASCII space,
/// or the end of the text.
fn token_end(bytes: &[u8], from: usize) -> usize {
    let quote = bytes[from];
    let skip = usize::from(matches!(quote, b'\'' | b'"'));
    let mut at = from + skip;
    if skip == 1 {
        match bytes[at..].iter().position(|&b| b == quote) {
            Some(n) => at += n + 1,
            None => return bytes.len(),
        }
    }
    bytes[at..]
        .iter()
        .position(|&b| b == b' ')
        .map_or(bytes.len(), |n| at + n)
}

/// The byte length of the character at `i` if it may be part of a word.
fn step(text: &str, i: usize, quoted: bool) -> Result<usize, &'static str> {
    let c = text.as_bytes()[i];
    if c < 0x80 {
        let allowed = CLASS[usize::from(c)] & if quoted { QUOTED } else { WORD } != 0;
        return if allowed { Ok(1) } else { Err(why_ascii(c)) };
    }
    let ch = text[i..].chars().next().ok_or("character not allowed")?;
    if ('\u{c0}'..='\u{24f}').contains(&ch) && ch != '\u{d7}' && ch != '\u{f7}' {
        return Ok(ch.len_utf8());
    }
    Err(if ch.is_control() {
        "control character"
    } else if is_format(ch) || is_invisible(ch) {
        "invisible or non-ASCII space character"
    } else {
        "character not allowed"
    })
}

fn why_ascii(c: u8) -> &'static str {
    match c {
        0..=0x1f | 0x7f => "control character",
        b'"' => "double quote",
        b'\'' => "quote inside a word",
        b';' | b'&' | b'|' | b'<' | b'>' | b'(' | b')' | b'{' | b'}' | b'$' | b'`' | b'\\'
        | b'!' | b'*' | b'?' | b'[' | b']' | b'#' => "shell metacharacter",
        _ => "character not allowed",
    }
}

/// Word rules that look at the whole unquoted word.
fn check_word(word: &str) -> Option<&'static str> {
    match word.as_bytes()[0] {
        b'=' | b'+' | b'%' => Some("word starts with a shell-special character"),
        b'~' if word != "~" && !word.starts_with("~/") => Some("~user is not supported"),
        _ if word.bytes().skip(1).any(|b| b == b'~') => Some("~ only starts a word"),
        b'-' if word == "-" || word == "--" => Some("a bare dash is not a path"),
        _ => None,
    }
}

/// Rules for the content of a quoted word; `closed` says the closing
/// quote was found.
fn check_quoted(content: &str, closed: bool) -> Option<&'static str> {
    match content.as_bytes().first() {
        _ if !closed => Some("unterminated quote"),
        None => Some("empty quoted word"),
        Some(b'~') => Some("quoted word starts with ~"),
        Some(b'-') => Some("quoted word starts with a dash"),
        _ => None,
    }
}

/// Checks the verb, its flags and their values, the pattern and the number
/// of paths.
fn finish<'a>(
    text: &'a str,
    words: [&'a str; MAX_WORDS],
    len: usize,
) -> Result<SafeCommand<'a>, Fallback> {
    let at = |w: &str| {
        let start = w.as_ptr() as usize - text.as_ptr() as usize;
        start..start + w.len()
    };
    let mut roles = [Role::Verb; MAX_WORDS];
    let (spec, mut k) = if words[0] == "git" {
        let k = 1 + usize::from(len > 1 && words[1] == "--no-pager");
        if k >= len {
            return Err(fallback(at(words[0]), "missing argument"));
        }
        let Some(spec) = GIT_SPECS.iter().find(|spec| spec.name == words[k]) else {
            return Err(fallback(at(words[k]), "not a safe git command"));
        };
        (spec, k + 1)
    } else {
        let Some(spec) = SPECS.iter().find(|spec| spec.name == words[0]) else {
            let why = if words[0].contains('=') {
                "environment assignment"
            } else {
                "not a safe command"
            };
            return Err(fallback(at(words[0]), why));
        };
        (spec, 1)
    };
    let (mut paths, mut pattern) = (0, false);
    while k < len {
        let word = words[k];
        roles[k] = Role::Flag;
        k += 1;
        if let Some(&(_, value)) = spec.valued.iter().find(|(flag, _)| *flag == word) {
            let next = words[..len].get(k).copied();
            if let Some(why) = bad_value(value, next) {
                return Err(fallback(at(word), why));
            }
            roles[k] = Role::Value;
            k += 1;
        } else if word.starts_with("--") {
            if !spec.long.contains(&word) {
                return Err(fallback(at(word), "flag not allowed"));
            }
        } else if let Some(flags) = word.strip_prefix('-') {
            if !flags.bytes().all(|b| spec.flags.contains(&b)) {
                return Err(fallback(at(word), "flag not allowed"));
            }
        } else if spec.pattern && !pattern {
            roles[k - 1] = Role::Pattern;
            pattern = true;
        } else {
            roles[k - 1] = Role::Path;
            paths += 1;
        }
    }
    match paths {
        _ if spec.pattern && !pattern => Err(fallback(at(words[0]), "missing argument")),
        n if n < spec.paths.start => Err(fallback(at(words[0]), "missing argument")),
        n if !spec.paths.contains(&n) => Err(fallback(at(words[0]), "too many arguments")),
        _ => Ok(SafeCommand {
            text,
            verb: spec.verb,
            words,
            roles,
            len,
        }),
    }
}

/// Why the word after a valued flag is not what it takes; `None` when it is.
/// A value never starts with a dash, so no flag hides behind one.
fn bad_value(value: Value, next: Option<&str>) -> Option<&'static str> {
    let digits =
        |n: &str, most| (1..=most).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit());
    let next = next.unwrap_or("");
    match value {
        Value::Depth if !digits(next, 2) => Some("-L needs a depth of 1 or 2 digits"),
        Value::Number if !digits(next, 3) => Some("the flag needs a number"),
        Value::Name if next.is_empty() || next.starts_with('-') => Some("the flag needs a value"),
        Value::Kind if !matches!(next, "f" | "d" | "l") => Some("-type needs f, d or l"),
        _ => None,
    }
}

pub mod git_config;

#[cfg(test)]
mod tests;
