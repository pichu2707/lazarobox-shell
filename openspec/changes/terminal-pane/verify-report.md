# Verification Report: terminal-pane

Change: terminal-pane (Slice 1). Mode: Strict TDD (cargo test), hybrid store. Branch feat/terminal-pane-05-cwd (tip c6eadc8).

## Verdict: PASS WITH WARNINGS (0 CRITICAL, 3 WARNING, 4 SUGGESTION)

## Completeness
All tasks in tasks.md are [x] (1.1-1.8, 2.1-2.4, 3.1-3.9, 4.1-4.8, 5.1-5.6, R.1-R.4). Manual tasks 1.4, 4.8, 5.5 are recorded as passed in Kitty (tasks.md notes). 0 incomplete.

## Execution evidence
- `cargo test` x3: 209 passed, 0 failed, 0 ignored each run (3.0 s; PTY tests stable). apply-progress reports 201: +8 tests came from later review fixes (documented in 4.x notes); not an inconsistency of substance.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- Existing snapshots: git diff vs main shows only 6 NEW .snap files (33 insertions, 0 modifications/deletions). `snapshot_normal_mode` untouched -> "Existing snapshots" scenario satisfied.
- Working tree: only design.md modified (user-owned, not touched).

## TDD compliance (strict)
| Check | Result | Details |
|---|---|---|
| TDD evidence in apply-progress | PASS (partial form) | PR 5 table present; PR 1-4 recorded as narrative with review fixes, no per-task table |
| Tests exist for each task | PASS | all test files exist |
| GREEN confirmed | PASS | 209/209 pass now |
| Triangulation | PASS | multi-case tests (keys, decscusr, copy, prefix) |
| RED for 5.1/5.2 | N/A, documented | behavior pre-existed (vt100 pinning); test passed first run, disclosed |

Assertion quality: scanned for tautologies/ghost loops. `resize_emits_resize_pty_in_every_mode` loops over a fixed 4-element array (non-empty, OK). No tautologies found. `restore_resets_the_cursor_and_bracketed_paste` uses `contains` (weak, see W1).
Test layers: unit/in-process (TestBackend+insta snapshots, fake PTY) plus real unix-PTY integration tests (portable.rs, runtime cd test). No coverage tool configured (skipped, informational).

## Spec compliance matrix (scenario -> covering test, all passed at runtime)

### terminal-session (4 req / 12 scn; 10 auto, 3 manual counted below) -> 4/4 req, 12/12 scn
- Spawn env: SHELL set -> `core::pty::tests::spawn_spec_uses_shell_cwd_and_terminal_env`, `spawn_spec_honours_another_shell`. SHELL unset/empty -> `spawn_spec_falls_back_to_sh_when_shell_is_unset`, `..._is_empty`.
- PTY I/O echo -> `core::pty::portable::tests::echo_round_trip` (+ fake `records_writes_in_order`, pump_tests).
- Resize propagates 39x100 -> `app::app_tests::resize_emits_resize_pty_in_every_mode`, `resize_resizes_the_emulator_too`, `portable::tests::stty_size_reflects_a_resize`. Degenerate -> `pane_size_reserves_the_statusline_row_with_a_1x1_minimum`. Reflow in nvim -> [manual] task 4.8.
- Shell exits -> `app_tests::pty_exit_quits`, `portable::tests::exit_is_reported_as_exited`, `exited_is_emitted_exactly_once`. No orphan -> `portable::tests::drop_leaves_no_orphan`, `drop_kills_background_jobs`. Terminal restored -> [manual] 4.8/5.5 plus `runtime::tests::restore_resets_the_cursor_and_bracketed_paste`, `sigterm_and_sighup_request_shutdown`.
- No mouse capture -> [manual] 4.8 native selection (no capture code present).

