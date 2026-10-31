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

#### S6 review fixes (PR 15 follow-up)

- W1: all four decoders emit `Usage` only when both the input and the output count are present; a missing one is no longer read as zero (a cached Ollama prompt omits `prompt_eval_count`).
- W2: a total never reads as complete when it is not. The run tracks, per worker, whether an attempt that ran ended without reporting (`gap`; cancelled and timed-out count, refused does not because it never ran). `Run::usage_partial`, `Run::total_partial`; the cost is dropped from a partial sum. `WorkerRow.partial` and `RunUsage.partial` render `(partial)`. The run row is marked partial while any worker is queued or running (chosen over deferring the row: simpler, and the row keeps updating).
- Suggestions: `1.0M` tier (no `1000.0k`), `<$0.0001` for a cost under half a unit of the fourth decimal. Gemini: `result.stats` in the recorded fixtures has no separate thoughts count (total minus input minus output is unexplained there), so only `output_tokens` is read; revisit if the CLI documents a thoughts field.
- W3 open question for the user (spec vs code): the spec says a failed turn emits nothing, and the decoders do that, but the runtime keeps the usage of earlier rounds and of failed attempts across a retry because those tokens were really spent. Keep usage of failed attempts?

### Carried-forward advisories

- S1e-1b: a message starting with `y` while the notice was shown used to accept it (closed by the Enter-only change in S1e-1c). Roots that are ancestors of secret folders are not refused (`jail::too_broad` is still "to be considered"); each read inside is still checked. A chat origin that is not valid UTF-8 gives no root and no recorded reason.
- S1e-2: a running worker row shows no elapsed time; the label has no `n/m` count (S3) and the alert has no error class (S2); the "cancel the run first" footer hint is in state but not drawn; line breaks in an error are drawn as spaces; no manual smoke, TestBackend renders stand in.
- S7-2 (verifier, none blocking): (1) a worker's batch can include the chat's leftover staged files, so the header mislabels them (the user still reviews every file, only the worker's own entries are marked `applied`); (2) an orphan approved later leaves the ledger at `not applied` and its header has no worker label; (3) a `Partial` apply marks every path `not applied`; (4) a timed-out worker's orphan may block staging for a later `code` worker in the same pane until the orchestrator turn ends (inferred, not probed); (5) several workers sharing one pane's batch is left to S3.
- macOS: staging and applying worker writes is unsupported, so `code` workers cannot write there.

## S2a: failure classification (PR 1 of 2 of S2, branch feat/agent-orchestrator-s2a) DONE

Status: tasks 2.1 and 2.2 done, plus the two S1d-3 advisories (retry to the front, no plain-prompt retry). Task 2.3 (detached picker, `RunOverrides`) is S2b.

### TDD cycle evidence

| Task | Test files | RED | GREEN | Notes |
|------|-----------|-----|-------|-------|
| 2.2 classify | core/ai/orchestra/errors/tests.rs, fixtures in tests/fixtures/orchestra/ | compile error (`classify` missing) then 2 assertion failures (usage limit vs 429 order; codex 409 wording) | 8 pass | real fixtures: gemini quota/model, claude/codex model errors; 429/409 fixtures per adapter are synthetic (decoded through each real decoder) |
| 2.2 claude rate-limit text | core/ai/chat/claude/tests.rs | 3 assertion failures (event ignored) | 41 claude tests pass | only a `rejected` `rate_limit_event` adds ` (rate limit: <type>)`; type cleaned through `error_message` |
| 2.2 runtime wiring | runtime/orchestra/tests.rs | existing 409 test changed to expect `Conflict`; new 429 + redaction test | 50 pass | `failed(engine, error)` uses `classify`; a jail refusal keeps `jail_error` text |
| retry to front | core/ai/orchestra/tests.rs | 2 tests changed to the new order, failed; a third (relaunch) failed after the change and was updated | pass | `Run::enqueue(.., front)` |
| no nonce on Retry | runtime/orchestra/tests.rs | compile error (`with_nonce`, `DecideError`) | pass | `RunManager::decide` returns `DecideError::{NotHalted, NoNonce}`; nonce drawn before the run changes |

Gates: fmt, clippy -D warnings (0), cargo test 7693 passed, 0 failed.

### Deviations and decisions
- Merge of origin/main into the S2a branch had 3 conflicts (app.rs, core/ai/chat/mod.rs, runtime/tools/host/files.rs), all both-sides additions; one test (`a_worker_write_outside_its_scope...`) now expects the folder-aware refusal text of local-model-writes.
- `classify(engine, &ChatError) -> Classified {class, message}`; `engine` is not used by the table today (wording is matched across adapters).
- Order: strong quota wording (insufficient_quota, exhausted your, credit balance, billing) > 429/rate limit > 409/conflict > 401/403/auth wording > quota/usage limit > timeout > other. A status code needs its own digits.
- The codex decoder keeps only the inner provider message, so a codex 409 classifies only if the message says `conflict`.
- `decide_run` still returns `bool`: a retry that fails with `NoNonce` leaves the alert open, and `r` can be pressed again.

### S2a review fixes
- W1: the claude decoder suffix is now `(limit window: X)` (no class keyword); a rejected `five_hour`/`*day*`/`*week*` window classifies as Quota.
- W2: `has_code` requires HTTP context (text opens with the code, or `status`/`HTTP`/`error`/`code` before it, or a reason phrase after) and rejects glued neighbours (`/data/429/x`, `sk-ab429cd`, `1.429s`, `main.rs:403:5`).
- W3: `failed()` builds the jail alert from the redacted `classified.message`.
- W4: `check quota`, `exceeded your current quota`, `resource has been exhausted` are strong quota wording; the synthetic gemini 429 fixture now expects Quota.
- S1: `conflict` needs 409 context, `conflict error`/`conflict_error` or a leading `conflict:`; `forbidden`/`permission denied` are Auth only through 403 context or another auth word; `billing` is weak quota.
- S2: a rejected limit is carried into the failure of a stream that ends with no result; an unknown-shape `rate_limit_event` no longer clears it.
- S3: the suffix is applied within `ERROR_CAP` (the message gives way); the `engine error` sentinel is replaced by an empty check on the raw type (kept at most 40 chars).
- S4: 429/409 fixture tests renamed `synthetic_*` and asserted per adapter; `tests/fixtures/orchestra/README.md` marks them synthetic; claude tests use an explicit `error_line`.

## S2b: run override and failure picker (re-delivered as 3 chained branches) DONE

Status: task 2.3 done (S2 complete), plus the two S2a follow-ups. Chain (feature-branch-chain): `feat/agent-orchestrator-s2b-1` (from s2a) -> `s2b-2` -> `s2b-3`. Strict TDD; every branch passes fmt, clippy -D warnings and the full `cargo test` on its own.

### Split
- s2b-1 (decisions never lost silently): short claude window is `rate_limit`; `Effect::OrchDecide`, `Chats::decide_run` and `RunManager::decide` carry the `RunId`; a decision for a run that ended or was replaced (`DecideError::Moved`) or finds no failure waiting (`NotHalted`) emits `OrchestraEvent::Dropped{run}` and the app says `the run already moved on: your answer was not applied` as a system line (never applied to another run); `NoNonce` emits `OrchestraEvent::Declined` and the alert reads `retry failed: could not seal the previous error · r try again · c/Esc cancel`. `OrchNote` enum in `ChatState`.
- s2b-2 (queued workers follow a relaunch): `Run::reresolve_queued`; only `Queued` workers of the failed task are re-resolved; `Running`, already `Refused` and `pending` same-task workers keep their spec (documented).
- s2b-3 (picker and overrides): `RunOverrides`, `FailurePicker`, `m`/`p`, `(override)` marker, alert keys with m/p, plus the review fixes below.

### TDD cycle evidence (re-delivery fixes; RED observed before each fix)
| Fix | Test | RED |
|-----|------|-----|
| short window | errors/tests.rs `a_rejected_short_window_is_a_rate_limit_not_a_quota` | assertion (Other) |
| decision carries run (W2/S1) | runtime/orchestra/tests.rs `an_answer_for_another_run_is_dropped_and_said`, `..._a_run_that_ended_...`, `an_answer_with_no_failure_waiting_is_said_not_silent`; app `the_answer_names_the_run_of_its_alert`, `a_dropped_answer_is_said_...` | compile errors (`Moved`, `Dropped`, `run` field) |
| NoNonce visible | runtime `a_retry_with_no_nonce_tells_the_app_why`; app `a_declined_retry_shows_why_...` | compile errors (`Declined`) |
| relaunch reaches queued | core/ai/orchestra/tests.rs (2 tests) | 2 assertion failures |
| C1 stale picker | `ctrl_c_closes_an_open_picker_...`, `ended_closes_an_open_picker`, `the_picker_is_not_shown_without_its_alert` | assertion failures |
| C2 new run inherits | `a_new_turn_before_ended_starts_from_the_menu_values` (replaces the test that enshrined the bug); `a_second_pick_in_the_run_keeps_the_first` (in-run behaviour kept) | assertion (claude instead of Unassigned) |
| W1 marker | `a_row_still_on_the_old_engine_is_not_marked` | `[(1,true),(2,true)]` |
| W3 notes | `a_picker_note_does_not_relabel_the_alert_or_erase_a_failed_retry` | retry-failed note erased |
| W4 label | `the_picker_says_esc_cancels` | ended `Esc back` |
| W5 unavailable | `an_unavailable_choice_is_refused_at_pick_time` | a Relaunch effect was emitted |
| W6 swallow | `the_open_picker_swallows_every_key_but_ctrl_c` | shifted key leaked |
| S2 repeat | `a_repeated_m_does_nothing` | `m` typed into the composer |

### Decisions
- Overrides are dropped at the replacing orchestrator send (`turn_effect`), not only at `Ended`: a new turn starts from the menu. The in-run relaunch rosters still carry earlier picks. Known edge: a new turn that spawns nothing leaves the old run with its overrides gone for later relaunches.
- `(override)` is set by the app only when the row's engine (and model, when one was picked) is the override; "use general" marks nothing.
- W5 (user decision): a choice whose roster entry is not `Ready` is refused at pick time with `engine not available`; no override, no relaunch, the user stays in the alert.
- Notes: `OrchNote::{RetryFailed, Picker}`, one of each kind; a picker note never relabels `r retry` nor erases a retry-failed note.
- The picker swallows every key but Ctrl+C; a held `r c m p` is swallowed (not typed). `roster()` reads the menu at call time (a menu save mid-run changes later relaunches for tasks not overridden).
- `races_keep_exactly_one_ended_as_the_last_event` ignores `Dropped` events: a dropped answer may follow `Ended` and belongs to no run.

### S2b review fixes 2 (RED observed before each fix; supersedes the Decisions above where it differs)

| Branch | Fix | Test | RED |
|--------|-----|------|-----|
| s2b-1 | notes die with their alert (Ctrl+C, `Ended`) via `ChatState::drop_orch_alert` | `a_note_does_not_leak_into_the_next_alert` | note still `RetryFailed` |
| s2b-1 | `Dropped` is always said with neutral text (`an earlier run already moved on: your answer was not applied`), also when another run is current in the pane (no silent no-op) | `a_dropped_answer_of_an_older_run_is_said_neutrally_in_a_newer_one` | line missing while another run was current |
| s2b-1 | no run manager: `OrchestraEvent::Unavailable` -> `orchestration is not available: your answer was not applied` | runtime `an_answer_with_no_run_manager_is_told`, app `an_unavailable_orchestration_is_said` | compile error (variant) |
| s2b-1 | window mark no longer pre-empts auth/conflict evidence | `a_window_mark_beside_an_auth_status_is_auth` | RateLimit instead of Auth |
| s2b-2 | one halt, with the count, for the queued workers an unavailable engine refuses; doc: `decide` replaces the whole roster, no `Running` case | thickened `a_relaunch_on_an_unavailable_engine_refuses_the_queued_workers_too` (halt worker, `3 workers refused: ...`, retry, relaunch on a ready engine) | halted on worker 2 |
| s2b-3 | overrides stay with the old run until the new run's first event (a failed start keeps them); roster of a new turn is menu-only | `a_new_turn_keeps_the_old_overrides_until_its_own_run_speaks`, updated `a_new_turn_before_ended_starts_from_the_menu_values` | overrides cleared at the send |
| s2b-3 | a pick the run manager drops (`Dropped`/`Unavailable`) is undone; any row or halt of the run keeps it | `a_dropped_pick_leaves_no_override_and_an_acknowledged_one_stays` | phantom override |
| s2b-3 | W5 gap: not-installed engine pick | `an_engine_that_is_not_installed_cannot_be_picked` | note was a picker note, not a transcript line |
| s2b-3 | engine-only pick equal to the menu engine is not `(override)` | `an_engine_only_pick_equal_to_the_menu_engine_is_not_marked` + `is_on` unit test | marked |
| s2b-3 | narrow footer wraps (guard, green from the start) | `the_alert_footer_wraps_on_a_narrow_pane` | n/a |

- Size cuts in s2b-3: no `RunOverrides::label`/`get`, no `override: ...` segment in the alert (rows keep `(override)`), no `OrchNote::Picker`/`clear_picker_note`/`clear_orch_overrides`; the picker's refusals (`cannot change: ...`, `engine not available`) are plain system lines. `WorkerRow.overridden` stays: the transcript renders rows without access to the overrides.
- Held Enter/Esc in the picker is ignored on purpose (the keys line stays visible); no notice.


## S3a: per-worker read scope and labelled approval queue (branch feat/agent-orchestrator-s3a, on the tracker + main v1.0.1)

Tasks: the S1b-2 security item (done), 3.1 first half (approval queue). Not done: `SessionKey` in `Waits`/`Tagged`, `n/m` label, all of 3.2.

| Unit | Change | Test | RED observed |
|------|--------|------|--------------|
| scope per worker | `Orchestra::allows`/`allows_write` gain the asking `WorkerId`; `Bound` grants only while `run.status(worker) == Running` | `a_scope_grant_belongs_to_the_running_worker_only`; the run-scope test now spawns a worker | with the status check off: `queued: not running yet` assertion failed |
| approval queue (core) | `Approvals` keeps `queue: VecDeque<Approval>` (`MAX_QUEUED` 16), `ApprovalAsk::is_worker`, `waiting`, `queued`, `take_promoted`; answer and withdraw promote the oldest; `withdraw` also removes a queued id | 5 tests in `core/ai/chat/approval.rs` (FIFO, own answer only, withdraw queued, chat ask still refused, bound) | compile errors (`waiting`, `MAX_QUEUED`) |
| approval queue (app) | `arm_promoted` re-arms the promoted ask (delay, run target, batch review); `withdraw_tool_approval` also drops queued asks of a finished call; `sync_tools` drains the queue; prompt target gets `(n waiting)` | `worker_asks_queue_fifo_and_each_answer_is_its_own`, `a_queued_ask_whose_call_is_over_leaves_the_queue`, `turning_tools_off_denies_the_queued_worker_asks_too` | prompt had no `waiting`; queued ask kept; with `if` instead of `while` only call 1 was denied |

### Decisions
- Scope per worker means the grant is bound to the asking worker and its lifetime; the roots stay the run's (they come from the chat origin, never from a worker). No per-worker root set exists yet.
- Only worker asks (`WorkerRead`, a labelled `Batch`) queue; a chat's own ask beside a pending one is still refused `another approval is pending`, as the chat-pane spec keeps for chats.
- A promoted ask is armed afresh so a key in flight cannot answer it.
- The cancel-worker key stays open for S3c (escalating Ctrl+C, user decision 2026-10-31); nothing here assumes a key.

### Recommended split for the rest of S3
- S3b: `SessionKey` (pane or run+worker) through `Waits`, `Tagged` and the stream/tool events so a cancelled worker withdraws its own pending and queued asks (spec: cancel denies pending asks), the `worker n/m` label, and the hub/token cross-session test.
- S3c: task 3.2 (`max_parallel`, `max_workers`, `run_timeout_mins`, siblings, retry before queued, escalating Ctrl+C cancel one/all).

### S3a review fixes (RED observed before each fix)

| Finding | Fix | Test | RED |
|---------|-----|------|-----|
| CRITICAL: a queued ask ran the shown-ask branch of `open_prompt` (re-aimed the chat, re-armed, replaced the review) | `open_prompt` touches the chat only when the new ask became the pending one; a queued ask only marks dirty | `a_queued_read_leaves_the_shown_batch_delay_and_target_alone`, `a_queued_batch_does_not_replace_the_shown_review`, `a_promoted_batch_opens_its_review_with_the_batch_delay` | all three failed with the guard disabled |
| WARNING: queue full denied silently as `another approval is pending` | own reason `QUEUE_FULL` ("approval queue full") and a pane notice | `a_full_queue_denies_with_its_own_reason_and_a_notice`, core `the_worker_queue_is_bounded` | reason constant did not exist (compile error) |
| SUGGESTION: user-opened review never queues | `open` queues only a tool call (`call.is_some()`) that `is_worker` | `a_labelled_orphan_review_is_refused_beside_a_pending_ask` | failed with the `call.is_some()` check removed |


## S3b: session key through the waits, the asks and the answers (branch feat/agent-orchestrator-s3b, on s3a)

Task 3.1 done (S3a + S3b). Not done: all of 3.2 and the cancel key (S3c). Four commits; strict TDD, behavioural RED where it could be.

| Unit | Change | Test | RED observed |
|------|--------|------|--------------|
| waits by session | `SessionKey {Pane, Worker(pane, WorkerKey)}` (core/ai/orchestra); `Waits` keyed `(SessionKey, turn, call)` with `impl Into<SessionKey>` so pane callers are unchanged; `Waits::end_worker` denies one worker's waits and refuses its later asks; `end_pane`/`end_turn` still take the workers' | `ending_one_worker_denies_only_its_own_asks`, `an_ended_worker_is_denied_its_later_asks`; guards green from the start: `an_answer_names_its_session` (spec: Cross-session call refused), `a_closed_pane_denies_its_workers_asks_as_well` | `left: None` (stub `end_worker` did nothing) |
| worker approver | `Approver::for_worker` / `end_worker` (defaults keep fakes working); `RunManager` gives each worker its own approver and calls `end_worker` on cancel, end and finish | `a_worker_ask_waits_under_its_own_session_and_cancelling_it_denies_the_ask`; `a_code_workers_batch_waits_for_the_users_review` now answers under the worker | the orchestrator session could still answer the ask |
| ask and answer carry the worker | `ApprovalAsk::WorkerRead.worker`, `BatchReview::labelled(.., worker)`, `ApprovalAsk::worker()`; `Effect::ToolDecision.worker`; `Chats::decide(impl Into<SessionKey>)`; `App::deny_for` | existing app tests now expect `worker: Some(..)` for worker asks and `None` for chat asks | (refactor, tests adapted) |
| n/m label | `Orchestra::workers(run)`, `Run::worker_count`, `WorkerScope::ask_label()` = `n/m <kind> <engine>><model>` | `a_worker_read_outside_the_scope_reaches_the_approver_and_cancel_denies_it` (worker 1 of 2) | label was `review codex>default` |
| retry keeps the key | `for_worker` revives an ended worker session so a retried worker asks again | `a_retried_worker_asks_again_after_its_first_attempt_ended` | second ask denied |
| app withdraws one worker | `App::withdraw_worker(pane, key)` (the door S3c calls) runs on a worker row that is not queued or running; `withdraw_where` generalizes `withdraw_tool_approval`; promotion is the S3a path | `a_worker_ending_withdraws_its_own_asks_and_promotes_the_next_worker`, `another_worker_ending_leaves_the_shown_ask_and_the_queue_alone` | both workers' asks stayed (`2 waiting`) |
| hub token | guard only | `a_workers_token_reaches_no_other_workers_dispatch` (spec: Cross-session call refused, Own socket per worker) | green from the start (S1 behaviour) |

### Decisions and deviations
- `Tagged`/stream events are NOT re-keyed: a worker's rows and asks arrive on the orchestrator turn's channel and render in its pane, and `CallId` comes from one per-host counter, so `(pane, turn, call)` is unambiguous in the app. The session travels on the ask instead (`ApprovalAsk::worker`), which is what the answer needs.
- The manager runs one worker at a time until 3.2, so "worker 1's ask is unaffected" is proved at `Waits` and app level, not with two live workers.
- `m` in `n/m` is the run's worker count when the ask is built.
- The runtime denies a cancelled worker's calls itself (`end_worker`); the app only drops the prompt, it sends no answer.

### For S3c
- Per-worker cancel: send the runtime's existing cancel of one worker (`RunManager::cancel_worker`, already reachable through the `cancel` tool); the row event it produces runs `withdraw_worker`. The escalating Ctrl+C needs a new `Effect` that calls `cancel_worker` for the selected worker and the "Ctrl+C again cancels the run" note.
- `max_parallel` makes "other workers' asks untouched" testable with live workers.


### S3b review fixes (RED observed before each fix)

| Finding | Fix | Test | RED |
|---------|-----|------|-----|
| W2 revived key had no attempt | `for_worker` gives each approver an attempt number (`Ended.current`); only the current attempt may ask, an earlier attempt's approver is denied unseen (`Ended::over`) | `the_approver_of_an_earlier_attempt_cannot_ask_after_a_retry` | stale ask waited (`left: None`) |
| W1a ghost ask, runtime end | `Approver::is_over`; `execute` skips the announce when its worker session is over after the slot wait | `a_worker_ended_before_its_ask_is_announced_never_shows_it` (full channel, end_worker meanwhile) | a ToolCall was announced |
| W1b ghost ask, app end | `ChatState::ended_workers` (set on a terminal row, cleared by a running row and by the run's end); `on_tool_ask` denies such an ask `the worker already ended` | `an_ask_that_arrives_after_its_worker_ended_is_refused_unseen` | no denial, ask queued |
| never silent | a `y` whose `decide` returns false for a worker ask emits `OrchestraEvent::AnswerLost`, shown as a system line | `a_yes_that_reaches_no_worker_wait_is_told`, `a_lost_answer_is_said_in_the_transcript` | written with the code (compile-level RED only) |
| gap: retry through `open_session` | none needed | `a_retried_workers_ask_waits_under_the_revived_session` (new `Script::FailThenHang`) | green from the start |
| gap: finish after the run ended | none needed: `end()` closes every task's session before the run leaves `runs` | `a_run_cancelled_closes_its_workers_sessions_for_good` | green from the start |

Note: a first attempt at W1a polled the wait and skipped on a denied `ENDED`; it broke `an_ended_turn_keeps_its_batch...` (an approver may deny with `ENDED` and still want the call announced), hence the explicit `is_over`.


## S3c: run limits, wall clock and the escalating Ctrl+C (branch feat/agent-orchestrator-s3c, on s3b)

Task 3.2 done; S3 complete (S3a + S3b + S3c). Two commits; strict TDD (compile-level or behavioural RED, mutation check where the test was green on first run).

| Unit | Change | Test | RED observed |
|------|--------|------|--------------|
| config keys | `max_parallel` (default 2, 1-4), `max_workers` (8, 1-32), `run_timeout_mins` (60, 1-480) in `OrchestratorConfig`; bad value falls back with the startup problem naming the range; hand-edited like `worker_timeout_secs` (a save never touches them); `unknown_orchestrator_keys_are_ignored` now uses `max_helpers` | `the_run_limits_are_read_with_their_ranges` | left `(2,8,60)` vs `(4,32,480)` |
| pure limits | `RunLimits {parallel, workers, wall}` + `Run::with_limits`; `MAX_RUNNING` removed; `Refusal::WorkerCap` | `the_parallel_limit_runs_two_and_queues_the_third`, `a_spawn_over_the_worker_cap_is_refused_with_the_limit`, `siblings_finish_after_a_failure_and_nothing_new_starts`, `cancel_after_a_failure_kills_the_running_sibling`, `a_retried_worker_starts_before_the_workers_already_queued` | mutation (parallel forced to 1): 4 failed; cap test green by construction |
| wall clock | `RunManager::start_clock` timer per run, `expire` sends `OrchestraEvent::Expired{run, after}` then ends the run (workers killed); app says `run stopped: it reached run_timeout_mins (N)`; worker cap says `worker limit reached: max_workers is N` | `the_wall_clock_kills_the_running_worker_and_ends_the_run` (paused time), `a_spawn_over_the_worker_cap_is_refused_naming_the_limit`, `a_run_that_reached_its_wall_clock_says_which_limit`, `the_configured_run_limits_go_with_the_orchestrator_turn` | wall test: elapsed 600s instead of 90s |
| live two workers | the test S3b could not write | `cancelling_one_of_two_live_workers_leaves_the_other_and_its_ask` | green from the start (S3b behaviour; now proved with two live workers) |
| selection + escalating Ctrl+C | `ChatState::{selected, cancel_armed}`; Alt+Down/Alt+Up select a live worker (wrap, notice when none); selected row prefix `> `; `App::chat_ctrl_c`; `Effect::OrchCancelWorker` -> `Chats::cancel_worker` -> `RunManager::cancel_worker` (the row event then runs `withdraw_worker`); window 2 s on the app clock; Esc unchanged; hint in `orch_guidance` (`worker_keys`) | `src/app/app_tests/orchestra_cancel.rs` (10 tests) | compile-level RED (no `selected_worker`, no effect); mutation (`escalate` disabled) failed `a_second_ctrl_c_within_two_seconds_cancels_the_run` |

### Decisions and deviations
- No worker selection existed: minimal selection added (Alt+Down / Alt+Up, gate on only). Not a Ctrl chord, so the "new chat keys live behind the prefix" rule is untouched; flagged for the user.
- A key held down (same key within the 500 ms hold window) does not escalate: `one_shot` swallows it.
- Ctrl+C with a worker selected while the failure alert is up cancels that worker and leaves the alert; Esc/`c` still answer the alert.
- Queue count: S3a's `(n waiting)` on the prompt target covers `Queue count`; the spec text says "2 more waiting", left as is.
- Retry-first queue order was already S1/S2 (`enqueue(front=true)`), now also tested with two slots.
- The wall clock needs a tokio runtime in `start` only when a wall is set (default `RunLimits` has none), so sync tests are unaffected.
- `chat_guidance` gains a line while a run has a live worker (`Alt+Up/Down select a worker · Ctrl+C cancels the run`); `the_alert_is_drawn...` expectation adapted, no snapshot changed.


## S4 first PR: ledger, handoff and finish tools (branch feat/agent-orchestrator-s4, on s3c)

Task 4.1a done (4.1 split: ~675 production lines before the continuation, so the continuation and `max_turns` are 4.1b). Strict TDD: compile-level RED for the new pure units (tests written first against the API, the build failed on the missing types), behavioural RED for the cancel-while-halted case.

| Unit | Change | Test | RED observed |
|------|--------|------|--------------|
| types | `core/ai/orchestra/ledger.rs`: `Verdict`, `Unit`, `Judgement`, `Handoff`, `Row`, bounded `Ledger` (`LEDGER_BYTES` 8 KiB; oldest summaries shortened to 24 B, then oldest resolved rows collapse into a count; goal and unresolved failures kept) | `ledger_tests.rs` (13 tests) | compile-level (missing module) |
| run | `Run::{handoff, stored_handoff, forced_by, fallbacks, finish, summary, run_ledger}`; `finish` refused `Live`/`Paused`/`Ended`, first `Missing` refusal then fallbacks; fields normalised (reason one line <= 200 B, others 2 KiB, blank -> `none`) | `ledger_tests.rs` | compile-level |
| tools | `OrchCall::{Handoff, Finish}`, table rows `handoff`/`finish` (six orchestration tools), `OrchestraEvent::Handoff` | `the_handoff_call_is_checked_and_a_bad_verdict_becomes_unknown`, tool-list tests updated | compile-level |
| runtime | `RunManager::{handoff, finish_run, announce_handoff}`; `end` builds fallbacks before `Ended` (cancel, expiry, quit, replaced run, natural end) | 6 new tests in `runtime/orchestra/tests.rs` | `choosing_cancel_on_a_failure_ends_the_run` failed `Unknown` vs `Blocked`: `decide(Cancel)` cancels inside `Run` before `end`; fixed with `Run::halted_at_cancel` |
| app | `OrchestraEvent::Handoff` -> system line `handoff worker N kind verdict: reason` for `off_course`/`blocked` only | `an_off_course_or_blocked_handoff_is_shown_in_the_transcript` | compile-level |
| prompts | both `orchestrate.md` tell the coordinator to call `handoff` after `collect` and `finish` at the end | `the_orchestrate_prompts_hold_the_coordinator_rules` | compile-free; red on the missing rule text |

### Decisions and deviations
- The table refuses a non-string `verdict` as `WrongType` (D17 said "not a string -> unknown"): the schema table types every argument. A missing or unknown string is `unknown` as the spec says; a wrong type is refused with a visible reason, never silent.
- Handoff text args are accepted up to 16 KiB by the table and cut to 2 KiB by the run (D17 "capped").
- Fallbacks are also built when the run ends for any reason (cancel, expiry, quit, replaced, natural end), not only at `finish`: the ledger is whole whatever ended it. `blocked` if a failure waited (`halted_at_cancel`).
- Persistence seam: `RunManager::announce_handoff` (runtime) and the `OrchestraEvent::Handoff` arm in `App::on_orchestra` (app). 4.2 saves there. The ledger render plus `Run::summary()` are ready for the `orchestra/{run}/ledger` note.
- `max_turns`: the spec and design give no default or range; proposal for 4.1b: total orchestrator turns including the first, default 6, range 1..24, hand-edited like the S3c keys.
- The ledger is not wrapped yet (D8: worker-derived summaries wrapped): wrapping needs a nonce and belongs with the continuation message in 4.1b.
- Continuation design note for 4.1b: `Chats::start_turn` with `orch` starts a NEW run (replacing the pane's); a continuation must attach to the existing run (new session over the same `Live`), so `RunManager` needs an `attach(pane, run)` and the run needs the goal (the user's message).

### S4 4.1a review fixes

Strict TDD: tests first; RED observed before each fix (compile-level for `HandoffError::Ended` and `OrchestraEvent::Filled`, behavioural for the rest; the overlong-verdict test was proved red by temporarily putting the old 32 B cap back).

| Fix | Change | Test | RED observed |
|-----|--------|------|--------------|
| ledger bound | `Row::failed` summaries capped at 200 B (`FAILED_BYTES`) and shrink to 24 B under pressure like the others; collapsing skips pinned failures (a hidden mask, not a prefix); `LEDGER_BYTES` doc now says the goal counts (it is the user's message, never cut) | `many_failures_with_long_summaries_stay_within_the_bound_and_listed`, `an_old_failure_does_not_block_later_rows_from_collapsing` | 32 x 2 KiB failures rendered over 8 KiB; old failure stopped collapsing |
| finish one-shot | `finish_warned` is the set of ids the refusal named; `finish` refuses again if a terminal worker without a handoff was not named before | `finish_refuses_again_for_a_worker_that_ended_after_the_warning` | second `finish` built the fallback instead of refusing |
| silent fallbacks | new `OrchestraEvent::Filled{run, workers}` after the built handoffs (finish and run end); app line `no handoff from the orchestrator for workers 2, 3` | `finish_announces_the_handoffs_it_filled_in_and_closes_the_record`, `the_handoffs_lazarobox_filled_in_are_named_in_the_transcript` | compile-level (no variant) |
| sanitise | reason: trim, first line, control characters removed; the ledger row summary (first line of `done`) loses control characters too | `the_reason_is_trimmed_before_its_first_line_and_loses_control_characters` | leading blank line gave `none`; ESC and `\r` kept |
| spec alignment | the `verdict` argument cap equals the other notes (16 KiB): an overlong verdict is stored `unknown`, a non-string stays refused; `Run::handoff` after `Finished`/`Cancelled` is `HandoffError::Ended`, answered `run ended` | `an_overlong_verdict_is_accepted_for_the_run_to_store_as_unknown`, `a_handoff_after_the_run_ended_is_refused` (core), runtime test above | overlong verdict `TooLong`; handoff after finish stored |
| doc order | `ids_text` moved above the `now()` doc comment in `runtime/orchestra.rs` | n/a | n/a |

## S4 task 4.1b: continuation turn and max_turns (branch feat/agent-orchestrator-s4b, on 4.1a)

Task 4.1b done. Strict TDD: compile-level RED for the pure `Run::next_turn` and the config key; behavioural RED for the app flush (the parked continuation test failed with the flush disabled, then passed). Gates: `cargo test` 7855 passed in the lib target, clippy `-D warnings` clean, `fmt --check` clean.

| Unit | Change | Test | RED observed |
|------|--------|------|--------------|
| config | `[ai.orchestrator] max_turns` (default 6, range 1..24), additive, hand-edited, bad value falls back alone naming `1 to 24` | `max_turns_is_read_with_its_range`, `the_run_limits_are_read_with_their_ranges` (3 bad rows) | compile-level (no field) |
| core | `RunLimits.turns` (default `usize::MAX`), `NextTurn::{Settle, Continue(n), Capped(n)}`, `Run::next_turn` (Open and a live worker or a terminal worker without handoff) | 3 tests in `ledger_tests.rs` | compile-level |
| runtime | `RunManager::attach(pane, run, scope)` (swaps the turn's channel, approver and pending; keeps roster, workers, limits), `turn_ended` -> `Continue(Continuation)` with the ledger wrapped under a fresh nonce (`label::wrap`, goal kept in `Live`), `Stopped{run, reason}` at the cap or with no nonce, then the usual `end` (fallbacks, workers cancelled) | 5 new tests in `runtime/orchestra/tests.rs`; 2 old S9 tests adapted (a terminal worker with no handoff now continues; the "worker still runs" test is superseded by the continuation tests) | compile-level |
| chats | `OrchSend{goal, attach}`; `open_session` attaches instead of starting; a continuation does not call `Waits::end_pane` (the run's workers keep their asks) | covered through the manager and app tests | n/a |
| app | `OrchestraEvent::Continue` starts `send_with(.., Some(run))` (composer draft untouched) after a transcript line `orchestrator turn N of M: continuing the run from its ledger`; parked in `ChatState` when the pane is not idle yet and flushed in `on_chat` when the turn ends; dropped on `Ended`; `Stopped` shown; a continuation that cannot start cancels the run and says so | 6 tests in `app_tests/orchestra_run.rs` | behavioural (flush disabled) |

### Decisions and deviations
- `max_turns` = total orchestrator turns of a run, the first included; default 6, range 1..24 (reversible default, user was away). Mirrored in the configuration spec (requirement, Defaults, new scenario "Bad max_turns falls back") and one line in design D8. config-persistence untouched: the key is hand-edited like the S3c limits.
- At the cap with work pending the run ends (fallbacks built, running workers cancelled). Consequence: `max_turns = 1` ends a run whose first turn ended with a worker running; before S4 such a run waited for its worker. A run whose turn ends with nothing pending still settles as before.
- The continuation message is a user message in the pane (visible, stored in the conversation history): the whole ledger render (goal included) wrapped in the untrusted-data block with a per-call nonce plus a fixed instruction head. The transcript therefore shows the ledger (bounded at 8 KiB); no hidden-message entry kind was added.
- Race: the manager's `Continue` can overtake the turn's own `Done` on the other channel; the app parks it until the pane is idle.
- A continuation turn replaces the run's events channel, approver and pending count for workers spawned from then on. Workers spawned in an earlier turn keep the earlier turn's channel (their tool rows were already lost once that turn ended; unchanged limitation, noted for S9).
- When the ledger cannot be wrapped (no nonce) the run stops with a visible line instead of continuing bare.
- Halted runs are not continued (the user's alert comes first); after a retry they settle as before.

### 4.1b review fixes

Strict TDD: each test written first and observed RED before the fix.

| Fix | Change | Test | RED observed |
|-----|--------|------|--------------|
| CRITICAL worker routing | A worker's tool events and asks no longer capture turn N's channel and approver. Each worker gets a `Route` (events sender + approver behind mutexes) in `Live.routes`; its scope holds a forwarding channel and a `Rerouted` approver that read the route when they act; `attach` retargets the routes of the running workers (new channel, `for_worker` approver of the new turn). | `a_surviving_workers_ask_goes_through_the_current_turn` (ask reaches the new channel, nothing on the dead one, `decide` under turn 7 refused, under turn 8 answered) | timed out waiting for the ask on the new channel |
| WARNING not orchestrating | `start_continuation` checks `orchestrating(id)` and that the pane's live run is the continuation's before any send; otherwise a line `run stopped: ...` and `cancel_chat` (no ledger message, no `ChatSend`) | `a_continuation_on_a_pane_that_stopped_orchestrating_cancels_the_run` | failed (a send went out) |
| WARNING pane closed | closing a pane or tab with a live run or orchestrator turn emits `ChatCancel` (a run used to outlive its pane until the wall clock) | `closing_the_pane_cancels_its_run` | failed (no `ChatCancel`) |
| WARNING error loop | `ChatState.turn_failure` is set when a turn ends (`Failed`, `Jail`, `Truncated`; a clean end or `Interrupted`, the shape of a turn cap, is not a failure); a failed turn is not continued: `run stopped: the orchestrator turn failed (<first line, 120 chars>)` and the run is cancelled (fallbacks built as usual) | `a_failed_orchestrator_turn_stops_the_run_and_names_the_cause` (both orders), `an_interrupted_orchestrator_turn_is_continued` | failed (the continuation went out) |
| WARNING line before send | the pre-checks above run before the line; the line is reworded `orchestrator turn N of M: starting from the run ledger`, so a runtime attach failure (run ended meanwhile) that follows reads coherently | covered by the not-orchestrating test (`continuing`/ledger absent) | n/a |
| suggestion | comment on `orch_continue`: one slot, turns are serial | n/a | n/a |

Residual: a worker's `Pending` for the orchestrator turn's timers stays the one captured at spawn (`Turn.session`), so a surviving worker's ask is not counted by the new turn's idle timer; the ask itself is shown and answerable.

### 4.1b re-review fixes

Strict TDD: deterministic tests (channels and request/reply on the worker's socket; no sleeps) written first, RED observed, then the fix.

| Fix | Change | Test | RED observed |
|-----|--------|------|--------------|
| CRITICAL forwarder | `Route::forward` never stops on a failed send; `Route` keeps channel, approver and a generation under one lock. A failed send retries once on the new channel when the route moved (plain rows only); an ask announcement that cannot be shown (no turn between turns) ends the worker's session, which denies the wait (reply to the worker is an error), until the next `attach` revives it | `a_worker_that_asks_between_turns_is_denied_and_asks_again_in_the_next` | the worker never got a reply (read timed out) |
| WARNING ask vs announce | `retarget` first ends the worker's session under the old approver (denying its asks in flight, the worker may ask again), then takes the new turn's `for_worker` approver and channel together | `attach_denies_the_asks_in_flight_under_the_old_turn` | the old ask stayed unanswered (read timed out) |
| WARNING revive | `attach` retargets only workers whose status is queued or running; an ended worker's route is dropped, its session stays closed | `attach_does_not_revive_a_worker_that_ended` | the revived worker announced an ask (read timed out) |
| prune | the route is removed when the worker finishes or is cancelled | `a_workers_route_goes_when_the_worker_ends` | 1 route left after the worker finished |
| WARNING timers | the worker's own turn watches the count it started with; `Rerouted::ask` also enters that count when it is not the current turn's (`Pending::same`) | `a_surviving_workers_ask_counts_for_both_turns_timers` | worker-side count 0 |
| check: user cancel | a race was real: a `Continue` sent before the manager heard of Esc/Ctrl+C started a turn on the cancelled run. `ChatState::cancel_run` drops the parked one and refuses later ones for that run; the `Interrupted` of a cancelled turn is dropped (turn moved on) | `a_user_cancel_stops_every_continuation_of_the_run` (Ctrl+C and Esc, both orders) | the late `Continue` produced an `OrchestrateSend` |

Residual: an ask registered under the old approver whose announcement was already queued in the forwarder when the route moved can still be shown on the new turn after its wait was denied; the answer then finds no wait (`AnswerLost`, said) and the worker already got its denial, so nothing hangs. The denial text a worker sees in these cases is the generic "the chat ended before the user answered".

## S4 task 4.2: handoff persistence in engram (branch feat/agent-orchestrator-s4c, on 4.1b)

Task 4.2 done. Strict TDD: behavioural RED for the app wiring (all 8 `orchestra_handoffs` tests failed with the `store_item` call disabled, then passed), compile-level RED for the event, the `MemCmd` variant and the key namespace (`the_orchestra_namespace_is_a_caller_key_too` failed first). The pure `persist` tests were written after their implementation (compile-level only). Gates: `cargo test` 7894 passed in the lib target, clippy `-D warnings` clean, `fmt --check` clean.

| Unit | Change | Test |
|------|--------|------|
| key namespace | `valid_topic_key` accepts the `orchestra/` family next to `chat/`; `topic_key` already existed on `MemCmd::Save` (S3 summary), and engram upserts by it, so `worker.rs` needed no upsert code | `the_orchestra_namespace_is_a_caller_key_too`, `a_repeated_handoff_save_carries_the_same_run_key` (fake engram, wire) |
| pure core | `orchestra/persist.rs`: `Item::of_handoff` (`orchestra/{run}/worker/{n}`, category `architecture`, all spec fields), `Item::of_ledger` (`orchestra/{run}/ledger`), `RunSave` (Waiting / Ready / Degraded, one `Degrade` action per run) | `persist_tests.rs` (7), redaction through `route::save`, inert data through `context_block` |
| runtime event | `OrchestraEvent::Closed{run, ledger, summary}` after the fallbacks on `finish` | `finish_announces_the_closed_ledger_and_the_summary` |
| runtime memory | `MemCmd::EnableForRun`: an inactive `Memory` keeps its start (`on_demand`) and starts the worker once for a run, on the configured port; the config is untouched | `a_run_starts_the_worker_of_an_inactive_memory_on_the_configured_port` |
| app | `app/handoff.rs` (`RunStore` per run): `Handoff` and `Closed` events become quiet saves; memory off sends `EnableForRun`, says `memory on for this run` once and resolves the project itself; replies for run saves are consumed before the chat's note path (no `record_note`, no "saved note" line); a failed or unanswered save, an unreachable engram or a project that does not come in `MEMORY_WAIT` degrade the run: `handoffs not persisted` once, no more sends, run goes on; the tick keeps the timer armed while a project is awaited | 8 tests in `app_tests/orchestra_handoffs.rs` |

### Decisions and deviations
- Engram is NOT started by the run. The chat memory starts `engram serve` only after the user confirms an offer; a run must not spawn a background server unasked, so an engram that is down degrades the run (`handoffs not persisted`). Spec text "MUST try to start it as the chat memory does" is therefore only met for the worker, not for the server. Reversible: reuse the offer flow in a later slice.
- Handoffs kept in memory in degraded mode are the `Run`'s own ledger (already holds every handoff); no second copy in the app.
- The final ledger is saved at `finish` only; a run ended by cancel, expiry or the turn cap saves its handoffs (fallbacks included) but no ledger note.
- Handoff notes use category `architecture` (the closed `MemKind` list is unchanged) plus the `orchestra/` key family.

### 4.2 review fixes

Strict TDD: all tests written first against stubs (`RUN_EPOCH`, `run_save_lost(pane, token)`), observed RED (8 of 16 app tests in `orchestra_handoffs` and `the_ledger_note_has_its_own_key_and_the_summary` failed), then fixed. Gates: fmt clean, clippy `-D warnings` clean, `cargo test` 7902 passed in the lib target (one unrelated timing flake in `runtime::chat::cli` passed on rerun). The handoff app suite now runs 16 tests (`cargo test --lib orchestra_handoffs`), the pure persistence suite 8 (`cargo test --lib persist`).

| # | Fix | Test | RED observed |
|---|-----|------|--------------|
| 1 | run saves are matched by (pane, token) in `answered`, `run_save_lost` and `on_run_reply` | `a_run_save_is_matched_by_pane_and_token` | another pane's failed/unavailable reply and expiry degraded the run; its note reply was consumed |
| 2 | expired run saves leave a tombstone `(pane, token)` in `lost_saves`; the late reply is consumed quietly | `a_late_reply_of_an_expired_run_save_is_dropped_quietly` | late `Saved` fell to the chat path |
| 3 | a run save adopts a session and emits link effects only when chat memory is active and the save is not stale (`save_is_stale`) | `a_run_save_adopts_no_session_while_chat_memory_is_off`, `a_stale_run_save_reply_adopts_no_session` | session adopted in both |
| 4 | privacy: memory is turned on for a run only when `[ai.memory] acknowledged`; else no `EnableForRun`, no lookup, `handoffs not persisted` plus `handoffs need the memory privacy note accepted`, once. `acknowledged` persists when memory is turned off (`config_edit` test `acknowledged && !enabled`) | `memory_never_acknowledged_is_not_turned_on_for_the_run` (off and on-unacknowledged) | `EnableForRun` sent |
| 5 | `drop_pane_stores` in `drop_tab` and the pane-removal path; `Ended` ends the store before the chat check; `ended_runs` makes `store_item` refuse ended runs and gone panes | `closing_the_pane_drops_the_stores_of_its_runs`, `an_ended_run_stores_nothing_more` | store leaked; handoff after `Ended` saved |
| 6 | the run's own `Resolve` carries `RUN_EPOCH` (`u32::MAX`, never a folder epoch); only replies with it resolve or degrade the run; the chat drops them as stale | `only_the_runs_own_project_reply_resolves_it` | chat's project reply resolved the run |
| 7 | ledger note is `Summary: ...` then the ledger (the save cuts from the end) | `the_ledger_note_has_its_own_key_and_the_summary` | ledger first |
| 8 | spec: hint `memory on for this run` once per run; orchestrator-handoffs and chat-memory deltas gain "Never acknowledged: not turned on"; requirement says the run never starts `engram serve`; design D13a records both |  |  |

| 9 (re-review) | `RunStore.tokens` now keeps each save's sent-at; `expire_stores` (memory tick) expires run saves older than `MEMORY_WAIT` independently of the chat's `pending_saves` (a folder change clears those), with the usual tombstone and one `handoffs not persisted`; `run_store_waiting` keeps the timer armed while tokens are in flight; an ended run's store then drops | `a_run_save_in_flight_over_a_folder_change_still_expires` | degrade count 0 (never expired) |

Residual: `lost_saves` and `ended_runs` are small sets that live for the app session.

## S5 (ODD flow), PR S5-1: task 5.1

Strict TDD: `steps_tests.rs` (8), two runtime tests, one persist test and one app test written first against stubs (`Steps::check`/`close`/`line` inert, no `scope`/`size` parse); observed RED (6 of 8 core tests failed: no ordering, no `NoHandoff`, read-only not honoured, ledger line empty, tool parse refused), then implemented. Gates: fmt clean, clippy `-D warnings` clean, `cargo test` 7915 passed (lib) plus the integration suites.

| Unit | What | Test |
|------|------|------|
| `core/ai/orchestra/steps.rs` | `Step` (7, ODD order), `Size`, `Grant{ReadOnly, Implement}`, `Mark`, `StepError`, `Steps` (handoff per step, closed flags, size with reason, sticky read-only, `line()` for the ledger) | `steps_tests.rs` |
| `run.rs` | `Run::handoff_step` (order check, then record, then close; engine from the `orchestrate` roster entry, status `done`; blocked verdict while halted as for workers); `Refusal::ReadOnly` before the worker cap; ledger gets the `Steps:` line | `a_step_is_not_closed_without_its_handoff`, `steps_close_in_order`, `track_is_for_large_goals_only`, `a_read_only_goal_refuses_a_code_worker`, `the_ledger_lists_the_closed_steps_with_grant_and_size` |
| `ledger.rs` | `Unit::Step(Step)`; `Facts.unit` replaces `worker` | existing ledger tests |
| `tools.rs` + registry | `handoff{id xor step}`; `OrchCall::StepHandoff{step, mark, note}`; `authorize` requires `scope` (read_only/implement), `classify` requires `size` (small/large) with the `reason` as its one line; the mark on any other step, or `id` with `step`, is an extra argument | `the_handoff_tool_takes_a_step_or_a_worker_never_both` |
| `persist.rs` | step key `orchestra/{run}/step/{step}`, title and `step:` line; no `worker:` line | `a_step_handoff_is_stored_under_its_own_stable_key` |
| runtime | `handoff_step` acks `step X: verdict` and announces the handoff (the app stores it like a worker's, or keeps it in the ledger when degraded); out of order says `step X needs Y closed first`; `code` on a read-only goal says `code workers are not spawned for a read-only goal` | `a_step_handoff_is_acked_sent_to_the_app_and_kept_in_order`, `a_read_only_goal_refuses_a_code_worker_with_a_reason` |
| app | transcript line `handoff step X off_course: reason`; save under the step key | `a_step_handoff_is_saved_under_its_step_key_and_off_course_is_said` |

### Decisions and deviations
- The read-only marker is chosen by the orchestrator in the `authorize` handoff (`scope`), not parsed from the goal text by lazarobox (a keyword match on user text is brittle across languages). It can only narrow: `read_only` is sticky, a later `implement` handoff does not lift it, and the default (no authorize) is the S7 behaviour where every write still waits for the user's batch review.
- Closing a step is the act of handing it off: the handoff is recorded and the step closes in the same call; `Steps::close` still refuses a step with no handoff (core invariant), and an out-of-order handoff stores nothing.
- `Step::Track` is refused for a goal classified `small`, and skipped in the order check for it; the "large needs its document" check beyond the track step itself is 5.2 (`FeatureDoc`).
- Spec "Implement is a draft before S7": S7 is merged, so a `code` worker may stage under the user's batch review; no change in this PR, the playbook text is 5.2.

### 5.1 review fixes

Strict TDD: the tests were written first against inert stubs (new `StepError` variants, `StepDone`, empty `short_line`, no checks). RED observed: 7 of 15 new/changed tests failed (`read_only_is_refused_while_a_code_worker_runs`, `read_only_refuses_the_queued_code_workers`, `a_retry_or_relaunch_never_restarts_a_code_worker_on_a_read_only_run`, `classify_needs_a_reason`, `a_repeat_may_narrow_authorize_and_classify_locks_with_the_next_step`, `a_read_only_goal_refuses_a_code_worker` (widen), `the_steps_line_counts_inside_the_budget`). `a_step_closes_while_halted_with_a_blocked_verdict` pins existing behaviour and passed at once. Gates: fmt clean, clippy `-D warnings` clean, `cargo test` 7923 passed (lib).

| # | Fix | Test |
|---|-----|------|
| 1a | `authorize read_only` is refused while a `code` worker runs: `cancel code worker N first: the goal is read-only` | `read_only_is_refused_while_a_code_worker_runs`, runtime `read_only_names_the_running_code_worker_and_classify_needs_a_reason` |
| 1b | queued `code` workers end `Refused` (reason `READ_ONLY`, no halt) when the goal becomes read-only; the ack lists them and the manager syncs their rows and transcript lines | `read_only_refuses_the_queued_code_workers` |
| 1c | `enqueue` refuses any `code` worker on a read-only run, so Retry and Relaunch halt again with the reason | `a_retry_or_relaunch_never_restarts_a_code_worker_on_a_read_only_run` |
| 3 | `classify` with a blank reason: `classify needs a reason` | `classify_needs_a_reason` |
| 4 | `read_only` to `implement` is refused (`Widen`); narrowing may repeat; `classify` cannot repeat once a later step is closed (`Locked`); tool description updated | `a_read_only_goal_refuses_a_code_worker`, `a_repeat_may_narrow_authorize_and_classify_locks_with_the_next_step` |
| 5 | comment: a step closes while halted with a forced `blocked` verdict | `a_step_closes_while_halted_with_a_blocked_verdict` |
| 6 | the steps line counts inside `render_within`; if the rows cannot make room it shrinks to `Steps: N closed` | `the_steps_line_counts_inside_the_budget` |
| 7 | `steps_mut` is used by `steps_tests`, which is not Linux-gated: left as is | |

Known limit (2): the read-only marker comes from the orchestrator's `authorize`, so a model that never calls it is never restricted; writes still wait for the user's batch review. Recorded in design D15 with the S7b/S10 revisit options (gate spawn on `authorize` closed, or derive the default from the goal).

## S5 (ODD flow), PR S5-2: task 5.2 (branch feat/agent-orchestrator-s5b, on 5.1)

Strict TDD: `feature_tests.rs` (7 pure tests) written against stubs (`build`/`correct` returned `NoDocument`, `draw` empty, `of_feature` empty key): 7 of 7 RED, then implemented. Run/tool tests (4) written before `Run::feature_build`/`feature_correct`, `StepError::Doc` and `OrchCall::{Feature, Correct}` existed (compile-level RED). Prompt playbook test RED (`playbook authorize in order`) before the `orchestrate_odd.md` files. The runtime test and the app test were written with the wiring (both passed after one test-side fix; the app one is a thin pass-through, its RED is the missing `OrchestraEvent::Feature`). The registry-args test was written together with the args (no separate RED). Gates: fmt clean, clippy `-D warnings` clean, `cargo test` 7930 passed in the lib target plus the integration suites.

| Unit | What | Test |
|------|------|------|
| `orchestra/feature.rs` | `FeatureDoc{run, specs: Vec<Spec{id,text}>, tasks: Vec<TaskRow{id,specs,title,status}>, log}`; `build(run, goal, specs, tasks)` (`S#` from 1, tasks written `S1,S2: title` with known refs, `log[0]` = goal verbatim), `correct(spec, text, correction)` (log gains the correction verbatim, one `S#` rewritten, atomic), `render()` and `render_for_ledger()` (L1 shown as `(the goal above)`); caps: 32 specs/tasks, 3 KiB rendered; control characters other than line breaks and tabs are dropped from the texts | `feature_tests.rs` |
| `persist.rs` | `Item::of_feature`: key `orchestra/{run}/feature`, same key on every correction (upsert) | `the_document_is_one_note_under_a_stable_key_and_inert_when_read_back` (wrapped through `context_block`) |
| `run.rs` | `Run.feature`; `feature_build` (one per run, `Exists` after; needs `track` open and a large goal; closes `track` with the step handoff in the same call), `feature_correct`; a plain `track` handoff is refused with `NoDocument`; the run ledger carries the document so a continuation turn and a degraded run still have it | `track_closes_with_the_feature_document_only`, `one_document_per_run_and_corrections_need_it`, `a_small_goal_has_no_document` |
| `tools.rs` + registry | the document rides the `track` handoff: `specs` (+`tasks`) builds it, `spec`+`text`+`correction` corrects it; any of these keys on another step, or a mix, is an extra argument | `the_handoff_tool_carries_the_document_and_its_corrections`, `the_handoff_tool_is_offered_the_feature_document_arguments` |
| runtime/app | `OrchestraEvent::Feature{run, doc}` (sent after the track handoff and after each correction); the app stores `Item::of_feature` through the S4 store (degraded mode keeps it in the run ledger); acks `feature document: N specs, M tasks, key orchestra/<run>/feature` and `S2 rewritten, log entry L2`; refusals share `step_refusal` | `the_feature_document_closes_track_and_a_correction_rewrites_one_spec`, `the_feature_document_is_saved_under_one_key_and_a_correction_reuses_it` |
| prompts | `prompts/{programmer,analyst}/orchestrate_odd.md` joined to the orchestrate prompt with `concat!` (still one builtin, <= 4 KiB): one playbook per step in order, closes only with its handoff, one question at a time, size never by counting, `implement` = drafts and review only, the document in the user's words | `the_orchestrate_prompts_hold_one_playbook_per_odd_step_in_order` |

### Decisions and deviations
- The lazarobox side cannot verify that the specs are the user's words (they come from the orchestrator model); what it guarantees is that nothing it does rewrites them (byte for byte through build, store and ledger) and that a correction touches exactly one `S#`. The playbook tells the model to quote.
- Tasks are strings `S1,S2: title` (flat tool schema) rather than nested objects; status starts `todo` and nothing updates it yet (the worker-to-task link is by prompt reference, per D15).
- `Scenario: A large goal needs its document`: the order check from 5.1 already names `track` when `implement` is attempted; 5.2 makes `track` itself unclosable without the document.
- Playbook `implement` stays "drafts and review only" although S7 write tools exist for `code` workers: the spec asks for it and the user's batch review is what makes anything final. S8 will add the test-evidence rule.
- The goal is not copied into the ledger twice: the ledger form of the document shows L1 as `(the goal above)`.
- Merged with the 5.1 review fixes (09c20789): `close_step` carries the `narrows` flag and returns `StepDone` (queued code workers refused, the runtime syncs them); the document build takes the same `Ended`/order checks, and `Widen`, `Locked`, `NoReason`, `CodeRunning` do not apply to `track` (they belong to `authorize`/`classify`). `step_refusal` maps every `StepError`. The ledger counts the feature document inside the bound: full steps and document, then the short steps line, then the document as a count line (`FeatureDoc::short_line`).

### 5.2 review fixes

Strict TDD: five pure tests in `feature_tests.rs` and one runtime test written first against variant/accessor stubs (`omitted()` returned 0, new `FeatureError` variants unused); observed RED: 6 of 17 failed (`a_newline_in_a_text_cannot_forge_a_line`, `blank_texts_are_refused`, `a_full_log_drops_its_oldest_entries_and_says_so`, `a_task_error_says_what_is_wrong`, `bidi_format_characters_are_dropped`, `a_document_needs_specs_and_known_references` for the unknown-ref split), then fixed. Gates: fmt, clippy `-D warnings` clean; `cargo test` 7945 lib tests passed.

| # | Fix | Test |
|---|-----|------|
| 1 | newlines are kept (the user's words) but every continuation line of a spec, a log entry (L1 included) and the ledger `Goal:` line is indented 4 spaces (`feature::indent`), in the stored note and the ledger; the goal in L1 goes through the same sanitiser | `a_newline_in_a_text_cannot_forge_a_line` (forged `S9:`, `S8:`, `L2:`, `L5:`, `Steps:` never at column 0 in note, ledger form and ledger) |
| 2 | blank spec (`spec S2 is empty`), blank correction text (`a correction needs the new text`), blank task title (`task 3 has no title`) are refused | `blank_texts_are_refused`, `document_refusals_name_their_reason` |
| 3 | a correction that overflows drops the oldest non-L1 entries; `omitted` keeps `L#` numbers stable and the render says `(N older log entries omitted)`; if the specs and tasks alone exceed the cap it is refused (`the specs and tasks alone exceed the size of the feature document`), nothing changes; the ack uses the true entry number | `a_full_log_drops_its_oldest_entries_and_says_so` |
| 4 | `MalformedTask(n)` vs `UnknownRef{task, spec}` (`task 2 names spec S9, which does not exist`) | `a_task_error_says_what_is_wrong` |
| 5 | bidi overrides U+202A-202E and isolates U+2066-2069 dropped in the same sanitiser | `bidi_format_characters_are_dropped` |

## S7b (RDD), PR S7b-1: task 7b.1 (branch feat/agent-orchestrator-s7b, on s5b)

Strict TDD: `review_tests.rs` (13 tests) written against a stub (`assess` always `Passive`, `Raise::new` accepted anything, `as_str` constant): behavioural RED, 12 of 13 failed (only `a_plain_source_file_is_passive_on_the_rules` passed), then `review.rs` implemented; one more RED found on the way (`.cargo/config` not a rule) fixed with the `.cargo` folder rule. Gates: see the final report of this batch.

| Unit | What | Test |
|------|------|------|
| `orchestra/review.rs` | `Tier{Passive,Medium,High}` (ordered), `Source{Rules, Raised(reason), Failed(why)}`, `Assessment{tier, source}`, `Raise::new(tier, reason)` (tier any case; reason sanitised: first line, control and bidi format chars dropped, capped at `REASON_BYTES`; blank reason or unknown tier refused with a `RaiseError` message), `assess(&[Change], Option<&Raise>)` | `review_tests.rs` |

### Decisions and deviations
- Rules give `Passive` or `High`; `Medium` is reachable only through the orchestrator raise in 7b.1. A raise that does not exceed the floor leaves `Source::Rules`.
- Failed assessment (`High`, `Source::Failed`): empty change set, blank/NUL path, absolute path (`/x`, `C:\x`), or a path that climbs above the root with `..`.
- Path matching is string-only (no filesystem, no symlinks): `\` is `/`, `.` and empty parts vanish, `..` pops, trailing dots and spaces of a name are dropped, ASCII case-insensitive, at any depth. Documented limit: a symlink or hard link that points elsewhere is judged by the staged path only.
- Rules: CI/build/credential folders (`.github`, `.gitlab`, `.circleci`, `.git`, `.cargo`, `.ssh`, `.gnupg`...), named build/dependency/lock files, extensions of config formats, locks and key material (`toml`, `yaml`, `json`, `ini`, `lock`, `pem`, `key`...), plus the read tools' `is_excluded_name` (`.env*`, credential and history files). Broad on purpose (any `*.json` is `high`): the cost of a false positive is one consent prompt.
- Not wired: the runtime, the `raise` tool and the consent are 7b.2/7b.3.

## S7b (RDD), PR S7b-2: task 7b.2, first half (branch feat/agent-orchestrator-s7b2, on s7b)

7b.2 stays `[ ]` in tasks.md: the pure state, the prompts and the consent prompt are done; the runtime that starts the lens, refuter and correction workers and raises the ask from the worker's `propose_batch` is the second half (7b.2b).

Strict TDD (every unit RED before GREEN):
- `rdd_tests.rs` against a stub (`Lens::id` empty, `parse_report` empty, `Review` no-ops): behavioural RED, 17 of 17 failed; then `rdd.rs` implemented, 17 passed. Later units RED the same way: `parse_verdicts` (stub returned all `None`: 1 failed), `consent_ask` (stub returned empty strings: 2 failed).
- Prompts: `the_lens_and_refuter_prompts_hold_the_review_rules` against stubs returning `""`: RED, then the 10 markdown files wired.
- Consent prompt: `orchestra_consent.rs` (4 tests) with the variant added and the prompt arm `None`: RED 2 of 4 (prompt text); y/n, queue and approval-mode tests passed at once because the worker-ask machinery already carries them.

| Unit | What | Test |
|------|------|------|
| `orchestra/rdd.rs` | `Lens`, `Consent`, `Finding`, `parse_report` (`FINDING <severe\|minor> <change\|existing> <observed\|inferred>: text`, malformed lines counted, 12 per lens), `parse_verdicts` (`CLAIM n upheld\|refuted: text`), `Review` (consent once, `pick` refused before consent or after a decline, report once per picked lens, inferred blockers need the refuter, `begin_correction` once, `finish_correction` checked against the targeted blockers only, `conclude` -> `NotReviewed`/`Clean`/`Corrected`/`BlockersOpen`), `lens_prompt`/`refuter_prompt` (candidate wrapped with the nonce), `consent_ask` | `rdd_tests.rs` (21) |
| prompts | `lens_{risk,resilience,readability,reliability}.md` and `refuter.md` per profile, `lens_builtin`/`refuter_builtin` | `prompts/tests.rs` |
| `ApprovalAsk::Consent{worker, label, change}` | a worker ask: FIFO queue, only the shown one touches the chat, never answered by an approval mode; prompt `worker <label>: review this high-risk change first?`, keys `y review it · n skip the review` | `app_tests/orchestra_consent.rs` |

Spec scenarios covered at the pure/app level: Consent first (`pick`/`report`/`conclude` refuse before the answer), Declined goes on unreviewed (`conclude` -> `NotReviewed`, no lens), One correction at most (`CorrectionUsed`, open blocker named by `open()`), Lens prompts exist (10 prompts, rules asserted, each <= 4 KiB).

### Decisions and deviations
- A decline is `Decision::Denied("the user declined")` and consent is `Decision::Sent` on the worker's session; the host (7b.2b) reads them, the orchestrator never sees either.
- An inferred blocker the refuter did not answer stays `None`: it does not count (not upheld); the receipt (7b.3) must name it as unanswered.
- Lens prompts are built-in only: the override-file chain is per `Task` and the six tasks are unchanged; "resolved like other tasks" is met by profile.
- Not done (7b.2b): `propose_batch` hook that assesses and asks, spawning lens/refuter/correction workers with `Toolset::ReadOnly`/`Code` under S3 limits, the staged view for lens readers, and the `raise` tool.
