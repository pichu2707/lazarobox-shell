# Exploration: Orchestrator Worker Flow

Written in the propose phase on behalf of the exploration agent, which had no write tool. Engram: `sdd/orchestrator-worker-flow/explore`. Checkout: `main` at v1.7.1.

## Trigger

With ORCH on (coordinator claude>haiku) the owner asked for changes across files and got code fragments and no files. **Whether a worker ran is unknown. No cause is established.**

## Read in code on main (re-checked in the propose phase)

| Fact | Where |
|---|---|
| `Run::spawn` takes a free-text prompt and refuses a code worker only on a read-only goal; `Task::Code` is compared by name | `src/core/ai/orchestra/run.rs:356-364`, `enqueue` at `:394` |
| `Run::next_turn` settles when nothing is live or pending, so a coordinator that ends with text and no worker ends the run; nothing in this path raises a notice | `run.rs:823-836` |
| The turn cap exists (`NextTurn::Capped`); default `max_turns` is 6, `max_parallel` 2, `max_workers` 8, worker timeout 600 s | `run.rs:831`, `src/core/ai/config.rs:320-331` |
| The code prompt says "Put code in fenced blocks" and "Show only the changed parts", and does not mention tools | `src/core/ai/chat/prompts/programmer/code.md:5-6` |
| `Toolset`: Orchestrator = Read+Orchestra, Code = Read+Write, others read-only; `for_kind` maps by task | `src/core/ai/orchestra/mod.rs:77-110` |
| `Family::Exec` and `ToolToggle::{Write,Exec}` exist; no file under `src/core/ai/orchestra/` or `src/runtime/orchestra*` reads `ToolToggle` | `src/core/ai/tools/registry.rs:12`, `src/core/ai/config.rs:197` |
| A command is refused while the pane's batch is staging or in review, and refused with `CHAT_BUSY` while the gate is busy | `src/runtime/tools/host/exec.rs:156,291-300` |
| Workers with Write stage under `StageKey::Pane(pane)`; the key has a single variant in the code read (`let StageKey::Pane(pane) = id.key`) | `src/runtime/orchestra.rs:1597`, `src/runtime/chat.rs:1111` |
| `ApprovalAsk::Exec` has no worker field; `is_worker()` and `worker()` do not cover it; `WorkerRead`, `Consent` and a labelled `Batch` do | `src/core/ai/chat/approval.rs:141-154,260-277` |
| A command asks on any engine and in any approval mode | `approval.rs:316-317` |
| `Handoff::of` only normalises and caps; verdict is the orchestrator's unless `blocked` | `src/core/ai/orchestra/ledger.rs:87-146` |
| With no tooling, tools not `On`, or no locality, `open_session` returns `Ok(None)` with no notice in that branch | `src/runtime/chat.rs:900-911` |
| `Task::SET_ALL` is `[Self; 5]`, and a test pins it to `Task::ALL[..5]` | `src/core/ai/config.rs:40,3351` |
| ORCH prompts and a test pin "No worker can run commands or tests" | `prompts/{programmer,analyst}/orchestrate.md:12`, `src/core/ai/chat/prompts/tests.rs:141` |

## Read in specs

- `agent-orchestrator`, "Worker toolsets": every worker but `code` is read-only; a write outside the run's scope is refused and never asked; staging inside the scope must not ask; review time counts against the worker deadline.
- `agent-orchestrator`, "Receipt-driven review": a `high` change asks the user's consent before any review; a declined change goes on `not reviewed`.
- `agent-orchestrator`, "ODD test-first evidence": deferred to 1.2 by the owner on 2026-10-31; its note states no worker is offered `run_command`.
- `agent-tools`, "Toolsets by session role": worker session = read tools only "until write tools exist". This text is already stale for `code` workers.
- `ai-task-models`, "Fixed task set": six named tasks, "no way to add". An unassigned non-general task resolves to `general`. `Set all tasks` covers five.

## Corrections to the exploration notes

- The approval file is `src/core/ai/chat/approval.rs`, not under `core/ai/tools/`.
- `external_tests.rs:267` is `src/runtime/chat/external_tests.rs`. It pins the user's server tools on an orchestrator turn. It does not pin worker toolsets, so it is not evidence that the menu may not limit workers. The real conflict is the two spec requirements above.

## Reported by exploration, not re-verified

- A worker's command ask would be addressed to the pane session and never reach the worker's wait (`src/app/tool.rs:66,145`, `approver.rs:289`). The missing field is read; the runtime outcome is inferred.
- `exec::plan` refuses without a jail; output is cleaned, redacted and capped at 32 KiB.
- One worker's `propose_batch` seals another's staged files.
- RDD fires only on high risk or on request (the spec agrees; `rdd.rs` was not re-read).
- Adding a Task touches `config.rs`, `menu.rs`, `picker.rs`, `prompts.rs`, `tools/mod.rs`, `orchestra/mod.rs`, `app/orchestra.rs`, `config_edit.rs`.
- The degrade path at `runtime/chat.rs:900-911` may be unreachable for `claude`.
- `code.md` is shared with chat turns and gets the tools block appended last, creating conflicting instructions.

## Inferred

- The fenced-block prompt may contribute to fragments. Not proven as the cause of the trigger.
- Parallel writers on one pane can block each other through the shared batch and run gate.
- Slice line estimates. The agent-orchestrator delivery recorded real sizes of about 2.4x to 3x its estimates (`archive/2026-11-02-agent-orchestrator/proposal.md`, O11).

## Not checked

- How `src/core/ai/orchestra/persist*` stores a worker's task kind, and what a build without `simple` does with a stored run that has one. Design must check this.
