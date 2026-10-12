use super::super::engine::{ChatError, Finish};
use super::*;

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

const MALFORMED: &str = "malformed stream data";
const TOO_LONG: &str = "stream line over the size limit";

/// Feeds `chunks` in order, then ends the stream.
fn run(decoder: &mut impl StreamDecoder, chunks: &[&[u8]]) -> Vec<StreamEvent> {
    let mut events: Vec<StreamEvent> = chunks.iter().flat_map(|c| decoder.feed(c)).collect();
    events.extend(decoder.finish());
    events
}

fn ndjson(chunks: &[&[u8]]) -> Vec<StreamEvent> {
    run(&mut NdjsonSplitter::server(), chunks)
}

fn sse(chunks: &[&[u8]]) -> Vec<StreamEvent> {
    run(&mut SseParser::server(), chunks)
}

const OLLAMA: &str = concat!(
    "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n",
    "{\"message\":{\"content\":\"lo \u{1f600}\"},\"done\":false}\n",
    "{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
);

const OPENAI: &str = concat!(
    ": keep-alive\r\n\r\n",
    "event: message\r\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}\r\n\r\n",
    "data:{\"choices\":[{\"delta\":{\"content\":\"lo \u{1f600}\"},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
    "data: [DONE]\r\n\r\n",
);

fn hello() -> Vec<StreamEvent> {
    vec![
        delta("Hel"),
        delta("lo \u{1f600}"),
        StreamEvent::Done(Finish::Stop),
    ]
}

/// Whole, every two-chunk cut and one byte at a time all agree.
fn same_at_every_split(text: &str, want: &[StreamEvent], decode: fn(&[&[u8]]) -> Vec<StreamEvent>) {
    let bytes = text.as_bytes();
    assert_eq!(decode(&[bytes]), want, "whole");
    for at in 0..=bytes.len() {
        let (a, b) = bytes.split_at(at);
        assert_eq!(decode(&[a, b]), want, "cut at {at}");
    }
    let singles: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(decode(&singles), want, "per byte");
}

#[test]
fn ollama_lines_give_deltas_then_one_final() {
    assert_eq!(ndjson(&[OLLAMA.as_bytes()]), hello());
    let length = "{\"message\":{\"content\":\"a\"}}\n{\"done\":true,\"done_reason\":\"length\"}\n";
    assert_eq!(
        ndjson(&[length.as_bytes()]),
        [delta("a"), StreamEvent::Done(Finish::Length)]
    );
}

#[test]
fn openai_events_give_deltas_then_exactly_one_final() {
    assert_eq!(sse(&[OPENAI.as_bytes()]), hello());
    let length = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    assert_eq!(
        sse(&[length.as_bytes()]),
        [delta("a"), StreamEvent::Done(Finish::Length)]
    );
}

#[test]
fn both_formats_survive_every_split_point() {
    same_at_every_split(OLLAMA, &hello(), ndjson);
    same_at_every_split(OPENAI, &hello(), sse);
    let lf = OPENAI.replace("\r\n", "\n");
    same_at_every_split(&lf, &hello(), sse);
}

#[test]
fn a_split_character_arrives_intact() {
    let bytes = OLLAMA.as_bytes();
    let emoji = bytes
        .windows(4)
        .position(|w| w == "\u{1f600}".as_bytes())
        .unwrap();
    for inside in 1..4 {
        let (a, b) = bytes.split_at(emoji + inside);
        let got = ndjson(&[a, b]);
        assert_eq!(got[1], delta("lo \u{1f600}"), "cut {inside} bytes in");
    }
}

#[test]
fn empty_lines_comments_and_empty_fragments_are_ignored() {
    assert_eq!(
        ndjson(&[b"\n\r\n{\"message\":{\"content\":\"\"}}\n"]),
        [StreamEvent::Error(ChatError::Interrupted)]
    );
    assert_eq!(
        sse(&[b": ping\n\n\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n"]),
        [StreamEvent::Error(ChatError::Interrupted)]
    );
}

#[test]
fn the_last_line_without_a_newline_still_counts() {
    assert_eq!(
        ndjson(&[b"{\"done\":true}"]),
        [StreamEvent::Done(Finish::Stop)]
    );
    assert_eq!(sse(&[b"data: [DONE]"]), [StreamEvent::Done(Finish::Stop)]);
}

#[test]
fn a_stream_that_ends_without_a_final_event_is_interrupted() {
    let cut = "{\"message\":{\"content\":\"Hel\"}}\n";
    assert_eq!(
        ndjson(&[cut.as_bytes()]),
        [delta("Hel"), StreamEvent::Error(ChatError::Interrupted)]
    );
    assert_eq!(ndjson(&[]), [StreamEvent::Error(ChatError::Interrupted)]);
    assert_eq!(sse(&[]), [StreamEvent::Error(ChatError::Interrupted)]);
}

