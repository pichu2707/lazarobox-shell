# Tasks: Agent Orchestrator

Gates on every PR: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Strict TDD: each GREEN task follows its RED task; a REFACTOR step is folded into the last task of each PR. README untouched until S10 (D14). Spec pointer format: `capability / Scenario`. Paths are under `src/` unless stated.

## Review Workload Forecast (whole change)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~11,200 across 10 slices (S1 ~5,960 in 7 PRs as decided, up to 13 PRs if the proposed splits are accepted). S2+ (~5,240) is NOT re-estimated yet; expect the same ~2x drift |
| 400-line budget risk | High (S1a, S1b, S1b-2, S1c, S1d, S1e-1, S1e-2 all exceed or approach 400 as scoped; S2+ exceed 400) |
| Chained PRs recommended | Yes |
| Suggested split | S1a -> S1b -> S1b-2 -> S1c -> S1d -> S1e-1 -> S1e-2 -> S2 ... S10 (S3 3 PRs, S9 3 PRs); further S1 splits proposed below, pending user decision |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Yes (S1c, S1d, S1e-1, S1e-2 re-estimates exceed 500 lines; proposed splits are pending user decision)
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Decisions (user, 2026-10-21):
- S1e is split into S1e-1 (app/core wiring) and S1e-2 (UI badge, prompts, insta snapshots).
- S1b is split: S1b (1b.1-1b.4 plus the `core/ai/mod.rs` wiring, 999 changed lines, `size:exception: approved`) and S1b-2 (1b.5-1b.8). S1b-2 is based on the S1b branch; S1c is now based on S1b-2. S1 is therefore 7 chained PRs.
- Re-estimate S1c, S1d, S1e-1, S1e-2 from real data before more apply work. Done below; the splits it suggests are `proposed, pending user decision`.
- The earlier `size:exception: approved` for S1d was given against a ~450-line estimate. The re-estimate is ~1,200, so that approval must be re-confirmed (or replaced by the proposed split). Later slices S2+ (and S3/S4/S9) must be re-asked at their own apply time.

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
| S1b | Pure core part 1: ids, `Toolset`, `OrchCall`, `Run`, `Roster`, worker types; `core/ai/mod.rs` wiring (`size:exception: approved`) | PR 2 | S1a branch |
| S1b-2 | Pure core part 2: `ReadScope` (`scope.rs`), collect cap and wrapper, gates | PR 3 | S1b branch |
| S1c | Host/hub seams (Toolset, ReadScope, HubKey); proposed split S1c-1 / S1c-2 | PR 4 (+1) | S1b-2 branch |
| S1d | RunManager + worker turns + prompts per task; proposed split S1d-1 / S1d-2 / S1d-3 | PR 5 (+2) | S1c branch (S1c-2 if split) |
| S1e-1 | App/core wiring: Ctrl+O and `orch` flag, send path, notice/scope/failure state and keys, `Entry::Worker`; proposed split S1e-1a / S1e-1b / S1e-1c | PR 6 (+2) | S1d branch (last S1d split if split) |
| S1e-2 | UI: ORCH badge, worker row, notice/scope/failure views, insta snapshots, final prompts, CHANGELOG; proposed split S1e-2a / S1e-2b | PR 7 (+1) | S1e-1 branch (last S1e-1 split if split) |
| S2..S10 | See below | one chain PR each (more where noted) | previous PR branch |

### Re-estimate summary (2026-10-21)

| PR | Old estimate | New estimate | Risk | Proposed split (pending user decision) |
|----|--------------|--------------|------|----------------------------------------|
| S1a | ~380 | ~1,160 (real) | Done | none |
| S1b | ~420 (whole S1b) | 999 (real) | Done | none; `size:exception: approved` |
| S1b-2 | ~210 (half of S1b) | ~450 | Medium | none (under 500) |
| S1c | ~360 | ~650 | High | S1c-1 (1c.1-1c.2, 1c.5-1c.6, ~340) + S1c-2 (1c.3-1c.4, ~330) |
| S1d | ~450 | ~1,200 | High | S1d-1 prompts (~230) + S1d-2 RunManager spawn path (~520) + S1d-3 control path and Chats wiring (~480) |
| S1e-1 | ~250 | ~900 | High | S1e-1a toggle and send (~330) + S1e-1b notice/scope (~300) + S1e-1c failure and `Entry::Worker` (~300) |
| S1e-2 | ~250 | ~600 | Medium | S1e-2a badge, worker row, approval label (~250) + S1e-2b notice/scope/alert views, prompts, CHANGELOG (~350) |

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

