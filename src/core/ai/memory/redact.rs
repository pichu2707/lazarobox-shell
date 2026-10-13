//! Redaction of secrets before any text leaves for engram. A hand-written
//! scanner (no regex crate): linear, idempotent and total.

/// What replaces every secret.
pub const MASK: &str = "[REDACTED]";

/// The redacted text and how many secrets were replaced.
#[derive(PartialEq, Eq, Debug)]
pub struct Redacted {
    pub text: String,
    pub hits: usize,
}

/// The shortest token that follows a key prefix, in characters, so that
/// `sk-learn` or `ghp_x` are not mistaken for keys.
const MIN_TOKEN: usize = 8;
/// Token characters after `AIza`.
const GOOGLE_TOKEN: usize = 35;
/// Upper-case letters or digits after `AKIA` or `ASIA`.
const AWS_TOKEN: usize = 16;
/// Token characters after a prefix that is also a common name start.
const LONG_TOKEN: usize = 30;
/// The shortest value of a `Basic`, `Token` or non-canonical `Bearer`
/// scheme, so that `basic usage` is not a credential.
const SCHEME_MIN: usize = 8;

/// Prefixes of API keys and tokens with the fewest token characters that
/// must follow each. `sk-` also covers `sk-ant-`. The long minimums are for
/// prefixes that also start ordinary names (`npm_config_registry`).
const PREFIXES: [(&[u8], usize); 19] = [
    (b"sk-", MIN_TOKEN),
    (b"sk_live_", MIN_TOKEN),
    (b"sk_test_", MIN_TOKEN),
    (b"rk_live_", MIN_TOKEN),
    (b"ghp_", MIN_TOKEN),
    (b"gho_", MIN_TOKEN),
    (b"ghs_", MIN_TOKEN),
    (b"ghu_", MIN_TOKEN),
    (b"ghr_", MIN_TOKEN),
    (b"github_pat_", MIN_TOKEN),
    (b"glpat-", MIN_TOKEN),
    (b"npm_", LONG_TOKEN),
    (b"hf_", LONG_TOKEN),
    (b"xoxa-", MIN_TOKEN),
    (b"xoxb-", MIN_TOKEN),
    (b"xoxe-", MIN_TOKEN),
    (b"xoxp-", MIN_TOKEN),
    (b"xoxr-", MIN_TOKEN),
    (b"xoxs-", MIN_TOKEN),
];

/// Words that, anywhere in a key name, mark `name = value` as a secret.
const KEYS: [&[u8]; 8] = [
    b"password",
    b"passwd",
    b"secret",
    b"token",
    b"api_key",
    b"api-key",
    b"apikey",
    b"access_key",
];

/// Authorization schemes whose value is a credential.
const SCHEMES: [&[u8]; 3] = [b"bearer", b"basic", b"token"];

