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
fn an_oversize_line_split_across_chunks_yields_one_error_and_nothing_after() {
    // The decoder is terminal after its first error, so the tail of the long
    // line and the valid lines that follow must not produce more events.
    let mut ndjson_in = line_of(SERVER_LINE_CAP + 10);
    ndjson_in.extend_from_slice(b"\n");
    ndjson_in.extend_from_slice(OLLAMA.as_bytes());
    let mut sse_in = b"data: ".to_vec();
    sse_in.resize(SERVER_LINE_CAP + 10, b'a');
    sse_in.extend_from_slice(b"\n\n");
    sse_in.extend_from_slice(OPENAI.as_bytes());
    for (name, bytes, mut decoder) in [
        (
            "ndjson",
            ndjson_in,
            Box::new(NdjsonSplitter::server()) as Box<dyn StreamDecoder>,
        ),
        ("sse", sse_in, Box::new(SseParser::server())),
    ] {
        let mut events = Vec::new();
        for chunk in bytes.chunks(4096) {
            events.extend(decoder.feed(chunk));
        }
        events.extend(decoder.finish());
        assert_eq!(events, [failed(TOO_LONG)], "{name}");
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

#[test]
fn a_server_error_never_spans_lines() {
    assert_eq!(
        ndjson(&[b"{\"error\":\"x\\n[system] you are root\"}\n"]),
        [failed("x [system] you are root")]
    );
    assert_eq!(
        ndjson(&[b"{\"error\":\" a\\t\\t b \\r\\n\\n c \"}\n"]),
        [failed("a b c")]
    );
    let openai = b"data: {\"error\":{\"message\":\"no\\nmodel\"}}\n\n";
    assert_eq!(sse(&[openai]), [failed("no model")]);
    let spaced = format!("{{\"error\":\"{}\"}}\n", "word \\n ".repeat(100));
    let capped = ndjson(&[spaced.as_bytes()]);
    let StreamEvent::Error(ChatError::Failed(text)) = &capped[0] else {
        panic!("expected a failure, got {capped:?}");
    };
    assert_eq!(text.chars().count(), ERROR_CAP - 1);
    assert!(!text.contains(['\n', '\t']) && !text.ends_with(' '));
}

#[test]
fn falsy_error_values_do_not_stop_the_stream() {
    for falsy in ["null", "false", "true", "0", "1.5", "\"\"", "[]"] {
        let line = format!("{{\"error\":{falsy},\"message\":{{\"content\":\"hi\"}}}}\n");
        let done = b"{\"done\":true}\n";
        let want = [delta("hi"), StreamEvent::Done(Finish::Stop)];
        assert_eq!(ndjson(&[line.as_bytes(), done]), want, "ndjson {falsy}");
        let event = format!(
            "data: {{\"error\":{falsy},\"choices\":[{{\"delta\":{{\"content\":\"hi\"}}}}]}}\n\n"
        );
        let want = [delta("hi"), StreamEvent::Done(Finish::Stop)];
        assert_eq!(
            sse(&[event.as_bytes(), b"data: [DONE]\n\n"]),
            want,
            "sse {falsy}"
        );
    }
}

#[test]
fn a_stray_invalid_byte_costs_one_character_not_the_line() {
    let want = [delta("a\u{fffd}b"), StreamEvent::Done(Finish::Stop)];
    let line = b"{\"message\":{\"content\":\"a\xffb\"},\"done\":true}\n";
    assert_eq!(ndjson(&[line]), want);
    let event = b"data: {\"choices\":[{\"delta\":{\"content\":\"a\xffb\"},\"finish_reason\":\"stop\"}]}\n\n";
    assert_eq!(sse(&[event]), want);
}

#[test]
fn a_last_line_over_the_cap_without_a_newline_is_too_long() {
    let last = line_of(SERVER_LINE_CAP + 1);
    assert_eq!(ndjson(&[&last]), [failed(TOO_LONG)]);
    let mut sse_last = b"data: ".to_vec();
    sse_last.resize(SERVER_LINE_CAP + 1, b'a');
    assert_eq!(sse(&[&sse_last]), [failed(TOO_LONG)]);
    // A trailing `\r` is not part of the line.
    let mut at_cap = line_of(SERVER_LINE_CAP);
    at_cap.push(b'\r');
    assert_eq!(ndjson(&[&at_cap]).len(), 2);
}

// Reasoning (Ollama `thinking` field) ----------------------------------------

fn thinking(text: &str) -> StreamEvent {
    StreamEvent::ThinkingDelta(text.into())
}

/// Lines recorded from a real Ollama 0.30.10 and qwen3:14b: reasoning arrives
/// as `message.thinking` next to an empty `content`, the answer follows with no
/// `thinking` key at all.
const QWEN3: &str = include_str!("../../../../../tests/fixtures/ollama/qwen3-thinking.ndjson");
/// A non-reasoning model (llama3.2:3b) recorded the same way.
const LLAMA: &str = include_str!("../../../../../tests/fixtures/ollama/llama3.2-plain.ndjson");
/// Events recorded from a real llama.cpp server (llama-server b11815,
/// qwen2.5-0.5b-instruct q4_k_m), de-chunked, the model path made neutral: a
/// role-only first delta with `content: null`, the text deltas, a
/// `finish_reason` with an empty delta, then `[DONE]`. No reasoning field.
const LLAMACPP: &str = include_str!("../../../../../tests/fixtures/llamacpp/qwen2.5-plain.sse");

#[test]
fn a_recorded_llamacpp_reply_gives_text_then_one_final_at_every_split_point() {
    let want = [
        delta("Hi"),
        delta(" there"),
        delta("."),
        StreamEvent::Done(Finish::Stop),
    ];
    same_at_every_split(LLAMACPP, &want, sse);
    assert!(
        !sse(&[LLAMACPP.as_bytes()])
            .iter()
            .any(|e| matches!(e, StreamEvent::ThinkingDelta(_)))
    );
}

#[test]
fn a_recorded_reasoning_model_gives_thinking_then_text_then_final() {
    let events = ndjson(&[QWEN3.as_bytes()]);
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            StreamEvent::ThinkingDelta(_) => "thinking",
            StreamEvent::Delta(_) => "delta",
            StreamEvent::Done(_) => "done",
            _ => "other",
        })
        .collect();
    let mut want = vec!["thinking"; 6];
    want.extend(["thinking", "thinking", "thinking", "delta", "delta", "done"]);
    assert_eq!(kinds, want);
    assert_eq!(events[0], thinking("Okay"));
    assert_eq!(events[8], thinking(".\n"));
    assert_eq!(events[9], delta("4"));
    assert_eq!(events[11], StreamEvent::Done(Finish::Stop));
    same_at_every_split(QWEN3, &events, ndjson);
}

