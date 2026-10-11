# Verification Report: panes-tabs

- Change: `panes-tabs` (Slice 2: panes and tabs), tracker PR #10 (`feat/panes-tabs` -> `main`)
- Verified commit: `7f8958c` (`origin/feat/panes-tabs`), detached checkout, own `--target-dir`
- Mode: Strict TDD, artifact store hybrid (openspec + engram)
- Native status before verify: `nextRecommended=verify`, no blocked reasons, 103/103 tasks
- Verdict: **PASS WITH WARNINGS** (0 CRITICAL, 4 WARNING, 5 SUGGESTION)

## 1. Completeness

| Metric | Value |
|---|---|
| Tasks in `tasks.md` | 103 |
| Tasks ticked | 103 |
| Tasks unticked | 0 |
| Manual Kitty checks (S6.7, S7.8, S7d.3, S8b.4, S8c.6, S9.7) | all ticked, each confirmed by the user |

The ticks match the code state: every slice named in `tasks.md` (S1, S2a, S2b, S3a/b, S4, S5a, S5b, S6, S7, S7d, S8a, S8b, S8c, S9, S9c) has its modules and tests on the tracker. `src/core/{layout,prefix,osc7,config}.rs`, `src/core/pane.rs` (`with_host`, `cwd`), `src/runtime.rs` (`Panes`, `shutdown`, `poll_cwds`, `local_hostname`, `load_config`) and `src/ui/components/{separators,tab_bar}.rs` all exist.

Observation: the PR body of S9c lists a follow-up task `S9c.4` (a relative `$HOME` is never a spawn fallback; test `a_relative_home_is_never_a_spawn_fallback`). `tasks.md` stops at S9c.3. The code and the test exist; only the task line is missing (SUGGESTION S-1).

## 2. Build and test evidence

| Command | Result |
|---|---|
| `cargo test` | exit 0, **513 passed, 0 failed, 0 ignored** (lib unit tests; bin and doc-tests have 0 tests) |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| `cargo fmt --check` | exit 0 |
| Known flaky `core::pty::portable::tests::drop_kills_background_jobs` | passed in the full run, no re-run needed |
| Coverage tool | not configured for the project; not run |

Vacuity scan: no `assert!(true)`, no `#[ignore]`, no `todo!()`/`unimplemented!()` in `src/`. Sampled assertions (`config_path` table, `close_kills_off_the_loop`, layout tables, S9c `SpawnPane` cwd asserts) compare exact values; the S9c.2 tightening to `effects.last()` is in place.

## 3. Spec compliance matrix

Legend: COMPLIANT = a covering test passed in the run above. MANUAL-OK = user confirmed in Kitty (ticked task). WARN = see section 7. Scenarios that share one test are grouped in one row.

### configuration (11 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Path resolution | `config::tests::the_path_prefers_xdg_then_home` | COMPLIANT |
| Relative or empty HOME is ignored | same test (explicit `p(None, Some(""))`, `p(None, Some("rel"))`) | COMPLIANT |
| Defaults | `defaults_are_statusline_bottom_tabbar_top_no_mouse` | COMPLIANT |
| Each value | `each_position_is_read`, `mouse_is_read_at_the_top_level`, `a_partial_file_keeps_the_other_defaults` | COMPLIANT |
| Unknown keys ignored | `unknown_keys_and_tables_are_ignored` | COMPLIANT |
| Missing file | `a_missing_file_is_silent_defaults` | COMPLIANT |
| Invalid value falls back per key | `an_invalid_value_falls_back_for_that_key_only`, `an_invalid_key_keeps_the_rest_and_the_notice_names_it`, `each_invalid_key_and_type_is_reported_with_its_key` | COMPLIANT |
| Several invalid keys | `several_invalid_keys_report_the_count_then_the_first` | COMPLIANT |
| Unparseable file | `unparseable_toml_gives_all_defaults_and_a_notice`, `only_unparseable_toml_is_an_error`, `an_unreadable_file_gives_defaults_and_a_notice` | COMPLIANT |
| Mouse reserved | `mouse_true_is_accepted_with_a_notice`, `a_bad_mouse_keeps_the_valid_positions`, `a_problem_notice_wins_over_the_mouse_notice` | COMPLIANT |
| Positions in Kitty (manual) | S8c.6 | MANUAL-OK |

