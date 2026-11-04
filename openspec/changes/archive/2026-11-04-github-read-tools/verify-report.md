# Verify Report: github-read-tools (issue #543)

Tree: origin/main 26732c6d (all chain PRs merged). Mode: hybrid, Strict TDD.

## Result
PASS WITH WARNINGS. 0 CRITICAL, 5 WARNING, 3 SUGGESTION.

## Execution
- `cargo test`: 9387 passed in the lib target (plus integration/doc targets: 8, 15, 25, 3, 2, 12, 1, 1), 0 failed, a few ignored. About 9454 ok lines.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- Snapshots: no failure, so no stale shared-target path issue.
- Not run (disclosed): M.1 to M.7.

## V.1 Compliance matrix (test that proves it; all listed pass)
github-tools
- The three tools: Issue list `render::tests::the_issue_list_shows_number_title_state_and_labels_in_the_wrapper`; Comments only on request `comments_follow_the_flag_and_not_the_json`, `a_call_row_shows_the_comments_option`; PR card `the_pr_card_has_branches_files_and_every_check_shape_and_no_patch`; Bad arguments `github::tests::a_bad_number_is_refused_and_never_clamped_to_issue_one`, `the_schemas_take_only_what_the_validation_allows`, `a_call_holds_no_model_text`.
- Availability: Off by default `no_github_tool_is_listed_unless_the_family_is_in_the_set`, `config::no_toggle_on_is_the_legacy_family_set` (see W3); Toggle on `app tool_approval::the_github_toggle_adds_its_family_and_turning_it_off_removes_it`; Master switch wins `github_grant::a_grant_adds_the_github_family_only_with_tools_on`; Grant counts in budget `github_grant::the_offered_tools_and_the_budget_cannot_drift`; Workers have none `tool_approval::a_chat_turn_keeps_its_tools_through_the_filter` and `Toolset` tests in registry (indirect, see S1); Model cannot enable them `host::github_tests::a_github_call_with_the_family_off_returns_an_unknown_tool_error`, `the_gate_refuses_a_github_call_unless_the_family_is_offered`.
- Repository is origin: Https and ssh `github_run_tests::both_origin_forms_read_the_same_repository_and_a_folder_change_moves_it`, `slug_of_reads_the_forms_of_a_github_origin`; No origin `git::tests::origin::no_origin_is_told_apart_from_no_repository` + host; Not a git folder `a_folder_that_is_not_a_repository_says_so_and_starts_nothing`; Non-GitHub origin `slug_of_refuses_every_other_host_and_every_odd_name` + host; Fork `a_fork_names_its_parent_once_and_the_parent_is_never_queried`; No repository argument `the_schemas_take_only_what_the_validation_allows`; Folder change same test as origin forms.
- Cloud consent: Local engine `a_local_engine_never_asks`; Cloud once per turn `a_cloud_engine_asks_once_per_repository_and_turn_before_gh_starts`; Next turn asks again same test (line ~607); Ollama cloud `an_ollama_cloud_model_asks_for_a_github_read_and_the_start_blocks_its_reads`, `cloud::names_only_tools_stay_free_...`; Declined `a_declined_consent_is_denied_and_the_turn_is_not_asked_again`, `a_decline_beats_a_parallel_call_asked_before_it`; Auto and run do not answer `approval::tests::no_approval_mode_answers_the_github_ask`, `tool_approval::a_github_read_prompts_in_every_mode_and_only_a_lowercase_y_accepts`.
- Fixed read: Injection in arguments `argv_is_the_exact_vector_of_each_call`, `gh_runs_the_fixed_argv_from_the_root_with_a_clean_environment`, `the_environment_filter_drops_...`; UI stays responsive `a_gh_that_never_answers_ends_at_its_bound`, `runner::tests` spawn_capped group (runner/tests.rs:705-766).
- Data, not instructions: Injection / Delimiter forgery `a_hostile_issue_stays_inside_one_wrapper_as_clean_data`, `a_forged_structure_in_...`; Escapes and bidi `github_text_loses_the_invisible_characters_of_every_range` + hostile test; Secrets `a_secret_in_any_field_is_masked`; secret at cap `a_secret_that_straddles_the_cap_is_masked_whole`; hidden char `a_hidden_character_does_not_hide_a_secret`; Output cap `a_long_body_and_long_comments_are_cut_and_the_row_says_so`, `the_total_is_capped_at_the_wrapper_limit_...`; List cut `a_list_cut_at_its_limit_says_more_exist`.
- Visible failures: gh missing `a_missing_gh_is_an_error_that_starts_no_block`; logged out, timeout, not found, other failure `failures_after_the_spawn_are_errors_with_fixed_words_and_still_block` (one table test; fixtures `tests/fixtures/github/*.stderr`).
- The chat grant: next message / toggle untouched `github_grant::a_grant_waits_for_the_next_message_and_leaves_the_toggle_alone`; pane close (new pane has none) covered by per-pane state, no dedicated named test (see S2); folder change `a_folder_change_or_restore_ends_the_grant_and_says_so`; Resume `github_command::a_resumed_conversation_with_the_command_in_it_opens_nothing` and `resume_into` clearing (see S2); Tools off `turning_the_tools_off_and_on_ends_the_grant`; Toggle on then off `a_save_turning_the_toggle_off_ends_every_grant`; Engine change `a_change_of_engine_keeps_the_grant`; Decline `n_and_esc_decline_and_the_dialog_does_not_come_back`; Words in a message `model_text_tool_results_and_stored_messages_cannot_open_or_answer_it`.
- No content in logs: `render::tests::an_unreadable_reply_leaves_its_text_out_of_the_error` and the fixed sentences in `failures_after_the_spawn_...` (SECRET-TOKEN-123).

