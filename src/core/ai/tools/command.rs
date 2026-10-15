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
            c if is_format(c) => return Err(Refusal::new("invisible format character")),
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
    Ok(Command(command.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(text: &str) -> bool {
        validate(text).is_err()
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
