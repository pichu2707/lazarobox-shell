# Tasks: Agent Orchestrator

Gates on every PR: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Strict TDD: each GREEN task follows its RED task; a REFACTOR step is folded into the last task of each PR. README untouched until S10 (D14). Spec pointer format: `capability / Scenario`. Paths are under `src/` unless stated.

## Review Workload Forecast (whole change)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~11,200 across 10 slices (S1 ~5,960 in 12 chained PRs, all decided). S2+ (~5,240) is NOT re-estimated yet; expect the same ~2x drift |
| 400-line budget risk | High (S1a ~1,160 and S1b ~1,130 already landed over budget; S1e-2 ~600 is one PR with `size:exception: approved`; S2+ exceed 400) |
| Chained PRs recommended | Yes |
| Suggested split | S1a -> S1b -> S1b-2 -> S1c-1 -> S1c-2 -> S1d-1 -> S1d-2 -> S1d-3 -> S1e-1a -> S1e-1b -> S1e-1c -> S1e-2 -> S2 ... S10 (S3 3 PRs, S9 3 PRs) |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: No (S1 split decided by the user on 2026-10-21; S2+ must be re-asked at their own apply time)
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Decisions (user, 2026-10-21):
- S1e is split into S1e-1 (app/core wiring) and S1e-2 (UI badge, prompts, insta snapshots).
- S1b is split: S1b (1b.1-1b.4 plus the `core/ai/mod.rs` wiring, `size:exception: approved`) and S1b-2 (1b.5-1b.8). S1b-2 is based on the S1b branch; S1c is based on S1b-2.
- S1b also includes fix commit 098fce2 (review fixes: pending-failure FIFO, queue FIFO, collect only on Done, deadline `checked_add`), +157/-28 lines. S1b total is ~1,130 changed lines excluding openspec; `size:exception: approved`.
- Re-estimate S1c, S1d, S1e-1, S1e-2 from real data before more apply work. Done below.
- S1c is split into S1c-1 -> S1c-2. DECIDED.
- S1d is split into S1d-1 -> S1d-2 -> S1d-3. DECIDED. The earlier S1d `size:exception: approved` (given against ~450) is withdrawn: no size exception for S1d parts.
- S1e-1 is split into S1e-1a -> S1e-1b -> S1e-1c. DECIDED.
- S1e-2 is NOT split: one PR (~600) with `size:exception: approved`.
- Scope of S1 is unchanged.
- Result: S1 = 12 chained PRs (S1a, S1b, S1b-2, S1c-1, S1c-2, S1d-1, S1d-2, S1d-3, S1e-1a, S1e-1b, S1e-1c, S1e-2), a linear chain, each based on the previous branch. Later slices S2+ (and S3/S4/S9) must be re-asked at their own apply time.

### Calibration (real vs estimate, lines changed excluding `.snap` and `openspec/`)

| Slice | Estimate | Real | Driver |
|-------|----------|------|--------|
| S1a | ~380 | ~1,160 (~250 production, ~900 tests) | 2 new menu rows churned ~10 existing menu test files (hardcoded row counts and positions) plus 24 snapshots |
| S1b part 1 (1b.1-1b.4 + mod.rs) | ~420 for all of S1b | 999 for about half (run.rs 367, tests.rs 325, tools.rs 198, worker.rs 69, mod.rs 39) | Production ~2x the guess; tests ~1:1 with production |

Rules of thumb used for the re-estimates: production ~2x the old guess, tests ~1:1 with production, plus churn where an exhaustive `match` or a struct literal is widened (for example `Entry`, `Call`, `TurnScope`, `Hub`). Whole-slice multiplier observed: 2.4x to 3x. Treat every figure below as +/-25%.

### Suggested Work Units (bases)

| Unit | Goal | PR | Base |
|------|------|----|------|
| S1a | Orchestrate row, gate keys, `availability_for(task)` | PR 1 | `feat/agent-orchestrator` |
| S1b | Pure core part 1: ids, `Toolset`, `OrchCall`, `Run`, `Roster`, worker types; `core/ai/mod.rs` wiring; review fixes 098fce2 (`size:exception: approved`) | PR 2 | S1a branch |
| S1b-2 | Pure core part 2: `ReadScope` (`scope.rs`), collect cap and wrapper, gates | PR 3 | S1b branch |
| S1c-1 | Tool registry and Hub seams: `registry_for`, `Call::Orchestra`, generic `Hub<K: HubKey>`, sweep | PR 4 | S1b-2 branch |
| S1c-2 | Host behavior: `TurnScope` fields, execute filter, scope check, ask label | PR 5 | S1c-1 branch |
| S1d-1 | Prompt per task, stub `orchestrate.md`, effective-engine "not recommended" mark | PR 6 | S1c-2 branch |
| S1d-2 | RunManager spawn path (`start`, `call`, worker turn, jail and scrub) | PR 7 | S1d-1 branch |
| S1d-3 | RunManager control path (`decide`, `cancel`, `shutdown`, retry) and `Chats::with_orchestra` wiring | PR 8 | S1d-2 branch |
| S1e-1a | Ctrl+O toggle, `orch` flag, send path, `app/orchestra.rs` skeleton | PR 9 | S1d-3 branch |
| S1e-1b | Notice and scope state, keys, config write effect | PR 10 | S1e-1a branch |
| S1e-1c | Failure state, retry/cancel keys, `Entry::Worker`, event handling | PR 11 | S1e-1b branch |
| S1e-2 | UI: ORCH badge, worker row, notice/scope/failure views, insta snapshots, final prompts, CHANGELOG (one PR, `size:exception: approved`) | PR 12 | S1e-1c branch |
| S7-1 | Write seams: change ledger, write scope, ledger calls, labelled batch review (no worker offered a write tool yet) | PR 13 | S1e-2 branch |
| S7-2 | `Toolset::Code` wiring, prompts, spec and CHANGELOG | PR 14 | S7-1 branch |
| S2..S10 | See below | one chain PR each (more where noted) | previous PR branch |

### Re-estimate summary (2026-10-21)

| PR | Old estimate | New estimate | Risk | Decision (user, 2026-10-21) |
|----|--------------|--------------|------|-----------------------------|
| S1a | ~380 | ~1,160 (real) | Done | none |
| S1b | ~420 (whole S1b) | ~1,130 (real, incl. fix commit 098fce2) | Done | none; `size:exception: approved` |
| S1b-2 | ~210 (half of S1b) | ~450 | Medium | none (under 500) |
| S1c | ~360 | ~650 | High | Split: S1c-1 (1c.1-1c.2, 1c.5-1c.6, ~340) + S1c-2 (1c.3-1c.4, ~330) |
| S1d | ~450 | ~1,200 | High | Split: S1d-1 prompts (~230) + S1d-2 spawn path (~520) + S1d-3 control path and Chats wiring (~480); no size exception |
| S1e-1 | ~250 | ~900 | High | Split: S1e-1a toggle and send (~330) + S1e-1b notice/scope (~300) + S1e-1c failure and `Entry::Worker` (~300) |
| S1e-2 | ~250 | ~600 | Medium | Not split: one PR, `size:exception: approved` |