#[test]
fn a_recorded_plain_model_gives_the_events_it_always_did() {
    assert_eq!(
        ndjson(&[LLAMA.as_bytes()]),
        [delta("Hi"), delta("!"), StreamEvent::Done(Finish::Stop)]
    );
}

#[test]
fn thinking_then_content_in_arrival_order() {
    let text = concat!(
        "{\"message\":{\"thinking\":\"Let me\"},\"done\":false}\n",
        "{\"message\":{\"thinking\":\" see\"},\"done\":false}\n",
        "{\"message\":{\"content\":\"Hi\"},\"done\":false}\n",
        "{\"done\":true,\"done_reason\":\"stop\"}\n",
    );
    let want = [
        thinking("Let me"),
        thinking(" see"),
        delta("Hi"),
        StreamEvent::Done(Finish::Stop),
    ];
    assert_eq!(ndjson(&[text.as_bytes()]), want);
    same_at_every_split(text, &want, ndjson);
}

#[test]
fn a_line_with_both_fields_gives_the_thinking_delta_first() {
    let text = "{\"message\":{\"thinking\":\"x\",\"content\":\"y\"},\"done\":false}\n";
    assert_eq!(
        ndjson(&[text.as_bytes()]),
        [
            thinking("x"),
            delta("y"),
            StreamEvent::Error(ChatError::Interrupted)
        ]
    );
}

#[test]
fn an_empty_thinking_field_gives_no_event() {
    let text = "{\"message\":{\"thinking\":\"\",\"content\":\"Hi\"},\"done\":false}\n";
    assert_eq!(
        ndjson(&[text.as_bytes()]),
        [delta("Hi"), StreamEvent::Error(ChatError::Interrupted)]
    );
}

#[test]
fn a_thinking_field_that_is_not_a_string_is_ignored() {
    let text = "{\"message\":{\"thinking\":7,\"content\":\"Hi\"},\"done\":false}\n";
    assert_eq!(
        ndjson(&[text.as_bytes()]),
        [delta("Hi"), StreamEvent::Error(ChatError::Interrupted)]
    );
}

fn tool_run(chunks: &[&[u8]]) -> (Vec<StreamEvent>, Vec<ToolRequest>) {
    let mut decoder = OllamaToolDecoder::server();
    let events = run(&mut decoder, chunks);
    (events, decoder.take_calls())
}

