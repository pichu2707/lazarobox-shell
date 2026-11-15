# Tasks: Local multi-step (measure, then tune)

Order: S1 measures (no behaviour change), S2 lands before S3 (C6), S3 only with numbers, S4 only if its table is accepted (D12), S5+ are the #442 follow-ups. Each slice is one PR under 400 production lines (tests, fixtures, snapshots and docs do not count). Strict TDD (`cargo test`): every implementation task has its RED test task first, through the public interface of the module; the GREEN task makes it pass; refactor last. MSRV 1.88: no `if let` guards in match arms (D17). S1 modifies no request body, prompt, reminder, round limit, `think`, profile or tool text.

Owner resolutions applied to the specs on 2026-11-15 (this tasks phase): (1) tools refusal is decided per turn, no session memory (chat-engines, HTTP 400 disambiguation); (2) profile family = exact name before `:`, so `qwen3` does not match `qwen3-coder` (Per-model profiles); (3) replay gives fixed answers for exec and git, real dispatch for files and batch (Replay guard).

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines (production) | S1 ~374 (range 340-400); S2 40-80; S3 ~220; S4 ~240; S5+ 30-120 each |
| 400-line budget risk | S1 Medium (93% of budget); S2-S4 Low |
| Chained PRs recommended | Yes across slices (S1 to S4 are separate PRs); S1 itself: No |
| Suggested split | S1 -> S2 -> S3 -> S4 -> S5+ (stacked); S1 single PR with a checkpoint (see below) |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |

Decision needed before apply: No (S1 under 400; re-check at the checkpoint)
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Medium

Does S1 need splitting? No, not up front. The design counts ~374 (eval.rs 170, scenarios 90, table 110, wiring 4). Checkpoint after task 2.5 (scorer + scenarios, ~260 real lines): if the running total plus the table (~110) would pass 400, split the table and the live harness into S1b (`table.rs` + `ollama_live.rs`, base = main after S1a merges). Levers before splitting: seeds to fixture files (-30). Verification tasks 1.1-1.4 can only shrink the estimate or add a few lines to the approver.

### Suggested Work Units

| Unit | Goal | Likely PR | Notes |
|------|------|-----------|-------|
| S1 | Scorer, scenarios, table, replay guard, live harness, baseline | PR 1 | base main; docs/fixtures/baseline included; fallback split S1a/S1b |
| S2 | 400 disambiguation | PR 2 | base main after PR 1; independent of S1 code, ordered before S3 |
| S3 | Profiles table and `think` | PR 3 | after S2; rows only with before/after table |
| S4 | Step check | PR 4 | after S3; ships only if accepted |
| S5+ | #442 `next_step` follow-ups | PR 5+ | one small PR each, independent |

## S1 Measure: scorer, replay guard, live harness, baseline (C1-C5, C8-C11) - ~374 lines

### 1. Verify the [I] items in source first (read-only; record each result under the task)

- [x] 1.1 Verify the approval-request variant names for exec, batch and commit in `src/runtime/tools/host.rs` (`Approver`, host.rs:96; the ask enum) and map them to `run_command`, `propose_batch`, `git_commit` for `ScenarioApprover` (D7). Record the exact names; if different, fix D7.
  - Result (`ApprovalAsk`, `src/core/ai/chat/approval.rs:94`): `Exec { program, args, cwd, .. }` is `run_command` (`Run { command }` is `run_in_pane`, not used); `Batch(Arc<BatchReview>)` is `propose_batch`; `Commit(Arc<CommitReview>)` is `git_commit`. D7 is right, with one gap: `GitAdd { paths, files }` is `git_add`, a separate ask that a commit run needs. `ScenarioApprover` must answer `GitAdd` as part of the `git_commit` step (approve when the scenario expects `git_commit`), or the model could never stage. `Decision::Sent` is the approval; `Decision::Denied(&'static str)` the denial.
- [x] 1.2 Verify the host refusal texts: the `denied: ` prefix, `FAILED`, `DROPPED` and every `refusal_text` form (grep the constants in `src/runtime/tools/` and `src/core/ai/tools/`). Record the list that `eval::refused` will pin; confirm a non-zero `run_command` exit is not among them (D5).
  - Result: the model sees the `Outcome.text`, with no status. Forms: `denied: <reason>` (host.rs:1039-1259, files.rs:870; reasons such as `NOT_APPROVED` = "the user did not approve it"); `ollama::FAILED` = "the tool failed" (panicked call); `ollama::DROPPED` = "result dropped: the tool output limit of this turn was reached"; `ollama::TOO_MANY` = "too many tool calls in one reply" (private, made `pub(super)` for the pin test); the 13 `refusal_text(ToolError)` strings (`unknown tool`, `missing argument`, ...); `paths::REFUSED` = "path not available". There is no `FAILED`/`DROPPED` upper-case form: D5's wording meant these constants. A non-zero `run_command` exit is `ToolStatus::Ok` with the rendered report (exec.rs:576): not a refusal. Known limit: other `ToolStatus::Error` texts without a prefix (for example `COULD_NOT`, `NOT_ENFORCED`, staging errors) cannot be told from a result by text alone; they are not in the list, and the harness may widen it with evidence from the live baseline. `eval::refused` pins the list; `eval_harness.rs` has the test against the real constants.