## S1a: task row and keys (PR 1, ~380 lines estimated, ~1,160 real)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~380 estimated; real ~1,160 (~250 production, ~900 tests) |
| 400-line budget risk | Medium |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

- [x] 1a.1 RED `core/ai/config.rs` tests: slot 5 `Orchestrate` parses; absent = unassigned; `[ai.orchestrator]` defaults (enabled=false, notice_accepted=false, worker_timeout_secs=600); bad value falls back; older-build keys ignored. Spec: configuration / Slot parsed, Absent means unassigned, Defaults, Bad value falls back, Older build ignores keys; ai-task-models / Task set integrity.
- [x] 1a.2 GREEN `core/ai/config.rs`: append `Task::Orchestrate` at index 5 (slot indices hold), add `OrchestratorConfig`, parse and clamp (30..3600).
- [x] 1a.3 RED `core/config_edit.rs` + `runtime/config_write.rs` tests: save slot writes `[ai.tasks.orchestrate]`; clearing removes keys; `notice_accepted` write; no failure-picker persistence hook. Spec: config-persistence / Save the slot, Clearing removes the keys, Notice acceptance saved.
- [x] 1a.4 GREEN `core/config_edit.rs`, `runtime/config_write.rs`: own the new keys and the effect that writes `notice_accepted`. Note: S1a ships the pieces (`orchestrator_flag`, `accept_notice`, `changed_orchestrator`); the app-level write effect lands in 1e1.6.
- [x] 1a.5 RED `core/menu.rs` + `core/ai/menu.rs` tests (TestBackend + insta): six task rows plus `Orchestrator` gate row toggling on/off; picker first entry; only orchestrate changes; popup size stable; Set all excludes Orchestrate; Ollama marked "not recommended" on that row only; Orchestrate falls back to General. Spec: config-menu / Task rows rendered, Orchestrator row is the global gate, Picker first entry, Only orchestrate changes, Popup size is stable, Fallback threshold follows the popup, Marks on rows; ai-task-models / Orchestrate falls back, Ollama marked not recommended, Other tasks unmarked, Set all leaves the orchestrator alone.
- [x] 1a.6 GREEN `core/menu.rs`, `core/ai/menu.rs`: rows, gate toggle, picker, marks, Set all exclusion.
- [x] 1a.7 RED `core/ai/chat/engine/availability_tests.rs`: availability per task, no fallback engine, server model must be detected, empty server model, CLI model not validated, effective assignment, worker kind uses its own row, Summarize row used. Spec: chat-engines / Availability per task, No fallback engine; ai-task-models / Effective assignment used, Fallback to general, Server model must be detected, Empty server model, CLI model not validated, Assignment never changed, Worker kind uses its own row, Summarize row is now used.
- [x] 1a.8 GREEN `core/ai/chat/engine.rs`: `availability_for(task, ...)` replaces the hard-wired `Task::Chat` (:281); keep a Chat wrapper so call sites compile.
- [x] 1a.9 Off-path characterization: snapshot proves chat with gate off is unchanged vs 0.18.0 (first written, passes before changes). Spec: agent-orchestrator / Disabled offers no tools.
- [x] 1a.10 REFACTOR + gates.

## S1b: pure core, part 1 (PR 2, ~1,130 lines real, `size:exception: approved`)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~420 originally for all of S1b; real ~1,130 excluding openspec: 999 for 1b.1-1b.4 plus the module wiring (run.rs 367, tests.rs 325, tools.rs 198, worker.rs 69, mod.rs 39), plus +157/-28 for fix commit 098fce2 |
| 400-line budget risk | High (exceeded) |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No (decided: split S1b, remaining tasks move to S1b-2; `size:exception: approved`, user 2026-10-21) |

Label the PR `size:exception`. Commits 3b1653e, 7d9899d, and 098fce2 (review fixes: pending-failure FIFO, queue FIFO, collect only on Done, deadline `checked_add`; +157/-28). The `core/ai/mod.rs` wiring (old 1b.8, second half) shipped here. Base: S1a branch.

- [x] 1b.1 RED `core/ai/orchestra/tests.rs`: `OrchCall` parse (spawn kinds general|code|chat|review|summarize; Orchestrate refused; wait clamped to 120 s; collect/cancel). Spec: agent-orchestrator / Workers cannot spawn, Bad kind refused, Wait is clamped.
- [x] 1b.2 GREEN `core/ai/orchestra/{mod,tools}.rs`: `RunId`, `WorkerId`, `Toolset`, `OrchCall`, tool schemas, parse.
- [x] 1b.3 RED `tests.rs`: `Run` transitions with explicit `Instant`s: spawn refused when Not Ready (`Refused{reason}`, no fallback), max 1 running (extras queue), deadline -> TimedOut, collect before done, cancel after done, failure -> Halted, halted blocks spawn / wait returns `awaiting_user`, retry reuses same spec, cancel. Spec: agent-orchestrator / Review worker runs on the Review assignment, Unassigned kind falls back as the menu says, Menu change after spawn, No jail refuses, Model missing refuses, Deadline kills the worker, Collect before done, Error shown as it came, No silent fallback, Orchestrator paused, Cancel after done.
- [x] 1b.4 GREEN `core/ai/orchestra/{worker,run}.rs`: `WorkerSpec`, `WorkerStatus`, `WorkerResult`, `RunState`, `UserChoice`, `Roster`, `Run`.
## S1b-2: ReadScope and collect wrapper (PR 3, ~450 lines, risk Medium)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~450 (scope.rs ~150 production + ~200 tests; collect cap and wrapper ~40 production + ~60 tests) |
| 400-line budget risk | Medium (slightly over 400; under the 500 split threshold) |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No (user decision 2026-10-21: S1b-2 = 1b.5-1b.8; over 400 is expected, label `size:exception` if it lands above 500) |

Base: S1b branch (`feat/agent-orchestrator-s1b`). S1c is based on this branch. Task IDs 1b.5-1b.8 are kept from the original plan.

Design notes grounded in the code: `paths::resolve(roots: &SecretRoots, origin, input) -> PathVerdict` (`core/ai/tools/paths.rs:328`) already does the deny-list and symlink-aware resolution; `ReadScope` reuses it and adds only the "inside the root" check on both the textual and the resolved path. The scratch roots (`/tmp`, `/private/tmp`, list at `paths.rs:172-186`) must be dropped from a scope root, but test tempdirs live under `/tmp`, so `ReadScope::new` takes an injectable scratch predicate (`&dyn Fn(&Path) -> bool`) whose production default is the `paths.rs` scratch list; tests inject a predicate that matches nothing (or a test-only root).

