# Design: Orchestrator Worker Flow

Evidence marks: **[R]** read in code for this design; **[I]** inferred, not executed; **[X]** upstream knowledge, not checked on a live server. Decisions 1-15 and assumptions A-I of `proposal.md` are binding. Rows marked **OWNER** needed the owner's confirmation; every one is now confirmed (2026-11-10) or was settled by milestones 1-2 (see the end).

## Technical Approach

Guarantees move from prompts into run state, in the existing split: pure rules in `src/core/ai/orchestra/` and `src/core/ai/chat/approval.rs`, I/O in `src/runtime/`, keys and notices in `src/app/`. Facts decide (ledger, step state, toggles, line counts); model text is data and never decides. Worker asks reuse the labelled ask queue. Review reuses RDD.

## Corrections to exploration and proposal

| Finding | Evidence | Consequence |
|---|---|---|
| The real silent degrade is the worker's, not the chat's. `open_session` returns `None` on no tooling, `Tools::Off`, unknown locality or a failed hub, and the worker runs as text with no notice | [R] `runtime/orchestra.rs:1576-1581,1623-1626` | W1 discloses; W2 refuses a writer with no tools. This is one candidate for the trigger; no cause is established |
| `MCP_VERIFIED` is claude, codex, gemini, ollama. A worker on llamacpp, lmstudio, vllm or opencode gets `Tools::Off`, so it can never write | [R] `chat/engine.rs:126`, `orchestra.rs:1510` | The llama.cpp acceptance can only end in the notice unless llamacpp is verified. Settled: the manual check M6 passed on 2026-11-07 and `llamacpp` joined `MCP_VERIFIED` (`src/core/ai/chat/engine.rs:134`; change `2026-11-07-llamacpp-tools`, archived) |
| `runtime/chat.rs:900-911` is not reachable by an ORCH turn in production: tooling is always set, and `orch_refusal` stops `Tools::Off` first. A plain chat with tools off passes there by design | [R] `runtime.rs:658`, `app/orchestra.rs:850-866`; the send path calling `orch_refusal` is [I] | No notice there; a `debug_assert` for ORCH only |
| Two clocks kill a worker: `worker_timeout_secs` (`Turn::run`) and `TOTAL_LIMIT`, 15 min over all rounds, which ignores waits. A 3600 s deadline is already capped at 900 s | [R] `orchestra.rs:1834`, `chat/ollama.rs:176`, `chat/cli.rs:1722`, `loopback.rs:175`, `wait_line.rs:29` | Decision 13 must cover both |
| CLI engines cap one MCP call at 900 s | [R] `tools/wiring.rs:16,152,236` | Known limit, see D5 |
| A worker's ask lives under the coordinator's turn. When that turn ends the ask is denied, and a denied batch stays `Proposed`; `propose` then refuses, so the worker cannot re-propose | [R] `chat.rs:535`, `approver.rs:209`, `files.rs:754`, `stage.rs:300`; the runtime sequence is [I] | Fixed in W10a: a re-propose of the own proposed batch asks again |
| Review workers count against `max_workers` and need handoffs, consuming coordinator turns. A mandatory review of 1 author uses up to 4 more; two authors exceed the default 8 | [R] `run.rs:365,767-776,827`, `orchestra/review.rs:943-946` | W7 makes them internal |
| Confirmed: the `Exec` ask has no worker, so its answer goes to the pane session and finds no wait, and with another prompt open it is refused instead of queued | [R] `approval.rs:260-277,1065-1068`, `tool.rs:62-70`, `approver.rs:105-116`; the outcome is [I] | W5 |
| Confirmed: staging and sealing are per pane, so one proposal seals a sibling's files | [R] `orchestra.rs:1597`, `files.rs:167-172` | W10a |
| Adding a task also touches `slots: [Assignment; 6]`, `ui/components/menu.rs:682,1009` and `spawnable()`, which returns `SET_ALL` | [R] `config.rs:407`, `tools.rs:147` | W8a |
| Assumption E does not match this repository's own test files: `foo_tests.rs` and `tests.rs` | [R] Glob of `src/core/ai/orchestra/` | Confirmed by the owner on 2026-11-10: `*_tests.*` and `tests.*` also count as tests (proposal assumption E) |

## Key Decisions

