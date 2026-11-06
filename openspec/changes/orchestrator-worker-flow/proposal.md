# Proposal: Orchestrator Worker Flow (workers that really work, quality through handoffs)

Status: reviewed by the owner on 2026-11-04; the question round is answered below. Facts and their evidence level are in `explore.md`.

## Intent

Orchestration exists so cheap or local models can do simple work without always depending on big expensive ones. Today that promise is not kept or observable:

- With ORCH on (claude>haiku) a request for changes across files returned fragments and no files. Whether a worker ran is **unknown**; no cause is established.
- [read] A coordinator that answers in text ends the run with no notice. A code worker may answer in text. Menu toggles do not limit workers. No worker can run a command. Handoffs are never compared with what changed.

## Decisions (owner, binding)

1. **`simple` worker, defined by role, "pick and shovel".** Executes direct, fully specified tasks (script, code, text, summary, producing an already defined design). Never designs, analyses, reviews or delegates. Tools: read, write, `run_command` when it exists and Exec is on; the menu is the ceiling.
2. **Brief.** The coordinator assigns it; a worker never selects itself. Minimum fields: goal, files, done when, out of scope. A missing field is refused visibly so the coordinator fixes it. Extra fields are allowed and flagged in the result, so the owner can decide to grow the format.
3. **Clarification.** A worker that does not understand ends with "needs specification: X". The coordinator answers or asks the user, then starts a new worker. At most two rounds per brief, then the run stops with a notice.
4. **Limit: 400 lines of code per file; tests have no fixed cap.** Exceeding asks the user: split in two parts, or keep one with the extra lines. No approval mode answers it. The limit must allow for well documented code.
5. **Undelegated text reply:** visible notice plus a key to ask the coordinator to delegate. No automatic retry. No model-free classifier of change vs question.
6. **Commands** run after the batch is approved; every command asks. No staged overlay.
7. **Write off in the menu:** a worker that must write is refused with a notice naming the toggle and telling the user how to turn Write on from the chat. Only the user can turn it on, never a model. The menu is a per-role ceiling and never widens beyond the role's toolset. Coordinator keeps Read+Orchestra; review stays read-only.
8. **Quality.** The app always checks handoffs against the ledger, with no model. Model review is mandatory for a cheap author, with no consent prompt, and optional for a strong one, with a visible warning when author and reviewer are the same model. The reviewer is the model the user assigned to the Review row, normally a cloud agent. When that row is a local model, a visible notice states the limits of local models as reviewers. Reuses the RDD pipeline; no new agent type.
9. **"Cheap" means local engines** for now.
10. **`summarize` stays its own task,** fed by the orchestrator or the handoff, which hold the context.
11. **Never fail silently.** Every refusal, gate and limit is visible. lazarobox never picks an engine or model; no automatic escalation to the cloud.
12. **Parallel writers.** Write-capable workers may run in parallel when the machine can take it, each with its own task and its own batch, so one worker's proposal never seals another's files.
13. **Waiting on the user stops the clock.** Time a worker spends waiting for the user's answer does not count against its deadline.
14. **Files in a brief** covers inputs and outputs. With Write off, `simple` is refused only if the brief lists files to write. `summarize` serves the flow; `simple` writes a summary only when the summary is a file to deliver.
15. **Split in two** ends the worker, and the coordinator issues two new briefs. It does not count as a clarification round.

## Assumptions (accepted by the owner on 2026-11-04)

From exploration:

- **A.** Comments, doc comments and blank lines do not count toward the 400; an unknown language counts every line and says so.
- **B.** The 400 measures lines the worker writes per file per brief, so a small fix in a big file does not trigger it.
- **C.** Writing outside the brief's listed files also asks rather than refuses.
- **D.** The limit and the brief format apply to `simple` workers only, not `code`.
- **E.** Tests are recognised by path (`tests/`, `*_test.*`, `*.spec.*`, `*.test.*`); inline tests such as `#[cfg(test)]` count as code.

Added by this proposal:

- **F.** C applies only inside the run's read scope. A write outside the scope stays refused, as the current spec requires.
- **G.** `simple` follows the existing fallback: unassigned resolves to `general`, shown visibly. If that is a cloud engine, the author is not "cheap", and decision 11 holds because the owner's menu chose it.
- **H.** S8 enforcement ("green without red refused") stays deferred; only its note that no worker has `run_command` is amended.
- **I.** `simple` joins the fixed task set outside `Set all tasks`, like `orchestrate`.

## Open points for design

These follow from the owner's answers and are not yet decided. Design proposes; the owner confirms.