## S1b: pure core, part 1 (PR 2, 999 lines real, `size:exception: approved`)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~420 originally for all of S1b; real 999 for 1b.1-1b.4 plus the module wiring (run.rs 367, tests.rs 325, tools.rs 198, worker.rs 69, mod.rs 39) |
| 400-line budget risk | High (exceeded) |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No (decided: split S1b, remaining tasks move to S1b-2; `size:exception: approved`, user 2026-10-21) |

Label the PR `size:exception`. Commits 3b1653e, 7d9899d. The `core/ai/mod.rs` wiring (old 1b.8, second half) shipped here. Base: S1a branch.

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

- [ ] 1b.5 RED `core/ai/orchestra/tests.rs`: `ReadScope`: root passes `paths::resolve`, `$HOME` and broad scratch roots dropped (scratch decided by the injected predicate), both textual target and resolved path must be inside, `..` and symlink escape, deny-list wins, scope expires with run, denied scope. Spec: agent-orchestrator / Read inside the scope runs without asking, Deny-list wins inside the scope, Requested and resolved path both checked, Excluded root dropped, Scope expires with the run, Denied scope; agent-tools / Excluded under scope.
- [ ] 1b.6 GREEN `core/ai/orchestra/scope.rs` reusing `core/ai/tools/paths.rs` (`resolve`, `SecretRoots`), with the injectable scratch predicate; production default uses the existing scratch list.
- [ ] 1b.7 RED+GREEN `collect` cap 32 KiB and wrapper via `label::wrap(nonce, ...)` (nonce is a parameter; no runtime dependency), control sequences removed, wrapper unforgeable. Spec: agent-orchestrator / Collect caps text, Wrapper cannot be forged, Control sequences removed; agent-tools / Wrapped.
- [ ] 1b.8 REFACTOR + gates (`cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`). The `core/ai/mod.rs` wiring is already done in S1b; only re-export `ReadScope` here if needed.

## S1c: host and hub seams (PR 4, ~650 lines re-estimated, risk High)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~650 (was ~360). `core/ai/tools/mod.rs` ~90 production + ~80 tests (the new `Call::Orchestra` variant widens every exhaustive `match` on `Call`: `name`, `argument`, `host.rs::execute`, `ask_for`); `runtime/tools/host.rs` ~120 production + ~200 tests (`TurnScope` struct literal at `runtime/chat.rs:693` and in tests, execute filter, scope check, ask label field on `ApprovalAsk`); `runtime/mcp/session.rs` ~70 production + ~100 tests (`Hub` is `HashMap<PaneId, Live>`, socket name at `:179`, `sweep` at `:284` parses only `s-<pid>-<digits>.sock`) |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Decision needed before apply | Yes: proposed split below, pending user decision |

Base: S1b-2 branch.

**Proposed split (proposed, pending user decision):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1c-1 | 1c.1, 1c.2, 1c.5, 1c.6 (`registry_for`, `Call::Orchestra`, generic `Hub<K: HubKey>`, sweep form) | ~340 | S1b-2 branch | Both are mechanical seams with no behavior change on the default path |
| S1c-2 | 1c.3, 1c.4 (`TurnScope` fields, execute filter, scope check, ask label) | ~330 | S1c-1 branch | The behavioral host change; uses `Call::Orchestra` and `ReadScope`; reviewed on its own |

If the split is rejected, S1c needs `size:exception` (user decision at apply time). S1d then bases on S1c-2.