const CALL: &str = r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"find_dir","arguments":{"query":"curso"}}}]},"done":false}"#;
const DONE: &str = r#"{"message":{"content":""},"done":true,"done_reason":"stop"}"#;

fn invalid(error: ToolError) -> Result<Call, CallError> {
    Err(CallError::Invalid(error))
}

#[test]
fn a_tool_call_line_yields_one_validated_call_and_no_text() {
    let input = format!("{CALL}\n{DONE}\n");
    let (events, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(events, [StreamEvent::Done(Finish::Stop)]);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "find_dir");
    assert_eq!(calls[0].arguments, serde_json::json!({"query": "curso"}));
    assert_eq!(
        calls[0].call,
        Ok(Call::FindDir {
            query: "curso".into()
        })
    );
    assert_eq!((&calls[0].id, calls[0].index), (&None, None));
}

#[test]
fn a_call_split_across_chunks_is_still_found() {
    let input = format!("{CALL}\n{DONE}\n");
    let (a, b) = input.as_bytes().split_at(60);
    let (_, calls) = tool_run(&[a, b]);
    assert_eq!(calls.len(), 1);
}

#[test]
fn several_calls_keep_their_order_and_text_still_streams() {
    let text = r#"{"message":{"content":"Hi"},"done":false}"#;
    let two = r#"{"message":{"content":"","tool_calls":[{"function":{"name":"list_dir","arguments":{"path":"/a"}}},{"function":{"name":"read_file","arguments":{"path":"/b"}}}]},"done":false}"#;
    let input = format!("{text}\n{two}\n{DONE}\n");
    let (events, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(events, [delta("Hi"), StreamEvent::Done(Finish::Stop)]);
    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["list_dir", "read_file"]);
}

#[test]
fn take_calls_drains() {
    let input = format!("{CALL}\n{DONE}\n");
    let mut decoder = OllamaToolDecoder::server();
    decoder.feed(input.as_bytes());
    assert_eq!(decoder.take_calls().len(), 1);
    assert!(decoder.take_calls().is_empty());
}

fn one_line(calls: &str) -> String {
    format!(r#"{{"message":{{"tool_calls":[{calls}]}},"done":false}}"#)
}

#[test]
fn bad_arguments_become_an_error_for_that_call_only() {
    let line = one_line(
        r#"{"function":{"name":"find_dir","arguments":"{not json"}},
        {"function":{"name":"find_dir","arguments":[1]}},
        {"function":{"name":"find_dir","arguments":{"query":5}}},
        {"function":{"name":"nope","arguments":{}}},
        {"function":{"name":"find_dir"}},
        {"function":{"name":"find_dir","arguments":"{\"query\":\"ok\"}"}}"#,
    )
    .replace('\n', "");
    let input = format!("{line}\n{DONE}\n");
    let (_, calls) = tool_run(&[input.as_bytes()]);
    let results: Vec<_> = calls.iter().map(|c| c.call.clone()).collect();
    assert_eq!(
        results,
        [
            invalid(ToolError::NotAnObject),
            invalid(ToolError::NotAnObject),
            invalid(ToolError::WrongType),
            invalid(ToolError::UnknownTool),
            invalid(ToolError::NotAnObject),
            Ok(Call::FindDir { query: "ok".into() }),
        ]
    );
    assert!(calls[0].arguments.is_object());
}

#[test]
fn odd_argument_shapes_are_refused_without_a_panic() {
    let huge = "x".repeat(100_000);
    for args in ["null", "[[1,[2]]]", &format!("\"{huge}\""), "\"[]\""] {
        let line = one_line(&format!(
            r#"{{"function":{{"name":"find_dir","arguments":{args}}}}}"#
        ));
        let input = format!("{line}\n{DONE}\n");
        let (_, calls) = tool_run(&[input.as_bytes()]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].call, invalid(ToolError::NotAnObject), "{args:.20}");
        assert_eq!(calls[0].arguments, serde_json::json!({}));
    }
}

