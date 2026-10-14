use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;

const PLAIN: &str = include_str!("../../../../../tests/fixtures/claude/plain.jsonl");
const REFUSAL: &str =
    include_str!("../../../../../tests/fixtures/claude/write-refusal-thinking.jsonl");
const SONNET: &str = include_str!("../../../../../tests/fixtures/claude/model-sonnet.jsonl");
const MODEL_ERROR: &str = include_str!("../../../../../tests/fixtures/claude/model-error.jsonl");

fn delta(text: &str) -> StreamEvent {
    StreamEvent::Delta(text.into())
}

fn done() -> StreamEvent {
    StreamEvent::Done(Finish::Stop)
}

/// Feeds `chunks` in order, then ends the stream.
fn run(chunks: &[&[u8]]) -> Vec<StreamEvent> {
    let mut decoder = ClaudeDecoder::new();
    let mut events: Vec<StreamEvent> = chunks.iter().flat_map(|c| decoder.feed(c)).collect();
    events.extend(decoder.finish());
    events
}

fn whole(text: &str) -> Vec<StreamEvent> {
    run(&[text.as_bytes()])
}

/// The text of every `Delta`, joined.
fn answer(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// A stream-json line with one text fragment.
fn text_line(text: &str) -> String {
    format!(
        "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}},\"parent_tool_use_id\":null}}\n"
    )
}

fn thinking_line(text: &str) -> String {
    format!(
        "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"thinking_delta\",\"thinking\":\"{text}\"}}}},\"parent_tool_use_id\":null}}\n"
    )
}

fn result_line(result: &str, extra: &str) -> String {
    format!(
        "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{result}\"{extra}}}\n"
    )
}

#[test]
fn a_plain_answer_is_one_delta_and_a_stop() {
    assert_eq!(whole(PLAIN), [delta("ok"), done()]);
}

#[test]
fn the_model_flag_fixture_streams_its_fragments_in_order() {
    let events = whole(SONNET);
    assert_eq!(events.last(), Some(&done()));
    assert!(events.len() > 3, "several fragments: {events:?}");
    let text = answer(&events);
    assert!(text.starts_with("\"All but 9 run away\""), "{text}");
    assert!(text.ends_with("**25 sheep**."), "{text}");
}

#[test]
fn the_pinned_flags_emit_thinking_blocks_with_no_text_so_no_reasoning_is_produced() {
    // M2 on claude 2.1.299: `thinking_delta` and `signature_delta` events
    // arrive, but the thinking text is empty and the signature is opaque.
    assert!(REFUSAL.contains("\"thinking_delta\""));
    assert!(REFUSAL.contains("\"signature_delta\""));
    let events = whole(REFUSAL);
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, StreamEvent::ThinkingDelta(_))),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&done()));
    let text = answer(&events);
    assert!(text.starts_with("I couldn't create the file"), "{text}");
    assert!(!text.contains("FAKE-SIGNATURE"));
}

#[test]
fn the_answer_comes_from_the_deltas_not_from_the_repeated_assistant_event() {
    let events = whole(REFUSAL);
    let streamed = answer(&events);
    let count = streamed.matches("I couldn't create the file").count();
    assert_eq!(count, 1, "the assistant and result copies are not added");
}

#[test]
fn non_empty_thinking_text_becomes_reasoning_in_arrival_order() {
    let mut text = String::new();
    text.push_str(&thinking_line("hmm "));
    text.push_str(&thinking_line("so"));
    text.push_str(&text_line("Hi"));
    text.push_str(&result_line("Hi", ""));
    assert_eq!(
        whole(&text),
        [
            StreamEvent::ThinkingDelta("hmm ".into()),
            StreamEvent::ThinkingDelta("so".into()),
            delta("Hi"),
            done(),
        ]
    );
}

#[test]
fn an_error_result_is_a_failure_with_its_message() {
    let events = whole(MODEL_ERROR);
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one failure expected: {events:?}");
    };
    assert!(message.starts_with("There's an issue with the selected model"));
    assert!(message.chars().count() <= ERROR_CAP);
}

#[test]
fn an_error_message_is_flattened_and_capped() {
    let long = "x".repeat(500);
    let line = format!(
        "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"a\\n\\u001b[31mb {long}\"}}\n"
    );
    let events = whole(&line);
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one failure expected: {events:?}");
    };
    assert!(message.starts_with("a b x"), "{message}");
    assert_eq!(message.chars().count(), ERROR_CAP);
}

