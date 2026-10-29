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

## S1c-1: tool registry and Hub seams (PR 4, branch feat/agent-orchestrator-s1c-1) DONE

Status: 4 of 4 tasks done (1c.1, 1c.2, 1c.5, 1c.6). Mode: Strict TDD. Fits coding-tools P0 (`Family`, `FamilySet`, `registry_with`, `parse_call_in`) instead of a parallel registry.

### TDD cycle evidence

| Task | Test files | Safety net | RED | GREEN | Notes |
|------|-----------|------------|-----|-------|-------|
| 1c.1/1c.2 | tools/mod.rs, tools/registry.rs, orchestra/tests.rs | 372 pass in `tools::` | compile errors (`registry_for`, `Toolset::families`, `Call::Orchestra`, `ArgKind::IntList` missing) | 5326 pass, golden fixtures unchanged | triangulated: 4 calls parsed, 3 toolsets, out-of-role, forged run_in_pane, 17 malformed inputs, user tool cannot take an orchestration name |
| 1c.5/1c.6 | runtime/mcp/session/tests.rs | 167 pass in `mcp::` | compile errors (`WorkerKey`, `Hub<K>` missing) | 5331 pass | socket names, run/worker not interchangeable, token isolation, close and drop remove sockets, worker session lists and refuses by family, sweep keeps 9 malformed names |

Gates: fmt ok, clippy 0 warnings, cargo test 5331 pass 0 fail 1 ignored, `cargo +1.88 check --all-targets` ok.

### Deviations and decisions
- `Toolset::families()`: ReadOnly = {Read}; Code = {Read} (nothing more until S7); Orchestrator = {Read, Orchestra}. No toolset gets `Pane`.
- The four tools are `ToolDef` rows (`Family::Orchestra`) with a new `ArgKind::IntList{items,min}`. The table checks shape and limits; `orchestra::tools::parse` then checks meaning (kind, id range, `wait` clamp). `OrchError` maps to `ToolError` (BadKind -> WrongType, EmptyPrompt -> MissingArgument).
- The S1b `orchestra::tools::specs()` listing was removed: the table is the only source of the schemas. The `kind` enum is no longer in the schema; the `spawn_agent` description names the five kinds (pinned by a test).
- Minimal exhaustive-match arms for `Call::Orchestra`: `ask_for` = None, `fs::run_read` = NotReadTool. The real host behavior (execute filter, `UnknownTool` without a handle) is S1c-2.
- `Hub` is `Hub<K: HubKey = PaneId>`; `WorkerKey{run, worker}` is in `core/ai/orchestra/mod.rs`. Call sites that infer nothing need `Hub::<PaneId>::new`.

## S1d, S1e and S7: RunManager, app wiring, views and code workers (PRs 6 to 14) DONE

Status: S1 is complete (S1d-1 to S1e-2) and S7 is complete (S7-1, S7-2). Mode: Strict TDD. Branches are chained `feat/agent-orchestrator-s1d-*`, `-s1e-*`, `-s7-*`; the tracker `feat/agent-orchestrator` merges to `main` behind the `[ai.orchestrator] enabled` gate (off by default).

### What landed in which PR

