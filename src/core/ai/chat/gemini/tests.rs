use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;
use crate::core::ai::chat::usage::TurnUsage;

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
fn a_plain_answer_is_one_delta_a_usage_and_a_stop() {
    // The echoed user message and the init line are not part of the answer.
    let usage = TurnUsage {
        input: Some(17318),
        output: Some(0),
        cache_read: Some(0),
        duration_ms: Some(2025),
        model: Some("gemini-2.5-flash".into()),
        ..TurnUsage::default()
    };
    assert_eq!(
        whole(PLAIN),
        [delta("ok"), StreamEvent::usage(usage), done()]
    );
}

#[test]
fn deltas_arrive_in_order_and_the_result_ends_the_turn() {
    let events = whole(MULTI);
    assert_eq!(events.len(), 5, "{events:?}");
    assert_eq!(events[0], delta("1,2,3,4,5,6,7,8,9,10,11,12,13,"));
    assert!(matches!(events[3], StreamEvent::Usage(_)));
    assert_eq!(events[4], done());
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
    let events = whole(SPLIT_TOKEN);
    assert_eq!(events.len(), 4, "{events:?}");
    assert_eq!((&events[0], &events[1]), (&delta("3"), &delta("91")));
    assert!(matches!(events[2], StreamEvent::Usage(_)));
    assert_eq!(events[3], done());
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
        without_usage(whole(QUOTA_ERROR)),
        [failed(
            "You have exhausted your daily quota on this model. Please wait and try again later. To increase your limits, request a quota increase through AI Studio, or switch to another /auth method"
        )]
    );
    // The quota run did spend tokens before it failed: they are reported.
    let events = whole(QUOTA_ERROR);
    let spent = usage_of(&events).expect("usage");
    assert_eq!((spent.input, spent.output), (Some(33972 - 15971), Some(89)));
    assert!(matches!(events[0], StreamEvent::Usage(_)));
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

#[test]
fn only_the_closing_bracket_of_the_api_wrapper_is_removed() {
    // A `]` inside the text stays; the wrapper's is the last one.
    assert_eq!(
        whole(&error_line("[API Error: bad [x] thing]")),
        [failed("bad [x] thing")]
    );
    // Text with no wrapper is left alone.
    assert_eq!(
        whole(&error_line("plain [x] text")),
        [failed("plain [x] text")]
    );
}

fn tool_use_line(name: &str) -> String {
    format!(
        "{{\"type\":\"tool_use\",\"tool_name\":\"{name}\",\"tool_id\":\"t\",\"parameters\":{{}}}}\n"
    )
}

fn run_wired(text: &str) -> Vec<StreamEvent> {
    let mut decoder = GeminiDecoder::wired();
    let mut events = decoder.feed(text.as_bytes());
    events.extend(decoder.finish());
    events
}

/// A turn that calls `name`, then answers.
fn calls_then_answers(name: &str) -> String {
    let mut text = tool_use_line(name);
    text.push_str("{\"type\":\"tool_result\",\"tool_id\":\"t\",\"status\":\"success\"}\n");
    text.push_str(&assistant_line("done"));
    text.push_str(success_line());
    text
}

#[test]
fn a_wired_turn_lets_a_lazarobox_tool_through() {
    for name in ["mcp_lazarobox_read_file", "mcp_lazarobox_run_in_pane"] {
        assert_eq!(
            run_wired(&calls_then_answers(name)),
            [delta("done"), done()],
            "{name}"
        );
    }
}

#[test]
fn a_wired_turn_still_refuses_every_other_tool() {
    // `read_file` is also gemini's own built-in: only the qualified name is
    // ours.
    for name in [
        "read_file",
        "run_shell_command",
        "mcp_other_read_file",
        "mcp_lazarobox",
        "mcp_lazarobox_",
        "MCP_LAZAROBOX_read_file",
        "lazarobox__read_file",
        " mcp_lazarobox_read_file",
        "",
    ] {
        assert_eq!(
            run_wired(&calls_then_answers(name)),
            [failed(TOOL_REFUSED)],
            "{name:?}"
        );
    }
}

