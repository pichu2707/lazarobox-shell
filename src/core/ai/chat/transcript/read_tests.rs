//! `read_rows`: the clean text under a chat COPY selection.

use super::*;
use crate::core::copy::{LineSpan, LineText, join};

/// The last column of a line selection in these tests.
const LAST: u16 = 79;

fn rows(parts: &[(LineKind, &str, &str, &str, usize)]) -> Vec<Line> {
    let mut out = Vec::new();
    for &(kind, first, rest, text, width) in parts {
        push_wrapped(&mut out, kind, first, rest, text, width);
    }
    out
}

fn text(lines: &[Line], span: LineSpan) -> String {
    join(&read_rows(lines, span))
}

fn all(lines: &[Line]) -> LineSpan {
    LineSpan::new((0, 0), (lines.len() - 1, LAST))
}

fn row(text: &str, wrapped: bool) -> LineText {
    LineText {
        text: text.to_owned(),
        wrapped,
    }
}

#[test]
fn a_char_selection_reads_from_the_anchor_cell_to_the_cursor_cell() {
    let lines = rows(&[(LineKind::Assistant, "", "", "hello world\nsecond line", 40)]);
    let span = LineSpan::new((0, 6), (1, 5));
    assert_eq!(
        read_rows(&lines, span),
        [row("world", false), row("second", false)]
    );
    assert_eq!(text(&lines, span), "world\nsecond");
    // Backwards (cursor before the anchor) reads the same cells.
    assert_eq!(text(&lines, LineSpan::new((1, 5), (0, 6))), "world\nsecond");
}

#[test]
fn a_line_selection_reads_whole_rows() {
    let lines = rows(&[(
        LineKind::Assistant,
        "",
        "",
        "hello world\nsecond line\nthird",
        40,
    )]);
    assert_eq!(
        text(&lines, LineSpan::new((0, 0), (1, LAST))),
        "hello world\nsecond line"
    );
}

#[test]
fn the_drawing_prefix_of_every_kind_is_not_copied() {
    let lines = rows(&[
        (LineKind::User, USER_PREFIX, "  ", "ask this", 40),
        (LineKind::System, SYSTEM_PREFIX, "  ", "a note", 40),
        (LineKind::Reasoning, GUTTER, GUTTER, "think\nmore", 40),
        (LineKind::Tool, TOOL_PREFIX, "  ", "read x", 40),
        (LineKind::Tool, "  ", "    ", "ok: done", 40),
    ]);
    assert_eq!(
        text(&lines, all(&lines)),
        "ask this\na note\nthink\nmore\nread x\nok: done"
    );
    // The continuation indent of a wrapped user row goes too.
    let lines = rows(&[(LineKind::User, USER_PREFIX, "  ", "abcd efgh", 8)]);
    assert_eq!(rows_text(&lines), ["› abcd", "  efgh"]);
    assert_eq!(text(&lines, all(&lines)), "abcd efgh");
    // Cells inside the prefix only hold nothing to copy.
    assert_eq!(text(&lines, LineSpan::new((0, 0), (0, 1))), "");
}

#[test]
fn a_hint_row_is_never_copied() {
    let lines = rows(&[
        (LineKind::Code, "", "", "```\nx\n```", 40),
        (LineKind::Hint, "", "", RUN_HINT, 40),
        (LineKind::Assistant, "", "", "after", 40),
    ]);
    assert_eq!(text(&lines, all(&lines)), "```\nx\n```\nafter");
    // A selection of the hint alone has nothing in it.
    assert!(read_rows(&lines, LineSpan::new((3, 0), (3, LAST))).is_empty());
}

#[test]
fn wrapped_prose_is_rejoined_with_the_space_the_wrap_dropped() {
    let lines = rows(&[(LineKind::Assistant, "", "", "alpha beta gamma delta", 12)]);
    assert_eq!(rows_text(&lines), ["alpha beta", "gamma delta"]);
    assert_eq!(
        read_rows(&lines, all(&lines)),
        [row("alpha beta ", true), row("gamma delta", false)]
    );
    assert_eq!(text(&lines, all(&lines)), "alpha beta gamma delta");
    // A word broken hard is rejoined with no space inside it.
    let lines = rows(&[(LineKind::Assistant, "", "", "abcdefghij kl", 4)]);
    assert_eq!(rows_text(&lines), ["abcd", "efgh", "ij", "kl"]);
    assert_eq!(text(&lines, all(&lines)), "abcdefghij kl");
}

#[test]
fn wrapped_code_is_rejoined_without_losing_a_character() {
    let lines = rows(&[(LineKind::Code, "", "", "let longname = 1;\nab  cd", 12)]);
    assert_eq!(rows_text(&lines), ["let longname", " = 1;", "ab  cd"]);
    assert_eq!(text(&lines, all(&lines)), "let longname = 1;\nab  cd");
    let lines = rows(&[(LineKind::Code, "", "", "ab  cd", 3)]);
    assert_eq!(rows_text(&lines), ["ab ", " cd"]);
    assert_eq!(text(&lines, all(&lines)), "ab  cd");
}

#[test]
fn columns_are_display_cells_so_a_wide_char_is_read_from_either_cell() {
    let lines = rows(&[(LineKind::Assistant, "", "", "日本語abc", 40)]);
    // 日 0-1, 本 2-3, 語 4-5, a 6.
    assert_eq!(text(&lines, LineSpan::new((0, 3), (0, 6))), "本語a");
    assert_eq!(text(&lines, LineSpan::new((0, 2), (0, 2))), "本");
    assert_eq!(text(&lines, LineSpan::new((0, 5), (0, 5))), "語");
}

fn rows_text(lines: &[Line]) -> Vec<&str> {
    lines.iter().map(|l| l.text.as_str()).collect()
}
