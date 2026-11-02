## Verification Report: ai-cost (#440, alpha) @ origin/feat/ai-cost 28667b3a (slices 1a-5 merged). Mode: strict TDD, hybrid.
Verdict: PASS WITH WARNINGS. 0 CRITICAL, 3 WARNING, 3 SUGGESTION. Ready for release excluding manual M.1-M.5.

Gate (worktree wt-ai-cost-5 detached at 28667b3a, CARGO_TARGET_DIR=target-ac-integ): cargo fmt --check OK; clippy --locked --all-targets -D warnings OK; cargo test --locked: 8874 passed, 0 failed, 1 ignored (lib) + integration suites all ok. runtime::notify and runtime::loopback did not flake this run. No .snap.new from this change.

Tasks: 34/34 implementation tasks [x] (1a.1-5.5). M.1-M.5 manual pending (expected, not failures).

Spec compliance (8 spec files, ~200 scenarios; each requirement has covering passing tests): 
- ai-usage (TurnUsage, capture per engine, lenient parsing, summing, billing, formats, thresholds, /cost, worker usage, charts, privacy, alpha, reading today): COMPLIANT. Evidence: usage::tests, claude/codex/gemini tests, stream::usage_tests, ollama_tests, llamacpp_tests, usage_view::tests, chat_usage, chat_cost, orchestra_usage, runtime::orchestra::tests, cost_tests snapshots.
- chat-engines (usage event order, one per request, not answer text): COMPLIANT (runtime::chat::tests, ollama/llamacpp tests, claude usage_comes_before_the_final_event).
- chat-history (v4/no version bump, NULL unknown, notice once per run, delete keeps usage, 90d prune, clock behind): COMPLIANT (chat_store tests, worker/tests prune tests).
- chat-pane (/cost, usage line, coordinator label, threshold notes): COMPLIANT.
- statusline (today segment, ageing, workers count via store day sum): COMPLIANT.
- config-menu (Usage row/view, limit editing, snapshots): COMPLIANT (usage_tests, usage_limit_tests, snapshots).
- configuration, config-persistence: COMPLIANT.

WARNING
1. apply-progress has no formal "TDD Cycle Evidence" table; evidence is prose per slice (RED/GREEN notes). Slice 3 menu tests were RED only as compile failures. Not blocking.
2. Scenario "Worker spend counts under per-token" has no dedicated test (logic shares per_token_spend with chat tests; only chat/day_spend_note covered).
3. Scenario "Usage not logged" has no dedicated test; code has no logging of usage (vacuously true).
SUGGESTION
1. Add a worker-record dollar test for day_usd under per-token vs subscription.
2. Add a log-capture test, or drop the scenario.
3. Statusline "Workers count" is covered indirectly (store day-sum test a_worker_row_keeps_its_role_and_worker_id_and_counts_in_the_day) - add a direct app-level scenario test.
Deviations (accepted, documented): slice 3 refused chars show a notice instead of being ignored; day notes may show on first read of the day; charts are plain text not widgets (spec/design D14 aligned).
Manual pending: M.1 claude, M.2 codex/gemini, M.3 ollama/llama.cpp, M.4 delete keeps usage, M.5 real orchestrated run.

Engram: sdd/ai-cost/verify-report (#3579), project lazarobox-shell.

---

Follow-up on release/1.2.0: S1 (warning 2 / suggestion 1, worker dollars toward `day_usd` under per-token vs subscription) and S3 (suggestion 3, status segment 1000 to 1300 tok after a 300-token worker record) were added on release/1.2.0.
