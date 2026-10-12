//! Minimal HTTP/1.x response decoding. Pure.
//!
//! `parse_head` and `Dechunker` are the two framing primitives. Detection
//! applies them to a complete reply (`parse_response`); the chat client feeds
//! them one read at a time, so a stream is never buffered whole.

/// Largest body accepted, in bytes.
pub const MAX_BODY: usize = 1024 * 1024;

/// Largest head (status line and headers, blank line included), in bytes.
pub const MAX_HEAD: usize = 16 * 1024;

/// Longest chunk-size line or trailer line kept, in bytes.
const MAX_LINE: usize = 16 * 1024;
/// Most bytes the whole trailer section may take, blank line included.
const MAX_TRAILER: usize = 16 * 1024;

/// How the body of a reply is delimited.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Framing {
    /// Exactly this many bytes.
    Length(usize),
    /// `Transfer-Encoding` ending in `chunked`.
    Chunked,
    /// Everything until the peer closes.
    Close,
}

/// The result of reading a head from what has arrived so far.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Head {
    /// No blank line yet and still room for one.
    Incomplete,
    /// Malformed, ambiguous, or longer than `MAX_HEAD`.
    Invalid,
    /// `head_len` counts the bytes up to and including the blank line.
    Complete {
        status: u16,
        framing: Framing,
        head_len: usize,
    },
}

/// Reads the head of a reply. Accepts only HTTP/1.x; rejects conflicting or
/// signed `Content-Length`, `Content-Length` with `Transfer-Encoding`, more
/// than one `Transfer-Encoding`, and a final coding other than `chunked` or
/// `identity`. The status is returned as is: the caller decides what to accept.
pub fn parse_head(raw: &[u8]) -> Head {
    let Some(split) = find(raw, b"\r\n\r\n") else {
        return if raw.len() >= MAX_HEAD {
            Head::Invalid
        } else {
            Head::Incomplete
        };
    };
    let head_len = split + 4;
    if head_len > MAX_HEAD {
        return Head::Invalid;
    }
    match head_fields(&raw[..split]) {
        Some((status, framing)) => Head::Complete {
            status,
            framing,
            head_len,
        },
        None => Head::Invalid,
    }
}

fn head_fields(head: &[u8]) -> Option<(u16, Framing)> {
    // Header bytes outside UTF-8 must not reject an otherwise valid reply.
    let head = String::from_utf8_lossy(head);
    let mut lines = head.split("\r\n");
    let mut status_line = lines.next()?.split_whitespace();
    if !status_line.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let code = status_line.next()?;
    if !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let status = code.parse().ok()?;
    let (mut length, mut chunked, mut seen_encoding) = (None, false, false);
    for (name, value) in lines.filter_map(|l| l.split_once(':')) {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return None;
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let n = value.parse::<usize>().ok()?;
            if length.is_some_and(|prev| prev != n) {
                return None;
            }
            length = Some(n);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            // One header only, and its final coding decides the framing:
            // chunked, or identity (none). Anything else cannot be decoded.
            if seen_encoding {
                return None;
            }
            seen_encoding = true;
            let last = value.rsplit(',').next().unwrap_or("").trim();
            if last.eq_ignore_ascii_case("chunked") {
                chunked = true;
            } else if !last.eq_ignore_ascii_case("identity") {
                return None;
            }
        }
    }
    let framing = match (chunked, length) {
        (true, Some(_)) => return None,
        (true, None) => Framing::Chunked,
        (false, Some(n)) => Framing::Length(n),
        (false, None) => Framing::Close,
    };
    Some((status, framing))
}

/// Decodes a complete response into `(status, body)`. Redirects are not
/// followed; incomplete, malformed or oversize (> 1 MiB) bodies give `None`.
pub fn parse_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let Head::Complete {
        status,
        framing,
        head_len,
    } = parse_head(raw)
    else {
        return None;
    };
    let rest = &raw[head_len..];
    let body = match framing {
        Framing::Chunked => {
            let mut body = Vec::new();
            let mut dechunker = Dechunker::new();
            match dechunker.feed(rest, &mut body) {
                Ok(Step::Done) => body,
                _ => return None,
            }
        }
        Framing::Length(n) => rest.get(..n)?.to_vec(),
        Framing::Close => rest.to_vec(),
    };
    (body.len() <= MAX_BODY).then_some((status, body))
}

