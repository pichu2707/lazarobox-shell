//! Validation of a command before it is shown for approval and again before
//! it is written to a pane.
//!
//! The rule is to refuse, never to clean: silently changing the text would
//! change what the user approved. The only edit is removing trailing newlines
//! and blanks, so that no Enter is implied by the written text.

use crate::core::ai::sanitize::is_format;

/// The longest command accepted, in bytes, after trailing blanks are removed.
pub const MAX_COMMAND_BYTES: usize = 4096;

/// A command that passed `validate`: exactly the text to show and to write.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Command(String);

impl Command {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a command was refused. The reason is short and never carries the
/// command text, so it is safe to show to the user and to return to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusal(&'static str);

impl Refusal {
    pub(crate) const fn new(reason: &'static str) -> Self {
        Self(reason)
    }

    pub fn reason(&self) -> &'static str {
        self.0
    }
}

/// Characters that draw as nothing or as blanks and are not in the output
/// sanitizer's format set: the combining grapheme joiner, Hangul and Khmer
/// fillers, Mongolian and variation selectors, the interlinear annotation
/// controls, the tag block, the braille blank and the half-width filler, the
/// shorthand-format and musical invisibles, the object replacement character,
/// and every non-ASCII whitespace (no-break space, ideographic space, ...).
pub(super) fn is_invisible(c: char) -> bool {
    (c.is_whitespace() && !c.is_ascii())
        || matches!(
            c,
            '\u{034f}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{2800}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{ffa0}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0000}'..='\u{e007f}'
                | '\u{e0100}'..='\u{e01ef}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{fffc}'
        )
}

/// Validate `text` for a pane.
///
/// Refused: ESC, any C0 control except `\n` and `\t`, DEL, any C1 control, a
/// `\r` that does not start a `\r\n` pair, bidi and other format characters
/// (the output sanitizer's set), empty or blank text, and text over
/// [`MAX_COMMAND_BYTES`] bytes once trailing newlines and blanks are removed.
/// The refusal reason never carries the text.
pub fn validate(text: &str) -> Result<Command, Refusal> {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' | '\t' => {}
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => return Err(Refusal::new("lone carriage return")),
            '\u{1b}' => return Err(Refusal::new("escape character")),
            c if c.is_control() => return Err(Refusal::new("control character")),
            c if is_format(c) || is_invisible(c) => {
                return Err(Refusal::new("invisible format character"));
            }
            _ => {}
        }
    }
    let command = text.trim_end_matches(['\n', '\r', ' ', '\t']);
    if command.trim().is_empty() {
        return Err(Refusal::new("empty command"));
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(Refusal::new("command too long"));
    }
    open_construct(command)?;
    Ok(Command(command.to_string()))
}

/// Why model text is refused because it would leave the shell waiting for
/// more input, or expand something the user did not see.
pub const UNBALANCED: &str = "the quotes of the command are not balanced";
pub const TRAILING_BACKSLASH: &str = "the command ends with a backslash";
pub const OPERATOR: &str = "the command ends with an operator";
pub const BACKTICK: &str = "the command uses a backtick";
pub const SUBSHELL: &str = "the command uses $(";
pub const HEREDOC: &str = "the command uses a heredoc";
/// Every reason above: `typed_bytes` returns them to the model as they are.
pub const CONTINUATION: [&str; 6] = [
    UNBALANCED,
    TRAILING_BACKSLASH,
    OPERATOR,
    BACKTICK,
    SUBSHELL,
    HEREDOC,
];

