# Design: Agent Orchestrator (one coordinator, many workers, tracked handoffs)

Inputs: `proposal.md` (O1-O8 and assumptions (a)-(e), all approved), `exploration.md` (approach C), engram `sdd/agent-orchestrator/proposal-questions`. Later binding decisions (2026-10-20): README written once, in the final docs slice; the end state is "build a programming project from the chat, without an IDE"; activation is a global gate plus a per-pane ORCH toggle; the picker opened from a failure alert is a run-scoped override, never persisted; delivery is `feature-branch-chain`.

## Technical Approach

A pure core, `src/core/ai/orchestra/`, holds the run, worker and ledger state machines, the tool schemas, the read scope and error classification. It has no IO and is driven by explicit `Instant`s. A runtime shell, `src/runtime/orchestra.rs`, owns a `RunManager`. The manager spawns worker turns with the existing `Runner` seam (`runtime/chat.rs:90`, `runner_with` :328). Each worker turn is wrapped in the same `Pipe` that chat turns use (:142), so sanitizing, the answer cap and error texts stay identical.

The orchestrator is a normal chat turn in an orchestrating pane (global `[ai.orchestrator] enabled` on AND the pane's ORCH mode on). It is resolved from a new `Task::Orchestrate` slot. Its MCP session offers four extra tools: `spawn_agent`, `wait`, `collect` and `cancel`. Those calls go through `ToolHost` to the `RunManager`, never to the file tools. Worker tool calls reuse the orchestrator turn's approver and event channel. That lets S1 ship without the `PaneId` → session-key refactor; S3 does the refactor.

The end state (decision 2): the orchestrator plans (S5), Code workers write (S7, write-tools SDD), a captured runner runs tests (S8, test-runner SDD), and Review workers review the change ledger (S7). This change builds the coordination, contracts and UX. The two other SDDs plug into fixed seams: `Toolset::Code` and `ChangeLedger` for writes, `Evidence` for tests.

## Architecture Decisions

| # | Topic | Options | Tradeoff | Decision |
|---|---|---|---|---|
| D1 | Where the run lives | (a) inside `Chats` · (b) a separate `RunManager`, held by `Chats` as `Option<Arc<RunManager>>` (`with_orchestra`, like `with_tools` :607) | (a) bloats an already large type. (b) keeps one owner for worker tasks, and `Chats::cancel`/`shutdown` (:791, :800) forward to it | **(b)** |
| D2 | Pane mode | global (every chat orchestrates) · per pane, toggled when `[ai.orchestrator] enabled` | Global would leave the Chat row meaningless | **Global gate plus per pane.** `enabled` (menu row `Orchestrator`, default off) is the gate. Each pane holds `orch: bool` in memory (default off, never saved). **Ctrl+O** toggles it: `on_chat_key` (`app/chat.rs:164`) binds Enter, Esc, Ctrl+C/Y/B/R/J/U, PageUp/Down, Alt+Enter, and Ctrl+O is free (`o` is added to the debounced set at :131). The ORCH badge is drawn beside `mode_badge` (`ui/components/chat.rs:155`) only when gate AND pane flag are on. Gate off means the key does nothing. Effective mode = gate AND flag, evaluated on every send. Only such a pane sends with `Task::Orchestrate`; every other pane keeps `Task::Chat`. The key is ignored while a run is active in the pane |
| D3 | Routing orchestration calls | new MCP server · new `Call::Orchestra(OrchCall)` through the existing `ToolHost` | A second server doubles the wiring for 3 CLIs (D12/D13 of agent-tools) | **Same server.** `TurnScope` gains `orchestra: Option<OrchHandle>`. Only an orchestrator turn sets it, and only then does `registry_for(Toolset::Orchestrator)` list the 4 tools. A call without a handle is `UnknownTool` |
| D4 | Worker sessions in S1 | fake `PaneId`s · full `SessionKey` now · a generic `Hub<K>` plus the orchestrator turn's approver | Fake ids collide with layout ids (`core/layout.rs:6`). The full refactor touches `app.rs` (~8k lines) | **Generic Hub key** (`impl HubKey for PaneId`, `WorkerKey{run,worker}`, socket `s-{pid}-w{run}-{n}.sock`, `sweep` taught the form, `session.rs:284`). `Waits` stays `PaneId`-keyed: worker asks register under the orchestrator's (pane, turn) with fresh `CallId`s (`host.rs:275`), so the existing overlay serves them. S3 generalizes `Waits`, `Tagged` and `Approvals` |
| D5 | Engine for spawn | lazarobox picks · menu at spawn | O1 | `cfg.resolve(task)` (`config.rs:359`, General fallback (a)). Availability goes through `availability_for(task, …)`, which replaces the hard-wired `Task::Chat` (`engine.rs:281`). The app sends a `Roster` (task → engine, model, `Availability`) with each orchestrator request, so the runtime never reads config. The app builds it from the menu configuration, then applies the pane's `RunOverrides` (D9) on top. Not Ready means `Refused{reason}`, never a fallback |
| D6 | Read scope | ask every cloud read · pre-approve inside a user-approved root set | O5 | `ReadScope{roots}`. Every root passes `paths::resolve` (`tools/paths.rs:328`: secret roots, `CREDENTIAL_DIRS`, outside `$HOME`). A root that is `$HOME` itself or a `jail::is_scratch` broad dir is refused. `host::execute` drops the `Read` ask (`host.rs:287-297`) only when BOTH the textual target (`display_target`) and the resolved real path sit under a root. Outside the scope, the normal ask (e) |
| D7 | Worker toolset | full registry · `Toolset` filter | O7, no recursion | `Toolset::{ReadOnly, Code, Orchestrator}`. Workers get `find_dir`, `list_dir` and `read_file`: no `run_in_pane`, no user servers, no orchestration tools. The filter is applied twice, to the offered list (`hub.open` tools, `chat.rs:716`) and in `execute` (a call outside the set is refused) |
| D8 | Long runs vs caps (MCP 900 s, turn 15 min, `loopback.rs:81-85`) | blocking delegate · async tools plus a ledger | approach C | `spawn_agent` returns at once. `wait` is clamped to at most 120 s. S1 runs are bounded by one orchestrator turn. S4 adds a `Ledger`: when the orchestrator turn ends while the run is Open, the app starts a continuation turn whose user message is the ledger render (worker summaries wrapped). Continuations are capped by `max_turns` |
| D9 | Failure | silent fallback · halt the run | O6, (c), (d) | A worker failure moves the run to `Halted{worker, class, text}`. No new spawn is allowed; siblings finish. While halted, `wait` returns `awaiting_user` and `spawn_agent` is refused with `run paused`. Retry relaunches the same `WorkerSpec` from scratch. Change model or change provider opens a standalone picker seeded from the failed worker's effective assignment (`AiMenu::open_picker(Target::Task(task))`, `core/ai/menu.rs:173`, used as a detached instance). Its confirm does NOT touch the menu draft or the config file: it returns `(engine, model)` to the app, which stores it in `RunOverrides` (`task → engine, model`, held by the pane's run state in the app) and sends `Relaunch{worker, roster}` with the override applied. Every later `Roster` of that run carries the override, so later workers of the task use it too. It is dropped when the run ends (Finished or Cancelled). Run state and worker rows show it (`engine>model` plus an override marker). Esc creates no override. A menu save during the run does not displace an active override |
| D10 | Error classification | per-decoder structured events · a pure text classifier over `ChatError` | Decoders already flatten provider errors into `Failed(text)` (`claude.rs:184`, `codex.rs:150`, `gemini.rs:109/138`). A new event variant touches every `match` | **Pure `classify(engine, &ChatError) -> ErrorClass`**, with golden fixtures per adapter. `Jail(_)` maps to JailRefusal. S2 lets `claude.rs` keep `rate_limit_event` (ignored today, :19) as text, so it can be classified |
| D11 | Worker output to the orchestrator | raw · wrapped | injection | `collect` wraps with `label::wrap(nonce, …)` (`tools/label.rs:19`, 32 KiB cap, a nonce per call). The worker text is never placed in an approval or a system prompt |
| D12 | Provider notice | per run · once | O2 | `[ai.orchestrator] notice_accepted`, shown inline in the chat pane the first time an orchestrating pane would start a run (keys: Enter or `y` accepts, Esc dismisses). It is modelled on the memory warning (`menu.rs:64-80` `warning`/`answer_warning`) but lives in the pane. Only accepting records `true` (config write effect); a dismissal leaves it `false`, so it shows again at the next run. It never gates use: the run starts without waiting |
| D13 | Handoffs | chat_store · engram with topic keys | O4 | `MemCmd::Save` gains `topic_key: Option<String>` (upsert). Keys: `orchestra/{run}/worker/{n}`, `orchestra/{run}/phase/{phase}`, `orchestra/{run}/ledger`. Saves only when `MemoryConfig::active()` (`config.rs:214`) and through the existing note redaction. Otherwise the run keeps them in memory and shows `handoffs not persisted` (b) |
| D14 | README | per slice · once | binding decision 1 | **No README edits until S10.** CHANGELOG `Internal` lines and openspec may change per slice. The feature stays default-off and undocumented until S10 |

