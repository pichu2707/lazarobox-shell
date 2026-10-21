# Tasks: Agent Orchestrator

Gates on every PR: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Strict TDD: each GREEN task follows its RED task; a REFACTOR step is folded into the last task of each PR. README untouched until S10 (D14). Spec pointer format: `capability / Scenario`. Paths are under `src/` unless stated.

## Review Workload Forecast (whole change)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~7,300 across 10 slices (S1 ~2,060 in 6 PRs) |
| 400-line budget risk | High (S1d carries `size:exception`; S2+ exceed 400) |
| Chained PRs recommended | Yes |
| Suggested split | S1a -> S1b -> S1c -> S1d -> S1e-1 -> S1e-2 -> S2 ... S10 (S3 3 PRs, S9 3 PRs) |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Decision (user, 2026-10-21): S1e is split into S1e-1 (app/core wiring) and S1e-2 (UI badge, prompts, insta snapshots). S1d stays one PR with `size:exception: approved`. S1 is 6 chained PRs. Later slices S2+ (and S3/S4/S9) must be re-asked at their own apply time.

### Suggested Work Units (bases)

| Unit | Goal | PR | Base |
|------|------|----|------|
| S1a | Orchestrate row, gate keys, `availability_for(task)` | PR 1 | `feat/agent-orchestrator` |
| S1b | Pure core `core/ai/orchestra/` | PR 2 | S1a branch |
| S1c | Host/hub seams (Toolset, ReadScope, HubKey) | PR 3 | S1b branch |
| S1d | RunManager + worker turns + prompts per task | PR 4 | S1c branch |
| S1e-1 | App/core wiring: Ctrl+O and `orch` flag, send path, notice/scope/failure state and keys, `Entry::Worker` | PR 5 | S1d branch |
| S1e-2 | UI: ORCH badge, worker row, notice/scope/failure views, insta snapshots, final prompts, CHANGELOG | PR 6 | S1e-1 branch |
| S2..S10 | See below | one chain PR each (more where noted) | previous PR branch |

## S1a: task row and keys (PR 1, ~380 lines, risk Medium)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~380 |
| 400-line budget risk | Medium |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

- [x] 1a.1 RED `core/ai/config.rs` tests: slot 5 `Orchestrate` parses; absent = unassigned; `[ai.orchestrator]` defaults (enabled=false, notice_accepted=false, worker_timeout_secs=600); bad value falls back; older-build keys ignored. Spec: configuration / Slot parsed, Absent means unassigned, Defaults, Bad value falls back, Older build ignores keys; ai-task-models / Task set integrity.
- [x] 1a.2 GREEN `core/ai/config.rs`: append `Task::Orchestrate` at index 5 (slot indices hold), add `OrchestratorConfig`, parse and clamp (30..3600).
- [x] 1a.3 RED `core/config_edit.rs` + `runtime/config_write.rs` tests: save slot writes `[ai.tasks.orchestrate]`; clearing removes keys; `notice_accepted` write; no failure-picker persistence hook. Spec: config-persistence / Save the slot, Clearing removes the keys, Notice acceptance saved.
- [x] 1a.4 GREEN `core/config_edit.rs`, `runtime/config_write.rs`: own the new keys and the effect that writes `notice_accepted`.
- [x] 1a.5 RED `core/menu.rs` + `core/ai/menu.rs` tests (TestBackend + insta): six task rows plus `Orchestrator` gate row toggling on/off; picker first entry; only orchestrate changes; popup size stable; Set all excludes Orchestrate; Ollama marked "not recommended" on that row only; Orchestrate falls back to General. Spec: config-menu / Task rows rendered, Orchestrator row is the global gate, Picker first entry, Only orchestrate changes, Popup size is stable, Fallback threshold follows the popup, Marks on rows; ai-task-models / Orchestrate falls back, Ollama marked not recommended, Other tasks unmarked, Set all leaves the orchestrator alone.
- [x] 1a.6 GREEN `core/menu.rs`, `core/ai/menu.rs`: rows, gate toggle, picker, marks, Set all exclusion.
- [x] 1a.7 RED `core/ai/chat/availability_tests.rs`: availability per task, no fallback engine, server model must be detected, empty server model, CLI model not validated, effective assignment, worker kind uses its own row, Summarize row used. Spec: chat-engines / Availability per task, No fallback engine; ai-task-models / Effective assignment used, Fallback to general, Server model must be detected, Empty server model, CLI model not validated, Assignment never changed, Worker kind uses its own row, Summarize row is now used.
- [x] 1a.8 GREEN `core/ai/chat/engine.rs`: `availability_for(task, ...)` replaces the hard-wired `Task::Chat` (:281); keep a Chat wrapper so call sites compile.
- [x] 1a.9 Off-path characterization: snapshot proves chat with gate off is unchanged vs 0.18.0 (first written, passes before changes). Spec: agent-orchestrator / Disabled offers no tools.
- [x] 1a.10 REFACTOR + gates.

