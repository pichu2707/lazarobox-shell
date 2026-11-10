# Verification Report: read-pane-screen

Mode: Strict TDD (cargo test), hybrid store. Date: 2026-11-07. Branch docs/openspec-closeouts-2 (change already merged and released, v1.14.0).

## Completeness
| Item | Result |
|------|--------|
| Tasks PR1 (1.1-1.8) | 8/8 checked |
| Tasks PR2 (2.1-2.11) | 11/11 checked |
| Manual PR1 / PR2 | Recorded 2026-11-07 (accepted as evidence) |

## Execution evidence
- `cargo test` (CARGO_TARGET_DIR=scratchpad/target-verify): lib 9886 passed, 0 failed, 1 ignored; all other binaries ok; tests/jail.rs 23 passed, 2 failed (`an_rwx_root_runs_what_it_holds_and_keeps_ssh_unreadable`, `the_helper_refuses_a_symlinked_or_a_broad_rwx_root`) with "jail not enforced: the rwx root is not allowed" = sandbox-environmental, unrelated to this change.
- `cargo clippy --all-targets`: clean. `cargo fmt --check`: clean.
- Coverage: not measured (no tool configured).

## Spec compliance matrix (all [auto] scenarios)
| Scenario | Covering test(s) | Result |
|---|---|---|
| agent-tools: Asks in every mode and engine | approval::tests::read_pane_asks_on_every_engine, no_approval_mode_answers_a_screen_ask; host panes_tests::read_pane_asks_on_every_engine_without_a_text; app screen_review::the_ask_opens_the_review_of_the_frozen_screen_in_every_mode | COMPLIANT |
| Approved text is the sent text | screen_review::what_is_sent_is_what_was_frozen_after_the_pane_writes_and_closes; panes_tests::an_approved_screen_is_wrapped_as_data_with_its_line_count | COMPLIANT |
| Pane closes after the freeze | same frozen/closes test | COMPLIANT |
| Secrets redacted | tools::screen::tests::an_assignment_split_across_soft_wraps_is_redacted | COMPLIANT |
| Scrollback tail kept | screen::tests::a_scrollback_tail_keeps_every_asked_row_in_order, over_the_cap_the_tail_is_kept...; pane::tests::screen_span_* | COMPLIANT |
| Large visible screen capped | screen::tests::over_the_cap..., a_single_line_longer_than_the_cap..., exactly_the_cap... | COMPLIANT |
| Targets limited | screen_review::the_chats_own_pane_and_an_unknown_one_are_denied_without_a_prompt | COMPLIANT |
| Not offered to other roles | mcp::tests::a_worker_or_orchestrator_shim_hides_and_refuses_read_pane; panes_tests::the_gate_refuses_read_pane_in_a_worker_or_orchestrator_scope, read_pane_is_refused_without_the_pane_family; tools::tests::read_pane_is_a_read_tool_in_the_default_list_only_with_the_pane_family | COMPLIANT |
| Bad arguments | tools::tests::read_pane_refuses_a_bad_call, ..._a_number_out_of_range_by_name, read_pane_parses_its_two_arguments | COMPLIANT |
| Tool registry (5 scenarios) / Off equals 0.18.0 characterization | pre-existing registry/golden tests, all green (golden regenerated per task 2.9) | COMPLIANT |
| chat-pane: Exact preview, scrollable (TestBackend+insta) | review::screen::tests::the_rows_equal_the_escape_of_the_lines, the_review_has_a_summary_and_one_context_row_per_line; screen_review::* | COMPLIANT (no dedicated insta snapshot found by name, see WARNING) |
| y locked until every line seen | screen_review::y_is_locked_until_every_line_was_seen_then_sends_the_frozen_text; review::screen::tests::y_is_locked_until_every_line_is_seen_and_the_delay_passed | COMPLIANT |
| Deny returns nothing | screen_review::n_and_esc_deny_and_say_so; a_generic_approve_never_answers_a_screen | COMPLIANT |
| Escape sequences neutralised | screen::tests::control_characters_never_reach_the_text; every_line_is_its_own_escape... | COMPLIANT |
| Notices: Denied/timeout | screen_review::n_and_esc_deny_and_say_so, a_read_that_times_out_says_it_was_denied | COMPLIANT |
| Gone pane | the_chats_own_pane_and_an_unknown_one_are_denied_without_a_prompt, a_screen_ask_refused_before_prompting_says_so_in_the_chat | COMPLIANT |
| Empty screen | screen_review::a_blank_screen_is_denied_with_a_notice_and_no_prompt | COMPLIANT |
| Truncated | screen_review::a_cut_text_is_flagged_and_keeps_the_tail; review::screen::tests::a_cut_adds_a_warning... | COMPLIANT |
| Scrollback in full-screen app | screen_review::scrollback_on_a_full_screen_app_says_only_the_visible_screen; screen::tests::scrollback_asked_on_the_alternate_screen_starts_with_the_note | COMPLIANT |
| mcp-server: Only registry / Family / Unknown / read_file / Policy / Events / Tools off | pre-existing, green | COMPLIANT |
| read_pane served and asked | mcp tests + host panes_tests + screen_review (served via same registry; ask path exercised in run mode) | COMPLIANT |
| read_pane not served to a worker | mcp::tests::a_worker_or_orchestrator_shim_hides_and_refuses_read_pane | COMPLIANT |

## Design coherence
Spot-checked against tasks and module layout (builder `tools/screen.rs`, `Pane::screen_span`, `ScreenReview`, `Answer::Screen`, `Effect::ToolScreen`). No deviation found.

## Issues
CRITICAL (0): none.
WARNING (2):
1. Cloud-engine manual variant not run (no authenticated CLI in isolated HOME); engine-independent path covered by unit tests.
2. No apply-progress artifact and no TDD cycle evidence table available (Strict TDD RED/GREEN order cannot be audited retroactively); the "exact preview insta snapshot" scenario has no snapshot test identifiable by name.
SUGGESTION (1):
1. tests/jail.rs shows 2 environmental failures only inside a sandbox; re-run outside it for a fully green record.

## Verdict: PASS WITH WARNINGS
