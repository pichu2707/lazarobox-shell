//! What a run printed, read back from the pane's screen (design D22). The
//! anchor taken at the write marks the line the command was typed on; the
//! output is every line below it, up to the line the new prompt is on.
//!
//! Nothing here may lose output, so there is no guessing about the prompt
//! (design D22): the command echo is the anchor row plus the rows it wraps
//! onto, and must contain the command (whitespace squeezed) or nothing is
//! read; a shell that rewrites the typed text, such as fish expanding an
//! abbreviation, therefore gives "output not available". Only blank edges are
//! trimmed. The emulator records no shell-integration marks (OSC 133), so the
//! rows of a multi-line prompt that sit above the new prompt line reach the
//! model as output; they are wrapped as untrusted data and redacted like any
//! other text, which is the accepted residual. When a run is still going, the
//! cursor line is read too and may be a redrawn prompt. The 8 KiB cap is on
//! the screen text: `clean` and `redact` run later and can change its length
//! by a few bytes (the wrapper's own cap is 32 KiB).

use super::settle::Anchor;
use crate::core::{
    copy::{LineSpan, LineText, join},
    pane::Pane,
};

/// Lines kept from the start of the output.
pub const HEAD: usize = 100;
/// Lines kept from the end of the output.
pub const TAIL: usize = 100;
/// Largest result, in bytes.
pub const CAP: usize = 8 * 1024;
/// Rows of command echo read past the head.
const ECHO: usize = 8;
/// Bytes kept free for the marker when one row has to be cut.
const MARKER_ROOM: usize = 64;

/// The output of the run anchored at `anchor`, read from `pane`. `None` when
/// it cannot be told: the line numbers stopped being comparable, the anchor
/// scrolled out, the cursor is hidden or `command` is not echoed on the anchor
/// row (so even "printed nothing" is only said of a run that was seen typed).
/// `prompt_below` says the cursor line holds the new prompt (and so is not
/// output); otherwise it holds the output so far, which for a run that has
/// not finished may be a redrawn prompt. At most two spans are read.
pub fn capture(
    pane: &mut Pane,
    anchor: &Anchor,
    command: &str,
    prompt_below: bool,
) -> Option<String> {
    if pane.line_generation() != anchor.generation || anchor.line < pane.first_line() {
        return None;
    }
    let cursor = pane
        .live_cursor_line()
        .filter(|&line| line >= anchor.line)?;
    let last = cursor
        .saturating_sub(usize::from(prompt_below))
        .max(anchor.line);
    let span = |first, last| LineSpan {
        first,
        last,
        first_col: 0,
        last_col: u16::MAX,
    };
    let rows = last - anchor.line + 1;
    let (mut head, mut tail, mut gap) = if rows <= HEAD + TAIL + ECHO {
        (pane.read_lines(span(anchor.line, last)), Vec::new(), 0)
    } else {
        let head_to = anchor.line + HEAD + ECHO - 1;
        let tail_from = last + 1 - TAIL;
        (
            pane.read_lines(span(anchor.line, head_to)),
            pane.read_lines(span(tail_from, last)),
            tail_from - head_to - 1,
        )
    };
    // The echo ends on the first row that does not wrap onto the next.
    let echo = head.iter().position(|row| !row.wrapped)? + 1;
    if !squeeze(&join(&head[..echo])).contains(&squeeze(command)) {
        return None;
    }
    head.drain(..echo);
    let leading = head.iter().take_while(|row| blank(row)).count();
    head.drain(..leading);
    let read = tail.len();
    let end = if tail.is_empty() {
        &mut head
    } else {
        &mut tail
    };
    while end.last().is_some_and(blank) {
        end.pop();
    }
    if read > 0 && tail.is_empty() {
        // Nothing is shown after the marker: the rows it replaced count too.
        gap += read;
    }
    Some(shape(head, tail, gap))
}

/// `text` with every run of whitespace as one space and none at the ends.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn blank(row: &LineText) -> bool {
    row.text.trim().is_empty()
}

