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

### 7b.1 review fixes

Strict TDD: ten tests added to `review_tests.rs` first (a `Lowering` variant stub only so it compiled). Observed RED: 9 of 46 failed (`more_build_files_are_high`, `repo_and_tool_config_is_high`, `more_build_extensions_are_high`, `commit_hooks_and_shell_scripts_are_high`, `a_non_ascii_look_alike_of_a_rule_is_high`, `the_reason_drops_invisible_and_line_separator_characters`, `a_tab_in_the_reason_becomes_a_space`, `a_reason_with_nothing_visible_is_refused`, `a_passive_raise_is_refused_because_the_tier_only_goes_up`); `a_non_ascii_name_with_no_rule_stays_passive` passed from the start (guard). Gates: fmt, clippy `-D warnings` clean, `cargo test` 7969 lib tests passed plus integration suites.

| # | Fix |
|---|-----|
| 1 | Rules broadened: Rakefile, Procfile, Containerfile, Vagrantfile, Brewfile, Podfile, `rust-toolchain`; `.gitmodules`, `.gitattributes`, `.tool-versions`, `.yarnrc`, `.pypirc`, `known_hosts`; extensions gradle, sbt, csproj, sln, props, targets, xml, mk, tf, sh; `.husky/` and `.githooks/` folders. "Broad on purpose" kept in the code comment. One test per family. |
| 2 | Every name is folded (uppercase then lowercase per char) before matching, so KELVIN SIGN in `Makefile` and LONG S in `.ssh` match. Simple folding only; fullwidth letters and other homoglyphs are not folded (documented). |
| 3 | One shared `ledger::one_line` (first line, tabs to spaces, control, U+2028/2029, zero-width, U+2060, U+FEFF, U+3164, U+2800 and bidi dropped) now serves the handoff reason and `Raise::new`; `cut` shared too. `Raise::new` refuses a reason with no visible character left. Ledger tests unchanged and green. |
| 4 | `Raise::new("passive", ..)` is refused with `RaiseError::Lowering` ("the orchestrator can only raise the tier above passive"). Earlier tests that used a passive raise now use `medium` against a high floor. |
| 5 | Module docs: renames and deletions are judged by the staged path only (`Change` has no source path; S7 must add it when it stages moves); symlinks judged by their own name. |

### 7b.2a review fixes (branch feat/agent-orchestrator-s7b2)

Strict TDD: the tests were written first against API stubs and failed behaviourally before each fix. Evidence: batch 1 (C1, C2, W2, S1, S3, S4): 6 of 7 new tests failed (`the_correction_waits_for_every_lens_and_closes_the_reports`, `the_cap_keeps_the_blockers_and_the_review_is_not_clean`, `malformed_lines_are_counted_per_lens_and_not_clean`, `unanswered_claims_settle_to_not_upheld_and_are_counted`, `a_verdict_is_final_and_closes_with_the_correction`, `a_review_that_needs_no_lens_is_clean_only_when_said_so`); the S4 test documents behaviour that already held and passed. Batch 2 (W3, W4, W5): 4 of 4 failed (`the_refuter_prompt_keeps_its_claims_whatever_the_listing_size`, `look_alike_finding_lines_are_counted_as_malformed`, `look_alike_claim_lines_are_counted_as_malformed`, `the_consent_ask_cleans_every_string_and_marks_a_cut_path`).

| Fix | Change |
|-----|--------|
| C1 | `begin_correction` is `Pending` until every picked lens reported; `pick`, `report`, `refute`, `settle_unanswered`, `no_lenses_needed` are refused with `CorrectionUsed` once it began; `open()` also lists blockers outside the correction's targets, so `conclude` is `BlockersOpen`, never `Corrected` (that last guard is unreachable through the API since the others hold) |
| C2 | the cap keeps blockers (severe + change) before the rest, in order found; `omitted` and `malformed` are kept per lens (`Review::counts()`, `omitted()`, `malformed()`); new `Outcome::Incomplete` when nothing blocks but anything was omitted, malformed or left unanswered |
| W2 | `Review::settle_unanswered() -> usize` (not upheld, counted by `unanswered()`); `refute` is final (`AlreadyJudged`) |
| W3 | `refuter_prompt` puts the claims before the staged listing |
| W4 | `parse_report` counts look-alike lines (bullet, quote, emphasis, list number, any case of `finding`) as malformed; `parse_verdicts` now returns `Verdicts{answers, malformed}` and counts look-alike claims, bad shapes and numbers naming no claim |
| W5 | `consent_ask` cleans the Raised and Failed strings; the local `one_line` marks a cut with `…` |
| S1 | `begin_correction`, `refute`, `settle_unanswered` check consent |
| S3 | `Review::no_lenses_needed()` (refused with picked lenses, then `pick` is `Waived`) lets a consented review conclude `Clean` |
| S4 | test documents that an echoed `FINDING` line can fabricate a blocker and start the one correction, never approve (accepted risk) |
| S5 | confirmed: the 4 KiB prompt test iterates all 4 lenses plus the refuter for both profiles (10) and asserts the count |

7b.2b API notes: `parse_verdicts` returns `Verdicts`; `Outcome::Incomplete` is new (a receipt must show it); after the refuter ends, call `settle_unanswered()` before `begin_correction`/`conclude`; ReviewError has `AlreadyJudged`, `LensesPicked`, `Waived`; `report`, `pick` and `refute` fail with `CorrectionUsed` after the correction began.

## S7b (RDD), PR S7b-3: task 7b.2, second half "7b.2b" (branch feat/agent-orchestrator-s7b3, on s7b2; 7b.2 now `[x]`)