| # | Topic | Choice | Rejected, and why |
|---|---|---|---|
| D1 | Brief form | Flat optional `spawn_agent` arguments for `simple`: `goal`, `inputs`, `outputs`, `done_when`, `out_of_scope` (`ArgKind::Str`, `StrList`). `prompt` becomes optional in the table and stays required for other kinds in `spawn()`; `simple` refuses it. Validated in `Run::spawn`, not in the parse, because parse errors carry no field name [R `tools.rs:182-200`, `OrchError` is content-free]; the refusal names the field | Parsing headings from `prompt`: model text would decide. Nested object: `ArgKind` has no object kind [R `registry.rs:48-70`] and small models fill flat arguments more reliably [I] |
| D2 | Extra fields | A declared optional `extra` list (`"name: value"`) is the schema's legal place, because the schema says `additionalProperties: false` and the parse refuses unknown keys at two layers [R `registry.rs:134,170`, `tools.rs:368`]. `parse_built_in` also moves undeclared keys of a `spawn_agent` with `kind: simple` into `extra`; other kinds stay strict. The worker gets them as data; the spawn result and a transcript line say `brief extra fields: x, y` | Refusing unknown keys: decision 2 allows them. Dropping: silent. `additionalProperties: true` on the tool: weakens the other kinds. An engine that enforces the schema itself refuses an undeclared key before lazarobox sees it [I]; `extra` covers that case |
| D3 | Clarification | Worker-only tool `needs_specification { what }` (`Family::Clarify`, in the `simple` toolset). Recording it refuses further writes; the next text ends the worker | Parsing "needs specification:" text: false positives when the deliverable holds the phrase, and the app would trust text. A worker that only writes the phrase is a normal result, flagged by W1 |
| D4 | Rounds | While a clarification of worker N is open, the next `simple` spawn must carry `answers: N`, or it is refused naming N. `round = parent + 1`. A third call stops the run with `Stopped` and a notice. Split opens no clarification | Goal or file similarity: a heuristic. Optional `answers`: omission escapes the cap |
| D5 | Stop the clock | One `Pending` per worker route, entered in `Rerouted::ask`, read by a new `pausing_timeout`. It covers `Turn::run` and, for worker turns only (task-local), the three `total` sites. A queued answer counts, because the wait registers at `ask`, before the app queues it [R]. Commands and model time count. CLI's 900 s per call stays: if the call ends, the batch is an orphan; a visible row is W5's test. The run wall clock (`run_timeout_mins`) keeps counting while the user decides: confirmed by the owner on 2026-11-10 | Extending the deadline after each answer: `timeout` cannot be extended and the race is real. The turn's `Pending`: shared, one worker's ask would pause its siblings. Changing `total` for chat: out of scope |
| D6 | Review wait | The proposer's clock also pauses during RDD. Lenses have their own deadlines and the wall clock still runs. Confirmed by the owner on 2026-11-10; it goes beyond decision 13, and the spec delta now says model review time does not count | Counting it: mandatory review on a 600 s deadline would time out authors |
| D7 | One batch per writer | `StageKey::Worker(PaneId, WorkerKey)`. `StageKey::pane()` serves badges, apply and close. A correction worker and lenses use the proposer's key (`review::State` maps id to proposer) | A sub-batch per worker inside the pane batch: every lifecycle rule would need a second key. Serializing writers: contradicts decision 12 |
| D8 | Command claim | `RunGate` claims by `PaneId`: one command per pane stays, and a sibling is refused at once with `CHAT_BUSY` | Per-worker claim: two builds in one tree race on locks and outputs |
| D9 | Several reviews | The existing FIFO: labelled batches queue (`MAX_QUEUED` 16), and promotion opens the review [R `tool.rs:471-492`]. Arrival order; the label names the worker | Grouped review: new UI, and the batches are independent |
| D10 | Same file, two workers | At spawn, a `simple` whose `outputs` overlap a live worker's `outputs` is refused, naming the worker. At apply, `Drifted` protects: the second batch writes nothing and the worker is told [R `apply.rs:107-110,231`] | Merging: not safe unattended. Refusing at stage: `code` has no brief, and drift already holds |
| D11 | Parallelism | `max_parallel` stays the user's ceiling. One notice when two workers share a local engine: it may queue them; set `max_parallel = 1`. Settled by the owner on 2026-11-06 (milestone 2, decision 5) and shipped in PR12 (`Run::shares_engine` `src/core/ai/orchestra/run.rs:392`, `RunManager::tell_shared` `src/runtime/orchestra.rs:1617`) | Reading `/slots` or `/api/ps`: engine-specific, and a machine's load is not in them. An engine semaphore: lazarobox would decide capacity |
| D12 | Local servers | [X] Ollama queues beyond `OLLAMA_NUM_PARALLEL`, up to `OLLAMA_MAX_QUEUE`, then 503; another model may force a swap. llama-server waits for a free `--parallel` slot and splits its context across slots. A queued request can pass `FIRST_BYTE_LIMIT` (120 s [R]) and fail "no answer": a visible halt with retry. Manual acceptance must check this | - |
| D13 | Line count | Pure `orchestra/limit.rs`: added rows of `tools::diff(base, text)`, per file. Sums over a worker's applied batches (`Slot.written`). Language table by extension: `//` and `/* */`; `#`; `--`; `"""` and `'''` when a line starts with them. Blank and whole-line comment lines do not count; a trailing comment counts as code. Unknown extension counts all and says so. Test by path (E). Limit 400 | File length: breaks B. A parser per language: an error here only produces an ask |
| D14 | When it asks | At `propose_batch`, before RDD and sealing, with one ask listing files over the limit and files outside `outputs` (C). `y` keeps; `n` or Esc splits. `Limit` is never answered by a mode: `answers_itself` and `route` fall through to the prompt [R `approval.rs:749,826-836`], pinned by a test on all three. Confirmed by the owner on 2026-11-10: ask at `propose_batch`; split discards the staged batch, with the count shown | At the staging call: the edit needs a dry run in the async layer, and the prompt interrupts mid-batch. Cost: a large file is generated before the ask. An orphan is never proposed, so the review row shows the count instead |
| D15 | Write from chat | `/write`, same shape as `/github`: per chat, in memory, `y` dialog `ToolGrant { toggle, folder }`, ended by folder change, tools off, or the toggle. Only a user command opens it: `on_tool_ask` denies the variant like `GithubGrant` [R `tool.rs:147`]. Applies to the next spawn through `Effect::OrchCeiling`; running workers keep their spec, like `set_roster` [R `run.rs:259`]. Approved by the owner on 2026-11-06 and shipped in milestone 1 (`src/app/write.rs`) | Saving the toggle: a chat command would change global config. Menu only: decision 7 asks for the chat |
| D16 | Exec from chat | `/exec` on the same path, in W6; every command still asks in every mode. Confirmed by the owner on 2026-11-10 ("Sí, /exec como /write (Recommended)"; told the menu Exec toggle is only a ceiling for workers: "mantendo, creo que estas rutas por teclado ayudan bastante a agilizar el trabajo") | Menu only: safer against a socially engineered "type /exec", but inconsistent. The prompt and the jail remain the gate either way |
| D17 | Menu ceiling | `Toolset::capped(menu) -> FamilySet` removes Write and Exec when the menu lacks them, and nothing else, so it can never widen. `WorkerSpec.families` is set in `resolve`. `Run.ceiling` is set at `start`, updated at `attach` and by `OrchCeiling`. `orchestra.rs:1511,1582` read `spec.families`. Coordinator and review are unaffected | A plain intersection: drops `Clarify`, which has no toggle. Reading the toggles at launch: the core would not know at spawn, and the refusal must happen there |
| D18 | Write off | `Refusal::WriteOff`: `code` always; `simple` only with non-empty `outputs` (decision 14). The text names the toggle and `/write`, is returned to the coordinator and is shown in the pane | Refusing every `simple`: breaks decision 14 |
| D19 | Worker asks | `Exec` gains `worker: Option<(WorkerKey, String)>`, filled by `exec::fill`; `is_worker()` and `worker()` cover it; the prompt target carries the label. So it queues, is answered in its session and is withdrawn with the worker | A `WorkerExec` variant: duplicates the prompt, `exec_notice` and pin rendering for a field. `stage_key` must be `Some` when Exec is on [R `exec.rs:141`] |
| D20 | Cheap | `cheap(engine, model) = locality(engine) == Local && !forwards_off_machine(engine, model)` [R]. Ollama `*-cloud` is not cheap | Locality alone: a cloud model would be called local |
| D21 | Mandatory review | In `before_batch_with`, a cheap author skips `consent` and goes to `review` with `Consent::Mandatory`, whatever the tier; the receipt says mandatory. Otherwise the high-tier consent stays [R `host/review.rs:73-87`]. A review that cannot start says so and the batch goes on, as today | Blocking the batch: the user's review remains the gate, and a blocked author loses work |
| D22 | Review notices | At review start: same `engine>model` as the author warns. A local Review row states its limits. A cloud row says the diff leaves the machine. Review workers become internal (`Slot.internal`): outside `max_workers` and `without_handoff`, still in the worker rows. Internal workers outside `max_workers`: confirmed by the owner on 2026-11-10 | Keeping them public: see Corrections |
| D23 | Gates | Spawn: a write-capable worker needs `steps.check(Step::Implement)`. Finish: `grant == Implement` with no write-capable worker refuses `finish`. Both give `Refusal::Gate(step)` with a visible line. Internal workers skip them | Soft warn-once like `finish_warned`: then the gate does nothing. No trap: a text reply settles when nothing is pending, and `max_turns` bounds the rest [R `run.rs:823-836`] |
| D24 | Gate override | A guidance line: `Enter gates off` when a gate refused in this run; it applies to the live run or the next one and says so. Never from a model. One line at a time, so Enter has one meaning: provider notice, failure alert, gate override, then undelegated. A pending override hides the undelegated line | A letter key: see D27 |
| D25 | Task comparisons | `spec.families.contains(Write)` at `run.rs:394,656,747`, via `WorkerSpec::writes()`. `run.rs:362` moves after `resolve`. A `simple` with no outputs is allowed on a read-only goal. `READ_ONLY` text is reworded and its test updated | `task == Code \|\| task == Simple`: wrong for a `simple` summary, and for a Write-off spec |
| D26 | Stored runs | No schema change. `run_workers.kind` and `status` are TEXT and `WorkerRecord` holds Strings; nothing parses them into `Task` [R `chat_store/runs.rs:89-93,225-234`, `record.rs:53-61`]. `simple` shows in old and new builds, never dropped. Brief and rounds stay in memory: a resume is a new run that relaunches nothing [R `run_resume.rs`], so they restart by a user's act. Pinning test only. Older builds ignoring `[ai.tasks.simple]` is [I] | Storing brief and rounds: a migration for data a resume never reuses |
| D27 | Undelegated notice | `Run::undelegated()`: no slots, not cancelled. Sent from `turn_ended` on `Settle` and from `finish_run`. The app parks it while the phase is not idle (the `park_continue` pattern, since the turn's end and this event use separate channels [R]), and drops it on a failed turn or a resume waiting for a reply. Guidance, key first: `Enter delegate · Esc dismiss \| no worker ran, nothing written`; shorter forms by width, as `github_hint` does. Enter on an empty composer, fresh press only, sends a fixed text of lazarobox's own and keeps the user's goal (the `resume_goal` pattern) | A letter on an empty composer: fires when the user starts typing "dame...". The precedent is `NOTICE` ("`y` is typed", owner decision 2026-10-29). Ctrl keys: `c y b r g o j u` are taken in `app/chat.rs` [R] |
| D28 | Handoff check | In `Handoff::of`, path-like tokens of `done` not in the worker's ledger give `claims not in ledger: x`, and a write-capable worker with `on_course` and nothing applied is flagged. Shown in the result and at close. Flag only; no schema change | A `files` argument: a small model omits it and nothing is flagged. Cost: a mention of an unchanged file is flagged, in neutral words |
| D29 | Worker prompt | A lazarobox-owned `prompts/worker.md`, appended in `launch` for write-capable workers only, stating it overrides the fenced-block style. Chat turns are untouched, and user overrides of `code.md` still apply. `simple` gets its own role prompt | Editing `code.md`: shared with chat |

## Data Flow

    coordinator -- spawn_agent(brief) --> Run::spawn: gate, ceiling, brief, overlap
       |                                     | refused: named text + pane line
       |                                     v
       |                             worker [StageKey::Worker]
       |                                     | write_file, edit_file (scope F)
       |                                     v
       |                              propose_batch
       |                                     v
       |     Limit ask (D14) --> mandatory RDD (D21) --> seal --> Batch(labelled)
       |     clock paused ===========================================>|
       |                                                 y: apply, drift check
       |                                                              v
       |                                            run_command: Exec(labelled)
       |<-- collect: ledger, flags, closing (clarify or split), wrapped

`what` from `needs_specification` is cleaned, redacted, cut to one line and reaches the coordinator inside the nonce wrapper, like every worker text.

## Interfaces

```rust
pub enum StageKey { Pane(PaneId), Worker(PaneId, WorkerKey) }
pub struct Brief {
    goal: String, inputs: Vec<String>, outputs: Vec<String>,
    done_when: String, out_of_scope: String,
    extra: Vec<(String, String)>, answers: Option<WorkerId>,
}
pub enum BriefError { Missing(&'static str), BadPath(String), NotAnswered(WorkerId) }
pub enum Closing { Clarify(String), Split(Vec<String>) }
// WorkerSpec gains families: FamilySet, brief: Option<Brief>
// ApprovalAsk gains Limit { worker, label, over, outside } and ToolGrant { toggle, folder }
```

## File Changes

| Slice | Files | Lines (production, read-based) |
|---|---|---|
| W1a facts disclosed | `orchestra/{run,changes}.rs`, `runtime/orchestra.rs`, `orchestra/event.rs`, `app/orchestra.rs` | 150-230 |
| W1b undelegated notice, key | `run.rs`, `runtime/orchestra.rs`, `app/{orchestra,chat}.rs`, `chat/guidance.rs` | 120-200 |
| W2a prompt, ceiling, refusals | `orchestra/{mod,run,worker}.rs`, `chat/engine.rs` (OrchSend), `app/orchestra.rs`, `prompts/worker.md`, `prompts.rs` | 170-280 |
| W2b `/write` | `chat/folder.rs` (Command), `app/chat.rs`, a grant module beside `app/github.rs`, `approval.rs`, `app/run.rs`, `runtime.rs` | 180-280 |
| W10a batch per worker | `tools/stage.rs`, `runtime/tools/{stage,host/gate,host/exec,host/orphan,host/files}.rs`, `runtime/{chat,orchestra}.rs` | 200-320 |
| W10b review on the proposer's key | `runtime/orchestra/review.rs`, `orchestra.rs`, `chat/review.rs`, local notice | 90-160 |
| W3 gates, override | `run.rs`, `steps.rs`, `runtime/orchestra.rs`, `app/orchestra.rs`, `prompts/*/orchestrate_odd.md` | 180-300 |
| W4 handoff check | `ledger.rs`, `run.rs`, `runtime/orchestra.rs` | 110-190 |
| W5 clock, labelled ask | `tools/pending.rs`, `orchestra.rs`, `chat/{ollama,cli}.rs`, `loopback.rs`, `approval.rs`, `host/exec.rs`, `app/run.rs` | 170-290 |
| W6 `run_command`, `/exec` | `mod.rs`, `orchestra.rs`, prompts and `prompts/tests.rs:141`, tool tests | 110-200 |
| W7 mandatory review | `rdd.rs`, `receipt.rs`, `run.rs`, `host/review.rs`, `orchestra/review.rs`, `tools/mod.rs` trait | 180-300 |
| W8a `simple` task | `config.rs`, `menu.rs`, `ui/components/menu.rs`, `picker.rs`, `config_edit.rs`, `prompts.rs`, `tools.rs` | 130-220 |
| W8b brief | new `orchestra/brief.rs`, `tools.rs`, `tools/mod.rs`, `run.rs`, `orchestra.rs`, prompts | 180-290 |
| W8c clarification | `registry.rs`, `tools/mod.rs`, `host.rs`, `run.rs`, `orchestra.rs` | 170-280 |
| W9a counter | new `orchestra/limit.rs` | 140-230 |
| W9b ask, split, outside | `approval.rs`, `host/review.rs`, `app/{run,tool}.rs`, `run.rs` | 180-290 |

**Total 2,460-4,060**, against 1,420-2,270 in the proposal. W10 alone, previously unexplored, is 290-480. Figures come from listing touch points, not from measurement; past deliveries ran 2.4x to 3x their estimates, so `sdd-tasks` must re-forecast, and a high end near 320 is a split candidate.

**Order:** W1a, W1b, W2a, W2b, W10a, W10b, W3, W4, W5, W6, W7, W8a, W8b, W8c, W9a, W9b. W10 moves up because W5, W6 and W7 all depend on the stage key; after them it would touch the same lines twice. W7 still precedes W8, as the proposal requires.

**Deviation from the proposal's risk table,** which says write-capable workers run one at a time per pane until W10 ships. This design adds no interim serialization: `code` workers already overlap today, only W1a, W1b, W2a and W2b ship before W10a, none of them adds a write-capable kind, and the guard would be removed four slices later. Closed by the owner on 2026-11-06 (owner decision 10: no interim guard).

## Testing Strategy

| Layer | What | How |
|---|---|---|
| Pure | Brief errors, D4 rounds, `capped` never widening, gates, `writes()`, counter per language and path, `cheap`, `Limit` in all three modes | `cargo test`, first, in-module or `*_tests.rs` |
| Runtime | `pausing_timeout` with a shown and a queued ask, per-worker isolation, two batches and drift, re-propose after a denied ask, Exec routed to its own session | Paused tokio clock, the existing fake runner |
| App | Enter key on an empty composer only, parked notice, forms at narrow widths, grant dialog denied from a tool call | `App::update`, insta snapshots |
| Stored | A record with kind `simple` in the interruption line and the resume message | `record_tests.rs` |
| Manual | **Unit tests cannot prove local tool calling.** Ollama and llama.cpp for W2, W6, W8; two parallel workers on one server for W10 | Owner, per slice |

## Migration / Rollout

No data migration. Each slice is one revert, gated by `[ai.orchestrator] enabled` and the pane toggle. Only the `simple` menu row and the `/write` and `/exec` commands are visible with ORCH off. `/write` also widens a plain chat's next turn, by the user's own `y`.

## Decisions only the owner can take

All ten are answered or settled. Items 2, 3, 4, 6, 7 (D16) and 8 were answered by the owner on 2026-11-10 (labels quoted as chosen); the others were settled in milestones 1-2.

1. Add llamacpp to `MCP_VERIFIED` after its manual check? **Settled** (2026-11-06 decision, check passed 2026-11-07): M6 passed on Qwen2.5-7B-Instruct, llama-server b11935, and `llamacpp` is in `MCP_VERIFIED` (`src/core/ai/chat/engine.rs:134`; change `2026-11-07-llamacpp-tools`, archived).
2. E: also treat `*_tests.*` and `tests.*` as tests? **Answered** 2026-11-10: "Sí, añadirlos (Recommended)". Both count as tests.
3. D6: pause the proposer's clock during model review as well as during user waits. **Answered** 2026-11-10: "Sí, pausarlo (Recommended)".
4. D5: the run wall clock (default 60 min) keeps counting while the user decides. Recommended: yes, it is the last bound. CLI's 900 s per call stays too. **Answered** 2026-11-10: "Sí, que siga contando (Recommended)".
5. D11: `max_parallel` as the only ceiling, with one notice. **Settled** 2026-11-06 (milestone 2, decision 5); shipped in PR12.
6. D14: ask at `propose_batch`, and split discards the batch. **Answered** 2026-11-10: "Sí, descartar el lote (Recommended)": split discards the staged batch, with the count shown.
7. D15, D16: `/write` per chat with a dialog; `/exec` on the same path. D15 **settled** 2026-11-06 (shipped in milestone 1). D16 **answered** 2026-11-10: "Sí, /exec como /write (Recommended)"; after the explanation that the menu Exec toggle is only a ceiling for workers: "mantendo, creo que estas rutas por teclado ayudan bastante a agilizar el trabajo".
8. D22: review workers outside `max_workers`, which raises possible cloud cost beyond the visible cap. **Answered** 2026-11-10: "Sí, fuera del límite (Recommended)".
9. D23: a hard finish gate rather than warn-once. **Settled** 2026-11-06 (milestone 2, decision 9); shipped in PR13 (`Refusal::Gate` `src/core/ai/orchestra/run.rs:567`, `Run::lift_gates` `run.rs:302`).
10. No interim one-writer guard before W10a, against the proposal's risk table. **Settled** 2026-11-06 (milestone 1): no guard; W10 shipped in PR11-PR12.

## Open Questions

- [ ] Whether each CLI engine enforces `additionalProperties` itself (D2, [I]).
- [ ] Live behaviour of D12 on a real Ollama server (M8, owed). On llama-server it was observed in M6 on 2026-11-07: a queued wait under 120 s completes, over 120 s halts with "no answer from the engine" and `r` retries.
- [ ] Whether W1's disclosure identifies the original trigger; if it does, later slices may shrink.