### copy-mode-scrollback (12 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Offsets are independent / Other panes keep running / Keys act on the focused pane | `copy_is_per_pane_and_other_panes_keep_running`, `copy_motions_scroll_the_pane` | COMPLIANT |
| Offset and total | `the_copy_position_tracks_motions_and_output_while_scrolled`, `ui::tests::the_statusline_shows_the_copy_position_while_scrolling`, `statusline::tests::the_copy_position_follows_the_mode_label_in_the_mode_block` | COMPLIANT |
| Bottom of the history | `statusline::tests::the_copy_position_shows_an_explicit_zero_offset` | COMPLIANT |
| Output while scrolled | `copy_offset_follows_the_pane_when_output_arrives_while_scrolled` | COMPLIANT |
| No history | `the_copy_position_is_absent_without_history`, `no_copy_position_leaves_the_bare_label` | COMPLIANT |
| Narrow width | `the_path_budget_accounts_for_the_copy_position`, `a_narrow_copy_statusline_never_panics` | COMPLIANT |
| Focus move exits COPY | `resize_ends_when_the_focus_moves_but_not_when_it_stays`, `the_focused_pane_exiting_in_copy_returns_to_a_live_terminal` | COMPLIANT |
| Focused pane exits while in COPY | `the_focused_pane_exiting_in_copy_returns_to_a_live_terminal` | COMPLIANT |
| Tab switch exits COPY | `a_tab_change_exits_resize_and_copy` | COMPLIANT |
| Cursor shape restored on focus exit | `cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit`, `child_shape_requests_during_copy_do_not_leak_until_exit` | COMPLIANT |