## Interfaces / Contracts (core, `src/core/ai/orchestra/`)

```rust
pub struct RunId(pub u64); pub struct WorkerId(pub u32);
pub enum Toolset { ReadOnly, Code /* S7 */, Orchestrator }
pub struct WorkerSpec { pub task: Task, pub prompt: String, pub engine: String,
    pub model: Option<String>, pub toolset: Toolset, pub deadline: Duration }
pub enum WorkerStatus { Queued, Running, Done, Failed, Cancelled, TimedOut, Refused }
pub enum ErrorClass { RateLimit, Conflict, Auth, Quota, Timeout, JailRefusal, Other } // spec ids: rate_limit, conflict, auth, quota, timeout, jail_refusal, other
pub struct WorkerResult { pub status: WorkerStatus, pub text: String /* capped 32 KiB at collect, unwrapped */,
    pub elapsed: Duration, pub error: Option<(ErrorClass, String)> /* Refused carries the refusal reason with class Other */,
    pub usage: Option<Usage> /* S6 */, pub changes: Vec<Change> /* S7 */, pub evidence: Vec<Evidence> /* S8 */ }
pub enum RunState { Open, Halted { worker: WorkerId, class: ErrorClass, text: String }, Finished, Cancelled }
pub enum OrchCall { Spawn { task: Task, prompt: String }, Wait { ids: Vec<WorkerId>, timeout: u16 /* clamped to 120 s */ },
    Collect { id: WorkerId } /* text capped at 32 KiB */, Cancel { id: WorkerId }, Finish { summary: String } /* S4 */ }
pub struct Run { /* pure: spawn() -> Result<WorkerId, Refusal>, on_event, on_tick(now), decide(UserChoice) */ }
pub enum UserChoice { Retry, Relaunch(Roster) /* roster already carries the run override */, Cancel }
pub struct RunOverrides(BTreeMap<Task, (String /*engine*/, Option<String> /*model*/)>); // app side, per pane run, never persisted
```

