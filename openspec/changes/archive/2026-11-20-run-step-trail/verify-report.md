# Verify Report: run-step-trail

Branch: docs/archive-run-step-trail (main + task checkbox updates). Mode: Strict TDD, artifact store openspec.

## Results
- `cargo test`: 11718 lib passed, 0 failed, 2 ignored; integration/other binaries 8+15+26+3+2+12+1+1 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean.
- Tasks: all checked; match merged code (trail.rs, run_store.rs, orchestra.rs, chat_store/runs).

## Spec coverage (requirement -> covering tests)
- Step lines: trail::tests::a_call_and_its_result_are_one_line_with_the_outcome, a_refused_call_keeps_its_reason_and_an_error_its_text, a_long_call_keeps_its_outcome, a_handoff_keeps_the_step_and_the_verdict.
- Cap and cut note: past_the_cap_the_oldest_go_and_the_count_is_kept, shown_lists_the_last_steps_and_counts_the_rest.
- Cleaning/redaction: secrets_in_a_call_or_its_result_are_scrubbed, a_handoff_reason_is_scrubbed, text_is_one_clean_line_without_tokens_and_cut.
- Storage/survival: the_text_round_trips_with_its_cut_count; chat_store::runs::tests::a_trail_is_stored_with_the_run_and_a_save_without_one_keeps_it, an_interrupted_run_comes_back_with_its_trail, a_database_whose_runs_lack_the_trail_column_gains_it_and_keeps_its_rows.
- Where shown: orchestra_trail::a_stopped_run_shows_the_steps_it_took, an_engine_error_with_the_run_open_shows_the_trail, a_stopped_run_shows_its_trail_once_when_it_ends_after, a_clean_finish_shows_its_trail_and_saves_it, the_end_of_a_replaced_run_does_not_show_its_trail_in_the_new_one, a_cancelled_run_shows_its_trail, an_interrupted_run_names_its_last_steps_after_a_restart.
- Failed save: a_failed_save_says_the_trail_may_be_missing, steps_of_a_run_that_never_started_are_not_dropped_silently.
- Display only: record::tests::the_resume_message_never_holds_the_trail, orchestra_resume::the_stored_trail_never_reaches_a_model_request.
- Resumed carry: a_resumed_run_that_stops_lists_the_old_steps_the_separator_and_its_own, carry_adds_the_cut_counts_and_drops_the_oldest_first, carry_puts_old_lines_then_the_separator_then_the_new_ones, a_predecessor_with_no_trail_gives_no_separator, an_empty_predecessor_adds_no_separator, a_chain_of_resumes_*.
- Worker lines: a_failed_worker_call_survives_a_restart, a_refused_worker_call_stores_its_fixed_reason.
- Generated text only: a_hostile_worker_result_is_not_stored, trail::tests::a_hostile_result_stores_none_of_its_text, every_fixed_reason_maps_to_its_label_*, the_trail_labels_follow_the_apps_refusal_texts (app and host).
- Worker share: worker_lines_never_evict_coordinator_lines_and_cuts_count_apart, the_cut_note_counts_dropped_lines_of_both_shares.
- Report 12+4: a_report_lists_the_last_twelve_coordinator_and_four_worker_lines, without_worker_lines_a_report_is_unchanged.
- Older reader: a_legacy_parse_of_an_eighty_line_text_gives_a_correct_total, a_stored_line_with_detail_or_a_cut_look_alike_round_trips.

## CRITICAL
None.

## WARNING
- Scenario "Predecessor row unchanged" has no test named for it. It holds by design (resume saves only the new row; the supersede path closes the old row without touching its trail, see runs::tests::a_run_that_supersedes_closes_the_old_row_only_while_it_is_interrupted), but no assertion compares the predecessor's stored trail after the new run saves.
- Scenario "Survives another restart" for a resumed run is covered only indirectly (a_chain_of_resumes_*, round-trip tests); no single end-to-end test reads a resumed row from the store.

## SUGGESTION
- Add a store-level test asserting the predecessor's `trail` is byte-equal after the successor saves.
- Spec notes the 180-byte code cap vs 160 in the proposal; align the proposal text when archiving.

## Verdict
PASS WITH WARNINGS (0 CRITICAL, 2 WARNING, 2 SUGGESTION). Ready for sdd-archive.
