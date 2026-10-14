use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;

const PLAIN: &str = include_str!("../../../../../tests/fixtures/gemini/plain.jsonl");
const MULTI: &str = include_str!("../../../../../tests/fixtures/gemini/multi-delta.jsonl");
const SPLIT_TOKEN: &str = include_str!("../../../../../tests/fixtures/gemini/split-token.jsonl");
const MODEL_ERROR: &str = include_str!("../../../../../tests/fixtures/gemini/model-error.jsonl");
const QUOTA_ERROR: &str = include_str!("../../../../../tests/fixtures/gemini/quota-error.jsonl");
const TOOL_USE: &str = include_str!("../../../../../tests/fixtures/gemini/tool-use.jsonl");

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
    let mut decoder = GeminiDecoder::new();
    let mut events: Vec<StreamEvent> = chunks.iter().flat_map(|c| decoder.feed(c)).collect();
    events.extend(decoder.finish());
    events
}

fn whole(text: &str) -> Vec<StreamEvent> {
    run(&[text.as_bytes()])
}

fn assistant_line(text: &str) -> String {
    format!(
        "{{\"type\":\"message\",\"timestamp\":\"t\",\"role\":\"assistant\",\"content\":\"{text}\",\"delta\":true}}\n"
    )
}

fn success_line() -> &'static str {
    "{\"type\":\"result\",\"timestamp\":\"t\",\"status\":\"success\",\"stats\":{\"total_tokens\":1}}\n"
}

fn error_line(message: &str) -> String {
    format!(
        "{{\"type\":\"result\",\"timestamp\":\"t\",\"status\":\"error\",\"error\":{{\"type\":\"unknown\",\"message\":\"{message}\"}}}}\n"
    )
}

#[test]
fn a_plain_answer_is_one_delta_and_a_stop() {
    // The echoed user message and the init line are not part of the answer.
    assert_eq!(whole(PLAIN), [delta("ok"), done()]);
}

#[test]
fn deltas_arrive_in_order_and_the_result_ends_the_turn() {
    let events = whole(MULTI);
    assert_eq!(events.len(), 4, "{events:?}");
    assert_eq!(events[0], delta("1,2,3,4,5,6,7,8,9,10,11,12,13,"));
    assert_eq!(events[3], done());
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.ends_with("39,40"), "{text}");
}

#[test]
fn a_token_split_across_deltas_is_just_text() {
    // M2: the model answered 391 as "3" then "91".
    assert_eq!(whole(SPLIT_TOKEN), [delta("3"), delta("91"), done()]);
}

#[test]
fn no_reasoning_is_exposed() {
    // 18.3a, gemini 0.63.0: the recorded runs (a flash model and a "thinking"
    // preview model asked to think) hold no thought event.
    assert!(!SPLIT_TOKEN.contains("thought") && !SPLIT_TOKEN.contains("reasoning"));
    // A message of another role is never shown as the answer.
    let mut text = String::from(
        "{\"type\":\"message\",\"role\":\"thought\",\"content\":\"secret\",\"delta\":true}\n",
    );
    text.push_str(&assistant_line("answer"));
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("answer"), done()]);
}

#[test]
fn an_event_kind_outside_the_allowlist_ends_the_turn() {
    // Fail closed: a newer gemini that adds an event (a thought, a plan, a
    // new kind of tool) is refused until lazarobox knows it.
    for kind in ["thought", "plan", "tool_call", "something_new", ""] {
        let mut text = assistant_line("a");
        text.push_str(&format!("{{\"type\":\"{kind}\"}}\n"));
        text.push_str(&assistant_line("late"));
        text.push_str(success_line());
        assert_eq!(
            whole(&text),
            [delta("a"), failed(UNKNOWN_EVENT)],
            "{kind:?}"
        );
    }
    // A line with no type at all is no known kind either.
    assert_eq!(whole("{}\n"), [failed(UNKNOWN_EVENT)]);
}

#[test]
fn the_warning_event_and_the_known_kinds_do_not_end_the_turn() {
    let mut text = String::from(
        "{\"type\":\"init\",\"session_id\":\"x\",\"model\":\"m\"}\n{\"type\":\"error\",\"severity\":\"warning\",\"message\":\"careful\"}\n",
    );
    text.push_str(&assistant_line("a"));
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_failed_result_gives_the_api_message_without_its_wrapper() {
    assert_eq!(
        whole(MODEL_ERROR),
        [failed(
            "models/nosuchmodel is not found for API version v1beta, or is not supported for generateContent. Call ModelService.ListModels to see the list of available models and their supported methods."
        )]
    );
    assert_eq!(
        whole(QUOTA_ERROR),
        [failed(
            "You have exhausted your daily quota on this model. Please wait and try again later. To increase your limits, request a quota increase through AI Studio, or switch to another /auth method"
        )]
    );
}

#[test]
fn an_error_after_some_text_keeps_the_text() {
    let mut text = assistant_line("part");
    text.push_str(&error_line("[API Error: boom]"));
    assert_eq!(whole(&text), [delta("part"), failed("boom")]);
}

#[test]
fn a_plain_failure_message_is_flattened_and_capped() {
    let long = "x".repeat(500);
    let events = whole(&error_line(&format!("a\\n\\u001b[31mb {long}")));
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one failure expected: {events:?}");
    };
    assert!(message.starts_with("a b x"), "{message}");
    assert_eq!(message.chars().count(), ERROR_CAP);
}