`spawn_agent{kind: general|code|chat|review|summarize, prompt}` (the five worker kinds of the spec); `Orchestrate` is not spawnable. Cancelling the whole run is `RunManager::cancel(pane)` (Ctrl+C, key), not a tool argument. Limits: S1 has max 1 running (extra spawns queue); S3 adds `max_parallel` (default 2, range 1..4), `max_workers` (default 8, range 1..32) and `run_timeout_mins` (default 60, range 1..480). Worker deadline: `worker_timeout_secs` (default 600 = 10 min, range 30..3600). Tool bounds: `wait` at most 120 s, `collect` at most 32 KiB. The worker's system prompt is `load_prompt(profile, task)`: `runtime/prompts.rs:79` is parametrized by task, with a new built-in `orchestrate.md` per profile (`core/ai/chat/prompts.rs:38-50`).

Runtime: `RunManager::{start(pane, turn, OrchScope), call(OrchCall) -> Outcome, decide(pane, UserChoice), cancel(pane), shutdown}`. App events go on their own channel, `(PaneId, OrchestraEvent::{Worker(row), Halted, Ended})`, because a worker can outlive a turn (S4).

## Data Flow

```
user msg (orchestrating pane: gate on AND pane ORCH on; any other pane sends Task::Chat as before)
  ─ app: Task::Orchestrate, Roster (menu + RunOverrides), ReadScope? ─→ Chats::send
  └ scope prompt first (run start) → OrchScope{roots}
orchestrator CLI ─MCP→ Hub<PaneId> → ToolHost(Orchestrator) → RunManager.call
   spawn → Roster[task] Ready? → worker turn: Runner + Pipe + Hub<WorkerKey>
            worker tools → ToolHost(ReadOnly, ReadScope, orchestrator approver)
            out-of-scope read → ApprovalAsk labelled "worker 1/1 review claude>sonnet"
   wait ≤120 s / collect (wrapped, ≤32 KiB) / cancel
worker Error → classify → Run::Halted → OrchestraEvent::Halted → failure notice
   [r]etry | [m]odel/[p]rovider → detached picker(task) → RunOverrides (run-scoped, not saved) → Relaunch | [c]ancel / Esc
run ends (Finished | Cancelled) → RunOverrides dropped; menu and config untouched
S4: turn ends, run Open → ledger continuation turn; handoffs → engram (topic_key) or in-memory
```

