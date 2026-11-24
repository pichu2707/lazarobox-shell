//! The tables of the grammar of [`super::parse`]: one [`Spec`] per head.

use std::ops::Range;

use super::{MAX_WORDS, Verb};

/// Which engines run a command with no prompt. The wide list is for engines
/// that are not local: a local one keeps today's seven verbs and asks for the
/// rest (design D3); `y` runs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tier {
    /// Every engine runs it: `cd pwd ls tree grep rg find`.
    Base,
    /// Cloud engines only.
    #[allow(dead_code)] // The first Extended verbs come with the next slice.
    Extended,
}

/// What the word after a flag of [`Spec::valued`] must be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Value {
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
pub(super) struct Spec {
    /// The head: the leading words of the command, separated by one space
    /// (`cd`, later `git status`). The longest matching head wins.
    pub(super) name: &'static str,
    /// Who may run it with no prompt.
    pub(super) tier: Tier,
    pub(super) verb: Verb,
    /// Short flags that may be bundled (`-la`).
    pub(super) flags: &'static [u8],
    /// Whole flag words (`--line-number`).
    pub(super) long: &'static [&'static str],
    /// Flags that take the next word as their value.
    pub(super) valued: &'static [(&'static str, Value)],
    /// The first word that is not a flag is a pattern, not a path.
    pub(super) pattern: bool,
    /// How many path arguments.
    pub(super) paths: Range<usize>,
}

/// The defaults every spec in [`SPECS`] starts from and overrides: no flag,
/// no pattern, and up to [`MAX_WORDS`] paths. `name` and `verb` are
/// placeholders; every spec sets its own.
pub(super) const BASE: Spec = Spec {
    name: "",
    tier: Tier::Base,
    verb: Verb::Pwd,
    flags: b"",
    long: &[],
    valued: &[],
    pattern: false,
    paths: 0..MAX_WORDS,
};

/// The context flags of `grep` and `rg`, and the match cap.
pub(super) const CONTEXT: [(&str, Value); 4] = [
    ("-A", Value::Number),
    ("-B", Value::Number),
    ("-C", Value::Number),
    ("-m", Value::Number),
];

pub(super) const SPECS: [Spec; 7] = [
    Spec {
        name: "cd",
        verb: Verb::Cd,
        paths: 0..2,
        ..BASE
    },
    Spec {
        name: "pwd",
        verb: Verb::Pwd,
        paths: 0..1,
        ..BASE
    },
    Spec {
        name: "ls",
        verb: Verb::Ls,
        flags: b"lahtr1",
        paths: 0..5,
        ..BASE
    },
    Spec {
        name: "tree",
        verb: Verb::Tree,
        flags: b"d",
        valued: &[("-L", Value::Depth)],
        paths: 0..2,
        ..BASE
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
        ..BASE
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
        paths: 0..MAX_WORDS,
        ..BASE
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
        paths: 0..MAX_WORDS,
        ..BASE
    },
];