Merged `feat/agent-orchestrator-s7b2` (PR #465 core fixes: `Verdicts`, `Outcome::Incomplete`, `settle_unanswered`, claims before the listing, sanitised consent) and adapted: the manager calls `settle_unanswered()` after the refuter and keeps `Verdicts.malformed` out of the transcript for 7b.3.

Strict TDD, RED observed first:
- core: `a_parked_worker_yields_its_slot...`, `raise_validates_its_arguments`, the seven-tool registry (3 of 3 failed against stubs/UnknownTool); `the_correction_prompt...` written after the function (no RED, a pure string builder).
- host (`orchestra_tests.rs`): 4 of 5 new tests failed before the hook (`a_high_change_asks_consent_first...`, `a_declined_consent_goes_on_unreviewed...`, `a_correction_worker_cannot_propose_the_batch`, `a_review_worker_reads_the_staged_view...`); the passive one passed from the start (guard). The view test needed a second change: a scoped read bypassed the staged view.
- runtime (`runtime/orchestra/tests.rs`): 7 manager tests failed against a stub `reviews()`/no-op `review` (declined, granted/clean, corrected, survives, unanswered, worker cap, raise); the end-to-end `declining_the_consent_leaves_the_workers_call_to_the_batch_review` (real `Waits`) passed on its first run.
- app: `a_review_line_reaches_the_transcript` (compile-RED then green).

| Unit | What |
|------|------|
| `host/review.rs` | `before_batch`: stages -> `assess` -> for `High` announces a call of its own (`review`, fresh `CallId`) with `consent_ask` built from the assessment and the staged ledger, awaits it, maps `Sent` to `Granted` and everything else but `ENDED` to `Declined`, then `Orchestra::review(..).await`; the batch is sealed and asked after, whatever was found. |
| `host.rs` | the hook before `files::seal`; `propose_batch` of a correction worker refused (`CORRECTION_PROPOSES`); `TurnScope::sees_stage` (read-only worker with a stage key): its scoped reads go through the staged view, kept in the root; `ToolHost::staged_changes`. |
| `tools/orchestra.rs` | `Orchestra::{assess, review, corrects}` with inert defaults. |
| `orchestra/review.rs` (runtime) | `RunManager::review` parks the waiting worker, starts lens workers (`risk`, `reliability`) and the refuter as `Task::Review` workers with the lens/refuter system prompt and the staged view, at most one `Task::Code` correction, then a refuter check of the targeted blockers over the re-read staged set; `Review::conclude` stored in `Live.review.reviews` (`RunManager::reviews`, the 7b.3 seam). |
| `Run::{park, unpark}` | a waiting worker does not hold a `max_parallel` slot; resuming never queues. |
| `raise` tool | `OrchCall::Raise`, the registry row (7 orchestration tools), the orchestrate prompts; `Raise::new` refuses passive and invisible reasons; the run keeps the highest raise. |
| `OrchestraEvent::Review{run, text}` | one transcript line: not reviewed / stopped (why) / clean / corrected / incomplete / blockers open. |

### Decisions and deviations
- W1: the consent is a call of its own announced before `propose_batch`, with its own `CallId`; its answer can never reach the worker's call (tests: distinct ids, `n` leaves the batch review, a stale consent id answers nothing).
- The orchestrator has no tool to pick lenses yet: the default pair is `risk` + `reliability` (`Review::pick` takes any; the orchestrator pick is a later seam).
- Review workers are ordinary run workers (visible to `wait`/`collect`, counted by `max_workers`, a handoff is asked of them or filled `unknown`).
- The correction is checked by a refuter run over the targeted blockers (uphold = still open; unanswered = open); the refuter prompt says an unsettled claim is refuted, so a validator that cannot tell reads as fixed: documented risk.
- The `raise` applies to the whole run (highest wins), not to one worker.
- A review that cannot start (worker cap, paused run, no nonce) says why in the transcript and the batch goes on unreviewed; a lens that fails halts the run like any failure and the review stops.
- Medium never asks (only `High` needs consent); each `propose_batch` of a high change asks again.
- The review time counts against the proposing worker's own deadline.
- Not done (7b.3): the receipt handoff and its line in the batch review.

### 7b.2b review fixes (branch feat/agent-orchestrator-s7b3)

Strict TDD, RED first (host: 7 of 7 new tests failed against the stubbed hook; runtime: 13 of 14 new tests failed or hung against the old manager, only `ctrl_c_during_a_review_ends_it_and_its_workers` was a guard that passed at once; prompts: the shape test failed on the refuter and the validator; the unsettled parse and `an_instruction_inside_a_finding...` were written after the code, a compile-RED and a pure string check).

| # | Fix | Test |
|---|-----|------|
| C1 | The correction check has its own `validator.md` (both profiles, `validator_builtin`) and only an explicit `refuted` clears a blocker; `unsettled` and silence leave it open. The refuter prompt says an unsettled claim is answered `CLAIM n unsettled: reason`, never refuted; `parse_verdicts` accepts it (not malformed, `Verdicts.unsettled`), it counts as unanswered so `settle_unanswered` gives `Incomplete`. A correction whose re-read staged set has no path with a new hash fixed nothing: the targets stay open. Per-target-path comparison is not possible (findings carry no path), so the test is on the whole set | `an_unsettled_claim_is_accepted...`, `the_lens_and_refuter_prompts_hold_the_review_rules`, `an_unsettled_correction_check_leaves_the_blocker_open`, `a_correction_that_staged_nothing_leaves_the_blocker_open` |
| W1 | After the review the host re-reads the batch; paths not in the assessed set go to `Orchestra::late` (`reviewed` or High): a line `staged after consent, not reviewed: <paths>` and `Reviewed.unreviewed` | `a_path_staged_during_the_review_is_reported_as_not_reviewed`, `a_high_path_staged_after_a_passive_assessment_is_reported`, `a_late_path_is_recorded_and_said_as_not_reviewed` |
| W2 | `Guard` in `RunManager::review`: on end or drop it cancels the workers it started and unparks the proposer. The drop path defers to a task (an abort can drop it while the manager lock is held) | `a_dropped_review_cancels_its_workers_and_unparks_the_proposer`, `ctrl_c_during_a_review...`, `cancelling_the_proposer_ends_its_review` |
| W3 | `correcting` is never cleared (a retried or relaunched correction cannot propose and is no second one); `viewers`, `systems` and `unread` are cleaned when the review ends | `the_correction_stays_marked_and_the_readers_are_cleaned` |
| W4 | A review needs room for 2 lenses and the refuter: otherwise nothing starts and the line says `could not start ... max_workers`; every worker started is cancelled by the guard | `a_review_that_cannot_fit_its_workers_starts_none`, `a_review_over_the_worker_cap_stops_visibly` |
| W5 | `Orchestra::review` returns `Result`: `Err` (run gone, manager gone) shows a row `review could not start: ...` and the batch goes on; `assess` of a gone run is `High` (Failed), not the rules alone; an ask nobody answered is `Answered::Unanswered(reason)` and recorded `consent not answered (reason)`, not declined; a failed read of the staged set is `None` and shows a row, never an empty (passive) set | `a_review_that_cannot_start_is_shown...`, `a_review_of_a_gone_run_cannot_start_and_its_assessment_is_high`, `a_consent_nobody_answered...`, `an_unanswered_consent_is_not_a_decline`, `an_unreadable_staged_set_is_said_and_never_passive`, `a_failed_read_of_the_staged_set_is_none_not_empty` |
| W6 | A review worker's read resolves its write target (works for a file only in the batch) and must be under the run's scope roots (`allows_write`); no prompt either way; outside it is refused and `Orchestra::unread` makes the review `Incomplete`. The read itself is kept in the chat root (`Within::Root`); the scope pin is the pre-check, a swap between check and read stays possible | `a_review_worker_reads_staged_only_files_in_scope_and_never_asks`, `an_unread_staged_file_makes_the_review_incomplete` |
| S1 | `OrchestraEvent::Review` doc admits the clipped blocker text | doc |
| S2 | the finding text is inside the wrapper; extra paths are W1 | `an_instruction_inside_a_finding_stays_inside_the_wrapper` |
| S4 | `park` false: `could not start: worker N is not running`, no worker starts | `a_review_for_a_worker_that_is_not_running_says_so` |
| S5 | a halted run, an ended proposer or a gone run end the wait: halted gives `Incomplete` with a line | `a_run_halted_during_a_review_ends_it_incomplete` |

Known limits kept: the lens pair is still the default; `Review::pick` by the orchestrator, the receipt (7b.3).

### 7b.2b re-review fixes

Strict TDD, RED first: the runtime tests for W-A (2), W-C and W-D (2) failed against the old manager (5 of 5; the check-cannot-start test also needed the trait to compile first), the host tests for W-D and W-E failed (2 of 2: the unread log held all five probes; no `changed` line), and the validation prompt test failed against a stub that returned the refuter prompt. The guard drop test was written after the code (no RED).

| # | Fix | Test |
|---|-----|------|
| W-A | Before the correction the room for it AND its check is counted (`worker_count` counts every slot ever spawned): with less, the correction is skipped with `the correction was skipped: ...` and the blockers stay open. A check that cannot start (a slot was taken meanwhile) is treated like a failed correction start: `the correction could not be checked: ...`, targets open, outcome `BlockersOpen`, review kept | `a_correction_without_room_for_its_check_is_skipped_visibly`, `a_check_that_cannot_start_keeps_the_blockers_open` |
| W-B | `validation_prompt` (rdd.rs): header `Claims to check: N`, and a lead that refutes only on clear evidence in the staged text that the problem is gone, else unsettled and open; replaces the `VALIDATE` lead and the refuter header | `the_validation_prompt_asks_for_clear_evidence_before_refuting` |
| W-C | The gate needs a path of the reviewed set whose hash changed; a new unrelated path counts for nothing: `the correction did not change the reviewed files` | `a_correction_that_only_stages_an_unrelated_file_leaves_the_blocker_open` |
| W-D | `Orchestra::review` returns the set as last reviewed (post-correction hashes for checked files). The host compares (path, hash) at seal time: new paths `Late::Staged`, reviewed paths with another hash `Late::Changed` (`changed after review, not reviewed: <paths>`, also in `Reviewed.unreviewed`) | `the_review_returns_the_hashes_it_last_reviewed`, `a_file_changed_after_its_review_is_said_as_not_reviewed`, `a_reviewed_file_restaged_during_the_review_is_reported_as_changed` |
| W-E | `unread` counts only a refused path that is a file of the staged set (`ReviewRead::RefusedStaged`); probes (`.git`, `node_modules`, `~`, absolute paths, outside the scope) are refused with a reason and count for nothing. An absolute path is refused as before (`resolve_write` takes relative paths only), uncounted | `a_review_worker_reads_staged_only_files_in_scope_and_never_asks` |
| S | A `Guard` dropped with no runtime says `its workers could not be cancelled (no runtime)` on the review line instead of failing silently | `a_guard_dropped_without_a_runtime_says_it_could_not_cancel` |

## S7b (RDD), PR S7b-4: task 7b.3 receipt handoff and its batch line (branch feat/agent-orchestrator-s7b4, on s7b3; 7b.3 now `[x]`, S7b complete)

Strict TDD, RED first: core `receipt_tests` 12 of 12 failed against a stub (`unimplemented!`/empty returns); runtime 4 of 4 receipt tests failed against the stub `receipt_line`/no emission; UI `a_receipt_line_heads_a_worker_batch...` failed behaviourally (header lacked the line). The `Run::receipt`, ledger-count and persist-key tests were written beside their code (no separate RED; one test of mine was wrong and was rewritten). Host assertion (`review.receipt()` through `seal`) compile-RED then green.

| Unit | What |
|------|------|
| `orchestra/receipt.rs` (core) | `Evidence`, `summary`, `judgement`, `line`: the receipt handoff built by lazarobox from the kept review (change = worker + staged paths, tier and source, consent word `granted`/`declined`/`not answered (why)`/`not asked`, lenses with why, correction, outcome in `done`; open blockers, not reviewed, could not read, omitted/malformed/unanswered in `left`; each finding with severity, cause, basis, blocked or not and the refuter word in `decisions`). Verdict is `blocked` while a blocker is open, else `unknown`: never `on_course`. Every dynamic string through `ledger::one_line`, clipped; lists show `+N more`; a field over 2 KiB ends `(+N more lines)` |
| `Unit::Receipt(worker, n)` | one handoff type (D17); key `orchestra/{run}/receipt/{worker}/{n}` (`persist.rs`); the app stores it like any handoff (redacted by the save) |
| `Run::receipt` | stores it (replaced by the same worker and n), facts from the reviewed worker; refused for an unknown worker or an ended run; ledger line `Reviews: N (M not reviewed, K blockers open)`, shrinking to `Reviews: N` as the last step of the 8 KiB bound |
| `Review` accessors | `consent_answer`, `refuter_word`, `correction_targets` |
| runtime `review.rs` | `Reviewed.paths`; `keep` and `late` call `receipt()`: the receipt is built and announced (`OrchestraEvent::Handoff`) after the review and again when a late or changed path is added (same key, an upsert); a receipt that cannot be stored is said on the review line. `RunManager::receipt_line`, `Orchestra::receipt_line` (default `None`) |
| batch review | `BatchReview::with_receipt` (worker batches only), `seal` asks the handle for the line, `ui/components/review.rs` header `label · receipt · counts…`; a chat's header is unchanged |

Decisions: a review that never began because only a late path was found (passive assessment) also gets a receipt (consent `not asked`, outcome `not reviewed`); several reviews of one worker are numbered 1.., the header shows the last. The header line is cut from the right in a narrow pane like the rest, so the gate text can go before it on very narrow panes.
Known limits: `findings carry no path`, so the receipt cannot say which path a blocker is in; the lens pick is still the default pair.

### 7b.3 review fixes

Strict TDD, RED first: UI header test (the receipt after the gate; 2 of 2 failed, the narrow 80/50 column test and the reordered expectation), core `a_clean_outcome_with_unreviewed_paths_does_not_read_clean_alone` and `a_single_line_over_the_cap_is_clipped_not_dropped` (2 of 2 failed), runtime `a_receipt_line_only_heads_the_batch_it_described`, `a_path_seen_late_twice_counts_once` and `a_receipt_of_a_run_that_is_gone_says_so` (3 of 3 failed against the old code).

| # | Fix | Test |
|---|-----|------|
| 1 | `Orchestra::receipt_line(run, worker, current)` takes the set being sealed (the host re-reads it in `seal`; unreadable = no line). `Reviewed.set` keeps (path, hash) as last reviewed; the line is shown only when every sealed path was reviewed as it is or is listed not reviewed (a late-only entry describes a set holding a listed path). A later batch wears no earlier receipt | `a_receipt_line_only_heads_the_batch_it_described` |
| 2 | The receipt note goes last in the header, after the gate and the refusal texts, so a narrow pane cuts it first | `a_narrow_pane_cuts_the_receipt_before_the_gate_or_the_refusal` (80 and 50 columns, batch and empty batch) |
| 3 | `clean`/`corrected` with unseen paths reads `clean (reviewed paths only)` in the line and in `outcome:` | `a_clean_outcome_with_unreviewed_paths_does_not_read_clean_alone` |
| 4 | `late` records a path once | `a_path_seen_late_twice_counts_once` |
| 5 | `receipt` and `receipt_line` say `(the run is gone)` on the review line | `a_receipt_of_a_run_that_is_gone_says_so` |
| 6 | `fit` clips a first line over the cap instead of dropping it | `a_single_line_over_the_cap_is_clipped_not_dropped` |

## S9 (run view and persistence), PR 9.1a: runs table and interrupted-run notice (branch feat/agent-orchestrator-s9, on s7b4; 9.1a `[x]`, 9.1b and 9.1c open)

Split planned up front in tasks.md: 9.1a store + interrupted notice, 9.1b run view (key), 9.1c resume choice + the two S4b between-turn gaps. Only 9.1a is done; the split follows the suggested one because the store, the view and the resume choice each have their own review surface.

Strict TDD evidence: core `record_tests` (5) were written beside `record.rs`, not RED first (one test of mine was wrong: `y` appears in "by"; fixed). The store, worker, app and manager tests were written with or just before their code; behavioural RED was seen when the existing tests that assert the exact table list (`a_fresh_store_gets_schema_v3`, `an_empty_database_at_version_zero_is_migrated`) and the two "toggle returns no effect" tests (`ctrl_o_turns_the_orch_mode_on_then_off`, `the_toggle_applies_to_the_next_message`) failed against the new behaviour and were updated deliberately; the first full run also failed my own `recovery_interrupts_only...` (wrong last assertion) and `more_than_three...` (a "more" line contains the counted words). Honest gap: not every unit was RED first.

| Unit | What |
|------|------|
| `core/ai/orchestra/record.rs` | `RunRecord`, `WorkerRecord`, `RunPhase` (`live`/`ended`/`interrupted`), `stored_goal` (sanitized, cut to 2 KiB on a char boundary), `interrupted_line` (goal first line, sanitized, clipped to 60 chars) |
| `runtime/chat_store/runs.rs` | tables `runs`, `run_workers`, `run_high` created `IF NOT EXISTS` by `migrate_locked` (target >= 3), no version bump: an older build ignores them and an old file gains them; `save_run` (upsert, replace workers, raise `run_high`, keep the newest 50 non-live runs), `recover_runs` (live + owner pid not this process + `kill(pid,0)` ESRCH becomes `interrupted`), `highest_run(path)` |
| `StoreCmd::SaveRun` / `RecoverRuns`, `StoreReply::RunsRecovered` / `RunsFailed{saving}` | worker handles, panic guard keeps the connection, a handle without a thread answers failures |
| `app/run_store.rs` | `RunKeep`: goal taken at the fresh turn, run saved on every `Worker` event, closed on `Ended`, `RecoverRuns` sent once per process at the first ORCH toggle-on, up to 3 interrupted runs named plus `N more runs...`, a save failure and a read failure said once each |
| `RunManager::with_run_floor` | the floor is read lazily once before the first id of the process (sync read of `run_high` at the first run start), `RunIds::raise` |

Decisions and limits:
- Gate OFF: no run command is ever sent; the three tables exist, empty (harmless: created by the migration for every user).
- Multi-instance safety: a second lazarobox must not interrupt the first's live run, hence `owner` pid plus a liveness check (pid reuse could keep a dead run `live`; it stays `live` in the DB, not shown, harmless).
- The notice appears at the first ORCH toggle-on after a start, not at process start: the store is opened lazily by design and a session that never orchestrates must not touch the run tables. A process that quits first leaves the runs `live`, found next time.
- A run only exists in the store from its first worker row (a run with no workers has nothing to resume).
- The floor read does a short synchronous SQLite open at the first run start of the process (on the chat runtime thread). A failure reads zero (the clock alone, as before) and the next save failure is said by `could not save the run`.
- Sequencing choice: run pane ids are not stored (they restart each process and mean nothing later).

### 9.1a review fixes

Strict TDD, tests first with an observed RED (commands run before any fix):
- App side (behavioural RED, 6 of 6 failed against the 9.1a code): `a_row_that_changes_nothing_stored_is_not_saved_again` (a usage-only row produced a save), `a_run_is_saved_as_its_workers_change...` (the save carried 2 workers, not the changed one), `closing_the_pane_of_a_live_run_closes_the_stored_run` (no close save on pane close), `a_failed_start_forgets_its_goal` and `closing_a_pane_forgets_its_goal` (1 goal kept, expected 0), core `the_stored_goal_hides_a_token_the_user_pasted` (the `sk-` token was stored). `the_interruption_line_hides_a_token_in_an_older_stored_goal` was written after the same change and passed at once (no RED for it).
- Store and manager side (compile RED: the tests use `Owner`, `Result` from `highest_run`, `next_run(pane)`, `OrchestraEvent::Notice`, so the build failed with 6 and 8 errors before the fixes). After the fixes the first full run failed only two older tests that encoded the old semantics (`saving_a_run_again_replaces...` now upserts; the worker round trip must change the token too), both updated on purpose.
- Verification of a premise, not a fix: `the_close_of_a_run_survives_a_long_queue` passed at once. Store commands go over an unbounded std queue (only replies use a bounded channel) and `shutdown` drains it, so a close save cannot be lost to a full `try_send`; the test pins that (300 saves, then the close, then shutdown).

| # | Fix | Tests |
|---|-----|-------|
| 1 | Owner is `Owner{pid, token}`: a per-process token (clock nanos mixed with the pid, drawn once). New nullable `runs.token` (in the `CREATE`, and `ALTER TABLE ADD COLUMN` for a table made without it; a row without a token counts as different). A row is recovered when its token differs from ours and its pid is dead or equals ours. Limit, documented on `Owner`: an unrelated live process holding a dead owner's pid is not told apart without the process start time, which is not read portably here, so that row stays `live` until the pid is free | `a_reused_pid_with_another_token_is_interrupted_and_the_same_token_is_not`, `a_row_without_a_token_is_found_when_its_pid_is_ours_or_gone`, `a_runs_table_without_the_token_column_gains_it_and_keeps_its_rows` |
| 2 | `highest_run` is read-only, no migration, `NOFOLLOW`, busy timeout 100 ms, missing file or tables read 0, errors are `StoreError` texts. The floor is read outside the `ids` lock (own `floor_read` lock). A failure sends `OrchestraEvent::Notice` once: `run ids could not be checked against stored runs: <why>` and ids go on from the clock | `a_locked_database_fails_the_floor_quickly_and_says_why`, `the_floor_reads_a_file_without_the_tables_as_zero_and_never_migrates_it`, `a_floor_that_cannot_be_read_is_said_once_and_ids_still_flow` |
| 3 | `Ended` is not guaranteed when a pane closes (the pane is removed first, then the manager ends the run). `keep_pane_closed` saves the run as ended, marks it ended and drops its `run_keep` entry and goal; a later `Ended` saves nothing | `closing_the_pane_of_a_live_run_closes_the_stored_run` |
| 4 | A row is saved only when a stored field changed (status, engine, model, kind); a save carries only the changed worker and the store upserts it and keeps the others; the close carries none. The ended save is never dropped (see the verification above) | `a_row_that_changes_nothing_stored_is_not_saved_again`, `a_save_upserts_the_workers_it_carries_and_keeps_the_others` |
| s1 | goal stored and shown through `redact_tokens(&clean(goal))` | `the_stored_goal_hides_a_token_the_user_pasted`, `the_interruption_line_hides_a_token_in_an_older_stored_goal` |
| s2 | the upsert no longer flips `interrupted` back to `live` (`WHERE runs.phase = 'live' OR excluded.phase = 'ended'`; its workers are not touched either); only the run's end closes it | `a_saved_live_run_never_flips_a_run_another_instance_interrupted` |
| s3 | the waiting goal is dropped when the turn fails with no run, and when the pane closes | `a_failed_start_forgets_its_goal`, `closing_a_pane_forgets_its_goal` |
| s4 | Gate OFF: the three tables (and the `token` column) are created by the migration for every user and stay empty; nothing writes to them unless a run happens (`with_the_gate_off_nothing_about_runs_is_sent`) | |

The added `OrchestraEvent::Notice` variant touches the two exhaustive matches in `runtime/orchestra/tests.rs` (one line each).

### 9.1a re-review follow-ups

Tests first, RED observed (6 failed against the previous code: close carried 0 workers on the manager and the pane-close paths, the next save after a `RunsFailed` carried 1 worker and an unchanged row was skipped, a cancelled turn kept its goal, the Notice wording).
- W1: the close (`Ended` and pane close) carries every known worker; a `RunsFailed{saving}` marks every run of that pane `resend` (the failure does not name a run), and the next save of each sends the full worker set once, even for a row that changed nothing.
- W2 (documented on `Owner` and `recover_runs`): two instances in different pid namespaces (containers) sharing one database with equal pids can falsely mark each other's live run as interrupted; like the reused unrelated pid, it needs the process start time to tell apart, which is not read portably.
- The floor Notice now says the protection is off for this session: `run ids could not be checked against stored runs (ids follow the clock this session): <why>`.
- `forget_goal` also runs from `cancel_chat` when the pane has no run (a turn cancelled before any run began).

## S9, PR 9.1b: full-pane worker transcript (branch feat/agent-orchestrator-s9b, on feat/agent-orchestrator-s9; 9.1b `[x]`, 9.1c open)

Strict TDD, RED first and observed per unit: core `run_trace_tests` (11 of 11 failed against `unimplemented!()` stubs, then 12 of 13 green after the store, the empty-transcript test failed alone before its line existed; `a_step_that_comes_after_its_run_ended_is_dropped` was added after the strict `orch_run == Some(run)` rule and was not seen red), runtime `a_workers_answer_reaches_the_app_as_it_streams`, `a_relaunched_worker_begins_a_new_transcript`, `a_routes_tool_rows_reach_the_app_as_trace` (3 of 3 failed against a no-op `Tracer`), prefix `capital_w_...` (failed: no `W` leaf), app `orchestra_view` (16 of 16 failed against no-op wiring, after making the two vacuous tests assert the view opened), UI `run_view_tests` (4 of 5 failed against an empty render; the tiny-pane no-panic test passes by design). `another_open_view_says_to_close_it_first` was added after the code (not seen red).

| Unit | What |
|------|------|
| `OrchestraEvent::Trace{run, worker, item}`, `TraceItem::{Begin, Text, Call, Result}` | what a worker's turn produced, raw; never logged |
| `runtime/orchestra.rs` `Tracer` | `launch` sends `Begin`; `drive` sends every sanitized `Delta`; `Route::deliver` sends the worker's `ToolCall`/`ToolResult` rows (the orchestrator turn still gets them) |
| `core/ai/chat/run_trace.rs` | `Traces` in `ChatState` (never in `entries`, so never stored or sent as history): per worker, 128 KiB (oldest dropped on a row/char boundary, `(earlier output dropped)`), 16 workers (oldest not on view dropped), sanitized again on entry, one line per tool row, orphan results ignored, `Begin` clears (retry), only the pane's run kept and a trace after `Ended` dropped; the view (`RunView`, scroll `top: Option<usize>` = follow when `None`) keeps its run's transcript until closed |
| `app/run_view.rs` | `Prefix W` (`view_worker`), keys of the open view, `refuse_over_run_view`; a failure alert closes the view; `chat.rs` routes view keys right after the approval, picker and folder checks |
| `ui/components/chat/run_view.rs` | title row (worker line), rows, key line; with an approval the prompt keeps its bottom rows |

Decisions: key `Ctrl+Space W` (capital like `T`, since `w` is the window group; `v`, `x`, `d`... are free but carry no meaning); unhinted root leaf (the root hint is full); gate off, no selection, a shell pane and another open view each show a notice. Ctrl+C is not swallowed in the view (it still cancels the selected worker / run); Esc closes the view instead of cancelling. Known limits: the transcript only holds what the app saw this process (nothing after a restart); a worker that ended is no longer selectable, so the view can only be opened on a live worker (an open one stays after its worker ends); a trace event is sent per delta (unbatched).

### 9.1b review fixes

Strict TDD, RED first and observed, in three commits.

| Commit | Fix | RED evidence |
|--------|-----|--------------|
| `fix(orchestra): trim a worker transcript by the excess and cap tool rows` | `trim` no longer pops a whole front Text: it drops only the excess, down to 7/8 of the cap (amortised), cutting the text on a char boundary and to a near row start; a part is popped whole only if it fits in the excess. A fresh trace that does not start with `Begin` (its start was evicted) shows `(earlier output dropped)`. Call and Result rows are capped at 512 bytes. Test helpers now send `Begin` first. | 4 of 4 failed: `a_call_after_a_full_text_drops_only_the_excess` (kept 739 bytes of ~128 KiB), `a_trim_cuts_down_to_leave_room`, `a_worker_traced_again_after_an_eviction_shows_the_cut`, `tool_rows_are_capped` |
| `fix(orchestra): coalesce worker text, redraw only the view on show and cache wrapped rows` | `Tracer` buffers text (flush at 4 KiB, after 50 ms, before any Call/Result/Begin and at the end of the turn, also on a deadline kill); a trace sets `dirty` only when its worker is on view; wrapped rows are cached per width and step (`Arc<Vec<Line>>`) | `the_wrapped_rows_are_kept_...` failed (ptr_eq); `small_deltas_are_sent_to_the_app_in_larger_pieces` failed against an immediate-send stub (1000 pieces); `a_trace_redraws_only_when_its_worker_is_on_view` failed ("no view" was dirty). `the_tail_of_a_worker_killed_by_its_deadline_is_not_lost` and `text_said_before_a_tool_row_comes_before_it` are guards that also pass against the stub (the stub never delays), so they were not seen red |
| third commit (wheel, paste, geometry, refusals) | wheel scrolls the view; paste refused with `Close the worker transcript first (Esc)`; key and wheel paging use the rows the last frame drew (`ChatState::run_view_drawn`, a `Cell` set by the render), so an approval prompt that takes rows cannot desync them; unused keys say `Esc closes the transcript`; `Prefix o` and `a` refuse while the view is open; the selected-worker hint drops the word `selected` so a 40-column pane wraps after `Ctrl+Space W transcript` instead of inside it | 6 of 6 failed: wheel (conversation, not view, moved), geometry with a pending approval (jump instead of 3 rows), paste, unused-key notice, `o`/`a` refusal, footer wrap |

Decisions: the geometry is shared through the render rather than recomputed in the app (the approval height lives in the UI layer); before the first frame the app falls back to pane rows minus 2. With an approval pending, the keys never reach the view (the approval takes them), so the mismatch showed through the wheel. The unused-key notice is not rate-limited: a notice is one replaceable line and held keys do not set it.

Follow-ups after the re-review: the tracer is flushed before `task.abort()` in `end` and `cancel_worker` (RED: `a_cancelled_workers_buffered_text_comes_before_its_cancelled_row` and `a_cancelled_runs_buffered_text_comes_before_ended` failed, the tail trailed the terminal row); `UNUSED_KEY` is set only when no notice is on the line (RED: `an_unused_key_keeps_the_notice_already_shown`, calling the handler directly, because every key clears the notice before its handler runs, so through the keyboard the guard only protects against a notice set earlier in the same event).

## S9, PR 9.1c: resume choice for an interrupted run and the S4b between-turn gaps (branch feat/agent-orchestrator-s9c, on feat/agent-orchestrator-s9b; 9.1c and 9.1 `[x]`, S9 complete)

Strict TDD, tests first with an observed RED each time:
- Gap notice: `an_ask_denied_between_turns_is_said_in_the_pane` failed (0 notices, expected 1) before `Tracer::denied_in_gap`; `Route::deliver` says the denial before `end_worker`, both when the route did not move and when it moved (the ask was denied by the retarget and is not retried).
- Re-listing: `an_interrupted_run_nobody_answered_is_named_again_until_it_ends` and the updated `recovery_interrupts_only_live_runs...` failed (the second ask returned nothing) before `recover_runs` selected `phase IN ('live','interrupted')` (only a live one needs the owner check and the UPDATE).
- Prefix: `r_is_an_unhinted_root_group_that_resumes_or_discards_a_run` failed to compile (no `ResumeRun`/`DiscardRun`); `every_action_is_reachable_exactly_once` count 35 -> 37 on purpose.
- Message: the three `resume_message` tests failed to compile (no method). `a_goal_that_contains_the_first_nonce_gets_another` passed at once (no RED; it pins the nonce search).
- App: `orchestra_resume.rs` (10 tests) was written with the prefix test, so its RED was the compile failure; afterwards a mutation (drop `keep_goal`) made `the_resumed_run_keeps_the_original_goal` fail, then reverted.

| Piece | What |
|-------|------|
| `RunRecord::resume_message` (core/record.rs) | seed text for the new run: run id, goal wrapped with `label::wrap` (sanitized, token-redacted, capped by the 2 KiB stored cap and `OUTPUT_CAP`), the unfinished workers by id/kind/engine/model (8 named, rest counted), "ask me for the read scope again", "do not relaunch any worker until I say so". The nonce has no random source in the app, so `nonce_for(run, goal)` walks a counter until the goal does not contain it (a goal cannot forge the closing tag) |
| `app/run_resume.rs` | `Ctrl+Space r r` resume, `Ctrl+Space r d` discard (unhinted root group `r` "run", labelled RUN; the root hint is full). Refusals each say why: gate off, not a chat, nothing interrupted, ORCH off in the pane (says Ctrl+O), a run active, pane busy or a draft in the composer (a send would clear it), engine not ready, tools refusal (the pane's own text) |
| resume | a NEW run through the normal send path (`send_with`, `attach: None`): fresh run id, normal scope ask (nothing of the old grant is stored or reused), `keep_goal` re-set to the original goal so the stored run does not nest seed text. The old row is closed `ended` by a `SaveRun` (no new phase or store command; `resumed` was not added) so it is not offered again |
| discard | same close, plus a pane line `run N discarded; it will not be offered again` |
| `RunKeep.interrupted` | the runs of `RunsRecovered`, newest first; each action takes the front one. The recovered notice now ends with a line naming both keys |
| store | `recover_runs` also returns runs still `interrupted` from an earlier start, so ignoring a run keeps it offered until it is resumed or discarded (it was named once only before) |
| gap 1 | `Tracer::denied_in_gap`: `worker N: a tool approval was refused between two orchestrator turns; the worker can ask again` (an `OrchestraEvent::Notice`) |

### Decisions and deviations
- Resume UI: a prefix group `r` (not a new unprefixed chat key, not a menu row). Chosen because the interrupted runs are named in the pane at the first ORCH toggle-on and a prefix leaf needs no modal state in the composer; the cost is two keys and the hint text in the pane line.
- Unfinished workers are only named in the seed; the orchestrator is told to propose and wait. Nothing relaunches by itself.
- The old row is `ended`, not a new `resumed` phase (the spec allows either; no schema or enum change).
- Gap 2 of `orchestra/s9-between-turn-gaps` (atomic ask registration plus announce in the host) is NOT done: the wait is registered in `ForTurn::ask` when the tool runs and the announce is a separate `StreamEvent` sent by the host, so making them atomic means changing the Approver contract across every tool path and the route, well beyond this PR's budget. The wait still ends at the next attach (`retarget` calls `end_worker`), at the total limit or at the run's end, as documented. Left as a follow-up.
- Dropped plain tool rows in the gap are no longer invisible: S9 9.1b traces every row to the worker transcript before delivery.

### 9.1c review fixes

Strict TDD, tests first with an observed RED each (commands run before the fix):
- W1 store: `a_runs_folder_is_stored_and_recovered...` failed (folder came back `None`), `a_run_that_supersedes_closes_the_old_row...` and `an_interrupted_run_older_than_thirty_days...` failed. The ALTER test passed at once (the column was optional for recovery; it pins the forward-compatible `ensure`).
- W1/W2/W3/S3 app (`orchestra_resume.rs`, 17 tests): 9 failed against the 9.1c code (cross-folder resume sent the goal, a NULL-folder run resumed, the notice showed other folders' goals, the old row closed at once, no `supersedes`, a failed/cancelled/ended turn lost the run, `OrchSend.goal` held the seed).
- W4: `asks_denied_in_one_gap_are_said_once_per_worker` and `an_ask_lost_to_a_moved_route_says_the_orchestrator_moved_on` failed with the dedup mutated off (the code was written with the refactor; RED was observed by flipping the condition, then restored).
- S2: `a_worker_field_with_line_breaks_stays_on_one_line` failed (a line break reached the seed). `a_goal_that_hides_the_nonce_behind_a_format_character_gets_another` passed at once (no RED): the goal was already cleaned before the nonce search; the fix makes the checked string the very one given to `wrap` (cleaned last).

| # | Fix |
|---|-----|
| W1 | `runs.folder` (nullable, added by `ensure` with the same ALTER pattern as `token`): the chat folder (`origin_cwd`, the one the conversations' `folder` column holds) of the first save, never changed by later saves. `r r` and `r d` act only on the newest interrupted run with a folder EQUAL to the chat's. A NULL folder (stored before this) is treated as another folder: never resumable or discardable from any pane (it ends by the 30-day cutoff). The startup notice names (goal, up to 3) only this folder's runs and counts the rest: `N runs interrupted in other folders; open a chat there to resume or discard them` (no path shown) |
| W2/W3 | Resume takes the run off this process's offer list into `RunKeep.resuming[pane]`; `keep_worker` of the new run's first worker (and a resend after a failed save) carries `supersedes`, and `save_run` closes that row with `UPDATE ... WHERE id = ? AND phase = 'interrupted'` (compare and set). If the turn fails, is cancelled, ends with no run, or the pane closes first, the run goes back to the list (`resume_not_started`) and the pane says `run N is still interrupted; resume it again with Ctrl+Space r r`. Residual: two instances can each resume the same row (the lists are per process); the second close is a no-op and no notice says so (not added: it would need a reply path from the store worker). A failed first save that is never resent leaves the row interrupted, offered again after the next restart |
| W4 | `Route::say_denied`: the gap notice is said once per worker and gap (`Current.said` holds the last gap). On the moved-route path it reads `worker N: a tool approval was refused when the orchestrator moved on; the worker can ask again`. `deliver` split into `deliver` + `send_from` so the moved path is testable |
| S1 | Wording softened (HOW_TO, CHANGELOG, seed): the read scope is NOT asked, it is set again from the chat folder; the orchestrator is told not to relaunch. Enforcement is documented in the spec delta note: only the prompt instruction ties a relaunch to the user's word. Traced `Run::spawn`: refusals are a paused or ended run, a `code` worker in a read-only step, and `max_workers`; workers start with no approval by user decision, so there is no code gate on relaunch. Real gates around a worker: the per-tool approvals (reads outside the scope follow the pane approval mode), staged writes with batch review, the jail, and the `max_parallel`/`max_workers`/turn/time limits |
| S2 | the nonce is searched against the string `wrap` receives (cleaned last); kind, engine and model are flattened to one line (whitespace runs become a space), cleaned, redacted and clipped |
| S3 | `OrchSend.goal` of the resumed turn WAS the seed (it feeds the run ledger and the feature document that go to the model, via `live.goal`). `RunKeep.resume_goal` is taken by `turn_effect`, so the goal is the stored one |
| S4 | `recover_runs` closes (`ended`) interrupted runs not updated for 30 days before selecting, so they are not named (simplest option; a row is touched at interruption time, so the age counts from the restart that found it) |
