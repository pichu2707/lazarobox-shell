use super::*;

const A: &str = "sha256:aaaa";
const B: &str = "sha256:bbbb";

fn line(digest: &str, total: u64, completed: u64) -> String {
    format!(
        "{{\"status\":\"pulling {}\",\"digest\":\"{digest}\",\"total\":{total},\"completed\":{completed}}}\n",
        &digest[7..]
    )
}

fn progress(done: u64, total: u64) -> Pulled {
    Pulled::Progress { done, total }
}

/// Every event of `reply` fed in pieces of `size` bytes, then the end.
fn decode(reply: &str, size: usize) -> Vec<Pulled> {
    let mut decoder = PullDecoder::new();
    let mut events = Vec::new();
    for piece in reply.as_bytes().chunks(size) {
        events.extend(decoder.feed(piece));
    }
    events.extend(decoder.finish());
    events
}

// Spec (ollama-download, Pull through the local server: Progress, Success).
#[test]
fn progress_sums_the_digests_seen_then_success_ends_it() {
    let reply = [
        "{\"status\":\"pulling manifest\"}\n".to_owned(),
        line(A, 100, 0),
        line(A, 100, 60),
        line(B, 300, 0),
        line(A, 100, 100),
        line(B, 300, 300),
        "{\"status\":\"verifying sha256 digest\"}\n".to_owned(),
        "{\"status\":\"writing manifest\"}\n".to_owned(),
        "{\"status\":\"success\"}\n".to_owned(),
    ]
    .concat();
    let want = vec![
        progress(0, 100),
        progress(60, 100),
        progress(60, 400),
        progress(100, 400),
        progress(400, 400),
        Pulled::Done,
    ];
    // Whatever the chunking, the same events.
    for size in [1, 7, reply.len()] {
        assert_eq!(decode(&reply, size), want, "pieces of {size}");
    }
}

#[test]
fn nothing_is_read_after_success() {
    let reply = "{\"status\":\"success\"}\n{\"error\":\"late\"}\n";
    assert_eq!(decode(reply, reply.len()), vec![Pulled::Done]);
}

// Spec (ollama-download, Error line fails the pull).
#[test]
fn an_error_line_fails_the_pull() {
    let reply = format!(
        "{}{{\"error\":\"pull model manifest: file does not exist\"}}\n{}",
        line(A, 10, 5),
        line(A, 10, 10)
    );
    assert_eq!(
        decode(&reply, 3),
        vec![
            progress(5, 10),
            Pulled::Failed("pull model manifest: file does not exist".to_owned())
        ]
    );
}

#[test]
fn an_error_reason_is_one_clean_line_of_at_most_200_chars() {
    let long = "x".repeat(500);
    let reply = format!("{{\"error\":\"bad\\u001b[31m\\nnews\\t{long}\"}}\n");
    let events = decode(&reply, reply.len());
    let [Pulled::Failed(reason)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert!(reason.starts_with("bad"), "{reason}");
    assert!(!reason.contains(['\u{1b}', '\n', '\t']), "{reason}");
    assert_eq!(reason.chars().count(), REASON_CAP);
}

// Spec (ollama-download, Pull through the local server: a closed stream).
#[test]
fn a_stream_that_ends_without_success_fails() {
    let reply = line(A, 10, 5);
    assert_eq!(
        decode(&reply, 4),
        vec![progress(5, 10), Pulled::Failed(CLOSED.to_owned())]
    );
    // The last line counts even with no newline after it.
    assert_eq!(decode("{\"status\":\"success\"}", 5), vec![Pulled::Done]);
}

#[test]
fn a_malformed_or_oversized_line_fails() {
    assert_eq!(
        decode("not json\n", 9),
        vec![Pulled::Failed(MALFORMED.to_owned())]
    );
    let huge = format!("{{\"status\":\"{}\"}}\n", "y".repeat(SERVER_LINE_CAP));
    assert_eq!(
        decode(&huge, 64 * 1024),
        vec![Pulled::Failed(TOO_LONG.to_owned())]
    );
}

#[test]
fn a_non_200_reason_names_the_status_and_the_error() {
    assert_eq!(
        status_reason(404, b"{\"error\":\"model not found\"}"),
        "HTTP 404: model not found"
    );
    assert_eq!(status_reason(500, b"oops\nhere"), "HTTP 500: oops here");
    assert_eq!(status_reason(502, b""), "HTTP 502");
}
