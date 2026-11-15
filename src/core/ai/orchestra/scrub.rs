//! A best-effort scrubber for the one-line summaries of the step trail:
//! values that follow a secret-looking name become `***`. It reads JSON
//! `"name": value`, `name=value` (also in URL queries), `--name value`,
//! `Name: value` headers, `curl -u user:pass`, `sshpass -p value` and
//! `mysql -pVALUE`; a quoted value is one unit. A name is secret when one of
//! its segments (split on `_`, `-`, `.`) is a known word, so `api_key` is and
//! `keyboard` is not. A heuristic over free text, not a guarantee: a secret
//! under an ordinary name passes, and so do the shapes it does not read:
//! text after an unbalanced apostrophe (`don't --password x`, the rest is
//! one quoted word), an attached `-uadmin:pw`, `password= value` split over
//! two words. The trail also goes through `redact_tokens`.

/// What replaces a value.
const MASK: &str = "***";

/// Name segments that mark a value as secret, lowercase.
const SECRET: [&str; 13] = [
    "pass",
    "passwd",
    "password",
    "passphrase",
    "pwd",
    "token",
    "secret",
    "key",
    "apikey",
    "auth",
    "authorization",
    "credential",
    "bearer",
];

fn sensitive(name: &str) -> bool {
    let (mut part, mut prev) = (String::new(), ' ');
    let mut parts = Vec::new();
    for c in name.chars() {
        // A segment ends at a separator and at a lower-to-upper step.
        if !c.is_alphanumeric() || (prev.is_lowercase() && c.is_uppercase()) {
            parts.push(std::mem::take(&mut part));
        }
        if c.is_alphanumeric() {
            part.extend(c.to_lowercase());
        }
        prev = c;
    }
    parts.push(part);
    parts
        .iter()
        .any(|p| SECRET.contains(&p.as_str()) || p == "credentials")
}

/// `text` with the values of secret-looking names masked.
pub fn scrub(text: &str) -> String {
    words(&json(text))
        .iter()
        .fold(State::default(), |state, word| state.word(word))
        .out
        .join(" ")
}