/// The body framing of a reply, applied to each read. Shared by the chat
/// client and the engram request so both apply the same rules.
pub enum Body {
    Length(usize),
    Chunked(Dechunker),
    Close,
}

impl Body {
    pub fn new(framing: Framing) -> Self {
        match framing {
            Framing::Length(n) => Body::Length(n),
            Framing::Chunked => Body::Chunked(Dechunker::new()),
            Framing::Close => Body::Close,
        }
    }

    /// Appends the body bytes of `bytes` to `out`; `Done` when the body ended.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<Step, Malformed> {
        match self {
            Body::Length(left) => {
                let n = (*left).min(bytes.len());
                out.extend_from_slice(&bytes[..n]);
                *left -= n;
                Ok(if *left == 0 { Step::Done } else { Step::More })
            }
            Body::Chunked(dechunker) => dechunker.feed(bytes, out),
            Body::Close => {
                out.extend_from_slice(bytes);
                Ok(Step::More)
            }
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The chunked framing is broken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Malformed;

/// Where a `Dechunker` stands after a feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// More bytes are needed.
    More,
    /// The final chunk and the blank line were read; later bytes are ignored.
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// A chunk-size line.
    Size,
    /// This many data bytes remain in the chunk.
    Data(usize),
    /// The CR that must follow the data.
    DataCr,
    /// The LF that must follow it.
    DataLf,
    /// Trailer lines, up to the blank one.
    Trailer,
    Done,
    Broken,
}

/// Incremental decoder of a chunked body. Strict: hex sizes (no sign, no
/// space), CRLF after every chunk, a final zero chunk, trailers, a blank line.
/// The result is the same at any split point of the input.
#[derive(Debug)]
pub struct Dechunker {
    phase: Phase,
    /// The size or trailer line being collected, without its CRLF.
    line: Vec<u8>,
    /// The last byte was a CR on a size or trailer line.
    cr: bool,
    /// Bytes read so far in the trailer section.
    trailer: usize,
}

impl Default for Dechunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Dechunker {
    pub fn new() -> Self {
        Self {
            phase: Phase::Size,
            line: Vec::new(),
            cr: false,
            trailer: 0,
        }
    }

    /// Consumes `bytes`, appending the decoded data to `out`.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<Step, Malformed> {
        let mut i = 0;
        while i < bytes.len() {
            match self.phase {
                Phase::Done => return Ok(Step::Done),
                Phase::Broken => return Err(Malformed),
                Phase::Data(left) => {
                    let n = left.min(bytes.len() - i);
                    out.extend_from_slice(&bytes[i..i + n]);
                    i += n;
                    self.phase = if n == left {
                        Phase::DataCr
                    } else {
                        Phase::Data(left - n)
                    };
                }
                Phase::DataCr => {
                    self.expect(bytes[i], b'\r', Phase::DataLf)?;
                    i += 1;
                }
                Phase::DataLf => {
                    self.expect(bytes[i], b'\n', Phase::Size)?;
                    i += 1;
                }
                Phase::Size | Phase::Trailer => {
                    let b = bytes[i];
                    i += 1;
                    if self.phase == Phase::Trailer {
                        self.trailer += 1;
                        if self.trailer > MAX_TRAILER {
                            self.phase = Phase::Broken;
                            return Err(Malformed);
                        }
                    }
                    if self.cr && b == b'\n' {
                        self.cr = false;
                        let line = std::mem::take(&mut self.line);
                        self.end_line(&line)?;
                    } else {
                        // A CR is held back until the next byte shows whether
                        // it ends the line.
                        if self.cr {
                            self.push(b'\r')?;
                        }
                        self.cr = b == b'\r';
                        if !self.cr {
                            self.push(b)?;
                        }
                    }
                }
            }
        }
        Ok(if self.phase == Phase::Done {
            Step::Done
        } else {
            Step::More
        })
    }