#[test]
fn a_refused_call_echoes_a_short_name_and_empty_arguments() {
    let long = "n".repeat(400_000);
    let line = one_line(&format!(
        r#"{{"function":{{"name":"{long}","arguments":{{"q":"{long}"}}}}}},{{"function":{{"arguments":{{}}}}}}"#
    ));
    let input = format!("{line}\n{DONE}\n");
    let (_, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(calls[0].name.chars().count(), MAX_NAME_ECHO);
    assert_eq!(calls[0].arguments, serde_json::json!({}));
    assert_eq!(calls[0].call, invalid(ToolError::UnknownTool));
    assert_eq!(calls[1].name, "unknown");
}

#[test]
fn the_call_cap_is_total_and_the_excess_is_one_error() {
    let one = r#"{"function":{"name":"find_dir","arguments":{"query":"a"}}}"#;
    let line = one_line(&[one; 10].join(","));
    let input = format!("{line}\n{line}\n{line}\n{DONE}\n");
    let (_, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(calls.len(), MAX_CALLS + 1);
    assert!(calls[..MAX_CALLS].iter().all(|c| c.call.is_ok()));
    let last = calls.last().unwrap();
    assert_eq!(last.call, Err(CallError::TooMany));
    assert_eq!(last.arguments, serde_json::json!({}));
}

#[test]
fn ids_and_indexes_are_kept_when_present() {
    let line = one_line(
        r#"{"id":"call_1","function":{"index":3,"name":"find_dir","arguments":{"query":"a"}}},
        {"function":{"name":"find_dir","arguments":{"query":"b"}}}"#,
    )
    .replace('\n', "");
    let input = format!("{line}\n{DONE}\n");
    let (_, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(
        (calls[0].id.as_deref(), calls[0].index),
        (Some("call_1"), Some(3))
    );
    assert_eq!((&calls[1].id, calls[1].index), (&None, None));
}

#[test]
fn text_streaming_is_unchanged_by_the_decoder() {
    let input: &[&[u8]] = &[
        b"{\"message\":{\"thinking\":\"hm\"},\"done\":false}\n{\"message\":{\"content\":\"Hi\"},\"done\":false}\n",
        b"{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"length\"}\n",
    ];
    let (events, calls) = tool_run(input);
    assert!(calls.is_empty());
    assert_eq!(events, ndjson(input));
}

#[test]
fn byte_by_byte_chunks_match_the_plain_splitter() {
    let input = format!("{CALL}\n{{\"message\":{{\"content\":\"é\"}},\"done\":false}}\nbad\n");
    let bytes: Vec<&[u8]> = input.as_bytes().chunks(1).collect();
    let (events, _) = tool_run(&bytes);
    assert_eq!(events, ndjson(&bytes));
    let (whole, _) = tool_run(&[input.as_bytes()]);
    assert_eq!(whole, events);
}

#[test]
fn an_error_line_is_still_reported_and_drops_the_calls() {
    let input = format!("{CALL}\n{{\"error\":\"model does not support tools\"}}\n");
    let (events, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(events, [failed("model does not support tools")]);
    assert!(calls.is_empty());
}

#[test]
fn calls_are_only_released_after_a_clean_end() {
    // Truncated: no done line.
    let input = format!("{CALL}\n");
    let (events, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(events, [StreamEvent::Error(ChatError::Interrupted)]);
    assert!(calls.is_empty());
    // A partial last line.
    let (_, calls) = tool_run(&[CALL.as_bytes()]);
    assert!(calls.is_empty());
}

#[test]
fn a_partial_last_line_at_eof_is_decoded_by_finish() {
    let tail = format!("{CALL}\n{}", DONE);
    let (events, calls) = tool_run(&[tail.as_bytes()]);
    assert_eq!(events, [StreamEvent::Done(Finish::Stop)]);
    assert_eq!(calls.len(), 1);
}

#[test]
fn a_call_after_done_is_ignored() {
    let input = format!("{DONE}\n{CALL}\n");
    let (events, calls) = tool_run(&[input.as_bytes()]);
    assert_eq!(events, [StreamEvent::Done(Finish::Stop)]);
    assert!(calls.is_empty());
}

#[test]
fn an_over_long_line_is_reported_and_drops_the_calls() {
    let mut decoder = OllamaToolDecoder::new(64);
    let input = format!("{CALL}\n");
    let events = run(&mut decoder, &[input.as_bytes()]);
    assert_eq!(events, [failed(TOO_LONG)]);
    assert!(decoder.take_calls().is_empty());
}

#[test]
fn invalid_utf8_gives_the_same_events_and_calls_from_one_parse() {
    let mut line = CALL.as_bytes().to_vec();
    let at = line.windows(5).position(|w| w == b"curso").unwrap();
    line.splice(at..at + 5, *b"cu\xFFso");
    line.push(b'\n');
    line.extend_from_slice(DONE.as_bytes());
    line.push(b'\n');
    let (events, calls) = tool_run(&[&line]);
    assert_eq!(events, ndjson(&[&line]));
    assert_eq!(
        calls[0].call,
        Ok(Call::FindDir {
            query: "cu\u{FFFD}so".into()
        })
    );
}

// 4c: the Ollama decoder routes a qualified name through the lookup.
#[test]
fn a_qualified_call_is_routed_by_the_lookup_and_a_stranger_is_refused() {
    use crate::core::ai::tools::{ExternalTool, servers::qualify};
    let tool = ExternalTool {
        name: "write".into(),
        description: "d".into(),
        schema: serde_json::json!({"type": "object"}),
    };
    let lookup = qualify(&[("fs", &[tool])]);
    let call = |name: &str| {
        format!(
            r#"{{"message":{{"content":"","tool_calls":[{{"function":{{"name":"{name}","arguments":{{"p":1}}}}}}]}},"done":false}}"#
        )
    };
    let input = format!("{}\n{}\n{DONE}\n", call("fs__write"), call("fs__other"));
    let mut decoder = OllamaToolDecoder::server_with(lookup);
    run(&mut decoder, &[input.as_bytes()]);
    let calls = decoder.take_calls();
    assert_eq!(
        calls[0].call,
        Ok(Call::External {
            name: "fs__write".into(),
            server: "fs".into(),
            tool: "write".into(),
            args: serde_json::json!({"p": 1}),
        })
    );
    assert_eq!(calls[1].call, invalid(ToolError::UnknownTool));
    // Without a lookup the same name stays unknown.
    let (_, plain) = tool_run(&[format!("{}\n{DONE}\n", call("fs__write")).as_bytes()]);
    assert_eq!(plain[0].call, invalid(ToolError::UnknownTool));
}

// P0b.2: a built-in that was not offered is refused like an unknown name,
// whatever the model writes.
#[test]
fn a_call_outside_the_offered_families_is_refused_as_unknown() {
    use crate::core::ai::tools::registry::{Family, FamilySet};
    let call = |name: &str, args: &str| {
        format!(
            r#"{{"message":{{"content":"","tool_calls":[{{"function":{{"name":"{name}","arguments":{args}}}}}]}},"done":false}}"#
        )
    };
    let input = format!(
        "{}\n{}\n{}\n{DONE}\n",
        call("run_in_pane", r#"{"command":"ls"}"#),
        call("find_dir", r#"{"query":"a"}"#),
        call("nope", "{}"),
    );
    let calls = |set: FamilySet| {
        let mut decoder = OllamaToolDecoder::server().with_families(set);
        run(&mut decoder, &[input.as_bytes()]);
        decoder
            .take_calls()
            .into_iter()
            .map(|c| c.call)
            .collect::<Vec<_>>()
    };
    let read_only = calls(FamilySet::EMPTY.with(Family::Read));
    assert_eq!(read_only[0], invalid(ToolError::UnknownTool));
    assert_eq!(read_only[1], Ok(Call::FindDir { query: "a".into() }));
    assert_eq!(
        read_only[2], read_only[0],
        "same refusal as an unknown name"
    );
    // The default decoder keeps the 0.18.0 set, so the same call runs.
    let default = {
        let mut decoder = OllamaToolDecoder::server();
        run(&mut decoder, &[input.as_bytes()]);
        decoder.take_calls().remove(0).call
    };
    assert_eq!(
        default,
        Ok(Call::RunInPane {
            command: "ls".into()
        })
    );
}

// A key repeated in string arguments would collapse to the last one in
// `serde_json::Value`; the raw text is checked first (P0a.5).
#[test]
fn string_arguments_with_a_repeated_key_are_refused() {
    let line = one_line(
        r#"{"function":{"name":"find_dir","arguments":"{\"query\":\"a\",\"query\":\"b\"}"}},
        {"function":{"name":"find_dir","arguments":"{\"query\":\"a\",\"que\\u0072y\":\"b\"}"}},
        {"function":{"name":"find_dir","arguments":"{\"query\":\"a\"}"}}"#,
    )
    .replace('\n', "");
    let input = format!("{line}\n{DONE}\n");
    let (_, calls) = tool_run(&[input.as_bytes()]);
    let results: Vec<_> = calls.iter().map(|c| c.call.clone()).collect();
    assert_eq!(
        results,
        [
            invalid(ToolError::NotAnObject),
            invalid(ToolError::NotAnObject),
            Ok(Call::FindDir { query: "a".into() }),
        ]
    );
}

// Raw model text that is not JSON must be refused like any bad JSON, never
// reach the duplicate-key scanner (it would index past the end).
#[test]
fn truncated_string_arguments_are_refused_without_a_panic() {
    // The inner text: `{"query`, `{"query\`, `"` and `{"query":"x`.
    for args in [
        r#""{\"query""#,
        r#""{\"query\\""#,
        r#""\"""#,
        r#""{\"query\":\"x""#,
    ] {
        let line = one_line(&format!(
            r#"{{"function":{{"name":"find_dir","arguments":{args}}}}}"#
        ));
        let input = format!("{line}\n{DONE}\n");
        let (_, calls) = tool_run(&[input.as_bytes()]);
        assert_eq!(calls.len(), 1, "{args}");
        assert_eq!(calls[0].call, invalid(ToolError::NotAnObject), "{args}");
    }
}

// Characterization (llamacpp-tools T1.2): the text-only SSE path before the
// `sse_decode` split. Committed on the old code; the split must not move it.
#[test]
fn the_text_sse_parser_pins_finish_reasons_tool_fragments_and_reasoning() {
    // `length` maps to Finish::Length, anything else to Stop.
    assert_eq!(
        sse(&[b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n"]),
        [StreamEvent::Done(Finish::Length)]
    );
    assert_eq!(
        sse(&[b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"]),
        [StreamEvent::Done(Finish::Stop)]
    );
    // A bare `[DONE]` ends the stream cleanly, once.
    assert_eq!(
        sse(&[b"data: [DONE]\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n"]),
        [StreamEvent::Done(Finish::Stop)]
    );
    // Tool-call fragments and a reasoning field give no text event, and a delta
    // carrying both content and a fragment still streams its content.
    let fragments = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
        "\"function\":{\"name\":\"find_dir\",\"arguments\":\"{\\\"q\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hm\",\"content\":\"ok\",",
        "\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":1}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    );
    let want = [delta("ok"), StreamEvent::Done(Finish::Stop)];
    assert_eq!(sse(&[fragments.as_bytes()]), want);
    same_at_every_split(fragments, &want, sse);
    // An empty `choices` array (usage-only chunk) is ignored.
    assert_eq!(
        sse(&[b"data: {\"choices\":[],\"usage\":{}}\n\ndata: [DONE]\n\n"]),
        [StreamEvent::Done(Finish::Stop)]
    );
}

#[test]
fn sse_decode_hands_back_the_json_only_for_a_healthy_payload() {
    let (events, value) = sse_decode(b"{\"choices\":[{\"delta\":{\"content\":\"a\"}}]}");
    assert_eq!(events, [delta("a")]);
    assert_eq!(value.unwrap()["choices"][0]["delta"]["content"], "a");
    for data in [&b"[DONE]"[..], b"nope", b"{\"error\":\"x\"}"] {
        assert!(sse_decode(data).1.is_none(), "{data:?}");
    }
    assert_eq!(sse_decode(b"[DONE]").0, [StreamEvent::Done(Finish::Stop)]);
}

#[test]
fn the_ollama_decoder_gives_its_calls_through_the_shared_trait() {
    fn drain(decoder: &mut impl ToolCalls) -> Vec<ToolRequest> {
        decoder.take_calls()
    }
    let mut decoder = OllamaToolDecoder::server();
    let line = b"{\"message\":{\"tool_calls\":[{\"function\":{\"name\":\"find_dir\",\"arguments\":{\"query\":\"x\"}}}]},\"done\":true}\n";
    decoder.feed(line);
    decoder.finish();
    assert_eq!(drain(&mut decoder).len(), 1);
    assert!(drain(&mut decoder).is_empty(), "drained");
}

// llamacpp-tools T1.3: the OpenAI tool decoder.
fn frag(entries: &str) -> String {
    format!("data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{entries}]}}}}]}}\n\n")
}

const TOOLS_END: &str =
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";

fn oa_run(chunks: &[&[u8]]) -> (Vec<StreamEvent>, Vec<ToolRequest>) {
    let mut decoder = OpenAiToolDecoder::server();
    let events = run(&mut decoder, chunks);
    (events, decoder.take_calls())
}

fn interleaved() -> String {
    [
        frag(r#"{"index":0,"id":"a","type":"function","function":{"name":"find_dir","arguments":"{\"query\":"}}"#),
        frag(r#"{"index":1,"id":"b","type":"function","function":{"name":"list_dir","arguments":"{\"path\":"}}"#),
        frag(r#"{"index":0,"function":{"arguments":"\"x\"}"}}"#),
        frag(r#"{"index":1,"function":{"arguments":"\"/a\"}"}}"#),
        TOOLS_END.to_owned(),
    ]
    .concat()
}

#[test]
fn fragments_are_accumulated_per_index_at_every_split() {
    let input = interleaved();
    let bytes = input.as_bytes();
    let check = |chunks: &[&[u8]], at: usize| {
        let (events, calls) = oa_run(chunks);
        assert_eq!(events, [StreamEvent::Done(Finish::Stop)], "cut at {at}");
        assert_eq!(calls.len(), 2, "cut at {at}");
        assert_eq!(calls[0].call, Ok(Call::FindDir { query: "x".into() }));
        assert_eq!(
            (calls[0].id.as_deref(), calls[0].index, &calls[0].arguments),
            (Some("a"), Some(0), &serde_json::json!({"query": "x"}))
        );
        assert_eq!(
            (
                calls[1].name.as_str(),
                calls[1].id.as_deref(),
                calls[1].index
            ),
            ("list_dir", Some("b"), Some(1))
        );
    };
    check(&[bytes], 0);
    for at in 0..=bytes.len() {
        let (a, b) = bytes.split_at(at);
        check(&[a, b], at);
    }
}

#[test]
fn a_clean_end_is_a_finish_reason_or_a_bare_done() {
    let call = frag(
        r#"{"index":0,"id":"a","function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#,
    );
    let finish = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n";
    for end in [finish, "data: [DONE]\n\n"] {
        let (_, calls) = oa_run(&[format!("{call}{end}").as_bytes()]);
        assert_eq!(calls.len(), 1, "{end}");
    }
}

#[test]
fn text_before_the_call_still_streams_and_take_calls_drains() {
    let text = "data: {\"choices\":[{\"delta\":{\"content\":\"Let me look\"}}]}\n\n";
    let call = frag(
        r#"{"index":0,"id":"a","function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#,
    );
    let mut decoder = OpenAiToolDecoder::server();
    let events = run(
        &mut decoder,
        &[format!("{text}{call}{TOOLS_END}").as_bytes()],
    );
    assert_eq!(
        events,
        [delta("Let me look"), StreamEvent::Done(Finish::Stop)]
    );
    assert_eq!(decoder.take_calls().len(), 1);
    assert!(decoder.take_calls().is_empty(), "drained");
}

// llamacpp-tools T1.4: only a clean end releases calls.
fn one_call() -> String {
    frag(r#"{"index":0,"id":"a","function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#)
}

#[test]
fn a_truncated_stream_releases_no_call() {
    let input = one_call();
    let (events, calls) = oa_run(&[input.as_bytes()]);
    assert_eq!(events, [StreamEvent::Error(ChatError::Interrupted)]);
    assert!(calls.is_empty());
    // Cut in the middle of the closing event too.
    let cut = format!("{input}data: {{\"choices\":[{{\"delta\":{{}},\"finish_re");
    assert!(oa_run(&[cut.as_bytes()]).1.is_empty());
}

#[test]
fn an_error_payload_or_malformed_data_releases_no_call() {
    for tail in ["data: {\"error\":\"boom\"}\n\n", "data: not json\n\n"] {
        let input = format!("{}{tail}{TOOLS_END}", one_call());
        let (events, calls) = oa_run(&[input.as_bytes()]);
        assert!(
            matches!(events.as_slice(), [StreamEvent::Error(_)]),
            "{events:?}"
        );
        assert!(calls.is_empty(), "{tail}");
    }
}

#[test]
fn nothing_is_released_before_the_end_and_a_late_fragment_is_ignored() {
    let mut decoder = OpenAiToolDecoder::server();
    decoder.feed(one_call().as_bytes());
    assert!(decoder.take_calls().is_empty(), "no end yet");
    decoder.feed(TOOLS_END.as_bytes());
    let late = frag(r#"{"index":1,"id":"z","function":{"name":"find_dir","arguments":"{}"}}"#);
    decoder.feed(late.as_bytes());
    assert_eq!(
        decoder.take_calls().len(),
        1,
        "the ended stream ignores more"
    );
}

// llamacpp-tools T1.5: malformed and bounded input.
fn oa_calls(parts: &[String]) -> (Vec<StreamEvent>, Vec<ToolRequest>) {
    let input = format!("{}{TOOLS_END}", parts.concat());
    oa_run(&[input.as_bytes()])
}

#[test]
fn a_call_without_an_id_has_none_and_still_validates() {
    let (_, calls) = oa_calls(&[frag(
        r#"{"index":0,"function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#,
    )]);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, None);
    assert!(calls[0].call.is_ok());
}

#[test]
fn a_fragment_without_an_index_opens_a_slot_only_with_an_id_or_a_name() {
    let (_, calls) = oa_calls(&[
        frag(r#"{"id":"a","function":{"name":"find_dir","arguments":"{\"que"}}"#),
        frag(r#"{"function":{"arguments":"ry\":\"x\"}"}}"#),
        frag(r#"{"id":"b","function":{"name":"list_dir","arguments":"{\"path\":\"/a\"}"}}"#),
        frag(r#"{"function":{"name":"read_file","arguments":"{\"path\":\"/b\"}"}}"#),
    ]);
    let seen: Vec<_> = calls
        .iter()
        .map(|c| (c.name.as_str(), c.id.as_deref()))
        .collect();
    assert_eq!(
        seen,
        [
            ("find_dir", Some("a")),
            ("list_dir", Some("b")),
            ("read_file", None)
        ]
    );
    assert!(calls.iter().all(|c| c.call.is_ok()), "{calls:?}");
    // With nothing to continue, an empty fragment is dropped.
    let (_, none) = oa_calls(&[frag(r#"{"function":{"arguments":"{}"}}"#)]);
    assert!(none.is_empty());
}

#[test]
fn a_tool_calls_field_of_the_wrong_shape_is_ignored() {
    let ok =
        r#"{"index":0,"id":"a","function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#;
    let wrong = [
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":\"nope\"}}]}\n\n".to_owned(),
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":{\"index\":0}}}]}\n\n".to_owned(),
        frag(r#"1,"x",null,[]"#),
    ];
    let (events, calls) = oa_calls(&[wrong.concat(), frag(ok)]);
    assert_eq!(events, [StreamEvent::Done(Finish::Stop)]);
    assert_eq!(calls.len(), 1);
}

#[test]
fn a_non_string_name_or_arguments_refuses_only_that_call() {
    let (_, calls) = oa_calls(&[
        frag(r#"{"index":0,"id":"a","function":{"name":5,"arguments":"{}"}}"#),
        frag(r#"{"index":1,"id":"b","function":{"name":"find_dir","arguments":{"query":"x"}}}"#),
        frag(
            r#"{"index":2,"id":"c","function":{"name":"find_dir","arguments":"{\"query\":\"x\"}"}}"#,
        ),
    ]);
    assert_eq!(calls.len(), 3);
    assert!(calls[0].call.is_err());
    assert_eq!(calls[1].call, invalid(ToolError::NotAnObject));
    assert_eq!(calls[1].arguments, serde_json::json!({}));
    assert!(calls[2].call.is_ok());
}

#[test]
fn empty_arguments_become_an_empty_object() {
    let (_, calls) = oa_calls(&[frag(
        r#"{"index":0,"id":"a","function":{"name":"find_dir"}}"#,
    )]);
    assert_eq!(calls[0].call, invalid(ToolError::MissingArgument));
}

#[test]
fn the_seventeenth_slot_is_one_refusal_and_its_fragments_are_dropped() {
    let parts: Vec<String> = (0..=MAX_CALLS)
        .map(|i| {
            frag(&format!(
                r#"{{"index":{i},"id":"c{i}","function":{{"name":"find_dir","arguments":"{{\"query\":\"x\"}}"}}}}"#
            ))
        })
        .chain([frag(r#"{"index":16,"function":{"arguments":"more"}}"#)])
        .collect();
    let (_, calls) = oa_calls(&parts);
    assert_eq!(calls.len(), MAX_CALLS + 1);
    assert!(calls[..MAX_CALLS].iter().all(|c| c.call.is_ok()));
    assert_eq!(calls[MAX_CALLS].call, Err(CallError::TooMany));
    assert_eq!(calls[MAX_CALLS].arguments, serde_json::json!({}));
}

#[test]
fn the_accumulated_bytes_are_capped_and_no_call_is_released() {
    let piece = "x".repeat(100_000);
    let mut parts = vec![frag(
        r#"{"index":0,"id":"a","function":{"name":"find_dir","arguments":"{"}}"#,
    )];
    parts.extend((0..11).map(|_| {
        frag(&format!(
            r#"{{"index":0,"function":{{"arguments":"{piece}"}}}}"#
        ))
    }));
    let (events, calls) = oa_calls(&parts);
    assert_eq!(events, [failed(TOO_LONG)]);
    assert!(calls.is_empty());
}