## Product end state: critical path to "a project built from the chat"

| Step | Provides | Owner |
|---|---|---|
| 1 | S1 contract, one read-only worker | this SDD |
| 2 | S2 failure alerts (429 is first-class under real load) | this SDD |
| 3 | S3 parallel workers, approval queue, `SessionKey` | this SDD |
| 4 | S4 ledger continuation (survives the 15-min cap) and handoffs | this SDD |
| 5 | **write-tools SDD**: `create_dir`/`write_file`/`edit_file`, a diff-capable approval overlay, writes confined to a project root | separate SDD |
| 6 | S7 Code workers get `Toolset::Code`, a per-worker `ChangeLedger`, and review over the diffs | this SDD (needs 5) |
| 7 | **test-runner SDD**: a captured, non-pane `run_command` with a timeout, output caps, a jail/profile for toolchains (cargo/npm/pytest; dependency fetch needs the network) | separate SDD |
| 8 | S8 red-green: `Evidence{command, exit, digest}`, failing then passing, required by the orchestrator | this SDD (needs 7, 6) |
| 9 | S5 SDD/TDD playbooks become real (plan → write → test → review) | this SDD |
| 10 | S9 run view, persistence; S10 README | this SDD |

Steps 1-4 and the write-tools SDD can proceed in parallel. The test-runner SDD is the longest pole: no design exists for it yet (`command-executor` is an unrelated old exploration). Commits stay with the user (non-goal), through the pane.

## Delivery: feature-branch-chain (binding, 2026-10-20)

The tracker branch is `feat/agent-orchestrator`; only it merges to `main`, after S10. S1 ships as 12 chained PRs (user decision 2026-10-21): S1a, S1b, S1b-2, S1c-1, S1c-2, S1d-1, S1d-2, S1d-3, S1e-1a, S1e-1b, S1e-1c, S1e-2. S1b carries `size:exception: approved`; S1e-2 is one PR (~600) with `size:exception: approved`; S1d gets no size exception. Real sizes ran ~2.4x to 3x the estimates, hence the splits detailed in `tasks.md`. S1a targets the tracker branch and each later PR targets the branch of the previous one (S1b on S1a, S1b-2 on S1b, S1c-1 on S1b-2, S1c-2 on S1c-1, ...) so every review diff stays one slice. Each PR is a work unit with its own start, verification (`cargo test`, `clippy --all-targets`, `fmt --check`) and rollback boundary. Later slices (S2 to S10) follow the same chain, sized per the table below. README is not touched until S10.

