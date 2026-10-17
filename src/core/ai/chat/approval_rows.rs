//! What the approval prompt draws, as rows, and whether it can show everything
//! the user would approve. The renderer and the `y` key both ask this module,
//! so what is drawn and what may be approved cannot drift apart.
//!
//! A prompt is approved only when its whole text (the path, or every line of
//! the command) is on screen. Otherwise it draws a short "too small" notice
//! and `y` does nothing until the pane is enlarged; `n`, Esc and Ctrl+C still
//! deny.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{ChatState, approval::ApprovalPrompt, wrap::wrap_ranges};

/// The most rows of the command drawn.
pub const MAX_COMMAND_ROWS: usize = 6;
/// What the prompt says when it cannot show the whole text.
pub const TOO_SMALL: &str = "pane too small to show it all: enlarge it or press n";
/// The keys line while the prompt does not answer yet.
pub const NOT_ARMED: &str = "…";
/// How a tab is drawn.
const TAB: char = '→';

/// What a row of the prompt holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Part {
    Head(String),
    Command(String),
    /// Why a `run` command was not run directly. Advice only: it is drawn
    /// when there is a spare row and never changes whether the command fits.
    Why(String),
    Marker(String),
    Keys(String),
}

/// The command as rows `width` columns wide, one or more per logical line.
fn command_rows(command: &str, width: usize) -> Vec<String> {
    command
        .split('\n')
        .flat_map(|line| {
            let chars: Vec<char> = line
                .chars()
                .filter(|c| *c != '\r')
                .map(|c| if c == '\t' { TAB } else { c })
                .collect();
            wrap_ranges(&chars, width)
                .into_iter()
                .map(|range| chars[range].iter().collect::<String>())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `text` cut to `width` display columns with an ellipsis.
fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut kept = String::new();
    for c in text.chars() {
        used += c.width().unwrap_or(0);
        if used + 1 > width {
            break;
        }
        kept.push(c);
    }
    kept.push('…');
    kept
}

/// The rows when the whole command fits in `height` rows, else `None`.
fn full_rows(prompt: &ApprovalPrompt, width: usize, height: usize) -> Option<Vec<Part>> {
    let rows = command_rows(&prompt.command, width.max(1));
    let head = Part::Head(prompt.target.clone());
    match height {
        0 | 1 => None,
        2 if rows.len() == 1 => Some(vec![head, Part::Command(rows[0].clone())]),
        2 => None,
        _ if rows.len() > (height - 2).min(MAX_COMMAND_ROWS) => None,
        _ => {
            let spare = rows.len() < (height - 2).min(MAX_COMMAND_ROWS);
            let mut out = vec![head];
            out.extend(rows.into_iter().map(Part::Command));
            if let Some(why) = prompt.why.as_deref().filter(|_| spare) {
                out.push(Part::Why(clip(why, width)));
            }
            let keys = if prompt.armed { prompt.keys } else { NOT_ARMED };
            out.push(Part::Keys(keys.to_owned()));
            Some(out)
        }
    }
}

/// The rows of the prompt for `width` x `height`, at most `height` of them.
/// Everything the user would approve is shown, or the prompt says the pane is
/// too small; text is never cut off silently.
pub fn parts(prompt: &ApprovalPrompt, width: usize, height: usize) -> Vec<Part> {
    full_rows(prompt, width, height).unwrap_or_else(|| match height {
        0 => Vec::new(),
        1 => vec![Part::Marker(TOO_SMALL.to_owned())],
        _ => vec![
            Part::Head(prompt.target.clone()),
            Part::Marker(TOO_SMALL.to_owned()),
        ],
    })
}

/// The rows the whole prompt wants at `width`.
pub fn wanted(prompt: &ApprovalPrompt, width: usize) -> usize {
    full_rows(prompt, width, MAX_COMMAND_ROWS + 3).map_or(2, |rows| rows.len())
}

/// Whether the prompt shows all of its text in a chat pane `width` x
/// `pane_height`. The chat pane gives the prompt `wanted` rows, less when the
/// pane is short (it keeps a transcript row), and `parts` is asked for exactly
/// that many: the same call the renderer makes.
pub fn fits(prompt: &ApprovalPrompt, width: usize, pane_height: usize) -> bool {
    let height = u16::try_from(pane_height).unwrap_or(u16::MAX);
    let rows = ChatState::clamp_composer_rows(wanted(prompt, width), height);
    full_rows(prompt, width, usize::from(rows)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ai::chat::approval::{READ_KEYS, RUN_KEYS};

    fn run(command: &str) -> ApprovalPrompt {
        ApprovalPrompt {
            target: "run in pane 1".into(),
            command: command.into(),
            keys: RUN_KEYS,
            armed: true,
            why: None,
        }
    }

    fn explained(command: &str) -> ApprovalPrompt {
        ApprovalPrompt {
            why: Some("shell metacharacter: `&&`".into()),
            ..run(command)
        }
    }

    fn read(path: &str) -> ApprovalPrompt {
        ApprovalPrompt {
            target: "send a file's text to the engine".into(),
            command: format!("read {path}"),
            keys: READ_KEYS,
            armed: true,
            why: None,
        }
    }

    #[test]
    fn a_prompt_that_is_not_armed_shows_dots_instead_of_the_keys() {
        let mut p = run("ls");
        p.armed = false;
        assert_eq!(
            parts(&p, 40, 8).last(),
            Some(&Part::Keys(NOT_ARMED.to_owned()))
        );
        p.armed = true;
        assert_eq!(
            parts(&p, 40, 8).last(),
            Some(&Part::Keys(RUN_KEYS.to_owned()))
        );
    }

    #[test]
    fn a_short_command_fits_a_roomy_pane() {
        assert!(fits(&run("cd x"), 40, 12));
        assert!(fits(&read("/home/u/a.txt"), 40, 12));
    }

    // `read ` and 100 characters at width 20 take 6 rows; with the head and the
    // keys that is 8, and a pane keeps a row for the transcript.
    #[test]
    fn a_long_path_in_a_narrow_pane_needs_a_tall_enough_pane() {
        let p = read(&"a".repeat(100));
        for height in [1, 3, 6, 8] {
            assert!(!fits(&p, 20, height), "{height}");
        }
        assert!(fits(&p, 20, 9));
        // More than six rows of text never fit.
        assert!(!fits(&read(&"a".repeat(150)), 20, 60));
    }

    #[test]
    fn a_five_row_pane_at_width_40_cannot_hold_a_three_line_command() {
        let p = run("echo a\necho b\necho c");
        // head + 3 rows + keys = 5 rows, but the composer may not take the
        // last transcript row: 4 at most in a 5-row pane.
        assert!(!fits(&p, 40, 5));
        assert!(fits(&p, 40, 6));
    }

    #[test]
    fn a_pane_of_three_rows_or_fewer_fits_at_most_a_one_line_command() {
        for height in 0..=3 {
            assert!(!fits(&run("a\nb"), 40, height), "{height}");
        }
        assert!(!fits(&run("ls"), 40, 1));
        assert!(fits(&run("ls"), 40, 3));
    }

    #[test]
    fn a_command_over_the_row_budget_never_fits_however_tall_the_pane() {
        let command = (1..=7).map(|n| format!("echo {n}")).collect::<Vec<_>>();
        assert!(!fits(&run(&command.join("\n")), 40, 60));
        assert!(fits(&run(&command[..6].join("\n")), 40, 60));
    }

    #[test]
    fn growing_the_pane_makes_it_fit_again() {
        let p = read(&"a".repeat(100));
        assert!(!fits(&p, 20, 8));
        assert!(fits(&p, 20, 12));
        assert!(!fits(&p, 20, 6));
        assert!(fits(&p, 120, 6));
    }

    #[test]
    fn when_it_does_not_fit_the_rows_say_so_and_never_cut_the_text() {
        let p = read(&"a".repeat(100));
        for height in 1..=7 {
            let rows = parts(&p, 20, height);
            assert!(
                rows.iter()
                    .any(|r| matches!(r, Part::Marker(t) if t == TOO_SMALL)),
                "{height}: {rows:?}"
            );
            assert!(!rows.iter().any(|r| matches!(r, Part::Command(_))));
        }
    }

    // What `fits` says is what `parts` draws.
    #[test]
    fn fits_agrees_with_the_rows_drawn() {
        use crate::core::ai::chat::ChatState;
        for command in ["ls", "a\nb\nc", &"x".repeat(90), "1\n2\n3\n4\n5\n6\n7"] {
            let p = run(command);
            for width in [10, 20, 40, 80] {
                for height in 0..=14usize {
                    let wanted = wanted(&p, width);
                    let rows = usize::from(ChatState::clamp_composer_rows(
                        wanted,
                        u16::try_from(height).unwrap(),
                    ));
                    let drawn = parts(&p, width, rows);
                    let whole = drawn.iter().any(|r| matches!(r, Part::Command(_)));
                    assert_eq!(
                        fits(&p, width, height),
                        whole,
                        "{command:?} {width}x{height}"
                    );
                }
            }
        }
    }

    // The reason is cut by display columns, so wide characters get the ellipsis.
    #[test]
    fn a_wide_reason_is_clipped_to_the_columns() {
        use unicode_width::UnicodeWidthStr;
        for (text, width, want) in [
            ("abcdef", 6, "abcdef"),
            ("abcdefg", 6, "abcde…"),
            ("日本語日本語", 12, "日本語日本語"),
            ("日本語日本語", 6, "日本…"),
            ("日本語日本語", 5, "日本…"),
            ("日本語日本語", 4, "日…"),
            ("aあ", 2, "a…"),
            ("abc", 0, ""),
        ] {
            let got = clip(text, width);
            assert_eq!(got, want, "{text:?} {width}");
            assert!(got.width() <= width, "{text:?} {width}");
        }
    }

    // `fits` is the same with or without a reason, for every command length.
    #[test]
    fn fits_does_not_depend_on_the_reason_for_one_to_seven_rows() {
        for rows in 1..=7usize {
            let command = (1..=rows)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            for width in [10, 20, 80] {
                for height in 0..=14usize {
                    let plain = run(&command);
                    let why = explained(&command);
                    assert_eq!(
                        fits(&plain, width, height),
                        fits(&why, width, height),
                        "{rows} rows {width}x{height}"
                    );
                }
            }
        }
    }

    // The reason sits between the command and the keys when there is room.
    #[test]
    fn the_reason_is_drawn_between_the_command_and_the_keys() {
        let rows = parts(&explained("cd a && ls"), 60, 12);
        assert_eq!(
            rows,
            [
                Part::Head("run in pane 1".into()),
                Part::Command("cd a && ls".into()),
                Part::Why("shell metacharacter: `&&`".into()),
                Part::Keys(RUN_KEYS.to_owned()),
            ]
        );
        // The same prompt without a reason has one row less.
        assert_eq!(parts(&run("cd a && ls"), 60, 12).len(), 3);
        assert_eq!(wanted(&explained("cd a"), 60), wanted(&run("cd a"), 60) + 1);
    }

    // The reason is advice: it never takes the room the command needs, and
    // it never changes whether the command fits.
    #[test]
    fn the_reason_never_changes_whether_the_command_fits() {
        for command in ["ls", "a\nb\nc", &"x".repeat(90), "1\n2\n3\n4\n5\n6\n7"] {
            for width in [10, 20, 40, 80] {
                for height in 0..=14usize {
                    assert_eq!(
                        fits(&explained(command), width, height),
                        fits(&run(command), width, height),
                        "{command:?} {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_pane_with_no_spare_row_drops_the_reason_and_keeps_the_command() {
        // head + command + keys = 3 rows: a pane of 4 gives the composer 3.
        let rows = parts(&explained("ls"), 40, 3);
        assert_eq!(rows.len(), 3);
        assert!(!rows.iter().any(|r| matches!(r, Part::Why(_))), "{rows:?}");
        assert!(rows.contains(&Part::Command("ls".into())));
        // The full budget of command rows leaves no room for it either.
        let six = (1..=6).map(|n| format!("echo {n}")).collect::<Vec<_>>();
        let rows = parts(&explained(&six.join("\n")), 40, MAX_COMMAND_ROWS + 2);
        assert!(!rows.iter().any(|r| matches!(r, Part::Why(_))), "{rows:?}");
    }

    #[test]
    fn a_reason_wider_than_the_pane_is_cut_with_an_ellipsis() {
        let rows = parts(&explained("ls"), 12, 8);
        assert!(rows.contains(&Part::Why("shell metac…".into())), "{rows:?}");
    }
}