### modal-input (37 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Table integrity | `prefix::tests::chords_are_unique_within_every_group`, `every_action_is_reachable_exactly_once`, `every_binding_and_group_has_text`, `prefix_key_is_bound_at_the_root_only`, `question_mark_is_reserved_and_unhinted` | COMPLIANT |
| Group opens | `group_keys_open_their_group_without_effects`, `prefix::tests::lookup_focus_keys_and_group_keys` | COMPLIANT |
| Binding in a group | `a_binding_in_a_group_returns_to_terminal_with_no_effect_yet` (S3b) plus the later action tests (`split_right_*`, `new_tab_*`) | COMPLIANT |
| Shift on `g B` | `shift_is_ignored_for_character_keys_in_the_prefix_flow`, `prefix::tests::shift_is_ignored_for_character_keys` | COMPLIANT |
| Focus follows the new pane | `split_right_spawns_a_side_by_side_pane_and_focuses_it`, `split_below_stacks_the_new_pane_and_focuses_it` | COMPLIANT |
| Focus keys | `focus_keys_move_to_the_geometric_neighbour`, `focus_at_an_edge_stays_put`, `root_focus_keys_with_one_pane_are_no_ops` | COMPLIANT |
| Reserved `?` | `question_mark_returns_to_terminal_with_no_effect` | COMPLIANT |
| Esc cancels / Unmapped key cancels / Ctrl+Space cancels | `group_esc_unmapped_and_prefix_key_cancel_and_are_swallowed`, `prefix::tests::the_prefix_key_cancels_inside_a_group` | COMPLIANT |
| Close pane confirmed | `close_pane_asks_then_y_closes_the_focused_pane` | COMPLIANT |
| Close pane declined | `close_pane_is_declined_by_anything_but_a_plain_y` | COMPLIANT |
| Last pane prompts quit | `prefix_q_asks_for_confirmation`, `the_last_pane_of_the_last_tab_quits` | COMPLIANT |
| Last pane of one of two tabs prompts close pane | `the_last_pane_of_one_of_two_tabs_asks_to_close_the_pane` | COMPLIANT |
| Only tab prompts quit | `close_tab_on_the_only_tab_asks_to_quit` | COMPLIANT |
| Removal cancels the prompt | `any_removal_cancels_a_pending_confirmation`, `a_pending_confirm_is_cancelled_when_a_pane_of_an_inactive_tab_exits` | COMPLIANT |
| Sticky / Repeat allowed | `resize_stays_until_esc_across_many_steps`, `repeat_steps_like_press_in_resize` | COMPLIANT |
| Esc exits | `esc_returns_to_terminal_without_effects` and the RESIZE exit tests | COMPLIANT |
| Keys swallowed | `other_keys_and_ctrl_space_are_swallowed_and_resize_stays` | COMPLIANT |
| Single pane RESIZE | `resize_with_a_single_pane_is_allowed_and_a_no_op` | COMPLIANT |
| Pane close in COPY | `the_focused_pane_exiting_in_copy_returns_to_a_live_terminal` | COMPLIANT |
| Tab switch in RESIZE | `a_tab_change_exits_resize_and_copy` | COMPLIANT |
| Focus change in RESIZE / Removal that keeps focus leaves RESIZE | `resize_ends_when_the_focus_moves_but_not_when_it_stays` | COMPLIANT |
| Passthrough | `terminal_passes_keys_to_the_pty`, `effects_name_the_focused_pane` | COMPLIANT |
| Repeat ignored in decision states | `repeat_in_prefix_does_nothing_and_stays_in_prefix`, `repeat_is_ignored_in_group_and_keeps_the_group_pending`, `repeat_of_y_in_confirm_quit_does_not_quit` | COMPLIANT |
| Release ignored | `release_key_events_are_ignored_in_every_mode`, `release_is_ignored_in_group`, `release_is_ignored_in_resize` | COMPLIANT |
| Input goes to the focused pane only | `keys_and_paste_go_to_the_focused_pane_only` | COMPLIANT |
| Enter PREFIX / Literal prefix | `prefix_key_enters_prefix_without_writing`, `held_prefix_key_enters_prefix_once_and_repeats_are_dropped`, `prefix_twice_sends_a_literal_nul` | COMPLIANT |
| Cancel/unknown | `prefix_cancel_and_unmapped_keys_are_swallowed` | COMPLIANT |
| Real terminal reports NUL (manual) | inherited from the base slice; confirmed in Kitty by the user on 2026-10-11, recorded as S9d.1 | OK (W-2 RESOLVED in PR #50) |
| Confirm / Decline (quit) | `confirm_quit_y_quits`, `confirm_quit_declined_by_n_esc_or_other_keys`, `confirm_quit_uppercase_y_declines` | COMPLIANT |
| Paste in COPY | `paste_goes_to_the_pty_only_in_terminal` | COMPLIANT |
| Resize only changed panes | `a_host_resize_resizes_only_the_panes_whose_size_changed`, `a_host_resize_resizes_every_pane_that_changed` | COMPLIANT |

### pane-layout (29 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Ids are never reused | `layout::tests::allocator_starts_at_first_and_never_reuses_an_id`, `allocator_panics_instead_of_wrapping` | COMPLIANT |
| Rects tile the area | `rects_and_separators_sum_to_the_area_for_nested_splits`, `rounding_follows_the_weights_and_gives_the_remainder_to_the_second` | COMPLIANT |
| Split right / below | `split_right_and_below_make_two_panes_with_weights_equal_to_cell_sizes`, `side_by_side_split_halves_with_a_separator_column`, `stacked_split_halves_with_a_separator_row` | COMPLIANT |
| Refused below minimum size | `split_applies_at_the_minimum_and_is_refused_one_cell_less`, `a_refused_split_is_a_no_op`, `a_refused_split_leaves_the_tree_and_panes_unchanged` | COMPLIANT |
| Tiny terminal never panics | `degenerate_extents_never_panic_and_the_first_child_keeps_everything`, `an_empty_area_and_the_far_corner_of_the_plane_do_not_overflow`, `tiny_terminals_never_panic_in_any_combination` | COMPLIANT |
| Minimum split sizes | `the_last_cell_before_the_minimum_is_still_allowed`, `split_checks_the_target_rect_not_the_whole_area` | COMPLIANT |
| Move focus / Edge no-op / Geometry beats tree order | `neighbour_moves_across_the_separator_and_stops_at_the_edges`, `neighbour_picks_the_largest_overlap_not_the_tree_order`, `neighbour_breaks_an_overlap_tie_by_the_lowest_start`, `neighbour_requires_exactly_one_separator_cell_between_the_rects` | COMPLIANT |
| Sibling expands | `remove_collapses_the_split_so_the_sibling_takes_the_whole_area`, `remove_after_split_restores_the_parent_rect` | COMPLIANT |
| Focus goes to the pane covering the old top-left | `focus_after_closing_the_focused_pane_is_the_one_covering_its_old_top_left`, `a_focused_shell_exit_focuses_the_pane_covering_its_top_left` | COMPLIANT |
| Closing an unfocused pane keeps focus | `an_unfocused_shell_exit_collapses_its_pane_and_keeps_focus` | COMPLIANT |
| Last pane closes the tab | `removing_the_last_pane_reports_was_last_and_an_unknown_one_not_found`, `the_last_pane_of_a_tab_closes_the_tab_without_quitting` | COMPLIANT |
| Border moves one cell | `resize_right_grows_the_left_pane_by_exactly_one_cell`, `h_and_l_move_the_border_and_tell_both_panes`, `a_step_after_a_terminal_resize_moves_one_cell_from_the_drawn_border` | COMPLIANT |
| Clamped at minimum | `a_step_that_would_cross_the_minimum_is_refused_and_changes_nothing`, `shrinking_a_side_never_squeezes_a_nested_pane_below_the_minimum` | COMPLIANT |
| Focused pane shrinks | `resize_left_shrinks_the_focused_left_pane_by_one_cell`, `the_border_moves_in_the_key_direction_whichever_pane_has_the_focus` | COMPLIANT |
| No border on that axis | `a_step_along_an_axis_without_a_split_is_a_no_op`, `a_step_without_a_border_on_that_axis_emits_nothing` | COMPLIANT |
| Single pane | `a_single_pane_ignores_every_step` | COMPLIANT |
| Zoom and restore | `w_z_zooms_the_focused_pane_over_the_whole_body`, `zooming_twice_restores_the_layout_and_the_pane_size`, `ui::tests::a_zoomed_pane_fills_the_body_without_separators` | COMPLIANT |
| Split while zoomed | `a_split_while_zoomed_unzooms_first_and_then_splits`, `a_refused_split_while_zoomed_still_unzooms` | COMPLIANT |
| Focus change while zoomed | `a_focus_key_while_zoomed_unzooms_and_moves_to_the_neighbour`, `removing_a_pane_while_zoomed_unzooms` | COMPLIANT |
| RESIZE while zoomed | `entering_resize_while_zoomed_unzooms_first` | COMPLIANT |
| Focused separator uses the mode accent | `separators::tests::the_separator_next_to_the_focused_pane_gets_the_accent`, `only_the_run_touching_the_focused_pane_is_accented` | COMPLIANT |
| Accent follows the input mode | `ui::tests::the_separator_accent_follows_the_mode_and_the_focus`, `the_separator_accent_follows_resize_copy_and_confirmation` | COMPLIANT |
| Focus change recolors | `separators::tests::separators_away_from_the_focus_are_muted_and_focus_change_recolors` | COMPLIANT |
| nvim in split panes, Focus/close/resize/zoom, exit collapses its pane (manual) | S6.7, S7.8 | MANUAL-OK |

### statusline (25 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Root hint in PREFIX, Root hint clipped and cleared | `ui::tests::the_statusline_shows_the_root_and_group_hints_and_drops_them_on_resolve`, `prefix::tests::root_hint_lists_only_the_hinted_entries`, `the_path_segment_shows_the_root_then_the_group_hint_then_the_cwd` | COMPLIANT |
| Group hint | `prefix::tests::group_hints_list_every_binding_in_table_order`, same UI test | COMPLIANT |
| Hint is generated from the table | `prefix::tests::hint_follows_the_hinted_flag_not_the_position`, `hint_uses_the_chord_label` | COMPLIANT |
| Hint clipped by whole entries, Narrow width | `prefix::tests::hint_is_clipped_by_whole_entries_with_an_ellipsis`, `hint_never_exceeds_the_width_nor_shows_a_partial_entry`, `narrow_widths_degrade_to_an_ellipsis_then_nothing`, `ui::tests::a_narrow_statusline_clips_the_hint_by_whole_entries_and_never_panics` | COMPLIANT |
| Hint gone after the group resolves | same UI test, `modes_without_a_hint_show_the_cwd` | COMPLIANT |
| Zoomed, Unzoomed | `ui::tests::the_statusline_shows_z_only_while_zoomed`, `statusline::tests::the_zoom_indicator_follows_the_mode_label`, `the_zoom_indicator_is_absent_when_not_zoomed`, `the_path_budget_accounts_for_the_zoom_indicator` | COMPLIANT |
| Notice shown | `ui::tests::a_spawn_failure_notice_replaces_the_cwd_until_the_next_key`, `a_notice_is_drawn_instead_of_the_hint`, `a_notice_wins_over_the_hint` | COMPLIANT |
| Notice cleared by next key | `the_notice_lasts_until_the_next_press_or_repeat`, `a_startup_notice_shows_until_the_next_key` | COMPLIANT |
| Statusline on top, Tiny terminal | `ui::tests::a_statusline_on_top_leaves_the_pane_below_it`, `the_statusline_on_top_and_the_tab_bar_at_the_bottom`, `tiny_terminals_never_panic_in_any_combination`, `ui::tests::a_terminal_too_small_to_tile_does_not_panic` | COMPLIANT |
| Label and color | `statusline::tests::snapshot_input_group_window`, `snapshot_input_group_tab`, `snapshot_input_group_go`, `snapshot_input_group_buffer`, `snapshot_input_resize`, `input_label_is_rendered_per_mode` | COMPLIANT |
| Mode color mapping | `theme::tests::input_mode_colors_follow_the_theme`, `input_mode_style_maps_each_mode_to_its_color_with_base_fg`, `accent_maps_each_app_mode_to_its_palette_color` | COMPLIANT |
| Existing snapshots | `statusline::tests::snapshot_input_terminal`, `snapshot_input_prefix`, `snapshot_input_copy`, `snapshot_normal_mode` | COMPLIANT |
| Close pane prompt, Close tab prompt, Quit prompt | `close_tab_prompt_reads_close_tab`, `ui::tests::the_close_prompt_is_in_the_mode_block_and_the_cwd_stays`, `statusline::tests::snapshot_input_confirm_quit` | COMPLIANT |
| cwd changes (unix PTY) | `runtime::tests::cd_in_a_real_shell_is_seen_by_the_poll`, `cwd_event_updates_the_cwd_and_marks_dirty` | COMPLIANT |
| OSC 7 cwd shown | `osc7_cwd_is_shown_and_survives_a_later_poll` | COMPLIANT |
| Focus switches cwd | `ui::tests::focus_switches_the_cwd_shown`, `the_cwd_label_follows_the_focused_pane` | COMPLIANT |
| Read failure | `runtime::tests::cwd_poll_yields_nothing_without_a_pid_or_when_the_lookup_fails`, `cwd_is_retained_when_no_new_cwd_arrives` | COMPLIANT |
| Shell name, Narrow width | `ui::tests::statusline_shows_the_cwd_and_the_shell_name`, `shell_name_is_the_basename_of_the_shell_with_an_sh_fallback`, `statusline::tests::narrow_width_keeps_the_mode_block_and_drops_the_right_segment` | COMPLIANT |

### tabs (27 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| New tab, New tab is appended | `new_tab_is_appended_activated_and_spawns_in_the_focused_cwd`, `a_new_tab_never_shifts_the_numbers` | COMPLIANT |
| Close tab confirmed, Next tab becomes active | `close_tab_asks_then_y_closes_the_tab_and_its_panes`, `closing_a_tab_activates_the_one_that_takes_its_index` | COMPLIANT |
| Close tab declined | `close_tab_is_declined_by_anything_but_a_plain_y` | COMPLIANT |
| Close the only tab asks Quit, quits | `close_tab_on_the_only_tab_asks_to_quit`, `closing_the_only_tab_quits` | COMPLIANT |
| Next wraps, Previous wraps | `next_and_previous_tab_wrap_around` | COMPLIANT |
| Go to tab N, Missing tab is a no-op | `goto_selects_the_tab_and_ignores_a_missing_one` | COMPLIANT |
| Single tab navigation | `navigation_with_one_tab_changes_nothing` | COMPLIANT |
| State preserved | `each_tab_keeps_its_layout_focus_and_zoom` | COMPLIANT |
| Background output | `output_for_a_background_tab_is_parsed`, `a_hidden_pane_still_receives_its_output` | COMPLIANT |
| Hidden with one tab | `ui::tests::no_tab_bar_is_drawn_with_one_tab` | COMPLIANT |
| Shown with two tabs | `tab_bar::tests::snapshot_two_tabs_second_active`, `ui::tests::the_tab_bar_is_drawn_on_the_top_row_with_two_tabs` | COMPLIANT |
| Tab label, Label without cwd | `tab_labels_are_the_number_and_the_cwd_basename`, `a_tab_label_without_a_cwd_is_its_number`, `the_tab_label_follows_the_osc7_cwd`, `a_root_or_missing_home_never_produces_a_tilde` | COMPLIANT |
| Body math, with configured positions | `the_body_starts_below_the_tab_bar_when_there_are_several_tabs`, `several_tabs_in_the_four_position_combinations`, `one_tab_with_the_statusline_on_top_moves_the_body_down`, `positions_change_the_ptys_body_and_emit_no_resize_for_a_same_height_move` | COMPLIANT |
| Bar disappears on close | `closing_back_to_one_tab_drops_the_bar_in_any_position`, `ui::tests::the_bar_disappears_when_one_tab_is_left` | COMPLIANT |
| Narrow width | `tab_bar::tests::snapshot_narrow_keeps_the_active_tab`, `never_panics_at_any_width`, `tabs_before_the_active_one_drop_only_as_needed` | COMPLIANT |
| Last shell exits | `the_last_shell_exiting_quits`, `pty_exit_quits` | COMPLIANT |
| Last pane of a tab with sibling tabs | `the_last_pane_of_a_tab_closes_the_tab_without_quitting`, `an_inactive_tab_closing_keeps_the_active_tab` | COMPLIANT |
| Tabs and tab bar (manual) | S8b.4 | MANUAL-OK |
| New tab from a vanished cwd | `a_new_tab_from_a_vanished_cwd_uses_the_launch_cwd` | COMPLIANT |
| New tab inherits cwd (manual) | S9.7; also auto `a_new_tab_inherits_the_osc7_cwd` | MANUAL-OK and COMPLIANT |

### terminal-emulation (28 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| BEL terminator, ST terminator | `pane::tests::osc7_with_bel_sets_the_cwd`, `osc7_with_st_sets_the_cwd`, `osc7::tests::valid_path_empty_host` | COMPLIANT |
| Percent-decoding | `osc7::tests::percent_escapes_decoded`, `percent_decode_unit` | COMPLIANT |
| Semicolon in path | `osc7::tests::semicolon_params_rejoined`, `pane::tests::osc7_semicolons_and_escapes_survive_the_parser_split` | COMPLIANT |
| Remote host rejected | `osc7::tests::remote_host_rejected`, `pane::tests::osc7_accepts_the_configured_hostname_only` | COMPLIANT |
| Malformed ignored | `bad_percent_escape_rejected`, `nul_and_control_bytes_rejected`, `relative_or_missing_path_rejected`, `non_file_scheme_rejected`, `payload_of_1024_bytes_or_more_rejected`, `pane::tests::a_rejected_osc7_keeps_the_previous_cwd` | COMPLIANT |
| Local hosts accepted | `local_hosts_accepted_case_insensitive`, `matching_hostname_accepted`, `a_hostname_makes_matching_osc7_reports_valid`, `panes_created_after_with_hostname_accept_the_hostname_too` | COMPLIANT |
| Non-UTF-8 path bytes | `osc7::tests::non_utf8_bytes_accepted` | COMPLIANT |
| Split across reads (OSC 7) | `pane::tests::osc7_split_across_chunks_is_reassembled` | COMPLIANT |
| No query reply (OSC 7) | `pane::tests::osc7_produces_no_reply_bytes` | COMPLIANT |
| cwd follows `cd` (manual) | S9.7 | MANUAL-OK |
| Unfocused pane has no cursor | `ui::tests::only_the_focused_pane_places_the_cursor_at_its_rect` | COMPLIANT |
| Shape follows focus, Unfocused shape change ignored | `cursor_shape_follows_the_focused_pane_and_ignores_the_others` | COMPLIANT |
| DECSCUSR (Bar steady, Bar blinking, Block steady, Default, Unknown parameter, Split across reads, No query reply) | `pane::tests::decscusr_*` (9 tests) | COMPLIANT |
| Restore on exit | `runtime::tests::restore_resets_the_cursor_and_bracketed_paste`, `cursor_style_is_applied_on_change` | COMPLIANT |
| Cursor shape in nvim (manual), Real apps (manual) | base slice; exercised by the user in Kitty during S6.7 and S7.8 (nvim in panes) | MANUAL-OK |
| Snapshot, Hidden cursor | `terminal_view::tests::snapshot_styled_screen`, `hidden_cursor_has_no_position` | COMPLIANT |
| Two panes clipped | `terminal_view::tests::a_pane_larger_than_the_area_is_clipped`, `ui::tests::every_pane_is_drawn_in_its_rect_with_a_separator_between`, `a_long_line_in_the_left_pane_leaves_the_separator_and_right_pane_intact` | COMPLIANT |
| Cursor offset to pane rect | `ui::tests::real_cursor_follows_the_pane_cursor`, `terminal_view::tests::cursor_is_placed_inside_the_area` | COMPLIANT |

### terminal-session (28 scenarios)

| Scenario | Evidence | Status |
|---|---|---|
| Output routed by id | `pty_output_is_routed_to_its_own_pane`, `runtime::tests::spawned_panes_tag_their_events_with_their_id`, `write_and_resize_are_routed_by_id` | COMPLIANT |
| Stale events dropped | `pty_events_from_unknown_panes_are_dropped`, `events_for_unknown_panes_are_still_dropped_with_several_panes`, `runtime::tests::late_events_from_a_closed_pane_have_no_effect`, `effects_for_unknown_ids_are_ignored` | COMPLIANT |
| Split inherits cwd | `a_split_spawns_in_the_cwd_of_the_focused_pane`, `a_split_inherits_the_osc7_cwd` | COMPLIANT |
| Vanished cwd falls back | `a_split_from_a_vanished_cwd_uses_the_launch_cwd`, `a_vanished_launch_cwd_falls_back_to_home`, `with_nothing_left_the_spawn_falls_back_to_the_root`, `a_relative_home_is_never_a_spawn_fallback` | COMPLIANT |
| Existing cwd is inherited unchanged | `an_existing_cwd_is_inherited_unchanged_with_a_dir_check` | COMPLIANT |
| Unknown cwd falls back | `a_split_from_a_pane_without_a_known_cwd_uses_the_launch_cwd`, `new_tab_falls_back_to_the_launch_cwd` | COMPLIANT |
| Failed split removes the pane | `a_failed_split_removes_the_pane_and_restores_focus`, `runtime::tests::a_failed_spawn_is_fed_back_as_an_event` | COMPLIANT |
| Failed new tab | `a_failed_new_tab_is_removed_and_the_previous_tab_stays` | COMPLIANT |
| Notice clears on next key | `the_notice_lasts_until_the_next_press_or_repeat` | COMPLIANT |
| Bad cwd is not a failure (auto, unix PTY) | `core::pty::portable::tests::a_missing_cwd_starts_the_shell_in_home` (characterization of `portable-pty`; HOME injected through the spawn env), recorded as S9d.2 | OK (W-1 RESOLVED in PR #50) |
| UI stays responsive | `runtime::tests::close_kills_off_the_loop` (fake handle), `pty::portable::tests::kill_force_kills_a_shell_ignoring_sighup` (real PTY) | COMPLIANT |
| Fallback for plain shell | `runtime::tests::cd_in_a_real_shell_is_seen_by_the_poll`, `the_poll_fills_in_for_a_shell_that_never_reports` | COMPLIANT |
| OSC 7 wins | `osc7_cwd_is_shown_and_survives_a_later_poll`, `a_rejected_osc7_leaves_the_poll_cwd_visible` | COMPLIANT |
| SHELL set, SHELL unset or empty | `pty::tests::spawn_spec_honours_another_shell`, `spawn_spec_falls_back_to_sh_when_shell_is_unset`, `spawn_spec_falls_back_to_sh_when_shell_is_empty`, `the_initial_spawn_describes_the_shell_in_the_focused_pane`, `the_initial_spawn_falls_back_to_sh_in_the_root` | COMPLIANT |
| Echo round trip | `portable::tests::echo_round_trip` | COMPLIANT |
| Resize propagates | `portable::tests::stty_size_reflects_a_resize`, `resize_emits_resize_pty_in_every_mode` | COMPLIANT |
| Degenerate size | `pane::tests::zero_size_is_clamped_to_one_cell`, `a_tiny_terminal_keeps_a_1x1_body` | COMPLIANT |
| Only changed panes resized, Unaffected pane untouched | `a_host_resize_resizes_only_the_panes_whose_size_changed` | COMPLIANT |
| Hidden tabs stay sized | `a_host_resize_resizes_the_panes_of_every_tab`, `a_new_tab_under_a_configured_layout_resizes_every_pane_once` | COMPLIANT |
| Zoom resizes only the zoomed pane | `a_host_resize_while_zoomed_resizes_only_the_zoomed_pane_until_unzoom` | COMPLIANT |
| Reflow in nvim (manual) | S7.8 | MANUAL-OK |
| One of two shells exits | `an_unfocused_shell_exit_collapses_its_pane_and_keeps_focus` | COMPLIANT |
| Last shell exits | `the_last_shell_exiting_quits` | COMPLIANT |
| Quit leaves no orphan | `runtime::tests::shutdown_kills_every_child_in_parallel`, `shutdown_gives_up_at_the_bound`, `portable::tests::drop_leaves_no_orphan`, `drop_kills_background_jobs` | COMPLIANT |
| Terminal restored, No orphan shells after quit (manual, Kitty) | base slice; confirmed in Kitty by the user on 2026-10-11, recorded as S9d.1 | OK (W-2 RESOLVED in PR #50) |

Summary: 0 scenarios failing; 1 auto scenario without a project test (W-1); 3 inherited manual scenarios without a recorded confirmation in this change (W-2). Every other scenario has a passing covering test or a user-confirmed manual check.

## 4. Design coherence (ADRs)

| ADR | Honoured | Notes |
|---|---|---|
| 1-3 Encapsulation, `App::new(cols,rows)`, `PaneId(u32)` | Yes | accessors present; ids from `next_id` with `checked_add` (`allocator_panics_instead_of_wrapping`) |
| 4-6 Binary tree, integer weights, `MIN_PANE` | Yes | tile and split tests above |
| 7-10 Neighbour, resize step, zoom, close focus | Yes | all tested |
| 11-13, 28 Static prefix tree, SHIFT rule, `?`, hint | Yes | `PREFIX_TREE`, `hint` shared for root and group |
| 14-16 Mode model, Repeat rules, focus change exits COPY and RESIZE | Yes | |
| 17, 20 Events and effects, one tagged channel | Yes | |
| 18 Spawn failure | Yes | second layer (portable-pty home fallback) per the ADR text; see W-1 |
| 19 Kill | Yes | detached reapers, `QUIT_KILL_BOUND` shutdown |
| 21 cwd source and S9c precedence | Yes | `osc7.or(proc)`; vanished cwd order: launch cwd, absolute home, root; ADR 21 records OSC 7 as sticky and FQDN vs short hostname as a deferred follow-up |
| 22-23 Hand-written OSC 7, `libc::gethostname` | Yes | no new dependency; `local_hostname_matches_the_kernel_hostname` |
| 24-27, 29-32 Separator accent, tab placement and label, notice priority, COPY indicator, config | Yes | |

Spec and design were updated for the S9c precedence change and the hostname and OSC 7 decisions: the terminal-session spec (Vanished cwd falls back, Existing cwd is inherited unchanged, Unknown cwd falls back) and the tabs spec (New tab from a vanished cwd) carry S9c; ADR 21 and ADR 23 carry the hostname and sticky-OSC 7 decisions. No deviation between code and ADRs was found.

## 5. Strict TDD compliance

| Check | Result |
|---|---|
| Test runner available and executed | Yes (`cargo test`) |
| Tests exist for every code task | Yes; no code task lacks a test |
| TDD cycle evidence | Recorded in the PR bodies of S9a (#47), S9b (#48), S9c (#49) as tables with RED commit, failing assertion, GREEN commit and REFACTOR. Earlier slices (S1 to S8) have RED and GREEN task lines in `tasks.md` and PR descriptions but no per-test evidence table |
| RED honesty | S2b notes its initial RED was compile-only; behaviour coverage was confirmed by mutation review (41 of 45 killed, survivors fixed). S9 RED commits used compiling stubs with assertion failures |
| Triangulation | Layout, prefix, osc7 and config use table-driven or multi-case tests |
| Assertion quality | No tautologies, no ignored tests, no assertion-free tests found by scan; behavioural assertions use exact values |

## 6. Known and accepted limitations (recorded in the design)

- OSC 7 is sticky after ssh or exec (ADR 21).
- FQDN vs short hostname mismatch is rejected as remote (ADR 21 and 23); not an issue for a short hostname.
- A `chmod 000` directory passes `is_dir` and then fails the spawn through `SpawnFailed` (ADR 18).

None of these violates a spec scenario.

## 7. Issues

### CRITICAL

None.

### WARNING

- **W-1 (RESOLVED in PR #50: test `a_missing_cwd_starts_the_shell_in_home`, task S9d.2)**: Scenario "Bad cwd is not a failure [auto, unix PTY]" (terminal-session) has no project test. The behaviour is delegated to `portable-pty` (its unix `as_command` falls back to the home directory when the cwd is not a directory), and since S9c the app also avoids vanished cwds before spawning (tested). The behaviour is believed correct but there is no runtime evidence in this repo. A strict reading of the verify rules would classify an auto scenario without a covering test as CRITICAL; it is reported as WARNING because the behaviour is third-party and covered twice. The user decides. Suggested fix: one `portable::tests` case spawning `/bin/sh` with a non-existent cwd and HOME set, asserting `pwd`.
- **W-2 (RESOLVED in PR #50: the user confirmed all three in Kitty on 2026-10-11, task S9d.1)**: Three manual scenarios inherited from the base terminal slice appear in the delta specs but are not tied to any task of this change: "Real terminal reports NUL" (modal-input), "Terminal restored" and "No orphan shells after quit" (terminal-session). The Kitty checks confirmed for this change do not name them. The quit path is exercised in every manual session, but there is no explicit record.
- **W-3**: Per-test TDD evidence tables exist only from S9 onward; S1 to S8 rely on task lines and PR text (process finding, no code impact).
- **W-4**: The `sdd/panes-tabs/apply-progress` observation (#2733) is out of date against `tasks.md`: it still lists S7d.3, S8c.6 and S9.7 as open and S9c #49 as open, while `tasks.md` ticks them and the user confirmed them.

### SUGGESTION

- **S-1**: Add `S9c.4` (a relative home is never a spawn fallback) to `tasks.md`; the code and test exist.
- **S-2**: Refresh apply-progress #2733 so it matches `tasks.md` before archive.
- **S-3**: The S3b test `a_binding_in_a_group_returns_to_terminal_with_no_effect_yet` keeps a name from the S3b stage; rename it in a later cleanup.
- **S-4**: `drop_kills_background_jobs` is timing sensitive; consider a deadline-based wait to remove the flake.
- **S-5**: The ADR 21 FQDN follow-up could state the user-visible symptom (the tab label and statusline stay on the polled value).

## 8. Deferred / out of scope (explicitly excluded from this change)

- The `?` command viewer (the `?` entry is reserved; `PREFIX_TREE` is already its data source).
- Keymap configuration (the prefix tree stays a static table).
- Per-pane dirty tracking (a single `dirty` flag redraws everything).
- FQDN vs short hostname matching (ADR 21 follow-up).
- Clearing a sticky OSC 7 value after ssh or exec (ADR 21 limitation).
- The `config-menu` change (stacked on this tracker, separate change).

## 9. Verdict

**PASS WITH WARNINGS.** All 103 tasks are ticked and match the code, the full suite is green (513 of 513), clippy and fmt are clean, and every delta-spec scenario is covered by a passing test or a user-confirmed manual check, with the exceptions recorded as W-1 and W-2. The change can be archived once the user accepts W-1 and W-2, or asks for the small test in W-1.

