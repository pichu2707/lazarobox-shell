use super::limit::{LIMIT, Written, count, from_diff};
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
    let t = "/* open\n * still comment\n * more\n */\nlet x = 1;\n";
    assert_eq!(count("a.rs", None, t).lines, 1);
    // Code after a closing marker counts.
    assert_eq!(count("a.rs", None, "/* a\nb */ let x = 1;\n").lines, 1);
    // A one-row block comment with code after it counts, without it does not.
    assert_eq!(
        count("a.rs", None, "/* a */ let x = 1;\n/* b */\n").lines,
        1
    );
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
    // An assignment of a string is code, and so is its body. The closing row
    // starts with the quotes but nothing closes it later, so it is code too.
    assert_eq!(count("a.py", None, "x = \"\"\"\nbody\n\"\"\"\n").lines, 3);
}

#[test]
fn a_closing_triple_quote_does_not_silence_the_rest_of_the_file() {
    let t = format!("x = \"\"\"\nbody\n\"\"\"\n{}", rows(500, "y ="));
    assert!(count("a.py", None, &t).lines >= 502);
}

#[test]
fn an_opener_inside_a_string_or_never_closed_does_not_silence_code() {
    let raw = format!(
        "let s = r#\"\n/* not a comment\n\"#;\n{}",
        rows(500, "let y =")
    );
    assert!(count("a.rs", None, &raw).lines >= 501);
    // A real comment after the string, and a `*/` inside a later string.
    let later = format!("{raw}/* real */\nlet t = 1;\n");
    assert!(count("a.rs", None, &later).lines >= 502);
    let closer = format!("/* not\n{}let t = \"*/\";\n", rows(500, "let y ="));
    assert!(count("a.rs", None, &closer).lines >= 501);
    let open = format!("/* never closed\n{}", rows(500, "let y ="));
    assert!(count("a.rs", None, &open).lines >= 500);
    let doc = format!("\"\"\"never closed\n{}", rows(500, "y ="));
    assert!(count("a.py", None, &doc).lines >= 500);
}

#[test]
fn a_block_never_reaches_across_context_rows_or_hunks() {
    let filler = rows(30, "keep");
    let base = format!("fn a() {{}}\n*/\n{filler}end\n");
    let new = format!(
        "/* open\nfn a() {{}}\n*/\n{filler}end\n{}",
        rows(500, "let y =")
    );
    let c = count("a.rs", Some(&base), &new);
    assert!(c.lines >= 501, "{}", c.lines);
}

#[test]
fn a_closed_block_still_hides_its_rows_inside_a_run() {
    assert_eq!(
        count("a.rs", None, "/* a\n * b\n * c */\nlet x = 1;\n").lines,
        1
    );
}

#[test]
fn a_test_directory_is_read_from_a_project_relative_path_only() {
    assert!(!count("/home/u/tests/app/src/big.rs", None, "x\n").test);
    assert!(!count("../tests/x.rs", None, "x\n").test);
    assert!(count("./tests/x.rs", None, "x\n").test);
    assert!(count("src\\tests\\x.rs", None, "x\n").test);
    // The file-name rules need no directory.
    assert!(count("/home/u/app/run_tests.rs", None, "x\n").test);
}

#[test]
fn only_the_final_suffix_pair_makes_a_spec_or_test_name() {
    assert!(!count("src/my.test.config.rs", None, "x\n").test);
    assert!(!count("src/a.spec.b.ts", None, "x\n").test);
    assert!(count("src/a.test.ts", None, "x\n").test);
    assert!(count("src/a.spec.ts", None, "x\n").test);
}

#[test]
fn written_keys_are_one_file_however_the_path_is_spelled() {
    let mut w = Written::default();
    for p in [
        "src/a.rs",
        "./src/a.rs",
        "src\\a.rs",
        "src/x/../a.rs",
        "src//a.rs",
    ] {
        w.add(p, count("src/a.rs", None, &rows(100, "x =")));
    }
    assert_eq!(w.total("src/a.rs").lines, 500);
    assert_eq!(w.total("./src/a.rs").lines, 500);
}