#[test]
fn nothing_follows_the_final_event_or_an_error() {
    let after = format!("{OLLAMA}{OLLAMA}");
    assert_eq!(ndjson(&[after.as_bytes()]), hello());
    let after = format!("{OPENAI}{OPENAI}");
    assert_eq!(sse(&[after.as_bytes()]), hello());
    assert_eq!(
        ndjson(&[b"{\"error\":\"boom\"}\n{\"message\":{\"content\":\"x\"}}\n"]),
        [failed("boom")]
    );
}

#[test]
fn malformed_data_is_an_error_event_not_a_panic() {
    assert_eq!(ndjson(&[b"{\"message\":\n"]), [failed(MALFORMED)]);
    assert_eq!(ndjson(&[b"not json\n"]), [failed(MALFORMED)]);
    assert_eq!(ndjson(&[b"{\"message\":\xff}\n"]), [failed(MALFORMED)]);
    assert_eq!(sse(&[b"data: {\"choices\":\n\n"]), [failed(MALFORMED)]);
    assert_eq!(sse(&[b"data: nope\n\n"]), [failed(MALFORMED)]);
}

#[test]
fn a_server_error_object_is_sanitized_and_capped() {
    assert_eq!(
        ndjson(&[b"{\"error\":\"model not found\"}\n"]),
        [failed("model not found")]
    );
    assert_eq!(
        ndjson(&[b"{\"error\":\"\\u001b[31mbad\\u001b[0m\\u202e model\\n\"}\n"]),
        [failed("bad model")]
    );
    let openai = b"data: {\"error\":{\"message\":\"no such model\",\"type\":\"x\"}}\n\n";
    assert_eq!(sse(&[openai]), [failed("no such model")]);
    assert_eq!(
        sse(&[b"data: {\"error\":\"plain\"}\n\n"]),
        [failed("plain")]
    );
    let long = format!("{{\"error\":\"{}\"}}\n", "e".repeat(500));
    assert_eq!(ndjson(&[long.as_bytes()]), [failed(&"e".repeat(ERROR_CAP))]);
    assert_eq!(
        ndjson(&[b"{\"error\":\"\\u001b[0m\"}\n"]),
        [failed("engine error")]
    );
    assert_eq!(ndjson(&[b"{\"error\":{}}\n"]), [failed("engine error")]);
}

/// A valid NDJSON line of exactly `len` bytes, without its newline.
fn line_of(len: usize) -> Vec<u8> {
    let head = b"{\"message\":{\"content\":\"";
    let tail = b"\"}}";
    let mut line = head.to_vec();
    line.resize(len - tail.len(), b'a');
    line.extend_from_slice(tail);
    line
}

#[test]
fn a_line_over_the_cap_is_an_error_and_one_at_the_cap_is_not() {
    for (cap, new) in [
        (
            SERVER_LINE_CAP,
            NdjsonSplitter::server as fn() -> NdjsonSplitter,
        ),
        (
            CLI_LINE_CAP,
            (|| NdjsonSplitter::new(CLI_LINE_CAP)) as fn() -> NdjsonSplitter,
        ),
    ] {
        let mut ok = line_of(cap);
        ok.extend_from_slice(b"\r\n{\"done\":true}\n");
        let got = run(&mut new(), &[&ok]);
        assert_eq!(got.len(), 2, "cap {cap}");
        assert_eq!(got[1], StreamEvent::Done(Finish::Stop));
        let mut big = line_of(cap + 1);
        big.push(b'\n');
        assert_eq!(run(&mut new(), &[&big]), [failed(TOO_LONG)], "cap {cap}");
        let mut piecewise = new();
        let events: Vec<_> = line_of(cap + 2)
            .chunks(4096)
            .flat_map(|c| piecewise.feed(c))
            .collect();
        assert_eq!(events, [failed(TOO_LONG)], "no newline, cap {cap}");
    }
}

#[test]
fn an_sse_event_over_the_cap_is_an_error() {
    let mut big = b"data: ".to_vec();
    big.extend(std::iter::repeat_n(b'a', SERVER_LINE_CAP + 1));
    big.extend_from_slice(b"\n\n");
    assert_eq!(sse(&[&big]), [failed(TOO_LONG)]);
    let mut cli = SseParser::new(CLI_LINE_CAP);
    let mut big = b"data: ".to_vec();
    big.extend(std::iter::repeat_n(b'a', CLI_LINE_CAP + 1));
    big.extend_from_slice(b"\n\n");
    assert_eq!(run(&mut cli, &[&big]), [failed(TOO_LONG)]);
    let many =
        "data: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".repeat(20);
    let mut small = SseParser::new(1000);
    assert_eq!(run(&mut small, &[many.as_bytes()]), [failed(TOO_LONG)]);
}
