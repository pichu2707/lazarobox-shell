# Verification Report: config-menu

**Change**: config-menu
**Mode**: Strict TDD (hybrid store: openspec + engram)
**Verified commit**: origin/feat/config-menu @ 74b5d29
**Verdict**: PASS WITH WARNINGS (0 CRITICAL, 6 WARNING of which W2 and W4 resolved in PR #46, 4 SUGGESTION)

## Completeness

| Metric | Value |
|---|---|
| Tasks total | 76 |
| Tasks checked | 76 |
| Tasks unchecked | 0 |
| Manual checks | CM3.10 and CM4b.11 (Kitty) ticked; user confirmed both OK |

tasks.md ticks match the code state: every task's artefact exists (core::menu, core::config_edit, runtime::config_write, ui::components::menu, Effect::SaveConfig, Panes::with_config_path, info_blue, the m binding in PREFIX_TREE).

## Execution evidence

| Command | Result |
|---|---|
| cargo test | 605 passed, 0 failed, 0 ignored (lib); bin 0 tests; doc-tests 0 |
| cargo clippy --all-targets -- -D warnings | clean, exit 0 |
| cargo fmt --check | clean, exit 0 |
| Flaky core::pty::portable::tests::drop_kills_background_jobs | passed in this run (nothing to re-run) |
| Coverage | not run (no coverage tool configured) |

Run as uid 1000, so the permission-based tests (read-only dir, read-only file) ran for real; they only self-skip as root.

## Spec compliance matrix (scenario -> test -> status)

COMPLIANT = a covering test passed at runtime in this run (or an accepted manual check).

### config-menu

| Scenario | Test(s) | Status |
|---|---|---|
| Open | app::opening_the_menu_enters_menu_without_writing, prefix_m_opens_the_menu_from_the_real_table | COMPLIANT |
| Esc closes | app::esc_closes_the_menu_back_to_terminal, esc_returns_to_terminal_without_effects | COMPLIANT |
| Reopen starts fresh | core::menu::open_selects_the_first_row_with_no_error; app::esc_restores_the_original_positions_and_relayouts | COMPLIANT |
| Descriptor integrity | core::menu::descriptor_table_is_well_formed, every_choice_get_returns_what_set_stored | COMPLIANT |
| Settings rows (insta) | ui::components::menu::snapshot_settings_popup_at_the_wanted_size, disabled_mouse_row_is_muted, selected_row_is_reversed_and_the_others_are_not; core::menu::rows_* | COMPLIANT |
| Move down/up, Wrap | core::menu::move_keys_wrap_in_both_directions | COMPLIANT |
| Disabled row skipped | core::menu::rows_list_title_then_settings_then_disabled_mouse, move_keys_wrap_in_both_directions | COMPLIANT |
| Repeat moves | core::menu::repeat_moves_like_a_press; app::a_held_enter_or_esc_does_nothing_but_a_held_j_moves | COMPLIANT |
| Cycle forward / backward+Space / Arrow keys | core::menu::cycle_keys_change_the_selected_value, cycling_acts_on_the_selected_row | COMPLIANT |
| Statusline moves live | app::cycling_the_tab_bar_moves_it_live, a_preview_with_one_tab_keeps_the_forced_body_height | COMPLIANT |
| Preview emits ResizePty only for changed panes | app::a_preview_emits_resize_only_for_panes_whose_size_changed, a_preview_with_several_tabs_moves_the_bar_without_a_resize | COMPLIANT |
| Esc reverts | app::esc_restores_the_original_positions_and_relayouts | COMPLIANT |
| Esc reverts the one-tab bar | app::esc_and_enter_hide_the_forced_bar_and_resize_back | COMPLIANT |
| External edit is not reloaded | structural (menu path performs no I/O); indirect via esc_restores_the_original_positions_and_relayouts | COMPLIANT (indirect, see S1) |
| Unknown key | app::unlisted_keys_and_release_emit_nothing_and_stay_in_menu, other_keys_and_ctrl_space_are_swallowed_and_resize_stays; core::menu::release_ctrl_space_and_unknown_keys_are_swallowed, shift_is_ignored_for_listed_keys_and_uppercase_is_swallowed | COMPLIANT |
| Paste ignored | app::paste_in_menu_writes_nothing, paste_goes_to_the_pty_only_in_terminal | COMPLIANT |
| Footer hint | ui::components::menu snapshots | COMPLIANT |
| Error replaces hint | core::menu::the_error_clears_on_the_next_press_which_then_acts, an_unlisted_key_clears_the_error_with_no_other_effect; ui::components::menu::snapshot_error_replaces_the_hint_and_is_clipped | COMPLIANT |
| No panic at any size | ui::components::menu::no_panic_at_any_size, nothing_is_drawn_at_zero_size; app::the_forced_bar_never_panics_on_tiny_terminals | COMPLIANT |
| Minimal message and Esc works | ui::components::menu::below_the_threshold_only_the_fallback_line_is_drawn, fallback_is_clipped_with_an_ellipsis, the_full_popup_needs_exactly_24_by_7; Esc is size independent | COMPLIANT (see W6) |
| Output keeps flowing | app::pane_output_is_still_parsed_while_the_menu_is_open | COMPLIANT |
| Pane exit keeps the menu | app::another_pane_exiting_keeps_the_menu_and_the_draft, a_background_tab_pane_exiting_keeps_the_menu_and_the_forced_bar | COMPLIANT |
| Spawn failure keeps the menu | app::a_spawn_failure_keeps_the_menu_and_leaves_its_error_alone | COMPLIANT |
| Host resize | app::a_host_resize_keeps_the_menu_with_the_forced_bar_and_preview, a_host_resize_between_open_and_esc_keeps_the_forced_bar_then_reverts | COMPLIANT |
| Last pane exits | app::closing_the_last_pane_of_the_last_tab_quits_with_the_menu_open | COMPLIANT |
| Menu in Kitty (manual) | user-run check CM3.10 and CM4b.11, OK | COMPLIANT (manual) |

### config-persistence

| Scenario | Test(s) | Status |
|---|---|---|
| Save closes | app::config_saved_closes_the_menu_and_keeps_the_draft, enter_with_a_changed_draft_requests_a_save_and_keeps_the_menu_open; runtime::enter_in_the_menu_saves_and_the_result_closes_it | COMPLIANT |
| Unchanged draft | app::enter_with_an_unchanged_draft_closes_the_menu | COMPLIANT |
| Comments and order preserved | config_edit::replaces_only_the_value_and_keeps_every_other_byte | COMPLIANT |
| Missing keys are created | config_edit::inserts_a_missing_table_and_leaves_the_rest_untouched, inserts_a_missing_key_into_an_existing_table, none_builds_a_document_with_only_the_listed_edits | COMPLIANT |
| Mouse untouched | config_edit::mouse_is_kept_when_present_and_never_added | COMPLIANT |
| Saved file parses back | config_edit::saved_text_parses_back_to_the_draft_positions, parity_with_config_parse_on_broken_input | COMPLIANT |
| Missing file and dirs | config_write::a_missing_file_and_its_directories_are_created | COMPLIANT |
| No config path | config_write::no_config_path_fails_and_writes_nothing; runtime::save_config_without_a_path_answers_one_failure | COMPLIANT |
| Atomic replace | config_write::a_save_replaces_the_file_and_leaves_no_temp, a_stale_temp_from_a_crash_does_not_block_the_save | COMPLIANT |
| Failure keeps the original | config_write::a_read_only_directory_keeps_the_original_and_leaves_no_temp, a_failed_rename_removes_the_temp | COMPLIANT |
| Read-only file refused | config_write::a_read_only_file_is_refused_untouched, a_read_only_target_behind_a_symlink_is_refused_untouched | COMPLIANT |
| Dangling symlink followed (8 ok, 9 refused) | config_write::a_dangling_relative_link_is_followed_and_its_target_created, an_absolute_dangling_target_is_followed_and_created, a_dangling_chain_is_followed_to_the_end, a_dangling_chain_of_eight_links_succeeds, a_dangling_chain_of_nine_links_is_refused_and_creates_nothing, a_symlink_loop_is_refused | COMPLIANT |
| Unparseable file refused | config_write::refused_files_map_to_their_message_and_stay_byte_identical; config_edit::refuses_a_syntax_error | COMPLIANT |
| Invalid value replaced | config_edit::replaces_an_invalid_value_with_the_valid_one | COMPLIANT |
| Non-table collision refused | config_edit::refuses_a_non_table_entry; config_write::refused_files_map_to_their_message_and_stay_byte_identical | COMPLIANT |
| Owned key holding a table refused | config_edit::refuses_an_owned_key_that_holds_a_table, refuses_an_owned_key_that_holds_an_array_of_tables, replaces_an_inline_array_of_tables | COMPLIANT |
| Line endings kept | config_edit::crlf_is_kept_when_replacing_in_place, crlf_is_kept_with_empty_edits, crlf_is_used_for_inserted_lines, a_crlf_inside_a_string_does_not_make_the_file_crlf, an_lf_inside_a_string_is_never_rewritten, a_line_continuation_inside_a_string_keeps_its_crlf, a_mixed_file_follows_its_first_line_ending_outside_strings, plus quote/hash/backslash scanner edge tests | COMPLIANT |
| Write fails | app::config_save_failed_keeps_the_menu_the_draft_and_shows_the_error; runtime::a_failed_save_leaves_the_menu_open_with_the_error | COMPLIANT |
| Retry and cancel after failure | app::esc_after_a_failed_save_reverts_to_the_original, a_double_enter_requests_two_identical_saves; core::menu::the_error_clears_on_the_next_press_which_then_acts | COMPLIANT |
| Exactly one result | runtime::save_config_writes_and_answers_one_saved, save_config_without_a_path_answers_one_failure, the_save_result_is_processed_before_the_next_key | COMPLIANT |
| Persistence in Kitty (manual) | user-run check CM4b.11, OK | COMPLIANT (manual) |

### modal-input

| Scenario | Test(s) | Status |
|---|---|---|
| Enter MENU | app::opening_the_menu_enters_menu_without_writing | COMPLIANT |
| Not from other modes | app::m_does_not_open_the_menu_from_other_modes; prefix::m_inside_a_group_does_not_open_the_menu | COMPLIANT |
| Repeat on Enter and Esc ignored | app::a_held_enter_or_esc_does_nothing_but_a_held_j_moves; core::menu::enter_saves_and_esc_cancels_on_press_only | COMPLIANT |
| Release ignored | app::release_key_events_are_ignored_in_every_mode, unlisted_keys_and_release_emit_nothing_and_stay_in_menu | COMPLIANT |
| Focus change keeps MENU | app::the_focused_pane_exiting_moves_the_focus_and_keeps_the_menu | COMPLIANT |
| m is in the table | prefix::m_is_a_hinted_root_leaf_that_opens_the_menu | COMPLIANT |
| Repeat in MENU | app::a_held_enter_or_esc_does_nothing_but_a_held_j_moves | COMPLIANT |
| Paste in MENU | app::paste_in_menu_writes_nothing | COMPLIANT |

### statusline

| Scenario | Test(s) | Status |
|---|---|---|
| MENU label and color | ui::components::statusline::snapshot_input_menu, input_label_is_rendered_per_mode; ui::menu_mode_shows_its_label_and_the_cwd_never_the_hint | COMPLIANT |
| Mode color mapping includes MENU | ui::theme::menu_has_its_own_accent_distinct_from_every_other_mode, accent_maps_each_app_mode_to_its_palette_color | COMPLIANT |
| Existing snapshots | full suite green; root_hint snapshot contains "m menu" | COMPLIANT |
| Root hint with menu | ui::the_statusline_shows_the_root_and_group_hints_and_drops_them_on_resolve (exact string), root_hint snapshot | COMPLIANT |
| Root hint stays table-generated | prefix::root_hint_lists_only_the_hinted_entries and the hint_* tests using FULL_ROOT | COMPLIANT |

### tabs

| Scenario | Test(s) | Status |
|---|---|---|
| Bar forced with one tab (render) | app::the_menu_forces_a_tab_bar_with_one_tab; ui::menu_popup_is_drawn_over_the_panes_after_the_bars | COMPLIANT |
| Bar hidden on close | app::esc_and_enter_hide_the_forced_bar_and_resize_back, closing_back_to_one_tab_drops_the_bar_in_any_position | COMPLIANT |
| Bar with several tabs unaffected | app::with_several_tabs_the_menu_never_resizes_for_the_bar | COMPLIANT |
| Tiny terminal with forced bar | app::the_forced_bar_never_panics_on_tiny_terminals | COMPLIANT |

Compliance: every scenario is covered by a passing test or an accepted manual check. 0 UNTESTED, 0 FAILING.

## Strict TDD compliance

| Check | Result |
|---|---|
| Test runner | cargo test executed; 605/605 pass |
| RED evidence | tasks.md records RED/GREEN pairs per slice and test files exist for each. apply-progress is a narrative summary with no "TDD Cycle Evidence" table; RED-at-assertion is documented only for the CM4b-iii boundary-8 fix. See W5. |
| Triangulation | Present: 8 vs 9 link boundary, size sweeps 0..60 x 0..12, CRLF/LF/mixed matrices, four bar-position combinations |
| Assertion quality | Exact: error strings asserted verbatim, exact root-hint string, exact directory listings, byte-identical file checks |
| Vacuous tests | None. Tests without a direct assertion are intentional no-panic sweeps (no_panic_at_any_size, a_terminal_too_small_to_tile_does_not_panic, the_forced_bar_never_panics_on_tiny_terminals). The root-only skip in a_read_only_directory_keeps_the_original_and_leaves_no_temp is guarded by a probe and did not trigger. |

## Design (ADR) coherence

| ADR | Status | Notes |
|---|---|---|
| CM-1 state placement + invariant | Honoured | marker + App.menu; debug_assert present |
| CM-2 live config as draft | Honoured | App.config |
| CM-3/4/5 descriptors, navigation, event kinds | Honoured | |
| CM-6/7 m at root, hint order | Honoured | exact string pinned |
| CM-8 forced bar as a real row | Honoured | |
| CM-9 apply_edit | Deviation: EditError::KeyIsTable added (design Interfaces lists it). CM-9 prose still says "No newline normalisation is done", but code follows the first line ending and adds a missing final newline (W2) |
| CM-10 two TOML stacks | Honoured | parity test present |
| CM-11 synchronous save | Honoured | temp file next to the target, create_new, sync_all, rename |
| CM-12 symlinks and permissions | Honoured and extended (dangling follow up to 8 hops, read-only refusal); design and spec both updated |
| CM-13..16 | Honoured | |

Spec and design were updated for KeyIsTable, the dangling-symlink follow, the read-only refusal and line endings (the spec has dedicated scenarios for each).

## Issues

### CRITICAL
None.

### WARNING
- W1. Relative HOME: "save failed: no config path (HOME is not set)" is misleading when HOME is set but relative. The spec ("no usable XDG_CONFIG_HOME or absolute HOME") is met; only the wording is off. Deferred follow-up.
- W2. RESOLVED in PR #46 (design CM-9 rewritten). Was: design CM-9 says "No newline normalisation is done, so untouched bytes stay identical", which contradicts the implemented and spec-covered rule (first line ending wins, mixed files normalised to it, missing final newline added). Update the CM-9 text. Mixed-ending normalisation is a deferred follow-up.
- W3. Deferred robustness gaps, none violating an explicit scenario: ownership not preserved on the replaced file; no directory fsync after rename (atomic but not power-loss durable); a dangling parent chain yields "File exists (os error 17)"; a dangling target with a parent-dir component creates a stray directory; BOM and trailing-newline edge cases.
- W4. RESOLVED in PR #46 (CM4a.2 updated, CM4a.14, CM4a.15 and CM4b.13 added, PR list added). Was: tasks.md has no tasks for the CM4b-iii hardening (dangling symlinks, read-only refusal, KeyIsTable, line endings); CM4a.2 still lists EditError::{Unparseable, NotATable} only. The work is spec and test covered, but task traceability lags the code.
- W5. apply-progress has no per-task TDD Cycle Evidence table, so RED-first for most slices cannot be re-verified from the artifact (only from commit history and PR text). Process warning, not a code defect.
- W6. "Minimal message and Esc works" has no single end-to-end test (render tiny, then Esc); it is covered by two independent tests (fallback render, size-independent key path).

### SUGGESTION
- S1. "External edit is not reloaded" has no test that touches a real file while the menu is open; it holds because the menu path does no I/O. Add one if the I/O boundary moves.
- S2. Add a pointer from design CM-9 to the spec's "Line endings kept" scenario so they cannot drift.
- S3. Add a coverage run (cargo llvm-cov) to give quantitative Strict TDD evidence.
- S4. The tracker rebase onto main (CM-16) is pending; re-run the full suite afterwards, since the app, ui, prefix and theme modules may conflict.

## Verdict

PASS WITH WARNINGS. No CRITICAL issues; ready for archive once the warnings are accepted.
