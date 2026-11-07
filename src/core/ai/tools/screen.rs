//! The text of a pane's screen that `read_pane` may show a model. Pure: the
//! caller reads the rows from the emulator and the user reviews exactly this
//! text before it is sent.
//!
//! Pipeline: re-join soft-wrapped rows (so an assignment split by a wrap is
//! still redacted), strip control characters, redact secrets, then cap the
//! tail. The cap comes last: a cut can never split a secret pattern.

use crate::core::ai::memory::redact::redact;
use crate::core::ai::sanitize::clean;
use crate::core::ai::tools::content::is_hidden;
use crate::core::copy::{LineText, join};

/// Largest text sent, in bytes, cut marker and notes included.
pub const TEXT_CAP: usize = 8 * 1024;

/// First line of the text when scrollback was asked on an alternate screen.
pub const ALT_NOTE: &str = "full-screen app: visible screen only";

/// What a pane's screen says, ready to review and to send.
#[derive(Clone, PartialEq, Eq)]
pub struct Screen {
    /// The exact text: optional notes, then the lines.
    pub text: String,
    /// Lines of screen content kept (notes and the cut marker not counted).
    pub lines: usize,
    /// Earlier lines were cut to fit [`TEXT_CAP`].
    pub cut: bool,
    /// Scrollback was asked but the pane shows a full-screen app.
    pub no_scrollback: bool,
}

/// Sizes only: the text is screen content and stays out of logs.
impl std::fmt::Debug for Screen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screen")
            .field("bytes", &self.text.len())
            .field("lines", &self.lines)
            .field("cut", &self.cut)
            .field("no_scrollback", &self.no_scrollback)
            .finish()
    }
}

fn marker(kept: usize) -> String {
    format!("[earlier lines cut: kept the last {kept} lines]")
}