#[test]
fn a_sum_saturates() {
    let mut w = Written::default();
    let mut big = count("src/a.rs", None, "x\n");
    big.lines = usize::MAX;
    w.add("src/a.rs", big);
    w.add("src/a.rs", big);
    assert_eq!(w.total("src/a.rs").lines, usize::MAX);
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
    let t = format!(
        "#[cfg(test)]\nmod tests {{\n{}}}\n",
        rows(401, "    let x =")
    );
    let c = count("src/a.rs", None, &t);
    assert!(!c.test && c.over());
}

#[test]
fn two_applied_batches_of_one_worker_on_one_file_sum() {
    let mut w = Written::default();
    w.add("src/a.rs", count("src/a.rs", None, &rows(300, "x =")));
    w.add("src/a.rs", count("src/a.rs", None, &rows(150, "y =")));
    assert_eq!(w.total("src/a.rs").lines, 450);
    assert!(w.total("src/a.rs").over());
}

#[test]
fn the_total_with_a_write_not_yet_applied_leaves_the_record_alone() {
    let mut w = Written::default();
    w.add("src/a.rs", count("src/a.rs", None, &rows(300, "x =")));
    let next = count("src/a.rs", None, &rows(101, "y ="));
    assert!(w.with("src/a.rs", next).over());
    assert_eq!(w.total("src/a.rs").lines, 300);
}

#[test]
fn files_do_not_add_to_each_other() {
    let mut w = Written::default();
    w.add("src/a.rs", count("src/a.rs", None, &rows(300, "x =")));
    w.add("src/b.rs", count("src/b.rs", None, &rows(300, "x =")));
    assert_eq!(w.total("src/a.rs").lines, 300);
    assert_eq!(w.total("src/c.rs").lines, 0);
}

#[test]
fn a_second_worker_and_a_new_brief_start_from_zero() {
    let mut first = Written::default();
    first.add("src/a.rs", count("src/a.rs", None, &rows(300, "x =")));
    let second = Written::default();
    assert_eq!(second.total("src/a.rs").lines, 0);
    let next = count("src/a.rs", None, &rows(300, "y ="));
    assert!(!second.with("src/a.rs", next).over());
    assert_eq!(first.total("src/a.rs").lines, 300);
}

#[test]
fn the_flags_of_a_sum_follow_its_writes() {
    let mut w = Written::default();
    w.add("a.xyz", count("a.xyz", None, "x\n"));
    w.add("a.xyz", count("a.xyz", None, "y\n"));
    assert!(w.total("a.xyz").every_line);
    w.add("tests/t.rs", count("tests/t.rs", None, &rows(900, "x")));
    assert!(w.total("tests/t.rs").test && !w.total("tests/t.rs").over());
}

#[test]
fn a_string_opened_by_code_is_parity_tracked_across_docstrings() {
    let t = format!(
        "x = \"\"\"\nb\n\"\"\"\n{}z = \"\"\"\nb\n\"\"\"\n",
        rows(500, "y =")
    );
    assert!(count("a.py", None, &t).lines >= 505);
    let doc = format!(
        "x = \"\"\"\nb\n\"\"\"\n{}def f():\n    \"\"\"doc\n    more\n    \"\"\"\n",
        rows(500, "y =")
    );
    assert!(count("a.py", None, &doc).lines >= 503);
    let single = format!("x = '''\nb\n'''\n{}", rows(500, "y ="));
    assert!(count("a.py", None, &single).lines >= 503);
}

#[test]
fn a_real_docstring_is_still_hidden_and_plain_block_rows_are_counted() {
    let t = "def f():\n    \"\"\"doc\n    more\n    \"\"\"\n    return 1\n";
    assert_eq!(count("a.py", None, t).lines, 2);
    // Accepted over-count: interior rows of a block that do not look like a
    // comment (no leading `*`) are code.
    assert_eq!(count("a.rs", None, "/* a\nplain\nplain */\n").lines, 2);
    let doc = "/** doc\n * x\n */\nfn a() {}\n";
    assert_eq!(count("a.rs", None, doc).lines, 1);
}

#[test]
fn a_drive_path_is_absolute() {
    assert!(!count("C:\\p\\tests\\a.rs", None, "x\n").test);
    assert!(!count("\\\\srv\\tests\\a.rs", None, "x\n").test);
    assert!(count("C:\\p\\a_test.rs", None, "x\n").test);
}
