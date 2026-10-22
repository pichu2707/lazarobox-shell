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

// ---- a CLI too old for an option we pass (#180) ----

fn shown(name: &str, last: Option<StreamEvent>, stderr: &str) -> String {
    let StreamEvent::Error(ChatError::Failed(text)) = verdict(name, last, false, exit(1), stderr)
    else {
        panic!("a failure was expected for {stderr:?}");
    };
    text
}

const NAMED: &str = "claude does not know the option --permission-prompts: this claude is older \
                     than lazarobox needs; update it (claude update) and try again";
const GENERIC_CLAUDE: &str = "this claude is older than lazarobox needs: it rejected an option; \
                              update it (claude update) and try again";

#[test]
fn one_of_our_own_flags_is_named_when_the_cli_does_not_know_it() {
    for stderr in [
        "error: unknown option '--permission-prompts'\n",
        "  error: unknown option '--permission-prompts'  \n",
        "error: unknown option \"--permission-prompts\"\n",
        "warming up\n\nerror: unknown option '--permission-prompts'\n\n",
        "error: unknown option '--permission-prompts'\n(Did you mean --permission-mode?)\n",
        "error: unknown option '--permission-prompts'\n\n  (Did you mean --permission-mode?)\n\n",
    ] {
        assert_eq!(shown("claude", None, stderr), NAMED, "{stderr:?}");
    }
    // A failing exit after a Done event reads the same.
    assert_eq!(
        verdict(
            "claude",
            done(),
            true,
            exit(1),
            "error: unknown option '--safe-mode'\n"
        ),
        failed(
            "claude does not know the option --safe-mode: this claude is older than lazarobox \
             needs; update it (claude update) and try again"
        )
    );
    assert_eq!(
        shown("codex", None, "error: unknown option '--ignore-rules'\n"),
        "codex does not know the option --ignore-rules: this codex is older than lazarobox \
         needs; update it and try again"
    );
}

#[test]
fn an_option_that_is_not_ours_is_never_echoed() {
    let long = format!("--{}", "a".repeat(4096));
    // Slack-shaped words are assembled here so no token-like literal sits in
    // the source (GitHub push protection flags one).
    let slack = ["xo", "xb"].concat();
    for option in [
        "--sk-abcdefghijklmnop".to_owned(),
        "--0123456789abcdef0123456789abcdef".to_owned(),
        format!("-{slack}-1234-5678-abcdefghijklmnopqrstuv"),
        format!("--{slack}-1234567890-abcdefghijklmnop"),
        "--jbswy3dpehpk3pxp".to_owned(),
        "--password-hunter2-correct-horse".to_owned(),
        "--model=sk-abcdefghijklmnop".to_owned(),
        "--permission-prompts=none".to_owned(),
        "--Permission-Prompts".to_owned(),
        "--permission-prompt".to_owned(),
        long,
    ] {
        let stderr = format!("error: unknown option '{option}'\n(Did you mean --model?)\n");
        let text = shown("claude", None, &stderr);
        assert_eq!(text, GENERIC_CLAUDE, "{option:.40}");
        assert!(
            !text.contains(option.trim_start_matches('-')),
            "{option:.40}"
        );
    }
    // Another CLI's flag is not claude's.
    assert_eq!(
        shown("claude", None, "error: unknown option '--ephemeral'\n"),
        GENERIC_CLAUDE
    );
    assert_eq!(
        shown(
            "codex",
            None,
            "error: unknown option '--sk-abcdefghijklmnop'\n"
        ),
        "this codex is older than lazarobox needs: it rejected an option; update it and try again"
    );
}

#[test]
fn a_different_final_error_wins_over_an_earlier_unknown_option_line() {
    for stderr in [
        "error: unknown option '--permission-prompts'\nconnection refused\n",
        "error: unknown option '--permission-prompts'\nconnection refused\n(Did you mean --x?)\n",
        "error: unknown option '--permission-prompts'\n(Did you mean --x?)\nconnection refused\n",
    ] {
        let text = shown("claude", None, stderr);
        assert!(!text.contains("older than"), "{text}");
        assert!(!text.contains("--permission-prompts"), "{text}");
    }
    assert_eq!(
        shown(
            "claude",
            None,
            "error: unknown option '--permission-prompts'\nconnection refused\n"
        ),
        "connection refused"
    );
}