agent-tools / tool families
- Code toolset, toggle second filter, Github family separate: `registry` tests, `github_tests::the_gate_refuses_...`, `config::each_toggle_adds_exactly_its_family_to_the_legacy_set`.
- Local read free / cloud once: the consent tests above.
- Blocked reads: Run mode asks, Ask and auto ask each time, Local unchanged, cloud differs by tool: `github_block::a_blocked_conversation_asks_for_each_cloud_read_and_search`, `a_local_read_is_unchanged_while_blocked`, `approval::tests::a_blocked_conversation_answers_no_cloud_read_or_search_itself`, `host github_tests::cloud::*`; Resume keeps it `github_block::a_resumed_conversation_opens_blocked_only_with_its_marker`.

pane-execution (auto-run block)
- Block starts `the_first_read_makes_every_command_take_the_prompt`, `a_stale_event_blocks_too`; y does not run it `y_types_the_command_without_enter_and_says_it_was_not_run`; Later turns `github_block_tests::the_block_survives_an_error_result_and_the_next_turn`; Resume stays blocked + newer rows `a_resumed_conversation_opens_blocked_only_with_its_marker`, `a_resumed_conversation_with_unknown_rows_opens_blocked`, `a_newer_rows_block_never_claims_github_text_was_read`; Other conversations `a_new_conversation_is_not_blocked`; Refused calls `a_call_refused_before_the_spawn_sends_no_started_event`; Started calls that fail `failures_after_the_spawn_...`; Nothing lifts it `nothing_lifts_the_block`; Auto and ask `auto_types_without_a_prompt_and_ask_only_types_while_blocked`; Marker `ui badge_tests::blocked_run_marker_80x6` (snapshot).

chat-pane
- Command (6 scenarios + refusals): `github_command::arguments_are_refused_an_escape_is_sent_and_other_text_is_a_message`, `the_command_probes_the_origin_and_is_not_a_message`, `the_command_is_in_neither_the_request_nor_the_store`, `the_command_is_refused_with_a_line_when_it_cannot_apply`, `a_repository_that_is_not_there_is_told_and_opens_nothing`, `the_command_works_during_a_stream_and_applies_next_message`, pending confirm covered inside the refusals test.
- Dialog: `y_grants_with_a_line_and_writes_nothing`, `n_and_esc_...`, `every_other_key_is_swallowed_with_a_notice`, `model_text_tool_results_...`, `a_tool_call_carrying_the_grant_is_denied_and_opens_nothing`, `a_key_before_the_dialog_is_armed_does_not_answer`; 80 columns `approval_tests::github_grant_80x16` snapshot; sanitized name `the_github_dialog_draws_no_control_character`.
- Hint: `github_hint::*` (6 tests) and `guidance_tests::github_hint_80x6` snapshot + `the_github_hint_is_one_muted_ascii_row_at_80_columns`.

