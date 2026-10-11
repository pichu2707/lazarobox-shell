# Verification Report: ai-engines (Phase 1: detection and selection)

**Change**: ai-engines
**Verified tip**: origin/feat/ai-engines @ 3eb1879 (PRs #70-#89), detached worktree
**Mode**: Strict TDD (cargo test), artifact store hybrid
**Date**: 2026-10-11

## Verdict: PASS WITH WARNINGS

0 CRITICAL, 5 WARNING, 4 SUGGESTION. Nothing blocks archive.

## Completeness

| Metric | Value |
|---|---|
| Implementation tasks (AE1.1-AE4.14) | 62 / 62 checked |
| Verify-prep tasks VP.1-VP.4 [manual] | done by the user (PASS), still `[ ]` in tasks.md |
| VP.5 (this verification) / VP.6 (archive Purpose edits) | open by nature (see W1) |
| Spec scenarios (4 deltas) | 27 + 30 + 14 + 14 = 85 total |
| Covered by a passing automated test | 81 |
| Covered by a user manual check (PASS) | 4 (Real machine, Docs mention limits, AI menu in Kitty, Positions in Kitty) |
| Uncovered | 0 (see note on one scenario, S1) |

## Build and test evidence

| Command | Result |
|---|---|
| `cargo test` | exit 0. 931 unit tests passed, 1 integration test (`tests/version_stdin.rs`) passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| `cargo fmt --check` | exit 0 |
| Coverage tool | not run (not configured as a gate) |

Tests never touch the real ports 11434/1234/8080/8000 (ephemeral `127.0.0.1:0` listeners and `detect_with`).

Strict TDD limitation: the RED-before-GREEN ordering cannot be proven from the merged tip. tasks.md records RED/GREEN pairs per slice and the apply-progress was partially reconstructed once, so the TDD cycle is taken from tasks.md, not re-derived. Test quality was judged from names and the review observations (2933, 2937, 2939, 2941), which include mutation-survivor lists; the follow-up PRs (#83-#84, #89) closed those gaps with named tests (for example `sanitize_covers_csi_intermediate_and_final_byte_edges`, `model_cycles_backward_through_three_choices`, `duplicate_model_names_do_not_trap_cycling`, `version_retries_a_spawn_that_hits_a_busy_text_file`). Test bodies were not re-read one by one; compliance below is by covering test name plus a passing run.

## Spec compliance matrix

Status COMPLIANT = a covering test passed at runtime. [M] = manual check reported PASS by the user on 2026-10-11 (binary 21f04e4; #88/#89 add only tests/refactor).

### ai-engine-detection (27)

| Scenario | Covering test(s) | Status |
|---|---|---|
| Catalog integrity | `core::ai::tests::catalog_has_the_eight_engines_in_order`, `catalog_servers_are_local_and_clis_have_no_endpoint` | COMPLIANT |
| Server matrix | `core::ai::tests::server_matrix`, `llamacpp_status_matrix` | COMPLIANT |
| Display text | `core::ai::tests::status_text_formats_every_status` | COMPLIANT |
| Valid Ollama | `parse::tests::ollama_lists_models_in_order`, `ai_detect::tests::http_get_returns_status_and_body`, `detect_with_probes_every_engine_and_nothing_else` | COMPLIANT |
| Valid OpenAI-style list | `parse::tests::lmstudio_checks_shape_only`, `core::ai::tests::lmstudio_valid_reply_is_available_with_exact_models` | COMPLIANT |
| Valid llama.cpp and vLLM | `parse::tests::marker_is_required_for_owned_servers`, `core::ai::tests::owned_servers_need_their_marker`, `detect_with_probes_every_engine_and_nothing_else` | COMPLIANT |
| Foreign OpenAI-compatible service | `core::ai::tests::owned_servers_need_their_marker`, `llamacpp_status_matrix`, `parse::tests::openai_rejects_foreign_bodies` | COMPLIANT |
| Foreign service answers HTML | `parse::tests::openai_rejects_foreign_bodies`, `core::ai::tests::llamacpp_status_matrix` | COMPLIANT |
| Foreign service answers other JSON | `core::ai::tests::foreign_json_with_a_binary_is_not_running`, `parse::tests::ollama_rejects_foreign_bodies`, `openai_rejects_foreign_bodies` | COMPLIANT |
| Non-200 or oversize | `core::ai::tests::non_200_and_oversize_replies_are_invalid`, `http::tests::oversize_bodies_give_none`, `redirects_are_returned_not_followed`, `ai_detect::tests::http_get_rejects_a_reply_over_the_cap`, `http_get_returns_a_redirect_without_following_it` | COMPLIANT |
| Partial entries ignored | `parse::tests::ollama_ignores_partial_entries` | COMPLIANT |
| Port closed | `core::ai::tests::closed_port_follows_the_binary_rule`, `ai_detect::tests::http_get_fails_fast_on_a_closed_port` | COMPLIANT |
| Found with version | `core::ai::tests::cli_on_path_is_available_with_version_and_default_model`, `parse::tests::version_is_the_first_non_empty_line`, `ai_detect::tests::version_returns_stdout`, `detect_with_probes_every_engine_and_nothing_else` | COMPLIANT |
| Absent or alias | `core::ai::tests::cli_off_path_is_not_installed`, `ai_detect::tests::find_on_path_finds_an_executable_file` (and the negatives `..._ignores_directories`, `..._ignores_files_without_the_exec_bit`) | COMPLIANT |
| Version hangs | `ai_detect::tests::version_kills_a_hanging_child_after_the_timeout`, `version_kills_the_whole_process_group_on_timeout`, `detect_with_probes_every_engine_and_nothing_else` | COMPLIANT |
| Version fails | `ai_detect::tests::version_gives_none_on_a_non_zero_exit`, `parse::tests::empty_output_has_no_version`, `core::ai::tests::cli_without_version_output_is_still_available` | COMPLIANT |
| Relative PATH entry skipped | `ai_detect::tests::find_on_path_skips_empty_and_relative_entries` | COMPLIANT |
| Hanging server | `ai_detect::tests::http_get_gives_up_on_a_silent_server`, `detect_with_answers_once_when_a_server_hangs` | COMPLIANT |
| UI stays responsive | `ai_detect::tests::spawn_detection_of_returns_without_waiting`, `app::app_tests::typing_is_not_held_back_while_detecting` | COMPLIANT |
| Three triggers | `app::app_tests::detection_is_single_flight`, `opening_the_menu_requests_a_detection_unless_one_is_in_flight`, `r_on_an_ai_row_requests_exactly_one_detection`, `runtime::tests::startup_requests_exactly_one_engine_detection`, `detect_engines_calls_the_injected_detector_once` | COMPLIANT |
| Trigger while a run is in flight is a no-op | `app::app_tests::detection_is_single_flight`, `opening_the_menu_requests_a_detection_unless_one_is_in_flight` | COMPLIANT |
| Completion re-arms the triggers | `app::app_tests::a_result_stores_the_detection_and_rearms_the_trigger` | COMPLIANT |
| Engines show detecting until the result | `app::app_tests::engines_read_detecting_until_the_whole_result_arrives`, `runtime::tests::the_first_menu_open_shows_the_saved_engine_as_checking` | COMPLIANT |
| Failed run still answers | `app::app_tests::a_failed_run_clears_the_flag_and_reads_unknown`, `ai_detect::tests::spawn_detection_of_answers_with_an_empty_detection_on_panic`, `core::ai::tests::status_text_covers_pending_failed_and_foreign_ids` | COMPLIANT |
| Traffic audit | `ai_detect::tests::http_get_sends_exactly_the_documented_request`, `detect_with_probes_every_engine_and_nothing_else` | COMPLIANT |
| Real machine [manual] | VP.1 | COMPLIANT [M] |
| Docs mention limits [manual] | README "Menu de IA" section names both limits (checked in the worktree: lines 69-72); VP.3 | COMPLIANT [M] |

### config-menu (30)

| Scenario | Covering test(s) | Status |
|---|---|---|
| AI rows rendered | `ui::components::menu::tests::snapshot_ai_popup_at_the_wanted_size`, `ai_status_cells_have_the_style_of_the_mouse_row`, `core::menu::ai_tests::the_ai_section_is_three_fixed_rows_with_a_muted_status` | COMPLIANT |
| Status follows the draft engine | `core::menu::ai_tests::status_text_follows_the_draft_engine`, `status_moves_with_the_engine_row` | COMPLIANT |
| Cycle engines | `core::menu::ai_tests::engine_cycles_none_then_available_engines_in_catalog_order` | COMPLIANT |
| Engine change resets model | `core::menu::ai_tests::changing_the_engine_resets_the_model_to_its_first_model` | COMPLIANT |
| Engine none clears model | `core::menu::ai_tests::engine_none_clears_engine_and_model` | COMPLIANT |
| Server without models | `core::menu::ai_tests::an_engine_with_no_models_shows_none_and_cannot_cycle` | COMPLIANT |
| Saved model no longer listed | `core::menu::ai_tests::a_saved_model_that_is_gone_is_shown_and_kept`, `a_saved_model_that_is_gone_is_the_last_choice_while_open`, `duplicate_model_names_do_not_trap_cycling` | COMPLIANT |
| Saved engine not available | `core::menu::ai_tests::a_saved_engine_that_is_not_available_is_still_offered` | COMPLIANT |
| Pending detection | `core::menu::ai_tests::a_saved_engine_is_checking_until_the_first_result` | COMPLIANT |
| No engines at all | `core::menu::ai_tests::with_no_engines_and_nothing_saved_the_row_is_none_and_fixed` | COMPLIANT |
| Result arrives while open | `core::menu::ai_tests::a_result_updates_in_place_and_keeps_selection_and_draft`, `app::app_tests::a_result_updates_the_open_menu_in_place_and_keeps_the_draft` | COMPLIANT |
| Engine disappears while open | `core::menu::ai_tests::a_vanished_engine_stays_in_the_draft_as_not_detected`, `a_draft_engine_that_vanishes_stays_a_choice_until_left` | COMPLIANT |
| Refresh on AI row | `core::menu::ai_tests::r_on_the_engine_or_model_row_asks_for_a_refresh`, `app::app_tests::r_on_an_ai_row_requests_exactly_one_detection` | COMPLIANT |
| Refresh elsewhere or repeated | `core::menu::ai_tests::r_elsewhere_repeated_or_shifted_gives_no_command` | COMPLIANT |
| Esc reverts AI | `core::menu::ai_tests::reopening_shows_the_saved_values_again`, `app::app_tests::config_saved_copies_the_ai_draft_and_esc_reverts_it` | COMPLIANT |
| Save closes with AI values | `app::app_tests::config_saved_copies_the_ai_draft_and_esc_reverts_it`, `enter_after_choosing_a_cli_engine_and_refreshing_asks_to_save_it`, `ai_edits_never_resize_a_pane` | COMPLIANT |
| Unchanged AI draft | `core::menu::ai_tests::an_unchanged_ai_draft_adds_no_edits`, `app::app_tests::an_unchanged_ai_draft_closes_without_a_save` | COMPLIANT |
| AI menu in Kitty [manual] | VP.2 (PASS on re-check, see W2) | COMPLIANT [M] |
| Descriptor integrity | `core::menu::tests::descriptor_table_is_well_formed`, `core::menu::ai_tests::the_ai_descriptors_are_well_formed`, `every_choice_get_returns_what_set_stored` | COMPLIANT |
| Settings rows | `ui::components::menu::tests::snapshot_settings_popup_at_the_wanted_size`, `snapshot_ai_popup_at_the_wanted_size`, `ui::tests::menu_popup_is_drawn_over_the_panes_after_the_bars`, `core::menu::tests::rows_list_title_then_settings_then_disabled_mouse` | COMPLIANT |
| Move down and up | `core::menu::tests::move_keys_wrap_in_both_directions`, `core::menu::ai_tests::navigation_visits_settings_engine_and_model_and_skips_status_and_mouse` | COMPLIANT |
| Wrap | same two tests | COMPLIANT |
| Disabled row is skipped | `core::menu::ai_tests::navigation_visits_settings_engine_and_model_and_skips_status_and_mouse` | COMPLIANT |
| Status row is skipped and navigation wraps | same test | COMPLIANT |
| Repeat moves | `core::menu::tests::repeat_moves_like_a_press`, `core::menu::ai_tests::repeat_moves_into_the_ai_rows_like_a_press` | COMPLIANT |
| Unknown key | `core::menu::tests::release_ctrl_space_and_unknown_keys_are_swallowed`, `app::app_tests::unlisted_keys_and_release_emit_nothing_and_stay_in_menu`, `other_keys_and_ctrl_space_are_swallowed_and_resize_stays` | COMPLIANT |
| Paste ignored | `app::app_tests::paste_in_menu_writes_nothing` | COMPLIANT |
| Footer hint | `ui::components::menu::tests::snapshot_settings_popup_at_the_wanted_size`, `the_refresh_hint_shows_only_on_the_ai_rows` | COMPLIANT |
| Footer hint on AI row | `ui::components::menu::tests::the_refresh_hint_shows_only_on_the_ai_rows`, `snapshot_ai_popup_at_the_wanted_size` | COMPLIANT |
| Error replaces hint | `ui::components::menu::tests::snapshot_error_replaces_the_hint_and_is_clipped`, `app::app_tests::the_next_key_clears_the_error_and_a_retry_can_succeed`, `core::menu::tests::the_error_clears_on_the_next_press_which_then_acts` | COMPLIANT |

### configuration (14)

| Scenario | Covering test(s) | Status |
|---|---|---|
| Defaults | `core::config::tests::defaults_are_statusline_bottom_tabbar_top_no_mouse`, `an_empty_file_has_unset_ai`, `core::ai::config::tests::missing_section_and_keys_are_unset` | COMPLIANT |
| Each value | `core::config::tests::each_position_is_read`, `a_partial_file_keeps_the_other_defaults` | COMPLIANT |
| Unknown keys ignored | `core::config::tests::unknown_keys_and_tables_are_ignored` | COMPLIANT |
| AI keys parsed | `core::ai::config::tests::engine_and_model_are_read`, `core::config::tests::load_with_ai_returns_the_saved_choice_without_a_notice` | COMPLIANT |
| Unknown engine id kept | `core::ai::config::tests::an_unknown_engine_id_is_kept_verbatim` | COMPLIANT |
| Empty string is unset | `core::ai::config::tests::an_empty_string_is_unset` | COMPLIANT |
| Missing file | `core::config::tests::a_missing_file_is_silent_defaults` | COMPLIANT |
| Invalid value falls back per key | `core::config::tests::an_invalid_value_falls_back_for_that_key_only`, `an_invalid_key_keeps_the_rest_and_the_notice_names_it` | COMPLIANT |
| Several invalid keys | `core::config::tests::several_invalid_keys_report_the_count_then_the_first`, `ai_problems_count_with_the_others` | COMPLIANT |
| Unparseable file | `core::config::tests::unparseable_toml_gives_all_defaults_and_a_notice`, `only_unparseable_toml_is_an_error` | COMPLIANT |
| Mouse reserved | `core::config::tests::mouse_true_is_accepted_with_a_notice`, `a_problem_notice_wins_over_the_mouse_notice` | COMPLIANT |
| Invalid AI types fall back per key | `core::ai::config::tests::a_non_string_key_falls_back_alone`, `core::config::tests::a_bad_ai_key_keeps_the_rest_and_the_notice_names_it`, `a_valid_ai_table_survives_another_invalid_key` | COMPLIANT |
| Non-table ai | `core::ai::config::tests::a_non_table_ai_is_one_problem`, `core::config::tests::a_non_table_ai_gives_one_notice` | COMPLIANT |
| Positions in Kitty [manual] | VP.4 | COMPLIANT [M] |

### config-persistence (14)

| Scenario | Covering test(s) | Status |
|---|---|---|
| ai is a scalar | `core::config_edit::tests::collisions_with_the_ai_keys_are_refused`, `runtime::config_write::tests::ai_collisions_map_to_their_message_and_stay_byte_identical` | COMPLIANT |
| engine holds a table | same two tests, plus `refuses_an_owned_key_that_holds_a_table` / `..._an_array_of_tables` | COMPLIANT |
| Scalar of wrong type replaced | `core::config_edit::tests::a_scalar_of_the_wrong_type_is_replaced_and_an_ignored_collision_is_kept` | COMPLIANT |
| Collision ignored when AI unchanged | same test, `a_position_only_save_leaves_the_ai_table_alone` | COMPLIANT |
| Comments and order preserved | `core::config_edit::tests::replaces_only_the_value_and_keeps_every_other_byte`, `existing_ai_keys_are_updated_in_place` | COMPLIANT |
| Missing keys are created | `core::config_edit::tests::inserts_a_missing_table_and_leaves_the_rest_untouched`, `inserts_a_missing_key_into_an_existing_table` | COMPLIANT |
| Mouse untouched | `core::config_edit::tests::mouse_is_kept_when_present_and_never_added` | COMPLIANT |
| Saved file parses back | `core::config_edit::tests::saved_text_parses_back_to_the_draft_positions`, `special_characters_and_the_draft_round_trip_without_a_notice`, `runtime::tests::saving_an_ai_engine_with_no_config_file_writes_it` | COMPLIANT |
| AI table created | `core::config_edit::tests::a_missing_ai_table_is_appended_after_the_existing_bytes` | COMPLIANT |
| Existing AI keys updated in place | `core::config_edit::tests::existing_ai_keys_are_updated_in_place` | COMPLIANT |
| No engine saved as empty string | `core::config_edit::tests::no_engine_is_written_as_empty_strings_and_reads_back_unset`, `core::menu::ai_tests::none_is_written_as_empty_strings` | COMPLIANT |
| Special characters escaped | `core::config_edit::tests::special_characters_and_the_draft_round_trip_without_a_notice` | COMPLIANT |
| AI untouched when unchanged | `core::config_edit::tests::a_position_only_save_leaves_the_ai_table_alone`, `core::menu::ai_tests::an_unchanged_ai_draft_adds_no_edits` | COMPLIANT |
| Undetected saved engine survives | `core::menu::ai_tests::a_saved_engine_that_is_not_available_is_still_offered`, `a_changed_ai_draft_writes_both_keys_after_the_settings`, `a_position_only_save_leaves_the_ai_table_alone` | COMPLIANT |

Note S1: for "Undetected saved engine survives" the exact combination (saved `codex` not installed, position change only, file keeps `engine = "codex"`) is covered by two tests that together imply it (menu adds no AI edits while the draft equals the original; persistence leaves `[ai]` alone), not by one end-to-end test. Behaviour holds; a single app-level test would pin it.

## Tasks state

tasks.md: AE1.1-AE4.14 all `[x]` and consistent with the code at the tip (all files named in the tasks exist; the stubs/RED/GREEN steps are history and not re-checkable). VP.1-VP.6 are `[ ]`. VP.1-VP.4 were verified by the user on 2026-10-11 and should be ticked in tasks.md; VP.5 is this verification; VP.6 is the archive-time edit below.

## Design coherence (AE-1..AE-12)

| ADR | Code | Result |
|---|---|---|
| AE-1 layout, `Probe`, `reduce` | `core/ai/{mod,parse,http,config}.rs`, pure | Follows (plus an unlisted `core/ai/menu.rs`, see D2) |
| AE-2 status rules | `reduce`; server binaries only looked up, never run | Follows |
| AE-3 shape checks, owner markers, no `/health` | `parse.rs`, tests above | Follows. Extra: model names sanitized, capped at 128 chars, list capped at 256, names that sanitize to empty dropped (D4) |
| AE-4 hand-written client, 1 MiB cap, HTTP/1.0 request | `ai_detect::http_get`, exact request pinned by test | Follows. `http::parse_response` is stricter than designed (D4) |
| AE-5 probes and timeouts | `HTTP_TIMEOUT` 800 ms, `VERSION_TIMEOUT` 2 s, PATH once | Follows with a stronger kill (D3) |
| AE-6 injected detector, channel, single flight | `Panes::with_detector`, `ai_rx` arm, `request_detection`, `spawn_detection_of` | Follows. `startup_effects(app)` is a testable helper (D6) |
| AE-7 `AiConfig`, `load_with_ai` | `Config` still `Copy`; `load_config` returns `(Config, AiConfig, Option<String>)` | Follows |
| AE-8 `Cow<'static, str>` for `ConfigEdit.value` | in place | Follows |
| AE-9 `AiMenu`, 3 fixed rows | `core/ai/menu.rs` | Follows, but `with_ai` takes two arguments (D1) |
| AE-10 choices, labels, `Refresh` | tested | Follows |
| AE-11 `edits()` and save copy | tested | Follows |
| AE-12 geometry | `rows + 4`, `fits(area, rows)`, `r refresh` hint | Follows, but width is 62 with the AI section (D5) |

Deviations:

- D1 `MenuState::with_ai(ai, engines)` is two-argument, not `(ai, engines, detecting)`. Pending state is derived from `engines: None`. Harmless; the `detecting` flag in `AiMenu` was dropped.
- D2 `src/core/ai/menu.rs` (AiMenu logic) is not in the design's file table; `core/menu/ai_tests.rs` also added.
- D3 `version()` goes beyond AE-5 `kill_on_drop`: the child runs in its own process group (`process_group(0)`), the whole group is killed (`killpg`), and on the normal-exit path `waitid(WNOWAIT)` runs before the kill and the reap, which closes review finding 2941(1). Includes an ETXTBSY retry (max 4) and an output cap. Backed by `version_kills_the_group_when_the_future_is_dropped`, `version_waits_for_the_child_not_for_the_pipe`, `version_caps_the_output_it_reads`. Uses the already-declared `libc` crate (no new dependency, confirmed against origin/main Cargo.toml).
- D4 Hardening not in the spec or design: control/ANSI/OSC/bidi/format character stripping, version cap of 40 chars re-trimmed after cap, model-name and list caps, strict HTTP framing (ambiguous framing, `Transfer-Encoding` rules where chunked must be last, strict chunk terminators, only HTTP/1.x status lines, non-UTF-8 headers tolerated). Responds to reviews 2933/2937/2941. Tested. Not described in the specs (see S2).
- D5 AI popup is 62 wide (`WANT_WIDTH_AI`) and 12 rows high; the Settings-only popup stays 51x8 so its snapshot is unchanged. `fits(area, rows)` is `width >= 24 && height >= rows + 3`. The design said nothing about the width.
- D6 Startup uses `startup_effects(&mut App)` and `app_from_config`/`load_config` helpers in `runtime.rs`, which enabled `startup_requests_exactly_one_engine_detection`, `startup_carries_the_saved_ai_choice_into_the_app`, `the_first_menu_open_shows_the_saved_engine_as_checking`.
- D7 `tests/version_stdin.rs` is a separate integration binary because it swaps fd 0 process-wide (review 2941(4)). Positive deviation.

## MODIFIED blocks vs main specs

I compared each MODIFIED block with its main requirement scenario by scenario.

| Capability / Requirement | Main scenarios | Preserved in delta | Result |
|---|---|---|---|
| config-menu / Sections and items are data | Descriptor integrity, Settings rows | both (extended) | Complete |
| config-menu / Navigation | Move, Wrap, Disabled row skipped, Repeat | all 4 + new Status skipped | Complete |
| config-menu / Input is swallowed | Unknown key, Paste ignored | both | Complete |
| config-menu / Footer | Footer hint, Error replaces hint | both + AI hint | Complete |
| configuration / Schema and defaults | Defaults, Each value, Unknown keys ignored | all 3 + 3 AI | Complete |
| configuration / Fallback and startup notice | Missing file, Invalid value, Several invalid, Unparseable, Mouse reserved, Positions in Kitty | all 6 + 2 AI | Complete |
| config-persistence / Owned keys only | Comments and order, Missing keys, Mouse untouched, Saved file parses back | all 4 + 7 AI | Complete |

Untouched main requirements that interact with the change and need no edit: config-menu "Tiny terminals" (threshold is "decided in design"), "Open and close", "Live preview and revert", "Events while open"; config-persistence "Save on Enter", "Refuse on errors".

## Archive-time manual edits (VP.6)

1. Main `openspec/specs/config-menu/spec.md` Purpose line: replace with the text given in the config-menu delta header (two sections, Settings and AI with the read-only Status row).
2. Main `openspec/specs/configuration/spec.md` Purpose line: replace with the text in the configuration delta header (also records the default AI engine and model).
3. `config-persistence`: no Purpose edit needed.
4. `ai-engine-detection` is a new capability and is copied as a whole.

## Issues

### CRITICAL
None.

### WARNING

- W1 tasks.md still shows VP.1-VP.4 as `[ ]` although the user completed them with PASS on 2026-10-11. The record is out of sync with reality; tick them (VP.5 after this report, VP.6 at archive). Not a code issue.
- W2 UX silent no-op before the first detection answer (obs 2952, deferred). While `engines` is `None` the Engine row cannot leave `none`/the saved id, so Enter with an unchanged draft closes the menu without saving or saying why. This matches the letter of the spec ("Pending detection", "Unchanged AI draft") and is pinned by `enter_before_any_detection_has_nothing_to_save`, but it caused the user's first VP.2 failure. Also the Engine choices collapse to `none` plus the saved id until the first answer. Planned for the next change (AI selector); it should be tracked, not forgotten.
- W3 The existing snapshot `menu_popup_is_drawn_over_the_panes_after_the_bars` changed (test area 60x12 -> 64x16, popup now includes the AI section). AE4.2 said existing Settings snapshots stay byte-identical. Cause: `open_menu` now always attaches the AI section. The Settings-only snapshots (`snapshot_settings_popup_at_the_wanted_size`, etc.) are unchanged. Intentional and covered by "Settings rows", but it is a deviation from the task text.
- W4 Design and spec lag the code: D1-D7 (two-argument `with_ai`, `core/ai/menu.rs`, process-group kill with `waitid`, 62-wide popup, sanitize and caps, strict HTTP framing, startup helpers). `design.md` AE-5 still says `kill_on_drop` only, AE-9 shows the three-argument `with_ai`, and AE-12 omits the width. The behaviour is tested and better than designed, but the archived design would be inaccurate.
- W5 Unspecified behaviours with user-visible effect are not in any spec: version capped at 40 chars, model-name caps (128/256), sanitization of control and bidi characters, and strict framing rejections (a response with ambiguous framing shows the engine as `not running`/`not installed` rather than `available`). Tested, but a reader of the spec cannot know them.

### SUGGESTION

- S1 Add one app-level test for "Undetected saved engine survives" (saved `codex` not installed, position-only save, `engine = "codex"` stays), see the note above.
- S2 At archive, backport D4 (sanitization, caps, strict framing) and the 62-wide AI popup into the ai-engine-detection and config-menu specs, or record them in the archive report as accepted additions.
- S3 Next change (AI selector): show `checking...` and a footer hint instead of closing silently when Enter is pressed before the first detection answer (W2).
- S4 Known residuals from the reviews, low risk: the process-group kill depends on children not calling `setsid`; ETXTBSY retry is bounded to 4 attempts; the `Detection` of a failed run reads `unknown` for all engines (by design, AE-6).

## Manual checks (user, 2026-10-11, binary from 21f04e4)

VP.1 PASS, VP.2 PASS on re-check (first attempt explained by W2), VP.3 PASS, VP.4 PASS. #88 and #89 add tests and a refactor only (verified here: all tests green at 3eb1879).

## Final verdict

PASS WITH WARNINGS. All 85 scenarios are covered (81 automated, 4 manual PASS), cargo test, clippy and fmt are clean, no CRITICAL findings. Recommended before archive: tick VP.1-VP.4 in tasks.md (W1) and decide whether to sync design.md and the specs with D1-D7 (W4, W5, S2). Then run sdd-archive with the two Purpose-line edits.

## Update 2026-10-11 (ledger)

The verdict and findings above are unchanged. This section records how the verify PR (`feat/ai-engines-verify`) treats each WARNING.

| Finding | Status | What this PR does |
|---|---|---|
| W1 VP tasks out of sync | Resolved | tasks.md ticks VP.1-VP.4 (user PASS, 2026-10-11) and VP.5 (this verification). VP.6 stays open on purpose, it is archive-time. |
| W2 Silent no-op before the first detection answer | Carried | Not fixed here. It moves to the AI selector change (see S3), which must show `checking...` and a footer hint instead of closing silently. |
| W3 Snapshot `menu_popup_is_drawn_over_the_panes_after_the_bars` changed | Accepted | The App always attaches the AI section, so the App-level snapshot MUST include it. Settings-only snapshots are unchanged. |
| W4 Design lags the code (D1-D7) | Resolved | design.md is synced with the delivered code (see "Review-driven changes" there). |
| W5 Unspecified user-visible behaviour | Resolved | ai-engine-detection gains the requirements "Untrusted text is sanitized and bounded" and "HTTP framing is strict"; config-menu states the 62/51 popup width. Coverage rows are in tasks.md. |

The post-plan PRs #77, #82, #83, #84, #88 and #89 are listed in tasks.md under "Review-driven slices".
