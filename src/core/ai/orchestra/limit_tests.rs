use super::limit::{LIMIT, count, from_diff};
use crate::core::ai::tools::diff::{MAX_ROWS, diff_within};
use std::time::Duration;

fn rows(n: usize, line: &str) -> String {
    (0..n).map(|i| format!("{line} {i}\n")).collect()
}

#[test]
fn a_create_counts_every_added_code_row() {
    let c = count("src/a.rs", None, &rows(12, "let x ="));
    assert_eq!(c.lines, 12);
    assert!(!c.every_line && !c.test);
}

#[test]
fn the_limit_is_400_and_401_goes_over() {
    assert_eq!(LIMIT, 400);
    assert!(!count("src/a.rs", None, &rows(400, "let x =")).over());
    assert!(count("src/a.rs", None, &rows(401, "let x =")).over());
}

#[test]
fn an_edit_counts_only_the_rows_it_adds() {
    let base = rows(2000, "let x =");
    let new = format!("{base}{}", rows(12, "let y ="));
    assert_eq!(count("src/a.rs", Some(&base), &new).lines, 12);
}

#[test]
fn blanks_and_whole_line_comments_do_not_count() {
    let mut t = rows(300, "let x =");
    t += &rows(150, "// note");
    t += &"\n".repeat(50);
    let c = count("src/a.rs", None, &t);
    assert_eq!(c.lines, 300);
    assert!(!c.over());
}

#[test]
fn doc_comments_do_not_count() {
    let t = "/// doc\n//! inner\n/** block doc */\nfn a() {}\n";
    assert_eq!(count("a.rs", None, t).lines, 1);
}

#[test]
fn a_trailing_comment_counts_as_code() {
    assert_eq!(count("a.rs", None, "let x = 1; // why\n").lines, 1);
}

#[test]
fn a_block_comment_is_tracked_across_added_rows() {
    let t = "/* open\nstill comment\nfn fake() {}\nclose */\nlet x = 1;\n";
    assert_eq!(count("a.rs", None, t).lines, 1);
    // Code after a closing marker counts.
    assert_eq!(count("a.rs", None, "/* a\nb */ let x = 1;\n").lines, 1);
    // A one-row block comment with code after it counts, without it does not.
    assert_eq!(count("a.rs", None, "/* a */ let x = 1;\n/* b */\n").lines, 1);
}

#[test]
fn each_language_family_has_its_markers() {
    for (path, comment) in [
        ("a.c", "// c"),
        ("a.go", "// go"),
        ("a.ts", "// ts"),
        ("a.java", "// java"),
        ("a.py", "# py"),
        ("a.sh", "# sh"),
        ("a.rb", "# rb"),
        ("a.toml", "# toml"),
        ("a.yaml", "# yaml"),
        ("a.sql", "-- sql"),
        ("a.lua", "-- lua"),
        ("a.hs", "-- hs"),
    ] {
        let t = format!("{comment}\ncode\n");
        let c = count(path, None, &t);
        assert_eq!((path, c.lines, c.every_line), (path, 1, false));
    }
    // A marker of another family is code: `#` in Rust, `//` in Python.
    assert_eq!(count("a.rs", None, "#[derive(Debug)]\n").lines, 1);
    assert_eq!(count("a.py", None, "// not a comment\n").lines, 1);
    assert_eq!(count("a.lua", None, "--[[ x\ny ]]\nz\n").lines, 1);
    assert_eq!(count("a.hs", None, "{- x\ny -}\nz\n").lines, 1);
    assert_eq!(count("a.sql", None, "/* x\ny */\nz\n").lines, 1);
}

#[test]
fn python_triple_quotes_count_only_when_a_line_starts_with_them() {
    let doc = "\"\"\"module doc\nmore\n\"\"\"\nx = 1\n";
    assert_eq!(count("a.py", None, doc).lines, 1);
    assert_eq!(count("a.py", None, "'''one row'''\nx = 1\n").lines, 1);
    // An assignment of a string is code, and so is its body: the opening
    // quotes do not start the line. The closing row does, so it is read as
    // the start of a docstring (the rule is by line start, as pinned).
    assert_eq!(count("a.py", None, "x = \"\"\"\nbody\n\"\"\"\n").lines, 2);
}

#[test]
fn an_unknown_language_counts_every_row_and_says_so() {
    let t = format!("{}\n\n{}", "// looks like a comment", rows(399, "x"));
    let c = count("notes.xyz", None, &t);
    assert_eq!(c.lines, 401);
    assert!(c.every_line && c.over());
    // No extension is unknown too.
    assert!(count("Makefile", None, "# a\n").every_line);
}

#[test]
fn a_truncated_diff_counts_every_added_line() {
    let t = rows(MAX_ROWS + 10, "// c");
    let c = count("a.rs", None, &t);
    assert_eq!(c.lines, MAX_ROWS + 10);
    assert!(c.every_line && c.over());
}

#[test]
fn a_coarse_diff_of_a_known_language_counts_every_added_line() {
    // Pinned (21.4): the diff gave up on being minimal, so its added rows
    // are the whole new file, and old lines would be credited as new ones.
    // Count them all and say so: an over-count only produces an ask.
    let base = rows(5000, "let x =");
    let new = format!("{base}// c\n");
    let d = diff_within(Some(&base), &new, Duration::ZERO);
    assert!(d.coarse);
    let c = from_diff("a.rs", &d);
    assert_eq!(c.lines, d.added);
    assert!(c.every_line);
}

#[test]
fn test_files_are_found_by_path_and_have_no_cap() {
    for path in [
        "tests/flow.rs",
        "crate/tests/deep/flow.rs",
        "a_test.go",
        "src/orchestra/run_tests.rs",
        "src/x/tests.rs",
        "a.spec.ts",
        "a.test.js",
    ] {
        let c = count(path, None, &rows(900, "x"));
        assert!(c.test && !c.over(), "{path}");
    }
}

#[test]
fn a_name_that_only_ends_like_a_test_is_not_one() {
    for path in ["src/attests.rs", "src/latest.rs", "src/contest.rs"] {
        let c = count(path, None, &rows(401, "x"));
        assert!(!c.test && c.over(), "{path}");
    }
}

#[test]
fn an_inline_test_module_counts_as_code() {
    let t = format!("#[cfg(test)]\nmod tests {{\n{}}}\n", rows(401, "    let x ="));
    let c = count("src/a.rs", None, &t);
    assert!(!c.test && c.over());
}