- [x] 1.3 Capture the exact Ollama "does not support tools" wording: from the owner's Ollama (0.30.10) POST `/api/chat` with `tools` to a model without tool support, or from the Ollama source if no such model is installed; save the body as `tests/fixtures/ollama/400-no-tools.txt` for S2 (D14). No path or user data in it (the task 5.1 scan applies).
  - Result: Ollama 0.30.10 is running but every installed model supports tools (`llama3.2:3b-ctx` answered a tool call; `qwen2.5-7b-rank` answered text; `deepseek-r1:70b` was not loaded to avoid a 70B load). The closest real reply, from `nomic-embed-text`, is `HTTP 400 {"error":"\"nomic-embed-text:latest\" does not support chat"}`, which shows the Ollama format `"<name>" does not support <capability>` (a quoted model name, then the capability). The fixture `tests/fixtures/ollama/400-no-tools.txt` is therefore INFERRED from that format with a neutral model name (`gemma2:2b`), not captured; S2.3 [manual] must confirm it with a real no-tools model. S2 should also treat `does not support chat` as a different 400 (not a tools refusal).
- [x] 1.4 Locate the function that composes the chat system prompt and the family set of a default chat, and the head budget (system head plus history cap) used by S3's addendum (D10, D15). Record file and function; the live harness reuses it (task 6.4) and S3 plugs the addendum there.
  - Result: the turn composes the tools system text in `rounds` (`src/runtime/chat/ollama.rs:199`): `tooled.system = prompts::with_tools(&plain.system, request.families)` (`src/core/ai/chat/prompts.rs:188`). The base chat prompt is pinned by the app (`src/app/chat.rs:674`, `Chat::send(.., system)`); its built-in is `prompts::builtin(Profile::Programmer, Task::Chat)`. The head budget is `budget::budget(engine, window, system, memory, tools_reserve)` (`src/core/ai/chat/budget.rs:41`) with `tools_reserve_tokens(families)` (`adapters.rs:324`); S3's addendum plugs in `prompts::with_tools` and weighs in `budget`. A default chat offers `FamilySet::LEGACY` (Read, Pane, PaneRead) until toggles are on; the coding set the harness uses is `LEGACY` plus Write, Exec, Git and GitWrite (registry.rs:46-60).

### 2. Scorer and scenarios (core, pure)

- [x] 2.1 RED: `src/core/ai/eval/tests.rs`: scorer scenarios of the spec on hand-built `ToolExchange` lists: Complete run, Missing step, Out of order, Ignored literal, Refused exchange is not completed, Text call does not count, Asking back fails (a), Asking back passes (ambiguous), Read-only run must not write, Scorer is pure (local-model-eval, Scorer).
- [x] 2.2 GREEN: `src/core/ai/eval.rs`: `Scenario`, `Category { WriteAndTest, EditExisting, ReadOnly }` with `key()`, `Expect`, `Order`, `RunResult { completed, missing, out_of_order, ignored, asked_back, forbidden }`, `score(&Scenario, &[ToolExchange], &str)`, `passed()`; `pub mod eval` in `src/core/ai/mod.rs`.
- [x] 2.3 RED then GREEN: `refused(result)` with the prefixes of 1.2, plus a test that pins each prefix against the real constants (D5); `asked_back(answer)`: answer ends in `?` or matches the phrase list ("Please provide ...").
- [x] 2.4 RED: scenario data tests: Engine-neutral scenario (no engine, model, URL, port, wire field in any field), Task (a) expectations (write, batch, pytest, commit; batch before commit; pytest before commit; commit message literal), Task (c) forbids write, exec and commit, each has a category.
- [x] 2.5 GREEN: `src/core/ai/eval/scenarios.rs`: tasks (a) `write-and-test`, (b) `edit-existing`, (c) `read-only` with the seeds (relative paths, generated file texts) and predicates (D2). Checkpoint: count production lines; decide on the S1a/S1b split here.
  - Checkpoint result (apply, 2026-11-15): production lines (no blanks or comments) are `eval.rs` 142, `scenarios.rs` 101 and about 3 of wiring = S1a ~246; the table measured 189 (not 110), so the running total passes 400. SPLIT: S1a = tasks 1.x, 2.1-2.5 (this PR); S1b = sections 3-7 (table, replay guard, fixtures, live harness; base = main after S1a merges). The `Expect` type has no `result` predicate (no step needed one); the pin test of 2.3 lives in `src/runtime/chat/eval_harness.rs`.