config-menu: `approval_tests::the_tools_tab_holds_nine_rows`, `family_tests` (row order, GitHub caution), `menu snapshot_github_row_at_80_columns`.
configuration: `config::the_github_toggle_is_its_own_key`, `a_wrong_typed_toggle_...`, `no_toggle_on_is_the_legacy_family_set`; grant never writes `y_grants_with_a_line_and_writes_nothing`.

### Scenarios with no dedicated test or only indirect proof
1. Consent: Esc and timeout end `denied` for GithubRead: only `n` and cancel are in the host test; Esc/timeout rely on the generic approval path.
2. Auto and run do not answer: the "second write returns `another approval is pending`" half has no GitHub-specific test (generic rule).
3. Consent right after a grant: no named test; grant and consent are tested separately.
4. Grant ends with the pane / Resume has no grant: no test named for them; resume clearing is exercised only indirectly.
5. `TERM=linux`: no code path of its own; asserted by `is_ascii` (disclosed in tasks.md PR 4 deviation 7).
6. Config scenarios "Old file with no key" and "enabled=false with github=true": covered only by the generic default and master-switch tests, not by a GitHub-named test.
7. Menu "nine `j` presses wrap": only row count and order tests.

## V.2
PASS. `git grep chat.db` in tests: every real-path use is `tempdir.join("lazarobox").join("chat.db")` (runtime/chat_store/tests.rs:23, worker/tests.rs:33, runs/tests.rs:14). The `/home/secret/chat.db` literals are trigger text. No network: `gh` is a fake script. Config: tests use `read_text`.

## V.3
PASS with a note. `a_verb_outside_the_boundary_is_an_unknown_tool` (tools/mod.rs:2203) still passes, but its family set does not include `Family::Github`; a GitHub-on pin is missing (W3). "Byte-identical with toggle off" is proven through `no_toggle_on_is_the_legacy_family_set` and the family-list test, not through a golden request compare.

## Unchecked boxes in tasks.md
- 1.21 (PR body Chain Context): bookkeeping, a PR body item, PRs are merged.
- V.1 to V.3: closed by this report; the boxes are to be ticked by the owner.
- M.1 to M.7: owner-only, not blocking, disclosed.

## Findings
CRITICAL: none.

WARNING
- W1 Residual: worker reads and worker grep in a blocked cloud conversation do not ask; find_dir, list_dir, glob, git_log, git_status do not ask (disclosed).
- W2 Shared sanitizer lacks the invisible-character gap fix (GitHub path has its own scrub); grant not pinned to the slug (disclosed).
- W3 No test pins that `git_push` stays unknown with `Family::Github` on, and no golden byte-compare of an adapter request with the toggle off (src/core/ai/tools/mod.rs:2203).
- W4 Ollama cloud naming is unverified (3.2); a cloud model not named `*-cloud` or `:cloud` reads as local and would not ask. Needs M.4.
- W5 Review budget: PR 2 about 625, PR 3 about 616, PR 4 about 664 production lines against the 400 plan; `/github` PR over the line budget (disclosed).

SUGGESTION
- S1 Add a named test that worker and orchestrator tool lists have no GitHub tool with the toggle on.
- S2 Add named tests for "grant ends with the pane" and "resume has no grant".
- S3 Add GithubRead-specific tests for Esc, timeout and `another approval is pending`, and a consent-after-grant test.

TDD slips (disclosed in tasks.md): drafts before tests in PR 2, host module in one pass in PR 3, parse_command/GhRepo/github_hint written with first tests in PR 4. Mutation checks were recorded for each.

next_recommended: sdd-archive (after owner decides on M.1 to M.7).
