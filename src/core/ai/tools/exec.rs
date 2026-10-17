//! The grammar of the commands that `run` mode may type AND execute.
//!
//! Allow-list, no deny-list: `cd`, `pwd`, `ls`, `tree` and `mkdir`, with the
//! flags of [`SPECS`] and at most [`MAX_WORDS`] words. A word is unquoted
//! (ASCII letters and digits, Latin letters U+00C0..=U+024F without the
//! multiplication and division signs, and ``. _ / @ % + : , = ~ -``; a `~`
//! only leads a word) or a whole
//! single-quoted `'...'`; words are separated by exactly one ASCII space.
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

/// The most words (the verb included) in a runnable command.
pub const MAX_WORDS: usize = 8;

/// Bit set in [`CLASS`] for a byte allowed in an unquoted word.
const WORD: u8 = 1;
/// Bit set in [`CLASS`] for a byte allowed inside single quotes.
const QUOTED: u8 = 2;

/// Classifies the ASCII bytes. Non-ASCII bytes never index it: they start a
/// UTF-8 decode that accepts only Latin letters, so no mark, joiner, filler,
/// other script or symbol can reach a shell.
const CLASS: [u8; 128] = {
    let mut table = [0; 128];
    let mut b = 0;
    while b < 128 {
        let c = b as u8;
        if c >= b' ' && c < 0x7f && c != b'\'' && c != b'\\' {
            table[b] |= QUOTED;
        }
        if c.is_ascii_alphanumeric()
            || matches!(c, b'.' | b'_' | b'/' | b'@' | b'%' | b'+')
            || matches!(c, b':' | b',' | b'=' | b'~' | b'-')
        {
            table[b] |= WORD;
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
    Mkdir,
}

/// What a verb accepts: bundled short flags, whether `-L <n>` is allowed, and
/// how many path arguments.
struct Spec {
    name: &'static str,
    verb: Verb,
    flags: &'static [u8],
    depth: bool,
    paths: Range<usize>,
}

const fn spec(
    name: &'static str,
    verb: Verb,
    flags: &'static [u8],
    depth: bool,
    paths: Range<usize>,
) -> Spec {
    Spec {
        name,
        verb,
        flags,
        depth,
        paths,
    }
}

const SPECS: [Spec; 5] = [
    spec("cd", Verb::Cd, b"", false, 0..2),
    spec("pwd", Verb::Pwd, b"", false, 0..1),
    spec("ls", Verb::Ls, b"lahtr1", false, 0..5),
    spec("tree", Verb::Tree, b"da", true, 0..2),
    spec("mkdir", Verb::Mkdir, b"p", false, 1..5),
];

/// Why a command is not run directly: the byte span of the offending token
/// and a short reason. The reason never carries the text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fallback {
    pub at: Range<usize>,
    pub why: &'static str,
}

/// A command that passed the grammar: the exact text to show and to write.
#[derive(Clone, Copy, Debug)]
pub struct SafeCommand<'a> {
    text: &'a str,
    verb: Verb,
    /// Every word; a quoted one is held without its quotes.
    words: [&'a str; MAX_WORDS],
    len: usize,
}

impl<'a> SafeCommand<'a> {
    pub fn as_str(&self) -> &'a str {
        self.text
    }

    pub fn verb(&self) -> Verb {
        self.verb
    }

    /// Every path the command touches and how: the shell's cwd first (for
    /// every verb except `cd`, because `ls`, `tree`, `pwd` and a relative
    /// `mkdir` act on it), then the arguments, or `~` for a bare `cd`.
    pub fn targets(&self) -> Targets<'a> {
        Targets { cmd: *self, at: 0 }
    }
}

/// The iterator of [`SafeCommand::targets`].
pub struct Targets<'a> {
    cmd: SafeCommand<'a>,
    at: usize,
}

impl<'a> Iterator for Targets<'a> {
    type Item = (&'a str, Want);

    fn next(&mut self) -> Option<Self::Item> {
        let cmd = &self.cmd;
        if self.at == 0 {
            self.at = 1;
            match (cmd.verb, cmd.len) {
                (Verb::Cd, 1) => return Some(("~", Want::Existing)),
                (Verb::Cd, _) => {}
                _ => return Some((".", Want::Existing)),
            }
        }
        let want = if cmd.verb == Verb::Mkdir {
            Want::New
        } else {
            Want::Existing
        };
        while self.at < cmd.len {
            let word = cmd.words[self.at];
            self.at += 1;
            match word {
                "-L" => self.at += 1,
                w if w.starts_with('-') => {}
                w => return Some((w, want)),
            }
        }
        None
    }
}

/// Decides whether `text` may run without a prompt, in one pass.
pub fn parse(text: &str) -> Result<SafeCommand<'_>, Fallback> {
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
        let quoted = bytes[i] == b'\'';
        if len == MAX_WORDS || (quoted && len == 0) {
            let why = if quoted {
                "the command is quoted"
            } else {
                "too many words"
            };
            return Err(fallback(i..token_end(bytes, i), why));
        }
        i += usize::from(quoted);
        while let Some(&c) = bytes.get(i) {
            if c == if quoted { b'\'' } else { b' ' } {
                break;
            }
            i += step(text, i, quoted).map_err(|why| fallback(start..token_end(bytes, i), why))?;
        }
        words[len] = &text[start + usize::from(quoted)..i];
        let why = if quoted {
            let closed = bytes.get(i) == Some(&b'\'');
            i += 1;
            check_quoted(words[len], closed).or_else(|| {
                let after = bytes.get(i).filter(|&&c| c != b' ');
                after.map(|_| "text after a quote")
            })
        } else {
            check_word(words[len])
        };
        if let Some(why) = why {
            return Err(fallback(start..i.min(bytes.len()), why));
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

/// The end of the token that starts at or contains `from`: the next ASCII
/// space, or the end of the text.
fn token_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| b == b' ')
        .map_or(bytes.len(), |n| from + n)
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

/// Rules for the content of a single-quoted word; `closed` says the closing
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

/// Checks the verb, its flags and the number of paths.
fn finish<'a>(
    text: &'a str,
    words: [&'a str; MAX_WORDS],
    len: usize,
) -> Result<SafeCommand<'a>, Fallback> {
    let at = |w: &str| {
        let start = w.as_ptr() as usize - text.as_ptr() as usize;
        start..start + w.len()
    };
    let Some(spec) = SPECS.iter().find(|spec| spec.name == words[0]) else {
        let why = if words[0].contains('=') {
            "environment assignment"
        } else {
            "not a safe command"
        };
        return Err(fallback(at(words[0]), why));
    };
    let (mut paths, mut k) = (0, 1);
    while k < len {
        let word = words[k];
        k += 1;
        if let Some(flags) = word.strip_prefix('-') {
            if spec.depth && word == "-L" {
                let depth = words[..len].get(k).filter(|d| (1..=2).contains(&d.len()));
                if !depth.is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())) {
                    return Err(fallback(at(word), "-L needs a depth of 1 or 2 digits"));
                }
                k += 1;
            } else if !flags.bytes().all(|b| spec.flags.contains(&b)) {
                return Err(fallback(at(word), "flag not allowed"));
            }
        } else {
            paths += 1;
        }
    }
    match paths {
        n if n < spec.paths.start => Err(fallback(at(words[0]), "missing argument")),
        n if !spec.paths.contains(&n) => Err(fallback(at(words[0]), "too many arguments")),
        _ => Ok(SafeCommand {
            text,
            verb: spec.verb,
            words,
            len,
        }),
    }
}

#[cfg(test)]
mod tests;
