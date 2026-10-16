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

/// Shortest run that is taken for a secret by its length alone.
const SECRET_RUN: usize = 20;

/// Shortest run starting like a known token (`ghp_`, `AKIA`, `eyJ`, ...).
const PREFIXED_RUN: usize = 8;

/// Shortest base64 run with only `/` as its mark (no `+` or `=`).
const SLASH_RUN: usize = 32;

/// Prefixes of API keys and session tokens that are secrets at any length.
const KEY_PREFIXES: [&str; 2] = ["sk-", "sess-"];

/// Prefixes of well-known tokens: GitHub (`ghp_`, `gho_`, `ghs_`,
/// `github_pat_`), GitLab (`glpat-`), Slack (`xox`), AWS (`AKIA`) and JWTs
/// (`eyJ`).
const TOKEN_PREFIXES: [&str; 8] = [
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "glpat-",
    "xox",
    "AKIA",
    "eyJ",
];

/// Authentication schemes whose next word is a credential.
const SCHEMES: [&str; 2] = ["bearer", "basic"];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn is_base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')
}

fn token_secret(run: &str) -> bool {
    run.len() >= SECRET_RUN
        || KEY_PREFIXES.iter().any(|p| run.starts_with(p))
        || (run.len() >= PREFIXED_RUN && TOKEN_PREFIXES.iter().any(|p| run.starts_with(p)))
}

fn base64_secret(run: &str) -> bool {
    if run.len() < SECRET_RUN {
        return false;
    }
    if run.contains(['+', '=']) {
        return true;
    }
    // Only `/`: a path looks the same, so ask for more (a long run that does
    // not start with `/` and mixes upper case, lower case and digits).
    run.len() >= SLASH_RUN
        && !run.starts_with('/')
        && run.contains('/')
        && run.chars().any(|c| c.is_ascii_uppercase())
        && run.chars().any(|c| c.is_ascii_lowercase())
        && run.chars().any(|c| c.is_ascii_digit())
}

/// Replaces every maximal run of `class` characters for which `secret` holds.
fn redact_runs(text: &str, class: fn(char) -> bool, secret: fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let word = rest.starts_with(class);
        let end = rest.find(|c: char| class(c) != word).unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        out.push_str(if word && secret(piece) { "…" } else { piece });
        rest = tail;
    }
    out
}

/// `scheme://user:pass@host` becomes `scheme://…@host`.
fn redact_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#'))
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(user) => {
                out.push('…');
                rest = &tail[user..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

/// The word of `token` that names a scheme, if it is one: the last piece
/// once quotes, braces, colons, commas and semicolons are split off.
fn is_scheme(token: &str) -> bool {
    token
        .rsplit(['"', '\'', '{', '}', ':', ',', ';'])
        .find(|piece| !piece.is_empty())
        .is_some_and(|piece| SCHEMES.iter().any(|s| piece.eq_ignore_ascii_case(s)))
}

/// The word after `Bearer` or `Basic` becomes `…`.
fn redact_schemes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_scheme = false;
    let mut rest = text;
    while !rest.is_empty() {
        let blank = rest.starts_with(char::is_whitespace);
        let end = rest
            .find(|c: char| c.is_whitespace() != blank)
            .unwrap_or(rest.len());
        let (piece, tail) = rest.split_at(end);
        if blank {
            out.push_str(piece);
        } else if after_scheme {
            out.push('…');
            after_scheme = false;
        } else {
            out.push_str(piece);
            after_scheme = is_scheme(piece);
        }
        rest = tail;
    }
    out
}

/// Replaces what looks like a credential with `…`, for text a CLI printed
/// and the user will see. It covers exactly:
///
/// - the userinfo of a URL (`scheme://user:pass@host` gives `scheme://…@host`);
/// - the word after the schemes `Bearer` and `Basic`, in any case, through
///   quotes, braces, colons, commas and semicolons (`"Bearer"`, `Bearer:`,
///   `{"Authorization":"Bearer x"}`);
/// - a run of letters, digits, `+`, `/` and `=` of 20 or more characters that
///   holds `+` or `=`, or one of 32 or more with `/` only, that does not start
///   with `/` and mixes upper case, lower case and digits (so paths stay);
/// - a run of letters, digits, `_` and `-` of 20 or more characters;
/// - a run starting with `sk-` or `sess-`, at any length;
/// - a run of 8 or more characters starting with `ghp_`, `gho_`, `ghs_`,
///   `github_pat_`, `glpat-`, `xox`, `AKIA` or `eyJ`.
///
/// Nothing else is changed: a secret in another shape is not caught.
pub fn redact_tokens(text: &str) -> String {
    let text = redact_userinfo(text);
    let text = redact_schemes(&text);
    let text = redact_runs(&text, is_base64_char, base64_secret);
    redact_runs(&text, is_token_char, token_secret)
}

#[cfg(test)]
mod tests;
