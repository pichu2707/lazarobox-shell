use super::super::engine::{ChatError, Finish};
use super::*;
use crate::core::ai::jail::NOT_ENFORCED_MARKER;

fn exit(code: i32) -> Option<Exit> {
    Some(Exit { code: Some(code) })
}

fn failed(text: &str) -> StreamEvent {
    StreamEvent::Error(ChatError::Failed(text.into()))
}

fn done() -> Option<StreamEvent> {
    Some(StreamEvent::Done(Finish::Stop))
}

#[test]
fn a_clean_exit_after_an_answer_keeps_the_decoders_final_event() {
    assert_eq!(
        verdict("claude", done(), true, exit(0), ""),
        StreamEvent::Done(Finish::Stop)
    );
    let length = Some(StreamEvent::Done(Finish::Length));
    assert_eq!(
        verdict("claude", length, true, exit(0), ""),
        StreamEvent::Done(Finish::Length)
    );
}

#[test]
fn a_final_event_without_any_answer_text_is_an_error() {
    assert_eq!(
        verdict("claude", done(), false, exit(0), ""),
        failed("claude returned no answer")
    );
}

#[test]
fn a_stop_that_we_caused_is_judged_by_the_final_event_alone() {
    assert_eq!(
        verdict("claude", done(), true, None, ""),
        StreamEvent::Done(Finish::Stop)
    );
}

#[test]
fn a_failing_exit_beats_a_done_event_and_shows_the_last_stderr_line() {
    let stderr = "starting\nsomething broke\n";
    assert_eq!(
        verdict("claude", done(), true, exit(1), stderr),
        failed("something broke")
    );
}

#[test]
fn a_failing_exit_without_stderr_names_the_status_only() {
    assert_eq!(
        verdict("claude", done(), true, exit(3), ""),
        failed("claude exited with status 3")
    );
    let signalled = Some(Exit { code: None });
    assert_eq!(
        verdict("claude", None, false, signalled, ""),
        failed("claude was stopped by a signal")
    );
}

#[test]
fn an_engine_error_is_reported_as_the_decoder_said() {
    let model = failed("There's an issue with the selected model");
    assert_eq!(
        verdict("claude", Some(model.clone()), false, exit(1), "noise\n"),
        model
    );
}

#[test]
fn an_interrupted_stream_blames_the_exit_when_the_process_failed() {
    let cut = Some(StreamEvent::Error(ChatError::Interrupted));
    assert_eq!(
        verdict("claude", cut.clone(), false, exit(2), "boom\n"),
        failed("boom")
    );
    assert_eq!(
        verdict("claude", cut, false, exit(0), ""),
        StreamEvent::Error(ChatError::Interrupted)
    );
}

#[test]
fn no_final_event_at_all_is_an_interruption() {
    assert_eq!(
        verdict("claude", None, false, exit(0), ""),
        StreamEvent::Error(ChatError::Interrupted)
    );
}

#[test]
fn status_125_with_the_marker_line_is_a_jail_error_whatever_else_happened() {
    let stderr = format!("{NOT_ENFORCED_MARKER}: ABI 2 is below the required 3\n");
    let jail = || StreamEvent::Error(ChatError::Jail("ABI 2 is below the required 3".into()));
    let cut = Some(StreamEvent::Error(ChatError::Interrupted));
    assert_eq!(verdict("claude", cut, false, exit(125), &stderr), jail());
    assert_eq!(verdict("claude", done(), true, exit(125), &stderr), jail());
}

#[test]
fn a_cli_that_exits_125_on_its_own_is_an_ordinary_failure() {
    let cut = Some(StreamEvent::Error(ChatError::Interrupted));
    assert_eq!(
        verdict("claude", cut.clone(), false, exit(125), "nope\n"),
        failed("nope")
    );
    // The marker mid-line or with another status proves nothing either.
    let quoted = format!("it said: {NOT_ENFORCED_MARKER}\n");
    assert_eq!(
        verdict("claude", cut.clone(), false, exit(125), &quoted),
        failed(&format!("it said: {NOT_ENFORCED_MARKER}"))
    );
    let marker = format!("{NOT_ENFORCED_MARKER}: x\n");
    assert_eq!(
        verdict("claude", cut, false, exit(1), &marker),
        failed(&format!("{NOT_ENFORCED_MARKER}: x"))
    );
}

#[test]
fn the_last_line_skips_blank_lines_and_trims() {
    assert_eq!(last_line("a\n\nb  \n\n \n"), Some("b".to_owned()));
    assert_eq!(last_line("a\r\nb\r\n"), Some("b".to_owned()));
    assert_eq!(last_line("\n \n"), None);
    assert_eq!(last_line(""), None);
}

#[test]
fn only_the_tail_of_a_long_stderr_counts() {
    let big = format!("{}\nlast words\n", "x".repeat(STDERR_CAP * 2));
    assert_eq!(last_line(&big), Some("last words".to_owned()));
}

#[test]
fn the_last_line_skips_lines_that_are_empty_once_sanitized() {
    assert_eq!(
        last_line("real error\n\u{1b}[2J\n\u{200b}\n"),
        Some("real error".to_owned())
    );
    assert_eq!(last_line("\u{1b}[2J\n"), None);
}

#[test]
fn a_failed_turn_never_shows_a_key_from_stderr() {
    let key = "sk-abcdefghijklmnop";
    let stderr = format!("warming up\n401 Unauthorized: invalid key {key}\n");
    let cut = Some(StreamEvent::Error(ChatError::Interrupted));
    let StreamEvent::Error(ChatError::Failed(text)) =
        verdict("codex", cut, false, exit(1), &stderr)
    else {
        panic!("a failure was expected");
    };
    assert_eq!(text, "401 Unauthorized: invalid key …");
    let bearer = "boom: Authorization: Bearer abc.def\n";
    let StreamEvent::Error(ChatError::Failed(text)) =
        verdict("codex", None, false, exit(1), bearer)
    else {
        panic!("a failure was expected");
    };
    assert!(!text.contains("abc.def"), "{text}");
}

#[test]
fn hidden_characters_cannot_split_a_key_out_of_the_redaction() {
    let cut = || Some(StreamEvent::Error(ChatError::Interrupted));
    let show = |stderr: &str| {
        let StreamEvent::Error(ChatError::Failed(text)) =
            verdict("codex", cut(), false, exit(1), stderr)
        else {
            panic!("a failure was expected");
        };
        text
    };
    assert_eq!(show("bad key sk-ab\u{200b}cdefghijkl\n"), "bad key …");
    assert_eq!(show("bad key sk-ab\u{1b}[31mcdefghijkl\n"), "bad key …");
    assert_eq!(
        show("bad key sk-ab\u{1b}]0;t\u{7}cdefghijkl\n"),
        "bad key …"
    );
    assert_eq!(show("Bearer\u{200b} abc123\n"), "Bearer …");
    for text in [show("x \u{1b}[2Jsk-abcdef\n"), show("y\u{202e}z\n")] {
        assert!(
            !text.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{text:?}"
        );
    }
}