## S1b: pure core (PR 2, ~420 lines, risk Medium)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~420 |
| 400-line budget risk | Medium |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

- [ ] 1b.1 RED `core/ai/orchestra/tests.rs`: `OrchCall` parse (spawn kinds general|code|chat|review|summarize; Orchestrate refused; wait clamped to 120 s; collect/cancel). Spec: agent-orchestrator / Workers cannot spawn, Bad kind refused, Wait is clamped.
- [ ] 1b.2 GREEN `core/ai/orchestra/{mod,tools}.rs`: `RunId`, `WorkerId`, `Toolset`, `OrchCall`, tool schemas, parse.
- [ ] 1b.3 RED `tests.rs`: `Run` transitions with explicit `Instant`s: spawn refused when Not Ready (`Refused{reason}`, no fallback), max 1 running (extras queue), deadline -> TimedOut, collect before done, cancel after done, failure -> Halted, halted blocks spawn / wait returns `awaiting_user`, retry reuses same spec, cancel. Spec: agent-orchestrator / Review worker runs on the Review assignment, Unassigned kind falls back as the menu says, Menu change after spawn, No jail refuses, Model missing refuses, Deadline kills the worker, Collect before done, Error shown as it came, No silent fallback, Orchestrator paused, Cancel after done.
- [ ] 1b.4 GREEN `core/ai/orchestra/{worker,run}.rs`: `WorkerSpec`, `WorkerStatus`, `WorkerResult`, `RunState`, `UserChoice`, `Roster`, `Run`.
- [ ] 1b.5 RED `tests.rs`: `ReadScope`: root passes `paths::resolve`, `$HOME` and broad scratch roots dropped, both textual target and resolved path must be inside, `..` and symlink escape, deny-list wins, scope expires with run, denied scope. Spec: agent-orchestrator / Read inside the scope runs without asking, Deny-list wins inside the scope, Requested and resolved path both checked, Excluded root dropped, Scope expires with the run, Denied scope; agent-tools / Excluded under scope.
- [ ] 1b.6 GREEN `core/ai/orchestra/scope.rs` reusing `core/ai/tools/paths.rs`.
- [ ] 1b.7 RED+GREEN `collect` cap 32 KiB and wrapper via `label::wrap(nonce, ...)`, control sequences removed, wrapper unforgeable. Spec: agent-orchestrator / Collect caps text, Wrapper cannot be forged, Control sequences removed; agent-tools / Wrapped.
- [ ] 1b.8 Gates; module wired in `core/ai/mod.rs` only.

## S1c: host and hub seams (PR 3, ~360 lines, risk Low)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~360 |
| 400-line budget risk | Low |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

- [ ] 1c.1 RED `core/ai/tools/mod.rs` tests: `registry_for(Toolset)`: Orchestrator lists spawn_agent/wait/collect/cancel (no `run_in_pane`), ReadOnly lists find_dir/list_dir/read_file only, default registry byte-identical. Spec: agent-tools / Role toolsets; agent-orchestrator / No run_in_pane, Read-only subset; lazarobox-mcp-server / Out-of-role call.
- [ ] 1c.2 GREEN `core/ai/tools/mod.rs`: `registry_for`, `Call::Orchestra(OrchCall)`.
- [ ] 1c.3 RED `runtime/tools/host.rs` tests: out-of-set call refused in `execute`; `Call::Orchestra` without handle = `UnknownTool`; read inside `ReadScope` skips the ask only when both paths inside; orchestrator never approves; label travels with the ask; closed session denies; wait bound held. Spec: agent-tools / Inside runs, outside asks; agent-orchestrator / Orchestrator cannot approve, Outside read asks with a label, Never silently decided; lazarobox-mcp-server / Bound held, Label travels, Closed session denies.
- [ ] 1c.4 GREEN `runtime/tools/host.rs`: `TurnScope{orchestra: Option<OrchHandle>, read_scope, toolset}`, execute filter, ask label.
- [ ] 1c.5 RED `runtime/mcp/session.rs` tests: `Hub<K: HubKey>` with `PaneId` and `WorkerKey{run,worker}`; socket `s-{pid}-w{run}-{n}.sock`; `sweep` knows the form. Spec: lazarobox-mcp-server / Own socket per worker (S3 behavior; S1 delivers the key form only).
- [ ] 1c.6 GREEN `runtime/mcp/session.rs`: generic Hub key, sweep. REFACTOR + gates.

## S1d: RunManager (PR 4, ~450 lines, risk High, size:exception: approved)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~450 |
| 400-line budget risk | High |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

size:exception: approved (user, 2026-10-21). Label the PR `size:exception`.