/// Refuses text that would leave the shell at a continuation prompt (an open
/// quote, a trailing backslash or operator) or hide a command (a backtick,
/// `$(` or `<<` outside single quotes). A later run would type into that
/// continuation. A backslash escapes the next character outside single quotes;
/// `$'...'` escapes are not modelled, which is harmless: run text is inert
/// (D20). Conservative: an apostrophe in a comment is refused too.
fn open_construct(command: &str) -> Result<(), Refusal> {
    let (mut quote, mut escaped) = (None, false);
    for (at, c) in command.char_indices() {
        let rest = &command[at..];
        match (quote, escaped, c) {
            (_, true, _) => escaped = false,
            (None | Some('"'), false, '\\') => escaped = true,
            (None | Some('"'), false, '`') => return Err(Refusal::new(BACKTICK)),
            (None | Some('"'), false, '$') if rest.starts_with("$(") => {
                return Err(Refusal::new(SUBSHELL));
            }
            (None, false, '<') if rest.starts_with("<<") => return Err(Refusal::new(HEREDOC)),
            (None, false, '\'' | '"') => quote = Some(c),
            (Some(open), false, c) if c == open => quote = None,
            _ => {}
        }
    }
    if quote.is_some() {
        return Err(Refusal::new(UNBALANCED));
    }
    if escaped {
        return Err(Refusal::new(TRAILING_BACKSLASH));
    }
    if command.ends_with('|') || command.ends_with("&&") {
        return Err(Refusal::new(OPERATOR));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(text: &str) -> bool {
        validate(text).is_err()
    }

    // Spec (pane-commands): text that leaves the shell inside an open quote or
    // a continuation line is refused, so what a later run types cannot be
    // read as part of it.
    #[test]
    fn an_open_quote_or_a_trailing_backslash_is_refused() {
        for bad in [
            "echo \"hello",
            "echo 'hello",
            "echo \"a'b",
            "echo it's",
            "ls \\",
            "ls \\\n",
            "echo \"a\\\"",
            "echo 'a\\'b'",
        ] {
            assert!(refused(bad), "{bad:?}");
        }
        assert_eq!(validate("echo \"hi").unwrap_err().reason(), UNBALANCED);
        assert_eq!(validate("ls \\").unwrap_err().reason(), TRAILING_BACKSLASH);
    }

    // Suggestion 1: text that would wait for more input, or run something
    // the model did not show, is refused too.
    #[test]
    fn a_trailing_operator_a_backtick_a_subshell_or_a_heredoc_is_refused() {
        let cases = [
            ("ls &&", OPERATOR),
            ("ls ||  ", OPERATOR),
            ("ls |", OPERATOR),
            ("ls &&\n", OPERATOR),
            ("echo `id`", BACKTICK),
            ("echo \"`id`\"", BACKTICK),
            ("echo $(id)", SUBSHELL),
            ("echo \"$(id)\"", SUBSHELL),
            ("cat <<EOF", HEREDOC),
            ("cat <<'EOF'", HEREDOC),
        ];
        for (text, want) in cases {
            assert_eq!(validate(text).unwrap_err().reason(), want, "{text:?}");
        }
    }

    #[test]
    fn escaped_or_single_quoted_forms_and_inner_operators_pass() {
        for ok in [
            "echo \\`x\\`",
            "echo '`x` $(y) <<z'",
            "echo \\$(x)",
            "ls | wc -l",
            "ls && pwd",
            "a || b",
            "echo $HOME",
            "cat < f",
            "echo '|'",
        ] {
            assert!(!refused(ok), "{ok:?}");
        }
    }

    #[test]
    fn balanced_quotes_and_escapes_pass() {
        for ok in [
            "echo \"hello\"",
            "echo 'a \"b\"'",
            "echo \"it's\"",
            "echo \\' \\\"",
            "echo \"a\\\"b\"",
            "echo 'a\\'",
            "printf '%s\\n' a\necho \"b\"",
        ] {
            assert!(!refused(ok), "{ok:?}");
        }
    }

    #[test]
    fn escape_is_refused() {
        assert!(refused("ls\u{1b}[31m"));
        assert!(refused("echo x\u{1b}[201~; rm -rf ~"));
    }

    #[test]
    fn the_paste_terminator_without_escape_is_plain_text() {
        let cmd = validate("echo x[201~").unwrap();
        assert_eq!(cmd.as_str(), "echo x[201~");
    }

    #[test]
    fn other_controls_are_refused() {
        for bad in [
            "a\u{07}b",
            "a\u{00}b",
            "a\u{7f}b",
            "a\u{9b}b",
            "a\u{80}b",
            "a\u{9f}b",
            "a\rb",
            "a\r",
            "a\u{202e}b",
            "a\u{200b}b",
            "a\u{2066}b",
            "a\u{0c}b",
            "a\u{0b}b",
        ] {
            assert!(refused(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn invisible_and_spoofing_characters_are_refused() {
        for c in [
            '\u{034f}',
            '\u{115f}',
            '\u{1160}',
            '\u{17b4}',
            '\u{17b5}',
            '\u{180b}',
            '\u{180d}',
            '\u{180f}',
            '\u{3164}',
            '\u{fe00}',
            '\u{fe0f}',
            '\u{ffa0}',
            '\u{fff9}',
            '\u{fffa}',
            '\u{fffb}',
            '\u{e0000}',
            '\u{e0041}',
            '\u{e007f}',
            '\u{e0100}',
            '\u{e01ef}',
            '\u{2800}',
        ] {
            assert!(
                refused(&format!("a{c}b")),
                "U+{:04X} must be refused",
                c as u32
            );
        }
        // Neighbours of the ranges stay valid.
        for c in [
            '\u{034e}',
            '\u{1161}',
            '\u{180a}',
            '\u{fdff}',
            '\u{e0080}',
            '\u{e01f0}',
            '\u{2801}',
        ] {
            assert!(
                !refused(&format!("a{c}b")),
                "U+{:04X} must be valid",
                c as u32
            );
        }
    }

    #[test]
    fn non_ascii_whitespace_and_musical_invisibles_are_refused() {
        for c in [
            '\u{00a0}',
            '\u{1680}',
            '\u{2000}',
            '\u{2005}',
            '\u{200a}',
            '\u{202f}',
            '\u{205f}',
            '\u{3000}',
            '\u{1d173}',
            '\u{1d17a}',
            '\u{1bca0}',
            '\u{1bca3}',
            '\u{fffc}',
        ] {
            assert!(
                refused(&format!("a{c}b")),
                "U+{:04X} must be refused",
                c as u32
            );
            assert!(refused(&format!("ls{c}")), "trailing U+{:04X}", c as u32);
        }
        // A plain space stays valid; so do the neighbours of the ranges.
        for c in [
            ' ',
            '\u{1d172}',
            '\u{1d17b}',
            '\u{1bc9f}',
            '\u{1bca4}',
            '\u{fffd}',
        ] {
            assert!(
                !refused(&format!("a{c}b")),
                "U+{:04X} must be valid",
                c as u32
            );
        }
    }

    #[test]
    fn next_line_is_refused() {
        // NEL is a C1 control that some terminals treat as a line break.
        assert!(refused("a\u{0085}b"));
        assert!(refused("ls\u{0085}"));
    }

    #[test]
    fn newline_and_tab_are_kept() {
        let text = "for f in *; do\n\techo $f\ndone";
        assert_eq!(validate(text).unwrap().as_str(), text);
    }

    #[test]
    fn trailing_newlines_and_blanks_are_removed() {
        assert_eq!(validate("cd /tmp\n\n").unwrap().as_str(), "cd /tmp");
        assert_eq!(validate("cd /tmp \t\n \n").unwrap().as_str(), "cd /tmp");
        assert_eq!(validate("cd /tmp\r\n").unwrap().as_str(), "cd /tmp");
        assert_eq!(validate("  ls").unwrap().as_str(), "  ls");
    }

    #[test]
    fn empty_blank_and_over_long_are_refused() {
        assert!(refused(""));
        assert!(refused(" \t\n\n  "));
        let ok = "a".repeat(MAX_COMMAND_BYTES);
        assert_eq!(validate(&ok).unwrap().as_str(), ok);
        assert!(validate(&format!("{ok}\n")).is_ok());
        assert!(refused(&"a".repeat(MAX_COMMAND_BYTES + 1)));
        // The cap counts bytes, not characters.
        assert!(refused(&"é".repeat(MAX_COMMAND_BYTES / 2 + 1)));
    }

    #[test]
    fn a_refusal_carries_no_command_text() {
        let err = validate("SECRET-TOKEN\u{1b}").unwrap_err();
        assert!(!err.reason().contains("SECRET"));
        assert!(!err.reason().is_empty());
    }
}
