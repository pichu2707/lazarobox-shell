# Verify Report: coding-tools

Mode: Strict TDD (`cargo test`), hybrid store. Checked on `main` @ 6395c781 (whole chain P0..M5-b merged).

## Verdict: PASS WITH WARNINGS (archive-ready; 0 CRITICAL, 4 WARNING, 3 SUGGESTION)

## Build / tests
| Command | Result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| `cargo test` (full) | exit 0: 7016 passed, 0 failed, 6 ignored (lib 6957, codex_outcome 15, jail 25, mcp_shim 3, nesting_guard 2, sandbox_denials 12, version_stdin 1, watch 1; doc-tests 0) |
| Ignored | 5 real-CLI live tests (claude/codex/gemini, manual M5/M6/M9/M10/M11/M14) + 1 `real_shells_split...` (needs bash/zsh) |
| `cargo test --lib tools::host::git` | 41/41 pass |
| Coverage | skipped: no cargo-llvm-cov / tarpaulin installed |

## Completeness
108 tasks: 105 `[x]`, 3 open (Z.1, Z.2, Z.3 at tasks.md:295-297), the expected closing tasks.
- Z.3 DECIDED by the user: `Evidence.digest` = cleaned tail text (<= 2 KiB, char-boundary cut, redacted). Code agrees: `src/core/ai/tools/run.rs:144,196` (`digest()`), tests `the_digest_is_at_most_2_kib_of_the_tail`, `the_digest_cuts_on_a_character_boundary`, `the_digest_is_cleaned_and_redacted` (run.rs:379-408). design.md:221 still shows it as an open `[ ]` question: update at archive.
- Z.1 (archive agent-tools first, then coding-tools) and Z.2 (repoint orchestrator rows 5/7, S7/S8/S10) are archive-time work, not blockers.

## TDD compliance
| Check | Result | Details |
|---|---|---|
| TDD evidence reported | OK | apply-progress.md has per-slice "TDD cycle evidence" for every batch (13 tables/sections plus RED notes in X-d1, X-d2, G-c, G-d) |
| Test files exist / GREEN | OK | all cited tests exist and pass on the full run |
| Triangulation | OK | multi-case tests across slices (e.g. 41 git host tests) |
| Late-written tests | WARNING | G-d commit host path, X-d2 notice, X-d1 items 4/6 were written with/after code; compensated by recorded mutation checks (apply-progress G-d, X-d2) |

Assertion audit: no tautologies (`assert!(true)`, `assert_eq!(1,1)`) found in `src/` or `tests/`. Not exhaustively reviewed line by line across ~7000 tests.

