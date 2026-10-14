use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;

const PLAIN: &str = include_str!("../../../../../tests/fixtures/codex/plain.jsonl");
const NO_REASONING: &str =
    include_str!("../../../../../tests/fixtures/codex/no-reasoning-item.jsonl");
const PREAMBLE: &str =
    include_str!("../../../../../tests/fixtures/codex/preamble-then-answer.jsonl");
const SHELL: &str = include_str!("../../../../../tests/fixtures/codex/shell-attempt.jsonl");
const MODEL_ERROR: &str = include_str!("../../../../../tests/fixtures/codex/model-error.jsonl");
const WARNING_ERROR: &str =
    include_str!("../../../../../tests/fixtures/codex/model-warning-error.jsonl");

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
    let mut decoder = CodexDecoder::new();
    let mut events: Vec<StreamEvent> = chunks.iter().flat_map(|c| decoder.feed(c)).collect();
    events.extend(decoder.finish());
    events
}

fn whole(text: &str) -> Vec<StreamEvent> {
    run(&[text.as_bytes()])
}

/// An `item.completed` line holding an agent message.
fn message_line(text: &str) -> String {
    format!(
        "{{\"type\":\"item.completed\",\"item\":{{\"id\":\"item_0\",\"type\":\"agent_message\",\"text\":\"{text}\"}}}}\n"
    )
}

fn completed_line() -> &'static str {
    "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1,\"reasoning_output_tokens\":0}}\n"
}

fn turn_failed_line(message: &str) -> String {
    format!("{{\"type\":\"turn.failed\",\"error\":{{\"message\":\"{message}\"}}}}\n")
}

#[test]
fn a_plain_answer_is_one_delta_and_a_stop() {
    assert_eq!(whole(PLAIN), [delta("ok"), done()]);
}

#[test]
fn the_recorded_run_that_spent_reasoning_tokens_exposes_no_reasoning() {
    // M2 on codex 0.142.5: the usage counts reasoning tokens, but with the
    // pinned flags no event carries reasoning text, so none is produced.
    assert!(NO_REASONING.contains("\"reasoning_output_tokens\":23"));
    assert!(!NO_REASONING.contains("\"reasoning\""));
    assert_eq!(whole(NO_REASONING), [delta("9"), done()]);
}

#[test]
fn a_reasoning_item_is_not_assumed_to_be_shown() {
    // No recorded format: an item of that type is skipped, never guessed at.
    let mut text = String::from(
        "{\"type\":\"item.completed\",\"item\":{\"id\":\"item_0\",\"type\":\"reasoning\",\"text\":\"secret thoughts\"}}\n",
    );
    text.push_str(&message_line("answer"));
    text.push_str(completed_line());
    assert_eq!(whole(&text), [delta("answer"), done()]);
}

#[test]
fn messages_of_one_turn_are_separated_by_a_blank_line() {
    let events = whole(PREAMBLE);
    assert_eq!(events.len(), 3, "{events:?}");
    let StreamEvent::Delta(first) = &events[0] else {
        panic!("{events:?}");
    };
    let StreamEvent::Delta(second) = &events[1] else {
        panic!("{events:?}");
    };
    assert!(first.starts_with("Voy a ejecutar"), "{first}");
    assert!(
        second.starts_with("\n\n1. Read `~/.ssh/config`"),
        "{second}"
    );
    assert_eq!(events[2], done());
}

#[test]
fn a_failed_turn_gives_the_api_message_not_the_json_around_it() {
    assert_eq!(
        whole(MODEL_ERROR),
        [failed(
            "The 'gpt-5.4-mini' model is not supported when using Codex with a ChatGPT account."
        )]
    );
}

#[test]
fn a_warning_item_is_not_a_failure_and_the_error_events_do_not_end_the_turn() {
    assert_eq!(
        whole(WARNING_ERROR),
        [failed(
            "The 'gpt-5-codex' model is not supported when using Codex with a ChatGPT account."
        )]
    );
}

#[test]
fn an_error_event_alone_is_a_retry_notice_and_the_turn_goes_on() {
    let mut text = String::from("{\"type\":\"error\",\"message\":\"Reconnecting... 1/5\"}\n");
    text.push_str(&message_line("late answer"));
    text.push_str(completed_line());
    assert_eq!(whole(&text), [delta("late answer"), done()]);
}

#[test]
fn a_plain_text_failure_message_is_flattened_and_capped() {
    let long = "x".repeat(500);
    let text = turn_failed_line(&format!("a\\n\\u001b[31mb {long}"));
    let events = whole(&text);
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one failure expected: {events:?}");
    };
    assert!(message.starts_with("a b x"), "{message}");
    assert_eq!(message.chars().count(), ERROR_CAP);
}

#[test]
fn an_empty_failure_says_engine_error() {
    assert_eq!(whole(&turn_failed_line("")), [failed("engine error")]);
    assert_eq!(
        whole("{\"type\":\"turn.failed\"}\n"),
        [failed("engine error")]
    );
}