- [x] 2.6 RED (owner, 2026-11-15, #441): the coordinator scenario: category `coordinate`, expectations (handoff authorize with scope implement, explore, resolve, classify with size small, then `spawn_agent` kind simple with an output file, in order), the coordinator writes nothing itself, a prose description or a call written as text completes nothing.
- [x] 2.7 GREEN: `Category::Coordinate` and `scenarios::COORDINATE` (task d), `ALL` holds four. The live harness and the replay wire it through `Toolset::Orchestrator` families and the `orchestrate` built-in prompt; the orchestration tools get fixed answers, so the scenario measures that the model CALLS the tools, not what a real run manager does after them (a run against the real manager is a follow-up, not S1).

### 3. Pass rule, acceptance, table

- [x] 3.0 (from S1a review) Close the unprefixed-failure gap of 1.2: `ToolExchange` holds the result text only (the loop keeps `outcome.text` and drops `is_error`, ollama.rs:`exchange`), so host failures without a refusal prefix (`nothing is staged`, `git failed`, `could not run git`, batch apply errors, `ToolStatus::Error` of host.rs ~1036 and 1202-1283) score as completed. Close it through the replay guard: the harness `Canned`/`ScenarioApprover` records the real `Outcome.is_error` per call (dispatch decorator) and `exchanges_from_body` consumers mark those exchanges failed before `score`, with a test of a failed `git_commit` that is not a completed step. No production change to `ToolExchange`.
- [x] 3.1 RED: `table` tests: `passes` 4 of 5 passes and 3 fails; over time 600 s judged by steps, 601 s failed; `accept` raised/without lowering, refused when any combo falls below 4/5, 5 to 4 noise accepted, Borderline repeats (4/5 on one side) reports `Repeat(combos)`; informative models never block (spec, Pass rule and acceptance rule).
- [x] 3.2 GREEN: `src/core/ai/eval/table.rs`: tally, `median`, `passes`, `accept -> Verdict { Accepted, Refused, Repeat }`, accepted set `qwen2.5:7b`, `qwen2.5-coder:14b`, `qwen3:14b`.
- [x] 3.3 RED: render tests: Header and rows (date, ollama version, hardware, runs, lazarobox sha), Median of five (10..50 = 30, five times listed), Per-category summary, Stable plain text (byte-identical twice, ASCII only, `\n` only, no escape, no `/` path, no user or host name), rows sorted by model then task, the failure reason of EVERY failed run printed (for example `asked back` for a trailing question after all steps, C10), failure kinds with counts (`missing:pytest x1`, `order`, `literal`, `asked`, `forbidden`, `unexpected`, `time`).
- [x] 3.4 GREEN: `render(&Header, &[Row])`, UTC `YYYY-MM-DD` by a days-to-civil helper (no new crate), hardware fallback chain env, `nvidia-smi` name+memory, `cpu` (the probing itself stays in the live harness).

### 4. Replay guard (CI-safe)

- [x] 4.1 RED: harness tests (`src/runtime/chat/eval_harness.rs` tests): `exchanges_from_body` pairs each assistant `tool_calls[i]` with the following `tool` messages in order from the LAST request body; text-only calls never appear (D4).
- [x] 4.2 GREEN (apply notes: the fixture parser is a strict hand-written one, because the crate has no direct `serde` dependency and S1 adds none; replies are built in the harness, so `calls_many` stays private; `throwaway_root`, `Folder`, `Rig` and the recording proxy live in `eval_harness.rs` so the replay and the live test share them, and the replay lab is under `target/olive` too): `eval_harness.rs` (`#[cfg(test)]`): `exchanges_from_body`, fixture parser (strict, `{ scenario, replies: [{ text, calls: [{ name, arguments }] }] }`), `Canned` dispatch (fixed non-refusal answers for `run_command` and git tools; forwards reads, writes and `propose_batch` to the real dispatch), `ScenarioApprover` from 1.1 (denies and records `unexpected`); add the three `#[cfg(test)] mod` lines in `src/runtime/chat.rs`.
- [x] 4.3 RED (the commit-first run also puts the commit before the test run, which follows the batch, so it reports the two pairs that hold the commit and nothing else; no transcript can break one pair alone): `eval_replay_tests.rs`: Good transcript passes; each bad transcript fails for its own reason only (`fib-no-pytest` missing, `fib-wrong-message` ignored literal, `fib-commit-first` out of order); Unexpected approval denied (a `propose_batch` in task (c)); CI-safe offline, lab removed at the end.
- [x] 4.4 GREEN: hand-written fixtures `tests/fixtures/ollama/eval/{fib-good,fib-no-pytest,fib-wrong-message,fib-commit-first}.json` and the replay driver (`Fake` + `tool_turn_with(Profile::Ollama, ..)` + `ToolHost::dispatch(TurnScope{..})`, D6). No lab under `/tmp`.
- [x] 4.6 Coordinator fixtures: `coordinate-good`, `coordinate-prose` (the qwen3:14b failure), `coordinate-text-call` (the qwen2.5-coder:14b failure); the good one passes, the others complete nothing. Also `read-only-batch` for the Unexpected approval scenario.
- [x] 4.5 Regression: existing recorded-body tests (adapters, `ollama_tests.rs`, `llamacpp_tests.rs` no `think`) pass with no edit to their expectations (spec, Request bytes unchanged).

### 5. Fixture curation check (C4)

- [x] 5.1 RED: `fixtures_are_clean` over `tests/fixtures/ollama/eval/*.json`: passes on shipped files; a planted `/home/x/y.py` fails naming file and kind, never the value; also fails on `~/`, `C:\`, the running `$HOME` and `$USER`, email-like and token-like (`sk-`, `ghp_`, 32+ hex/base64) strings; a field outside messages and tool calls fails; a file over 16 KiB fails (Fixture curation, all three scenarios).
- [x] 5.2 GREEN: the scan in test code (no production lines).

### 6. Live harness (`#[ignore]`, never CI)

- [x] 6.1 RED (auto): `throwaway_root` with `TMPDIR=/tmp` is not under `/tmp` and refuses a `/tmp` `$LAZAROBOX_EVAL_DIR`; repo gone after a panicking run; seeding leaves the global git config byte-identical (author set in the repo's config, `GIT_CONFIG_GLOBAL` to an empty file); Ignored by default: the live tests carry `#[ignore]` and no workflow under `.github/` passes `--ignored` or `--include-ignored` (grep test).
- [x] 6.2 GREEN: `src/runtime/chat/ollama_live.rs`, `#[cfg(all(test, target_os = "linux"))]`: `throwaway_root()` (`$LAZAROBOX_EVAL_DIR` or `target/olive`), per-run `tempfile` folder with `home/repo/`, seed + `git init` + one commit, `ToolHost::new(Some(home), ..)` (D9).
- [x] 6.3 GREEN: env and pre-checks: `LAZAROBOX_EVAL_MODEL` (comma list, required, usage panic), `LAZAROBOX_EVAL_RUNS` (default 5), `_TASKS`, `_HARDWARE`, `_OUT`, `_DUMP`; panics naming the cause for `GET /api/version`, a model missing in `/api/tags` (never pulls), `python3 -m pytest --version` (Missing model is a failure, not a pass).
- [x] 6.4 GREEN (verify fix W1: the live rig runs `run_command` and git for real through the `target/debug/lazarobox` sandbox helper, `Mode::Live`; replay keeps `Mode::Replay`; both record outcomes by call order, W2): recording proxy (accept, record body, forward to `127.0.0.1:11434`), sequential runs, `tokio::time::timeout(601 s, tool_turn_with(..))` timed with `Instant`, over 600 s failed by time, production system prompt from 1.4, scoring via `exchanges_from_body`, table printed and written to `_OUT`; Unexpected approval denied in live too.
- [x] 6.5 Cap: the whole live run has the 10-minute turn cap per run (C8); the total can take hours and that is documented in the file header, not capped.
- [x] 6.6 GREEN: `LAZAROBOX_EVAL_DUMP=<dir>` writes each run in the fixture format with the repo root replaced by `.` as raw material for hand curation (outside the repo).
- [x] 6.7 Docs: header of `ollama_live.rs` and a short note in `CONTRIBUTING`/dev docs (if present, else the module doc): the run command `cargo test --lib ollama_live -- --ignored --nocapture --test-threads=1`, env vars, fixture curation steps (C4).

### 7. Baseline and close

- [ ] 7.1 [manual] First live baseline on the owner's machine (RTX 4080 16 GB, Ollama 0.30.10): `qwen2.5:7b`, `qwen2.5-coder:14b`, `qwen3:14b` (accepted set) plus `qwen3:14b-nothink` (informative, C6), 4 tasks x 5 runs each; can take hours. Save the table as `openspec/changes/local-multistep/scores/s1-baseline.txt` and paste it in the PR.
- [ ] 7.2 [manual] Curate at most one real transcript per task from the `_DUMP` output into `tests/fixtures/ollama/eval/` (throwaway repo only), check `fixtures_are_clean` passes; optional for S1.
- [x] 7.3 Gate: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`; `rm -rf target` of the worktree after the checks (AGENTS.md).

### S1 hardening (before the first live baseline; test/eval-hardening)

- [x] H1 `parse_errors_list_every_tool_error_variant`: an exhaustive match (no wildcard) pins `PARSE_ERRORS` to every `ToolError`, so `mark_failed`'s order pairing cannot shift when a variant is added.
- [x] H2 Live pre-check `sandbox_probe`: runs `pytest --version` through the real helper path before any model call and fails naming the cause (a nonzero `exit code N` counts, it is not an error outcome); offline tests with a passing, a failing and an injected failing runner.
- [x] H3 `helper_age_warning`: the live test prints a warning when `target/debug/lazarobox` is older than the test binary (`cargo test --lib` does not rebuild it).
- [x] H4 Offline test: in a live-style rig (StandIn helper) `run_command` and `git_add` go through the `Judge`; orchestration tools stay `recorded`.
- [x] H5 `table::accept` refuses with `nothing rose` instead of an empty list (the only production change).
- [x] H6 `_DUMP` files are `<scenario>-<model>-<n>.json`, `n` claimed with `create_new` (`write_dump`), so a run never overwrites another.

## S2 HTTP 400 disambiguation (C6) - ~40-80 lines

- [ ] S2.1 RED: Other 400 is not a tools refusal, Unreadable 400 body (empty, non-JSON, truncated), Long error body capped at 200, Notice does not echo the message (`SECRET-TOKEN-123`), Tools refusal still degrades silently (per turn, nothing remembered), using the fixture `400-no-tools.txt` from 1.3.
- [ ] S2.2 GREEN: `Profile::rejected` (ollama.rs:115-118) for Ollama: `HTTP 400: ` prefix, JSON `error` contains `does not support tools` (case-insensitive); everything else takes the existing error arm (:254-259); update `OLLAMA_NO_TOOLS_NOTICE` and `REJECTED` docs ("any 400" removed). llama.cpp unchanged.
- [ ] S2.3 Gate: before/after table not required (no behaviour measured), but the S1 replay guard passes; [manual] one live check with a model that refuses tools.

## S3 Profiles and `think` (C6, C7) - ~220 lines

- [ ] S3.1 RED: `profile_for` (family by equality of the name before `:`, `qwen3-coder` does not match `qwen3`, exact wins, namespace stripped, no row = none); No row no change (body byte-identical); Unset sends nothing; Set sends a top-level boolean; Rejected `think` is visible (S2 notice, no retry); Profile limit applies only to its family; Addendum counted in the head; Profile is not configurable.
- [ ] S3.2 GREEN: `src/core/ai/chat/model_profiles.rs` (`ModelProfile`, `Key`, `PROFILES = &[]`), `think` in `finish_body` (adapters.rs:465, Ollama wire only), `max_rounds(set, model)` (ollama.rs:55), addendum at the composer of 1.4, `reminders` gate on `go_on`.
- [ ] S3.3 [manual] Seed rows only from S1 tables (for example `qwen3:14b` vs `qwen3:14b-nothink`); each row's PR attaches before/after; no row ships otherwise.

## S4 Step check (C1, C2) - ~240 lines

- [ ] S4.1 RED: `step_check::check` table (Claimed commit not shown, Requested pytest not run, Everything shown, Question or no tools, Never turns text into a call), then loop tests via `Fake` (One reminder shared with `go_on`, request cap).
- [ ] S4.2 GREEN: `src/core/ai/chat/step_check.rs` (pure), refusal helper moved from `eval` to `chat`, hook as third arm of the reminder match (ollama.rs:266-289), flag renamed `reminded` (:214).
- [ ] S4.3 [manual] Before/after table of the accepted set; ship only if accepted by `accept` (Measured acceptance), repeat borderline combos.

## S5+ #442 follow-ups (C2)

- [ ] S5.1 One PR each in `src/core/ai/chat/next_step.rs`: courtesy closings, emoji tail, `Finish::Length`, loop tests; each RED first, each with a before/after table when it changes behaviour.