### terminal-emulation (4 req / 20 scn) -> 4/4 req, 20/20 scn
- Colors/text -> `pane::tests::sgr_colors_and_attributes_are_mapped`, `printed_text_lands_in_consecutive_cells`, `truecolor_and_256_colors_are_mapped`. Mode tracking -> `modes_track_application_cursor_and_bracketed_paste`.
- DA1 -> `da1_is_answered`; DSR 5n -> `dsr_5n_reports_ok`; DSR 6n -> `dsr_6n_reports_one_based_cursor_position`; split -> `query_split_across_chunks_replies_exactly_once`.
- DECSCUSR: bar steady `decscusr_6_is_steady_bar`; bar blinking `decscusr_5_is_blinking_bar`; block steady `decscusr_2_is_steady_block`; default `decscusr_0_and_missing_ps_reset_to_default`; unknown `decscusr_unknown_ps_is_ignored`; split `decscusr_split_across_chunks_applies_once_complete`; no reply `decscusr_never_produces_reply_bytes`; mapping `decscusr_all_known_values_map_per_spec`.
- Restore on exit -> `runtime::tests::restore_resets_the_cursor_and_bracketed_paste`, `cursor_style_is_applied_on_change`, `cursor_style_is_not_reapplied_when_unchanged`, `every_shape_maps_to_its_decscusr_code` (PARTIAL on ordering, see W1). Cursor in nvim -> [manual] 1.4/4.8.
- View: snapshot -> `terminal_view::tests::snapshot_styled_screen`, `maps_colors_and_attributes`, `wide_char_is_drawn_once_and_the_next_cell_is_skipped`; hidden cursor -> `hidden_cursor_has_no_position`; real apps -> [manual] 1.4/4.8. Default colors from theme -> `default_background_comes_from_the_theme` (bg only, see W2).

### key-encoding (3 req / 7 scn) -> 3/3, 7/7
- Basics -> `keys::tests::printable_chars_are_utf8`, `ctrl_letters_map_to_control_codes`, `alt_prefixes_escape`, `simple_keys`. Normal arrows -> `arrows_normal_mode`; application -> `arrows_application_mode`; nvim arrows -> [manual] 1.4/4.8. Paste enabled -> `paste_bracketed_wraps_text`; disabled and multi-line -> `paste_unbracketed_normalizes_newlines_to_cr`; marker stripping -> `bracketed_paste_strips_embedded_end_marker`, `..._deeply_nested_reassembly`. F-keys/nav -> `function_keys`, `navigation_keys_normal`.

### modal-input (5 req / 9 scn + repeat/release rules) -> 5/5, 9/9
- Passthrough -> `terminal_passes_keys_to_the_pty`, `terminal_encodes_with_the_pane_modes`. Enter PREFIX -> `prefix_key_enters_prefix_without_writing`. Literal -> `prefix_twice_sends_a_literal_nul`. Cancel/unknown -> `prefix_cancel_and_unmapped_keys_are_swallowed`. Real NUL in Kitty -> [manual] 1.4 (verified via real pty) and 4.8 "Ctrl+Space twice".
- Quit: prompt -> `prefix_q_asks_for_confirmation`; confirm -> `confirm_quit_y_quits`; decline -> `confirm_quit_declined_by_n_esc_or_other_keys`; uppercase Y -> `confirm_quit_uppercase_y_declines`.
- Enter COPY -> `prefix_open_bracket_enters_copy_without_writing`. Paste/resize -> `paste_goes_to_the_pty_only_in_terminal`, `resize_emits_resize_pty_in_every_mode`.
- Release/Repeat rules -> `release_key_events_are_ignored_in_every_mode`, `repeat_in_terminal_writes_the_encoded_bytes`, `repeat_of_k_in_copy_moves_the_view`, `repeat_in_prefix_does_nothing_and_stays_in_prefix`, `repeat_of_y_in_confirm_quit_does_not_quit`, `held_prefix_key_enters_prefix_once_and_repeats_are_dropped`. Data-driven table -> `prefix::tests::binding_keys_and_actions_are_unique`, `every_binding_is_reachable_through_lookup`.

### copy-mode-scrollback (6 req / 13 scn) -> 6/6, 13/13
- Line motion -> `copy::tests::line_motions_move_one_row_and_return`; half page -> `half_page_moves_half_the_pane_height`; top/bottom -> `double_g_goes_to_the_top`, `top_and_bottom_jump_to_the_bounds`; clamping -> `motions_clamp_at_the_top`, `motions_clamp_at_the_bottom`; lone g -> `lone_g_is_discarded_by_the_next_key`.
- Keys swallowed -> `app_tests::copy_swallows_keys`.
- Output during COPY -> `app_tests::output_during_copy_keeps_the_same_content_visible`, `copy_offset_follows_the_pane_when_output_arrives_while_scrolled`, `pane::tests::scrolled_view_stays_anchored_when_new_output_arrives`.
- Alt screen: enter -> `entering_copy_on_the_alternate_screen_shows_the_current_screen`; clamped -> `copy_on_the_alternate_screen_stays_at_zero`, `pane::tests::alternate_screen_has_no_scrollback_and_main_history_survives`.
- Cursor: block in COPY / restore -> `cursor_shape_is_a_steady_block_in_copy_and_restores_on_exit`, `child_shape_requests_during_copy_do_not_leak_until_exit`.
- Exit -> `copy_exit_returns_to_terminal_at_the_bottom`, `copy::tests::exit_keys_are_q_esc_and_i`. Real session -> [manual] 5.5.