- **"When the machine can take it."** Assumed: `max_parallel` stays the user's ceiling, with no hardware measuring. Check how a local server behaves with concurrent requests.
- **Several batches in one run.** How the user reviews them in order, and what happens when two workers touch the same file (brief overlap check, apply-time drift).
- **The per-pane command claim** stays one command at a time per pane unless design shows otherwise.
- **Turning Write on from the chat.** A per-chat grant like `/github`, or another form. Whether Exec gets the same path.
- **Mandatory cloud review** sends a local author's diff to the cloud and costs money. The owner accepted a cloud reviewer; the user sees it through the Review row and a notice.

## Scope

### In Scope: nine slices, in this order

Lines are production only and [inferred]. Each slice ships independently, and ORCH stays off by default.

| # | Delivers | Lines | Needs | Rollback (each is one revert) |
|---|---|---|---|---|
| W1 | Disclosure: undelegated reply notice and key; code worker that ended with an empty ledger; batch staged and never proposed; refused spawns; a turn degraded to no tools | 150-220 | - | Notices disappear; runs behave as today |
| W2 | Worker prompt that requires tools and forbids fragments as a result, without changing chat turns; menu ceiling | 100-150 | - | Prompt and families return to hard-coded |
| W3 | Spawn and finish gates on step state, with turn cap and visible override | 100-180 | W1 | Remove the gates; `spawn` as today |
| W4 | App-checked handoffs against the ledger, shown in result and close | 120-180 | W1 | Verdict returns to orchestrator text only |
| W5 | Labelled worker command ask, queued and answered in its own session | 150-220 | - | Ask variant unused; no user-visible loss |
| W6 | `run_command` for workers after approval, under Exec ceiling; prompts updated | 120-200 | W2, W5 | Toolset returns to Read+Write; prompts restored |
| W7 | Mandatory review for cheap authors; same-model warning | 120-200 | W4 | Review returns to high-risk or on request |
| W8 | `simple` task: menu row, role prompt, brief check, clarification rounds | 200-300 | W2 | Row gone; older builds ignore the table; stored runs per design |
| W9 | Per-file limit ask; writes outside the brief ask | 210-320 | W8 | `simple` writes with no limit; brief check remains |
| W10 | One batch per write-capable worker; parallel writers; several batch reviews in a run | 150-300, not yet explored | W2 | Workers return to one batch per pane |

Total: 1,420-2,270. Design decides where W10 falls in the order, since parallel `simple` workers depend on it.

**Budget honesty.** The earlier 150-220 estimate for `simple` did not cover decisions 2 to 4; re-estimated at 410-620, it does not fit one PR, hence W8 and W9. Every slice fits 400 on its estimate, but the agent-orchestrator delivery ran about 2.4x to 3x its estimates (whether those figures included tests is not known). At that rate W8 and W9 exceed 400 even from their low end, W1 and W5 can at their low end, and every other slice can at its high end. `sdd-tasks` must forecast this and split further where needed. This is a chain of PRs under any outcome; the owner picks the chain strategy after tasks.

**Order rationale.** W1 goes first: it shows what actually happens and may identify the cause of the trigger before anything else changes it. W7 precedes W8, so a cheap `simple` author never writes without the quality gate.

### Out of Scope (non-goals)