## Spec compliance (11 capability specs, ~164 scenarios, all tagged [auto] except one `[ask]`)
Method: scenario-to-test mapping by test names and apply-progress records, plus full green run. Test names are sentence-style, so mapping is by behavior, not a 1:1 name. Representative verification of the high-risk and amended scenarios:
| Area | Evidence | Status |
|---|---|---|
| git-tools: toggles, verb boundary, unknown verb, worker cannot commit | `without_the_git_family_...`, `a_turn_that_cannot_stage_cannot_add_or_commit`, `a_verb_outside_the_boundary_is_an_unknown_tool` | COMPLIANT |
| git config cannot run code | `a_planted_fsmonitor_and_external_diff_never_run`, `a_hooks_path_set_by_the_repo_config_is_not_run` | COMPLIANT |
| tracked secret / log redaction | `a_modified_tracked_env_file_is_not_in_the_diff`, `a_secret_in_a_commit_subject_is_redacted_in_the_log` | COMPLIANT |
| Cloud ask: only git_diff (amended) | `a_diff_asks_on_a_cloud_engine_and_runs_only_after_y`, `status_and_log_run_without_asking_on_a_local_and_a_cloud_engine`; spec "Cloud asks" says git_diff only | COMPLIANT |
| git_add: count and refusal, no review fallback (amended) | `an_add_asks_with_the_paths_and_stages_only_after_y`, `an_add_whose_paths_would_not_show_whole_is_refused_before_the_prompt`, `an_add_path_with_a_quote_is_refused...`, `adding_a_folder_leaves_the_secret_files_below_it_out`, `an_add_with_staged_changes_must_propose_them_first` | COMPLIANT |
| commit review, gate, nothing staged, signing, hooks | `a_commit_asks_with_the_review_and_commits_nothing_before_y`, `a_commit_with_nothing_staged_...`, `an_approved_commit_is_verbatim_unsigned_...`, `a_failing_pre_commit_hook_...`; gate via `a_generic_approve_never_answers_a_commit` + coverage-gate tests | COMPLIANT (hook-in-real-Landlock is MANUAL) |
| Post-commit drift warns, never undoes (amended) | `a_hook_that_stages_a_secret_makes_the_result_a_warning_and_resets_nothing`, `a_hook_that_stages_another_file_is_reported_by_name`, `a_failed_commit_that_changed_the_index_is_reported`, `a_hook_that_changes_nothing_leaves_a_plain_success` | COMPLIANT |
| command-runner: no jail, unproposed batch, cwd escape, one-run gate, timeout kills group, cancel, redaction, Evidence input | host exec tests, `a_command_that_outlives_its_timeout_is_killed_and_says_so`, runner tests, digest tests, tests/jail.rs (25 pass) | COMPLIANT |
| code-search, code-write, agent-tools, chat-pane, config-menu/persistence, MCP server, chat-engines | covered by the 6957-test lib run (glob/grep, staging, drift, rollback, orphan, quit, descriptor, MCP shim tests) | COMPLIANT (behavioral sampling) |

Scenarios with no passing automated runtime coverage: none found. Real-environment parts are listed as manual below.

## Design coherence
Follows design D-table; documented deviations are accepted: host.rs rebase points (X-d1 18 lines, G-c 10, G-d 11) recorded; `Evidence.digest` as tail text; G-d add shows file count instead of a diffstat; no `ToolStatus::Warning`, so drift warnings are an error result carrying the summary (WARNING W3).

## Findings
### CRITICAL
None.

### WARNING
- W1 design.md:221 still lists the `Evidence.digest` meaning as an open `[ ]` item; Z.3 is decided. Close it (and tasks.md:297) before archive.
- W2 git-tools spec "add" requirement (specs/git-tools/spec.md, `### Requirement: add`) still says "one-line prompt showing the paths". It does not mention the file count nor the refusal when paths do not fit in 400 characters. The user amendment should be mirrored here (verify against the amended spec text before archiving).
- W3 No `ToolStatus::Warning`: post-commit drift and the unchecked-hooks case surface as an error result even when the commit succeeded (apply-progress G-d fixes). The model may read a created commit as a failure. Decide whether to add the status later.
- W4 Test environment dependence: git host tests need `/usr/bin/git` in a system place and skip with a stderr line otherwise (git_tests.rs); a CI image without system git passes without exercising them. Also DEFERRED live "waiting for a slot" state for the global run cap (apply-progress X-d2; refusal after 60 s covers the spec scenario "Global cap").

### SUGGESTION
- S1 Install a coverage tool (cargo-llvm-cov) so changed-file coverage can be reported in strict-TDD verify.
- S2 Known documented limits worth a future follow-up: post-commit check compares only HEAD's tree (commit-msg hook edits, other refs not detected); PATH for git/commands is the process PATH.
- S3 Several slices exceeded their size estimate (W-e4 542, G-d ~560 production lines); per-slice exceptions were recorded.

## Manual checks (pending, not failures)
1. llamacpp-tools T3.0.
2. `run_command` with a real toolchain (cargo/go/npm/pytest) in the real Landlock jail.
3. bsdtar on macOS.
4. HF download 5.2 / 5.3.
5. Git hooks under real Landlock (tests use a stand-in helper).
6. Live CLI tests (claude/codex/gemini M5/M6/M9/M10/M11/M14), ignored by default.

## Next
`sdd-archive` (archive `agent-tools` first, then `coding-tools`; fix W1 and review W2 as part of it).
