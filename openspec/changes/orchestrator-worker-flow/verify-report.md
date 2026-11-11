# Verify report: orchestrator-worker-flow (tip origin/feat/orch-line-limit-2, 3266bf7e, PR #687)

Verdict: PASS WITH WARNINGS. CRITICAL 0, WARNING 2, SUGGESTION 3. Ready to archive after manual steps M11-M17 (owner).

## Execution
- `cargo test --locked --lib` (clean worktree outside /tmp): 10552 passed, 0 failed, 1 ignored. `cargo fmt --check` ok.
- First run in a worktree under /tmp/claude-1000: 168 failures, all environmental (jail rejects a /tmp path as "too broad": ai_detect, chat::cli, notify, git_tests, exec_tests, jail). Not code defects; reproducible only because the worktree lived under /tmp.
- Clippy and integration tests (`--tests`) were not run (scope: `--lib` once).

## Tasks vs reality
184 `[x]`, 0 open. Spot-checked PR21/PR22 against code: host/limit.rs, approval.rs (Limit), app/run.rs, limit.rs, orchestra_tests.rs, orchestra_limit.rs.

## Decisions
- K1: orchestrate prompts 5138 (analyst) and 5073 (programmer) bytes, cap 6144, pinned by src/core/ai/chat/prompts/tests.rs:16-24. Honoured.
- K2: LIMIT_KEYS = "y keep · n or Esc split" (src/core/ai/chat/approval.rs:1043); outside-only decline wording and no Closing::Split (src/runtime/tools/host/limit.rs, Denied(DECLINED) arm). Tests orchestra_tests.rs:2682, app_tests/orchestra_limit.rs:143. Honoured.
- D14: ask at propose_batch before review and seal (host/limit.rs check, runs again before seal); Limit never answered by a mode (approval.rs:316; test orchestra_limit.rs:190); clock paused (limit.rs `_paused`; test orchestra_tests.rs:2538); split discards batch with count (test :2555). Honoured.
- Visible notice rule: uncountable batch -> `could not count the staged lines: nothing is proposed` (fail closed, test :2810); cancel/expiry -> "limit ask was not answered", nothing discarded; split and decline emit a pane Notice. Honoured.

## Findings
WARNING 1: Outside-brief check is skipped when a brief output is `.` (project root covers every file) - src/core/ai/orchestra/run.rs `outside_brief` (tasks.md 22.5 as-built). Documented and in README, but spec "Unlisted file inside the scope asks" has no carve-out; confirm intended or add a spec note.
WARNING 2: Later batch touching a file already over 400 asks again (by design, tasks.md PR #686 review fixes). Not covered by a spec scenario; add a sentence/scenario or accept.
SUGGESTION 1: tasks.md:654 still says 19.5, 19.7, 19.8 "stay open"; they are ticked (647-650). Stale historical note.
SUGGESTION 2: Scenario-level traceability: tests are named by behaviour, not by spec scenario, so a name-based scenario-to-test map (169 scenarios) is not mechanically possible; coverage was checked per slice via as-built notes and spot checks only.
SUGGESTION 3: design.md Open Questions remain unchecked (D2 CLI additionalProperties enforcement, M8 live Ollama D12, W1 trigger identification); carry into archive as known gaps. Add a note that tests must not run from a worktree under /tmp (jail "too broad").

## Pending (owner, not failures)
M11 (W5 clock), M12 (W6 run_command and /exec), M13 (W7 mandatory review), M14 (W8a simple row), M15 (W8b brief), M16 (W8c clarification), M17 (W9b limit ask). Also M8 (Ollama live), open questions above.

## Cleanup
Both worktrees and target-verify removed; main checkout untouched, nothing pushed.

## Resolution

- WARNING 1 and WARNING 2: resolved by owner decision on 2026-11-11 (option A). The spec notes are added in this PR (a project-root output covers every file; a later batch over the limit asks again) and design decision D30 records them.
- SUGGESTION 1: fixed in this PR (the stale note in `tasks.md` now says 19.5, 19.7 and 19.8 were delivered in PR #683).
- Known gaps carried to archive: D2 (whether each CLI engine enforces `additionalProperties`), M8 (live Ollama behaviour for D12), W1 (whether the disclosure identifies the original trigger).
- Manual steps M11-M17 are pending with the owner.
