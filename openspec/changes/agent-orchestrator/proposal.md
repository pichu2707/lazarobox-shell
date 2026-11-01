# Proposal: Agent Orchestrator (one coordinator, many workers, tracked handoffs)

## Intent

The AI menu assigns an engine and model to five tasks (General, Code, Chat, Review, Summarize), but only `Task::Chat` is resolved at runtime (`engine.rs:281`, `app/context.rs:101`, `runtime/prompts.rs:79`; even the Prefix-s summary uses Chat). The "P6 orchestrator" exists only as a roadmap line in the ai-engines and ai-chat proposals. Nothing is built.

The user works with strong models through their own CLIs and subscriptions and wants a **powerful coordinator, not a toy**: an orchestrator agent that splits work across several workers, each on the engine and model the menu gives its task, keeps handoffs that show whether the project is on course, and drives TDD/SDD phases the way this repo does. This change finally makes the other four task rows mean something.

## Users and situations

- A user in a chat pane who asks for a multi-step job (explore, plan, review, draft) and wants it split across the right models.
- A user running ODD work who wants step handoffs and a feature document persisted and checked, and a high-risk change reviewed (RDD) before deciding on it.
- Anyone on a consumer CLI login (Pro/Max, ChatGPT, Google account) who must know the provider risk once.

## Decisions (user, binding; engram `sdd/agent-orchestrator/proposal-questions`)