- [ ] 1c.1 RED `core/ai/tools/mod.rs` tests: `registry_for(Toolset)`: Orchestrator lists spawn_agent/wait/collect/cancel (no `run_in_pane`), ReadOnly lists find_dir/list_dir/read_file only, default registry byte-identical. Spec: agent-tools / Role toolsets; agent-orchestrator / No run_in_pane, Read-only subset; lazarobox-mcp-server / Out-of-role call.
- [ ] 1c.2 GREEN `core/ai/tools/mod.rs`: `registry_for`, `Call::Orchestra(OrchCall)`.
- [ ] 1c.3 RED `runtime/tools/host.rs` tests: out-of-set call refused in `execute`; `Call::Orchestra` without handle = `UnknownTool`; read inside `ReadScope` skips the ask only when both paths inside; orchestrator never approves; label travels with the ask; closed session denies; wait bound held. Spec: agent-tools / Inside runs, outside asks; agent-orchestrator / Orchestrator cannot approve, Outside read asks with a label, Never silently decided; lazarobox-mcp-server / Bound held, Label travels, Closed session denies.
- [ ] 1c.4 GREEN `runtime/tools/host.rs`: `TurnScope{orchestra: Option<OrchHandle>, read_scope, toolset}`, execute filter, ask label.
- [ ] 1c.5 RED `runtime/mcp/session.rs` tests: `Hub<K: HubKey>` with `PaneId` and `WorkerKey{run,worker}`; socket `s-{pid}-w{run}-{n}.sock`; `sweep` knows the form. Spec: lazarobox-mcp-server / Own socket per worker (S3 behavior; S1 delivers the key form only).
- [ ] 1c.6 GREEN `runtime/mcp/session.rs`: generic Hub key, sweep. REFACTOR + gates.

## S1d: RunManager (PR 5, ~1,200 lines re-estimated, risk High)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,200 (was ~450). `runtime/orchestra.rs` RunManager ~400 production (start, call dispatch for the four tools, worker turn through `runner_with` + `Pipe`, jail and env scrub reuse, own event channel, timers, cancel, shutdown, decide/retry) + ~500 tests (fake `Runner`, paused tokio time, temp HOME, real Hub, 9 scenarios); `runtime/chat.rs` + `runtime.rs` wiring (`Chats::with_orchestra`, `cancel`/`shutdown` forward; `runtime.rs` is ~4,200 lines) ~100; prompts (`runtime/prompts.rs`, `core/ai/chat/prompts.rs` real arm replacing the `Task::Orchestrate` fallback at `:53`, 2 stub `.md`) ~30 production + ~60 tests; 1d.6 follow-up ~40 + ~40 tests. Calibration: S1b's `run.rs` alone was 367 lines for a pure state machine |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Decision needed before apply | Yes: the earlier `size:exception: approved` was against ~450 lines; re-confirm it for ~1,200 or take the proposed split (pending user decision) |

Base: S1c branch (S1c-2 if S1c is split).

**Proposed split (proposed, pending user decision):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1d-1 | 1d.1, 1d.2, 1d.6 (prompt per task, stub `orchestrate.md`, effective-engine "not recommended" mark) | ~230 | S1c branch | Independent of the RunManager; touches prompts and menu only |
| S1d-2 | 1d.3 and 1d.4, spawn path: `start`, `call` (spawn/wait/collect), worker turn, jail and scrub, Done/Failed/TimedOut/Refused, no tool leak, log hygiene | ~520 | S1d-1 branch | The core of the manager; testable with a fake `Runner` and no UI |
| S1d-3 | 1d.3 and 1d.4, control path: `decide`, `cancel`, `shutdown`, halt then retry, worker read reaching the approver; plus 1d.5 `Chats::with_orchestra` and `runtime.rs` wiring | ~480 | S1d-2 branch | Control and integration into `Chats`; reviewable once the spawn path is settled |

If the split is rejected, S1d needs `size:exception` re-confirmed at ~1,200 lines. S1e-1 then bases on the last S1d PR.

