use super::*;

fn msgs(count: usize, len: usize) -> Vec<ChatMessage> {
    (0..count)
        .map(|i| ChatMessage {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: format!("{i:04}{}", "x".repeat(len.saturating_sub(4))),
        })
        .collect()
}

fn total(context: &Context) -> usize {
    context
        .messages
        .iter()
        .map(|m| m.content.chars().count())
        .sum()
}

#[test]
fn a_short_history_is_sent_whole_in_order() {
    let history = msgs(6, 10);
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages, history);
    assert!(!context.omitted);
}

#[test]
fn an_empty_history_sends_nothing() {
    let context = build_context(&[], SERVER_HISTORY_CHARS);
    assert!(context.messages.is_empty());
    assert!(!context.omitted);
}

#[test]
fn sixty_short_messages_keep_the_newest_forty() {
    let history = msgs(60, 10);
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages, history[20..]);
    assert!(context.omitted);
}

#[test]
fn exactly_forty_is_not_an_omission() {
    let history = msgs(40, 10);
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages.len(), 40);
    assert!(!context.omitted);
}

#[test]
fn a_server_keeps_at_most_12000_characters() {
    let history = msgs(10, 5_000);
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages, history[8..]);
    assert!(total(&context) <= 12_000);
    assert!(context.omitted);
}

#[test]
fn a_cli_keeps_at_most_100000_characters() {
    let history = msgs(40, 5_000);
    let context = build_context(&history, CLI_HISTORY_CHARS);
    assert_eq!(context.messages.len(), 20);
    assert_eq!(context.messages, history[20..]);
    assert!(total(&context) <= 100_000);
    assert!(context.omitted);
}

#[test]
fn the_newest_message_is_sent_whole_even_over_the_cap() {
    let mut history = msgs(4, 100);
    history.push(ChatMessage {
        role: Role::User,
        content: "y".repeat(40_000),
    });
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages, history[4..]);
    assert!(context.omitted);
}

#[test]
fn the_cap_counts_characters_not_bytes() {
    let history = vec![
        ChatMessage {
            role: Role::User,
            content: "é".repeat(7_000),
        },
        ChatMessage {
            role: Role::Assistant,
            content: "é".repeat(5_000),
        },
    ];
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages.len(), 2);
    assert!(!context.omitted);
}

#[test]
fn it_stops_at_the_first_message_that_does_not_fit() {
    // A small old message must not jump over a big one that was dropped.
    let history = vec![
        ChatMessage {
            role: Role::User,
            content: "a".into(),
        },
        ChatMessage {
            role: Role::Assistant,
            content: "b".repeat(11_990),
        },
        ChatMessage {
            role: Role::User,
            content: "c".repeat(20),
        },
    ];
    let context = build_context(&history, SERVER_HISTORY_CHARS);
    assert_eq!(context.messages, history[2..]);
    assert!(context.omitted);
}

#[test]
fn clis_get_the_large_cap_and_servers_the_small_one() {
    for cli in ["claude", "codex", "gemini"] {
        assert_eq!(history_chars(cli), CLI_HISTORY_CHARS);
    }
    for server in ["ollama", "lmstudio", "llamacpp", "vllm"] {
        assert_eq!(history_chars(server), SERVER_HISTORY_CHARS);
    }
}
