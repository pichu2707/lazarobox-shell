use super::super::engine::{ChatError, Finish};
use super::super::stream::{CLI_LINE_CAP, ERROR_CAP};
use super::*;
use crate::core::ai::chat::usage::{CostSource, TurnUsage};

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
    run_as(ClaudeDecoder::new(), chunks)
}

fn run_as(mut decoder: ClaudeDecoder, chunks: &[&[u8]]) -> Vec<StreamEvent> {
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
fn a_plain_answer_is_one_delta_usage_and_a_stop() {
    assert_eq!(
        whole(PLAIN),
        [delta("ok"), StreamEvent::usage(plain_usage()), done()]
    );
}

/// The numbers of `plain.jsonl`.
fn plain_usage() -> TurnUsage {
    TurnUsage {
        input: Some(2),
        output: Some(4),
        cache_read: Some(0),
        cache_write: Some(606),
        reasoning: Some(0),
        cost_micros: Some(4_936),
        cost_source: Some(CostSource::EngineList),
        duration_ms: Some(1321),
        model: Some("claude-opus-5-5".into()),
        partial: false,
    }
}

/// The usage of a stream: the `Usage` events, in order.
fn usages(events: &[StreamEvent]) -> Vec<TurnUsage> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Usage(u) => Some(TurnUsage::clone(u)),
            _ => None,
        })
        .collect()
}

#[test]
fn usage_comes_before_the_final_event() {
    let events = whole(PLAIN);
    let at = |want: fn(&StreamEvent) -> bool| events.iter().position(want);
    let usage = at(|e| matches!(e, StreamEvent::Usage(_)));
    let end = at(|e| matches!(e, StreamEvent::Done(_)));
    assert!(usage.is_some() && usage < end, "{events:?}");
}

#[test]
fn model_usage_of_the_main_model_and_a_subagent_is_summed() {
    let extra = r#","total_cost_usd":0.5,"duration_api_ms":900,"modelUsage":{"claude-opus-5-5":{"inputTokens":10,"outputTokens":20,"cacheReadInputTokens":30,"cacheCreationInputTokens":40,"thinkingTokens":5,"costUSD":0.4},"claude-haiku-4-5":{"inputTokens":1,"outputTokens":2,"cacheReadInputTokens":3,"cacheCreationInputTokens":4,"thinkingTokens":1,"costUSD":0.1}}"#;
    let got = usages(&whole(&result_line("ok", extra)));
    assert_eq!(
        got,
        [TurnUsage {
            input: Some(11),
            output: Some(22),
            cache_read: Some(33),
            cache_write: Some(44),
            reasoning: Some(6),
            cost_micros: Some(500_000),
            cost_source: Some(CostSource::EngineList),
            duration_ms: Some(900),
            model: None,
            partial: false,
        }]
    );
}

#[test]
fn without_model_usage_the_top_level_usage_is_read() {
    let extra = r#","usage":{"input_tokens":7,"output_tokens":8,"cache_read_input_tokens":9,"cache_creation_input_tokens":10}"#;
    let got = usages(&whole(&result_line("ok", extra)));
    assert_eq!(got.len(), 1);
    assert_eq!(
        (
            got[0].input,
            got[0].output,
            got[0].cache_read,
            got[0].cache_write
        ),
        (Some(7), Some(8), Some(9), Some(10))
    );
    assert_eq!(got[0].cost_micros, None);
    assert_eq!(got[0].cost_source, None);
}

#[test]
fn cost_falls_back_to_the_sum_of_model_costs() {
    let extra = r#","modelUsage":{"a":{"inputTokens":1,"costUSD":0.25},"b":{"inputTokens":1,"costUSD":0.5}}"#;
    let got = usages(&whole(&result_line("ok", extra)));
    assert_eq!(got[0].cost_micros, Some(750_000));
}