- [ ] 1d.1 RED `runtime/prompts.rs` tests: prompt per task (worker gets its kind's prompt), override still untrusted, built-in `orchestrate.md` per profile exists. Spec: prompt-profiles / Worker gets its kind's prompt, Override still untrusted, Built-ins exist, Docs deferred.
- [ ] 1d.2 GREEN `runtime/prompts.rs`, `core/ai/chat/prompts.rs`, `prompts/*/orchestrate.md` (stub text; final wording in S1e-2/S5).
- [ ] 1d.3 RED `runtime/orchestra/tests.rs` (fake `Runner`, `tokio::time::pause`, temp HOME, real Hub): spawn -> Done/Failed/TimedOut/Refused; worker same jail as chat; API keys survive scrub; cancel kills the process; halt then retry relaunches same spec; worker read outside scope reaches the approver; no tool leak to workers; log hygiene (no content in logs). Spec: chat-engines / Same jail as chat, API keys survive the scrub; agent-orchestrator / Unusable orchestrator engine, Retry relaunches the same spec, Ctrl+C cancels all, Log hygiene.
- [ ] 1d.4 GREEN `runtime/orchestra.rs`: `RunManager::{start, call, decide, cancel, shutdown}`; worker turn via `runner_with` + `Pipe`; events on their own channel `(PaneId, OrchestraEvent)`.
- [ ] 1d.5 GREEN `runtime/chat.rs` (`with_orchestra`, `cancel`/`shutdown` forward), `runtime.rs` wiring. REFACTOR + gates.

## S1e-1: app/core wiring (PR 5, ~250 lines, risk Low)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~250 |
| 400-line budget risk | Low |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

Base: S1d branch. State and key handling only; no rendering (snapshots land in S1e-2).

- [ ] 1e1.1 RED `app_tests/orchestra_toggle.rs` (state asserts, no insta): Ctrl+O toggles `orch`, `o` debounced (held key toggles once), per-pane independence, no-op with gate off, blocked during a run, existing keys unchanged, gate off-then-on, Enabled alone does not orchestrate, toggle applies to next message. Spec: modal-input / Ctrl+O toggles, Held key toggles once, Existing keys unchanged; chat-pane / Toggle is per pane, Toggle does nothing with the gate off, Toggle blocked during a run; agent-orchestrator / Enabled alone does not orchestrate, Toggle applies to the next message, Panes are independent, Gate off then on.
- [ ] 1e1.2 GREEN `app/chat.rs`, `core/ai/chat/mod.rs` (`orch` flag, in memory). Verify Ctrl+O is not swallowed by terminal/multiplexer (design open item).
- [ ] 1e1.3 RED `app_tests/orchestra_send.rs`: orchestrating pane sends `Task::Orchestrate` with Roster (menu), others `Task::Chat`; Orchestrate engine runs the turn; unusable engine shown. Spec: agent-orchestrator / Orchestrate engine runs the turn, Unusable orchestrator engine.
- [ ] 1e1.4 GREEN `app.rs` send path (:925), `app/orchestra.rs` (new).
- [ ] 1e1.5 RED notice and scope state/key tests: notice shown once and never blocks, Enter/y accepts and persists, Esc dismisses and shows again; scope prompt blocks reads until answered, keys, denied scope. Spec: agent-orchestrator / Shown once never blocks, Not shown again, Not accepted is shown again; modal-input / Notice keys, Scope keys; chat-pane / Prompt blocks reads until answered; config-menu / Accept persists, Dismiss does not accept.
- [ ] 1e1.6 GREEN `app/orchestra.rs` notice/scope state, key handling, config write effect.
- [ ] 1e1.7 RED failure state tests: `OrchestraEvent::Halted` stores raw error; r retry, c/Esc/Ctrl+C cancel drive `RunManager::decide`/`cancel`; alert persists until answered; `Entry::Worker` rows follow status and stay out of history. Spec: agent-orchestrator / Row follows the status, Not in history, Error shown as it came; chat-pane / Alert persists; modal-input / Retry key, Cancel keys, Ctrl+C.
- [ ] 1e1.8 GREEN `core/ai/chat/mod.rs` (`Entry::Worker`), `app/orchestra.rs` event handling. REFACTOR + gates.

## S1e-2: UI, snapshots, prompts (PR 6, ~250 lines, risk Low)

| Field | Value |
|-------|-------|
| Estimated changed lines | ~250 |
| 400-line budget risk | Low |
| Chained PRs recommended | Yes (already the S1 split) |
| Decision needed before apply | No |

Base: S1e-1 branch.

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

S1a -> S1b -> S1c -> S1d -> S1e-1 -> S1e-2 (sequential, no parallel work inside the chain; each PR's base is the previous branch). Then S2, S3, S6 depend only on S1 (sequential in the chain, parallelizable in analysis). S4 after S3; S5 after S4; S9 after S4; S7 needs write-tools SDD; S8 needs test-runner SDD and S7; S10 last.