| PR | Slice | Landed |
|----|-------|--------|
| 6 to 8 | S1d-1..3 | Prompt per task and the "not recommended" mark; `RunManager` spawn path (`start`, `call`, worker turn, jail and scrub) and control path (`decide`, `cancel`, `shutdown`, retry); `Chats::with_orchestra` |
| 9 (#401) | S1e-1a | Ctrl+O toggle, `orch` flag, send path on `Task::Orchestrate`, `app/orchestra.rs` |
| 10 (#403) | S1e-1b | No run-start scope prompt (workers are announced, not asked); read scope from the chat origin; worker reads outside it follow the approval mode; non-blocking provider notice; `notice_accepted` renamed `acknowledged` |
| 11 (#404) | S1e-1c | Run events reach the app (`AppEvent::Orchestra`); failure alert with `r` retry and `c`/Esc cancel; `Entry::Worker` rows; a run ends by itself; the notice accepts with Enter only; the orchestrator turn may use the user's MCP tools, each call asking `y` |
| 12 (#405) | S1e-2 | ORCH badge, worker rows by status, notice and alert views, final `orchestrate.md` prompts, 7 new snapshots, CHANGELOG line (`size:exception`, one PR) |
| 13 (#406) | S7-1 | `ChangeLedger`, write scope check against the run scope and chat root, worker-labelled batch review header; no worker offered a write tool yet |
| 14 (#407) | S7-2 | `Toolset::Code` (read tools plus `create_dir`, `write_file`, `edit_file`, `propose_batch`), `collect` lists `files changed:`, review time counts against the deadline, timed-out batch stays as an orphan, prompts and spec |

### Merge and CI maintenance (2026-10-29)

- The tracker was brought up to date with `main` (chat folder, chat.db schema 3, CLI flags, shared `Hold` key tracker) and merged down the chain. `main`'s `Hold` replaced the orchestrator's own `held_chat_key` debounce; the orchestrator keys now use the shared `repeat` flag.
- Tests that press Enter twice with the mock clock call `let_go()` between the presses, or the second Enter reads as a hold.
- macOS CI failed three S7-2 tests (`a_code_workers_batch_waits_for_the_users_review`, `collect_lists_both_staged_files_and_neither_is_on_disk`, `a_worker_timed_out_in_review_leaves_its_batch_as_an_orphan`) with `path not available`. Cause: staging probes the target with `write::open_target`, which is `Unsupported` off Linux by design (writes need `openat2` and `renameat2`). A symlinked HOME is not the cause (`a_write_is_staged_when_home_is_reached_through_a_link` passes on Linux). The tests are now Linux-only, like the host-level write tests.

### S6: usage (PR 15, branch feat/agent-orchestrator-s6, stacked on #407)

Status: 1 of 1 task done (6.1), in one PR (about 310 production lines, tests excluded). Three work-unit commits: decoders, core accounting, runtime plumbing and rows.

| Step | Test files | RED | GREEN |
|------|-----------|-----|-------|
| Decoders | core/ai/chat/{claude,codex,gemini,stream}/tests.rs, engine/usage_tests.rs, runtime/chat/tests.rs | 6 failures (no `Usage` event) after the enum variant existed | claude result, codex `turn.completed`, gemini `result.stats`, Ollama final line each emit one `Usage` before `Done`; none where the CLI reports no count or the turn failed |
| Core accounting | core/ai/orchestra/tests.rs | compile errors (`WorkerEvent::Usage`, `Run::usage`) | `WorkerEvent::Usage` sums on the slot across attempts; `WorkerResult.usage`; `Run::usage`, `Run::total_usage` |
| Rows | core/ai/chat/worker_row_tests.rs, runtime/orchestra/tests.rs | compile errors (`RunUsage`), then `done.usage == None` | `WorkerRow.usage`, `Entry::RunUsage`, `drive` forwards every round's usage; 1 snapshot refreshed |

Decisions (the spec leaves them open; the most conservative reading was taken):
- `Usage { input, output, cost_micros: Option<u64> }`. The cost is in millionths of a dollar so `StreamEvent` stays `Eq`.
- `input` means all prompt tokens, cached ones included, so the three CLIs read the same: codex and gemini already count cache in `input_tokens`; for claude it is `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`.
- A report with neither an input nor an output count emits nothing (a claude `total_cost_usd` alone is not usage). A failed turn emits nothing. Gemini reports no cost; codex and Ollama neither.
- Sums: a cost is known only when every summed report carries one; otherwise the cost is left out, never a partial total.
- A `Done` worker with no usage shows `n/a`; other statuses show usage only when they have it. The run row appears once one worker is `Done`, shows `run n/a` when none reported, and moves to the end of the transcript as rows change.
- A retried worker keeps the usage of its failed attempt (the tokens were spent). A worker cut off by its deadline reports none (its turn is dropped).
- llama.cpp (OpenAI-style SSE) is not decoded: its `usage` chunk needs `stream_options`, outside this task.

Open questions for the user: (1) is counting cache reads and writes as input right for claude, or should the row show fresh tokens only? (2) should a timed-out worker's partial usage be kept (needs the runtime to read events before the deadline)? (3) should llama.cpp usage be added (needs a request option)?

### Carried-forward advisories

- S1e-1b: a message starting with `y` while the notice was shown used to accept it (closed by the Enter-only change in S1e-1c). Roots that are ancestors of secret folders are not refused (`jail::too_broad` is still "to be considered"); each read inside is still checked. A chat origin that is not valid UTF-8 gives no root and no recorded reason.
- S1e-2: a running worker row shows no elapsed time; the label has no `n/m` count (S3) and the alert has no error class (S2); the "cancel the run first" footer hint is in state but not drawn; line breaks in an error are drawn as spaces; no manual smoke, TestBackend renders stand in.
- S7-2 (verifier, none blocking): (1) a worker's batch can include the chat's leftover staged files, so the header mislabels them (the user still reviews every file, only the worker's own entries are marked `applied`); (2) an orphan approved later leaves the ledger at `not applied` and its header has no worker label; (3) a `Partial` apply marks every path `not applied`; (4) a timed-out worker's orphan may block staging for a later `code` worker in the same pane until the orchestrator turn ends (inferred, not probed); (5) several workers sharing one pane's batch is left to S3.
- macOS: staging and applying worker writes is unsupported, so `code` workers cannot write there.