/// The text of the first [`HEAD`] and last [`TAIL`] rows of `head` + `tail`
/// (`gap` rows lie unread between them), with the marker where rows are
/// missing, cut to [`CAP`] bytes on a character boundary.
fn shape(head: Vec<LineText>, tail: Vec<LineText>, gap: usize) -> String {
    let mut skipped = gap;
    let mut cut = (gap > 0).then_some(head.len());
    let mut rows = head;
    rows.extend(tail);
    if rows.len() > HEAD + TAIL {
        skipped += rows.len() - HEAD - TAIL;
        rows.drain(HEAD..rows.len() - TAIL);
        cut = Some(HEAD);
    }
    let mut tail = rows.split_off(cut.unwrap_or(rows.len()));
    let mut head = rows;
    let mut text = render(&head, &tail, skipped);
    // Over the cap: give up the rows next to the marker, one at a time.
    while text.len() > CAP && head.len() + tail.len() > 1 {
        if head.len() >= tail.len() {
            head.pop();
        } else {
            tail.remove(0);
        }
        skipped += 1;
        text = render(&head, &tail, skipped);
    }
    if text.len() > CAP {
        if let Some(row) = head.first_mut().or(tail.first_mut()) {
            let mut at = CAP - MARKER_ROOM;
            while !row.text.is_char_boundary(at) {
                at -= 1;
            }
            row.text.truncate(at);
        }
        text = render(&head, &tail, skipped);
    }
    text
}

