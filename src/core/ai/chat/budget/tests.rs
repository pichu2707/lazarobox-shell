use super::*;
use crate::core::ai::chat::engine::{
    CLI_HISTORY_CHARS, Role, SERVER_HISTORY_CHARS, build_context_tokens,
};

/// A tools reserve in tokens, as a turn with a few tools would pay.
const RESERVE: usize = 1_500;

fn message(role: Role, chars: usize) -> ChatMessage {
    ChatMessage {
        role,
        content: "x".repeat(chars),
    }
}

#[test]
fn a_token_is_about_four_characters_rounded_up() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2);
    // Characters, not bytes.
    assert_eq!(estimate_tokens("ñññññññ"), 2);
}

#[test]
fn an_unknown_window_keeps_the_character_caps() {
    // The system prompt (3 characters) comes off a server's cap.
    assert_eq!(
        budget("llamacpp", None, "sys", "", 0),
        Budget::Chars(SERVER_HISTORY_CHARS - 3)
    );
    assert_eq!(
        budget("claude", None, "sys", "", RESERVE),
        Budget::Chars(CLI_HISTORY_CHARS)
    );
    // The memory block comes off the cap, as before.
    assert_eq!(
        budget("llamacpp", None, "sys", &"m".repeat(2_000), 0),
        Budget::Chars(SERVER_HISTORY_CHARS - 2_000 - 3)
    );
}

#[test]
fn a_zero_window_counts_as_unknown() {
    assert_eq!(
        budget("llamacpp", Some(0), "", "", 0),
        Budget::Chars(SERVER_HISTORY_CHARS)
    );
}

#[test]
fn a_known_window_gives_three_quarters_minus_the_fixed_parts() {
    // The share and the reserve are pinned to their numbers, not to the
    // constants: 8192 * 75% = 6144; system 100 tokens, memory 50, tools 1500.
    assert_eq!(PROMPT_SHARE, 75);
    let got = budget(
        "llamacpp",
        Some(8_192),
        &"s".repeat(400),
        &"m".repeat(200),
        RESERVE,
    );
    assert_eq!(
        got,
        Budget::Tokens {
            room: 4_494,
            window: 8_192
        }
    );
}

#[test]
fn the_tools_reserve_is_only_paid_when_tools_are_offered() {
    let without = budget(
        "llamacpp",
        Some(8_192),
        &"s".repeat(400),
        &"m".repeat(200),
        0,
    );
    assert_eq!(
        without,
        Budget::Tokens {
            room: 5_994,
            window: 8_192
        }
    );
}

#[test]
fn a_window_smaller_than_the_fixed_parts_leaves_no_room_but_does_not_underflow() {
    assert_eq!(
        budget("llamacpp", Some(1_000), &"s".repeat(40_000), "", RESERVE),
        Budget::Tokens {
            room: 0,
            window: 1_000
        }
    );
}

#[test]
fn a_bigger_window_admits_more_history_than_the_cap() {
    let history: Vec<_> = (0..10)
        .map(|i| {
            message(
                if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                5_000,
            )
        })
        .collect();
    let Budget::Tokens { room, .. } = budget("llamacpp", Some(32_768), "", "", RESERVE) else {
        panic!("known window");
    };
    let context = build_context_tokens(&history, room);
    // 10 * 1250 tokens fit in the ~23k room; the 12k char cap kept only 2.
    assert_eq!(context.messages.len(), 10);
    assert!(!context.omitted);
}

#[test]
fn a_small_window_drops_the_oldest_by_tokens() {
    let history: Vec<_> = (0..10)
        .map(|i| {
            message(
                if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                4_000,
            )
        })
        .collect();
    // Room for 2500 tokens: two messages of 1000.
    let context = build_context_tokens(&history, 2_500);
    assert_eq!(context.messages, history[8..]);
    assert!(context.omitted);
}

#[test]
fn the_newest_message_is_kept_whole_whatever_the_room() {
    let history = [message(Role::User, 40_000)];
    let context = build_context_tokens(&history, 0);
    assert_eq!(context.messages.len(), 1);
}