### statusline (4 req / 9 scn) -> 4/4, 9/9
- Label/color snapshots -> `statusline::tests::snapshot_input_{terminal,prefix,copy,confirm_quit}`, `input_label_is_rendered_per_mode`, `input_block_uses_the_input_mode_color_and_bold`. Color mapping -> `theme::tests::input_mode_style_maps_each_mode_to_its_color_with_base_fg`, `input_mode_colors_follow_the_theme`. Existing snapshots -> `snapshot_normal_mode` unchanged (git diff).
- Prompt -> `snapshot_input_confirm_quit`, `ui::tests::statusline_follows_the_input_mode`.
- cwd changes -> `runtime::tests::cd_in_a_real_shell_is_seen_by_the_poll` (+ manual 5.5 `/tmp`); read failure -> `runtime::tests::cwd_poll_yields_nothing_without_a_pid_or_when_the_lookup_fails`, `app_tests::cwd_is_retained_when_no_new_cwd_arrives`, `unchanged_cwd_does_not_mark_dirty`; `~` -> `home_is_shown_as_a_tilde`, `a_root_or_missing_home_never_produces_a_tilde`.
- Shell name -> `ui::tests::statusline_shows_the_cwd_and_the_shell_name`, `app_tests::shell_name_is_the_basename_of_the_shell_with_an_sh_fallback`. Narrow -> `snapshot_input_narrow`, `narrow_width_keeps_the_mode_block_and_drops_the_right_segment`, `degenerate_widths_do_not_panic`.

Totals: 26 requirements / 70 scenarios (incl. 10 [manual]); all covered, 0 untested, 0 failing. Manual ones are covered by tasks 1.4, 4.8, 5.5 (user, Kitty, 2026-10-06).

## Deviation review
| Deviation | Verdict |
|---|---|
| ADR 11: anchoring via vt100 pinning + `sync_copy_offset`, no `scrollback_len` delta | ACCEPTABLE. Requirement "stable viewport" is met and asserted on visible cell content (`output_during_copy_keeps_the_same_content_visible`, `scrolled_view_stays_anchored_...`). Task 5.2 text still says "delta"; the note records the change. |
| Default fg = Color::Reset (bg = theme.bg_base) | ACCEPTABLE with warning W2: theme has no fg colour, so Reset is the only sane choice; spec wording "default colors from theme" is only half literal. |
| `AppEvent::Cwd` deferred to PR 5 | ACCEPTABLE; delivered in PR 5 (tests above). |
| Repeat-key handling | ACCEPTABLE; spec was amended to match and 6 tests cover it. |
| Quit accepts only lowercase `y` | ACCEPTABLE; now written into the spec; tested. |
| Process-group jobs under job control not killed | ACCEPTABLE limitation (W3); orphan scenario holds for the shell and its group. |
| DRAIN_GRACE best-effort | ACCEPTABLE limitation; trailing output may be dropped on quit under huge backlog. |
| Legacy key encoding limits | ACCEPTABLE; pinned by `documented_limitations_are_pinned`; spec forbids Kitty flags. |

## Findings
### CRITICAL
None.

### WARNING
- W1. Spec "Restore on exit" says DefaultUserShape is emitted AFTER leaving the alternate screen. Implementation (`runtime.rs` `restore()` / panic hook) emits it BEFORE `ratatui::restore()`, and the test only uses `contains`, so ordering is neither implemented nor asserted. Cursor style is terminal-global in Kitty and the manual check confirmed the cursor is restored, so the user impact is nil; either fix the order/test or relax the spec wording.
- W2. Default foreground is `Color::Reset`, not from the theme (spec: "default colors from the theme"). Only the background comes from the theme (test covers bg only). Amend the spec or add a theme fg.
- W3. Background jobs in their own process group (job control) survive teardown (accepted limitation, documented in 4.8 note (b)); consider recording it in the spec as a limitation.

### SUGGESTION
- S1. tasks.md 5.2 text still mentions "anchor offset by scrollback_len() delta"; the DONE note explains, but the spec/design should be synced at archive (design.md ADR 11).
- S2. apply-progress lacks a per-task TDD Cycle Evidence table for PR 1-4 (only PR 5 has one); the test count there (201) is stale vs 209.
- S3. The panic-hook restore path has no direct test (shares `restore_to`, which is tested).
- S4. No coverage tool configured (cargo-llvm-cov) so changed-file coverage was not measured.

## next_recommended
sdd-archive (no CRITICAL). Optionally resolve W1/W2 by syncing spec wording first.