#[test]
fn prose_that_mentions_an_unknown_option_is_not_mapped() {
    for stderr in [
        "Unknown option in config file\n",
        "warning: unknown option '--permission-prompts' ignored\n",
        "error: unknown option '--permission-prompts' in settings.json\n",
        "Error: unknown option --permission-prompts\n",
        "unknown option '--permission-prompts'\n",
        "error: unknown option '--permission-prompts\n",
        "error: unknown option ''--permission-prompts''\n",
    ] {
        let text = shown("claude", None, stderr);
        assert!(!text.contains("older than"), "{stderr:?} -> {text}");
    }
    assert_eq!(
        shown("claude", None, "Unknown option in config file\n"),
        "Unknown option in config file"
    );
}

#[test]
fn a_secret_in_the_option_slot_never_reaches_the_screen() {
    let stderr = "error: unknown option '--token=sk-abcdefghijklmnop'\n";
    assert_eq!(shown("claude", None, stderr), GENERIC_CLAUDE);
    // Hidden characters cannot make the line look like one of ours.
    let stderr = "error: unknown option '--permission\u{200b}-prompts'\n";
    assert_eq!(shown("claude", None, stderr), GENERIC_CLAUDE);
}

#[test]
fn other_failures_keep_the_plain_stderr_line() {
    assert_eq!(
        shown("claude", None, "error: unknown command 'x'\n"),
        "error: unknown command 'x'"
    );
}

// ---- the list of our own flags cannot drift from the argv builders ----

fn flags_of<'a>(args: impl IntoIterator<Item = &'a String>) -> std::collections::BTreeSet<String> {
    args.into_iter()
        .filter(|a| a.starts_with('-') && a.len() > 1)
        .map(|a| a.split('=').next().unwrap_or(a).to_owned())
        .collect()
}

#[test]
fn the_known_flags_are_exactly_what_the_argv_builders_pass() {
    use crate::core::ai::chat::adapters::{Invocation, adapter_for};
    use crate::core::ai::chat::engine::{ChatMessage, ChatRequest, Role, Tools};
    use crate::core::ai::tools::wiring::{McpWire, Secret, Unwire, claude, gemini};
    use std::path::Path;

    let wire = || McpWire {
        socket: "/run/user/1000/lazarobox/s-1-1.sock".into(),
        token: Secret::new("0123456789abcdef0123456789abcdef"),
        unwire: Unwire::none(),
    };
    for engine in ["claude", "codex", "gemini"] {
        let request = ChatRequest {
            families: crate::core::ai::tools::registry::FamilySet::LEGACY,
            engine: engine.into(),
            model: Some("some-model".into()),
            system: "be brief".into(),
            memory: None,
            tools: Tools::Off,
            messages: vec![ChatMessage {
                role: Role::User,
                content: "hi".into(),
            }],
        };
        let Invocation::Process(job) = adapter_for(engine).unwrap().build(&request) else {
            panic!("{engine}: a process was expected");
        };
        let mut emitted = flags_of(&job.args);
        emitted.extend(job.tmp_files.iter().map(|f| f.flag.to_owned()));
        // What the runtime adds to a turn with tools on.
        let shim = Path::new("/usr/bin/lazarobox");
        let extra = match engine {
            "claude" => claude(shim, Path::new("/tmp/mcp.json"), &wire()).map(|w| w.args),
            "gemini" => gemini(shim, Path::new("/tmp/p.toml"), &wire()).map(|w| w.args),
            _ => None,
        };
        emitted.extend(flags_of(extra.iter().flatten()));
        let known: std::collections::BTreeSet<String> = known_flags(engine)
            .iter()
            .map(|f| (*f).to_owned())
            .collect();
        assert_eq!(known, emitted, "{engine}");
        for flag in known_flags(engine) {
            assert!(
                flag.starts_with('-') && !flag.chars().any(|c| c.is_control() || c == ' '),
                "{flag:?}"
            );
        }
    }
    assert!(known_flags("unknown-cli").is_empty());
}