/// The text of `rows`, oldest first; `None` when the screen is blank. `alt`:
/// the pane runs an alternate-screen app; `scrollback_asked`: the model asked
/// for history.
pub fn build(rows: &[LineText], alt: bool, scrollback_asked: bool) -> Option<Screen> {
    // Strip what the review shows as `<U+XXXX>`, so the preview is the text.
    let visible: String = clean(&join(rows))
        .chars()
        .filter(|&c| !is_hidden(c))
        .collect();
    let redacted = redact(&visible).text;
    let redacted = redacted.trim_end();
    if redacted.is_empty() {
        return None;
    }
    let note = (alt && scrollback_asked).then_some(ALT_NOTE);
    let lines: Vec<&str> = redacted
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .collect();
    let prefix = note.map_or(0, |n| n.len() + 1);
    let mut skip = 0;
    let mut cut = false;
    // Drop the oldest line until the text and its marker fit the cap.
    let mut body = loop {
        let kept = &lines[skip..];
        let size = kept
            .iter()
            .map(|l| l.len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let extra = if cut { marker(kept.len()).len() + 1 } else { 0 };
        if prefix + extra + size <= TEXT_CAP || kept.len() <= 1 {
            break kept.join("\n");
        }
        skip += 1;
        cut = true;
    };
    if body.len() + prefix + if cut { marker(1).len() + 1 } else { 0 } > TEXT_CAP {
        // One line alone is over the cap: keep its tail.
        let room = TEXT_CAP - prefix - marker(1).len() - 1;
        let mut start = body.len() - room;
        while !body.is_char_boundary(start) {
            start += 1;
        }
        body = body[start..].to_owned();
        cut = true;
    }
    let kept = body.lines().count();
    let mut text = String::new();
    if let Some(note) = note {
        text.push_str(note);
        text.push('\n');
    }
    if cut {
        text.push_str(&marker(kept));
        text.push('\n');
    }
    text.push_str(&body);
    Some(Screen {
        text,
        lines: kept,
        cut,
        no_scrollback: note.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::tools::{diff::escape, label};

    fn row(text: &str) -> LineText {
        LineText {
            text: text.to_owned(),
            wrapped: false,
        }
    }

    fn rows(lines: &[&str]) -> Vec<LineText> {
        lines.iter().map(|l| row(l)).collect()
    }

    // Spec (agent-tools, read_pane): the visible screen is the text.
    #[test]
    fn the_visible_rows_are_the_text_without_trailing_blanks() {
        let screen = build(&rows(&["$ cargo test   ", "ok", "   ", ""]), false, false).unwrap();
        assert_eq!(screen.text, "$ cargo test\nok");
        assert_eq!(screen.lines, 2);
        assert!(!screen.cut && !screen.no_scrollback);
    }

    #[test]
    fn a_scrollback_tail_keeps_every_asked_row_in_order() {
        let screen = build(&rows(&["old 1", "old 2", "now"]), false, true).unwrap();
        assert_eq!(screen.text, "old 1\nold 2\nnow");
        assert_eq!(screen.lines, 3);
    }

    // D7: the cap is on the whole text, marker included; the tail is kept.
    #[test]
    fn over_the_cap_the_tail_is_kept_on_a_line_boundary_with_a_marker_inside_the_cap() {
        let lines: Vec<String> = (0..400)
            .map(|i| format!("{i:03} {}", "x".repeat(55)))
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let screen = build(&rows(&refs), false, true).unwrap();
        assert!(screen.cut);
        assert!(screen.text.len() <= TEXT_CAP, "{}", screen.text.len());
        let (head, body) = screen.text.split_once('\n').unwrap();
        assert_eq!(
            head,
            format!("[earlier lines cut: kept the last {} lines]", screen.lines)
        );
        assert_eq!(body.lines().count(), screen.lines);
        assert_eq!(body.lines().last().unwrap(), lines[399]);
        assert_eq!(body.lines().next().unwrap(), lines[400 - screen.lines]);
        // Not cut as much as needed: the next line up would not have fit.
        assert!(screen.text.len() + 60 > TEXT_CAP);
    }

    #[test]
    fn a_single_line_longer_than_the_cap_is_cut_to_its_tail() {
        let long = format!("{}END", "é".repeat(TEXT_CAP));
        let screen = build(&rows(&[&long]), false, false).unwrap();
        assert!(screen.cut);
        assert!(screen.text.len() <= TEXT_CAP);
        assert!(screen.text.ends_with("END"));
    }

    // D6: soft-wrapped rows are re-joined before redaction.
    #[test]
    fn an_assignment_split_across_soft_wraps_is_redacted() {
        let split = vec![
            LineText {
                text: "export MY_SEC".to_owned(),
                wrapped: true,
            },
            LineText {
                text: "RET=hunter2hunter2".to_owned(),
                wrapped: false,
            },
        ];
        let screen = build(&split, false, false).unwrap();
        assert!(!screen.text.contains("hunter2"), "{}", screen.text);
        assert!(screen.text.contains("MY_SECRET=[REDACTED]"));
        // The same secret on one row is redacted too.
        let one = build(&rows(&["MY_SECRET=hunter2hunter2"]), false, false).unwrap();
        assert!(!one.text.contains("hunter2"));
    }

    #[test]
    fn control_characters_never_reach_the_text() {
        let screen = build(&rows(&["a\u{1b}[31mb\u{202e}c"]), false, false).unwrap();
        assert_eq!(screen.text, "abc");
    }

    #[test]
    fn a_blank_screen_has_no_text() {
        assert_eq!(build(&rows(&["", "   ", ""]), false, true), None);
        assert_eq!(build(&[], false, false), None);
    }

    // Alternate screen: only the visible grid, and the text says so.
    #[test]
    fn scrollback_asked_on_the_alternate_screen_starts_with_the_note() {
        let screen = build(&rows(&["vim", "~"]), true, true).unwrap();
        assert_eq!(screen.text, format!("{ALT_NOTE}\nvim\n~"));
        assert!(screen.no_scrollback);
        assert_eq!(screen.lines, 2);
        // No scrollback asked: nothing to explain.
        let plain = build(&rows(&["vim", "~"]), true, false).unwrap();
        assert_eq!(plain.text, "vim\n~");
        assert!(!plain.no_scrollback);
    }

    // D5: what the user reads is what the model gets.
    #[test]
    fn every_line_is_its_own_escape_and_the_wrapped_result_holds_the_text() {
        let screen = build(
            &rows(&["$ ls", "a\tb", "ünï ✓", "❤\u{fe0f}x\u{2800}y\u{3164}z"]),
            true,
            true,
        )
        .unwrap();
        assert!(screen.text.contains("❤x"), "{}", screen.text);
        for line in screen.text.lines() {
            assert_eq!(escape(line), line);
        }
        let wrapped = label::wrap("0123456789abcdef", &screen.text);
        assert!(wrapped.contains(&screen.text), "{wrapped}");
        assert!(!wrapped.contains("[truncated"));
    }

    #[test]
    fn debug_shows_sizes_never_the_text() {
        let screen = build(&rows(&["top secret screen"]), false, false).unwrap();
        let shown = format!("{screen:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("bytes"), "{shown}");
    }

    #[test]
    fn blank_lines_that_redaction_leaves_at_the_edges_are_trimmed() {
        let screen = build(
            &rows(&["", "\u{2800}", "one", "two", "\u{3164}", ""]),
            false,
            false,
        )
        .unwrap();
        assert_eq!(screen.text, "one\ntwo");
        assert_eq!(screen.lines, 2);
    }

    #[test]
    fn exactly_the_cap_is_not_cut_and_one_byte_more_is() {
        let fit = "a".repeat(TEXT_CAP / 2 - 1);
        let two = [fit.as_str(), fit.as_str()];
        // 2 * (4095) + 1 newline = 8191; pad the last line to 8192 exactly.
        let exact = format!("{}a", two[1]);
        let screen = build(&rows(&[&fit, &exact]), false, false).unwrap();
        assert_eq!(screen.text.len(), TEXT_CAP);
        assert!(!screen.cut);
        let over = format!("{exact}a");
        let screen = build(&rows(&[&fit, &over]), false, false).unwrap();
        assert!(screen.cut);
        assert!(screen.text.len() <= TEXT_CAP);
    }

    #[test]
    fn the_alt_note_counts_toward_the_cap_with_a_cut_and_with_the_single_line_fallback() {
        let lines: Vec<String> = (0..400)
            .map(|i| format!("{i:03} {}", "x".repeat(55)))
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let screen = build(&rows(&refs), true, true).unwrap();
        assert!(screen.cut && screen.no_scrollback);
        assert!(screen.text.starts_with(ALT_NOTE));
        assert!(screen.text.len() <= TEXT_CAP, "{}", screen.text.len());
        let long = "y".repeat(TEXT_CAP * 2);
        let screen = build(&rows(&[&long]), true, true).unwrap();
        assert!(
            screen.cut && screen.text.len() <= TEXT_CAP,
            "{}",
            screen.text.len()
        );
        assert!(screen.text.starts_with(ALT_NOTE));
    }

    #[test]
    fn the_marker_growing_from_99_to_100_lines_still_fits() {
        for width in 70..90 {
            let lines: Vec<String> = (0..300).map(|_| "z".repeat(width)).collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let screen = build(&rows(&refs), true, true).unwrap();
            assert!(
                screen.text.len() <= TEXT_CAP,
                "{width}: {}",
                screen.text.len()
            );
        }
    }
}
