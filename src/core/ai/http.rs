//! Minimal HTTP/1.x response decoding. Pure.

/// Largest body accepted, in bytes.
pub const MAX_BODY: usize = 1024 * 1024;

/// Decodes a complete response into `(status, body)`. Redirects are not
/// followed; incomplete, malformed or oversize (> 1 MiB) bodies give `None`.
pub fn parse_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = find(raw, b"\r\n\r\n")?;
    // Header bytes outside UTF-8 must not reject an otherwise valid reply.
    let head = String::from_utf8_lossy(&raw[..split]);
    let rest = &raw[split + 4..];
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
    let (mut length, mut chunked) = (None, false);
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
            // Chunked framing is the last coding of the last header.
            chunked = value
                .rsplit(',')
                .next()
                .is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"));
        }
    }
    let body = if chunked {
        if length.is_some() {
            return None;
        }
        dechunk(rest)?
    } else if let Some(n) = length {
        rest.get(..n)?.to_vec()
    } else {
        rest.to_vec()
    };
    (body.len() <= MAX_BODY).then_some((status, body))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Decodes a chunked body. Every chunk must end in CRLF and the stream in
/// `0`, optional trailers and a blank line; anything else gives `None`.
fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let eol = find(rest, b"\r\n")?;
        let size = std::str::from_utf8(&rest[..eol]).ok()?;
        let size = size.split(';').next()?;
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let size = usize::from_str_radix(size, 16).ok()?;
        rest = &rest[eol + 2..];
        if size == 0 {
            // Trailers, then the blank line that ends the message.
            loop {
                let eol = find(rest, b"\r\n")?;
                rest = &rest[eol + 2..];
                if eol == 0 {
                    return Some(body);
                }
            }
        }
        body.extend_from_slice(rest.get(..size)?);
        if rest.get(size..size + 2)? != b"\r\n" {
            return None;
        }
        rest = &rest[size + 2..];
    }
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
            parse_response(&raw(
                "Transfer-Encoding: gzip\r\nTransfer-Encoding: chunked"
            )),
            ok("ok")
        );
        assert_eq!(
            parse_response(&plain("Transfer-Encoding: chunked, gzip")),
            ok("ok")
        );
        assert_eq!(
            parse_response(&plain("Transfer-Encoding: identity")),
            ok("ok")
        );
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
}
