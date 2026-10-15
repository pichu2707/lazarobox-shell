use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;

// `api-error` and `unknown-error` are real `opencode run --format json`
// output (opencode 1.19.0), scrubbed. The others follow the event schema of
// opencode's `run` command and were not captured from a successful run.
const PLAIN: &str = include_str!("../../../../../tests/fixtures/opencode/plain.jsonl");
const MULTI: &str = include_str!("../../../../../tests/fixtures/opencode/multi-text.jsonl");
const TOOL_USE: &str = include_str!("../../../../../tests/fixtures/opencode/tool-use.jsonl");
const API_ERROR: &str = include_str!("../../../../../tests/fixtures/opencode/api-error.jsonl");
const UNKNOWN_ERROR: &str =
    include_str!("../../../../../tests/fixtures/opencode/unknown-error.jsonl");

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

/// Feeds `chunks` in order, then ends the stream.
fn run(chunks: &[&[u8]]) -> Vec<StreamEvent> {
    let mut decoder = OpencodeDecoder::new();
    let mut events: Vec<StreamEvent> = chunks.iter().flat_map(|c| decoder.feed(c)).collect();
    events.extend(decoder.finish());
    events
}

fn whole(text: &str) -> Vec<StreamEvent> {
    run(&[text.as_bytes()])
}

fn text_line(text: &str) -> String {
    format!("{{\"type\":\"text\",\"part\":{{\"type\":\"text\",\"text\":\"{text}\"}}}}\n")
}

fn finish_line(reason: &str) -> String {
    format!("{{\"type\":\"step_finish\",\"part\":{{\"reason\":\"{reason}\"}}}}\n")
}

#[test]
fn a_plain_answer_is_one_delta_and_a_stop() {
    assert_eq!(whole(PLAIN), [delta("Hi!"), done()]);
}

#[test]
fn several_texts_are_separated_by_a_blank_line() {
    assert_eq!(
        whole(MULTI),
        [delta("First note."), delta("\n\nThe answer."), done()]
    );
}

#[test]
fn the_result_is_the_same_whatever_the_chunking() {
    let expected = whole(MULTI);
    let bytes = MULTI.as_bytes();
    for cut in [1, 2, 7, 50, bytes.len() - 1] {
        let (a, b) = bytes.split_at(cut);
        assert_eq!(run(&[a, b]), expected, "cut at {cut}");
    }
}

#[test]
fn a_length_finish_is_a_length_stop() {
    let input = format!("{}{}", text_line("cut"), finish_line("length"));
    assert_eq!(
        whole(&input),
        [delta("cut"), StreamEvent::Done(Finish::Length)]
    );
}

#[test]
fn any_other_finish_reason_is_a_stop() {
    let input = format!("{}{}", text_line("ok"), finish_line("other"));
    assert_eq!(whole(&input), [delta("ok"), done()]);
}

#[test]
fn a_tool_event_ends_the_turn_with_an_error() {
    assert_eq!(whole(TOOL_USE), [failed(TOOL_REFUSED)]);
}

#[test]
fn a_tool_calls_finish_without_a_tool_event_is_refused() {
    let input = finish_line("tool-calls");
    assert_eq!(whole(&input), [failed(TOOL_REFUSED)]);
}

#[test]
fn an_api_error_shows_its_message() {
    assert_eq!(
        whole(API_ERROR),
        [failed(
            "OpenCode's free tier can only be used from within OpenCode. Download it at https://opencode.ai and keep building."
        )]
    );
}

#[test]
fn an_unknown_error_shows_its_message() {
    assert_eq!(
        whole(UNKNOWN_ERROR),
        [failed(
            "Unexpected server error. Check server logs for details."
        )]
    );
}

#[test]
fn an_error_without_a_message_shows_its_name() {
    let line = "{\"type\":\"error\",\"error\":{\"name\":\"ProviderAuthError\"}}\n";
    assert_eq!(whole(line), [failed("ProviderAuthError")]);
}

#[test]
fn an_error_with_nothing_to_show_is_generic() {
    assert_eq!(whole("{\"type\":\"error\"}\n"), [failed("engine error")]);
}

#[test]
fn an_error_message_is_capped_and_flattened() {
    let long = "x ".repeat(ERROR_CAP);
    let line = format!(
        "{{\"type\":\"error\",\"error\":{{\"name\":\"E\",\"data\":{{\"message\":\"a\\n\\nb {long}\"}}}}}}\n"
    );
    let events = whole(&line);
    let [StreamEvent::Error(ChatError::Failed(text))] = events.as_slice() else {
        panic!("expected one failure, got {events:?}");
    };
    assert!(text.starts_with("a b x"));
    assert!(text.chars().count() <= ERROR_CAP);
    assert!(!text.contains('\n'));
}

#[test]
fn an_unknown_event_kind_fails_closed() {
    assert_eq!(
        whole("{\"type\":\"permission_request\"}\n"),
        [failed(UNKNOWN_EVENT)]
    );
}

#[test]
fn reasoning_is_not_shown() {
    let input = format!(
        "{{\"type\":\"reasoning\",\"part\":{{\"text\":\"hmm\"}}}}\n{}{}",
        text_line("ok"),
        finish_line("stop")
    );
    assert_eq!(whole(&input), [delta("ok"), done()]);
}

#[test]
fn empty_text_and_noise_lines_are_skipped() {
    let input = format!(
        "warning on stdout\n\n{}{}{}",
        text_line(""),
        text_line("ok"),
        finish_line("stop")
    );
    assert_eq!(whole(&input), [delta("ok"), done()]);
}

#[test]
fn malformed_json_ends_the_turn() {
    assert_eq!(whole("{not json\n"), [failed("malformed stream data")]);
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    let input = format!("{}{}", text_line("ok"), finish_line("stop"));
    let trimmed = input.trim_end();
    assert_eq!(whole(trimmed), [delta("ok"), done()]);
}

#[test]
fn nothing_after_the_final_event_is_decoded() {
    let input = format!(
        "{}{}{}",
        text_line("a"),
        finish_line("stop"),
        text_line("b")
    );
    assert_eq!(whole(&input), [delta("a"), done()]);
}

#[test]
fn a_stream_that_never_finishes_is_interrupted() {
    assert_eq!(
        whole(&text_line("a")),
        [delta("a"), StreamEvent::Error(ChatError::Interrupted)]
    );
}

#[test]
fn a_line_over_the_cap_ends_the_turn() {
    let huge = vec![b'x'; CLI_LINE_CAP + 10];
    let mut input = b"{\"type\":\"text\",\"part\":{\"text\":\"".to_vec();
    input.extend(huge);
    input.push(b'\n');
    assert_eq!(run(&[&input]), [failed("stream line over the size limit")]);
}
