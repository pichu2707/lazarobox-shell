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

/// Longest run of CSI parameters and intermediates that is swallowed, in
/// characters, before the sequence is abandoned. The same cap applies to the
/// intermediates and repeated ESCs of a two-character escape sequence.
///
/// Once the cap is reached the sequence is abandoned and the remaining
/// characters are processed as ordinary text, so the rest of the parameter
/// bytes and the final byte of a very long CSI show up as plain text. That is
/// harmless: the escape itself is gone and every control is still dropped.
pub(crate) const CSI_CAP: usize = 256;

/// Where the sanitizer is inside an escape sequence.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum State {
    #[default]
    Ground,
    /// After ESC: the next character picks the sequence; `seen` counts the
    /// intermediates and repeated ESCs swallowed.
    Esc { seen: usize },
    /// Inside CSI parameters, up to the final byte; `seen` counts the
    /// parameter and intermediate characters swallowed.
    Csi { seen: usize },
    /// Inside a string sequence; `osc` also ends at BEL.
    Str { osc: bool },
    /// ESC seen inside a string: `\` ends it, anything else aborts it.
    StrEsc { osc: bool },
}

/// Strips terminal escapes, control and format characters from text, keeping
/// `\n` and `\t`. Stateful across `push` calls.
#[derive(Default)]
pub struct StreamSanitizer {
    state: State,
    /// String-sequence characters swallowed since the last ordinary text.
    /// It survives restarts (`ESC ] ESC ]`), so chaining sequences cannot get
    /// around `STRING_CAP`.
    swallowed: usize,
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
        self.swallowed = 0;
    }

    fn step(&mut self, c: char, out: &mut String) {
        match self.state {
            State::Ground => match c {
                '\u{1b}' => self.state = State::Esc { seen: 0 },
                '\n' | '\t' => self.emit(c, out),
                _ if c.is_control() || is_format(c) => {}
                _ => self.emit(c, out),
            },
            State::Esc { seen } => match c {
                '[' => self.state = State::Csi { seen: 0 },
                ']' => self.open_string(true),
                'P' | '_' | '^' | 'X' => self.open_string(false),
                '\u{1b}' | '\u{20}'..='\u{2f}' if seen + 1 >= CSI_CAP => self.state = State::Ground,
                '\u{1b}' | '\u{20}'..='\u{2f}' => self.state = State::Esc { seen: seen + 1 },
                '\u{30}'..='\u{7e}' => self.state = State::Ground,
                _ => {
                    self.state = State::Ground;
                    self.step(c, out);
                }
            },
            State::Csi { seen } => match c {
                '\u{20}'..='\u{3f}' if seen + 1 >= CSI_CAP => self.state = State::Ground,
                '\u{20}'..='\u{3f}' => self.state = State::Csi { seen: seen + 1 },
                '\u{40}'..='\u{7e}' => self.state = State::Ground,
                _ => {
                    self.state = State::Ground;
                    self.step(c, out);
                }
            },
            State::Str { osc } => match c {
                '\u{9c}' => self.state = State::Ground,
                '\u{7}' if osc => self.state = State::Ground,
                '\u{1b}' => self.swallow(State::Esc { seen: 0 }, State::StrEsc { osc }),
                _ => self.swallow(State::Ground, State::Str { osc }),
            },
            State::StrEsc { .. } => match c {
                '\\' => self.state = State::Ground,
                _ => {
                    self.state = State::Esc { seen: 0 };
                    self.step(c, out);
                }
            },
        }
    }

    /// Ordinary text refills the string budget.
    fn emit(&mut self, c: char, out: &mut String) {
        self.swallowed = 0;
        out.push(c);
    }

    /// Starts a string sequence unless the budget is already spent.
    fn open_string(&mut self, osc: bool) {
        self.state = if self.swallowed >= STRING_CAP {
            State::Ground
        } else {
            State::Str { osc }
        };
    }

    /// Counts one swallowed character; once `STRING_CAP` is reached the
    /// sequence is abandoned and the state becomes `spent`.
    fn swallow(&mut self, spent: State, next: State) {
        self.swallowed += 1;
        self.state = if self.swallowed < STRING_CAP {
            next
        } else {
            spent
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

/// Shortest run of letters, digits, `_` and `-` that is taken for a secret.
const SECRET_RUN: usize = 20;

/// Prefixes of API keys and session tokens.
const KEY_PREFIXES: [&str; 2] = ["sk-", "sess-"];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Replaces what looks like a credential with `…`, for text a CLI printed
/// and the user will see: the word after `Bearer`, a run of 20 or more
/// letters, digits, `_` or `-`, and a run starting like an API key (`sk-`,
/// `sess-`).
pub fn redact_tokens(text: &str) -> String {
    // Pass 1: the word after `Bearer`.
    let mut bearer = String::with_capacity(text.len());
    let mut after_bearer = false;
    let mut rest = text;
    while !rest.is_empty() {
        let blank = rest.starts_with(char::is_whitespace);
        let end = rest
            .find(|c: char| c.is_whitespace() != blank)
            .unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        if blank {
            bearer.push_str(piece);
        } else if after_bearer {
            bearer.push('…');
            after_bearer = false;
        } else {
            bearer.push_str(piece);
            after_bearer = piece.eq_ignore_ascii_case("bearer");
        }
        rest = tail;
    }
    // Pass 2: long runs and key prefixes.
    let mut out = String::with_capacity(bearer.len());
    let mut rest = bearer.as_str();
    while !rest.is_empty() {
        let word = rest.starts_with(is_token_char);
        let end = rest
            .find(|c: char| is_token_char(c) != word)
            .unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        let secret = word
            && (piece.len() >= SECRET_RUN || KEY_PREFIXES.iter().any(|p| piece.starts_with(p)));
        out.push_str(if secret { "…" } else { piece });
        rest = tail;
    }
    out
}

#[cfg(test)]
mod tests;
