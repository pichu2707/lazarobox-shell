# Verify report: gsc-lazarobox-client

Mode: Strict TDD (`cargo test`). Verified at `origin/main` 3bc82e08 (PR1 #737, PR2 #739, PR3 #740 merged), 2026-11-13.

## Verdict

READY TO ARCHIVE. 0 CRITICAL, 2 WARNING (1 fixed in this commit), 3 SUGGESTION. Owner-gated items are PENDING-OWNER, not failures.

## Execution (umask 022)

| Check | Result |
|---|---|
| `cargo test` | 11205 passed, 0 failed, 1 ignored (lib); all other targets ok |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| build folder `target` | removed afterwards (AGENTS.md) |

## Tasks vs code

Completed tasks 1.1-1.9, 2.1-2.8, 2.10-2.12, 3.1-3.3 match the code (modules, constants, tests present). Open: 2.9, 2.13, 2.14, 4.x (PENDING-OWNER). `PROJECT_NUMBER` is `None` in `lazarobox_client.rs` line 11; the route ships disabled with its notice, as designed.

## Scenario matrix

lazarobox-google-client:
- Link fetches, twice, before `uv`: `fetch_tests::the_cache_path_fetches_on_every_link_before_any_check`
- Nothing else fetches: `fetch_tests::nothing_but_link_reaches_the_fetch` (source walk, verified to fail with a dummy caller) plus the empty-PATH own-path proof in the same file. Source check, not an effect-level run of every UI path (S1).
- Command line fixed / plain HTTP never: `hf::tests::the_google_client_request_is_the_fixed_command_line`
- Redirect is a failure: `hf::tests::a_google_client_redirect_comes_back_as_its_status`, `lazarobox_client::tests::other_statuses_are_failures_and_redirects_are_named`
- Oversize: `lazarobox_client::tests::a_body_over_16_kib_is_refused_without_parsing`, `google_client::tests::an_oversize_answer_says_larger_than_16_kib_and_is_never_accepted`, `hf::tests::a_google_client_reply_without_a_length_is_still_capped`
- Not a Desktop client: `a_body_that_is_not_a_desktop_client_is_refused`
- Matching id / foreign project / longer number: `an_id_of_the_pinned_project_is_accepted`, `a_foreign_project_is_refused`, `a_longer_number_that_starts_with_the_pin_is_refused`, `a_client_of_another_project_is_refused`
- Pin not filled: `without_a_pin_the_answer_is_not_configured_before_anything_else`, `google_client::tests::without_a_pin_no_curl_runs_and_the_notice_says_so`, `the_pin_constant_is_digits_only`
- Cache written private: `secrets_write::google::tests::the_cache_is_saved_0600_and_no_other_file_changes`
- Symlink or loose file refused: `a_cache_that_is_loose_linked_or_a_symlink_is_refused_without_the_secret`, `a_loose_linked_or_symlinked_cache_is_refused_with_the_cache_name`
- Failure leaves nothing: shared writer test `secrets_write::tests::a_failed_step_leaves_the_original_and_no_temporary_file` (not cache-specific, S2). Identical bytes skip: `identical_existing_bytes_skip_the_write` (shared).
- Network down, cache valid: `fetch_tests::a_failed_fetch_with_a_saved_copy_links_and_carries_the_notice`, `google_client::tests::a_refused_answer_keeps_a_valid_cache_and_says_why`
- Network down, no cache: `fetch_tests::a_failed_fetch_without_a_cache_stops_before_any_check`, `connectors_client_tests::a_link_failure_is_the_footer_notice_and_offers_the_own_client`
- Mismatch does not poison the cache: `google_client::tests::a_refused_answer_keeps_a_valid_cache_and_says_why`
- Each cause has a notice / missing curl: `google_client::tests::an_unreachable_host_and_a_missing_curl_have_their_notice`, `a_refused_answer_without_a_cache_fails_and_writes_nothing`, `lazarobox_client::tests::each_notice_is_the_design_text`, `no_notice_holds_the_secret_or_the_body`
- Footer draws instruction and cause: `ui::components::menu::tests::a_link_failure_footer_draws_its_instruction_whole`
- Own-client user untouched: `fetch_tests` (own path spawns no curl, empty PATH) plus `route_tests::a_declared_gsc_is_left_alone_and_a_held_value_is_kept`. No byte-identical `secrets.toml`/`config.toml` assertion on an own-client run (S1).

config-menu:
- Search Console only: `route_tests::search_console_alone_on_the_lazarobox_route_needs_no_own_client`, `secrets_write::google::route_tests::the_lazarobox_route_plans_the_cache_path_with_no_own_client_file`
- Neither own client nor checklist (62 cols + insta): `connectors_client_tests::the_lazarobox_route_draws_its_client_line_and_no_checklist_or_own_client`
- Lazarobox wins over own client: `route_tests::a_readable_own_client_does_not_change_the_lazarobox_route`
- Explicit own client: `the_own_route_is_as_in_issue_736`, `the_own_route_keeps_its_client_line_checklist_and_pre_check`
- Workspace unchanged / both ticked: `workspace_on_the_lazarobox_route_is_refused`, `both_ticked_use_the_own_client_for_both`, `with_workspace_ticked_u_has_no_effect_and_the_screen_says_why`
- Pin not filled disabled route: `without_a_pin_the_route_is_disabled_with_a_notice_and_own_is_selected`, `route_tests::without_a_pin_the_lazarobox_route_is_not_planned`
- Declared gsc left alone: `a_declared_gsc_is_left_alone_and_a_held_value_is_kept`
- Link failure offers own client: `a_link_failure_is_the_footer_notice_and_offers_the_own_client`
- Route state (`u`, default, not kept, kept value, `o` form): `the_default_route_is_lazarobox_with_a_pin_and_own_without_one`, `u_toggles_the_route`, `the_choice_of_the_own_client_is_not_kept_after_leaving_the_screen`, `a_held_own_path_makes_the_route_own_and_says_so`, `a_held_cache_path_makes_the_route_lazarobox_and_says_so`, `a_cancelled_o_form_keeps_the_lazarobox_route_and_a_saved_one_selects_own`, `declining_the_replace_prompt_leaves_the_route_unchanged`
- `f` preset: `the_preset_writes_the_cache_path_with_a_pin_even_with_an_own_client` (XDG via `the_cache_path_is_next_to_secrets_toml_and_absolute`); `without_a_pin_the_preset_offers_the_own_client_and_says_so`; `enter_switches_between_the_routes_after_the_replace_prompt`; `the_client_file_line_guides_the_switch_to_your_own_client`; `declining_the_prompt_keeps_the_held_value`; `the_other_gsc_rows_and_entries_are_unchanged`

mcp-secrets:
- Link writes only the cache: `the_cache_is_saved_0600_and_no_other_file_changes`
- Read-only everywhere else: `fetch_tests::nothing_but_link_reaches_the_fetch` (source check)
- Cache read back with pin / foreign / loose / two files separate: `a_pinned_private_cache_reads_back_with_no_notice`, `a_cache_of_another_project_is_refused_with_a_notice_without_the_secret`, `a_loose_linked_or_symlinked_cache_is_refused_with_the_cache_name`, `a_missing_cache_is_none_and_the_two_files_are_never_read_as_each_other`
- Chat tools cannot read the cache (task 2.11): test in `core/ai/tools/paths.rs` (also with the XDG extra root)

UNTESTED scenarios: none. Weakly tested: S1, S2.

## Findings

### CRITICAL
None.

### WARNING
- W1 (FIXED in this commit): `design.md` Notices table was outdated against the code (first-draft texts such as `lazarobox client not fetched (<why>); link again later, or u uses your own client`; the real notices are instruction-first, `own client: Esc, f on gsc` then `lazarobox: not fetched (<why>); link later`). The table is rewritten from the code and marked superseded by the spec's Failure notices. Task 1.3 wording ("equals the design table verbatim") still points at the old table; harmless.
- W2: `design.md` decisions 9/10 and `tasks.md` 2.5/2.7 keep earlier wording (`u uses your own client` in the notices) that PR2 review fixes changed; apply-progress records the deviations. Left as is: specs (synced in PR3) and code agree. Not blocking.

### SUGGESTION
- S1: "Nothing else fetches", "Users already linked are untouched" and "Read-only everywhere else" are proven by a source walk plus an empty-PATH check, not by running each UI path with a recording curl and asserting `secrets.toml`/`config.toml` byte-identical. Documented deviation; an effect-level test would be stronger.
- S2: Cache-specific "failure leaves no temp or partial file" and "identical bytes skip the write" are proven only by the generic writer tests (same code path). A cache-targeted assertion would pin it.
- S3: The Link failure footer is clipped at 62 columns, so the cause is only partly visible in some cases (known limit in apply-progress). Revisit if users report it.

## PENDING-OWNER (not failures)
- 2.9 `PROJECT_NUMBER` still `None` (needs O1); the route stays disabled in released builds until then.
- 2.13 live link check (needs O1, O2, O4, O6).
- 2.14 / O11 manual `XDG_CONFIG_HOME` check.
- 4.1 / O5-O8 (branding, Testing user, publishing, announcement). Do not announce before publishing.
- Archive must merge the three deltas into `openspec/specs/` (`lazarobox-google-client` does not exist there yet).

## Next
`sdd-archive`.