#[test]
fn an_error_subtype_fails_even_without_the_flag() {
    let line = "{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"result\":\"\"}\n";
    assert_eq!(
        whole(line),
        [StreamEvent::Error(ChatError::Failed("engine error".into()))]
    );
}

#[test]
fn the_result_text_is_the_answer_only_when_nothing_was_streamed() {
    let only = result_line("whole answer", "");
    assert_eq!(whole(&only), [delta("whole answer"), done()]);
    let empty = result_line("", "");
    assert_eq!(whole(&empty), [done()]);
}

#[test]
fn a_max_tokens_stop_is_a_length_finish() {
    let mut text = text_line("a");
    text.push_str(&result_line("a", ",\"stop_reason\":\"max_tokens\""));
    assert_eq!(
        whole(&text),
        [delta("a"), StreamEvent::Done(Finish::Length)]
    );
}

#[test]
fn every_split_point_gives_the_same_events() {
    for fixture in [PLAIN, MODEL_ERROR] {
        let want = whole(fixture);
        let bytes = fixture.as_bytes();
        for at in 0..=bytes.len() {
            let (a, b) = bytes.split_at(at);
            assert_eq!(run(&[a, b]), want, "cut at {at}");
        }
    }
    let want = whole(REFUSAL);
    let bytes = REFUSAL.as_bytes();
    for at in (0..=bytes.len()).step_by(13) {
        let (a, b) = bytes.split_at(at);
        assert_eq!(run(&[a, b]), want, "cut at {at}");
    }
    let singles: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(run(&singles), want, "per byte");
}

#[test]
fn a_split_character_arrives_intact() {
    let text = format!(
        "{}{}",
        text_line("caf\u{e9} \u{1f600}"),
        result_line("x", "")
    );
    let bytes = text.as_bytes();
    let emoji = bytes
        .windows(4)
        .position(|w| w == "\u{1f600}".as_bytes())
        .unwrap();
    for inside in 1..4 {
        let (a, b) = bytes.split_at(emoji + inside);
        assert_eq!(
            run(&[a, b]),
            [delta("caf\u{e9} \u{1f600}"), done()],
            "cut {inside} bytes in"
        );
    }
}

#[test]
fn a_long_result_event_is_accepted_and_not_repeated() {
    let long = "r".repeat(CLI_LINE_CAP - 4096);
    let mut text = text_line("short");
    text.push_str(&result_line(&long, ""));
    assert_eq!(whole(&text), [delta("short"), done()]);
}

#[test]
fn a_line_over_the_cap_fails_the_stream() {
    let long = "r".repeat(CLI_LINE_CAP + 1);
    let text = result_line(&long, "");
    assert_eq!(
        whole(&text),
        [StreamEvent::Error(ChatError::Failed(
            "stream line over the size limit".into()
        ))]
    );
}

#[test]
fn a_stream_without_a_result_is_interrupted() {
    let text = text_line("half");
    assert_eq!(
        whole(&text),
        [delta("half"), StreamEvent::Error(ChatError::Interrupted)]
    );
    assert_eq!(whole(""), [StreamEvent::Error(ChatError::Interrupted)]);
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    let text = format!("{}{}", text_line("a"), result_line("a", "").trim_end());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_malformed_line_fails_the_stream() {
    assert_eq!(
        whole("not json\n"),
        [StreamEvent::Error(ChatError::Failed(
            "malformed stream data".into()
        ))]
    );
}

#[test]
fn nothing_follows_the_final_event() {
    let mut text = result_line("a", "");
    text.push_str(&text_line("late"));
    text.push_str(&result_line("b", ""));
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn blank_lines_other_events_and_empty_fragments_are_ignored() {
    let mut text = String::from("\n\r\n");
    text.push_str("{\"type\":\"system\",\"subtype\":\"init\",\"tools\":[]}\n");
    text.push_str("{\"type\":\"rate_limit_event\"}\n");
    text.push_str("{\"type\":\"something_new\",\"x\":1}\n");
    text.push_str(&text_line(""));
    text.push_str(&thinking_line(""));
    text.push_str(&text_line("a"));
    text.push_str(&result_line("a", ""));
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_subagent_stream_is_not_part_of_the_answer() {
    let sub = text_line("inner").replace(
        "\"parent_tool_use_id\":null",
        "\"parent_tool_use_id\":\"toolu_x\"",
    );
    let mut text = sub;
    text.push_str(&text_line("outer"));
    text.push_str(&result_line("outer", ""));
    assert_eq!(whole(&text), [delta("outer"), done()]);
}