- [x] 1b.5 RED `core/ai/orchestra/tests.rs`: `ReadScope`: root passes `paths::resolve`, `$HOME` and broad scratch roots dropped (scratch decided by the injected predicate), both textual target and resolved path must be inside, `..` and symlink escape, deny-list wins, scope expires with run, denied scope. Spec: agent-orchestrator / Read inside the scope runs without asking, Deny-list wins inside the scope, Requested and resolved path both checked, Excluded root dropped, Scope expires with the run, Denied scope; agent-tools / Excluded under scope.
- [x] 1b.6 GREEN `core/ai/orchestra/scope.rs` reusing `core/ai/tools/paths.rs` (`resolve`, `SecretRoots`), with the injectable scratch predicate; production default uses the existing scratch list.
- [x] 1b.7 RED+GREEN `collect` cap 32 KiB and wrapper via `label::wrap(nonce, ...)` (nonce is a parameter; no runtime dependency), control sequences removed, wrapper unforgeable. Spec: agent-orchestrator / Collect caps text, Wrapper cannot be forged, Control sequences removed; agent-tools / Wrapped.
- [x] 1b.8 REFACTOR + gates (`cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`). The `core/ai/mod.rs` wiring is already done in S1b; only re-export `ReadScope` here if needed.
- [x] 1b.9 Follow-ups from the S1b re-review (098fce2), RED first: (a) extend `a_second_failure_while_halted_...` to assert the final promote follows queue order; (b) test `cancel_run` with a non-empty `pending`; (c) test a Refused worker's retry landing in `pending`; (d) test `Relaunch` with a pending failure; (e) reset `slot.started = None` before the refusal branch in `enqueue`, so a refused retry reports zero elapsed time.

## S1c: host and hub seams (2 chained PRs: S1c-1 = PR 4, S1c-2 = PR 5; ~650 lines re-estimated in total)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~650 in total (was ~360), split ~340 + ~330 below. `core/ai/tools/mod.rs` ~90 production + ~80 tests (the new `Call::Orchestra` variant widens every exhaustive `match` on `Call`: `name`, `argument`, `host.rs::execute`, `ask_for`); `runtime/tools/host.rs` ~120 production + ~200 tests (`TurnScope` struct literal at `runtime/chat.rs:693` and in tests, execute filter, scope check, ask label field on `ApprovalAsk`); `runtime/mcp/session.rs` ~70 production + ~100 tests (`Hub` is `HashMap<PaneId, Live>`, socket name at `:179`, `sweep` at `:284` parses only `s-<pid>-<digits>.sock`) |
| 400-line budget risk | Medium per PR (~340 and ~330, each under 400; +/-25%) |
| Chained PRs recommended | Yes |
| Decision needed before apply | No (split decided, user 2026-10-21) |

**Split (decided, user 2026-10-21):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1c-1 | 1c.1, 1c.2, 1c.5, 1c.6 (`registry_for`, `Call::Orchestra`, generic `Hub<K: HubKey>`, sweep form) | ~340 | S1b-2 branch | Both are mechanical seams with no behavior change on the default path |
| S1c-2 | 1c.3, 1c.4 (`TurnScope` fields, execute filter, scope check, ask label) | ~330 | S1c-1 branch | The behavioral host change; uses `Call::Orchestra` and `ReadScope`; reviewed on its own |

S1d-1 bases on the S1c-2 branch. Task order inside S1c-2 is 1c.3 then 1c.4; S1c-1 runs 1c.1, 1c.2, 1c.5, 1c.6 in that order.

### S1c-1 (PR 4, ~340 lines, base: S1b-2 branch): tasks 1c.1, 1c.2, 1c.5, 1c.6

- [x] 1c.1 RED `core/ai/tools/mod.rs` tests: `registry_for(Toolset)`: Orchestrator lists spawn_agent/wait/collect/cancel (no `run_in_pane`), ReadOnly lists find_dir/list_dir/read_file only, default registry byte-identical. Spec: agent-tools / Role toolsets; agent-orchestrator / No run_in_pane, Read-only subset; lazarobox-mcp-server / Out-of-role call.
- [x] 1c.2 GREEN `core/ai/tools/mod.rs`: `registry_for`, `Call::Orchestra(OrchCall)`.
- [x] 1c.5 RED `runtime/mcp/session.rs` tests: `Hub<K: HubKey>` with `PaneId` and `WorkerKey{run,worker}`; socket `s-{pid}-w{run}-{n}.sock`; `sweep` knows the form. Spec: lazarobox-mcp-server / Own socket per worker (S3 behavior; S1 delivers the key form only).
- [x] 1c.6 GREEN `runtime/mcp/session.rs`: generic Hub key, sweep. REFACTOR + gates (S1c-1 closes here).

### S1c-2 (PR 5, ~330 lines, base: S1c-1 branch): tasks 1c.3, 1c.4

- [x] 1c.3 RED `runtime/tools/host.rs` tests: out-of-set call refused in `execute`; `Call::Orchestra` without handle = `UnknownTool`; read inside `ReadScope` skips the ask only when both paths inside; orchestrator never approves; label travels with the ask; closed session denies; wait bound held. Spec: agent-tools / Inside runs, outside asks; agent-orchestrator / Orchestrator cannot approve, Outside read asks with a label, Never silently decided; lazarobox-mcp-server / Bound held, Label travels, Closed session denies.
- [x] 1c.4 GREEN `runtime/tools/host.rs`: `TurnScope{orchestra: Option<OrchHandle>, read_scope, toolset}`, execute filter, ask label. REFACTOR + gates (S1c-2 closes here). Done: `OrchHandle` is `Arc<dyn Orchestra>` (`runtime/tools/orchestra.rs`, the RunManager implements it in S1d-2); `read_scope` is `Option<WorkerScope{handle, run, label}>`; a chat's `toolset` is its request's families; the ask is `ApprovalAsk::WorkerRead{path, label}` with label `<kind> <engine>><model>`; the host bounds `wait` at 120 s with `awaiting_user`. A worker read outside the scope asks on every engine, local ones included (user, 2026-10-27; agent-tools spec wording aligned).

#### Obligations from the S1b-2 security review (S1c-2)

- [x] The read opens exactly the path returned by `ReadScope::allows` with open_beneath/O_NOFOLLOW semantics and re-judges the opened fd's real path (`SecretRoots::judge_real` + `starts_with(root.real)`); the `SecretRoots` snapshot is built per call; consider an ask when `nlink > 1`. Done: the scope check and the open share one snapshot built for the call; `open_checked` proves the fd is that path and judges it again. Decision (parent, 2026-10-27): a scoped read of a file with `nlink > 1` stays refused, with no ask (the stricter behaviour of every read).
- [x] A denied or expired scope makes the host ASK, never silently allow or refuse.
- [x] The ask label MUST clip `OrchCall::argument()` (a spawn prompt can be up to 16 KiB) before it is shown. Done: an orchestration call never asks; its argument is shown only in the row summary, cut by `summary::clip` to `SUMMARY_CAP` (120 characters), and the worker label's engine and model are cut like a server name (64 characters each).

#### Obligation when the chain picks up main (from the S1c-2 native review, `R3-scoped-read-drops-lines`)

