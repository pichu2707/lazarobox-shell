use super::super::engine::{ChatError, Finish, Usage};
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

fn usage(input: u64, output: u64, cost_micros: Option<u64>) -> StreamEvent {
    StreamEvent::Usage(Usage {
        input,
        output,
        cost_micros,
    })
}

#[test]
fn a_plain_answer_is_one_delta_a_usage_and_a_stop() {
    // 2 fresh + 606 cache-creation + 0 cache-read prompt tokens, $0.004936.
    assert_eq!(
        whole(PLAIN),
        [delta("ok"), usage(608, 4, Some(4936)), done()]
    );
}

#[test]
fn a_result_without_usage_emits_no_usage_event() {
    let text = result_line("fine", "");
    assert_eq!(whole(&text), [delta("fine"), done()]);
}

#[test]
fn a_result_with_usage_but_no_cost_leaves_the_cost_unknown() {
    let text = result_line("x", ",\"usage\":{\"input_tokens\":7,\"output_tokens\":3}");
    assert_eq!(whole(&text), [delta("x"), usage(7, 3, None), done()]);
}

#[test]
fn a_failed_result_emits_no_usage_event() {
    let events = whole(MODEL_ERROR);
    assert!(
        events.iter().all(|e| !matches!(e, StreamEvent::Usage(_))),
        "{events:?}"
    );
}

#[test]
fn a_usage_object_with_no_counts_emits_nothing() {
    let text = result_line("x", ",\"usage\":{},\"total_cost_usd\":0.5");
    assert_eq!(whole(&text), [delta("x"), done()]);
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
fn a_usage_with_only_one_count_emits_nothing_never_a_zero() {
    for counts in [
        "{\"input_tokens\":7}",
        "{\"output_tokens\":3}",
        "{\"cache_read_input_tokens\":9,\"output_tokens\":3}",
    ] {
        let text = result_line("x", &format!(",\"usage\":{counts},\"total_cost_usd\":0.5"));
        assert_eq!(whole(&text), [delta("x"), done()], "{counts}");
    }
}

fn failed_text(events: &[StreamEvent]) -> String {
    match events {
        [StreamEvent::Error(ChatError::Failed(text))] => text.clone(),
        other => panic!("expected one failure, got {other:?}"),
    }
}

/// A `result` line that fails the turn.
fn error_line(result: &str) -> String {
    format!(
        "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"{result}\"}}\n"
    )
}

fn limit_event(status: &str, kind: &str) -> String {
    format!(
        "{{\"type\":\"rate_limit_event\",\"rate_limit_info\":{{\"status\":\"{status}\",\"rateLimitType\":\"{kind}\"}}}}\n"
    )
}

#[test]
fn a_rejected_rate_limit_event_is_kept_as_text_on_the_failure() {
    let mut text = limit_event("rejected", "five_hour");
    text.push_str(&error_line("Claude AI usage limit reached"));
    assert_eq!(
        failed_text(&whole(&text)),
        "Claude AI usage limit reached (limit window: five_hour)"
    );
}

#[test]
fn an_allowed_rate_limit_event_adds_nothing_to_a_failure() {
    let mut text = limit_event("allowed", "five_hour");
    text.push_str(&error_line("boom"));
    assert_eq!(failed_text(&whole(&text)), "boom");
}

#[test]
fn a_rejected_rate_limit_type_is_cleaned_like_any_engine_text() {
    let mut text = limit_event("rejected", "x\\u001b[31m y");
    text.push_str(&error_line("boom"));
    let got = failed_text(&whole(&text));
    assert!(!got.contains('\u{1b}'), "{got:?}");
    assert!(got.starts_with("boom (limit window: "), "{got:?}");
}

#[test]
fn an_unknown_shape_limit_event_does_not_clear_a_rejected_one() {
    let mut text = limit_event("rejected", "five_hour");
    text.push_str("{\"type\":\"rate_limit_event\"}\n");
    text.push_str(&error_line("boom"));
    assert_eq!(failed_text(&whole(&text)), "boom (limit window: five_hour)");
}

#[test]
fn a_rejected_limit_is_kept_when_the_stream_ends_without_a_result() {
    let text = limit_event("rejected", "five_hour");
    let got = failed_text(&whole(&text));
    assert!(got.ends_with("(limit window: five_hour)"), "{got}");
}

#[test]
fn the_limit_suffix_survives_a_long_message_within_the_cap() {
    let mut text = limit_event("rejected", "five_hour");
    text.push_str(&error_line(&"x".repeat(500)));
    let got = failed_text(&whole(&text));
    assert!(got.ends_with("(limit window: five_hour)"), "{got}");
    assert!(
        got.chars().count() <= super::super::stream::ERROR_CAP,
        "{got}"
    );
}

#[test]
fn an_empty_limit_type_reads_as_rejected() {
    let mut text = limit_event("rejected", "");
    text.push_str(&error_line("boom"));
    assert_eq!(failed_text(&whole(&text)), "boom (limit window: rejected)");
}
