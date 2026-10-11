//! Minimal HTTP/1.x response decoding. Pure.

/// Largest body accepted, in bytes.
pub const MAX_BODY: usize = 1024 * 1024;

/// Decodes a complete response into `(status, body)`. Redirects are not
/// followed; incomplete, malformed or oversize (> 1 MiB) bodies give `None`.
pub fn parse_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = find(raw, b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let rest = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let mut status_line = lines.next()?.split_whitespace();
    if !status_line.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let status = status_line.next()?.parse().ok()?;
    let (mut length, mut chunked) = (None, false);
    for (name, value) in lines.filter_map(|l| l.split_once(':')) {
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<usize>().ok()?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.eq_ignore_ascii_case("chunked");
        }
    }
    let body = if chunked {
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

/// Decodes a chunked body; a truncated stream gives `None`.
fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let eol = find(rest, b"\r\n")?;
        let size = std::str::from_utf8(&rest[..eol]).ok()?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
        if body.len() > MAX_BODY {
            return None;
        }
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