- **O1 Menu is the single source of truth.** A new task row **Orchestrate** (own engine and model) decides who orchestrates. Workers get the engine and model the menu assigns to their task (General, Code, Chat, Review, Summarize). lazarobox never picks a model.
- **O2 No API key required.** A one-time informational notice (accept once, never blocks) says providers recommend API keys for automation and that consumer accounts carry risk. The README carries the same note.
- **O3 Full vision in ONE SDD**, delivered as chained PRs. S1 is only the first slice.
- **O4 Handoffs in engram** (like apply-progress): what was done, what is left, decisions.
- **O5 Approvals.** The user approves a read scope at run start; reads inside it are pre-approved. The scope applies the jail deny-lists (agent-tools T5) to both the requested and the resolved path. The orchestrator never approves anything.
- **O6 Worker failure stops the run** at the moment it happens and shows the concrete provider error (409, 429, auth, any). Inline options: retry, change model, change provider, cancel. Change model/provider opens the menu picker for that task and relaunches the worker. No silent fallback. 429 is a first-class case.
- **O7 Separate SDDs.** write-tools and a test-runner tool are dependencies with their own SDDs. Until they exist, workers read, plan, review and draft. **Real TDD red-green is impossible without them.** Update 2026-10-27: the write tools shipped as coding-tools M2 (#342, staged batches with a full-pane user review); the test runner is coding-tools M3 (`run_command`), not started.
- **O8 Defaults.** The orchestrator has no `run_in_pane`; worker results are rows in the chat (live run view later); runs are not resumable after a restart in early slices; cost connects later to the project-17 "Measure AI usage" item.
- **O9 Activation (2026-10-20).** A global gate `[ai.orchestrator] enabled` (menu row `Orchestrator`, on/off, default off) PLUS a per-pane toggle: key Ctrl+O and an `ORCH` badge in the chat pane, following the AUTO/RUN badge pattern. Only panes with ORCH on run their chat turns on the Orchestrate assignment; other panes keep the Chat assignment. The pane flag is in memory only.
- **O10 Failure picker is run-scoped (2026-10-20).** Change model / change provider from a failure alert applies until the run ends and is NOT persisted to the menu or config. It applies to the relaunched worker and later workers of that task in the same run, and is shown in the run state and worker rows. After the run the menu value is unchanged.
- **O11 Delivery (2026-10-20).** `feature-branch-chain`, tracker branch `feat/agent-orchestrator`. S1 is 12 chained PRs (user decision 2026-10-21): S1a, S1b, S1b-2, S1c-1, S1c-2, S1d-1, S1d-2, S1d-3, S1e-1a, S1e-1b, S1e-1c, S1e-2. S1b carries `size:exception: approved` (~1,130 lines with review fixes); S1e-2 is one PR (~600) with `size:exception: approved`; S1d gets no size exception. Real sizes ran ~2.4x to 3x the estimates (S1a ~1,160, S1b ~1,130), so S1c, S1d and S1e-1 were re-estimated and split as detailed in `tasks.md`. The scope of S1 is unchanged. README is untouched until S10.
- **O12 ODD + RDD replace the SDD phases (2026-10-27).** The user: "la idea es poder hacer un sistema ODD + RDD creo que sería lo más óptimo". The run follows Organic Driven Development instead of the explore → propose → spec → design → tasks → apply → verify phases, and a high-risk change gets a receipt-driven review (RDD) before the user sees it:
  - **ODD**: the run classifies the goal as small or large. Small work is one worker. Large work gets a feature document (specs quoting the goal verbatim, tasks, log), with one worker per task, each handed the document by reference. Test-first evidence comes from the test runner, never from worker text. S5 becomes the ODD flow; S8 becomes ODD test-first evidence.
  - **RDD**: new slice S7b. A change from `code` workers is assessed for risk; a high-risk change is reviewed by `review` workers through up to four lenses (risk, resilience, readability, reliability), with a refuter for inferential blockers and at most one bounded correction, and ends in a receipt. The user consents per change; the orchestrator never consents and never approves.
  - Commits stay out of scope (non-goal below). A receipt is information, never an approval to apply or commit.
  - The user's choices P1-P4 are listed under **Decisions (O12)**.

## Scope

### In Scope (chained PRs; a slice may span more than one ~400-line PR, sized by sdd-tasks)

| # | Slice | Depends on |
|---|---|---|
| S1 | **Contract + one read-only worker.** Orchestrate row; `[ai.orchestrator] enabled` global gate (default off) and `Orchestrator` menu row; per-pane ORCH toggle (Ctrl+O) and badge; one-time provider notice; availability parametrized by `Task`; tools `spawn_agent`, `wait` (bounded, at most 120 s), `collect`, `cancel` offered only to the orchestrator; one worker at a time; run-start read scope; worker tools = read-only subset; result untrusted-wrapped; worker result row; failure stops the run with the raw error and offers retry/cancel; delivered as 12 chained PRs S1a, S1b, S1b-2, S1c-1, S1c-2, S1d-1, S1d-2, S1d-3, S1e-1a, S1e-1b, S1e-1c, S1e-2 (user decision 2026-10-21; no README change) | - |
| S2 | **Failure alerts.** Per-adapter error classification (rate limit 429, conflict 409, auth, quota, timeout, jail refusal, other); inline notice; change model / change provider via the menu picker for that task as a run-scoped override (not persisted), then relaunch | S1 |
| S3 | **Multiple workers.** `PaneId` → session key generalization; `max_parallel` (default 2); labelled approval queue for asks outside the scope; cancel one or all; per-run limits (workers, wall clock) | S1 |
| S4 | **Handoffs and ledger.** Engram handoff per worker and per ODD step; orchestrator turns continue from a compact in-memory ledger so long runs survive the 15-min turn cap; on-course check against the run goal | S3 |
| S5 | **ODD flow (read-only until S7).** Orchestrate playbook prompts and ledger steps: authorize → explore → resolve (one question to the user) → classify (small or large) → track (large only: feature document) → implement task by task → close (verified outcome and a risk line). Until S7, implement produces drafts and review findings only (O12) | S4 |
| S6 | **Usage.** `StreamEvent::Usage` per adapter; per-worker and per-run rows where the CLI reports them | S1 |
| S7 | **Write integration.** Code workers get the coding-tools write tools; their changes are staged and reach the disk only through the user's batch review; per-worker change ledger ("files changed") | **coding-tools M2** (merged, #342) |
| S7b | **Receipt-driven review (RDD).** Risk assessment of a worker change; for a high-risk change, per-change user consent, up to four lens reviews by `review` workers, a refuter, at most one bounded correction, and a receipt handoff (O12) | S7, S4 |
| S8 | **DEFERRED to 1.2 (user, 2026-10-31).** **ODD test-first evidence.** For a task with a runnable deterministic test, the run requires failing evidence before the implementation and passing evidence after, both from `run_command`; a task without one records an explicit exception | **coding-tools M3** (`run_command`), S7 |
| S9 | **Run view and persistence.** Full-pane worker transcript; runs stored and resumable after restart | S4 |
| S10 | **Final docs and end-to-end acceptance.** The only slice that edits the README (usage, `Orchestrator` gate and Ctrl+O, Orchestrate row, provider-terms note, Ollama "not recommended"); CHANGELOG; manual acceptance of "build a programming project from the chat, without an IDE" (plan, failing test, implementation, passing test, review) | S1 to S7b and S9, **coding-tools M2** (S8 and M3 for workers deferred to 1.2) |

### Out of Scope (non-goals)

- Building write tools or a test runner (separate SDDs).
- Silent fallback to another engine or model; lazarobox choosing models.
- Nested orchestrators (workers never get `spawn_agent`).
- `run_in_pane` for the orchestrator.
- Requiring API keys; OpenAI-compatible adapter; git worktrees per run; commits or PRs by agents.
- Cost budgets with real money until S6 data exists and project-17 lands.

## Worker contract (product level)

- **In (`WorkerSpec`)**: task kind, prompt, engine and model resolved from the menu at spawn, toolset by kind, deadline.
- **Out (`WorkerResult`)**: status (done, failed, cancelled, timed out, refused), final text (capped, wrapped as untrusted data), elapsed, provider error class and message, later usage and files changed.
- **Refusal at spawn** when the engine is unavailable (no jail, outdated, model missing): structured, never a fallback.
- **Isolation**: one stateless CLI process per worker turn, private workdir, same jail as a chat turn.

## Capabilities

### New Capabilities
- `agent-orchestrator`: run lifecycle, orchestration tools, worker contract, limits, the read scope of a run (the chat origin, no prompt), failure alert and relaunch, provider notice, the ODD flow and the receipt-driven review.
- `orchestrator-handoffs`: handoff content, engram topic keys, on-course check, behaviour when memory is off.

### Modified Capabilities
- `ai-task-models`: the fixed task set gains Orchestrate; "Runtime use of the chat assignment" widens to every task.
- `config-menu`: the Orchestrate row; the `Orchestrator` gate row; the picker opened from a failure notice (run-scoped override, never saved).
- `configuration`, `config-persistence`: a sixth slot, `[ai.orchestrator]` keys, notice acceptance.
- `chat-engines`: availability by task; provider error classification; usage events (S6).
- `chat-pane`: worker rows (announced workers, no prompt), failure notice, badge, approval labels and queue.
- `modal-input`: ORCH toggle key (Ctrl+O), notice option keys, failure alert keys (r retry, m model, p provider, c or Esc cancel), cancel keys.
- `prompt-profiles`: orchestrate prompt, ODD playbooks and RDD lens prompts.
- `chat-memory`, `engram-client`: handoff notes respect opt-in and redaction; topic-key upsert.
- Pending in `openspec/changes/agent-tools` (not yet archived): `agent-tools`, `lazarobox-mcp-server` gain per-session keying, worker toolsets and pre-approved read scope.

## Approach

Exploration approach C: async `spawn_agent` / `wait` / `collect` / `cancel` over a RunManager that owns worker tasks independently of the orchestrator turn. Pure core `core/ai/orchestra/` (run, worker, ledger state machines); runtime shell `runtime/orchestra.rs` reusing `runner_with`, the Hub and `Waits` after keying them by session instead of `PaneId`. The whole contract is tested with a fake `Runner` and paused tokio time.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/orchestra/` | New |
| `src/runtime/orchestra.rs` | New |
| `src/core/ai/config.rs`, `src/core/ai/menu.rs` | Modified (Orchestrate slot, keys) |
| `src/core/ai/chat/{engine,adapters}.rs`, decoders | Modified (task availability, errors, usage) |
| `src/runtime/chat.rs`, `src/runtime/mcp/session.rs`, approver | Modified (session keying, S3) |
| `src/core/ai/tools/`, `src/runtime/tools/host.rs` | Modified (toolsets, read scope) |
| `src/app.rs`, `src/ui/components/chat.rs` | Modified (rows, notice, queue) |
| `prompts/`, `README.md`, `CHANGELOG.md` | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Provider terms on consumer logins (see below) | Med | O2 notice + README; unmodified binaries; lazarobox never reads tokens; API keys stay usable (verify jail env scrub keeps `*_API_KEY`) |
| 429 under parallel load on one subscription | High | `max_parallel` default 2; S2 alert names the limit; no automatic retry storm |
| CLI caps: MCP 900 s, turn total 15 min | High | Async tools, bounded `wait`; S4 ledger continuation; S1 runs bounded by one orchestrator turn |
| `PaneId` refactor blast radius (`app.rs` ~8k lines) | High | Isolated in S3; thin worker wrapper in S1 |
| Prompt injection via worker output | High | Untrusted wrapper (design D11); orchestrator cannot approve; writes always ask |
| Small local models cannot orchestrate | Med | Menu marks Ollama on the Orchestrate row as not recommended; documented |
| Gemini CLI reportedly replaced by Antigravity CLI on 2026-06-18 (secondary source, FAQ) | Med | Re-verified 2026-10-30 (Gemini CLI README): confirmed for free/AI Pro/Ultra plans; S2 keeps the Gemini adapter only, an `agy` (Antigravity CLI) adapter is a follow-up |
| Vision overreaches before write-tools/test-runner | Med | O7; S7/S8 gated; copy says "drafts" until then |
| A review worker or refuter is talked into a pass by the change it reviews | Med | The candidate is wrapped as untrusted data; reviewers are read-only; a receipt never approves; the user decides the batch (S7b) |

**Provider terms (reading of public text, checked 2026-10-20, not legal advice):**

- Anthropic, [legal-and-compliance](https://code.claude.com/docs/en/legal-and-compliance): "Developers building products or services that interact with Claude's capabilities, including those using the Agent SDK, should use API key authentication"; also "Nor does it prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription"; "Advertised usage limits for Pro and Max plans assume ordinary, individual usage". [Consumer Terms](https://www.anthropic.com/legal/consumer-terms) s.3 bars "automated or non-human means" except "via an Anthropic API Key or where we otherwise explicitly permit it".
- OpenAI, [Codex auth](https://learn.chatgpt.com/docs/auth): "API keys are still the recommended default for automation." [Terms of Use](https://openai.com/policies/terms-of-use/): may not "Automatically or programmatically extract data or Output" (scope for local `codex exec` unclear).
- Google, [Gemini CLI ToS](https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md): accessing its services "using third-party software, tools, or services ... may be grounds for suspension or termination of your account"; [FAQ](https://geminicli.com/docs/resources/faq/): "the supported and secure method is to use a Vertex AI or Google AI Studio API key." Highest enforcement risk.

## Rollback Plan

`[ai.orchestrator] enabled = false` is the default; with it off, no orchestration tool is offered and chat behaves as in 0.18.0. Delivery is `feature-branch-chain` on `feat/agent-orchestrator`. Each slice is revertable on its own; S3 (session keying) is the only deep refactor and is reverted as one PR. Older builds ignore the new keys and the sixth slot. Handoff notes in engram are inert data.

## Dependencies

- coding-tools M2 write tools (S7, merged in #342). coding-tools M3 `run_command` shipped in 0.28.0 for chat panes; S8, which would give it to workers, is deferred to 1.2 (user decision, 2026-10-31).
- Engram running with chat memory on, for persisted handoffs (S4).
- Project-17 "Measure AI usage" for cost semantics (S6).

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass on every slice.
- [ ] With the feature off, nothing changes for existing chats; with the gate on, only panes with ORCH on orchestrate.
- [ ] A model/provider change from a failure alert lasts for the run only and leaves the menu value unchanged.
- [ ] S1: the orchestrator on its menu assignment spawns a Review worker that runs on the Review assignment, reads only inside the approved scope, and returns a result row; the orchestrator never sees an approval prompt.
- [ ] A forced 429 from a fake worker stops the run at once and shows the provider message with retry / change model / change provider / cancel; no other engine runs.
- [ ] S3: two workers run in parallel; cancelling one denies its pending asks.
- [ ] S4: each worker and ODD step leaves an engram handoff (done, left, decisions).
- [ ] S5: a large goal gets a feature document whose specs quote the goal verbatim, and each task worker is handed the document by reference; a small goal runs as one worker with no document.
- [ ] S7b: a high-risk change is reviewed only after the user consents for that change, by the lenses its risk calls for, with at most one bounded correction, and ends in a receipt; a declined change goes on to the user's batch review unreviewed.
- [ ] The notice appears once, is accepted, and never blocks.

## Proposal question round

Resolved: two rounds (engram `sdd/agent-orchestrator/proposal-questions`), recorded as O1 to O8.

Assumptions, reviewed by the user one at a time on 2026-10-27:
- (a) Confirmed. Orchestrate follows the existing fallback-to-General rule; Ollama is allowed but marked "not recommended".
- (b) Changed by the user on 2026-10-27 ("Avisa y activa"): with chat memory off or engram down, the run turns memory on for its own handoffs (starting engram when needed) and says so once; the menu setting is not changed. Only when engram cannot be reached does the run keep handoffs in memory and show "handoffs not persisted". Rejected: always degrade without trying.
- (c) Confirmed. When one worker fails during a parallel run, no new worker starts and running siblings finish their turn, but the orchestrator is not resumed until the user picks an option.
- (d) Changed by the user: retry relaunches the same `WorkerSpec` from scratch (CLI turns are stateless), with the failed attempt's error appended to the prompt as untrusted data, sanitized and capped like the alert. Rejected: a retry identical to the failed attempt.
- (e) Confirmed. Reads outside the run scope ask the user through the labelled queue; they are never silently denied or allowed.

## Decisions (O12)

Proposed by the planner on 2026-10-27 and confirmed by the user the same day, one at a time.

- **P1 What RDD reviews. Confirmed 2026-10-27 (user: "vamos"): (a).** The staged batch, before the user's batch review, so a correction never touches the disk and the user sees the receipt next to the diff. Rejected: (b) the applied files, after the user approved the batch.
- **P2 Who consents. Confirmed 2026-10-27: (a).** The user, once per high-risk change, through an inline prompt like the scope prompt. Rejected: (b) a per-run setting chosen at run start.
- **P3 How risk is assessed. Confirmed 2026-10-27: (a).** Deterministic rules in the pure core (paths and kinds: build files, CI, dependencies and lockfiles, config formats, secrets) give the floor, and the orchestrator may only raise it. Rejected: (b) the orchestrator's judgment alone.
- **P4 Lenses and workers. Confirmed 2026-10-27: (a).** Each lens is one `review` worker on the Review assignment, run in parallel once S3 lands and one by one before. Rejected: (b) one `review` worker covers every selected lens.
