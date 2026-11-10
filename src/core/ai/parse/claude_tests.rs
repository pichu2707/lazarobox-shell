//! `claude -p /model --output-format json`: the aliases of its `Available:`
//! line.

use super::{claude_models, claude_paid_turn};

/// The reply of claude 2.1.322 (odd/tasks/cli-model-lists.md, L4): exit 0,
/// `num_turns` 0, `total_cost_usd` 0, and this `result` verbatim.
pub(crate) const REAL_REPLY: &str = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":2700,"num_turns":0,"result":"Current model: `Opus 5.5 (default)` (effort: high)\nUsage: /model <name>. Available: sonnet, opus, haiku, fable, best, sonnet[1m], opus[1m], fable[1m], opusplan, default, or a full model ID.","session_id":"00000000-0000-4000-8000-000000000000","total_cost_usd":0}"#;

/// A successful reply whose `result` is `text`.
fn reply(text: &str) -> String {
    serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "num_turns": 0,
        "result": text,
    })
    .to_string()
}

fn aliases(raw: &str) -> Option<Vec<String>> {
    claude_models(raw).map(|pairs| {
        pairs
            .into_iter()
            .map(|(label, value)| {
                assert_eq!(label, value, "the label is the alias");
                value
            })
            .collect()
    })
}

fn strs(v: &[&str]) -> Option<Vec<String>> {
    Some(v.iter().map(|s| (*s).to_owned()).collect())
}

#[test]
fn the_real_reply_gives_every_alias_in_reply_order_without_default() {
    assert_eq!(
        aliases(REAL_REPLY),
        strs(&[
            "sonnet",
            "opus",
            "haiku",
            "fable",
            "best",
            "sonnet[1m]",
            "opus[1m]",
            "fable[1m]",
            "opusplan",
        ])
    );
}

#[test]
fn the_line_is_read_with_some_tolerance() {
    let cases = [
        // No trailing period.
        (
            "Available: sonnet, opus, or a full model ID",
            &["sonnet", "opus"][..],
        ),
        // `and` instead of `or`, odd spacing.
        (
            "Available:  sonnet ,opus,haiku , and a full model ID.",
            &["sonnet", "opus", "haiku"],
        ),
        // The line first, more lines after it.
        (
            "Available: opus, fable, or a full model ID.\nUsage: /model <name>.",
            &["opus", "fable"],
        ),
        // No `full model ID` tail, a trailing period.
        (
            "Usage: /model <name>. Available: sonnet, opus.",
            &["sonnet", "opus"],
        ),
        // The last two joined by `or` / `and`.
        (
            "Available: sonnet, opus or haiku.",
            &["sonnet", "opus", "haiku"],
        ),
        ("Available: sonnet and opus", &["sonnet", "opus"]),
        // The sentence ends; what follows is not a list.
        (
            "Available: sonnet, opus. Pick one, or two.",
            &["sonnet", "opus"],
        ),
    ];
    for (text, want) in cases {
        assert_eq!(aliases(&reply(text)), strs(want), "{text}");
    }
}

#[test]
fn an_unsafe_or_repeated_alias_is_dropped() {
    let text = "Available: -rf, two words, ópus, a\u{200b}b, sonnet, sonnet, default, haiku, or a full model ID.";
    assert_eq!(aliases(&reply(text)), strs(&["sonnet", "haiku"]));
    let long = format!("Available: {}, opus.", "x".repeat(129));
    assert_eq!(aliases(&reply(&long)), strs(&["opus"]));
}

#[test]
fn a_line_with_no_alias_is_an_empty_list() {
    assert_eq!(
        aliases(&reply("Available: default, or a full model ID.")),
        Some(Vec::new())
    );
}

#[test]
fn anything_that_is_not_a_model_reply_is_none() {
    let not_result = REAL_REPLY.replace(r#""type":"result""#, r#""type":"assistant""#);
    let failed = REAL_REPLY.replace(r#""is_error":false"#, r#""is_error":true"#);
    let subtype = REAL_REPLY.replace(
        r#""subtype":"success""#,
        r#""subtype":"error_during_execution""#,
    );
    // The model answered: the list would be its words, not the CLI's.
    let a_turn = REAL_REPLY.replace(r#""num_turns":0"#, r#""num_turns":1"#);
    let cases = [
        String::new(),
        "not json".to_owned(),
        "[]".to_owned(),
        "{}".to_owned(),
        r#"{"type":"result","is_error":false,"result":7}"#.to_owned(),
        REAL_REPLY[..REAL_REPLY.len() - 5].to_owned(),
        reply("Current model: opus. Usage: /model <name>."),
        not_result,
        failed,
        subtype,
        a_turn,
    ];
    for raw in cases {
        assert_eq!(aliases(&raw), None, "{raw}");
    }
}

#[test]
fn the_list_is_capped() {
    let many: Vec<String> = (0..100).map(|i| format!("m{i}")).collect();
    let text = format!("Available: {}.", many.join(", "));
    assert_eq!(claude_models(&reply(&text)).map(|l| l.len()), Some(64));
}

// Review: a reply without `num_turns` cannot prove no model turn was taken.
#[test]
fn a_reply_without_num_turns_is_none() {
    let bare = REAL_REPLY.replace(r#""num_turns":0,"#, "");
    assert!(!bare.contains("num_turns"));
    assert_eq!(claude_models(&bare), None);
}

#[test]
fn a_reply_that_took_a_turn_or_cost_money_is_a_paid_turn() {
    let a_turn = REAL_REPLY.replace(r#""num_turns":0"#, r#""num_turns":1"#);
    let a_cost = REAL_REPLY.replace(r#""total_cost_usd":0"#, r#""total_cost_usd":0.004"#);
    assert!(claude_paid_turn(&a_turn));
    assert!(claude_paid_turn(&a_cost));
    assert!(!claude_paid_turn(REAL_REPLY));
    assert!(!claude_paid_turn("not json"));
}