## File Changes and Slicing (estimates include tests; budget ~400 per PR; delivery above)

| Slice | Files | Est. lines | PRs |
|---|---|---|---|
| **S1a** task row and keys | `core/ai/config.rs` (Orchestrate appended at index 5 so `slots` indices hold, `[ai.orchestrator]` enabled/notice_accepted/worker_timeout_secs), `core/config_edit.rs`, `runtime/config_write.rs`, `core/menu.rs` (`Orchestrator` gate row, toggle), `core/ai/menu.rs`, `core/ai/chat/engine.rs` (`availability_for(task)`), `availability_tests.rs` | ~1,160 real (est. ~380) | 1 |
| **S1b** pure core part 1 | `core/ai/orchestra/{mod,run,worker,tools,tests}.rs` (specs, `OrchCall` parse, `Run` state machine, `Roster`), `core/ai/mod.rs` wiring | real ~1,130 incl. review fixes 098fce2 (est. ~420 for all of S1b); `size:exception: approved` | 1 |
| **S1b-2** ReadScope and collect wrapper | `core/ai/orchestra/scope.rs` (`ReadScope` reusing `paths::resolve`, injectable scratch predicate), `run.rs` collect cap 32 KiB and `label::wrap` wrapper, tests | ~450 | 1 |
| **S1c** host and hub seams | `runtime/tools/host.rs` (`Toolset`, `read_scope`, `orchestra` in `TurnScope`), `core/ai/tools/mod.rs` (`registry_for`, `Call::Orchestra`), `runtime/mcp/session.rs` (`HubKey`, sweep), tests | ~650 (was ~360) | 2 (S1c-1 tools + Hub, S1c-2 host) |
| **S1d** RunManager | `runtime/orchestra.rs` (plus `orchestra/tests.rs`), `runtime/chat.rs` (`with_orchestra`, cancel/shutdown forward), `runtime/prompts.rs` (prompt per task), `runtime.rs` wiring | ~1,200 (was ~450) | 3 (S1d-1 prompts, S1d-2 spawn path, S1d-3 control path and wiring) |
| **S1e-1** app/core wiring | `app/orchestra.rs` (new), `app.rs` (send path :925 uses the Orchestrate slot only in orchestrating panes), `app/chat.rs` (Ctrl+O, `orch` flag per pane, `o` debounced), `core/ai/chat/mod.rs` (`Entry::Worker`, `orch` flag), notice/scope/failure state and keys | ~900 (was ~250) | 3 (S1e-1a toggle and send, S1e-1b notice/scope, S1e-1c failure and `Entry::Worker`) |
| **S1e-2** UI | `ui/components/chat.rs` (+ `worker.rs`, ORCH badge, provider notice, scope prompt, failure retry/cancel views), insta snapshots, `prompts/*/orchestrate.md`, CHANGELOG | ~600 (was ~250) | 1 (not split; `size:exception: approved`) |
| **S2** failure alerts | `core/ai/orchestra/errors.rs` (+ fixtures per adapter), `core/ai/chat/claude.rs` (rate-limit text), the alert change model/provider → detached picker → `RunOverrides` (no config write), override marker in run state and worker rows, `app/orchestra.rs`, UI | ~540 | 2 |
| **S3** parallel | `runtime/tools/approver.rs` (`Waits` by `SessionKey`), `runtime/chat.rs` (`Tagged`), `core/ai/chat/approval.rs` (`Approvals.pending` → labelled FIFO), `app.rs`/`app/tool.rs`, `[ai.orchestrator] max_parallel/max_workers/run_timeout_mins`, cancel one/all | ~1100 | 3 |
| **S4** ledger and handoffs | `core/ai/orchestra/ledger.rs`, `core/ai/memory/port.rs` (`topic_key`), `runtime/engram/worker.rs`, `Finish` tool, continuation in `app/orchestra.rs`, `max_turns`, on-course check | ~750 | 2 |
| **S5** phases | `prompts/*/orchestrate*.md` playbooks, `Ledger.phase`, phase handoff keys | ~350 | 1 |
| **S6** usage | `StreamEvent::Usage` in 3 decoders and Ollama, per-worker/per-run rows | ~450 | 1-2 |
| **S7** writes (needs write-tools) | `Toolset::Code`, `ChangeLedger`, collect "files changed" | ~500 | 2 |
| **S8** red-green (needs test-runner) | `Evidence`, orchestrator gates, prompts | ~400 | 1 |
| **S9** run view and persistence | `app/run_view.rs`, `ui/components/run_view.rs`, `runtime/chat_store` runs table, resume | ~900 | 3 |
| **S10** docs and end-to-end acceptance | `README.md` (usage, `Orchestrator` gate and Ctrl+O, menu row, provider-terms notice, Ollama "not recommended"), CHANGELOG, the manual "build a project from the chat" acceptance run | ~150 | 1 |