/// Replaces every secret in `text` with `[REDACTED]`: key and token prefixes,
/// AWS and Google keys, JWTs, the value of a `Bearer`, `Basic` or `Token`
/// authorization, the userinfo of a URL, PEM blocks, `<private>` regions and
/// the value of an assignment to a `password`, `secret`, `token`, `api_key`,
/// `access_key` or `*_key` name. Idempotent, total and linear in the input.
pub fn redact(text: &str) -> Redacted {
    let bytes = text.as_bytes();
    let mut scan = Scan {
        text,
        bytes,
        jwt_dead_until: 0,
        assign_dead_until: 0,
    };
    let mut out = String::with_capacity(text.len());
    let mut hits = 0;
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match scan.hit_at(i) {
            // A value that is already the mask: skip it, count nothing, so a
            // second pass leaves the first one's output alone.
            Some((start, end)) if &text[start..end] == MASK => i = end,
            Some((start, end)) => {
                out.push_str(&text[copied..start]);
                out.push_str(MASK);
                hits += 1;
                copied = end;
                i = end;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    Redacted { text: out, hits }
}

struct Scan<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// A JWT attempt that failed rules out every start inside its first run,
    /// which keeps `eyJ-eyJ-eyJ-...` linear.
    jwt_dead_until: usize,
    /// The same for an assignment name.
    assign_dead_until: usize,
}

fn is_token(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

impl Scan<'_> {
    /// The span `(start, end)` to mask when a secret starts at `i`.
    /// Both ends are on character boundaries: they sit next to ASCII bytes.
    fn hit_at(&mut self, i: usize) -> Option<(usize, usize)> {
        match self.bytes[i] {
            b'<' => self.private(i),
            b'-' => self.pem(i),
            b':' => self.url_userinfo(i),
            c if c.is_ascii_alphabetic() => self
                .scheme(i)
                .or_else(|| self.prefixed(i))
                .or_else(|| self.aws_or_google(i))
                .or_else(|| self.jwt(i))
                .or_else(|| self.assignment(i)),
            c if is_token(c) => self.assignment(i),
            _ => None,
        }
    }

    /// Whether the character before `i` is not an ASCII letter or digit.
    /// Any non-ASCII character counts as a boundary (CJK letters are
    /// alphanumeric to Unicode, yet a key can follow them), which fails
    /// closed. `i` is always the position of an ASCII byte, so it is a
    /// character boundary.
    fn word_start(&self, i: usize) -> bool {
        self.text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    }

    fn starts_ci(&self, at: usize, pat: &[u8]) -> bool {
        self.bytes
            .get(at..at + pat.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(pat))
    }

    /// End of the run of bytes matching `pred` from `from`.
    fn run(&self, from: usize, pred: impl Fn(u8) -> bool) -> usize {
        from + self.bytes[from..].iter().take_while(|&&c| pred(c)).count()
    }

    /// Whether at least `min` bytes from `from` match `pred` (cheap, bounded).
    fn at_least(&self, from: usize, min: usize, pred: impl Fn(u8) -> bool) -> bool {
        self.bytes[from.min(self.bytes.len())..]
            .iter()
            .take(min)
            .take_while(|&&c| pred(c))
            .count()
            == min
    }

    fn private(&self, i: usize) -> Option<(usize, usize)> {
        const OPEN: &[u8] = b"<private>";
        const CLOSE: &[u8] = b"</private>";
        if !self.starts_ci(i, OPEN) {
            return None;
        }
        let from = i + OPEN.len();
        let end = (from..self.bytes.len())
            .find(|&k| self.starts_ci(k, CLOSE))
            .map_or(self.bytes.len(), |k| k + CLOSE.len());
        Some((i, end))
    }

    fn pem(&self, i: usize) -> Option<(usize, usize)> {
        const BEGIN: &str = "-----BEGIN ";
        const END: &str = "-----END ";
        const DASHES: &str = "-----";
        if !self.text[i..].starts_with(BEGIN) {
            return None;
        }
        let from = i + BEGIN.len();
        let end = self.text[from..]
            .find(END)
            .map(|k| from + k + END.len())
            .and_then(|k| self.text[k..].find(DASHES).map(|d| k + d + DASHES.len()))
            .unwrap_or(self.bytes.len());
        Some((i, end))
    }

    /// `Bearer`, `Basic` or `Token` (any case) and its value. A canonical
    /// `Bearer` takes any value; the other spellings need a value that looks
    /// like a credential.
    fn scheme(&self, i: usize) -> Option<(usize, usize)> {
        let word = SCHEMES.iter().find(|w| self.starts_ci(i, w))?;
        if !self.word_start(i) {
            return None;
        }
        let after = i + word.len();
        let start = self.run(after, is_blank);
        if start == after || !self.bytes.get(start)?.is_ascii_alphanumeric() {
            return None;
        }
        let end = self.run(start, |c| {
            c.is_ascii_alphanumeric() || b"._~+/=-".contains(&c)
        });
        let canonical = self.bytes[i..].starts_with(b"Bearer");
        (canonical || self.looks_like_credential(start, end)).then_some((start, end))
    }

    /// A value of at least `SCHEME_MIN` bytes with a digit, a symbol or an
    /// upper-case letter after the first character.
    fn looks_like_credential(&self, start: usize, end: usize) -> bool {
        end - start >= SCHEME_MIN
            && self.bytes[start + 1..end]
                .iter()
                .any(|c| !c.is_ascii_lowercase())
    }

    /// The `user:password` of `scheme://user:password@host`.
    fn url_userinfo(&self, i: usize) -> Option<(usize, usize)> {
        if !self.bytes[i..].starts_with(b"://") || !self.bytes[..i].last()?.is_ascii_alphanumeric()
        {
            return None;
        }
        let start = i + 3;
        let end = self.run(start, |c| {
            !c.is_ascii_whitespace() && !b"/?#@\"'<>[]\\".contains(&c)
        });
        let info = &self.text[start..end];
        (self.bytes.get(end) == Some(&b'@') && info.contains(':') && info != MASK)
            .then_some((start, end))
    }

    fn prefixed(&self, i: usize) -> Option<(usize, usize)> {
        if !self.word_start(i) {
            return None;
        }
        for (prefix, min) in PREFIXES {
            if self.bytes[i..].starts_with(prefix) {
                let from = i + prefix.len();
                if self.at_least(from, min, is_token) {
                    return Some((i, self.run(from, is_token)));
                }
            }
        }
        None
    }

    fn aws_or_google(&self, i: usize) -> Option<(usize, usize)> {
        let from = i + 4;
        if !self.word_start(i) {
            return None;
        }
        if self.bytes[i..].starts_with(b"AKIA") || self.bytes[i..].starts_with(b"ASIA") {
            let upper = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
            if self.at_least(from, AWS_TOKEN, upper) {
                return Some((i, self.run(from, is_token)));
            }
        } else if self.bytes[i..].starts_with(b"AIza")
            && self.at_least(from, GOOGLE_TOKEN, is_token)
        {
            return Some((i, self.run(from, is_token)));
        }
        None
    }

    fn jwt(&mut self, i: usize) -> Option<(usize, usize)> {
        if i < self.jwt_dead_until || !self.bytes[i..].starts_with(b"eyJ") || !self.word_start(i) {
            return None;
        }
        let first_end = self.run(i + 3, is_token);
        let end = self.jwt_tail(first_end);
        if end.is_none() {
            self.jwt_dead_until = first_end;
        }
        end.map(|end| (i, end))
    }

    /// The end of a JWT whose header segment ends at `dot`, if the payload
    /// (`eyJ...`) and a signature follow.
    fn jwt_tail(&self, dot: usize) -> Option<usize> {
        if self.bytes.get(dot) != Some(&b'.') || !self.bytes[dot + 1..].starts_with(b"eyJ") {
            return None;
        }
        let payload_end = self.run(dot + 4, is_token);
        if self.bytes.get(payload_end) != Some(&b'.') {
            return None;
        }
        let end = self.run(payload_end + 1, is_token);
        (end > payload_end + 1).then_some(end)
    }

    /// Whether `name` (an identifier of token characters) names a secret: it
    /// holds one of `KEYS`, or ends in a `key` segment after another one
    /// (`private_key`, `x-api-key`).
    fn secret_name(name: &[u8]) -> bool {
        let lower = name.to_ascii_lowercase();
        let holds = |word: &[u8]| lower.windows(word.len()).any(|w| w == word);
        KEYS.iter().any(|k| holds(k))
            || lower.len() > 4
                && lower.ends_with(b"key")
                && matches!(lower[lower.len() - 4], b'_' | b'-')
    }

    /// `name = value` or `name: value`: the span of the value. The name is the
    /// whole identifier, so `SECRET_KEY` and `x-api-key` count.
    fn assignment(&mut self, i: usize) -> Option<(usize, usize)> {
        if i < self.assign_dead_until {
            return None;
        }
        let name_end = self.run(i, is_token);
        // A failure rules out every start inside the same identifier: they
        // share the end and name less. It keeps `a-a-a-a-...` linear.
        let value = Self::secret_name(&self.bytes[i..name_end])
            .then(|| self.assignment_value(name_end))
            .flatten();
        if value.is_none() {
            self.assign_dead_until = name_end;
        }
        value
    }

    /// The value after a secret name that ends at `name_end`.
    fn assignment_value(&self, name_end: usize) -> Option<(usize, usize)> {
        let mut at = name_end;
        if matches!(self.bytes.get(at), Some(b'"' | b'\'')) {
            at += 1;
        }
        at = self.run(at, is_blank);
        if !matches!(self.bytes.get(at), Some(b'=' | b':')) {
            return None;
        }
        let start = self.run(at + 1, is_blank);
        let (start, end) = match self.bytes.get(start) {
            Some(&quote @ (b'"' | b'\'')) => (start + 1, self.quoted_end(start + 1, quote)),
            _ => (start, self.run(start, |c| !c.is_ascii_whitespace())),
        };
        (end > start).then_some((start, end))
    }

    /// The end of a quoted value that starts at `from`: the closing `quote`
    /// (a backslash escapes the next byte) or the end of the line.
    fn quoted_end(&self, from: usize, quote: u8) -> usize {
        let mut k = from;
        while let Some(&c) = self.bytes.get(k) {
            match c {
                b'\n' | b'\r' => break,
                b'\\' if !matches!(self.bytes.get(k + 1), None | Some(b'\n' | b'\r')) => k += 2,
                c if c == quote => break,
                _ => k += 1,
            }
        }
        k.min(self.bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn text(input: &str) -> String {
        redact(input).text
    }

    #[test]
    fn key_and_token_prefixes_are_masked() {
        let table = [
            ("sk-abcDEF1234567890", "[REDACTED]"),
            ("sk-ant-api03-AbCdEf_123456-xyz", "[REDACTED]"),
            ("ghp_0123456789abcdefghij", "[REDACTED]"),
            ("gho_0123456789abcdefghij", "[REDACTED]"),
            ("github_pat_11ABCDEFG0123456789_abcdefXYZ", "[REDACTED]"),
            ("glpat-abcdefghij0123456789", "[REDACTED]"),
            ("xoxb-123456789012-abcdefABCDEF", "[REDACTED]"),
            ("xoxp-123456789012-abcdefABCDEF", "[REDACTED]"),
            ("AKIAIOSFODNN7EXAMPLE", "[REDACTED]"),
            ("AIzaSyA-0123456789abcdefghijklmnopqrstu", "[REDACTED]"),
            (
                "use key sk-abcDEF1234567890 for tests",
                "use key [REDACTED] for tests",
            ),
            ("(ghp_0123456789abcdefghij).", "([REDACTED])."),
        ];
        for (input, want) in table {
            assert_eq!(text(input), want, "{input:?}");
        }
    }

    #[test]
    fn prefixes_inside_words_or_too_short_are_left_alone() {
        for input in [
            "task-force-alpha-beta",
            "sk-learn",
            "AKIA123",
            "risk-assessment-2024",
        ] {
            assert_eq!(text(input), input, "{input:?}");
        }
    }

    /// Characters that are not ASCII: a secret right after any of them is
    /// still at a word start.
    const NON_ASCII_LEADS: [&str; 6] = [
        "\u{201c}",
        "\u{ff1a}",
        "\u{2192}",
        "\u{2014}",
        "\u{1f511}",
        "\u{4e2d}",
    ];

    #[test]
    fn a_secret_after_a_non_ascii_character_is_still_masked() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.c2lnbmF0dXJl";
        let secrets = [
            "sk-ant-api03-AbCdEf_123456",
            "ghp_0123456789abcdefghij",
            "AKIAIOSFODNN7EXAMPLE",
            "AIzaSyA-0123456789abcdefghijklmnopqrstu",
            jwt,
        ];
        for lead in NON_ASCII_LEADS {
            for secret in secrets {
                let want = format!("{lead}[REDACTED]");
                assert_eq!(
                    text(&format!("{lead}{secret}")),
                    want,
                    "{lead:?} {secret:?}"
                );
            }
            assert_eq!(
                text(&format!("{lead}Bearer abc.def")),
                format!("{lead}Bearer [REDACTED]"),
                "{lead:?} bearer"
            );
        }
    }

    #[test]
    fn an_ascii_letter_before_a_prefix_still_blocks_it() {
        for input in ["xsk-abcDEF1234567890", "9ghp_0123456789abcdefghij"] {
            assert_eq!(text(input), input, "{input:?}");
        }
    }

    #[test]
    fn a_jwt_is_masked() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.c2lnbmF0dXJl";
        assert_eq!(text(&format!("jwt {jwt} end")), "jwt [REDACTED] end");
    }

    #[test]
    fn a_bearer_value_is_masked_and_the_header_kept() {
        assert_eq!(
            text("Authorization: Bearer abc.def"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(
            text("Bearer abc.def trailing"),
            "Bearer [REDACTED] trailing"
        );
    }

    #[test]
    fn a_terminated_pem_block_is_masked_whole() {
        let pem =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
        assert_eq!(
            text(&format!("key:\n{pem}\nafter")),
            "key:\n[REDACTED]\nafter"
        );
    }

    #[test]
    fn an_unterminated_pem_block_runs_to_the_end() {
        let input = "before\n-----BEGIN PRIVATE KEY-----\nMIIEow\nstill secret";
        assert_eq!(text(input), "before\n[REDACTED]");
    }

    #[test]
    fn assignments_keep_the_key_name_and_mask_the_value() {
        let table = [
            ("password=hunter2 ok", "password=[REDACTED] ok"),
            ("API_KEY: zz9", "API_KEY: [REDACTED]"),
            ("passwd=x", "passwd=[REDACTED]"),
            ("Secret:abc def", "Secret:[REDACTED] def"),
            ("token = abc\nnext line", "token = [REDACTED]\nnext line"),
            ("export MY_APIKEY=abc123;", "export MY_APIKEY=[REDACTED]"),
        ];
        for (input, want) in table {
            assert_eq!(text(input), want, "{input:?}");
        }
    }

    #[test]
    fn a_quoted_value_is_masked_up_to_its_closing_quote() {
        let table = [
            (
                "password = \"my secret phrase here\" ok",
                "password = \"[REDACTED]\" ok",
            ),
            ("{\"password\": \"x y\"}", "{\"password\": \"[REDACTED]\"}"),
            ("token: 'two words' after", "token: '[REDACTED]' after"),
            (
                "secret=\"say \\\"hi\\\" now\" end",
                "secret=\"[REDACTED]\" end",
            ),
            // Unterminated: to the end of the line, not further.
            (
                "password=\"never closed\nnext",
                "password=\"[REDACTED]\nnext",
            ),
            // The quote of another line is not a closing quote.
            ("password=\"a b\nc\" d", "password=\"[REDACTED]\nc\" d"),
        ];
        for (input, want) in table {
            let out = redact(input);
            assert_eq!(out.text, want, "{input:?}");
            assert_eq!(redact(&out.text).text, want, "idempotent {input:?}");
        }
        assert_eq!(redact("password=\"\"").hits, 0, "empty quotes hide nothing");
    }

    #[test]
    fn key_names_with_a_prefix_or_suffix_are_assignments() {
        let table = [
            ("SECRET_KEY=abc123", "SECRET_KEY=[REDACTED]"),
            (
                "AWS_SECRET_ACCESS_KEY=wJalrXUt",
                "AWS_SECRET_ACCESS_KEY=[REDACTED]",
            ),
            ("JWT_SECRET_KEY: s3cr3t", "JWT_SECRET_KEY: [REDACTED]"),
            ("api-key: abc123", "api-key: [REDACTED]"),
            ("x-api-key: abc123", "x-api-key: [REDACTED]"),
            ("access_key = abc", "access_key = [REDACTED]"),
            ("private_key=abc", "private_key=[REDACTED]"),
            ("DB_PASSWORD=pw", "DB_PASSWORD=[REDACTED]"),
            ("GITHUB_TOKEN=abc", "GITHUB_TOKEN=[REDACTED]"),
            (
                "\"client_secret\": \"abc def\"",
                "\"client_secret\": \"[REDACTED]\"",
            ),
            ("_TOKEN=abc", "_TOKEN=[REDACTED]"),
            ("1password=abc", "1password=[REDACTED]"),
        ];
        for (input, want) in table {
            let out = redact(input);
            assert_eq!(out.text, want, "{input:?}");
            assert_eq!(redact(&out.text).text, want, "idempotent {input:?}");
        }
    }

    #[test]
    fn more_key_prefixes_are_masked() {
        let tail = "0123456789abcdefghijABCDEFGHIJ012345";
        for prefix in [
            "sk_live_", "sk_test_", "rk_live_", "ghs_", "ghu_", "ghr_", "ghp_", "npm_", "hf_",
            "xoxe-",
        ] {
            assert_eq!(
                text(&format!("k {prefix}{tail}.")),
                "k [REDACTED].",
                "{prefix}"
            );
        }
        assert_eq!(text("ASIAIOSFODNN7EXAMPLE"), "[REDACTED]");
        assert_eq!(text("id ASIAIOSFODNN7EXAMPLE."), "id [REDACTED].");
    }

    #[test]
    fn short_or_embedded_new_prefixes_are_left_alone() {
        for input in [
            "sk_live_abc",
            "npm_config_registry",
            "hf_short",
            "ghs_x",
            "taskghs_0123456789abcdef",
            "ASIA",
            "ASIAN",
            "ASIA PACIFIC",
        ] {
            assert_eq!(text(input), input, "{input:?}");
        }
    }

    #[test]
    fn authorization_schemes_are_case_insensitive_and_include_basic_and_token() {
        let table = [
            (
                "Authorization: Basic dXNlcjpwYXNzd29yZA==",
                "Authorization: Basic [REDACTED]",
            ),
            (
                "authorization: basic dXNlcjpwYXNzd29yZA==",
                "authorization: basic [REDACTED]",
            ),
            (
                "Authorization: Token abc123def456",
                "Authorization: Token [REDACTED]",
            ),
            (
                "authorization: bearer abc123def456",
                "authorization: bearer [REDACTED]",
            ),
            (
                "AUTHORIZATION: BEARER abc123def456",
                "AUTHORIZATION: BEARER [REDACTED]",
            ),
            (
                "Authorization: Bearer abc",
                "Authorization: Bearer [REDACTED]",
            ),
        ];
        for (input, want) in table {
            let out = redact(input);
            assert_eq!(out.text, want, "{input:?}");
            assert_eq!(redact(&out.text).text, want, "idempotent {input:?}");
        }
    }

    #[test]
    fn ordinary_words_that_look_like_schemes_are_left_alone() {
        for input in [
            "a basic usage example",
            "Basic authentication is weak",
            "the token configuration is documented",
            "Token Authentication",
            "a bearer of bad news",
            "token = abc",
            "basic",
        ] {
            let out = redact(input);
            assert!(!out.text.contains("Basic [REDACTED]"), "{input:?}");
            assert_eq!(
                out.text.replace("token = [REDACTED]", "token = abc"),
                input,
                "{input:?}"
            );
        }
    }

    #[test]
    fn url_userinfo_is_masked_and_the_rest_of_the_url_kept() {
        let table = [
            (
                "postgres://admin:hunter2@db.local:5432/app",
                "postgres://[REDACTED]@db.local:5432/app",
            ),
            (
                "see https://user:p%40ss@example.com/x?y=1 now",
                "see https://[REDACTED]@example.com/x?y=1 now",
            ),
            (
                "redis://:secret@cache:6379",
                "redis://[REDACTED]@cache:6379",
            ),
        ];
        for (input, want) in table {
            let out = redact(input);
            assert_eq!(out.text, want, "{input:?}");
            assert_eq!(redact(&out.text).text, want, "idempotent {input:?}");
        }
        for input in [
            "https://example.com/a:b@c",
            "https://example.com:8080/x",
            "ssh://git@github.com/o/r",
            "mail me@example.com or http://host?u=a:b@c",
            "://",
            "ok://:x\"y:' z@h",
            "ok://a:[REDACTED]@h",
        ] {
            assert_eq!(redact(input).hits, 0, "{input:?}");
        }
    }

    #[test]
    fn a_key_name_without_a_value_is_left_alone() {
        for input in ["password=", "token:\nvalue", "set a password: ", "token ="] {
            assert_eq!(redact(input).hits, 0, "{input:?}");
        }
    }

    #[test]
    fn private_regions_are_masked_terminated_or_not() {
        assert_eq!(text("a <private>x</private> b"), "a [REDACTED] b");
        assert_eq!(text("a <PRIVATE>x</Private> b"), "a [REDACTED] b");
        assert_eq!(text("a <private>never closed"), "a [REDACTED]");
        assert_eq!(
            text("a <private>one</private> b <private>two</private>"),
            "a [REDACTED] b [REDACTED]"
        );
    }

    #[test]
    fn hits_count_every_replacement() {
        let out = redact("sk-abcDEF1234567890 and password=x and <private>y</private>");
        assert_eq!(out.hits, 3);
        assert_eq!(
            out.text,
            "[REDACTED] and password=[REDACTED] and [REDACTED]"
        );
    }

    #[test]
    fn plain_text_is_unchanged() {
        for input in [
            "we decided to keep tokens short and use a password manager",
            "the secret to good tests is small units",
            "ünï cødé 日本語 \u{1f980}",
            "",
        ] {
            let out = redact(input);
            assert_eq!(out.text, input);
            assert_eq!(out.hits, 0);
        }
    }

    /// A tiny xorshift so the corpus is seeded and repeatable.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick<'a>(&mut self, pool: &[&'a str]) -> &'a str {
            pool[(self.next() % pool.len() as u64) as usize]
        }
    }

    const POOL: &[&str] = &[
        "sk-",
        "sk-ant-",
        "ghp_",
        "AKIA",
        "AIza",
        "xoxb-",
        "github_pat_",
        "eyJ",
        ".",
        "abcdefgh12345678",
        "ABCDEFGHIJKLMNOP",
        "Bearer ",
        "Authorization: ",
        "basic ",
        "Token ",
        "://",
        "user:",
        "@",
        "SECRET_KEY",
        "x-api-key",
        "sk_live_",
        "npm_",
        "ASIA",
        "password",
        "token",
        "API_KEY",
        "secret",
        "=",
        ":",
        " ",
        "\n",
        "\t",
        "<private>",
        "</private>",
        "<PRIVATE>",
        "-----BEGIN PRIVATE KEY-----",
        "-----END PRIVATE KEY-----",
        "[REDACTED]",
        "ok",
        "é",
        "日本",
        "\u{1f980}",
        "-",
        "_",
        "\"",
        "'",
    ];

    #[test]
    fn redacting_twice_equals_redacting_once_on_a_seeded_corpus() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for case in 0..3000 {
            let len = (rng.next() % 40) as usize;
            let input: String = (0..len).map(|_| rng.pick(POOL)).collect();
            let once = redact(&input);
            let twice = redact(&once.text);
            assert_eq!(twice.text, once.text, "case {case}: {input:?}");
            assert_eq!(
                twice.hits, 0,
                "case {case}: nothing is left to mask in {:?}",
                once.text
            );
        }
    }

    #[test]
    fn arbitrary_chars_never_panic() {
        let mut rng = Rng(42);
        for _ in 0..2000 {
            let len = (rng.next() % 64) as usize;
            let input: String = (0..len)
                .map(|_| char::from_u32((rng.next() % 0x11_0000) as u32).unwrap_or('\u{fffd}'))
                .collect();
            let _ = redact(&input);
        }
    }

    #[test]
    fn a_mebibyte_of_hostile_input_takes_linear_time() {
        let units = [
            "sk-",
            "password:",
            "-----BEGIN ",
            "<private>",
            "Bearer ",
            "token=   ",
            "eyJ.eyJ.",
            "AKIA",
            "ASIA",
            "npm_",
            "Basic ",
            "token ",
            "a-b_c-",
            "://",
            "://u:",
            "password=\"",
            "SECRET_KEY",
            "x-api-key ",
            "a",
        ];
        for unit in units {
            let input = unit.repeat((1 << 20) / unit.len());
            let started = Instant::now();
            let out = redact(&input);
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{unit:?} took {:?}",
                started.elapsed()
            );
            assert!(out.text.len() <= input.len() * MASK.len(), "{unit:?}");
        }
    }
}