- Staged overlay; running commands on staged content.
- Cloud escalation; lazarobox choosing engines or models.
- Marking cheap models by task or responsibility.
- The question queue from #503, unless design proves it necessary.
- A model-free change-vs-question classifier; automatic retry.
- S8 test-first enforcement (assumption H).
- New agent types; nested orchestration; the ORCH notice on Ctrl+O (#596, shipped).

## Capabilities

### New Capabilities

None. The behaviour extends the capabilities below.

### Modified Capabilities

- `agent-orchestrator`: Worker toolsets (`simple`, Exec, menu ceiling, asks while staging), Worker contract (brief, clarification), ODD flow (gates, stale "tests" text), S8 note, Risk and Receipt-driven review (cheap authors), undelegated turn.
- `agent-tools`: Toolsets by session role (worker session, already stale).
- `orchestrator-handoffs`: app check against the ledger.
- `ai-task-models`: Fixed task set gains `simple`.
- `config-menu`, `configuration`: the `simple` row and slot.
- `prompt-profiles`: worker, `simple` and orchestrate prompts.
- `chat-pane`, `modal-input`: notices, limit ask, delegate key.
- `command-runner`, `code-write`: worker-labelled command ask; limit ask before a batch.

## Approach

Move the guarantees from prompts into app state, keeping the pure core and runtime shell:

- Facts the app can see decide (ledger, step state, toggles, line counts); model text never does.
- Worker asks copy the `WorkerRead` and labelled `Batch` pattern, so answers reach the right session.
- The review path reuses RDD on the staged batch, before the user's batch review.

Design decides the mechanisms: the brief's form (`spawn_agent` is free text today), how the marker is recognised, the line counter and the override key.

## Affected Areas

| Area | Impact | Description |
|---|---|---|
| `src/core/ai/orchestra/{mod,run,steps,ledger,rdd}.rs` | Modified | Toolset, gates, brief, rounds, check, cheap review |
| `src/runtime/orchestra.rs`, `src/runtime/orchestra/review.rs`, `src/app/orchestra.rs` | Modified | Families, notices, key, review trigger |
| `src/core/ai/chat/approval.rs`, `src/app/tool.rs`, `src/runtime/tools/host/{exec,files}.rs` | Modified | Worker command ask, limit ask |
| `src/core/ai/config.rs`, `src/core/menu.rs`, `src/core/config_edit.rs`, picker | Modified | `simple` task and row |
| `src/core/ai/chat/prompts/` | Modified | Worker and `simple` prompts; orchestrate text and its test |
| `src/runtime/chat.rs` | Modified | Possible visible notice at the degrade path |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Parallel workers share a per-pane batch and run gate: blocks, or one worker's proposal seals another's files | High | Decision 12: one batch per worker (W10); until W10 ships, write-capable workers run one at a time per pane |
| Prompt injection on small models, now with commands | High | Every command asks in every mode; jail; output stays untrusted; no mode answers limit asks; "needs specification" is treated as untrusted and capped at two rounds |
| Gates trap a legitimate run; clarification consumes the default 6 turns | Med | Turn cap, visible override, notice naming the gate; manual run of W3 and W8 together |
| Local models call tools unreliably | High | **Unit tests cannot prove this.** Manual acceptance per engine (Ollama, llama.cpp) for W2, W6, W8; failure is a visible notice |
| The notice shows after legitimate answers, as there is no classifier | High | Neutral text: no worker ran, nothing written, key to delegate; never an accusation |
| Cost: mandatory review may spend cloud money on local work | Med | Decision 8: the user chooses the Review row; existing usage rows; visible notice |
| Spec conflicts: S8, Toolsets by role, "no worker can run commands" | Certain | Deltas in spec phase; prompts and test changed with W6 |
| Resume of stored runs: unknown `simple` kind, brief and round counters, a downgrade | Med | Not yet checked; design must read `persist` and keep a stored run visible, never dropped |
| Comment-aware counting is wrong in some language | Med | Unknown language counts every line and says so (A); the result is only an ask, never a refusal |
| Worker times out during review, so tests never run | High | Decision 13: waiting on the user stops the clock |
| Estimates run low | High | Re-forecast in `sdd-tasks`; split, do not stretch |

## Rollback Plan

Per slice in the table above. Everything is gated by `[ai.orchestrator] enabled` (default off) and the per-pane ORCH toggle; with either off, chat is unaffected, and only the new `simple` menu row remains visible. Reverts go in reverse order where the "Needs" column links slices. Handoff data stays inert.

## Dependencies

- None external. Local engines available for manual acceptance.

## Success Criteria

Observable by the owner:

- [ ] With ORCH on, a change request ends with a batch to review or a visible notice, never only fragments.
- [ ] After a text-only coordinator turn the notice says no worker ran, and its key starts delegation.
- [ ] With Write off, a worker that must write is refused, and the notice names the toggle and how to turn it on from the chat. With Exec off, a worker has no `run_command`.
- [ ] Two write-capable workers run in parallel with different tasks, and each batch is reviewed on its own.
- [ ] A local model in the Review row shows a notice about its limits.
- [ ] With Exec on, after approving a batch, a worker's command asks with the worker's label, runs, and its output reaches the coordinator.
- [ ] A handoff claiming a file the ledger does not show is flagged in the result, with no model involved.
- [ ] A local author's batch arrives at batch review with a review receipt; same-model review shows a warning.
- [ ] `simple`: a brief missing "done when" is refused visibly; a third clarification round stops the run with a notice.
- [ ] `simple` writing 401 counted lines in a file asks split or keep, in every approval mode including `run`.
- [ ] No engine other than the menu's runs at any point.
- [ ] Manual acceptance on one Ollama and one llama.cpp model: the worker writes through tools and the batch is applied, or a visible notice explains why not.
- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