#[test]
fn a_wired_tool_call_without_a_name_or_with_a_non_text_name_is_refused() {
    for line in [
        "{\"type\":\"tool_use\",\"parameters\":{}}\n",
        "{\"type\":\"tool_use\",\"tool_name\":7}\n",
        "{\"type\":\"tool_use\",\"tool_name\":[\"mcp_lazarobox_read_file\"]}\n",
    ] {
        assert_eq!(run_wired(line), [failed(TOOL_REFUSED)], "{line}");
    }
}

#[test]
fn a_wired_turn_keeps_failing_closed_on_an_unknown_event() {
    let text = "{\"type\":\"thought\",\"content\":\"x\"}\n";
    assert_eq!(run_wired(text), [failed(UNKNOWN_EVENT)]);
}

#[test]
fn a_text_only_turn_refuses_even_our_own_tool() {
    assert_eq!(
        whole(&calls_then_answers("mcp_lazarobox_read_file")),
        [failed(TOOL_REFUSED)]
    );
}

fn result_with(stats: &str) -> String {
    format!(
        "{{\"type\":\"result\",\"timestamp\":\"t\",\"status\":\"success\",\"stats\":{stats}}}\n"
    )
}

fn usage_of(events: &[StreamEvent]) -> Option<TurnUsage> {
    events.iter().find_map(|e| match e {
        StreamEvent::Usage(u) => Some(TurnUsage::clone(u)),
        _ => None,
    })
}

#[test]
fn cached_is_taken_out_of_input_and_there_are_no_dollars() {
    let stats = r#"{"total_tokens":99,"input_tokens":1000,"output_tokens":40,"cached":600,"duration_ms":7}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!(got.input, Some(400));
    assert_eq!(got.cache_read, Some(600));
    assert_eq!(got.output, Some(40));
    assert_eq!(got.duration_ms, Some(7));
    assert_eq!(
        (got.cost_micros, got.cost_source, got.model),
        (None, None, None)
    );
}

#[test]
fn cached_larger_than_input_saturates_at_zero() {
    let stats = r#"{"input_tokens":5,"output_tokens":1,"cached":9}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!((got.input, got.cache_read), (Some(0), Some(9)));
}

#[test]
fn input_without_a_cached_count_stays_whole() {
    let stats = r#"{"input_tokens":12,"output_tokens":3}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!((got.input, got.cache_read), (Some(12), None));
}

#[test]
fn an_unknown_stats_shape_is_no_data_and_the_answer_completes() {
    for stats in ["\"oops\"", "{\"models\":{\"x\":[1]}}", "[]", "{}", "null"] {
        let events = whole(&format!("{}{}", assistant_line("hi"), result_with(stats)));
        assert_eq!(events, [delta("hi"), done()], "{stats}");
    }
}

#[test]
fn a_bad_count_is_none_for_that_field_only() {
    let stats = r#"{"input_tokens":"x","output_tokens":-1,"cached":3}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!(
        (got.input, got.output, got.cache_read),
        (None, None, Some(3))
    );
}

#[test]
fn an_error_result_that_spent_tokens_reports_them_before_the_error() {
    let line = r#"{"type":"result","status":"error","error":{"message":"boom"},"stats":{"input_tokens":4,"output_tokens":2}}"#;
    let events = whole(&format!("{line}\n"));
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(matches!(events[0], StreamEvent::Usage(_)));
    assert!(matches!(events[1], StreamEvent::Error(_)));
}

fn without_usage(events: Vec<StreamEvent>) -> Vec<StreamEvent> {
    events
        .into_iter()
        .filter(|e| !matches!(e, StreamEvent::Usage(_)))
        .collect()
}

#[test]
fn a_net_input_field_wins_over_the_difference() {
    let stats = r#"{"input_tokens":100,"input":37,"output_tokens":3,"cached":60}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!((got.input, got.cache_read), (Some(37), Some(60)));
}

#[test]
fn a_malformed_cached_count_makes_the_input_unknown() {
    let stats = r#"{"input_tokens":100,"output_tokens":3,"cached":"x"}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!(
        (got.input, got.cache_read, got.output),
        (None, None, Some(3))
    );
}

#[test]
fn more_than_one_model_is_flagged_and_gets_no_model_name() {
    let stats = r#"{"input_tokens":5,"output_tokens":1,"models":{"a":{},"b":{}}}"#;
    let got = usage_of(&whole(&result_with(stats))).expect("usage");
    assert_eq!((got.model.clone(), got.multi_model), (None, true));
}
