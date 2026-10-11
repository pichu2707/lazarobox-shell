# Verify Report: copy-selection

- Change: `copy-selection` (project lazarobox-shell)
- Verified: tracker tip `origin/feat/copy-selection` at `ade13e1` (merge of #55, #60, #61/#62, #63/#64, #65, #66)
- Mode: Strict TDD, hybrid store. Date: 2026-10-11

## Verdict

**PASS WITH WARNINGS.** CRITICAL 0, WARNING 3, SUGGESTION 4. Every automated scenario in the four deltas has at least one passing test. The 6 `[manual]` scenarios (VP.1-VP.6) are pending user checks in Kitty, not failures. The change can go to archive after the manual checks and the archive-time edits listed below.

## Build and test evidence

| Check | Command | Result |
|---|---|---|
| Tests | `cargo test` | 774 passed, 0 failed, 0 ignored |
| Lint | `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| Format | `cargo fmt --check` | exit 0 |
| Progression | apply-progress history: 705 -> 718 -> 735 -> 752 -> 763 -> 770 -> 774 | final count 774 matches the last recorded batch |

## Task completeness (tasks.md)

- CS1.1-1.19, CS2a.1-2a.19, CS2b.1-2b.14, CS3.1-3.18: all 70 implementation tasks are ticked `[x]`.
- Unticked: VP.1-VP.6 (`[manual]`, user-run in Kitty), VP.7 (verify prep; satisfied by this report), VP.8 (archive prep, see below).
- tasks.md has no entries for the post-plan slices CS4 (#65 review fixes) and CS5 (#66 held-key debounce). They are tracked only in apply-progress (see S1).

## Spec scenario coverage

### copy-selection (30 scenarios)

| Scenario | Covering tests |
|---|---|
| Start character selection | `core::copy::tests::v_starts_a_character_selection_at_the_cursor`; `ui::tests::the_statusline_shows_the_selection_marker_after_the_position` |
| Start line selection | `core::copy::tests::shift_v_starts_a_line_selection_at_the_cursor`; `ui::tests::the_alternate_screen_shows_the_line_marker_without_an_indicator` |
| Same key clears | `core::copy::tests::the_same_selection_key_again_clears` |
| Other key switches kind | `core::copy::tests::the_other_selection_key_switches_kind_and_keeps_the_anchor` |
| Motion extends backwards | `core::copy::tests::moving_left_extends_a_character_span_backwards`, `moving_past_the_anchor_reorders_the_span`, `span_constructor_orders_reversed_points` |
| Character selection within a row | `core::pane::tests::read_lines_includes_both_end_cells_and_reads_empty_cells_as_spaces`; `app::app_tests::yank_emits_the_clipboard_sequence_and_leaves_copy` |
| Multi-row line selection | `core::copy::tests::join_separates_logical_lines_with_a_newline`; `app::app_tests::a_line_selection_yanks_the_rows_joined_by_newline` |
| Trailing blank rows dropped | `core::copy::tests::join_of_nothing_is_empty_and_trailing_blank_rows_are_dropped`; `app::app_tests::a_line_yank_never_ends_in_a_newline` |
| Soft-wrapped rows | `core::copy::tests::join_glues_soft_wrapped_rows_without_a_separator`; `core::pane::tests::read_lines_reports_the_soft_wrap_flag_of_each_row` |
| Soft-wrapped row keeps trailing spaces | `core::copy::tests::join_keeps_trailing_spaces_of_a_soft_wrapped_row` |
| Trailing blanks trimmed | `core::copy::tests::join_trims_trailing_blanks_at_logical_line_ends_and_the_last_row` |
| Wide character once | `core::pane::tests::read_lines_emits_a_wide_character_once`; `app::app_tests::a_wide_character_is_yanked_once` |
| Selection spans scrollback | `core::pane::tests::read_lines_spans_history_and_screen_and_restores_the_offset`, `read_lines_pages_across_more_rows_than_the_pane_height`; `app::app_tests::yanking_from_scrollback_restores_the_bottom_viewport` |
| Yank emits OSC 52 | `core::copy::tests::encode_matches_the_rfc_4648_vectors`, `encode_handles_multibyte_utf8_and_high_bytes`, `osc52_wraps_the_base64_in_the_clipboard_sequence`; `app::app_tests::yank_emits_the_clipboard_sequence_and_leaves_copy`; `runtime::tests::set_clipboard_is_buffered_and_touches_nothing_else`, `flush_host_writes_the_exact_bytes_flushes_and_clears`, `flush_host_with_nothing_pending_writes_nothing`, `flush_on_exit_writes_pending_bytes_on_every_outcome` |
| Yank of blank rows | `app::app_tests::yanking_only_blank_rows_sends_nothing_and_stays_in_copy` (asserts notice `Nothing to copy`) |
| Yank without selection | `core::copy::tests::y_with_a_selection_yanks_and_without_is_ignored`; `app::app_tests::yank_without_a_selection_changes_nothing` |
| Paste elsewhere | [manual] VP.1 pending |
| Line yank runs nothing | [manual] VP.2 pending |
| Yank over ssh | [manual] VP.3 pending |
| At the cap | `core::copy::tests::osc52_accepts_exactly_the_cap_and_rejects_one_more_group`; `app::app_tests::a_selection_of_exactly_the_cap_is_sent` |
| Over the cap | `core::copy::tests::osc52_rejects_on_size_without_needing_valid_encoding_work`; `app::app_tests::an_oversized_selection_sends_nothing_and_stays_in_copy` |
| Oversized in a real terminal | [manual] VP.4 pending |
| Esc clears then exits | `core::copy::tests::esc_with_a_selection_clears_it_and_stays`; `app::app_tests::esc_clears_the_selection_then_leaves_copy_and_q_drops_it` |
| `q` drops the selection | `core::copy::tests::q_and_i_exit_even_with_a_selection_and_drop_it`; same app test as above |
| Resize drops it | `app::app_tests::a_host_resize_drops_the_selection_and_clamps_the_cursor`, `a_sibling_exit_that_resizes_the_focused_pane_drops_the_selection`; `core::pane::tests::line_generation_bumps_on_resize` |
| Screen switch drops it | `app::app_tests::a_screen_switch_drops_the_selection_and_keeps_copy`; `core::pane::tests::line_generation_bumps_on_each_screen_flip` |
| Screen switch resets the cursor | `app::app_tests::a_screen_round_trip_puts_the_cursor_back_on_the_live_cursor`; `a_resize_that_shortens_the_pane_clamps_the_cursor_line` |
| Focus change drops it | `app::app_tests::leaving_copy_by_a_focus_change_forgets_the_selection` |
| Output keeps the selection on its content | `app::app_tests::output_keeps_the_selection_on_its_content`; `core::pane::tests::first_line_counts_evictions_with_a_scrolled_view_and_keeps_vt100_pin` |
| Eviction of the anchor | `app::app_tests::eviction_clamps_the_anchor_and_keeps_the_selection` (now also asserts `copy_cursor()`), `copy_output_past_the_scrollback_cap_does_not_panic_and_stays_in_sync`; `core::pane::tests::first_line_counts_evictions_at_offset_zero`, `first_line_is_zero_below_the_cap`, `first_line_ignores_scrolls_that_stay_in_history`, `read_lines_skips_evicted_lines` |
| Character highlight | `ui::components::terminal_view::tests::a_character_selection_is_reversed_on_exactly_its_cells`, `a_wide_character_is_highlighted_from_either_of_its_cells`; `ui::tests::the_focused_pane_paints_the_selection_and_the_other_does_not` |
| Line highlight | `ui::components::terminal_view::tests::a_line_selection_reverses_the_whole_row` |
| No selection | `ui::components::terminal_view::tests::without_a_highlight_nothing_is_reversed`; `ui::tests::leaving_copy_removes_the_highlight` |
| (design) highlight follows scrolling / XOR | `terminal_view::tests::the_highlight_follows_the_top_line_of_the_viewport`, `the_highlight_xors_with_inverse_cells`; `ui::tests::the_highlight_lands_on_the_selected_rows_of_a_scrolled_viewport` |

### copy-mode-scrollback (17 scenarios incl. the delta-only ones)

| Scenario | Covering tests |
|---|---|
| Line motion | `core::copy::tests::k_on_the_top_visible_row_scrolls_the_viewport_one_line`, `j_below_the_bottom_row_scrolls_down_only_as_needed`; `app::app_tests::vertical_keys_move_the_cursor_and_scroll_only_at_the_edge` |
| Cursor moves inside the viewport | `core::copy::tests::vertical_motion_inside_the_viewport_leaves_the_offset` |
| Half page | `core::copy::tests::half_pages_move_cursor_and_offset_by_twelve`, `half_page_moves_at_least_one_row_on_a_tiny_pane` |
| Top and bottom | `core::copy::tests::double_g_goes_to_the_top`, `top_and_bottom_jump_with_the_cursor` |
| Clamping | `core::copy::tests::vertical_motion_clamps_at_the_first_and_last_line`, `half_pages_clamp_at_both_bounds`, `stale_offset_beyond_history_is_clamped_on_move`, `no_history_keeps_every_vertical_motion_at_zero` |
| Horizontal motion brings an off-screen cursor back | `core::copy::tests::a_cursor_scrolled_off_screen_returns_with_the_next_motion`; `app::app_tests::a_horizontal_motion_brings_an_off_screen_cursor_back` |
| Lone g | `core::copy::tests::lone_g_is_discarded_by_the_next_key` |
| Cursor starts at the live cursor | `core::pane::tests::live_cursor_ignores_the_scrollback_offset`, `live_cursor_is_none_when_the_child_hides_it`; `core::copy::tests::enter_puts_the_cursor_on_the_live_cursor`, `enter_clamps_the_live_cursor_into_the_pane`, `enter_with_a_hidden_cursor_uses_the_bottom_row_at_column_zero`; `app::app_tests::entering_copy_puts_the_cursor_on_the_childs_cursor`, `entering_copy_with_a_hidden_cursor_uses_the_bottom_row` |
| Horizontal motion | `core::copy::tests::horizontal_motions_follow_the_spec_sequence`, `horizontal_motions_clamp_at_both_ends`; `core::pane::tests::line_end_is_the_last_non_blank_column`; `app::app_tests::horizontal_keys_move_the_copy_cursor_and_dollar_uses_the_line_end` |
| End of a blank row | `core::copy::tests::line_end_on_a_blank_row_is_column_zero`; `app::app_tests::dollar_on_a_blank_row_goes_to_column_zero` |
| Block in COPY | `app::app_tests::cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit`, `child_shape_requests_during_copy_do_not_leak_until_exit` |
| Restore on exit | same two tests |
| Block follows the COPY cursor | `ui::tests::the_real_cursor_follows_the_copy_cursor_in_copy`, `the_copy_cursor_is_shown_even_when_the_child_hid_its_cursor`; `app::app_tests::the_copy_cursor_is_none_outside_copy_and_when_scrolled_off_screen` |
| Exit | `core::copy::tests::exit_keys_are_q_esc_and_i`; `app::app_tests::copy_exit_returns_to_terminal_at_the_bottom` |
| Esc with selection / `q` with selection | `core::copy::tests::esc_with_a_selection_clears_it_and_stays`, `q_and_i_exit_even_with_a_selection_and_drop_it`; `app::app_tests::esc_clears_the_selection_then_leaves_copy_and_q_drops_it` |
| Real session | [manual] VP.5 pending |

Unchanged original scenarios of the capability (Isolation, Stable viewport, Alternate screen, Per-pane state, Position indicator, Focus change) are still covered by the pre-existing, still-green tests (`copy_swallows_keys`, `output_during_copy_keeps_the_same_content_visible`, `copy_on_the_alternate_screen_stays_at_zero`, `copy_is_per_pane_and_other_panes_keep_running`, `the_copy_position_*`, `the_focused_pane_exiting_in_copy_returns_to_a_live_terminal`, `a_tab_change_exits_resize_and_copy`).

### modal-input (15 scenarios)

| Scenario | Covering tests |
|---|---|
| Passthrough / Input goes to the focused pane only | `app::app_tests::keys_and_paste_go_to_the_focused_pane_only`, `repeat_in_terminal_writes_the_encoded_bytes` |
| Repeat ignored in decision states | `repeat_in_prefix_does_nothing_and_stays_in_prefix`, `repeat_is_ignored_in_group_and_keeps_the_group_pending`, `repeat_of_y_in_confirm_quit_does_not_quit` |
| Release ignored | `release_key_events_are_ignored_in_every_mode`, `release_is_ignored_in_group`, `release_is_ignored_in_resize` |
| Repeat in MENU | `a_held_enter_or_esc_does_nothing_but_a_held_j_moves` |
| Repeat of selection keys in COPY | `core::copy::tests::repeat_of_selection_and_exit_keys_changes_nothing`; `app::app_tests::repeat_of_one_shot_keys_changes_nothing_in_copy` |
| Repeat of motions in COPY | `core::copy::tests::repeat_of_motions_moves_like_press`; `app::app_tests::repeat_of_j_moves_the_copy_cursor_one_row`, `repeat_of_k_in_copy_moves_the_view` |
| Held exit key does not leak into TERMINAL | `a_held_q_or_i_does_not_leak_into_terminal_after_leaving_copy`, `a_held_esc_does_not_leak_into_terminal_after_leaving_copy`, `a_held_y_does_not_leak_into_terminal_after_a_yank`, `a_release_of_the_exit_key_ends_the_debounce`, `another_key_after_leaving_copy_is_passed_through` |
| Held exit key sent as Presses is dropped within the window | `a_held_exit_key_sent_as_presses_is_dropped_for_the_whole_hold`, `a_held_yank_key_sent_as_presses_is_dropped_and_keeps_the_notice`, `a_long_hold_keeps_being_dropped_past_the_window_from_the_exit` |
| Deliberate press after the window passes through | `a_long_hold_keeps_being_dropped_past_the_window_from_the_exit`, `a_held_exit_key_sent_as_presses_is_dropped_for_the_whole_hold` (600 ms case) |
| Another key ends the debounce | `another_key_ends_the_debounce`, `the_debounce_needs_the_identical_key_and_modifiers` |
| A paste ends the debounce | `a_paste_ends_the_debounce` |
| Window boundary and refresh | `a_long_hold_keeps_being_dropped_past_the_window_from_the_exit` (500 dropped / 501 passes; 499, 998 dropped / 1499 passes, per apply-progress) |
| A yank that stays in COPY does not arm the debounce | `a_yank_that_stays_in_copy_does_not_arm_the_debounce` |

Remaining original modal-input scenarios (Prefix, Quit, Enter COPY, Paste/resize, Prefix tree, Group cancellation, Close confirmations, RESIZE, Focus change, MENU) are outside the MODIFIED block and their existing tests are green.

### statusline (10 scenarios)

| Scenario | Covering tests |
|---|---|
| Marker with indicator | `ui::components::statusline::tests::the_marker_follows_the_position_indicator`, `no_marker_leaves_the_indicator_alone`; `ui::tests::the_statusline_shows_the_selection_marker_after_the_position`; `the_label_stays_copy_with_a_selection` |
| Line marker without history | `statusline::tests::the_line_marker_follows_the_label_without_history`; `ui::tests::the_alternate_screen_shows_the_line_marker_without_an_indicator` |
| Marker gone | `statusline::tests::no_marker_leaves_the_indicator_alone`; `core::copy::tests::esc_with_a_selection_clears_it_and_stays` |
| Narrow width | `statusline::tests::a_narrow_statusline_with_a_marker_never_panics`, `the_path_budget_accounts_for_the_marker`, `the_marker_sits_before_the_zoom_indicator` |
| Copied notice | `app::app_tests::the_copied_notice_counts_characters_and_clears_on_the_next_key` |
| Multi-row count | same test (`foo\nbar` -> 7) |
| Singular notice | same test |
| Too-large notice | `an_oversized_selection_sends_nothing_and_stays_in_copy` |
| Nothing-to-copy notice | `yanking_only_blank_rows_sends_nothing_and_stays_in_copy` |
| Notice cleared by next key | `the_copied_notice_counts_characters_and_clears_on_the_next_key`, `a_newer_notice_replaces_the_copied_notice`, `the_notice_lasts_until_the_next_press_or_repeat` |

Result: no automated scenario is uncovered. Pending manual checks: VP.1, VP.2, VP.3, VP.4, VP.5, VP.6 (tmux `set-clipboard`).

## MODIFIED-block completeness vs `openspec/specs/<cap>/spec.md`

- `copy-mode-scrollback` Navigation: all 5 original scenarios (Line motion, Half page, Top and bottom, Clamping, Lone g) present, reworded for the cursor model. Cursor in COPY: Block in COPY and Restore on exit present. Exit COPY: Exit and Real session present. OK.
- `modal-input` TERMINAL mode: all 5 original scenarios (Passthrough, Repeat ignored in decision states, Release ignored, Input goes to the focused pane only, Repeat in MENU) present. OK.
- `statusline`: ADDED only, so nothing to compare.
- Both MODIFIED blocks carry full replacement text, with `(Previously: ...)` lines.

## Design ADR check (CS-1..CS-12) against code

Implemented as designed: CS-1 (stable line numbers, `first_line`), CS-2 (probe in `Pane::feed`), CS-3 (cursor, motions, edge scrolling), CS-6 (`read_lines` plus pure `join`), CS-7 (hand-written `encode`), CS-8 (`osc52`, BEL, pre-encode cap check), CS-9 (`host_out` buffered, flushed in the tick), CS-11 (XOR REVERSED), CS-12 (label stays `COPY`, marker after the indicator).

Deviations (all intentional, made in review slices CS4/CS5, reflected in the spec deltas but NOT in design.md):

| ADR | Design text | Code now | Source |
|---|---|---|---|
| CS-4 | Repeat dropped for `v V y Esc q i`; nothing about leaving COPY | `App.held_exit` drops Repeat of the exit key in TERMINAL until Release or another Press; 500 ms same-key (code + modifiers) Press debounce (`HELD_EXIT_WINDOW`, injected clock `App::with_clock`); paste ends it; failed yanks do not arm it | #65, #66 |
| CS-5 | Resize drops the selection only; cursor clamped | `sync_copy` on a `line_generation` change re-enters COPY (selection dropped, cursor on the live cursor, offset 0). Resize reaches this through the generation bump | #65 |
| CS-3 | Only vertical motions run `ensure_visible` | any motion, including `h l 0 $`, brings an off-screen cursor back | #65 |
| CS-6/CS-10 | Trailing blank cells trimmed; no trailing `\n` for the last row | trailing blank rows are also dropped (`foo\n\nbar`, never `\n` at the end) | #65 |
| CS-10 | Yank always sends and shows `Copied N characters` | empty text sends nothing, keeps COPY and the selection, notice `Nothing to copy` | #65 |
| CS-9 | Flush in the tick only | also `flush_on_exit` on every loop outcome (Quit, shutdown, channel closed, error) | #65 |
| README | n/a | documents the 75 000-byte limit and the tmux `set-clipboard` requirement | #65 |

Review findings (engram 2913, 2914, 2916) status:
- 2913.1 held q/Esc leak: fixed (`held_exit`), and 2916 showed Repeat is not delivered without Kitty flags, so #66 added the time-window debounce. Tests present.
- 2913.2 `h/l/0/$` not bringing the cursor back: fixed (`a_horizontal_motion_brings_an_off_screen_cursor_back`).
- 2913.3 `copy_cursor` mutation survivor: fixed (the eviction test asserts `copy_cursor() == Some((0, 3))`).
- 2913.4 alt-screen return leaves cursor off screen: fixed (`a_screen_round_trip_puts_the_cursor_back_on_the_live_cursor`).
- 2914.1 empty yank clobbers the clipboard: fixed (`Nothing to copy`). 2914.2 OSC lost on Quit: fixed (`flush_on_exit`). 2914.3 trailing newline: fixed (`a_line_yank_never_ends_in_a_newline`). 2914.4 `copy_highlight` formula survivor: covered by `ui::tests::the_highlight_lands_on_the_selected_rows_of_a_scrolled_viewport`.

## Findings

### CRITICAL
None.

### WARNING
- **W1: design.md is stale.** The ADR table (CS-3, CS-4, CS-5, CS-6, CS-9, CS-10) still describes the pre-review behaviour; the held-exit debounce, the cursor reset on generation change, the empty-yank rule, the blank-row rule and `flush_on_exit` exist only in the deltas, code and apply-progress. Archive will carry a design that disagrees with the code unless it is updated or annotated.
- **W2: manual checks pending.** VP.1-VP.6 (Paste elsewhere, Line yank runs nothing, Yank over ssh, Oversized in a real terminal, Real session, tmux `set-clipboard`) have not been run. These are the only end-to-end proofs of OSC 52 delivery and of the held-key debounce on a real terminal.
- **W3: TDD evidence is prose only.** apply-progress records RED-first and mutation checks in sentences (for example the `<=` to `<` and always-arm mutations in #66), but has no per-task TDD cycle table (RED / GREEN / REFACTOR per task). Strict-TDD ordering could not be verified from artifacts; this report did not re-run mutation testing. The CS2a/CS2b/CS3 RED-first claims rest on the tasks.md plan and apply-progress.

### SUGGESTION
- **S1:** Add CS4 and CS5 sections to tasks.md (or note them in the archive report), and extend the coverage table with the scenarios added after planning: Trailing blank rows dropped, Yank of blank rows, Nothing-to-copy notice, Screen switch resets the cursor, Horizontal motion brings an off-screen cursor back, and the 7 new held-exit scenarios.
- **S2:** Add a manual check VP.9: hold `q` / `y` / Esc in a real Kitty session to confirm that nothing reaches the shell (the 500 ms window assumes OS auto-repeat arrives as Presses).
- **S3:** Documented tradeoff to keep visible in user docs: a deliberate retype of the exit key within 500 ms of leaving COPY is dropped.
- **S4:** The OSC 52 and tmux limitation (design Risks) belongs in user docs; the README already covers it, so VP.6 only needs to record the result.

## Archive-time actions

1. **Hand-edit the main spec Purpose line** in `openspec/specs/copy-mode-scrollback/spec.md` (VP.8). Replace "Vim-style read-only navigation of ~10k lines of scrollback. Selection/yank is out of scope. All scenarios [auto] unless tagged." with a Purpose that reads "Vim-style navigation of ~10k lines of scrollback with character/line selection and yank (see copy-selection)." (keep "All scenarios [auto] unless tagged."). A MODIFIED block cannot carry the Purpose.
2. Merge the new `copy-selection` capability and the ADDED statusline requirements into `openspec/specs/`, and replace the MODIFIED requirements (modal-input TERMINAL mode; copy-mode-scrollback Navigation, Cursor in COPY, Exit COPY) in place.
3. Decide on W1 (update design.md or annotate) before archiving; tick or annotate VP.1-VP.8 in tasks.md.

## Next recommended

Run the manual checks VP.1-VP.6 (and S2), then `sdd-archive` once W1 is resolved or accepted. No `sdd-apply` rework is needed.

## Update 2026-10-11: manual-check ledger

The original verdict (PASS WITH WARNINGS) and the findings above are kept as written at verification time. This section records what happened afterwards.

| Check | Result | Evidence |
|---|---|---|
| VP.1 Paste elsewhere | PASS | Clipboard bytes verified with `xclip`: exactly `hola`. |
| VP.2 Line yank runs nothing | PASS | Clipboard `prueba-linea`, 12 bytes, no trailing newline; pasted at a zsh prompt without running. |
| VP.3 Yank over ssh | DEFERRED by the user | No remote install yet; to be done later on a remote host. |
| VP.4 Oversized | PASS | Notice `Selection too large to copy`; stayed in COPY with the selection; clipboard unchanged. |
| VP.5 Real session | PASS | Run in a real Kitty session. |
| VP.6 tmux `set-clipboard` | DEFERRED / not applicable | The user does not use tmux; the README documents the setting. |
| VP.9 Held `q` / `y` / Esc in real Kitty (new, from S2) | PASS | Nothing leaks into the prompt; a deliberate `q` after about 1 s is typed. |

Note: a reported bracketed-paste marker turned out to be a stale lazarobox session running an old, deleted binary. A fresh session pastes cleanly, and a pty harness confirmed the paste path. No code change.

### Findings status after the update

- **W1 (design.md stale): resolved** by this PR, which syncs the ADR table with the delivered behaviour.
- **W2 (manual checks pending): resolved except VP.3 and VP.6**, which are deferred (see above).
- **W3 (TDD evidence is prose only): unchanged.**
- **S1: addressed** by this PR (tasks.md gets CS4 and CS5 and the extended coverage table). **S2: addressed** by VP.9. S3 and S4 unchanged.

Next recommended: `sdd-archive` once the deferred VP.3 and VP.6 are accepted as deferred.
