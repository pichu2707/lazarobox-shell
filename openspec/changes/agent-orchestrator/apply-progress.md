# Apply progress: agent-orchestrator

## S1a: task row and keys (PR 1, branch feat/agent-orchestrator-s1a)

Status: 10 of 10 tasks done (1a.1 to 1a.10). S1b to S10 pending.

### TDD cycle evidence

| Task | Test files | RED | GREEN | Notes |
|------|-----------|-----|-------|-------|
| 1a.9 | core/ai/chat/engine/availability_tests.rs (+ insta snapshot) | passes before changes (characterization) | passes after | pins the gate-off chat path vs 0.18.0 |
| 1a.1/1a.2 | core/ai/config.rs, core/ai/picker/assign_tests.rs, core/ai/chat/prompts/tests.rs | compile errors then 2 runtime failures (Set all) | 126 config tests | `Task::SET_ALL` added; prompt table keeps 10 prompts |
| 1a.3/1a.4 | core/config_edit.rs, runtime/config_write.rs | compile error (`orchestrator_flag`) | all pass | removal edit for a cleared orchestrate slot |
| 1a.5/1a.6 | core/menu/orchestrator_tests.rs + updated menu tests, ui/components/menu.rs | compile error then count/order failures | 247 menu + ui tests | popup grows to 18 rows; 26 snapshots reviewed |
| 1a.7/1a.8 | core/ai/chat/engine/availability_tests.rs | compile error (`availability_for`) | 30 pass | chat wrapper kept |
| 1a.10 | all | n/a | fmt, clippy (0 warnings), test (5261 pass, 0 fail) | no refactor needed beyond folded cleanups |

### Deviations and decisions
- Table name: specs say `[ai.orchestrate]`; design.md and tasks.md say `[ai.tasks.orchestrate]`. Implemented `[ai.tasks.orchestrate]` (existing `ai.tasks.<id>` convention, `Task::table()`). Specs fixed to match (user decision 2026-10-21).
- `worker_timeout_secs` out of range falls back to 600 with a startup notice (spec) instead of clamping (task text).
- Static rows: spec lists Profile, Context, Memory, Tools, Orchestrator; 0.18.0 also has Approval. Kept Approval; `Orchestrator` is last. AI tab is 13 rows, MENU_BODY 14, popup 18 rows, minimum height 17.
- `core/ai/chat/availability_tests.rs` in tasks.md is really `core/ai/chat/engine/availability_tests.rs`.
- `builtin(profile, Task::Orchestrate)` is a placeholder (chat prompt) until S1d adds `orchestrate.md`.

## S1b: pure core `core/ai/orchestra/` (PR 2, branch feat/agent-orchestrator-s1b) PARTIAL

Status: 4 of 8 tasks done (1b.1 to 1b.4). 1b.5 to 1b.8 pending (stopped at the 600-line size rule: 999 changed lines excluding .snap/openspec after commit 2).

### TDD cycle evidence

| Task | Test files | RED | GREEN | Notes |
|------|-----------|-----|-------|-------|
| 1b.1/1b.2 | core/ai/orchestra/tests.rs | compile error (`tools` module, `Toolset`, `WorkerId` missing) | 5 pass | prompt cap 16 KiB (`MAX_PROMPT_BYTES`), `wait` timeout optional (default 120 s), max 32 ids |
| 1b.3/1b.4 | core/ai/orchestra/tests.rs | compile error (`run`, `worker` modules missing) | 13 pass in orchestra::tests | refusal creates a `Refused` worker and halts the run; `collect` returns raw text for now (wrapper is 1b.7) |

Gates after commit 2: fmt ok, clippy 0 warnings, cargo test 5274 passed, 0 failed, 6 ignored.

### Deviations and decisions
- `ErrorClass` lives in `worker.rs` (design puts it in S2 `errors.rs`): `RunState::Halted` and `WorkerResult` need it now; S2 adds `classify`.
- `Run::spawn` returns `Ok(id)` for an unavailable engine (worker `Refused`, run `Halted`) per spec "refusal is a worker failure"; `Err(Refusal::{Paused,Ended})` only for a halted or ended run.
- `Run::decide` returns the ids to kill on `Cancel`, empty otherwise.
- Refusal text comes from `chat::guidance::guidance`; `Unassigned` gets its own text ("no engine assigned to <task>").
- Size: tests and run.rs are larger than estimated (~420); S1b is at 999 lines after 1b.4, so 1b.5 to 1b.8 need a split decision.

## S1b-2: ReadScope and collect wrapper (PR 3, branch feat/agent-orchestrator-s1b-2) DONE

Status: 5 of 5 tasks done (1b.5 to 1b.9). S1b's 1b.1-1b.4 stay done.

### TDD cycle evidence

| Task | Test files | RED | GREEN | Notes |
|------|-----------|-----|-------|-------|
| 1b.5/1b.6 | core/ai/orchestra/tests.rs | assertion failures (stub `ReadScope` allowed nothing, dropped nothing; 4 of 6 failed) | 24 pass | tests also triangulate the textual-only check (`other/in` links into the root) |
| 1b.6 default | tests.rs | assertion failure (`for_run` stub dropped nothing) | pass | `for_run` uses `jail::is_scratch`; the test relies on the temp dir being a scratch folder |
| 1b.7 | tests.rs | 4 of 5 assertion failures (stub returned raw text) | 29 pass | `Run::collect_wrapped(nonce, id)` over `label::wrap`; `collect` stays raw |
| 1b.9 (e) | tests.rs | assertion failure (elapsed 5s, expected 0) | pass | `enqueue` resets `started` before the refusal branch |
| 1b.9 (a)-(d) | tests.rs | characterization tests: pass on first run (behavior already correct) | pass | no RED possible |
| 1b.8 | all | n/a | fmt, clippy (0 warnings), cargo test 5294 pass, 0 fail, 6 ignored | nothing to refactor; no re-export needed |

### Deviations and decisions
- `ReadScope` API: `new(run, secrets, origin, inputs, &scratch) -> (scope, dropped)`, `for_run` (jail scratch list), `denied(run)`, `allows(&Run, secrets, origin, input)`. Scope is tied to a `RunId` and grants nothing once the run is Finished/Cancelled or for another run.
- Textual check reuses `paths::display_target`; real check reuses `paths::resolve`. Both must sit under the same root.
- Error text wrapping (orchestration errors) is not added here: only `collect` exists in core; the error path arrives with the runtime (S1d).