- [x] The chain branched from main at f3fc629 (#289), before coding-tools M1-b, so its `read_file` has no paging. On main, `fs::execute_with` passes the call's `lines` window (`start_line` / `max_lines`) and the staged view to `read_file` (`src/runtime/tools/fs.rs:217`). When main is merged into the chain, the scoped read MUST do the same: `read_in_scope` / `fs::read_real` take the call's `lines` and the view and page exactly as a chat read does. Add a RED test first: a worker read inside its scope with `start_line` / `max_lines` returns that window and the paging markers, like the same chat read. Decision (user, 2026-10-27): record it here and fix it at the sync, not before. Done at the sync (06495f3): `fs::read_real` pages the scoped read like a chat read; test `orchestra_tests::a_scoped_read_is_paged_and_keeps_its_refusals`.

## S1d: RunManager (3 chained PRs: S1d-1 = PR 6, S1d-2 = PR 7, S1d-3 = PR 8; ~1,200 lines re-estimated in total)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,200 in total (was ~450), split ~230 + ~520 + ~480 below. `runtime/orchestra.rs` RunManager ~400 production (start, call dispatch for the four tools, worker turn through `runner_with` + `Pipe`, jail and env scrub reuse, own event channel, timers, cancel, shutdown, decide/retry) + ~500 tests (fake `Runner`, paused tokio time, temp HOME, real Hub, 9 scenarios); `runtime/chat.rs` + `runtime.rs` wiring (`Chats::with_orchestra`, `cancel`/`shutdown` forward; `runtime.rs` is ~4,200 lines) ~100; prompts (`runtime/prompts.rs`, `core/ai/chat/prompts.rs` real arm replacing the `Task::Orchestrate` fallback at `:53`, 2 stub `.md`) ~30 production + ~60 tests; 1d.6 follow-up ~40 + ~40 tests. Calibration: S1b's `run.rs` alone was 367 lines for a pure state machine |
| 400-line budget risk | High (S1d-2 ~520 and S1d-3 ~480 stay above 400 even after the split) |
| Chained PRs recommended | Yes |
| Decision needed before apply | No (split decided, user 2026-10-21). The earlier S1d `size:exception: approved` (given against ~450) is withdrawn: no size exception for S1d parts |

**Split (decided, user 2026-10-21):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1d-1 | 1d.1, 1d.2, 1d.6 (prompt per task, stub `orchestrate.md`, effective-engine "not recommended" mark) | ~230 | S1c-2 branch | Independent of the RunManager; touches prompts and menu only |
| S1d-2 | 1d.3a, 1d.4a, spawn path: `start`, `call` (spawn/wait/collect), worker turn, jail and scrub, Done/Failed/TimedOut/Refused, no tool leak, log hygiene | ~520 | S1d-1 branch | The core of the manager; testable with a fake `Runner` and no UI |
| S1d-3 | 1d.3b, 1d.4b, control path: `decide`, `cancel`, `shutdown`, halt then retry, worker read reaching the approver; plus 1d.5 `Chats::with_orchestra` and `runtime.rs` wiring | ~480 | S1d-2 branch | Control and integration into `Chats`; reviewable once the spawn path is settled |

S1e-1a bases on the S1d-3 branch.

### S1d-1 (PR 6, ~230 lines, base: S1c-2 branch): tasks 1d.1, 1d.2, 1d.6

- [x] 1d.1 RED `runtime/prompts.rs` tests: prompt per task (worker gets its kind's prompt), override still untrusted, built-in `orchestrate.md` per profile exists. Spec: prompt-profiles / Worker gets its kind's prompt, Override still untrusted, Built-ins exist, Docs deferred.
- [x] 1d.2 GREEN `runtime/prompts.rs`, `core/ai/chat/prompts.rs`, `prompts/*/orchestrate.md` (stub text; final wording in S1e-2/S5). Done: `load_prompt(dir, profile, kind)` already resolves any task, so a worker loads its own kind (tests pin it, override rules included); the two `orchestrate.md` hold the coordinator rules of the Orchestrate prompt requirement.
- [x] 1d.6 Follow-up from S1a review: the "not recommended" mark checks only an explicit `orchestrate` assignment. When the slot is unassigned and falls back to a server engine in `general`, mark it too (use the effective engine, `resolve(Task::Orchestrate)`, in `task_label` and the engine list). RED test first. REFACTOR + gates (S1d-1 closes here). Done: `task_label` marks by `resolve(task)`, so `(default) · not recommended` when general is a server; the engine list already marked every server on the orchestrate picker, assignment or not.

### S1d-2 (PR 7, ~520 lines, base: S1d-1 branch): tasks 1d.3a, 1d.4a

- [x] 1d.3a RED `runtime/orchestra/tests.rs` (fake `Runner`, `tokio::time::pause`, temp HOME, real Hub), spawn path: spawn -> Done/Failed/TimedOut/Refused; worker same jail as chat; API keys survive scrub; no tool leak to workers; log hygiene (no content in logs). Spec: chat-engines / Same jail as chat, API keys survive the scrub; agent-orchestrator / Unusable orchestrator engine, Log hygiene. Done: 13 tests in `runtime/orchestra/tests.rs`; the scrub already kept the three API-key variables (they are not in `SCRUBBED`), so that scenario is one test in `chat/cli/tests.rs` and no jail code changed.
- [x] 1d.4a GREEN `runtime/orchestra.rs`: `RunManager::{start, call}` (spawn/wait/collect); worker turn via `runner_with` + `Pipe`; events on their own channel `(PaneId, OrchestraEvent)`. REFACTOR + gates (S1d-2 closes here). Done: `start(pane, OrchScope)` returns `(RunId, OrchHandle)`, a per-run handle that implements `Orchestra` (the trait's `call` names no pane, so the turn is bound in the handle; `turn` travels in the scope's approver); an unusable `orchestrate` engine starts no run and returns the guidance. A worker turn is the chat's `Runner` inside `chat::in_session` (the task-locals a chat turn gets), read through a fresh `Pipe`, cut at the spec's deadline; events are `OrchestraEvent::{Worker(row), Halted}` on an unbounded channel (`Ended` comes with `cancel`/`shutdown` in S1d-3); the log is a content-free `LogLine` (ids, kind, status, class). The `cancel` tool answers "not available yet" until S1d-3.

#### Obligations from the S1c-1 review (S1d-2)

- [x] The worker-open path MUST pass `toolset.families()` to `Hub::open`, never `FamilySet::LEGACY`. Add a test (or a `debug_assert!`) that `Family::Pane` is absent from every worker and orchestrator session. Done: both a `debug_assert!` in `open_session` and test `a_worker_session_lists_only_the_read_tools_and_refuses_the_rest` (real Hub: lists `find_dir`, `list_dir`, `read_file` only, refuses forged `spawn_agent` and `run_in_pane`); the orchestrator's set is pinned in `core/ai/orchestra/tests.rs`.
- Note: user MCP tools can no longer reach a worker by construction; `HubKey::allows_external` (false for `WorkerKey`) makes `Hub::open` ignore the caller's `Slot`. Keep the worker path passing `Slot::default()` anyway.

### S1d-3 (PR 8, ~480 lines, base: S1d-2 branch): tasks 1d.3b, 1d.4b, 1d.5

- [x] 1d.3b RED `runtime/orchestra/tests.rs`, control path: cancel kills the process; halt then retry relaunches same spec with the previous error appended as untrusted data; worker read outside scope reaches the approver; Ctrl+C cancels all; shutdown. Spec: agent-orchestrator / Retry relaunches the same spec, Ctrl+C cancels all. Done: 11 tests (retry, paused until decided, cancel choice, cancel one by tool, cancel after done, worker read asks and cancel denies it, Ctrl+C through `Chats`, shutdown, dropped manager, replaced run `Ended`, timed-out error wrapped) plus one `chat/tests.rs` test for a chat with no run manager; each observed failing against stubs before GREEN.
- [x] 1d.4b GREEN `runtime/orchestra.rs`: `RunManager::{decide, cancel, shutdown}`. Done: `decide(pane, UserChoice)` drives `Run::decide` (`Retry` stores the alert text, and the relaunch's prompt is the spec's followed by it under `label::wrap`; `Cancel` ends the run); `cancel(pane)` and a replaced run end through one path: workers `cancelled`, turns aborted, sessions closed (their asks end denied), rows, then `OrchestraEvent::Ended{run}`, nothing after; `shutdown()` ends every run at the call, refuses later starts, drops the worker hub and returns a future that resolves once every worker turn is gone; the `cancel` tool cancels one worker (`already ended: <status>` for a finished one); a dropped run aborts its turns (S1d-2 advisory A4).
- [x] 1d.5 GREEN `runtime/chat.rs` (`with_orchestra`, `cancel`/`shutdown` forward), `runtime.rs` wiring. REFACTOR + gates (S1d-3 closes here). Done: `Chats::with_orchestra(Arc<RunManager>)`; `cancel` ends the pane's run before its turn and waits; `shutdown` waits for the runs inside its bound; `runtime.rs` builds one `RunManager` on the chat's runner, prompt folder and socket folders. Its events are dropped until S1e-1a hands the receiver to the app; with no manager attached every chat path is unchanged.

#### Obligations from the S1d-2 review (S1d-3)

- [x] Replacing a run on the same pane MUST send `Ended` for the old run (today its worker is killed with no app event; test `replacing_a_run_kills_the_old_worker` pins the kill). Done: a replaced run ends like a cancelled one, its worker's `cancelled` row then `Ended`; test `replacing_a_run_sends_ended_for_the_old_run`.

#### Advisories from the S1d-3 verifier (2026-10-29), for later slices

- [x] After a retry, an already queued worker starts before the retried one (`Run::enqueue` puts the retry at the back); decide in S2 whether a retry should start first. Decided (user, 2026-10-30): the retried worker starts BEFORE the queued ones. Done in S2a: `Run::enqueue(.., front)`; `decide` queues at the front, `spawn` at the back; two tests changed to pin the new order.
- [x] When no nonce can be drawn, `decide(Retry)` sends the plain prompt without the error; consider returning an error as `collect` does (S2). Decided (user, 2026-10-30): return an error, never the plain prompt. Done in S2a: `RunManager::decide` draws the nonce before the run changes and returns `DecideError::NoNonce` (the run stays halted, nothing relaunches); the wrapped failure is stored at decide time.

#### Obligations from the S1b-2 security review (S1d-2, S1d-3)

- [x] (S1d-2) Nonce bytes per call from the OS CSPRNG; never reused or shown to the worker. Done: `collect` reads 8 bytes of `/dev/urandom` per call (as `tools::new_nonce` does) and returns no text when it cannot; test `every_collect_wraps_under_a_fresh_nonce_the_worker_never_saw`.
- [x] (S1d-2) RunIds are allocated monotonically and never reused across restarts; a resumed run re-asks the scope. Done: a run id is the wall clock in microseconds, always above the last id handed out; a read scope lives in memory with its run only, so a later run or process has none until the user answers again; tests `run_ids_grow_with_the_clock_and_never_repeat`, `a_scope_belongs_to_its_run_and_a_new_run_asks_again`.
- [x] (S1d-3) Worker error text (failed/timed-out) is wrapped with `label::wrap` too, not only Done text. Done: the orchestrator never gets a worker's error text (`wait` says `awaiting_user`, `collect` says `ended <status> with no text`); the only model that does is the retried worker, under a fresh `label::wrap` nonce, or not at all when no nonce can be drawn; tests `retry_relaunches_the_same_spec_with_the_error_wrapped`, `a_timed_out_worker_error_reaches_a_model_only_wrapped`.

## S1e-1: app/core wiring (3 chained PRs: S1e-1a = PR 9, S1e-1b = PR 10, S1e-1c = PR 11; ~900 lines re-estimated in total)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~900 in total (was ~250), split ~330 + ~300 + ~300 below. Production ~450: `orch` flag and toggle with debounce ~80, send path at `app.rs:925` (`resolve(Task::Chat)`) plus Roster build from `availability_for` per task ~80, notice and scope state and keys ~150, failure state, keys and event handling ~100, config write effect ~40; `Entry::Worker` widens the exhaustive matches in `core/ai/chat/mod.rs` (`:542`, `:971`), `transcript.rs`, `tool.rs` and the `ui` list ~40. Tests ~450: four new `app_tests/orchestra_*.rs` files at ~110 each, state asserts only. `app.rs` is ~7,500 lines, so new logic goes in `app/orchestra.rs` |
| 400-line budget risk | Medium per PR (~330, ~300, ~300; +/-25%) |
| Chained PRs recommended | Yes |
| Decision needed before apply | No (split decided, user 2026-10-21) |

State and key handling only; no rendering (snapshots land in S1e-2).

**Split (decided, user 2026-10-21):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1e-1a | 1e1.1-1e1.4 (Ctrl+O toggle, `orch` flag, send path, `app/orchestra.rs` skeleton) | ~330 | S1d-3 branch | Smallest end-to-end behavior: an orchestrating pane sends `Task::Orchestrate` |
| S1e-1b | 1e1.5-1e1.6 (notice state, keys, config write effect; read scope from the chat origin) | ~300 | S1e-1a branch | One concern: first-run notice and the read scope of a run (no scope prompt, user decision 2026-10-29) |
| S1e-1c | 1e1.7-1e1.8 (failure state, retry/cancel keys, `Entry::Worker`, event handling) | ~300 | S1e-1b branch | Failure handling plus the `Entry` enum widening |

S1e-2 bases on the S1e-1c branch.

### S1e-1a (PR 9, ~330 lines, base: S1d-3 branch): tasks 1e1.1-1e1.4

- [x] 1e1.1 RED `app_tests/orchestra_toggle.rs` (state asserts, no insta): Ctrl+O toggles `orch`, `o` debounced (held key toggles once), per-pane independence, no-op with gate off, blocked during a run, existing keys unchanged, gate off-then-on, Enabled alone does not orchestrate, toggle applies to next message. Spec: modal-input / Ctrl+O toggles, Held key toggles once, Existing keys unchanged; chat-pane / Toggle is per pane, Toggle does nothing with the gate off, Toggle blocked during a run; agent-orchestrator / Enabled alone does not orchestrate, Toggle applies to the next message, Panes are independent, Gate off then on. Done in `app_tests/orchestra_toggle.rs` (S1e-1a, #401): badge by gate and pane mode, toggle, held Ctrl+O once, per-pane, gate off, blocked during a run, existing Ctrl keys unchanged.
- [x] 1e1.2 GREEN `app/chat.rs`, `core/ai/chat/mod.rs` (`orch` flag, in memory). Verify Ctrl+O is not swallowed by terminal/multiplexer (design open item). Done (S1e-1a, #401): `orch` flag in memory, Ctrl+O in `app/chat.rs`. Whether a terminal or multiplexer swallows Ctrl+O is not verified here.
- [x] 1e1.3 RED `app_tests/orchestra_send.rs`: orchestrating pane sends `Task::Orchestrate` with Roster (menu), others `Task::Chat`; Orchestrate engine runs the turn; unusable engine shown. Spec: agent-orchestrator / Orchestrate engine runs the turn, Unusable orchestrator engine. Done in `app_tests/orchestra_send.rs` (S1e-1a, #401): orchestrating pane sends `Task::Orchestrate` with the roster, others `Task::Chat`, unusable engine shows its guidance.
- [x] 1e1.4 GREEN `app.rs` send path (:925), `app/orchestra.rs` (new). REFACTOR + gates (S1e-1a closes here). Done (S1e-1a, #401): send path in `app.rs`, `app/orchestra.rs`.

### S1e-1b (PR 10, ~300 lines, base: S1e-1a branch): tasks 1e1.5-1e1.6

- [x] 1e1.5 RED notice state/key tests and read scope tests: notice shown once and never blocks, Enter accepts and persists only with an empty composer (`y` was dropped in S1e-1c, user decision 2026-10-29), Esc dismisses and shows again, a typed Enter never accepts; a run starts with no prompt, its scope root is the chat origin, a refused origin gives no root, a worker read outside the scope follows the approval mode as a chat read. Spec: agent-orchestrator / Shown once never blocks, Not shown again, Not accepted is shown again, No prompt at run start, Refused origin gives no root, Outside read follows the mode as a chat read; modal-input / Notice keys, Typed Enter does not accept; chat-pane / Worker starts without a prompt; config-menu / Accept persists, Dismiss does not accept. Done in `app_tests/orchestra_notice.rs` (S1e-1b, #403; Enter only after S1e-1c, #404) and `runtime/chat/tools_tests.rs` (scope root is the chat origin, a refused origin gives no root).
- [x] 1e1.6 GREEN `app/orchestra.rs` notice state, key handling, config write effect; `runtime/chat.rs` scope roots from the chat origin; `WorkerRead` routed by the approval mode. REFACTOR + gates (S1e-1b closes here). Done (S1e-1b, #403): notice state and keys in `app/orchestra.rs`, config write effect, scope roots from the chat origin, `WorkerRead` answered by the approval mode.
- [ ] The provider notice is accepted only with `y` after an arm delay (as approvals), or only with an empty composer; Enter typed as part of a message must not accept it (key audit, 2026-10-29). Picked: only with an empty composer (modal-input, Notice keys). Then (user decision, 2026-10-29) Enter only: `y` is typed; test `y_is_typed_and_does_not_accept_the_notice` (S1e-1c).
- [x] Rename `[ai.orchestrator] notice_accepted` to `acknowledged`, as `[ai.memory]` does (config freeze, 2026-10-29).

### S1e-1c (PR 11, ~300 lines, base: S1e-1b branch): tasks 1e1.7-1e1.8

- [x] 1e1.7 RED failure state tests: `OrchestraEvent::Halted` stores raw error; r retry, c/Esc/Ctrl+C cancel drive `RunManager::decide`/`cancel`; alert persists until answered; `Entry::Worker` rows follow status and stay out of history. Spec: agent-orchestrator / Row follows the status, Not in history, Error shown as it came; chat-pane / Alert persists; modal-input / Retry key, Cancel keys, Ctrl+C.
- [x] 1e1.8 GREEN `core/ai/chat/mod.rs` (`Entry::Worker`), `app/orchestra.rs` event handling. REFACTOR + gates. Done: `runtime.rs` hands the run manager's events to the app (`AppEvent::Orchestra`; `OrchestraEvent`/`WorkerRow` live in `core/ai/orchestra/event.rs`); `Halted` stores the raw error as `OrchAlert` until answered or the run ends; `r` retries and `c`/Esc cancel through `Effect::OrchDecide` -> `Chats::decide_run` -> `RunManager::decide`; Ctrl+C keeps the chat cancel and also ends a run that outlived its turn; a run is active from its turn until its `Ended`. `Entry::Worker` is one row per (run, worker), updated in place, a plain `LineKind::Tool` line in the transcript (yank, COPY), never a message, never stored. Tests `app_tests/orchestra_run.rs`, `chat/worker_row_tests.rs`, `the_answer_to_a_failure_reaches_the_run_through_chats`.
- [x] The failure alert keys (`r`, `c`, and in S2 `m`, `p`) act only with an empty composer or after an arm delay, so typed text cannot pick an option (key audit, 2026-10-29). Picked: only with an empty composer, as the notice; Esc is not a letter and cancels with text in the composer too. Test `letters_typed_into_a_message_never_answer_the_alert`.

- [x] A run ends by itself once its orchestrator turn ended and no worker is queued or running; a halted run waits for its answer (user decision, 2026-10-29). Done: `Chats` tells `RunManager::turn_ended` when an orchestrator turn is over; the run ends at once or when its last worker finishes, with one `Ended`. Tests `a_run_ends_by_itself_once_its_turn_and_workers_are_done`, `a_run_whose_worker_still_runs_ends_when_it_finishes`, `a_halted_run_stays_until_answered_then_ends_by_itself`, `the_end_of_an_orchestrator_turn_ends_its_idle_run`.

- [x] The orchestrator session keeps offering the user's MCP server tools, each call asking `y` as in a chat; workers never get them (user decision, 2026-10-29). Done: `TurnScope::offers` lets a user server tool through for a chat or the orchestrator turn, never a worker. Tests `an_orchestrator_server_tool_asks_and_runs_only_after_y`, `an_orchestrator_turn_lists_the_user_tool_and_a_call_waits_for_y`; the worker case of `an_out_of_set_call_is_refused_in_execute_unasked` stays.

#### Obligations from the S1b-2 security review (S1e-1b)

- [ ] Scope roots come only from the chat origin (no scope prompt, user decision 2026-10-29), never from model/worker text; a dropped root's reason is sanitized (`clean`/`clip`) before display.
- [ ] (suggestion) Consider refusing or flagging roots that are ancestors of secret roots or credential parents (~/.config, ~/.local, ~/Library, ~/.mozilla), per `jail::too_broad`.

## S1e-2: UI, snapshots, prompts (PR 12, ~600 lines re-estimated, `size:exception: approved`)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~600 (was ~250), excluding `.snap` files. Production ~260: ORCH badge beside `mode_badge` (`ui/components/chat.rs:155-165`) ~30, new `ui/components/worker.rs` ~80, notice/alert views ~150. Tests ~250 (TestBackend + insta, five groups). Final `orchestrate.md` wording for two profiles ~100. CHANGELOG ~3. Snapshot files are excluded but add review load |
| 400-line budget risk | Medium (~600, over 400) |
| Chained PRs recommended | Yes (already the S1 chain) |
| Decision needed before apply | No (decided, user 2026-10-21: NOT split; one PR with `size:exception: approved`) |

Base: S1e-1c branch. Tasks 1e2.1-1e2.4. Label the PR `size:exception`. Closes S1.

- [x] 1e2.1 RED insta snapshots (TestBackend): ORCH badge follows gate AND flag; worker row per status; notice view; failure alert content (raw error, r/c keys); approval label shown. Spec: chat-pane / Badge follows both switches, Row rendered, Alert content, Label shown.
- [x] 1e2.2 GREEN `ui/components/chat.rs` (badge beside `mode_badge`, notice, alert views), `ui/components/worker.rs`.
- [x] 1e2.3 RED+GREEN prompt test then final `prompts/*/orchestrate.md` wording (built-ins exist per profile, docs deferred). Spec: prompt-profiles / Built-ins exist, Docs deferred.
- [x] 1e2.4 CHANGELOG Internal line (no README). REFACTOR + gates + manual smoke with fake runner. Done: `ChatView::orchestrating` draws ` ORCH ` left of the mode badge (`ai_purple`, `O` when narrow; `ui/mod.rs` passes `App::orchestrating`); worker rows keep their text, failed/timed_out/refused in the tool alert colour (`transcript.rs`, no `ui/components/worker.rs` needed); `App::orch_guidance` gives the notice (`Enter accept · Esc dismiss`) and the alert (`run paused: worker <kind> <engine>><model> <status>: <error> · r retry · c/Esc cancel`) to `chat_guidance`, after the `engram serve` offer, joined by ` | ` so nothing a key answers is hidden, and paging counts their rows; the `orchestrate` prompts allow the user's own server tools (each call asks, workers never get them). The worker read prompt keeps its S1c-2 label. Carried forward: elapsed time of a running row (the row has no start time) and the `n/m` label (S3). Manual smoke: the binary needs a live terminal and engines, so the TestBackend screens of `orchestra_toggle.rs`, `orchestra_notice.rs`, `orchestra_run.rs` and `tool_approval.rs` stand in for it.

## S2 to S10 (coarser; each slice RED tests first, then GREEN, one or more PRs on the chain)

### S2: failure alerts (2 PRs, ~540, risk High, decision before apply: Yes)
- [x] 2.1 Re-verify Gemini vs Antigravity CLI before starting (open risk). Verified 2026-10-30: the Gemini CLI README says free individual and Google AI Pro/Ultra access moved to Antigravity CLI (`agy`) on 2026-06-18; Gemini CLI stays for Code Assist Standard/Enterprise and API-key/Vertex use. Decision: keep the Gemini adapter only in S2; an `agy` adapter is a follow-up (its error text is undocumented and the consumer-plan ToS for wrapping it is unclear).
- [x] 2.2 (S2a) RED+GREEN `core/ai/orchestra/errors.rs` `classify` with golden fixtures per adapter, secret in message redacted; `core/ai/chat/claude.rs` keeps rate-limit text. Spec: chat-engines / Each adapter classifies, Secret in message; agent-orchestrator / 429 is its own class, Class table.
- [x] 2.3 (S2b) RED+GREEN detached picker, `RunOverrides`, relaunch, later workers use override, (note from the S1b re-review: today `Relaunch` re-resolves only the halted worker, and already-queued workers keep their spawn-time spec; the override must reach them too), override marker, dropped at run end, abandoned picker, unavailable override engine, never persisted. Spec: config-menu / Change model opens level 2 .. Esc creates no override; config-persistence / A failure picker choice is never persisted; agent-orchestrator / Change provider relaunches .. Picker abandoned; modal-input / Change keys. Delivered as three chained branches (s2b-1 decisions never lost, s2b-2 queued workers follow a relaunch, s2b-3 picker and overrides with the review fixes). Done: `core/ai/orchestra/overrides.rs` (`RunOverrides`, `FailurePicker` over a detached `AiMenu`), `Run::decide` re-resolves the queued workers of the task on `Relaunch`, `App::roster` applies the pane's overrides (kept in `ChatState` with the run id, dropped on `Ended`), `WorkerRow.overridden` and an `(override)` row marker, the alert names the override, keys `m`/`p`. Also `OrchestraEvent::Declined` and `Dropped` (an answer carries its `RunId`) (a refused retry brings the alert back with `retry failed: ...`) and the short-window claude rate limit classifies as `rate_limit`.

### S3: multiple workers (3 PRs, ~1,100, risk High, decision: Yes) depends on S1

- [x] (S1b-2 security review) `ReadScope` is per run; the spec says per worker. Revisit before S3 parallel workers. Done (S3a): `Orchestra::allows`/`allows_write` take the asking `WorkerId`; the run manager grants only while that worker is `Running` (queued, ended, cancelled and unknown workers get nothing). Test `a_scope_grant_belongs_to_the_running_worker_only`.
- [x] 3.1 (S3a: labelled FIFO `Approvals` queue, `MAX_QUEUED` 16, prompt count, re-arm on promotion, queued asks withdrawn with their call, tools-off drains the queue. S3b: `SessionKey` in `Waits`, in the ask (`ApprovalAsk::worker`) and in `Effect::ToolDecision`; per-worker approver; `Waits::end_worker`; `App::withdraw_worker`; `n/m` label. Deviation: `Tagged` and the stream events keep `(pane, turn)` because a worker's rows render in the orchestrator's pane and call ids are host-unique; the session rides on the ask) `SessionKey` generalization: `Waits`, `Tagged`, `Approvals` (labelled FIFO) in `runtime/tools/approver.rs`, `runtime/chat.rs`, `core/ai/chat/approval.rs`, `app.rs`, `app/tool.rs`. Spec: agent-tools / Cross-session call refused, FIFO order; lazarobox-mcp-server / Own socket per worker.
- [x] 3.2 (S3c: `RunLimits` in `Run`, `[ai.orchestrator]` keys, worker cap, wall clock timer in `RunManager`, `Expired` event, `Effect::OrchCancelWorker`, worker selection and the escalating Ctrl+C; the queue count `(n waiting)` and the retry-first queue order were done at S3a and S1) `max_parallel`, `max_workers`, `run_timeout_mins`; failure with siblings; cancel one/all (pin keys, design open item). Spec: agent-orchestrator / Parallel limit, Worker cap, Wall clock, Isolated sessions, Siblings finish, Cancel after failure, Queue of two workers, Cancel denies pending asks; modal-input / Cancel the selected worker; chat-pane / Queue count.

### S4: handoffs and ledger (2 PRs, ~750, risk High, decision: Yes) depends on S3
- [ ] 4.1 `core/ai/orchestra/ledger.rs`, `Finish` tool, continuation in `app/orchestra.rs`, `max_turns`, on-course verdict (define channel in S4 design). Spec: orchestrator-handoffs / Continuation from the ledger, Ledger bounded, Verdict stored, Malformed verdict, Failure forces blocked.
- [ ] 4.2 `core/ai/memory/port.rs` `topic_key`, `runtime/engram/worker.rs` upsert, handoff content, redaction, degraded mode, inert data, hint. Spec: engram-client / both; chat-memory / all three; orchestrator-handoffs / Handoff content .. Injected handoff; chat-pane / Hint once.

### S5: ODD flow (2 PRs, ~500, risk Low) depends on S4 (O12, D15); S7 is already merged by then (plan change, user 2026-10-29), so `implement` may stage writes and a change counts as written only once `collect` lists it applied
- [ ] 5.1 `Ledger.step` and `Step` order, `Size` with its reason, step handoff keys, a read-only goal refuses `spawn_agent{kind: code}`. Spec: agent-orchestrator / Read-only goal spawns no code worker, Implement is a draft before S7; orchestrator-handoffs / Step needs a handoff.
- [ ] 5.2 `FeatureDoc` (specs verbatim, tasks with `S#`, log with `L1` = goal), one handoff per run, a correction appends to the log and rewrites one `S#`; ODD playbooks `prompts/*/orchestrate*.md`. Spec: agent-orchestrator / A large goal needs its document, Specs keep the user's words; orchestrator-handoffs / A correction keeps the log; prompt-profiles / Playbook per step.

### S6: usage (1-2 PRs, ~450, risk Medium) depends on S1
- [x] 6.1 `StreamEvent::Usage` in claude/codex/gemini/Ollama decoders, per-worker and per-run rows. (done in one PR, branch feat/agent-orchestrator-s6) Spec: chat-engines / Claude result usage, No usage reported; agent-orchestrator / Usage summed.

### S7: write integration (2 PRs, ~500, risk Medium, decision: No) depends on coding-tools M2 (merged, #342) and S1 only (moved up, user 2026-10-29)

Decision (user, 2026-10-29): "El worker propone su lote". A `code` worker stages in its pane's batch (`StageKey::Pane`) and proposes it itself with `propose_batch`, waiting in its turn for the user's answer; `collect` lists `files changed` (path, size, diff hash, applied / not applied). The review time counts against the worker's deadline; on timeout the batch stays as an orphan the user can still review. Base: `feat/agent-orchestrator-s1e-2`. PR 1 (S7-1): the seams, no worker offered a write tool yet; PR 2 (S7-2): the wiring, prompts and docs.

- [x] 7.1 `Toolset::Code` with the coding-tools write tools, `ChangeLedger` over staged changes, "files changed" in collect; writes reach the disk only through the user's batch review. Spec: agent-orchestrator / Writes wait for the batch review, Files changed listed.
  - [x] 7.1a (PR S7-1) `core/ai/orchestra/changes.rs` (`Change`, `ChangeLedger`, `diff_hash`), the ledger per worker slot in `Run` and first in `collect_wrapped`; `ReadScope::allows_write`; `Orchestra::{allows_write, staged, applied}` and `WorkerScope.worker`; the host stages a worker's write only inside the scope (refused as a chat write is, never asked), reports it to the ledger and marks an applied batch; `BatchReview::labelled`, drawn first in the review header (a chat's batch is byte-identical). Spec: agent-orchestrator / A write outside the scope is refused, The approval mode never approves a worker's batch, Files changed listed.
  - [x] 7.1b (PR S7-2) `Toolset::Code` = read + write and `for_kind(Code)`; the run manager opens a `code` worker's session with the pane's stage key and answers the ledger calls; the `orchestrate` prompts say only an applied file was written. Spec: agent-orchestrator / Writes wait for the batch review, Files changed listed, Review time counts against the deadline, Read-only subset; Implement is a draft before S7.
  - Done: tests in `core/ai/orchestra/tests.rs` (ledger, hash, collect, write scope), `runtime/tools/host/orchestra_tests.rs` (in-scope staging, refusals, labelled batch, approval mode), `runtime/orchestra/tests.rs` (end to end over the worker socket: toolset, files changed, user review, timeout orphan), `ui/components/review.rs` (label), `prompts/tests.rs`. Carried forward: several workers staging in one pane's batch at once is S3 (`MAX_RUNNING` is 1); the review header of an orphan reviewed with Ctrl+G carries no worker label.

### S7b: receipt-driven review, RDD (3 PRs, ~900, risk High) depends on S7 and S4 (O12, D16; design final after P1-P4)
- [ ] 7b.1 `assess` and `Tier` in the pure core: path rules give the floor, the orchestrator only raises, a failed assessment is `High`. Spec: agent-orchestrator / A lockfile is high, The orchestrator cannot lower.
- [ ] 7b.2 `ConsentAsk` inline, decline goes on `not reviewed`, lens workers over the staged view (candidate wrapped), refuter batch, at most one correction, lens and refuter prompts. Spec: agent-orchestrator / Consent first, Declined goes on unreviewed, One correction at most; prompt-profiles / Lens prompts exist.
- [ ] 7b.3 Receipt handoff and its line in the batch review; a receipt never approves. Spec: agent-orchestrator / A receipt never approves; orchestrator-handoffs / Declined consent still leaves a receipt.

### S8: ODD test-first evidence (1 PR, ~400, risk Medium) EXTERNAL DEPENDENCY: coding-tools M3 `run_command` (not started) plus S7
- [ ] 8.1 `Evidence{command, exit, digest}` from `run_command`, failing then passing, or an explicit exception with its reason. Spec: agent-orchestrator / Green without red refused, Exception recorded.

### S9: run view and persistence (3 PRs, ~900, risk High, decision: Yes) depends on S4
- [ ] 9.1 `app/run_view.rs`, `ui/components/run_view.rs`, `runtime/chat_store` runs table, resume. Spec: agent-orchestrator / Restart shows an interrupted run.
- Note (S1d-2 review): run ids are wall-clock microseconds kept above the last one issued, which holds within one process only; once runs are stored, persist the highest id issued and start above it, so a clock set back across a restart cannot reuse an id.

### S10: docs and acceptance (1 PR, ~150, risk Low) depends on S1..S9 and S7b plus coding-tools M2 and M3
- [ ] 10.1 README (usage, `Orchestrator` gate and Ctrl+O, Orchestrate row, provider-terms note, Ollama "not recommended"), CHANGELOG. Spec: agent-orchestrator / README written once [manual].
- [ ] 10.2 Manual acceptance: build a small project from the chat; missing-dependency path automated. Spec: agent-orchestrator / Small task from the chat [manual], Missing dependency.

## Dependency graph

S1a -> S1b -> S1b-2 -> S1c-1 -> S1c-2 -> S1d-1 -> S1d-2 -> S1d-3 -> S1e-1a -> S1e-1b -> S1e-1c -> S1e-2 (12 PRs, sequential, no parallel work inside the chain; each PR's base is the previous branch; decided by the user 2026-10-21). Then S7 (S7-1 -> S7-2) right after S1e-2: it needs only S1 and coding-tools M2 (merged) (plan change, user 2026-10-29). Then S2, S3, S6 depend only on S1 (sequential in the chain, parallelizable in analysis). S4 after S3; S5 after S4; S9 after S4; S7b needs S7 and S4; S8 needs coding-tools M3 and S7; S10 last.

Merge to main (plan change, user 2026-10-29): the chain merges into `main` behind the `Orchestrator` gate (`[ai.orchestrator] enabled`, off by default) before S10, starting with S1 plus S7; README stays untouched until S10 (D14).

coding-tools Z.2 (2026-10-29): rows 5 and 7 of `design.md` and the S7, S8 and S10 dependencies above already name `coding-tools` (S7 needs M2, S8 needs M3) since ef8a52c; no "write-tools SDD" or "test-runner SDD" dependency is left in this change, so Z.2 can be ticked in `coding-tools/tasks.md`.