- [ ] 1d.1 RED `runtime/prompts.rs` tests: prompt per task (worker gets its kind's prompt), override still untrusted, built-in `orchestrate.md` per profile exists. Spec: prompt-profiles / Worker gets its kind's prompt, Override still untrusted, Built-ins exist, Docs deferred.
- [ ] 1d.2 GREEN `runtime/prompts.rs`, `core/ai/chat/prompts.rs`, `prompts/*/orchestrate.md` (stub text; final wording in S1e-2/S5).
- [ ] 1d.3 RED `runtime/orchestra/tests.rs` (fake `Runner`, `tokio::time::pause`, temp HOME, real Hub): spawn -> Done/Failed/TimedOut/Refused; worker same jail as chat; API keys survive scrub; cancel kills the process; halt then retry relaunches same spec; worker read outside scope reaches the approver; no tool leak to workers; log hygiene (no content in logs). Spec: chat-engines / Same jail as chat, API keys survive the scrub; agent-orchestrator / Unusable orchestrator engine, Retry relaunches the same spec, Ctrl+C cancels all, Log hygiene.
- [ ] 1d.4 GREEN `runtime/orchestra.rs`: `RunManager::{start, call, decide, cancel, shutdown}`; worker turn via `runner_with` + `Pipe`; events on their own channel `(PaneId, OrchestraEvent)`.
- [ ] 1d.5 GREEN `runtime/chat.rs` (`with_orchestra`, `cancel`/`shutdown` forward), `runtime.rs` wiring. REFACTOR + gates.
- [ ] 1d.6 Follow-up from S1a review: the "not recommended" mark checks only an explicit `orchestrate` assignment. When the slot is unassigned and falls back to a server engine in `general`, mark it too (use the effective engine, `resolve(Task::Orchestrate)`, in `task_label` and the engine list). RED test first.

## S1e-1: app/core wiring (PR 6, ~900 lines re-estimated, risk High)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~900 (was ~250). Production ~450: `orch` flag and toggle with debounce ~80, send path at `app.rs:925` (`resolve(Task::Chat)`) plus Roster build from `availability_for` per task ~80, notice and scope state and keys ~150, failure state, keys and event handling ~100, config write effect ~40; `Entry::Worker` widens the exhaustive matches in `core/ai/chat/mod.rs` (`:542`, `:971`), `transcript.rs`, `tool.rs` and the `ui` list ~40. Tests ~450: four new `app_tests/orchestra_*.rs` files at ~110 each, state asserts only. `app.rs` is ~7,500 lines, so new logic goes in `app/orchestra.rs` |
| 400-line budget risk | High |
| Chained PRs recommended | Yes |
| Decision needed before apply | Yes: proposed split below, pending user decision |

Base: S1d branch (last S1d PR if split). State and key handling only; no rendering (snapshots land in S1e-2).

**Proposed split (proposed, pending user decision):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1e-1a | 1e1.1-1e1.4 (Ctrl+O toggle, `orch` flag, send path, `app/orchestra.rs` skeleton) | ~330 | last S1d PR | Smallest end-to-end behavior: an orchestrating pane sends `Task::Orchestrate` |
| S1e-1b | 1e1.5-1e1.6 (notice and scope state, keys, config write effect) | ~300 | S1e-1a branch | One concern: first-run notice and scope prompts |
| S1e-1c | 1e1.7-1e1.8 (failure state, retry/cancel keys, `Entry::Worker`, event handling) | ~300 | S1e-1b branch | Failure handling plus the `Entry` enum widening |

If the split is rejected, S1e-1 needs `size:exception` (user decision at apply time).

- [ ] 1e1.1 RED `app_tests/orchestra_toggle.rs` (state asserts, no insta): Ctrl+O toggles `orch`, `o` debounced (held key toggles once), per-pane independence, no-op with gate off, blocked during a run, existing keys unchanged, gate off-then-on, Enabled alone does not orchestrate, toggle applies to next message. Spec: modal-input / Ctrl+O toggles, Held key toggles once, Existing keys unchanged; chat-pane / Toggle is per pane, Toggle does nothing with the gate off, Toggle blocked during a run; agent-orchestrator / Enabled alone does not orchestrate, Toggle applies to the next message, Panes are independent, Gate off then on.
- [ ] 1e1.2 GREEN `app/chat.rs`, `core/ai/chat/mod.rs` (`orch` flag, in memory). Verify Ctrl+O is not swallowed by terminal/multiplexer (design open item).
- [ ] 1e1.3 RED `app_tests/orchestra_send.rs`: orchestrating pane sends `Task::Orchestrate` with Roster (menu), others `Task::Chat`; Orchestrate engine runs the turn; unusable engine shown. Spec: agent-orchestrator / Orchestrate engine runs the turn, Unusable orchestrator engine.
- [ ] 1e1.4 GREEN `app.rs` send path (:925), `app/orchestra.rs` (new).
- [ ] 1e1.5 RED notice and scope state/key tests: notice shown once and never blocks, Enter/y accepts and persists, Esc dismisses and shows again; scope prompt blocks reads until answered, keys, denied scope. Spec: agent-orchestrator / Shown once never blocks, Not shown again, Not accepted is shown again; modal-input / Notice keys, Scope keys; chat-pane / Prompt blocks reads until answered; config-menu / Accept persists, Dismiss does not accept.
- [ ] 1e1.6 GREEN `app/orchestra.rs` notice/scope state, key handling, config write effect.
- [ ] 1e1.7 RED failure state tests: `OrchestraEvent::Halted` stores raw error; r retry, c/Esc/Ctrl+C cancel drive `RunManager::decide`/`cancel`; alert persists until answered; `Entry::Worker` rows follow status and stay out of history. Spec: agent-orchestrator / Row follows the status, Not in history, Error shown as it came; chat-pane / Alert persists; modal-input / Retry key, Cancel keys, Ctrl+C.
- [ ] 1e1.8 GREEN `core/ai/chat/mod.rs` (`Entry::Worker`), `app/orchestra.rs` event handling. REFACTOR + gates.

## S1e-2: UI, snapshots, prompts (PR 7, ~600 lines re-estimated, risk Medium)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~600 (was ~250), excluding `.snap` files. Production ~260: ORCH badge beside `mode_badge` (`ui/components/chat.rs:155-165`) ~30, new `ui/components/worker.rs` ~80, notice/scope/alert views ~150. Tests ~250 (TestBackend + insta, five groups). Final `orchestrate.md` wording for two profiles ~100. CHANGELOG ~3. Snapshot files are excluded but add review load |
| 400-line budget risk | Medium |
| Chained PRs recommended | Yes |
| Decision needed before apply | Yes: proposed split below, pending user decision |

Base: S1e-1 branch (last S1e-1 PR if split).

**Proposed split (proposed, pending user decision):**

| PR | Tasks | Est. | Base | Why |
|----|-------|------|------|-----|
| S1e-2a | 1e2.1 and 1e2.2 for the badge, worker row and approval label | ~250 | last S1e-1 PR | Always-visible chrome; small snapshot set |
| S1e-2b | 1e2.1 and 1e2.2 for the notice, scope and failure alert views; 1e2.3, 1e2.4 (final prompts, CHANGELOG, smoke) | ~350 | S1e-2a branch | Modal views plus prose; closes S1 |

If the split is rejected, S1e-2 fits one PR with a note that it is ~600 lines (`size:exception` per user decision at apply time).

- [ ] 1e2.1 RED insta snapshots (TestBackend): ORCH badge follows gate AND flag; worker row per status; notice and scope prompt views; failure alert content (raw error, r/c keys); approval label shown. Spec: chat-pane / Badge follows both switches, Row rendered, Alert content, Label shown.
- [ ] 1e2.2 GREEN `ui/components/chat.rs` (badge beside `mode_badge`, notice, scope, alert views), `ui/components/worker.rs`.
- [ ] 1e2.3 RED+GREEN prompt test then final `prompts/*/orchestrate.md` wording (built-ins exist per profile, docs deferred). Spec: prompt-profiles / Built-ins exist, Docs deferred.
- [ ] 1e2.4 CHANGELOG Internal line (no README). REFACTOR + gates + manual smoke with fake runner.

## S2 to S10 (coarser; each slice RED tests first, then GREEN, one or more PRs on the chain)

### S2: failure alerts (2 PRs, ~540, risk High, decision before apply: Yes)
- [ ] 2.1 Re-verify Gemini vs Antigravity CLI before starting (open risk).
- [ ] 2.2 RED+GREEN `core/ai/orchestra/errors.rs` `classify` with golden fixtures per adapter, secret in message redacted; `core/ai/chat/claude.rs` keeps rate-limit text. Spec: chat-engines / Each adapter classifies, Secret in message; agent-orchestrator / 429 is its own class, Class table.
- [ ] 2.3 RED+GREEN detached picker, `RunOverrides`, relaunch, later workers use override, override marker, dropped at run end, abandoned picker, unavailable override engine, never persisted. Spec: config-menu / Change model opens level 2 .. Esc creates no override; config-persistence / A failure picker choice is never persisted; agent-orchestrator / Change provider relaunches .. Picker abandoned; modal-input / Change keys.

### S3: multiple workers (3 PRs, ~1,100, risk High, decision: Yes) depends on S1
- [ ] 3.1 `SessionKey` generalization: `Waits`, `Tagged`, `Approvals` (labelled FIFO) in `runtime/tools/approver.rs`, `runtime/chat.rs`, `core/ai/chat/approval.rs`, `app.rs`, `app/tool.rs`. Spec: agent-tools / Cross-session call refused, FIFO order; lazarobox-mcp-server / Own socket per worker.
- [ ] 3.2 `max_parallel`, `max_workers`, `run_timeout_mins`; failure with siblings; cancel one/all (pin keys, design open item). Spec: agent-orchestrator / Parallel limit, Worker cap, Wall clock, Isolated sessions, Siblings finish, Cancel after failure, Queue of two workers, Cancel denies pending asks; modal-input / Cancel the selected worker; chat-pane / Queue count.

### S4: handoffs and ledger (2 PRs, ~750, risk High, decision: Yes) depends on S3
- [ ] 4.1 `core/ai/orchestra/ledger.rs`, `Finish` tool, continuation in `app/orchestra.rs`, `max_turns`, on-course verdict (define channel in S4 design). Spec: orchestrator-handoffs / Continuation from the ledger, Ledger bounded, Verdict stored, Malformed verdict, Failure forces blocked.
- [ ] 4.2 `core/ai/memory/port.rs` `topic_key`, `runtime/engram/worker.rs` upsert, handoff content, redaction, degraded mode, inert data, hint. Spec: engram-client / both; chat-memory / all three; orchestrator-handoffs / Handoff content .. Injected handoff; chat-pane / Hint once.

### S5: SDD/TDD phases, read-only (1 PR, ~350, risk Low) depends on S4
- [ ] 5.1 Playbooks `prompts/*/orchestrate*.md`, `Ledger.phase`, dependency order, apply as draft, phase handoff keys. Spec: prompt-profiles / Playbook per phase; agent-orchestrator / Dependency enforced, Apply is a draft; orchestrator-handoffs / Phase needs a handoff.

### S6: usage (1-2 PRs, ~450, risk Medium) depends on S1
- [ ] 6.1 `StreamEvent::Usage` in claude/codex/gemini/Ollama decoders, per-worker and per-run rows. Spec: chat-engines / Claude result usage, No usage reported; agent-orchestrator / Usage summed.

### S7: write integration (2 PRs, ~500, risk Medium, decision: No) EXTERNAL DEPENDENCY: write-tools SDD (must be applied first)
- [ ] 7.1 `Toolset::Code`, `ChangeLedger`, "files changed" in collect, every write asks. Spec: agent-orchestrator / Write always asks, Files changed listed.

### S8: TDD red-green (1 PR, ~400, risk Medium) EXTERNAL DEPENDENCY: test-runner SDD (does not exist yet) plus S7
- [ ] 8.1 `Evidence{command, exit, digest}`, orchestrator requires failing then passing evidence. Spec: agent-orchestrator / Green without red refused.

### S9: run view and persistence (3 PRs, ~900, risk High, decision: Yes) depends on S4
- [ ] 9.1 `app/run_view.rs`, `ui/components/run_view.rs`, `runtime/chat_store` runs table, resume. Spec: agent-orchestrator / Restart shows an interrupted run.

### S10: docs and acceptance (1 PR, ~150, risk Low) depends on S1..S9 plus write-tools SDD and test-runner SDD
- [ ] 10.1 README (usage, `Orchestrator` gate and Ctrl+O, Orchestrate row, provider-terms note, Ollama "not recommended"), CHANGELOG. Spec: agent-orchestrator / README written once [manual].
- [ ] 10.2 Manual acceptance: build a small project from the chat; missing-dependency path automated. Spec: agent-orchestrator / Small task from the chat [manual], Missing dependency.

## Dependency graph

S1a -> S1b -> S1b-2 -> S1c -> S1d -> S1e-1 -> S1e-2 (sequential, no parallel work inside the chain; each PR's base is the previous branch). Proposed splits (pending user decision) keep the chain linear: S1c-1 -> S1c-2, S1d-1 -> S1d-2 -> S1d-3, S1e-1a -> S1e-1b -> S1e-1c, S1e-2a -> S1e-2b. Then S2, S3, S6 depend only on S1 (sequential in the chain, parallelizable in analysis). S4 after S3; S5 after S4; S9 after S4; S7 needs write-tools SDD; S8 needs test-runner SDD and S7; S10 last.