#[test]
fn a_json_failure_without_a_message_keeps_a_safe_one_line_text() {
    let text = turn_failed_line("{\\\"status\\\":500}");
    let events = whole(&text);
    let [StreamEvent::Error(ChatError::Failed(message))] = events.as_slice() else {
        panic!("one failure expected: {events:?}");
    };
    assert_eq!(message, "{\"status\":500}");
}

#[test]
fn a_command_that_codex_starts_ends_the_turn_with_an_error() {
    // M3 on codex 0.142.5 with only the sandbox flag: the model ran a shell
    // command. The pinned flags remove the tool, and if one slips through the
    // turn stops here instead of showing a transcript of tool calls.
    let events = whole(SHELL);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(matches!(&events[0], StreamEvent::Delta(t) if t.starts_with("I’ll run")));
    assert_eq!(events[1], failed(TOOL_REFUSED));
}

#[test]
fn file_changes_and_mcp_calls_end_the_turn_too_but_other_items_do_not() {
    for kind in ["file_change", "mcp_tool_call", "collab_tool_call"] {
        let text = format!(
            "{{\"type\":\"item.started\",\"item\":{{\"id\":\"item_1\",\"type\":\"{kind}\"}}}}\n"
        );
        assert_eq!(whole(&text), [failed(TOOL_REFUSED)], "{kind}");
    }
    let mut text = String::new();
    for kind in ["web_search", "todo_list", "something_new"] {
        text.push_str(&format!(
            "{{\"type\":\"item.completed\",\"item\":{{\"id\":\"item_1\",\"type\":\"{kind}\"}}}}\n"
        ));
    }
    text.push_str(&message_line("fine"));
    text.push_str(completed_line());
    assert_eq!(whole(&text), [delta("fine"), done()]);
}

#[test]
fn only_a_completed_message_counts() {
    let mut text = String::from(
        "{\"type\":\"item.started\",\"item\":{\"id\":\"item_0\",\"type\":\"agent_message\",\"text\":\"draft\"}}\n",
    );
    text.push_str(
        "{\"type\":\"item.updated\",\"item\":{\"id\":\"item_0\",\"type\":\"agent_message\",\"text\":\"draft 2\"}}\n",
    );
    text.push_str(&message_line("final"));
    text.push_str(completed_line());
    assert_eq!(whole(&text), [delta("final"), done()]);
}

#[test]
fn an_empty_message_is_skipped_and_a_turn_with_no_text_is_just_done() {
    let mut text = message_line("");
    text.push_str(completed_line());
    assert_eq!(whole(&text), [done()]);
}

#[test]
fn every_split_point_gives_the_same_events() {
    for fixture in [PLAIN, PREAMBLE, MODEL_ERROR, WARNING_ERROR, SHELL] {
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
    let text = message_line("héllo wörld ✓");
    let full = format!("{text}{}", completed_line());
    let bytes = full.as_bytes();
    let at = full.find('é').unwrap() + 1;
    assert_eq!(
        run(&[&bytes[..at], &bytes[at..]]),
        [delta("héllo wörld ✓"), done()]
    );
}

#[test]
fn a_long_message_is_accepted() {
    let long = "r".repeat(CLI_LINE_CAP - 4096);
    let mut text = message_line(&long);
    text.push_str(completed_line());
    // `assert!`: a failure must not print megabytes.
    assert!(whole(&text) == [delta(&long), done()]);
}

#[test]
fn a_line_over_the_cap_fails_the_stream() {
    let long = "r".repeat(CLI_LINE_CAP + 1);
    assert!(whole(&message_line(&long)) == [failed("stream line over the size limit")]);
}

#[test]
fn a_stream_without_an_end_is_interrupted() {
    let text = message_line("half");
    assert_eq!(
        whole(&text),
        [delta("half"), StreamEvent::Error(ChatError::Interrupted)]
    );
    assert_eq!(whole(""), [StreamEvent::Error(ChatError::Interrupted)]);
}

#[test]
fn a_last_line_without_a_newline_still_counts() {
    let text = format!("{}{}", message_line("a"), completed_line().trim_end());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_malformed_line_fails_the_stream() {
    assert_eq!(whole("{not json\n"), [failed("malformed stream data")]);
}

#[test]
fn nothing_follows_the_final_event() {
    let mut text = message_line("a");
    text.push_str(completed_line());
    text.push_str(&message_line("late"));
    text.push_str(&turn_failed_line("late"));
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn stray_text_blank_lines_and_other_events_are_ignored() {
    let mut text = String::from("\n\r\nWARN something on stdout\n");
    text.push_str(
        "{\"type\":\"thread.started\",\"thread_id\":\"x\"}\n{\"type\":\"turn.started\"}\n",
    );
    text.push_str(&message_line("a"));
    text.push_str("WARN again\n");
    text.push_str("{\"type\":\"unknown.event\"}\n");
    text.push_str(completed_line());
    assert_eq!(whole(&text), [delta("a"), done()]);
}

#[test]
fn a_broken_json_line_is_still_malformed_even_after_stray_text() {
    assert_eq!(
        whole("WARN x\n{\"type\":\n"),
        [failed("malformed stream data")]
    );
}