#[test]
fn the_fill_of_a_known_window_counts_every_part() {
    let messages = [message(Role::User, 4_000)];
    let got = fill(
        Some(10_000),
        &"s".repeat(400),
        &"m".repeat(200),
        RESERVE,
        &messages,
    );
    // 1000 history + 100 system + 50 memory + 1500 tools.
    assert_eq!(
        got,
        Some(Fill {
            used: 2_650,
            capacity: 10_000
        })
    );
    assert_eq!(got.map(Fill::percent), Some(26));
}

#[test]
fn the_fill_leaves_the_tools_reserve_out_when_no_tools_are_offered() {
    let messages = [message(Role::User, 4_000)];
    let got = fill(Some(10_000), &"s".repeat(400), "", 0, &messages);
    assert_eq!(
        got,
        Some(Fill {
            used: 1_100,
            capacity: 10_000
        })
    );
}

#[test]
fn there_is_no_fill_without_a_known_window() {
    let messages = [message(Role::User, 6_000)];
    assert_eq!(fill(None, "system", "", RESERVE, &messages), None);
    assert_eq!(fill(Some(0), "system", "", RESERVE, &messages), None);
}

#[test]
fn the_percent_never_passes_one_hundred() {
    let over = Fill {
        used: 9_999,
        capacity: 10,
    };
    assert_eq!(over.percent(), 100);
    assert_eq!(
        Fill {
            used: 0,
            capacity: 0
        }
        .percent(),
        100
    );
}

#[test]
fn an_unknown_window_of_a_server_pays_the_head_out_of_the_character_cap() {
    // 12,000 - 1,000 system - 2,000 memory - 4 * 500 reserve = 7,000.
    let got = budget("ollama", None, &"s".repeat(1_000), &"m".repeat(2_000), 500);
    assert_eq!(got, Budget::Chars(7_000));
    // No tools offered: only the system prompt and the memory come off.
    assert_eq!(
        budget("ollama", None, &"s".repeat(1_000), "", 0),
        Budget::Chars(SERVER_HISTORY_CHARS - 1_000)
    );
}

#[test]
fn a_head_larger_than_the_cap_leaves_the_history_floor_and_does_not_underflow() {
    assert_eq!(
        budget("llamacpp", None, &"s".repeat(20_000), "", RESERVE),
        Budget::Chars(UNKNOWN_WINDOW_FLOOR)
    );
    assert_eq!(UNKNOWN_WINDOW_FLOOR, 4_000);
}

#[test]
fn a_roomy_head_still_leaves_the_cap_minus_the_head() {
    // 12,000 - 1,000 system - 4 * 500 reserve = 9,000, above the floor.
    assert_eq!(
        budget("ollama", None, &"s".repeat(1_000), "", 500),
        Budget::Chars(9_000)
    );
}

#[test]
fn the_floor_applies_after_the_memory_is_taken_off_and_at_its_edge() {
    // Memory alone leaves 12,000 - 9,000 = 3,000 before any head: the floor.
    assert_eq!(
        budget("ollama", None, "sys", &"m".repeat(9_000), 0),
        Budget::Chars(UNKNOWN_WINDOW_FLOOR)
    );
    // Memory beyond the whole cap does not underflow either.
    assert_eq!(
        budget("ollama", None, "", &"m".repeat(20_000), 0),
        Budget::Chars(UNKNOWN_WINDOW_FLOOR)
    );
    // Exactly the floor, and one above it, are kept as computed.
    assert_eq!(
        budget("ollama", None, &"s".repeat(8_000), "", 0),
        Budget::Chars(4_000)
    );
    assert_eq!(
        budget("ollama", None, &"s".repeat(7_999), "", 0),
        Budget::Chars(4_001)
    );
}

#[test]
fn the_floor_is_for_unknown_windows_of_servers_only() {
    assert_eq!(
        budget("claude", None, &"s".repeat(20_000), "", RESERVE),
        Budget::Chars(CLI_HISTORY_CHARS)
    );
    // A known window keeps its own arithmetic: no floor, room may be 0.
    assert_eq!(
        budget("ollama", Some(1_000), &"s".repeat(20_000), "", 0),
        Budget::Tokens {
            room: 0,
            window: 1_000
        }
    );
}

#[test]
fn a_cli_keeps_its_cap_whatever_the_head() {
    assert_eq!(
        budget("claude", None, &"s".repeat(5_000), "", RESERVE),
        Budget::Chars(CLI_HISTORY_CHARS)
    );
}
