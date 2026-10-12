//! Output sanitizer for text coming from an engine. A byte front end
//! (`Utf8Stream`) feeds a stateful `StreamSanitizer`, so an escape sequence or
//! a UTF-8 character split across chunks is handled as if it were whole.

/// Bidirectional and zero-width format characters: invisible, and able to
/// reorder or hide the text around them. Includes the line and paragraph
/// separators, the soft hyphen, the word joiner and invisible operators, the
/// deprecated U+206A..U+206F controls and the tag block.
pub(crate) fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{e0000}'..='\u{e007f}'
    )
}

/// Longest unterminated string sequence (OSC, DCS, APC, PM, SOS) that is
/// swallowed, in characters.
pub(crate) const STRING_CAP: usize = 4096;

/// Where the sanitizer is inside an escape sequence.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum State {
    #[default]
    Ground,
    /// After ESC: the next character picks the sequence.
    Esc,
    /// Inside CSI parameters, up to the final byte.
    Csi,
    /// Inside a string sequence; `osc` also ends at BEL. `seen` counts the
    /// swallowed characters.
    Str { osc: bool, seen: usize },
    /// ESC seen inside a string: `\` ends it, anything else aborts it.
    StrEsc { osc: bool, seen: usize },
}

/// Strips terminal escapes, control and format characters from text, keeping
/// `\n` and `\t`. Stateful across `push` calls.
#[derive(Default)]
pub struct StreamSanitizer {
    state: State,
}

impl StreamSanitizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            self.step(c, &mut out);
        }
        out
    }

    /// Ends the stream: an unterminated sequence is dropped.
    pub fn finish(&mut self) {
        self.state = State::Ground;
    }

    fn step(&mut self, c: char, out: &mut String) {
        match self.state {
            State::Ground => match c {
                '\u{1b}' => self.state = State::Esc,
                '\n' | '\t' => out.push(c),
                _ if c.is_control() || is_format(c) => {}
                _ => out.push(c),
            },
            State::Esc => match c {
                '[' => self.state = State::Csi,
                ']' => self.state = State::Str { osc: true, seen: 0 },
                'P' | '_' | '^' | 'X' => {
                    self.state = State::Str {
                        osc: false,
                        seen: 0,
                    }
                }
                '\u{1b}' => {}
                '\u{20}'..='\u{2f}' => {}
                '\u{30}'..='\u{7e}' => self.state = State::Ground,
                _ => {
                    self.state = State::Ground;
                    self.step(c, out);
                }
            },
            State::Csi => match c {
                '\u{20}'..='\u{3f}' => {}
                '\u{40}'..='\u{7e}' => self.state = State::Ground,
                _ => {
                    self.state = State::Ground;
                    self.step(c, out);
                }
            },
            State::Str { osc, seen } => match c {
                '\u{9c}' => self.state = State::Ground,
                '\u{7}' if osc => self.state = State::Ground,
                '\u{1b}' => {
                    self.state = State::StrEsc {
                        osc,
                        seen: seen + 1,
                    }
                }
                _ => self.swallow(osc, seen + 1),
            },
            State::StrEsc { .. } => match c {
                '\\' => self.state = State::Ground,
                _ => {
                    self.state = State::Esc;
                    self.step(c, out);
                }
            },
        }
    }

    /// Keeps swallowing a string sequence until it has eaten `STRING_CAP`
    /// characters, then abandons it.
    fn swallow(&mut self, osc: bool, seen: usize) {
        self.state = if seen >= STRING_CAP {
            State::Ground
        } else {
            State::Str { osc, seen }
        };
    }
}

/// Decodes a byte stream to text, carrying an incomplete character across
/// chunks. Invalid bytes, and a cut tail at the end, become U+FFFD.
#[derive(Default)]
pub struct Utf8Stream {
    /// An incomplete character cut by the previous chunk (3 bytes or less).
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest = self.pending.as_slice();
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    out.push_str(valid);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (valid, tail) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{fffd}');
                            rest = &tail[n..];
                        }
                        None => {
                            rest = tail;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }

    /// Ends the stream: a cut character becomes U+FFFD.
    pub fn finish(&mut self) -> String {
        if std::mem::take(&mut self.pending).is_empty() {
            String::new()
        } else {
            "\u{fffd}".into()
        }
    }
}

/// Sanitizes a whole string.
pub fn clean(text: &str) -> String {
    let mut sanitizer = StreamSanitizer::new();
    let out = sanitizer.push(text);
    sanitizer.finish();
    out
}

#[cfg(test)]
mod tests;