/// The words of `text`: split on spaces outside quotes.
fn words(text: &str) -> Vec<&str> {
    let (mut out, mut start, mut quote) = (Vec::new(), 0, None);
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, ' ') => {
                out.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

/// What the words so far ask of the next one.
#[derive(Default)]
struct State {
    out: Vec<String>,
    /// The next word is a value to mask (a flag or header named a secret).
    mask: bool,
    /// The masked value may be a scheme (`Bearer`) before the credential.
    scheme: bool,
    /// The next word is `user:pass` (after `-u`).
    user: bool,
    /// The next word is a password (after `sshpass -p`).
    sshpass_value: bool,
    /// Inside a `mysql` or `sshpass` command, until `&&`, `;`, `|`.
    mysql: bool,
    sshpass: bool,
}

impl State {
    fn push(mut self, text: impl Into<String>) -> Self {
        self.out.push(text.into());
        self
    }

    fn word(mut self, word: &str) -> Self {
        let bare = word.trim_matches(['\'', '"']);
        if self.mask && !word.is_empty() {
            self.mask = false;
            if bare.starts_with('-') {
                return self.push(word);
            }
            let scheme = ["bearer", "basic", "digest"].contains(&bare.to_lowercase().as_str());
            self.mask = std::mem::take(&mut self.scheme) && scheme;
            return self.push(MASK);
        }
        if std::mem::take(&mut self.sshpass_value) {
            return self.push(MASK);
        }
        if std::mem::take(&mut self.user)
            && let Some((name, _)) = word.split_once(':')
        {
            return self.push(format!("{name}:{MASK}"));
        }
        if matches!(word, "&&" | ";" | "|" | "||" | "&") || word.ends_with(';') {
            (self.mysql, self.sshpass) = (false, false);
            return self.push(word);
        }
        let command = word.rsplit('/').next().unwrap_or(word);
        self.mysql |= command.starts_with("mysql");
        self.sshpass |= command == "sshpass";
        if (self.mysql || self.sshpass) && word.len() > 2 && word.starts_with("-p") {
            return self.push(format!("-p{MASK}"));
        }
        if self.sshpass && word == "-p" {
            self.sshpass_value = true;
        }
        self.user = matches!(word, "-u" | "--user");
        // A header in one quoted unit: `"X-Api-Key: abcd"`.
        let quoted = word.len() > 1
            && word.starts_with(['\'', '"'])
            && word.ends_with(word.as_bytes()[0] as char);
        if quoted {
            let q = &word[..1];
            return self.push(format!("{q}{}{q}", scrub(bare)));
        }
        if let Some(name) = word
            .strip_suffix(':')
            .filter(|n| sensitive(n) && !n.contains('"'))
        {
            self.mask = true;
            self.scheme = name.to_lowercase().contains("auth");
            return self.push(word);
        }
        if let Some((name, value)) = word.split_once(':')
            && !name.contains('"')
            && !value.starts_with("//")
            && !value.contains(MASK)
            && sensitive(name)
            && !value.is_empty()
        {
            let scheme = ["bearer", "basic"].contains(&value.to_lowercase().as_str());
            self.mask = scheme;
            return self.push(format!("{name}:{MASK}"));
        }
        if word.starts_with('-') && !word.contains('=') && sensitive(word) && takes_value(word) {
            self.mask = true;
        }
        self.push(pairs(word))
    }
}

/// Whether a secret-named flag takes the secret as its next word: not
/// `--password-stdin`, `--no-auth` or `--key-file`.
fn takes_value(flag: &str) -> bool {
    let flag = flag.to_lowercase();
    !(flag.split(|c: char| !c.is_alphanumeric()))
        .any(|part| ["stdin", "no", "file"].contains(&part))
}

/// A word whose `name=value` segments (split on `?` and `&`) are masked; a
/// quoted value goes whole.
fn pairs(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while !rest.is_empty() {
        let quoted = rest
            .split_once('=')
            .is_some_and(|(_, v)| v.starts_with(['\'', '"']));
        let end = if quoted {
            rest.len()
        } else {
            rest.find(['?', '&']).map_or(rest.len(), |i| i + 1)
        };
        let (segment, tail) = rest.split_at(end);
        let body = segment.trim_end_matches(['?', '&']);
        match body.split_once('=') {
            Some((name, value)) if !value.is_empty() && sensitive(name) => {
                out.push_str(&format!("{name}={MASK}"));
                out.push_str(&segment[body.len()..]);
            }
            _ => out.push_str(segment),
        }
        rest = tail;
    }
    out
}

/// `text` with the values of JSON members named like a secret masked: a
/// string value inside its quotes, any other value (an array whole) up to its
/// end. It reads from each `:` back to its key, so no quote count matters.
fn json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    for (colon, _) in text.match_indices(':') {
        let key = text[..colon].trim_end();
        let Some(key) = key.strip_suffix('"') else {
            continue;
        };
        let name = key.rsplit_once('"').map_or("", |(_, name)| name);
        if colon < done || !sensitive(name) || !key.contains('"') {
            continue;
        }
        let value = &text[colon + 1..];
        let lead = value.len() - value.trim_start().len();
        let start = colon + 1 + lead;
        // A key with no value (clipped text) has nothing to mask.
        if start >= text.len() {
            continue;
        }
        let (len, quote) = value_len(&text[start..]);
        out.push_str(&text[done..start]);
        out.push_str(&format!("{quote}{MASK}{quote}"));
        done = start + len;
    }
    out.push_str(&text[done..]);
    out
}

/// The bytes a JSON value takes at the start of `value`, and the quote that
/// wraps it in the output: a string with its quotes, an object or array to
/// its matching close (string-aware), anything else up to a separator; all to
/// the end of the text when it is cut. In a shell string the JSON quotes are
/// `\"`.
fn value_len(value: &str) -> (usize, &'static str) {
    let shell = ["\\\"", "{\\\"", "[\\\""]
        .iter()
        .any(|p| value.starts_with(p));
    let (mut depth, mut string, mut skip) = (0usize, false, false);
    let mut chars = value.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let quote = if shell {
            c == '\\' && chars.peek().is_some_and(|(_, n)| *n == '"')
        } else {
            c == '"' && !skip
        };
        if quote {
            if shell {
                chars.next();
            }
            string = !string;
            // A string value ends at its closing quote.
            if !string && depth == 0 {
                let (len, wrap) = if shell {
                    (at + 2, "\\\"")
                } else {
                    (at + 1, "\"")
                };
                return (len, wrap);
            }
            continue;
        }
        skip = !shell && string && c == '\\' && !skip;
        if string {
            continue;
        }
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' if depth > 1 => depth -= 1,
            '}' | ']' if depth == 1 => return (at + 1, ""),
            ',' | '}' | ']' | ' ' if depth == 0 => return (at, ""),
            _ => {}
        }
    }
    (value.len(), "")
}

#[cfg(test)]
#[path = "scrub_tests.rs"]
mod tests;