#[test]
fn a_bad_or_negative_cost_is_no_cost_and_the_counts_survive() {
    for cost in ["-1", "\"free\""] {
        let extra =
            format!(r#","total_cost_usd":{cost},"usage":{{"input_tokens":3,"output_tokens":4}}"#);
        let events = whole(&result_line("ok", &extra));
        let got = usages(&events);
        assert_eq!(got.len(), 1, "{cost}");
        assert_eq!((got[0].input, got[0].output), (Some(3), Some(4)), "{cost}");
        assert_eq!(
            (got[0].cost_micros, got[0].cost_source),
            (None, None),
            "{cost}"
        );
        assert_eq!(events.last(), Some(&done()), "{cost}");
    }
}

#[test]
fn a_bad_count_is_none_for_that_field_only() {
    let extra =
        r#","usage":{"input_tokens":"many","output_tokens":-5,"cache_read_input_tokens":7}"#;
    let got = usages(&whole(&result_line("ok", extra)));
    assert_eq!(
        (got[0].input, got[0].output, got[0].cache_read),
        (None, None, Some(7))
    );
}

#[test]
fn a_malformed_usage_object_gives_no_record_and_the_answer_completes() {
    for extra in [
        r#","usage":"oops""#,
        r#","modelUsage":[1,2]"#,
        r#","usage":{}"#,
    ] {
        let events = whole(&result_line("ok", extra));
        assert_eq!(events, [delta("ok"), done()], "{extra}");
    }
}

#[test]
fn an_error_result_still_reports_the_usage_before_the_error() {
    let extra = r#","usage":{"input_tokens":3,"output_tokens":1}"#;
    let text = format!("{{\"type\":\"result\",\"is_error\":true,\"result\":\"boom\"{extra}}}\n");
    let events = whole(&text);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(matches!(events[0], StreamEvent::Usage(_)));
    assert!(matches!(events[1], StreamEvent::Error(_)));
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
        whole("{not json\n"),
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

#[test]
fn stray_non_json_output_before_and_inside_the_stream_is_ignored() {
    let mut text = String::from("Warning: something on stdout\n");
    text.push_str("  (node:1) ExperimentalWarning: x\n");
    text.push_str(&text_line("a"));
    text.push_str("DeprecationWarning: mid-stream\n\t \n");
    text.push_str(&text_line("b"));
    text.push_str(&result_line("ab", ""));
    assert_eq!(whole(&text), [delta("a"), delta("b"), done()]);
    let singles: Vec<&[u8]> = text.as_bytes().chunks(1).collect();
    assert_eq!(run(&singles), [delta("a"), delta("b"), done()]);
}

#[test]
fn a_broken_json_line_is_still_malformed_even_after_stray_text() {
    let text = format!("noise\n{{\"type\":\n{}", result_line("a", ""));
    assert_eq!(
        whole(&text),
        [StreamEvent::Error(ChatError::Failed(
            "malformed stream data".into()
        ))]
    );
}

fn start_line(block: &str) -> String {
    format!(
        "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{block}}},\"parent_tool_use_id\":null}}\n"
    )
}

fn assistant_line(block: &str) -> String {
    format!(
        "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"x\"}},{block}]}},\"parent_tool_use_id\":null}}\n"
    )
}

fn tool_use(name: &str) -> String {
    format!("{{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"{name}\",\"input\":{{}}}}")
}

fn refused() -> Vec<StreamEvent> {
    vec![StreamEvent::Error(ChatError::Failed(TOOL_REFUSED.into()))]
}

/// Both decoders: the tolerant one (no tools offered) and the strict one.
type Make = fn() -> ClaudeDecoder;

fn modes() -> [(&'static str, Make); 2] {
    [
        ("text-only", ClaudeDecoder::new),
        ("wired", ClaudeDecoder::wired),
    ]
}

fn whole_as(make: Make, text: &str) -> Vec<StreamEvent> {
    run_as(make(), &[text.as_bytes()])
}

fn message_start_line(block: &str) -> String {
    format!(
        "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"message_start\",\"message\":{{\"role\":\"assistant\",\"content\":[{block}],\"usage\":{{\"server_tool_use\":{{\"web_search_requests\":0}}}}}}}},\"parent_tool_use_id\":null}}\n"
    )
}

fn delta_line(delta: &str) -> String {
    format!(
        "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{delta}}},\"parent_tool_use_id\":null}}\n"
    )
}

/// Every place a content block can show up.
fn lines_with(block: &str) -> [String; 3] {
    [
        start_line(block),
        assistant_line(block),
        message_start_line(block),
    ]
}

#[test]
fn a_lazarobox_tool_use_is_allowed_only_in_a_wired_turn_and_adds_no_event() {
    for name in ["mcp__lazarobox__find_dir", "mcp__lazarobox__run_in_pane"] {
        for line in lines_with(&tool_use(name)) {
            let stream = format!("{}{}{}", text_line("a"), line, result_line("a", ""));
            assert_eq!(
                whole_as(ClaudeDecoder::wired, &stream),
                [delta("a"), done()],
                "{name}"
            );
            // No tools were offered, so even our own is foreign.
            assert_eq!(
                whole_as(ClaudeDecoder::new, &stream),
                [delta("a")]
                    .into_iter()
                    .chain(refused())
                    .collect::<Vec<_>>(),
                "{name}"
            );
        }
    }
}

#[test]
fn any_other_tool_use_fails_closed_in_both_modes_and_ends_the_stream() {
    let names = [
        "Bash",
        "Read",
        "mcp__other__find_dir",
        "mcp__lazarobox",
        "mcp__lazarobox_evil__x",
        "lazarobox__find_dir",
        // The bare prefix names no tool.
        "mcp__lazarobox__",
        "",
    ];
    for (mode, make) in modes() {
        for name in names {
            for line in lines_with(&tool_use(name)) {
                let stream = format!("{}{}{}", text_line("a"), line, result_line("late", ""));
                assert_eq!(
                    whole_as(make, &stream),
                    [delta("a")]
                        .into_iter()
                        .chain(refused())
                        .collect::<Vec<_>>(),
                    "{mode} {name:?}"
                );
            }
        }
    }
}

#[test]
fn a_server_side_tool_or_a_tool_use_without_a_name_fails_closed_in_both_modes() {
    for (mode, make) in modes() {
        for block in [
            "{\"type\":\"server_tool_use\",\"id\":\"s\",\"name\":\"web_search\",\"input\":{}}",
            "{\"type\":\"server_tool_use\",\"id\":\"s\",\"name\":\"mcp__lazarobox__find_dir\",\"input\":{}}",
            "{\"type\":\"tool_use\",\"id\":\"s\"}",
            "{\"type\":\"tool_use\",\"name\":7}",
        ] {
            for line in lines_with(block) {
                assert_eq!(whole_as(make, &line), refused(), "{mode} {block}");
            }
        }
    }
}

#[test]
fn a_block_type_in_another_case_or_a_computer_use_block_is_refused_in_both_modes() {
    for (mode, make) in modes() {
        for block in [
            "{\"type\":\"Tool_Use\",\"id\":\"s\",\"name\":\"mcp__lazarobox__find_dir\",\"input\":{}}",
            "{\"type\":\"TOOL_USE\",\"name\":\"Bash\"}",
            "{\"type\":\"computer_use\",\"id\":\"s\"}",
            "{\"type\":\"mcp_tool_result\",\"id\":\"s\"}",
        ] {
            for line in lines_with(block) {
                assert_eq!(whole_as(make, &line), refused(), "{mode} {block}");
            }
        }
    }
}

#[test]
fn an_unknown_benign_block_type_ends_a_wired_turn_but_not_a_text_only_one() {
    let block = "{\"type\":\"citation_block\",\"x\":1}";
    for line in lines_with(block) {
        let stream = format!("{}{}{}", text_line("a"), line, result_line("a", ""));
        assert_eq!(
            whole_as(ClaudeDecoder::new, &stream),
            [delta("a"), done()],
            "tolerant: {line}"
        );
        assert_eq!(
            whole_as(ClaudeDecoder::wired, &stream)
                .into_iter()
                .skip(1)
                .collect::<Vec<_>>(),
            refused(),
            "strict: {line}"
        );
    }
}

#[test]
fn the_blocks_of_a_plain_answer_pass_in_both_modes() {
    for (mode, make) in modes() {
        for block in [
            "{\"type\":\"text\",\"text\":\"x\"}",
            "{\"type\":\"thinking\",\"thinking\":\"\"}",
            "{\"type\":\"redacted_thinking\",\"data\":\"x\"}",
        ] {
            for line in lines_with(block) {
                let stream = format!("{line}{}", result_line("a", ""));
                assert_eq!(
                    whole_as(make, &stream),
                    [delta("a"), done()],
                    "{mode} {block}"
                );
            }
        }
    }
}

#[test]
fn a_tool_block_in_a_delta_is_refused_in_both_modes() {
    for (mode, make) in modes() {
        for delta in [
            "{\"type\":\"tool_use_delta\"}",
            "{\"type\":\"server_tool_use\"}",
            "{\"type\":\"x\",\"content_block\":{\"type\":\"tool_use\",\"name\":\"Bash\"}}",
        ] {
            assert_eq!(
                whole_as(make, &delta_line(delta)),
                refused(),
                "{mode} {delta}"
            );
        }
    }
}

#[test]
fn the_deltas_of_a_tool_call_and_the_usage_counters_are_not_tool_blocks() {
    // `server_tool_use` inside `message.usage` is a counter, not a block.
    for (mode, make) in modes() {
        let stream = format!(
            "{}{}{}",
            message_start_line("{\"type\":\"text\",\"text\":\"\"}"),
            delta_line("{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}"),
            result_line("a", "")
        );
        assert_eq!(whole_as(make, &stream), [delta("a"), done()], "{mode}");
    }
}

#[test]
fn every_fixture_decodes_the_same_in_both_modes() {
    for fixture in [PLAIN, SONNET, MODEL_ERROR, REFUSAL] {
        assert_eq!(
            whole_as(ClaudeDecoder::wired, fixture),
            whole_as(ClaudeDecoder::new, fixture)
        );
    }
}

#[test]
fn a_subagent_tool_use_is_judged_like_any_other() {
    let line = start_line(&tool_use("Bash")).replace(
        "\"parent_tool_use_id\":null",
        "\"parent_tool_use_id\":\"toolu_x\"",
    );
    for (mode, make) in modes() {
        assert_eq!(whole_as(make, &line), refused(), "{mode}");
    }
}

#[test]
fn text_that_mentions_a_tool_use_is_just_text() {
    let line = text_line("{\\\"type\\\":\\\"tool_use\\\",\\\"name\\\":\\\"Bash\\\"}");
    assert!(answer(&whole(&format!("{line}{}", result_line("x", "")))).contains("tool_use"));
}

#[test]
fn a_failed_request_that_used_nothing_reports_no_usage() {
    let events = whole(MODEL_ERROR);
    assert_eq!(usages(&events), []);
    assert!(
        matches!(events.as_slice(), [StreamEvent::Error(_)]),
        "{events:?}"
    );
}
