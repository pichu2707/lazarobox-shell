# Verification Report: model-context-window

Mode: Strict TDD (cargo test), hybrid store. Date: 2026-11-07. Branch docs/openspec-closeouts-2 (change already merged and released).

## Completeness
Phases 0-7: all tasks checked (0.1, 1.1-1.2, 2.1-2.5, 3.1-3.3, 4.1-4.3, 5.1-5.4, 6.1-6.2, 7.1-7.2). 7.2 manual recorded 2026-11-07 (/api/ps context_length 16384 = sent num_ctx, no reload after 10m20s idle).

## Execution evidence
- `cargo test`: lib 9886 passed, 0 failed; tests/jail.rs 2 failures "jail not enforced: the rwx root is not allowed" (sandbox-environmental, unrelated).
- `cargo clippy --all-targets`: clean. `cargo fmt --check`: clean.

## Spec compliance matrix ([auto] scenarios)
| Scenario | Covering test(s) | Result |
|---|---|---|
| Window from same reply | ai_detect::show_tests::one_show_reply_fills_the_tools_and_the_window, the_traffic_is_one_tags_get_and_one_show_post_per_model | COMPLIANT |
| Bad reply leaves window unknown | show_tests::bad_replies_leave_only_their_own_window_unknown, a_failed_probe_leaves_the_window_unknown, an_oversized_reply_is_unknown | COMPLIANT |
| Digest cache keeps/refreshes | show_tests::an_unchanged_digest_is_not_probed_again_but_a_new_one_is, the_window_is_cached_by_digest_with_the_tools | COMPLIANT |
| Call limit | show_tests::the_call_limit_leaves_the_rest_of_the_windows_unknown | COMPLIANT |
| Failed probe keeps last window | show_tests::a_failed_probe_on_the_next_run_keeps_the_last_known_window, a_new_answer_replaces_the_kept_window_and_an_unlisted_model_drops_it; core::ai::tests::carrying_windows_fills_only_the_listed_models_that_lost_theirs | COMPLIANT |
| llama.cpp unchanged | ai_detect::tests::a_running_llama_server_reports_its_context_window; show_tests::the_other_servers_never_get_a_post | COMPLIANT |
| chat.db untouched | no new migration; existing chat_store user_version/mode tests green; task 7.1 confirms | COMPLIANT (no change-specific test, see SUGGESTION) |
| Known window capped / small model / unknown no num_ctx / zero keep-alive / stable / other engines | adapters::tests::ollama_options_add_num_ctx_and_the_default_keep_alive_and_nothing_else, an_unknown_window_sends_keep_alive_without_options, an_explicit_keep_alive_replaces_the_default_and_zero_is_kept, other_engines_never_carry_the_ollama_options, the_tool_rounds_carry_the_cache_options_too; budget tests | COMPLIANT |
| Oldest pair dropped / nothing to trim / newest alone | window_tests::a_trim_drops_whole_pairs_oldest_first_and_starts_on_a_user_message, a_message_that_fits_or_an_unknown_window_say_nothing; engine::history_tests::the_newest_message_is_sent_whole_even_over_the_cap, a_window_that_would_start_on_an_assistant_starts_on_a_user | COMPLIANT |
| Loopback client scenarios (traffic, body, tools body, context block, paths, host, redirect, non-200, framing, cap, streaming) | pre-existing adapter/http tests incl. tools_off_leaves_the_ollama_body_exactly_as_before, the_body_holds_the_model_the_system_prompt_the_history_and_the_stream_flag | COMPLIANT |
| Context window: reasoning, within cap, over cap (40), CLI cap | engine::history_tests (sixty_short_messages_keep_the_newest_forty, clis_get_the_large_cap_and_servers_the_small_one), budget::tests::a_cli_keeps_its_cap_whatever_the_head | COMPLIANT |
| Unknown window cap/head/floor | budget::tests::an_unknown_window_of_a_server_pays_the_head_out_of_the_character_cap, a_roomy_head_still_leaves_the_cap_minus_the_head, a_head_larger_than_the_cap_leaves_the_history_floor_and_does_not_underflow, the_floor_applies_after_the_memory_is_taken_off_and_at_its_edge | COMPLIANT |
| Known window replaces fixed cap | budget::tests::a_known_window_gives_three_quarters_minus_the_fixed_parts, a_bigger_window_admits_more_history_than_the_cap; window_tests::a_known_window_lets_more_history_through_than_the_cap | COMPLIANT |
| Over-window once per streak | window_tests::an_overflow_streak_says_so_once_and_again_only_after_a_send_that_fits, the_setup_counts_toward_the_over_window_notice | COMPLIANT |
| Unknown Ollama window visible | app ollama_window::an_unknown_window_shows_a_statusline_notice_once_per_model, a_notice_already_showing_is_kept... | COMPLIANT |
| External MCP schema weight | window_tests::the_external_tools_the_runtime_reports_count_in_the_fill, forgetting_the_external_tools_leaves_nothing_to_reserve | COMPLIANT |
| chat-pane marker shown / short / window trim | state_tests::the_marker_is_shown_once_before_the_oldest_sent_message, a_short_conversation_has_no_marker; window_tests::a_small_window_drops_history_the_cap_would_have_kept | COMPLIANT |
| config: default / valid / bad cap falls back / menu never writes | config::tests::ollama_options_default_to_a_16384_cap, ollama_options_accept_a_valid_cap, a_bad_ollama_cap_falls_back_with_the_exact_problem, the_ollama_limits_are_inclusive; config_edit byte-identical tests | COMPLIANT |
| keep-alive: unset 30m / explicit / seconds / each to its server | config::tests::keep_alive_is_a_duration_or_a_number_of_seconds, a_wrong_keep_alive_is_left_out_with_a_problem; adapters::tests (keep_alive, llamacpp_sends_cache_prompt_when_it_is_set, ollama_sends_keep_alive_when_it_is_set) | COMPLIANT |

## Design coherence
Apply notes record anchor drift, W1-W6 review fixes and owner decisions (cap wins over Modelfile num_ctx; floor 4000; MCP/worker budgets deferred). Code and specs are consistent with them (floor and cap range present in specs).

## Issues
CRITICAL (0): none.
WARNING (3):
1. W4/W5 accepted gaps: external MCP schemas of the first turn and orchestra worker prompts stay outside the budget; a follow-up issue was to be opened by the coordinator (verify it exists).
2. No apply-progress artifact; Strict TDD RED/GREEN order cannot be audited (tasks note S5 tests were mutation-checked, not RED-first).
3. Manual 7.2 was run on Ollama only (one model); acceptable per scope.
SUGGESTION (1):
1. Add an explicit test pinning chat.db `user_version` 3 / mode 0600 across a detection run plus a chat turn (scenario "chat.db is untouched") rather than relying on pre-existing store tests. tests/jail.rs sandbox failures are environmental.

## Verdict: PASS WITH WARNINGS