## Testing Strategy (strict TDD)

| Layer | What | How |
|---|---|---|
| Core unit | `Run` transitions (spawn limits, halt blocks spawn, retry reuses the spec, cancel denies), `OrchCall` parsing (wait clamped to 120), `ReadScope` (symlink out of a root, `..`, secret root, `$HOME` refused), `classify` fixtures, ledger render, topic keys | Pure, with explicit `Instant`s, written red first |
| Runtime | spawn → Done/Failed/TimedOut/Refused; cancel kills; halt then retry; the orchestrator gets no approval ask; worker read outside the scope reaches the approver | Fake `Runner` (`chat.rs:90`) scripted per engine, `tokio::time::pause`, temp HOME, a real `Hub` on a temp dir |
| App | Ctrl+O toggle per pane and badge (gate off, panes independent, blocked during a run), notice once, scope prompt, worker row, failure notice keys, picker opened on the right task, run override (relaunch and later workers use it, menu draft and file untouched, dropped at run end) | `app_tests/orchestra_*.rs` with the existing app harness, plus insta snapshots |
| Off-path | feature off: the registry, the request and the snapshot are byte-identical to 0.18.0 | characterization tests first |

## Migration / Rollout

No data migration. Older builds ignore `[ai.orchestrator]` and `[ai.tasks.orchestrate]`. The feature is default-off (global gate, then a per-pane opt-in) and undocumented until S10. Delivery is the `feature-branch-chain` above. Each slice reverts on its own; S3 (`SessionKey`) reverts as its PR set.

## Open Questions / Risks

- [x] D2 per-pane toggle: key is Ctrl+O, agreed in the `chat-pane` and `modal-input` deltas. Verify at S1e that no terminal/multiplexer layer swallows it.
- [ ] S3 cancel-worker and cancel-run keys are not pinned yet (the `modal-input` delta says "a chat key"); pick them at S3 against the chat key table.
- [ ] The channel by which the orchestrator reports an on-course verdict (S4) is not defined beyond `Finish{summary}`; define it in S4 design.
- [ ] D10 text classification depends on CLI wording; fixtures pin it, and it is re-verified each CLI bump. Gemini → Antigravity must be re-checked before S2.
- [ ] D6 TOCTOU: the scope decides only the ask. The read itself re-resolves under the deny-list, so a swapped link can at most read an allowed file without an ask.
- [ ] D4 shares the orchestrator's (pane, turn) for worker asks. In S1 a second simultaneous ask is denied `another approval is pending` (acceptable with one worker); S3 removes this.
- [ ] S1 runs end with the orchestrator turn (15 min); a long worker is cut there until S4.
- [ ] The test-runner SDD does not exist yet; it gates S8 and the product goal.