fn render(head: &[LineText], tail: &[LineText], skipped: usize) -> String {
    let marker = format!("[\u{2026} {skipped} lines not shown \u{2026}]");
    let parts = [
        join(head),
        if skipped > 0 { marker } else { String::new() },
        join(tail),
    ];
    let parts: Vec<&str> = parts
        .iter()
        .map(String::as_str)
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pane::PaneSize;

    const OLD: &str = "$ ";

    fn pane(rows: u16, cols: u16, scrollback: usize) -> Pane {
        Pane::new(PaneSize { rows, cols }, scrollback)
    }

    fn crlf(text: &str) -> String {
        text.replace('\n', "\r\n")
    }

    /// The shell shows `before`, the command is typed at the cursor and
    /// printed `output`, then the shell shows `after`.
    fn ran(pane: &mut Pane, before: &str, command: &str, output: &str, after: &str) -> Anchor {
        pane.feed(crlf(before).as_bytes());
        let anchor = Anchor {
            line: pane.live_cursor_line().unwrap(),
            col: pane.live_cursor().unwrap().1,
            generation: pane.line_generation(),
            cwd: "/".into(),
        };
        pane.feed(crlf(&format!("{command}\n{output}{after}")).as_bytes());
        anchor
    }

    fn simple(command: &str, output: &str) -> Option<String> {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, command, output, OLD);
        capture(&mut p, &anchor, command, true)
    }

    fn numbered(count: usize, width: usize) -> String {
        (1..=count)
            .map(|n| format!("{n:0>width$}\n"))
            .collect::<String>()
    }

    #[test]
    fn multi_line_output_is_read_without_the_echo_or_the_prompt() {
        assert_eq!(
            simple("ls", "a\nb  c\n d\n").as_deref(),
            Some("a\nb  c\n d")
        );
    }

    #[test]
    fn a_command_that_printed_nothing_gives_an_empty_body() {
        assert_eq!(simple("cd x", "").as_deref(), Some(""));
    }

    #[test]
    fn blank_edges_are_trimmed_and_inner_blanks_kept() {
        assert_eq!(simple("ls", "\n\na\n\nb\n\n").as_deref(), Some("a\n\nb"));
    }

    #[test]
    fn a_wrapped_command_echo_is_skipped_whole() {
        let command = "ls aaaaaaaaaaaaaaaaaaaaaaaa";
        let mut p = pane(24, 10, 2000);
        let anchor = ran(&mut p, OLD, command, "x\n", OLD);
        assert_eq!(
            capture(&mut p, &anchor, command, true).as_deref(),
            Some("x")
        );
    }

    #[test]
    fn output_that_wraps_is_joined_back() {
        let mut p = pane(24, 10, 2000);
        let anchor = ran(&mut p, OLD, "ls", "0123456789abcdef\n", OLD);
        let got = capture(&mut p, &anchor, "ls", true);
        assert_eq!(got.as_deref(), Some("0123456789abcdef"));
    }

    #[test]
    fn a_screen_that_does_not_echo_the_command_is_not_read() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "pwd", "/x\n", OLD);
        assert_eq!(capture(&mut p, &anchor, "ls", true), None);
    }

    #[test]
    fn a_thousand_lines_keep_the_head_and_the_tail_with_a_marker() {
        let mut p = pane(24, 80, 5000);
        let anchor = ran(&mut p, OLD, "ls", &numbered(1000, 4), OLD);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines.len(), 201, "{got}");
        assert_eq!(lines[0], "0001");
        assert_eq!(lines[99], "0100");
        assert_eq!(lines[100], "[\u{2026} 800 lines not shown \u{2026}]");
        assert_eq!(lines[101], "0901");
        assert_eq!(lines[200], "1000");
    }

    #[test]
    fn exactly_the_most_that_fits_has_no_marker() {
        let mut p = pane(24, 80, 5000);
        let anchor = ran(&mut p, OLD, "ls", &numbered(200, 4), OLD);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        assert_eq!(got.lines().count(), 200);
        assert!(!got.contains("not shown"), "{got}");
    }

    #[test]
    fn long_lines_are_cut_to_eight_kib_by_dropping_rows_next_to_the_marker() {
        let mut p = pane(24, 120, 5000);
        let anchor = ran(&mut p, OLD, "ls", &numbered(300, 100), OLD);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        assert!(got.len() <= CAP, "{}", got.len());
        assert!(got.starts_with(&"0".repeat(97)), "{got}");
        assert!(got.ends_with("300"), "the tail's last row stays");
        assert!(got.contains("lines not shown"), "{got}");
    }

    #[test]
    fn one_row_over_the_cap_is_cut_on_a_character_boundary() {
        let mut p = pane(5, 5000, 100);
        let line = "\u{20ac}".repeat(4000);
        let anchor = ran(&mut p, OLD, "ls", &format!("{line}\n"), OLD);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        assert!(got.len() <= CAP, "{}", got.len());
        assert!(got.chars().all(|c| c == '\u{20ac}'), "{got:?}");
        assert!(got.len() > CAP - 2 * MARKER_ROOM);
    }

    #[test]
    fn a_resize_since_the_anchor_makes_the_output_unavailable() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "ls", "a\n", OLD);
        p.resize(PaneSize { rows: 24, cols: 60 });
        assert_eq!(capture(&mut p, &anchor, "ls", true), None);
    }

    #[test]
    fn an_anchor_that_scrolled_out_makes_the_output_unavailable() {
        let mut p = pane(5, 80, 10);
        let anchor = ran(&mut p, OLD, "ls", "", "");
        for n in 0..60 {
            p.feed(format!("{n}\r\n").as_bytes());
        }
        p.feed(OLD.as_bytes());
        assert_eq!(p.line_generation(), anchor.generation);
        assert!(p.first_line() > anchor.line, "{}", p.first_line());
        assert_eq!(capture(&mut p, &anchor, "ls", true), None);
    }

    #[test]
    fn a_hidden_cursor_makes_the_output_unavailable() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "ls", "a\n", "\x1b[?25l");
        assert_eq!(capture(&mut p, &anchor, "ls", true), None);
    }

    const TWO: &str = "\u{256d}\u{2500} user ~/dir\n\u{2570}\u{2500} $ ";

    // A prompt line that reaches the model is acceptable (it is wrapped and
    // redacted); a lost output line is not.
    #[test]
    fn a_two_line_prompt_never_costs_an_output_line() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, TWO, "ls", "a\nb\n", &format!("\n{TWO}"));
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        assert!(got.starts_with("a\nb"), "{got}");
    }

    #[test]
    fn a_two_line_prompt_over_the_two_span_threshold_keeps_both_ends() {
        let mut p = pane(24, 80, 5000);
        let anchor = ran(&mut p, TWO, "ls", &numbered(300, 3), TWO);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines[0], "001");
        assert_eq!(lines[100], "[\u{2026} 101 lines not shown \u{2026}]");
        assert_eq!(lines[200], "\u{256d}\u{2500} user ~/dir");
    }

    /// The rows above the old prompt are the last command's output, which
    /// must never be taken for prompt decoration.
    fn after_a_run_printing(prior: &str, output: &str) -> Option<String> {
        let mut p = pane(24, 80, 2000);
        p.feed(crlf(&format!("$ ls\n{prior}")).as_bytes());
        let anchor = ran(&mut p, OLD, "ls", output, OLD);
        capture(&mut p, &anchor, "ls", true)
    }

    #[test]
    fn bullet_lists_in_consecutive_runs_keep_every_line() {
        let got = after_a_run_printing("- a\n- b\n", "- c\n- d\n");
        assert_eq!(got.as_deref(), Some("- c\n- d"));
    }

    #[test]
    fn a_table_separator_and_a_graph_row_are_output() {
        let prior = "|---|---|\n| x | y |\n";
        let got = after_a_run_printing(prior, "| a | b |\n|---|---|\n| c | d |\n| e | f |\n");
        assert_eq!(
            got.as_deref(),
            Some("| a | b |\n|---|---|\n| c | d |\n| e | f |")
        );
        let got = after_a_run_printing("| * x\n| * y\n", "| * a\n| * b\n");
        assert_eq!(got.as_deref(), Some("| * a\n| * b"));
    }

    #[test]
    fn running_pwd_twice_prints_the_same_line_twice() {
        assert_eq!(
            after_a_run_printing("/home/u\n", "/home/u\n").as_deref(),
            Some("/home/u")
        );
    }

    #[test]
    fn a_run_still_going_reads_up_to_the_cursor_line_included() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "ls", "a\npart", "");
        assert_eq!(
            capture(&mut p, &anchor, "ls", false).as_deref(),
            Some("a\npart")
        );
    }

    #[test]
    fn spaces_around_and_inside_the_command_do_not_fail_the_echo_check() {
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "ls", "a\n", OLD);
        assert_eq!(capture(&mut p, &anchor, "ls ", true).as_deref(), Some("a"));
        let mut p = pane(24, 80, 2000);
        let anchor = ran(&mut p, OLD, "ls -la", "a\n", OLD);
        assert_eq!(
            capture(&mut p, &anchor, "ls  -la", true).as_deref(),
            Some("a")
        );
        assert_eq!(
            capture(&mut p, &anchor, "ls\t-la", true).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn a_cursor_still_on_the_anchor_line_is_silence_only_if_the_echo_is_there() {
        for (typed, want) in [("ls", Some("")), ("", None)] {
            let mut p = pane(24, 80, 2000);
            p.feed(OLD.as_bytes());
            let anchor = Anchor {
                line: p.live_cursor_line().unwrap(),
                col: 2,
                generation: p.line_generation(),
                cwd: "/".into(),
            };
            p.feed(typed.as_bytes());
            for below in [true, false] {
                let got = capture(&mut p, &anchor, "ls", below);
                assert_eq!(got.as_deref(), want, "{typed:?} {below}");
            }
        }
    }

    #[test]
    fn the_two_span_threshold_sits_at_210_and_211_rows() {
        for (lines, hidden) in [(209, 9), (210, 10), (211, 11)] {
            let mut p = pane(24, 80, 5000);
            let anchor = ran(&mut p, OLD, "ls", &numbered(lines, 3), OLD);
            let got = capture(&mut p, &anchor, "ls", true).unwrap();
            let rows: Vec<&str> = got.lines().collect();
            let marker = format!("[\u{2026} {hidden} lines not shown \u{2026}]");
            assert_eq!(rows.len(), 201, "{lines}");
            assert_eq!(rows[100], marker, "{lines}");
            assert_eq!(rows[0], "001", "{lines}");
            assert_eq!(rows[200], format!("{lines:03}"), "{lines}");
        }
    }

    #[test]
    fn a_run_going_over_the_two_span_threshold_includes_the_cursor_line() {
        let mut p = pane(24, 80, 5000);
        let anchor = ran(&mut p, OLD, "ls", &numbered(300, 3), "tail");
        let got = capture(&mut p, &anchor, "ls", false).unwrap();
        let rows: Vec<&str> = got.lines().collect();
        assert_eq!(rows[100], "[\u{2026} 101 lines not shown \u{2026}]");
        assert_eq!(rows[200], "tail");
    }

    #[test]
    fn a_two_row_echo_is_skipped_when_the_head_is_cut() {
        let command = "ls aaaaaaaaaaaaaaaa";
        let mut p = pane(24, 10, 5000);
        let anchor = ran(&mut p, OLD, command, &numbered(300, 3), OLD);
        let got = capture(&mut p, &anchor, command, true).unwrap();
        let rows: Vec<&str> = got.lines().collect();
        assert_eq!(rows[0], "001");
        assert_eq!(rows[100], "[\u{2026} 100 lines not shown \u{2026}]");
        assert_eq!(rows[200], "300");
    }

    #[test]
    fn a_right_prompt_on_the_echo_row_is_part_of_the_echo() {
        let mut p = pane(24, 80, 2000);
        let echoed = format!("ls{}12:00", " ".repeat(60));
        let anchor = ran(&mut p, OLD, &echoed, "a\n", OLD);
        assert_eq!(capture(&mut p, &anchor, "ls", true).as_deref(), Some("a"));
    }

    #[test]
    fn a_tail_of_only_blank_rows_still_counts_what_was_not_read() {
        let mut p = pane(24, 80, 5000);
        let output = format!("{}{}", numbered(150, 3), "\n".repeat(150));
        let anchor = ran(&mut p, OLD, "ls", &output, OLD);
        let got = capture(&mut p, &anchor, "ls", true).unwrap();
        let last = got.lines().last().unwrap();
        assert!(last.contains("lines not shown"), "{got}");
        assert!(last.contains("193"), "{last}");
    }
}