#[test]
fn an_empty_or_missing_failure_says_engine_error() {
    assert_eq!(whole(&error_line("")), [failed("engine error")]);
    assert_eq!(
        whole("{\"type\":\"result\",\"status\":\"error\"}\n"),
        [failed("engine error")]
    );
    // Any status that is not a success is a failure.
    assert_eq!(
        whole("{\"type\":\"result\",\"status\":\"weird\"}\n"),
        [failed("engine error")]
    );
}

#[test]
fn a_tool_call_ends_the_turn_with_an_error() {
    // M3 on gemini 0.63.0 with tools on: a harmless `update_topic` and
    // `read_file` both show as `tool_use`. The pinned flags and the policy
    // remove every tool, and if one slips through the turn stops here.
    let events = whole(TOOL_USE);
    assert_eq!(events, [failed(TOOL_REFUSED)]);
}

#[test]
fn text_before_a_tool_call_is_kept() {
    let mut text = assistant_line("let me look");
    text.push_str("{\"type\":\"tool_use\",\"tool_name\":\"read_file\",\"parameters\":{}}\n");
    text.push_str(&assistant_line("late"));
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("let me look"), failed(TOOL_REFUSED)]);
}

#[test]
fn a_tool_result_alone_is_not_a_call() {
    let mut text =
        String::from("{\"type\":\"tool_result\",\"tool_id\":\"x\",\"status\":\"error\"}\n");
    text.push_str(&assistant_line("fine"));
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("fine"), done()]);
}

#[test]
fn only_assistant_text_counts() {
    let mut text = String::from(
        "{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}\n{\"type\":\"message\",\"role\":\"system\",\"content\":\"hi\",\"delta\":true}\n{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"\",\"delta\":true}\n{\"type\":\"message\",\"role\":\"assistant\",\"delta\":true}\n{\"type\":\"message\",\"role\":\"assistant\",\"content\":7,\"delta\":true}\n",
    );
    text.push_str(&assistant_line("a"));
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_turn_with_no_text_is_just_done() {
    assert_eq!(whole(success_line()), [done()]);
}

#[test]
fn every_split_point_gives_the_same_events() {
    for fixture in [
        PLAIN,
        MULTI,
        SPLIT_TOKEN,
        MODEL_ERROR,
        QUOTA_ERROR,
        TOOL_USE,
    ] {
        let want = whole(fixture);
        let bytes = fixture.as_bytes();
        for cut in 0..=bytes.len() {
            assert_eq!(run(&[&bytes[..cut], &bytes[cut..]]), want, "cut {cut}");
        }
        let singles: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(run(&singles), want);
    }
}

#[test]
fn a_split_character_arrives_intact() {
    let text = assistant_line("héllo wörld ✓");
    let full = format!("{text}{}", success_line());
    let bytes = full.as_bytes();
    let at = full.find('é').unwrap() + 1;
    assert_eq!(
        run(&[&bytes[..at], &bytes[at..]]),
        [delta("héllo wörld ✓"), done()]
    );
}

#[test]
fn a_long_delta_is_accepted() {
    let long = "r".repeat(CLI_LINE_CAP - 4096);
    let mut text = assistant_line(&long);
    text.push_str(success_line());
    // `assert!`: a failure must not print megabytes.
    assert!(whole(&text) == [delta(&long), done()]);
}

#[test]
fn a_line_over_the_cap_fails_the_stream() {
    let long = "r".repeat(CLI_LINE_CAP + 1);
    assert!(whole(&assistant_line(&long)) == [failed("stream line over the size limit")]);
}

#[test]
fn a_stream_without_a_result_is_interrupted() {
    let text = assistant_line("half");
    assert_eq!(
        whole(&text),
        [delta("half"), StreamEvent::Error(ChatError::Interrupted)]
    );
    assert_eq!(whole(""), [StreamEvent::Error(ChatError::Interrupted)]);
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    let text = format!("{}{}", assistant_line("a"), success_line().trim_end());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_malformed_line_fails_the_stream() {
    assert_eq!(whole("{not json\n"), [failed("malformed stream data")]);
}

#[test]
fn nothing_follows_the_final_event() {
    let mut text = assistant_line("a");
    text.push_str(success_line());
    text.push_str(&assistant_line("late"));
    text.push_str(&error_line("late"));
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn stray_text_and_blank_lines_are_ignored() {
    let mut text = String::from("\n\r\nWARN something on stdout\n");
    text.push_str("{\"type\":\"init\",\"session_id\":\"x\",\"model\":\"m\"}\n");
    text.push_str(&assistant_line("a"));
    text.push_str("WARN again\n");
    text.push_str(success_line());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_broken_json_line_is_still_malformed_even_after_stray_text() {
    assert_eq!(
        whole("WARN x\n{\"type\":\n"),
        [failed("malformed stream data")]
    );
}