    fn expect(&mut self, got: u8, want: u8, next: Phase) -> Result<(), Malformed> {
        if got == want {
            self.phase = next;
            Ok(())
        } else {
            self.phase = Phase::Broken;
            Err(Malformed)
        }
    }

    fn push(&mut self, b: u8) -> Result<(), Malformed> {
        if self.line.len() >= MAX_LINE {
            self.phase = Phase::Broken;
            return Err(Malformed);
        }
        self.line.push(b);
        Ok(())
    }

    fn end_line(&mut self, line: &[u8]) -> Result<(), Malformed> {
        if self.phase == Phase::Trailer {
            if line.is_empty() {
                self.phase = Phase::Done;
            }
            return Ok(());
        }
        match chunk_size(line) {
            Some(0) => self.phase = Phase::Trailer,
            Some(n) => self.phase = Phase::Data(n),
            None => {
                self.phase = Phase::Broken;
                return Err(Malformed);
            }
        }
        Ok(())
    }
}

/// The size on a chunk-size line; extensions after `;` are ignored.
fn chunk_size(line: &[u8]) -> Option<usize> {
    let line = std::str::from_utf8(line).ok()?;
    let size = line.split(';').next()?;
    if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    usize::from_str_radix(size, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(body: &str) -> Option<(u16, Vec<u8>)> {
        Some((200, body.as_bytes().to_vec()))
    }

    #[test]
    fn content_length_frames_the_body() {
        let raw = b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\nhelloEXTRA";
        assert_eq!(parse_response(raw), ok("hello"));
        let short = b"HTTP/1.0 200 OK\r\nContent-Length: 9\r\n\r\nhello";
        assert_eq!(parse_response(short), None);
    }

    #[test]
    fn close_delimited_body_is_the_rest() {
        assert_eq!(
            parse_response(b"HTTP/1.0 200 OK\r\n\r\n{\"a\":1}"),
            ok("{\"a\":1}")
        );
    }

    #[test]
    fn chunked_body_is_decoded() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext\r\npedia\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw), ok("Wikipedia"));
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWi";
        assert_eq!(parse_response(cut), None);
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-LENGTH: 2\r\n\r\nokXX";
        assert_eq!(parse_response(raw), ok("ok"));
        let chunked = b"HTTP/1.1 200 OK\r\ntransfer-encoding: Chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), ok("ok"));
    }

    #[test]
    fn redirects_are_returned_not_followed() {
        let raw = b"HTTP/1.1 301 Moved\r\nLocation: http://x/\r\nContent-Length: 3\r\n\r\nmov";
        assert_eq!(parse_response(raw), Some((301, b"mov".to_vec())));
    }

    #[test]
    fn malformed_responses_give_none() {
        assert_eq!(parse_response(b""), None);
        assert_eq!(parse_response(b"garbage\r\n\r\nbody"), None);
        assert_eq!(parse_response(b"HTTP/1.0 abc OK\r\n\r\nbody"), None);
        assert_eq!(parse_response(b"HTTP/1.0 200 OK\r\nno terminator"), None);
    }

    const CHUNKED: &str = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";

    fn chunked(body: &str) -> Option<(u16, Vec<u8>)> {
        parse_response(format!("{CHUNKED}{body}").as_bytes())
    }

    #[test]
    fn chunked_framing_is_strict() {
        assert_eq!(chunked("3\r\nabc\r\n0\r\n"), None);
        assert_eq!(chunked("3\r\nabc\r\n0\r\n\r"), None);
        assert_eq!(chunked("3\r\nabcXX0\r\n\r\n"), None);
        assert_eq!(chunked("+3\r\nabc\r\n0\r\n\r\n"), None);
        assert_eq!(chunked(" 3\r\nabc\r\n0\r\n\r\n"), None);
        assert_eq!(chunked("3\r\nabc\r\n0\r\nX: y\r\n"), None);
        assert_eq!(chunked("3;x=y\r\nabc\r\n0\r\n\r\n"), ok("abc"));
        assert_eq!(
            chunked("3\r\nabc\r\n0\r\nX-T: 1\r\nY: 2\r\n\r\n"),
            ok("abc")
        );
    }

    #[test]
    fn non_utf8_headers_do_not_reject_the_response() {
        let raw = b"HTTP/1.1 200 OK\r\nX-Bad: \xff\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(raw), ok("{}"));
    }

    #[test]
    fn ambiguous_framing_is_rejected() {
        let dup = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\nabc";
        assert_eq!(parse_response(dup), None);
        let same = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\nabc";
        assert_eq!(parse_response(same), ok("ab"));
        let both = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n";
        assert_eq!(parse_response(both), None);
        let signed = b"HTTP/1.1 200 OK\r\nContent-Length: +2\r\n\r\nok";
        assert_eq!(parse_response(signed), None);
    }

    #[test]
    fn chunked_is_the_last_transfer_coding_of_the_last_header() {
        let body = "2\r\nok\r\n0\r\n\r\n";
        let raw = |te: &str| format!("HTTP/1.1 200 OK\r\n{te}\r\n\r\n{body}").into_bytes();
        let plain = |te: &str| format!("HTTP/1.1 200 OK\r\n{te}\r\n\r\nok").into_bytes();
        assert_eq!(
            parse_response(&raw("Transfer-Encoding: gzip, chunked")),
            ok("ok")
        );
        assert_eq!(
            parse_response(&raw("Transfer-Encoding: gzip,CHUNKED ")),
            ok("ok")
        );
        assert_eq!(
            parse_response(&plain("Transfer-Encoding: identity")),
            ok("ok")
        );
    }

    #[test]
    fn unsupported_transfer_encodings_are_rejected() {
        let body = "2\r\nok\r\n0\r\n\r\n";
        let raw = |te: &str| format!("HTTP/1.1 200 OK\r\n{te}\r\n\r\n{body}").into_bytes();
        let plain = |te: &str| format!("HTTP/1.1 200 OK\r\n{te}\r\n\r\nok").into_bytes();
        // More than one header: only one framing decision may exist.
        let two = "Transfer-Encoding: gzip\r\nTransfer-Encoding: chunked";
        assert_eq!(parse_response(&raw(two)), None);
        let same = "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked";
        assert_eq!(parse_response(&raw(same)), None);
        // The final coding must be chunked or identity.
        assert_eq!(
            parse_response(&plain("Transfer-Encoding: chunked, gzip")),
            None
        );
        assert_eq!(parse_response(&plain("Transfer-Encoding: gzip")), None);
        assert_eq!(parse_response(&plain("Transfer-Encoding: ")), None);
    }

    #[test]
    fn malformed_headers_and_status_are_rejected() {
        let space = b"HTTP/1.1 200 OK\r\nContent-Length : 2\r\n\r\nok";
        assert_eq!(parse_response(space), None);
        assert_eq!(parse_response(b"HTTP/1.1 +200 OK\r\n\r\nok"), None);
    }

    #[test]
    fn one_byte_over_the_cap_is_rejected_in_every_framing() {
        let body = vec![b'x'; MAX_BODY + 1];
        let mut close = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        close.extend(&body);
        assert_eq!(parse_response(&close), None);
        let mut length =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        length.extend(&body);
        assert_eq!(parse_response(&length), None);
        let mut chunks = format!("{CHUNKED}{:x}\r\n", body.len()).into_bytes();
        chunks.extend(&body);
        chunks.extend(b"\r\n0\r\n\r\n");
        assert_eq!(parse_response(&chunks), None);
    }

    #[test]
    fn chunked_bodies_at_the_cap_are_accepted() {
        let body = vec![b'x'; MAX_BODY];
        let mut raw = format!("{CHUNKED}{:x}\r\n", body.len()).into_bytes();
        raw.extend(&body);
        raw.extend(b"\r\n0\r\n\r\n");
        assert_eq!(parse_response(&raw), Some((200, body)));
    }

    #[test]
    fn multi_chunk_bodies_over_the_cap_are_rejected() {
        let half = vec![b'x'; MAX_BODY / 2 + 1];
        let mut raw = CHUNKED.as_bytes().to_vec();
        for _ in 0..2 {
            raw.extend(format!("{:x}\r\n", half.len()).into_bytes());
            raw.extend(&half);
            raw.extend(b"\r\n");
        }
        raw.extend(b"0\r\n\r\n");
        assert_eq!(parse_response(&raw), None);
    }

    #[test]
    fn only_http_1_x_status_lines_are_accepted() {
        assert_eq!(parse_response(b"HTTP/2 200\r\n\r\nok"), None);
        assert_eq!(parse_response(b"HTTP/0.9 200 OK\r\n\r\nok"), None);
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\n\r\nok"), ok("ok"));
        assert_eq!(parse_response(b"HTTP/1.0 200 OK\r\n\r\nok"), ok("ok"));
    }

    #[test]
    fn oversize_bodies_give_none() {
        let mut raw = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        raw.extend(vec![b'x'; 2 * 1024 * 1024]);
        assert_eq!(parse_response(&raw), None);
        let mut at_cap = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        at_cap.extend(vec![b'x'; 1024 * 1024]);
        assert_eq!(
            parse_response(&at_cap),
            Some((200, vec![b'x'; 1024 * 1024]))
        );
    }

    // parse_head

    fn complete(raw: &[u8]) -> (u16, Framing, usize) {
        match parse_head(raw) {
            Head::Complete {
                status,
                framing,
                head_len,
            } => (status, framing, head_len),
            other => panic!("expected a complete head, got {other:?}"),
        }
    }

    #[test]
    fn head_reports_status_framing_and_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(complete(raw), (200, Framing::Length(5), raw.len() - 5));
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(complete(raw), (200, Framing::Chunked, raw.len()));
        let raw = b"HTTP/1.0 404 Not Found\r\n\r\n";
        assert_eq!(complete(raw), (404, Framing::Close, raw.len()));
    }

    #[test]
    fn head_is_incomplete_until_the_blank_line_at_every_split() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
        for cut in 0..raw.len() {
            assert_eq!(parse_head(&raw[..cut]), Head::Incomplete, "cut at {cut}");
        }
        assert!(matches!(parse_head(raw), Head::Complete { .. }));
    }

    #[test]
    fn head_rejects_the_same_framing_as_detection() {
        let bad: [&[u8]; 8] = [
            b"HTTP/2 200\r\n\r\n",
            b"HTTP/1.1 +200 OK\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: +2\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length : 2\r\n\r\n",
        ];
        for raw in bad {
            assert_eq!(
                parse_head(raw),
                Head::Invalid,
                "{:?}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn head_over_the_cap_is_invalid_with_or_without_a_blank_line() {
        // No blank line yet and already at the cap: it can never fit.
        let mut open = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        open.resize(MAX_HEAD, b'a');
        assert_eq!(parse_head(&open), Head::Invalid);
        // One byte short of the cap, still waiting.
        assert_eq!(parse_head(&open[..MAX_HEAD - 1]), Head::Incomplete);
        // A head that ends exactly at the cap is fine; one byte more is not.
        let mut exact = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        exact.resize(MAX_HEAD - 4, b'a');
        exact.extend(b"\r\n\r\n");
        assert_eq!(complete(&exact).2, MAX_HEAD);
        let mut over = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        over.resize(MAX_HEAD - 3, b'a');
        over.extend(b"\r\n\r\n");
        assert_eq!(parse_head(&over), Head::Invalid);
    }

    // Dechunker

    const WIRE: &[u8] = b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\n";

    fn run(parts: &[&[u8]]) -> (Vec<u8>, Result<Step, Malformed>) {
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        let mut last = Ok(Step::More);
        for part in parts {
            last = d.feed(part, &mut out);
            if last.is_err() {
                break;
            }
        }
        (out, last)
    }

    #[test]
    fn dechunker_decodes_at_every_two_way_split() {
        for cut in 0..=WIRE.len() {
            let (out, last) = run(&[&WIRE[..cut], &WIRE[cut..]]);
            assert_eq!(out, b"Wikipedia", "cut at {cut}");
            assert_eq!(last, Ok(Step::Done), "cut at {cut}");
        }
    }

    #[test]
    fn dechunker_decodes_one_byte_at_a_time() {
        let parts: Vec<&[u8]> = WIRE.chunks(1).collect();
        let (out, last) = run(&parts);
        assert_eq!(out, b"Wikipedia");
        assert_eq!(last, Ok(Step::Done));
    }

    #[test]
    fn dechunker_is_not_done_until_the_blank_line_at_every_cut() {
        for cut in 0..WIRE.len() {
            let (_, last) = run(&[&WIRE[..cut]]);
            assert_eq!(last, Ok(Step::More), "cut at {cut}");
        }
    }

    #[test]
    fn dechunker_yields_data_as_it_arrives() {
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        assert_eq!(d.feed(b"a\r\nhello", &mut out), Ok(Step::More));
        assert_eq!(out, b"hello");
        assert_eq!(d.feed(b"world\r\n0\r\n\r\n", &mut out), Ok(Step::Done));
        assert_eq!(out, b"helloworld");
    }

    #[test]
    fn dechunker_ignores_bytes_after_the_end() {
        let (out, last) = run(&[b"2\r\nok\r\n0\r\n\r\nGARBAGE", b"more"]);
        assert_eq!(out, b"ok");
        assert_eq!(last, Ok(Step::Done));
    }

    #[test]
    fn dechunker_framing_is_strict_at_every_split() {
        let bad: [&[u8]; 8] = [
            b"3\r\nabcXX0\r\n\r\n",
            b"+3\r\nabc\r\n0\r\n\r\n",
            b" 3\r\nabc\r\n0\r\n\r\n",
            b"\r\nabc\r\n0\r\n\r\n",
            b"g\r\nabc\r\n0\r\n\r\n",
            b"3\r\nabc\n0\r\n\r\n",
            b"ffffffffffffffffffff\r\n",
            b"3 \r\nabc\r\n0\r\n\r\n",
        ];
        for wire in bad {
            for cut in 0..=wire.len() {
                let (_, last) = run(&[&wire[..cut], &wire[cut..]]);
                assert_eq!(last, Err(Malformed), "{:?} cut at {cut}", wire);
            }
        }
    }

    #[test]
    fn dechunker_stays_broken_after_an_error() {
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        assert_eq!(d.feed(b"zz\r\n", &mut out), Err(Malformed));
        assert_eq!(d.feed(b"0\r\n\r\n", &mut out), Err(Malformed));
        assert!(out.is_empty());
    }

    #[test]
    fn dechunker_refuses_an_endless_line() {
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        assert_eq!(d.feed(b"1;", &mut out), Ok(Step::More));
        let ext = vec![b'x'; MAX_LINE];
        assert_eq!(d.feed(&ext, &mut out), Err(Malformed));
        // The same for a trailer line that never ends.
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        assert_eq!(d.feed(b"0\r\n", &mut out), Ok(Step::More));
        assert_eq!(d.feed(&ext, &mut out), Ok(Step::More));
        assert_eq!(d.feed(b"x", &mut out), Err(Malformed));
    }

    #[test]
    fn dechunker_streams_more_than_the_detection_body_cap() {
        let block = vec![b'x'; 64 * 1024];
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        let mut total = 0;
        for _ in 0..24 {
            let mut wire = format!("{:x}\r\n", block.len()).into_bytes();
            wire.extend(&block);
            wire.extend(b"\r\n");
            out.clear();
            assert_eq!(d.feed(&wire, &mut out), Ok(Step::More));
            total += out.len();
        }
        assert!(total > MAX_BODY);
        assert_eq!(d.feed(b"0\r\n\r\n", &mut out), Ok(Step::Done));
    }

    /// A trailer section of exactly `total` bytes, the closing blank line
    /// included, made of short header lines.
    fn trailer_section(total: usize) -> Vec<u8> {
        let mut left = total - 2;
        let mut out = Vec::new();
        while left > 0 {
            let len = if left > 70 { 64 } else { left };
            out.extend(b"x: ");
            out.extend(std::iter::repeat_n(b'y', len - 5));
            out.extend(b"\r\n");
            left -= len;
        }
        out.extend(b"\r\n");
        assert_eq!(out.len(), total);
        out
    }

    /// Cuts worth trying on a long wire: the start, the end and a sample.
    fn sample_cuts(len: usize) -> Vec<usize> {
        let mut cuts: Vec<usize> = (0..=40).chain(len.saturating_sub(80)..=len).collect();
        cuts.extend((0..len).step_by(997));
        cuts
    }

    #[test]
    fn dechunker_accepts_a_trailer_section_at_the_cap() {
        let mut wire = b"2\r\nok\r\n0\r\n".to_vec();
        wire.extend(trailer_section(MAX_TRAILER));
        for cut in sample_cuts(wire.len()) {
            let (out, last) = run(&[&wire[..cut], &wire[cut..]]);
            assert_eq!(out, b"ok", "cut at {cut}");
            assert_eq!(last, Ok(Step::Done), "cut at {cut}");
        }
    }

    #[test]
    fn dechunker_refuses_a_trailer_section_over_the_cap_at_every_sampled_split() {
        let mut wire = b"2\r\nok\r\n0\r\n".to_vec();
        wire.extend(trailer_section(MAX_TRAILER + 1));
        for cut in sample_cuts(wire.len()) {
            let (_, last) = run(&[&wire[..cut], &wire[cut..]]);
            assert_eq!(last, Err(Malformed), "cut at {cut}");
        }
    }

    #[test]
    fn dechunker_refuses_an_endless_trailer_made_of_short_lines() {
        let (mut d, mut out) = (Dechunker::new(), Vec::new());
        assert_eq!(d.feed(b"0\r\n", &mut out), Ok(Step::More));
        let line = b"x: y\r\n";
        let mut fed = 0;
        let mut result = Ok(Step::More);
        while result.is_ok() && fed <= 4 * MAX_TRAILER {
            result = d.feed(line, &mut out);
            fed += line.len();
        }
        assert_eq!(result, Err(Malformed));
        assert!(fed <= MAX_TRAILER + line.len(), "refused after {fed} bytes");
        // Broken for good, even for the bytes that would have closed it.
        assert_eq!(d.feed(b"\r\n", &mut out), Err(Malformed));
    }

    #[test]
    fn body_length_stops_at_its_count_and_ignores_the_rest() {
        let mut body = Body::new(Framing::Length(3));
        let mut out = Vec::new();
        assert_eq!(body.feed(b"ab", &mut out), Ok(Step::More));
        assert_eq!(body.feed(b"cdef", &mut out), Ok(Step::Done));
        assert_eq!(out, b"abc");
    }

    #[test]
    fn body_length_zero_is_done_on_an_empty_feed() {
        let mut body = Body::new(Framing::Length(0));
        assert_eq!(body.feed(b"", &mut Vec::new()), Ok(Step::Done));
    }

    #[test]
    fn body_chunked_decodes_across_feeds() {
        let mut body = Body::new(Framing::Chunked);
        let mut out = Vec::new();
        assert_eq!(body.feed(b"3\r\nab", &mut out), Ok(Step::More));
        assert_eq!(body.feed(b"c\r\n0\r\n\r\n", &mut out), Ok(Step::Done));
        assert_eq!(out, b"abc");
        assert_eq!(
            Body::new(Framing::Chunked).feed(b"zz\r\n", &mut out),
            Err(Malformed)
        );
    }

    #[test]
    fn body_close_never_ends_by_itself() {
        let mut body = Body::new(Framing::Close);
        let mut out = Vec::new();
        assert_eq!(body.feed(b"ab", &mut out), Ok(Step::More));
        assert_eq!(body.feed(b"cd", &mut out), Ok(Step::More));
        assert_eq!(out, b"abcd");
    }
}
