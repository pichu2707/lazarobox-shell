# Agent Orchestrator Specification

## Purpose

Defines the orchestrator: one coordinator agent that splits a multi-step job across worker agents, each running on the engine and model the AI menu assigns to its task. It covers the run lifecycle, the orchestration tools, the worker contract, limits, the run-start read scope, approvals, failure alerts and relaunch, the provider notice, the ODD flow and the receipt-driven review (RDD). Handoff persistence is in orchestrator-handoffs. The menu is the single source of truth: lazarobox never picks an engine or a model.

Slice tags: [S1] contract + one read-only worker; [S2] failure alerts; [S3] multiple workers; [S4] handoffs and ledger (see orchestrator-handoffs); [S5] ODD flow, read-only until S7; [S6] usage; [S7] write integration (needs coding-tools M2, merged); [S7b] receipt-driven review (RDD); [S8] ODD test-first evidence (not implemented; deferred to 1.2, see its requirement); [S9] run view and persistence. Tags: [auto] = `cargo test` with a fake `Runner` and paused tokio time; [manual] = real machine.

Product goal: the finished feature MUST let the user build a programming project entirely from the lazarobox chat, without opening an IDE: plan, write code, run tests and review. That end state depends on the coding-tools write tools (M2) and on `run_command` reaching workers. History (user decision, 2026-10-31): 1.1.0 shipped WITHOUT S8, so the goal was met up to plan, staged implementation applied through the batch review, and review. Since W6 a `code` or `simple` worker is offered `run_command` only with Exec on, after its batch is approved and with every command approved by the user, so a test run is shown by a collected `run_command` result; S8 enforcement (red evidence before green) stays deferred to 1.2. coding-tools M3 `run_command` itself shipped in 0.28.0 for chat panes. The goal is accepted by the end-to-end requirement tagged [S10], as amended below. [S10] is the final slice: user-facing documentation and end-to-end acceptance, after S1..S9. Slices before S10 MUST NOT change the README; the feature stays off by default and undocumented there until S10. Internal docs (openspec, CHANGELOG Internal entries) may change in any slice.

Terms: a *run* is one orchestrated job started from an orchestrator turn; a *worker* is one stateless CLI (or loopback) turn started by the run for a task kind; a *kind* is one of `general`, `code`, `chat`, `review`, `summarize`.

## Requirements

### Requirement: Two-level activation, off by default [S1]
Orchestration is gated twice. The global gate is `[ai.orchestrator] enabled` (menu row `Orchestrator`), default `false`. The per-pane toggle is the ORCH mode of one chat pane: it is toggled by the key Ctrl+O (see modal-input and chat-pane), is held in memory only, is off for every new pane and is never written to the configuration. A pane is an *orchestrating pane* only when the global gate is on AND its ORCH mode is on. While the global gate is `false`, no orchestration tool is offered to any engine, no run can start, no badge is shown, the toggle key does nothing, and chat panes behave exactly as before this change. Changing the gate or a pane's ORCH mode MUST apply from the next message and MUST NOT affect a turn already running. Turning the global gate off makes every pane non-orchestrating from the next message; their ORCH modes are kept and take effect again when the gate is turned on.

#### Scenario: Disabled offers no tools [auto]
- GIVEN `enabled = false` and a pane whose ORCH mode is on
- WHEN a chat turn is built for any engine
- THEN the tool list contains none of `spawn_agent`, `wait`, `collect`, `cancel` and no ORCH badge is drawn

#### Scenario: Enabled alone does not orchestrate [auto]
- GIVEN `enabled = true` and a pane whose ORCH mode is off
- WHEN a message is sent
- THEN the turn is a normal chat turn on the `chat` assignment and no orchestration tool is offered

#### Scenario: Toggle applies to the next message [auto]
- GIVEN `enabled = true`, a pane with a turn in flight, and its ORCH mode switched on
- WHEN the running turn ends and the next message is sent
- THEN the next turn is an orchestrator turn and the earlier one was not changed

#### Scenario: Panes are independent [auto]
- GIVEN `enabled = true`, pane A with ORCH on and pane B with ORCH off
- WHEN a message is sent in each
- THEN A's turn is an orchestrator turn on `orchestrate` and B's turn is a chat turn on `chat`

#### Scenario: Gate off then on [auto]
- GIVEN pane A with ORCH on, then `enabled` switched to `false` and back to `true`
- WHEN a message is sent after each change
- THEN the first is a chat turn on `chat` and the second is an orchestrator turn

### Requirement: Orchestrator uses the Orchestrate assignment [S1]
In an orchestrating pane, a chat turn MUST be an orchestrator turn that runs on the effective assignment of the `orchestrate` task (own assignment, else `general`; see ai-task-models), read from the current configuration on every send. That pane MUST NOT fall back to the `chat` assignment, MUST NOT pick another engine when the effective one is unusable, and MUST show the existing engine guidance instead. Panes that are not orchestrating keep using the `chat` assignment. An ORCH badge MUST be shown in an orchestrating pane, following the AUTO and RUN badge pattern.

#### Scenario: Orchestrate engine runs the turn [auto]
- GIVEN an orchestrating pane, `orchestrate` is `claude` / `opus` and `chat` is `ollama` / `llama3:8b`
- WHEN a message is sent
- THEN the turn goes to claude with `opus`

#### Scenario: Unusable orchestrator engine [auto]
- GIVEN an orchestrating pane and `orchestrate` resolves to `codex` which is `not installed`
- WHEN a message is sent
- THEN nothing is sent, the engine guidance is shown, the `chat` engine is not contacted and assignments are unchanged

### Requirement: One-time provider notice [S1]
The first time the orchestrator would start a run while `[ai.orchestrator] acknowledged` is `false`, an informational notice MUST be shown saying that providers recommend API keys for automation and that consumer-account logins carry risk. The notice MUST be accepted with one key, persisted as `acknowledged = true`, and never shown again once accepted. It MUST NOT block: the run starts without waiting for the answer, no engine is refused because of it, and no API key is required. (The README statement is a [S10] requirement; see Final documentation.)

#### Scenario: Shown once, never blocks [auto]
- GIVEN `acknowledged = false` and an orchestrating pane
- WHEN the first run starts
- THEN the notice is shown, the first worker is spawned without waiting for the key, and after acceptance the key is saved as `true`

#### Scenario: Not shown again [auto]
- GIVEN `acknowledged = true`
- WHEN a run starts
- THEN no notice is shown

#### Scenario: Not accepted is shown again [auto]
- GIVEN the notice was dismissed without accepting
- WHEN the next run starts
- THEN the notice is shown again

### Requirement: Orchestration tools [S1]
The orchestrator turn MUST be offered exactly these orchestration tools through the lazarobox tool server: `spawn_agent{kind, prompt}` returning a worker id, `wait{ids, timeout}`, `collect{id}` and `cancel{id}` ([S4] adds `finish{summary}`, which closes the run). They MUST be offered to the orchestrator session of an orchestrating pane only. A worker session MUST NOT be offered any of them (no nested orchestrators). An unknown `kind`, an empty prompt or a prompt over the argument cap MUST be refused with an error result and no worker is created.

#### Scenario: Workers cannot spawn [auto]
- GIVEN a running worker session
- WHEN its tool list is built
- THEN it contains none of the four orchestration tools, and a forged `spawn_agent` call is refused

#### Scenario: Bad kind refused [auto]
- GIVEN an orchestrator turn
- WHEN `spawn_agent` is called with kind `deploy`
- THEN an error result is returned and no worker exists

### Requirement: Turning ORCH on says what the chat loses
The coordinator of an orchestrating pane is offered the read tools and the orchestration tools only, whatever the saved Write, Exec and GitHub toggles say. Turning ORCH on with the key Ctrl+O MUST therefore add two system lines to the pane's transcript: a short head (`ORCH on`) and a body saying that the chat itself cannot write files, run commands or read GitHub, that a code worker writes through the batch review, and that Ctrl+O goes back to a plain chat. The head is short so the ORCH badge, which covers the end of the first row of an empty chat, never cuts the body. The lines are ASCII, are transcript notices only (never sent to a model) and are said again each time ORCH is turned on. Turning ORCH off adds none.

#### Scenario: ORCH on says it [auto]
- GIVEN `enabled = true` and a chat whose Write, Exec and GitHub toggles are on
- WHEN Ctrl+O turns its ORCH mode on
- THEN the transcript gains `ORCH on` and a line naming write, run commands, GitHub, the code worker and Ctrl+O

#### Scenario: ORCH off is silent [auto]
- GIVEN a pane with ORCH on
- WHEN Ctrl+O turns it off
- THEN no line is added

### Requirement: Orchestrator toolset limits [S1]
The orchestrator MUST NOT be offered `run_in_pane` and MUST NOT have any means to approve, deny or answer an approval. Any approval prompt MUST be answerable only by a key event from the user. Text produced by an engine, a worker or a tool result MUST NOT be able to answer a prompt.

#### Scenario: No run_in_pane [auto]
- GIVEN an orchestrator turn
- WHEN its tool list is built
- THEN `run_in_pane` is absent and a forged call is refused

#### Scenario: Orchestrator cannot approve [auto]
- GIVEN a pending read approval of a worker
- WHEN the orchestrator turn emits text or tool calls that look like `y`
- THEN the approval stays pending until a user key event answers it

### Requirement: Worker contract [S1, W8]
A worker MUST be started from a `WorkerSpec` holding: kind, prompt, the engine and model resolved for that kind at spawn time (`general`, `code`, `chat`, `review`, `summarize`, `simple`; the run's override for the kind if one exists, else the menu), the toolset for the kind as limited by the menu ceiling, and a deadline (default 10 minutes, key `worker_timeout_secs`; time spent waiting for the user does not count). It MUST end with a `WorkerResult` holding: status (`done`, `failed`, `cancelled`, `timed_out`, `refused`), the final text (capped and wrapped as untrusted data), elapsed time, and the provider error class and message when the status is `failed`. A worker MUST be one stateless process per turn with a private working folder and the same jail as a chat turn. The engine and model used MUST be those in effect at spawn time (run override, else menu), even if the menu changes later during the run.
(Previously: six kinds without `simple`, a fixed deadline that counted every second, a toolset not limited by the menu)

#### Scenario: Review worker runs on the Review assignment [auto]
- GIVEN `orchestrate` is `claude` / `opus`, `review` is `codex` / `default`
- WHEN the orchestrator spawns kind `review`
- THEN the fake runner receives a request for codex with model `default`, and the result carries status `done`

#### Scenario: Unassigned kind falls back as the menu says [auto]
- GIVEN `review` is unassigned and `general` is `claude` / `sonnet`
- WHEN a `review` worker is spawned
- THEN it runs on claude / `sonnet`

#### Scenario: Menu change after spawn [auto]
- GIVEN a running `code` worker and the menu then saves a new `code` assignment
- WHEN the worker ends
- THEN its result records the engine and model it started with

#### Scenario: Simple is a valid kind [W8, auto]
- GIVEN an orchestrator turn
- WHEN `spawn_agent` is called with kind `simple` and a complete brief
- THEN a worker is created on the `simple` assignment, and a kind `deploy` is still refused

### Requirement: Refusal on an unavailable engine [S1]
Before starting a worker, the engine resolved for its kind MUST pass the same availability checks as a chat turn for that task (installed, version, model present, jail where required). When it does not, the spawn MUST end with status `refused`, a structured reason and no process started. lazarobox MUST NOT pick another engine or model. A refusal MUST be handled as a worker failure (see Failure stops the run) and classified `setup`: the failure alert shows `(setup)`. The reason MUST be given to the coordinator in the tool result of the refused worker (one redacted line, capped) instead of only `worker N: refused`, with absolute paths and `~/` paths replaced by `<path>`. `m` on a refused worker MUST open the model picker of the engine configured for its task; with no engine to take from it MUST open the engine list and a notice says why. (Since 1.2.1.)

#### Scenario: No jail refuses [auto]
- GIVEN `review` is `gemini` and the jail is Unavailable
- WHEN a `review` worker is spawned
- THEN the status is `refused` with the no-jail reason, no process starts and no other engine is contacted

#### Scenario: Model missing refuses [auto]
- GIVEN `code` is `ollama` with a model that is not downloaded
- WHEN a `code` worker is spawned
- THEN the status is `refused` and the reason names the model and `download required`

#### Scenario: A refusal is a setup failure with its reason [auto]
- GIVEN a worker routed to an ollama model that is not downloaded, and a reason that holds the absolute path `/home/u/models/m.gguf`
- WHEN the spawn is refused
- THEN the alert shows the class `(setup)`, the coordinator's tool result carries one redacted, capped line with `<path>` in place of the absolute path, and no other class text is added

### Requirement: Worker toolsets [S1, S7, W2, W6, W8]
Every worker but a `code` or `simple` one MUST be offered only the read-only tools (`find_dir`, `list_dir`, `read_file`). No worker MUST be offered `run_in_pane`, an orchestration tool or a user server tool. [S7] `code` workers MUST be offered the read-only tools and the coding-tools write tools (`create_dir`, `write_file`, `edit_file`, `propose_batch`). [W8] `simple` workers MUST be offered the same, within the limits of their role. [W6] `code` and `simple` workers MAY also be offered `run_command` when Exec is on (see Worker commands); no other worker MUST be. [W2] The write tools are offered only when Write is on and `run_command` only when Exec is on (Menu ceiling for worker toolsets); the menu never widens a role. Their writes MUST be staged and reach the disk only through the user's batch review, labelled with the worker; no scope and no approval mode (`run` included) pre-approves a write. A write MUST be staged only inside the run's read scope: one outside it, into a path the secret rules guard or outside the chat's root MUST be refused with the refusal text of a plain chat write, and is never asked. Staging inside the scope by a `code` worker MUST NOT ask (an `edit_file` included, on a cloud engine: reads inside the scope run unasked). [W9] A `simple` worker's staging inside the scope asks only for a write outside its brief's files and for a file over the line limit.
(Previously: only `code` had write tools, no worker could be offered `run_command`, the menu did not limit workers, and the review time counted against the deadline)

[S7] The worker stages in its pane's batch and proposes it itself with `propose_batch`, waiting in its turn for the user's answer (user decision "El worker propone su lote", 2026-10-29). The ask goes to the orchestrator turn's approver, like a worker's read outside the scope. [W6] The time the worker waits for the user does not count against its deadline; the worker's own time still does. A worker that times out ends `timed_out`, nothing is written, and its staged batch stays as an orphan the user can still review (Ctrl+G) once the pane's turn has ended. A batch a worker staged and never proposed also stays in the pane's batch, as an orphan of the pane, and the user is told ([W1]). [W10] From W10 each write-capable worker has its own batch.

#### Scenario: Read-only subset [auto]
- GIVEN a `review` worker
- WHEN its tool list is built
- THEN it is exactly `find_dir`, `list_dir`, `read_file`

#### Scenario: Writes wait for the batch review [S7, auto]
- GIVEN a `code` worker with a write tool and a run's read scope
- WHEN it calls a write tool inside the scope
- THEN the change is staged with the worker label and nothing is written before the user approves the batch

#### Scenario: The approval mode never approves a worker's batch [S7, auto]
- GIVEN a `code` worker whose batch waits for review, in approval mode `run`
- WHEN the approval mode answers the ask instead of the user
- THEN the batch is denied and nothing is written

#### Scenario: A write outside the scope is refused [S7, auto]
- GIVEN a run whose read scope is `~/dev/app/src` in a chat rooted at `~/dev/app`
- WHEN its `code` worker writes `README.md`, `.env` or `../other/x.rs`
- THEN each is refused with the refusal text of a plain chat write, nothing is asked and nothing is staged

#### Scenario: Review time does not count against the deadline [W6, auto]
- GIVEN a `code` worker whose proposed batch waits for the user longer than its deadline
- WHEN the user then approves
- THEN the worker does not end `timed_out` because of the wait

#### Scenario: A deadline reached while working leaves an orphan [S7, auto]
- GIVEN a `code` worker that staged a batch and kept working until its deadline
- WHEN the deadline passes
- THEN the worker ends `timed_out`, nothing is written and the batch stays as an orphan the user can review

#### Scenario: A code worker with run_command [W6, auto]
- GIVEN Exec on
- WHEN a `code` worker's tool list is built
- THEN it holds the read tools, the write tools and `run_command`, and no `run_in_pane`

### Requirement: Read scope of a run [S1]
A run MUST start with no prompt and no key: its read scope is set at run start, without asking (user decision, 2026-10-29). Its roots default to the chat origin folder. Roots MUST come only from the chat origin (and, later, from the user), never from model or worker text. Every root MUST pass the path policy (`paths::resolve`: the secret roots, `CREDENTIAL_DIRS`, outside `$HOME`); a root that is excluded, outside `$HOME`, `$HOME` itself or a broad scratch folder gives no root, and the reason is kept for display, sanitized (`clean`/`clip`). Refusing roots that are ancestors of secret roots or credential parents (`jail::too_broad`) is to be considered. Reads inside the scope MUST run without asking, for this run only, and the scope expires with the run. The scope MUST apply the path policy deny-lists of agent-tools to every candidate path, checking both the requested path (`~` expanded, relative resolved, `.` and `..` folded) and the resolved real path; a path is inside the scope only if both are inside a root and neither is excluded.

#### Scenario: No prompt at run start [auto]
- GIVEN an orchestrating pane whose chat origin is `~/dev/app`
- WHEN a run starts
- THEN no prompt is shown, no key is awaited, and the run's scope root is `~/dev/app`

#### Scenario: Read inside the scope runs without asking [auto]
- GIVEN a run whose chat origin is `~/dev/app` and a worker reading `~/dev/app/src/a.rs` on a cloud engine in `ask` mode
- WHEN `read_file` is called
- THEN it runs with no approval prompt

#### Scenario: Deny-list wins inside the scope [auto]
- GIVEN a scope `~/dev/app` containing `.env` and a symlink `link` to `~/.ssh/id_ed25519`
- WHEN `read_file` is called on each
- THEN both are refused with the one identical refusal message and no approval is offered, in `ask`, `auto` and `run` alike, for any spelling of the path

#### Scenario: Requested and resolved path both checked [auto]
- GIVEN a scope `~/dev/app` and `~/dev/app/x` is a symlink to `~/other/y`
- WHEN `read_file` is called on `~/dev/app/x`
- THEN the read is outside the scope and follows the approval mode

#### Scenario: Refused origin gives no root [auto]
- GIVEN an orchestrating pane whose chat origin is `$HOME` (or excluded, such as `~/.aws`)
- WHEN a run starts
- THEN the scope has no root, the reason is kept sanitized, and every worker read follows the approval mode

#### Scenario: Scope expires with the run [auto]
- GIVEN a run that ended
- WHEN a worker of it reads inside its old scope, or a new run starts
- THEN the old scope grants nothing, and the new run has its own scope from the chat origin

### Requirement: Reads outside the scope follow the approval mode [S1, S3]
A worker read outside the run's scope MUST follow the pane's approval mode exactly as a chat read does (chat-auto-permissions: `run` lets go only what it lets go for a chat read; otherwise the read asks), never be allowed by a timer and never be silently denied. When it asks, the prompt MUST show the sanitized path and a worker label with kind and engine>model. [S1] With one worker there is one pending prompt. [S3] With several workers the prompts MUST form a FIFO queue, one visible at a time, each labelled `worker n/m <kind> <engine>><model>`; a timer MUST NOT approve. Declining, timing out or cancelling MUST end as `denied` and no file is opened. Cancelling a worker MUST deny its pending and queued asks.

#### Scenario: Outside read asks with a label [auto]
- GIVEN a scope `~/dev/app`, a pane in `ask` mode and a `review` worker on `claude>opus` (cloud) reading `~/dev/other/a.rs`
- WHEN `read_file` is called
- THEN an approval shows the sanitized path and the label with `review` and `claude>opus`, and the file is not opened before `y`

#### Scenario: Outside read follows the mode as a chat read [auto]
- GIVEN a worker read outside the scope and the same read made by the chat in the same pane and mode
- WHEN `read_file` is called in `ask`, `auto` and `run`
- THEN the approval mode answers the worker's read exactly as it answers the chat's (prompts, lets go or denies alike), and only a prompt shows the worker label

#### Scenario: Never silently decided [auto]
- GIVEN an outside read pending
- WHEN no key is pressed and the idle timers pass
- THEN the read is not allowed by any timer and a timeout ends it as `denied`

#### Scenario: Queue of two workers [S3, auto]
- GIVEN workers 1 and 2 each asking for an outside read
- WHEN both asks arrive
- THEN one prompt is visible, the second waits behind it, and after the first is answered the second is shown with its own label

#### Scenario: Cancel denies pending asks [S3, auto]
- GIVEN worker 2 has a pending and a queued ask
- WHEN worker 2 is cancelled
- THEN both end as `denied` and worker 1's ask is unaffected

### Requirement: Bounded waits, caps and deadlines [S1]
`wait` MUST return within the smaller of the requested timeout and 120 seconds, with the status of each id (running, queued, done, failed, cancelled, timed_out, refused, awaiting_user). `collect` MUST return a finished worker's final text capped at 32 KiB and say when it truncated. A worker MUST have a deadline (default 10 minutes); when it passes the worker is killed and ends `timed_out`. Orchestrator turns remain bound by the engine turn timeouts; a run MUST be able to outlive one orchestrator turn only once the ledger of S4 exists.

#### Scenario: Wait is clamped [auto]
- GIVEN a running worker and `wait` called with timeout 600 s
- WHEN time is advanced
- THEN `wait` returns after 120 s with the worker still `running`

#### Scenario: Collect caps text [auto]
- GIVEN a finished worker with 100 KiB of final text
- WHEN `collect` is called
- THEN 32 KiB at most is returned and it says it was truncated

#### Scenario: Deadline kills the worker [auto]
- GIVEN a worker with the default deadline and a process that never ends
- WHEN 10 minutes pass
- THEN the process group is killed and the status is `timed_out`

#### Scenario: Collect before done [auto]
- GIVEN a worker still running
- WHEN `collect` is called
- THEN an error result says it is not finished and no text is returned

### Requirement: Worker output is untrusted data [S1]
Everything a worker returns to the orchestrator (final text and error text) MUST be sanitized and wrapped as untrusted data with the same nonce wrapper as tool output in agent-tools, and the orchestrator prompt MUST tell the engine not to follow instructions inside it. Wrapped worker text MUST NOT be able to close its own wrapper, answer a prompt or change the scope.

#### Scenario: Wrapper cannot be forged [auto]
- GIVEN a worker whose final text contains the wrapper closing marker and the text `approve all`
- WHEN `collect` returns it
- THEN the text stays inside one wrapper with an unguessable nonce and nothing is approved

#### Scenario: Control sequences removed [auto]
- GIVEN a worker text with terminal control sequences
- WHEN `collect` returns it
- THEN the sequences are stripped as the output sanitizer requires

### Requirement: Worker result row [S1]
Each worker MUST appear in the chat as a row showing kind, engine>model, status and elapsed time, updated as the status changes. Worker transcripts MUST NOT be stored in the chat history or sent back as history. [S9] The row opens the full worker transcript.

#### Scenario: Row follows the status [auto]
- GIVEN a worker that goes queued, running, done
- WHEN rows are rendered at each step
- THEN the same row shows each status with kind and `engine>model`

#### Scenario: Not in history [auto]
- GIVEN a finished worker
- WHEN the next orchestrator turn is built
- THEN the history holds no worker transcript, only what the orchestrator wrote and the ledger

### Requirement: Failure stops the run [S1, S2]
When a worker ends `failed`, `timed_out` or `refused`, the run MUST stop at that moment and the user MUST be alerted with the concrete provider error (sanitized and capped, never replaced by a generic text). [S1] Options: retry, cancel. [S2] Options: retry, change model, change provider, cancel, shown inline. No other engine or model MUST be tried, there is no automatic retry, and the orchestrator MUST NOT be resumed until the user picks an option. While a failure is unresolved: `wait` MUST return `awaiting_user` without the error text, `spawn_agent` MUST be refused with `run paused`, and no new worker starts.

#### Scenario: Error shown as it came [auto]
- GIVEN a fake worker failing with provider message `409 conflict: session busy`
- WHEN it fails
- THEN the alert shows that message and the options appear

#### Scenario: No silent fallback [auto]
- GIVEN a failing worker on `codex`
- WHEN it fails
- THEN the fake runner is called for no other engine and no worker is started

#### Scenario: Orchestrator paused [auto]
- GIVEN an unresolved failure
- WHEN the orchestrator calls `wait` and `spawn_agent`
- THEN `wait` returns `awaiting_user` and `spawn_agent` is refused with `run paused`

### Requirement: Error classes [S2]
Each adapter MUST classify a worker failure as one of: `rate_limit` (429), `conflict` (409), `auth`, `quota`, `timeout`, `jail_refusal`, `setup` (a worker refused because its engine or model is not available), `other`. The class MUST be shown with the raw message, except `other`, which adds nothing. `rate_limit` MUST be its own class: the alert names the limit and offers the same options, and lazarobox MUST NOT retry on its own.

#### Scenario: 429 is its own class [auto]
- GIVEN a worker whose provider answers 429
- WHEN the failure is classified
- THEN the class is `rate_limit`, the alert shows the provider message, and no retry is made without the user

#### Scenario: Class table [auto]
- GIVEN decoder fixtures for 409, auth failure, quota exhausted, a timeout, a jail exit 71 and an unknown error
- WHEN each is classified
- THEN they give `conflict`, `auth`, `quota`, `timeout`, `jail_refusal`, `setup`, `other`

### Requirement: Failure options and relaunch [S2]
`retry` MUST relaunch the same `WorkerSpec` from scratch (CLI turns are stateless) with the engine and model of the failed attempt, and its prompt MUST be followed by the failed attempt's error, sanitized and capped as in the alert and wrapped as untrusted data. `change model` MUST open the menu picker for that task at the model level of its engine; `change provider` MUST open it at the engine level. A confirmed choice is a *run-scoped override*, not a menu change: it MUST apply to the failed worker's task for the rest of this run only, MUST NOT change the menu draft or the saved configuration and MUST NOT write the config file. The worker MUST be relaunched with the override, and every later worker of the same task in the same run MUST also use it; workers of other tasks are unaffected. The override MUST end with the run (finished or cancelled) and a new run starts from the menu values. The run state and the rows of the affected workers MUST show that an override is active, with the `engine>model` in use. Leaving the picker without a choice MUST return to the alert with the same options and nothing is relaunched. `cancel` MUST end the run (see Cancel). The relaunched worker keeps its worker id and its row. An override is still subject to Refusal on an unavailable engine.

#### Scenario: Retry relaunches the same spec [auto]
- GIVEN a failed `review` worker with prompt P on `codex` whose error was E
- WHEN retry is chosen
- THEN a new attempt starts on codex with the same worker id, and its prompt is P followed by E wrapped as untrusted data

#### Scenario: Change provider relaunches on the new one [auto]
- GIVEN a failed `review` worker on `codex`
- WHEN change provider is chosen and `claude` / `sonnet` is picked
- THEN the run's `review` override is `claude` / `sonnet`, the worker relaunches on claude and the menu `review` assignment is unchanged

#### Scenario: Override reaches later workers of the task [auto]
- GIVEN an override `review` = `claude` / `sonnet` created by a failure alert, and the run resumed
- WHEN the orchestrator spawns another `review` worker and then a `code` worker
- THEN the `review` worker runs on claude / `sonnet` and the `code` worker runs on its menu assignment

#### Scenario: Override shown [auto, TestBackend + insta]
- GIVEN an active override for `review`
- WHEN the run state and the `review` worker rows are rendered
- THEN the run state names the override and each affected row shows `claude>sonnet` marked as an override

#### Scenario: Override ends with the run [auto]
- GIVEN a run that used an override for `review` and has ended
- WHEN the menu value of `review` is read and a new run spawns a `review` worker
- THEN the menu value is unchanged, the config file was never written for it and the new worker uses the menu assignment

#### Scenario: Override is not a menu save [auto]
- GIVEN the picker opened from an alert and a choice confirmed
- WHEN the menu is opened afterwards, during the same run
- THEN its draft shows the menu values, not the override, and nothing is pending to save

#### Scenario: Menu saved during the run [auto]
- GIVEN an active override for `review` and the user saves a different `review` assignment in the menu
- WHEN a later `review` worker is spawned in the same run
- THEN it uses the override until the run ends

#### Scenario: Override engine unavailable [auto]
- GIVEN the user picks an engine that is not installed through `change provider`
- WHEN the worker is relaunched
- THEN it ends `refused`, the alert is shown again and no other engine is contacted

#### Scenario: Picker abandoned [auto]
- GIVEN the picker opened from the alert
- WHEN Esc is pressed
- THEN the alert is shown again, no override is created and nothing starts

### Requirement: Failure with other workers running [S3]
When one worker fails during a parallel run, no new worker MUST start, running siblings MUST be allowed to finish their current turn, queued workers MUST stay queued, and the orchestrator MUST NOT be resumed until the user picks an option. Sibling results are kept for `collect` after the decision. `cancel` ends the whole run: siblings are killed and their pending asks denied.

#### Scenario: Siblings finish [auto]
- GIVEN workers 1 and 2 running and worker 2 fails
- WHEN worker 1 completes
- THEN worker 1 is `done`, nothing new starts, and the orchestrator remains paused

#### Scenario: Cancel after failure [auto]
- GIVEN worker 2 failed and worker 1 is running
- WHEN cancel is chosen
- THEN worker 1 is killed and the run ends `cancelled`

### Requirement: Multiple workers and limits [S3]
`max_parallel` (default 2, range 1 to 4) MUST bound the workers running at once; further spawns MUST be `queued` and start in FIFO order as slots free. Per-run limits (`max_workers`, `run_timeout_mins`; see configuration) MUST bound the number of workers and the wall-clock time; a spawn over a limit MUST be refused with the limit named, and an elapsed wall clock MUST stop the run and kill running workers. Session keys, sockets, tokens, scopes and approvals MUST be per worker, so one worker's state never reaches another's. [S1] Until S3, at most one worker runs and any further spawn is queued.

#### Scenario: Parallel limit [auto]
- GIVEN `max_parallel = 2` and three spawns
- WHEN they are spawned
- THEN two run, one is `queued`, and it starts when one ends

#### Scenario: Worker cap [auto]
- GIVEN the per-run worker limit reached
- WHEN `spawn_agent` is called
- THEN it is refused naming the limit

#### Scenario: Wall clock [auto]
- GIVEN the run wall-clock limit with a worker running
- WHEN the limit passes in paused time
- THEN the worker is killed and the run ends with the limit named

#### Scenario: Isolated sessions [auto]
- GIVEN two workers
- WHEN worker 1 presents worker 2's token or socket
- THEN the call is refused

### Requirement: Cancel [S1, S3]
Ctrl+C in the chat pane MUST cancel the orchestrator turn and every worker of the run, kill their process groups and deny their pending asks. The `cancel` tool MUST cancel one worker. [S3] A key MUST cancel one selected worker or all. Quitting lazarobox MUST kill every worker process. A cancelled worker ends `cancelled`, and cancelling an already finished worker is a no-op result.

#### Scenario: Ctrl+C cancels all [auto]
- GIVEN an orchestrator turn and a running worker
- WHEN Ctrl+C is pressed
- THEN both are cancelled, the worker's process group is killed and the status is `cancelled`

#### Scenario: Cancel one by tool [auto]
- GIVEN workers 1 and 2 running
- WHEN `cancel` is called for worker 1
- THEN worker 1 ends `cancelled` and worker 2 keeps running

#### Scenario: Cancel after done [auto]
- GIVEN a finished worker
- WHEN `cancel` is called
- THEN the result says it already ended and its status is unchanged

### Requirement: A run ends by itself [S1]
A run MUST end by itself, as a cancel does but with nothing left to stop, once its orchestrator turn has ended and none of its workers is queued or running (user decision, 2026-10-29): the pane then has no active run and Ctrl+O works again with no key pressed. A run halted on a failure MUST NOT end by itself: the alert still needs an answer, and after a retry the run ends once the relaunched worker is done. Ctrl+C, the alert's cancel keys and a new run of the pane end it as before.

#### Scenario: Turn and workers done [auto]
- GIVEN an orchestrator turn whose workers all finished
- WHEN the turn ends
- THEN the run ends once and Ctrl+O toggles the mode again

#### Scenario: A worker still runs [auto]
- GIVEN an orchestrator turn that ended with a worker still running
- WHEN the worker finishes
- THEN the run ends then, not before

#### Scenario: Halted run waits [auto]
- GIVEN an orchestrator turn that ended with the run halted on a failure
- WHEN time passes
- THEN the run stays until the alert is answered

### Requirement: ODD flow [S5, W6, W8]
The run MUST follow the ODD steps in order: `authorize`, `explore`, `resolve`, `classify`, `track` (large goals only), `implement`, `close`. A step MUST NOT be recorded as done without a handoff for it.
- `authorize`: a goal that asks to investigate, explain, review, compare or plan is read-only, and no `code` worker, and no `simple` worker whose brief lists files to write, MUST be spawned for it.
- `resolve`: the orchestrator MAY ask the user one question, then MUST wait for the answer; it MUST NOT ask a second question in the same turn.
- `classify`: the goal is `small` when it is understood, touches no high-risk item and could be finished from the goal and the change alone; otherwise `large`. The number of files or tasks MUST NOT decide it.
- `track`: a `large` goal MUST get a feature document (header, `Specs` with numbered `S#` quoting the user's words verbatim, `Tasks` with the linked `S#`, and `Log` whose `L1` is the goal verbatim), saved as a handoff. A `small` goal MUST NOT get one.
- `implement`: each task worker MUST receive the feature document by reference (document, task id, `S#`) and report which `S#` it covered.
- `close`: the run MUST report the verified outcome, every failed, skipped or pending check, and a risk line.
Until S7, `implement` MUST produce only drafts and review findings, and the orchestrator MUST NOT claim code was written or tests passed. Since S7, a `code` or `simple` worker MAY stage changes, and a change is written only when the user approves its batch: the orchestrator MUST NOT claim a file was written unless `collect` lists it `applied`, and MUST call every other change a draft. [W6] A test counts as run or passed only when a collected `run_command` result shows it; otherwise the orchestrator MUST say it was not run and ask the user to run it. S8 enforcement (red evidence before green) stays deferred.
(Previously: `authorize` named `code` only; the orchestrator could never claim tests passed because no worker had `run_command`)

#### Scenario: Read-only goal spawns no code worker [auto]
- GIVEN the goal `explain how the read scope works`
- WHEN the orchestrator calls `spawn_agent` with kind `code`
- THEN it is refused as outside a read-only goal

#### Scenario: Read-only goal also covers simple [W8, auto]
- GIVEN the goal `explain how the read scope works`
- WHEN the orchestrator calls `spawn_agent` with kind `simple` and a brief that lists an output file
- THEN it is refused as outside a read-only goal

#### Scenario: A large goal needs its document [auto]
- GIVEN a run classified `large` with no feature document handoff
- WHEN step `implement` is started
- THEN it is refused naming the missing `track` step

#### Scenario: Specs keep the user's words [auto]
- GIVEN the goal `añade un comando "lazarobox doctor" que diga "todo bien"`
- WHEN the feature document is built
- THEN an `S#` holds `"lazarobox doctor"` and `"todo bien"` byte for byte, and `L1` is the goal verbatim

#### Scenario: Implement is a draft before S7 [auto]
- GIVEN step `implement` with no write tools offered
- WHEN it completes
- THEN its result is labelled a draft and no file in the workspace changed

#### Scenario: A test is not claimed without a result [W6, auto]
- GIVEN a run whose workers collected no `run_command` result
- WHEN `close` reports the outcome
- THEN every test is reported as not run

### Requirement: ODD test-first evidence [S8] (not implemented; deferred to 1.2)
Delta note (user decision, 2026-10-31; amended by this change): this requirement and its two scenarios are still NOT implemented and stay deferred; they are kept, not deleted. The statement that no worker or orchestrator turn is offered `run_command` no longer holds once W6 ships: a `code` or `simple` worker may then run commands after its batch is approved, under the Exec ceiling and with a user ask for each (Worker commands). That is not S8. S8 is the enforcement below (green without red refused), which nothing in this change adds. Until it exists `close` reports tests as run only when a collected command result shows it, and RDD (S7b) reviews by reading and does not depend on evidence.
(Previously: "In 1.1.0 no worker and no orchestrator turn is offered `run_command`"; `close` reported every test as not run; the `orchestrate` prompts forbade claiming any test ran)

Once `run_command` exists, for a task with a runnable deterministic test the run MUST require failing evidence before the implementation is accepted and passing evidence after it. Evidence MUST come from the `run_command` result, never from worker text alone. A task without a runnable test MUST record an explicit exception with its reason, and is then checked by a review worker instead.

#### Scenario: Green without red refused [S8, auto, deferred to 1.2]
- GIVEN an implementation result with passing evidence and no earlier failing evidence
- WHEN the task is closed
- THEN it is refused as missing red evidence

#### Scenario: Exception recorded [S8, auto, deferred to 1.2]
- GIVEN a documentation-only task
- WHEN it is closed without evidence
- THEN its handoff holds the exception and its reason, and the close is accepted

### Requirement: Usage per worker and run [S6]
Where a CLI reports token or cost data, its decoder MUST emit a usage event; per-worker and per-run rows MUST show the sums. Where the CLI reports nothing, the value MUST show as unknown and MUST NOT be estimated.

#### Scenario: Usage summed [auto]
- GIVEN two workers reporting usage events
- WHEN the run row is built
- THEN it shows the sum, and a worker without usage shows `n/a`

### Requirement: Write integration and change ledger [S7]
Each `code` worker MUST keep a ledger of the files it staged (path, size, diff hash), shown in its result as `files changed`, each `applied` or `not applied`. Its writes MUST be staged and reach the disk only when the user approves the batch in the batch review. A path is `applied` only once a batch holding it was approved and written; staging it again makes it `not applied` until a later batch applies it. The ledger goes first in the `collect` result, inside the untrusted-data wrapper (its paths are the worker's names), so the text cap never hides it. The path is relative to the chat's root, the size is the staged text in bytes (`folder` for a folder) and the diff hash is a 64-bit FNV-1a of the replaced text and the new text, which tells two versions apart and is no security boundary.

#### Scenario: Files changed listed [S7, auto]
- GIVEN a `code` worker that staged two files
- WHEN `collect` returns
- THEN the result lists both files with sizes, and neither is on disk before the user approves the batch

### Requirement: Risk assessment of a change [S7b]
Before a `code` worker's staged change reaches the user's batch review, the run MUST assess its risk as `passive`, `medium` or `high`. Deterministic rules in the core MUST set the floor: a change to build files, CI, dependencies or lockfiles, config formats, or a path the secret rules guard is `high`. The orchestrator MAY raise the tier with a one-line reason and MUST NOT lower it. A failed assessment MUST count as `high`.

#### Scenario: A lockfile is high [S7b, auto]
- GIVEN a staged change to `Cargo.lock`
- WHEN it is assessed
- THEN the tier is `high` whatever the orchestrator said

#### Scenario: The orchestrator cannot lower [S7b, auto]
- GIVEN a change the rules mark `high` and an orchestrator verdict `passive`
- WHEN it is assessed
- THEN the tier stays `high`

### Requirement: Receipt-driven review [S7b, W7]
A `high` change, or any change the user asks to review, MUST ask the user for consent for that change, inline, before any review starts, except a change by a cheap author, which is reviewed with no consent prompt (Mandatory review for a cheap author). A declined change MUST go on to the batch review unreviewed, marked `not reviewed`. On consent (or for a cheap author):
- The orchestrator MUST pick only the lenses the change calls for, from `risk`, `resilience`, `readability` and `reliability`, with a one-line reason.
- Each lens MUST run as a read-only `review` worker over the staged change, wrapped as untrusted data.
- Only a severe finding the change caused MUST block. A finding that already held before the change is a follow-up.
- Inferential blockers MUST go to one refuter worker before they count.
- At most one bounded correction MUST run, by a `code` worker, scoped to the blockers that survived; its result is validated against those blockers only.
- The review MUST end in a receipt handoff (tier, lenses, findings, correction, outcome), shown in the batch review next to the diff.
A receipt MUST NOT approve the batch, and the orchestrator MUST NOT consent or approve anything.
(Previously: consent was asked for every reviewed change, whoever the author)

#### Scenario: Consent first [S7b, auto]
- GIVEN a `high` change by a non-cheap author
- WHEN the review is due
- THEN the user is asked for consent and no review worker runs before the answer

#### Scenario: Declined goes on unreviewed [S7b, auto]
- GIVEN a `high` change by a non-cheap author and a declined consent
- WHEN the batch review opens
- THEN it shows the change marked `not reviewed` and no review worker ran

#### Scenario: A cheap author needs no consent [W7, auto]
- GIVEN a `high` change by a local-engine worker
- WHEN the review is due
- THEN no consent prompt is shown and a review runs

#### Scenario: One correction at most [S7b, auto]
- GIVEN a review whose correction still leaves a blocker
- WHEN the correction result is validated
- THEN no second correction runs, and the receipt names the open blocker for the user to decide

#### Scenario: A receipt never approves [S7b, auto]
- GIVEN a receipt with no blockers
- WHEN the batch review opens
- THEN the batch still waits for the user's `y`

### Requirement: Run view and persistence [S9]
A key MUST open a full-pane transcript of a worker. Runs MUST be stored locally and, after a restart, shown as interrupted with a resume choice; a resumed run relaunches unfinished workers from scratch only on the user's choice, and sets the read scope again from the chat folder without asking (user decision, 2026-10-29, Read scope of a run): the old run's grant is never reused.

#### Scenario: The transcript of a worker [S9, auto]
- GIVEN a run with a running worker that streamed text and called a tool
- WHEN the worker is selected and the transcript key is pressed
- THEN a full-pane read-only view shows its output and tool rows, cleaned of control characters, bounded to the last 128 KiB, following new output until scrolled

#### Scenario: A worker prompt over its Ollama window [#557, auto]
- GIVEN a worker on Ollama whose system prompt, prompt and offered tools are estimated over the `num_ctx` its turn is stamped with
- WHEN its turn starts
- THEN its transcript begins with the system row `the worker prompt and setup are about <tokens> tokens, over the <window>-token window: the server may cut them`, and the turn still runs; a worker that fits, or whose window is unknown, gets no row

#### Scenario: Restart shows an interrupted run [S9, auto]
- GIVEN a stored run with one running worker when lazarobox quit
- WHEN lazarobox starts again
- THEN the run shows as interrupted, nothing runs until the user chooses, and a resumed run sets its scope again from the chat folder without asking

#### Scenario: Resume is offered only in the run's folder [S9, auto]
- GIVEN an interrupted run stored with folder `~/a` (or with no folder, from before folders were kept)
- WHEN a chat in `~/b` resumes or discards the newest interrupted run
- THEN it is refused with a notice, its goal is in no turn, and the startup line only counts runs of other folders

Note (S9 9.1c, what enforces "unfinished workers relaunch only on the user's choice"). Resume starts a NEW run whose seed message names the unfinished workers and tells the orchestrator to relaunch none until the user says so. That instruction is the only thing tying a relaunch to the user's word: it is a model instruction, not a hard gate. Traced spawn path (`Run::spawn`): it refuses only a paused or ended run, a `code` worker in a read-only step, and more than `max_workers`; by user decision (chat-pane, "Worker starts without a prompt") a worker starts with no approval, so nothing in code blocks a spawn after resume. Every worker is announced by its row, and what a worker can do is bounded by the existing per-tool gates: reads outside the run's scope follow the pane approval mode, a `code` worker's writes are staged for the batch review, and the jail and the `max_parallel`/`max_workers`/turn/time limits apply. The read scope is not asked (user decision 2026-10-29): it is set again for the new run from the chat folder (Read scope of a run), and nothing of the old grant is stored or reused. Closing the resumed run (D6): the run that the user's reply to the resume turn starts closes the interrupted one with its first worker, so a restart does not name it again; the link lasts that one reply. While the pane waits for the reply the run is off the list (`r r` says it waits) and the pane says "waits for your reply"; a toggle of ORCH, a reply that starts no worker, a second turn or closing the pane gives it back to the list, said. A resume turn that failed or was cancelled leaves the run offered with no link. A reply that failed or was cancelled keeps the wait for a retry; any normal end of a turn that was the reply gives the run back to the list, still linked so a worker that arrives after that end closes it.

### Requirement: Final documentation [S10]
The README MUST be updated once, in the final slice, with: how to enable and use the orchestrator (the `Orchestrator` menu gate and the per-pane Ctrl+O toggle), the Orchestrate menu row, the provider-terms statement (providers recommend API keys for automation, consumer logins carry risk, no key required) and the Ollama "not recommended" note. Delta (2026-10-31): it also states the keys, the config keys, the memory and privacy gate, the persistence of runs and the known limits, among them that workers cannot run commands or tests in 1.1.0 (S8 deferred). No earlier slice MUST edit the README for this feature. (This change, W6) From the release that ships W6 the README MUST instead state that a worker can run a command only with Exec on, only after its batch is approved, and only after the user approves each command; S8 enforcement stays deferred.
(Previously: the known-limits statement said workers cannot run commands or tests)

#### Scenario: README written once [manual]
- GIVEN the final slice is merged
- WHEN the README is read
- THEN it documents the orchestrator, the menu row and the provider note, and the earlier slices' diffs contain no README change for this feature

### Requirement: Build a project from the chat, end to end [S10]
With coding-tools M2 built and S1..S7b and S9 merged, a user MUST be able, from one lazarobox chat and without opening an IDE, to take a small programming task through plan, implementation and review: the orchestrator drives the ODD steps, a `code` worker stages files (applied only through the user's batch review and listed in `files changed`), a high-risk change gets a receipt-driven review, and handoffs record each step. A test counts as run only when a collected `run_command` result shows it (W6, see Worker commands); otherwise the orchestrator says it was not run, never claims it passed, and asks the user to run it. The legs "failing test" and "passing test", with red evidence before and green evidence after from `run_command`, belong to S8 and are deferred to 1.2 (user decision, 2026-10-31). If the write tools are missing the requirement is not accepted and the feature is described as drafts only.

#### Scenario: Small task from the chat [S10, manual]
- GIVEN the gate on, a chat pane with ORCH on, a scratch project, and `orchestrate`, `code` and `review` assigned to available engines
- WHEN the user asks for a function with tests in the chat and approves the batch
- THEN the files (function and tests) are written only after the batch review, a review result is returned, handoffs exist for each ODD step, and the user never left the chat
- AND the orchestrator says a test ran only if a collected `run_command` result shows it (Exec on, each command approved by the user); otherwise it says it did not run the tests and the user runs them; the recorded failing and passing runs of the original scenario are S8 and deferred to 1.2

#### Scenario: Missing dependency [S10, auto]
- GIVEN no write tool or `run_command` tool is registered (a worker has `run_command` only with Exec on, see Worker toolsets)
- WHEN the end-to-end flow is requested
- THEN the orchestrator states it can only produce drafts and does not claim code was written or tests passed
- Covered by `the_orchestrate_prompts_say_only_an_applied_file_was_written` and `the_orchestrate_prompts_hold_the_coordinator_rules` (src/core/ai/chat/prompts/tests.rs); the toolsets themselves are pinned by the `Worker toolsets` tests.

### Requirement: No content in logs [S1]
Worker prompts, worker output, provider error text and paths MUST NOT be written to logs or error values that outlive the screen; logs may carry ids, kinds, statuses and error classes.

#### Scenario: Log hygiene [auto]
- GIVEN a run with a failing worker
- WHEN logs are captured
- THEN they hold the class and status and none of the prompt, output or error message text

### Requirement: Resume and discard ask the store on demand [S9]
`Prefix r r` (resume) and `Prefix r d` (discard) MUST NOT report `No interrupted run to resume or discard` while the run store has not answered. When the interrupted runs are not known yet (for example ORCH was never turned on since the start), the key MUST ask the store on the spot, off the UI thread, and show the notice `Looking for interrupted runs...`; the answer MUST complete the key that waited, including `Turn ORCH on in this pane first`. A failed read, or one with no answer within 10 s, MUST be said and retried by the next press, never reported as `No interrupted run`. The key MUST redraw the screen after its answer, also when the answer comes at once, so the notice and the clearing of the `RUN` key hint are shown. (Since 1.2.1.)

#### Scenario: The store is asked when the runs are unknown [auto]
- GIVEN ORCH never turned on since the start and an interrupted run in the store
- WHEN `Prefix r r` is pressed
- THEN the notice `Looking for interrupted runs...` shows, the store is asked off the UI thread, and the answer completes the resume

#### Scenario: A silent store is not an empty one [auto]
- GIVEN a store that does not answer within 10 s
- WHEN `Prefix r d` is pressed
- THEN the failure is said, `No interrupted run to resume or discard` is not shown, and the next press asks again

#### Scenario: The key always redraws [auto]
- GIVEN the store already answered that there is no interrupted run
- WHEN `Prefix r r` is pressed
- THEN the notice `No interrupted run to resume or discard` is shown and the screen is redrawn, so the `RUN` key hint is cleared

### Requirement: An undelegated coordinator reply is disclosed [W1]
When an orchestrator turn ends with a text answer and the run has spawned no worker, the pane MUST add a transcript notice saying, in neutral words and without accusing the model, that no worker ran, that nothing was written, and which key asks the coordinator to delegate. No automatic retry MUST follow, and no model-free classifier of "change request" versus "question" MUST decide whether the notice is shown. The notice MUST be shown after every such turn, including a legitimate answer to a question. A turn in which at least one worker was spawned MUST NOT get this notice.

#### Scenario: Text-only turn gets the notice [W1, auto]
- GIVEN an orchestrating pane and an orchestrator turn that ends with text and no `spawn_agent` call
- WHEN the turn ends
- THEN the transcript gains a notice stating no worker ran, nothing was written, and the delegate key
- AND no further turn starts by itself

#### Scenario: A legitimate answer gets the same neutral notice [W1, auto]
- GIVEN the user asked a question and the coordinator answered in text with no worker
- WHEN the turn ends
- THEN the same notice is shown, and its text contains no accusation and no claim that the answer was wrong

#### Scenario: A turn with a worker gets none [W1, auto]
- GIVEN an orchestrator turn that spawned one worker
- WHEN the turn ends
- THEN no undelegated-reply notice is shown

### Requirement: Silent worker outcomes are disclosed [W1]
The pane MUST show a transcript notice, naming the worker id, kind and `engine>model`, in each of these cases: (a) a `code` or `simple` worker ended `done` with an empty change ledger; (b) a batch a worker staged was never proposed (it stays an orphan of the pane); (c) a spawn was refused for any reason (the reason is the redacted, capped line the coordinator receives); (d) a turn of the coordinator or of a worker was degraded to no tools, saying why. A notice MUST NOT be sent to a model as history.

#### Scenario: Empty ledger [W1, auto]
- GIVEN a `code` worker that ended `done` and staged nothing
- WHEN its result is recorded
- THEN a notice names the worker and says it changed no file

#### Scenario: Staged and never proposed [W1, auto]
- GIVEN a worker that staged two files and ended without `propose_batch`
- WHEN it ends
- THEN a notice says its batch was staged and never proposed and that the pane holds it as an orphan

#### Scenario: Refused spawn is seen by the user [W1, auto]
- GIVEN a `spawn_agent` call refused for any reason
- WHEN the refusal is returned to the coordinator
- THEN the transcript also shows a notice with the same reason, redacted and capped

#### Scenario: Turn degraded to no tools [W1, auto]
- GIVEN a turn whose tools could not be offered (no tooling, tools not on, or no locality)
- WHEN the turn starts
- THEN a notice says which turn ran without tools and the reason, and the turn still runs as it does today

### Requirement: Spawn refusals that create no worker [W2, W8]
A refusal made by the app before a worker exists (a missing brief field, Write off for a worker that must write, a read-only goal, a step-state gate, a limit) MUST be returned to the coordinator as an error result naming the cause, MUST create no worker, MUST NOT start a process, and MUST NOT pause the run or raise a failure alert. It is not a worker `failed`, `timed_out` or `refused` status (see Failure stops the run, which applies to a worker that exists). The refusal is shown to the user (see Silent worker outcomes are disclosed). An engine or model that is unavailable stays a worker `refused` as before (Refusal on an unavailable engine).

#### Scenario: A refusal before a worker does not pause the run [W2, auto]
- GIVEN a `simple` spawn refused for a missing brief field
- WHEN the coordinator then calls `wait` and `spawn_agent` again
- THEN `wait` does not return `awaiting_user`, `spawn_agent` is not refused with `run paused`, and no failure alert is open

### Requirement: Menu ceiling for worker toolsets [W2]
The Write and Exec toggles of the AI menu MUST be a per-role ceiling for workers: a worker is offered write tools only when its role includes them and Write is on, and `run_command` only when its role includes it and Exec is on. The menu MUST NOT widen a toolset beyond what its role allows (a `review`, `summarize`, `general` or `chat` worker stays read-only with Write and Exec on). The coordinator keeps the read tools and the orchestration tools whatever the toggles say. A toggle change MUST apply from the next spawn and MUST NOT change a worker already running.

#### Scenario: Write off removes write tools [W2, auto]
- GIVEN Write is off and a `code` worker is spawned for a brief with no file to write
- WHEN its tool list is built
- THEN it contains no write tool

#### Scenario: The menu never widens [W2, auto]
- GIVEN Write and Exec are on
- WHEN a `review` worker's tool list is built
- THEN it is exactly `find_dir`, `list_dir`, `read_file`

#### Scenario: Exec off means no run_command [W2, auto]
- GIVEN Exec is off
- WHEN the tool list of any worker is built
- THEN `run_command` is absent and a forged call is refused with an error result

### Requirement: A worker that must write is refused when Write is off [W2]
With Write off, a spawn of a worker that must write MUST be refused (see Spawn refusals that create no worker) with a notice that names the toggle (`Write`), says that the worker was not started, and tells the user how to turn Write on from the chat. A `code` worker always must write. A `simple` worker must write only if its brief lists files to write (see The brief). Turning Write on from the chat MUST be possible only by a user action: no model output, tool result, worker text or approval mode MUST be able to turn it on. Turning it on MUST apply to spawns from then on.

#### Scenario: Code worker refused with Write off [W2, auto]
- GIVEN Write is off
- WHEN the coordinator spawns a `code` worker
- THEN the spawn is refused, no worker exists, and the notice names `Write` and the chat step to turn it on

#### Scenario: A model cannot turn Write on [W2, auto]
- GIVEN Write is off and an orchestrator or worker text that asks to enable it
- WHEN the text and all tool results are processed
- THEN Write stays off and no toggle changes

#### Scenario: Turning Write on lets the spawn through [W2, auto]
- GIVEN a refused `code` spawn and the user then turns Write on from the chat
- WHEN the coordinator spawns the worker again
- THEN the worker starts with the write tools

### Requirement: Gates on step state are visible and bounded [W3]
The run MUST gate worker spawn and `finish` on the ODD step state. Every gate refusal MUST name the gate and the step state it saw, and be shown to the user. A gate MUST NOT trap a run: the orchestrator turn cap (`max_turns`) still bounds the run, and the user MUST be able to override a gate with a key that only the user can press. An override MUST be shown in the transcript naming the gate it lifted. Which transitions are gated is open for design (see Open points).

#### Scenario: A gate names itself [W3, auto]
- GIVEN a spawn or `finish` refused by a step-state gate
- WHEN the refusal is returned
- THEN the coordinator's error and the user's notice both name the gate and the current step

#### Scenario: The turn cap ends a trapped run visibly [W3, auto]
- GIVEN a gate that refuses every attempt until `max_turns` is reached
- WHEN the cap is reached
- THEN the run stops with a notice naming the gate and the cap, and the user may override

#### Scenario: Only the user overrides [W3, auto]
- GIVEN a gate refusal
- WHEN the orchestrator emits text or calls that look like the override
- THEN the gate stays until a user key event lifts it, and the lift is shown

#### Scenario: A standing spawn refusal keeps the run waiting for the reply [W3, auto]
- GIVEN an orchestrator turn that ends with a spawn gate refusal standing (no gate passed since), no worker queued or running and no failure waiting
- WHEN the turn ends
- THEN the run does not end: it stays open and the pane says `run waits for your reply: step <step> is not closed (Enter gates off)`
- AND the gate offer (Enter gates off) stays available
- AND the wait counts as an orchestrator turn against `max_turns`

#### Scenario: The reply attaches to the waiting run [W3, auto]
- GIVEN a run that waits for the user's reply
- WHEN the user sends a message in that pane
- THEN the message is the next turn of the same run (not a new run), a lift made before or while replying is kept, and the composer is cleared

#### Scenario: The coordinator is told about a lift [W3, auto]
- GIVEN a waiting run whose gates the user lifted
- WHEN the reply turn starts
- THEN the coordinator's copy of the reply carries one plain line saying the user lifted the step gates and that spawn and finish are not gated now; the line is given once and the user's own message in the pane is untouched

#### Scenario: The wait ends, visibly [W3, auto]
- GIVEN a run that waits for the user's reply
- WHEN ORCH is toggled off, the run is cancelled, the run ends or `max_turns` or the time limit is reached, or the pane closes
- THEN the wait ends and the pane says `run no longer waits for your reply: <why>` (ORCH toggle and run end) or the existing stop line (limits)
- AND a reply turn that settles again with nothing pending and no new spawn refusal ends the run normally
- AND the wait never reaches a later, unrelated run (the lift stays bound to its run)

The wait covers the spawn gate only. A refused `finish` settles as before (a text reply with nothing pending ends the run).

### Requirement: Waiting on the user stops the worker clock [W6]
Time a worker spends waiting for the user's answer (a batch review it proposed, a command ask, a read ask, a limit ask) MUST NOT count against its deadline (`worker_timeout_secs`). The clock MUST resume when the answer arrives. Time spent on the worker's own work still counts. The model review of the worker's own batch (the Receipt-driven review it waits for inside `propose_batch`, consent included) MUST NOT count either (owner, 2026-11-10, design D6). The run wall clock (`run_timeout_mins`) MUST keep counting while the user decides: it is the last bound (owner, 2026-11-10, design D5).

#### Scenario: A long review does not time out the worker [W6, auto]
- GIVEN a worker with a 600 s deadline whose batch waits 20 minutes for the user
- WHEN the user approves and the worker then runs its tests
- THEN the worker is not `timed_out` and the time it waited was not counted

#### Scenario: Work time still counts [W6, auto]
- GIVEN a worker that runs for the whole deadline without waiting on the user
- WHEN the deadline passes
- THEN it ends `timed_out`

#### Scenario: A model review does not time out its author [W7, auto]
- GIVEN a worker with a 600 s deadline whose batch is under a model review for longer than that
- WHEN the review ends and the user approves the batch
- THEN the worker is not `timed_out` because of the review

#### Scenario: The run wall clock still counts [W5, auto]
- GIVEN a run whose wall clock ends while a worker waits for the user
- WHEN the wall clock passes
- THEN the run ends as expired, as it does today, and the wait did not stop it

### Requirement: Worker commands [W6]
A worker whose role includes `run_command` MUST be offered it only when Exec is on. Its command MUST run only after the worker's own batch was approved and applied (no staged overlay; tests run against the disk), and a call made before that MUST be refused with the existing `propose them first` text, visible to the worker. Every command MUST ask the user, labelled with the worker, in every approval mode; the allow-list, jail, refusals and output rules of command-runner apply unchanged. The command output MUST reach the coordinator as untrusted data through the worker's result. The coordinator MUST NOT be offered `run_command`.

#### Scenario: Command asks with the worker label [W6, auto]
- GIVEN Exec on, an approved batch and a worker calling `cargo test`
- WHEN the call is made, in `ask` and in `run` mode
- THEN a prompt labelled `worker n/m <kind> <engine>><model>` shows the program, args and cwd, and nothing starts before `y`

#### Scenario: Before the batch is applied [W6, auto]
- GIVEN a worker whose batch is staged and not yet applied
- WHEN it calls `run_command`
- THEN the call is refused with `propose them first` and no prompt opens

#### Scenario: Output reaches the coordinator [W6, auto]
- GIVEN an approved command that printed a failing test
- WHEN the coordinator collects the worker
- THEN the cleaned, capped output is inside the untrusted-data wrapper

### Requirement: Mandatory review for a cheap author, optional for a strong one [W7]
A change staged by a cheap author MUST be reviewed by a model before it reaches the user's batch review, with no consent prompt, using the RDD pipeline (no new agent type). A change staged by any other author MUST be reviewed only when Receipt-driven review requires it (a `high` change, with consent) or the user asks. The reviewer MUST be the model the user assigned to the Review row (effective assignment, so an unassigned row resolves to `general`, shown visibly). lazarobox MUST NOT pick another reviewer. When a review runs and its engine and model equal the author's, a visible warning MUST say so. When the reviewer is a local engine, a visible notice MUST state the limits of local models as reviewers. When the author is local and the reviewer is a cloud engine, a visible notice MUST say that the staged diff is sent to that engine and model. If the review cannot run (reviewer unavailable), it ends as a `refused` worker and the run stops as any worker failure; the change MUST NOT be silently shown as reviewed. The receipt MUST be shown in the batch review, and a receipt never approves.

#### Scenario: Cheap author, no consent [W7, auto]
- GIVEN a `simple` worker on `ollama>llama3:8b` that staged a `passive` change
- WHEN its batch is due for review
- THEN a review worker runs on the Review row with no consent prompt, and the batch review shows the receipt

#### Scenario: Strong author, optional [W7, auto]
- GIVEN a `code` worker on `claude>sonnet` that staged a `passive` change
- WHEN its batch is due for review and the user did not ask for a review
- THEN no review worker runs

#### Scenario: Same model warns [W7, auto]
- GIVEN a review that runs on the same engine and model as the author
- WHEN the review starts
- THEN a visible warning says author and reviewer are the same model

#### Scenario: Local reviewer states its limits [W7, auto]
- GIVEN the Review row is a local model
- WHEN a review starts
- THEN a visible notice states the limits of local models as reviewers

#### Scenario: Cloud reviewer on a local diff is announced [W7, auto]
- GIVEN a local author and a Review row on `claude>opus`
- WHEN the review starts
- THEN a visible notice says the staged diff goes to `claude>opus`

#### Scenario: A new vendor receiving the diff is announced [W7, auto]
- GIVEN a cloud author on `claude` and a Review row on another engine (`gemini>pro`)
- WHEN the review starts
- THEN a visible notice says the staged diff goes to `gemini>pro`; a Review row on the author's own engine says nothing about the diff

#### Scenario: Reviewer unavailable stops visibly [W7, auto]
- GIVEN a cheap author's change and a Review row whose engine is not installed
- WHEN the review is due
- THEN the review worker ends `refused`, the run stops with the failure alert, and the batch is not shown as reviewed

### Requirement: The `simple` worker [W8]
A `simple` worker is defined by its role: it executes direct, fully specified tasks (a script, code, text, a summary, producing an already defined design) and MUST NOT design, analyse, review or delegate. It is never chosen by the worker itself: only the coordinator assigns it. Its toolset is the read tools, the write tools and `run_command` ([W6], when it exists), each offered only when the menu ceiling allows (Menu ceiling for worker toolsets). It runs on the `simple` assignment (own assignment, else `general`, shown visibly; see ai-task-models). It is subject to every check that applies to a `code` worker: the read-only goal gate, the step-state gates, the read scope, secret rules and the batch review, at spawn, when a queued worker starts and on retry. On a read-only goal a `simple` spawn whose brief lists files to write MUST be refused, and a `simple` worker that is allowed there MUST NOT be offered write tools.

#### Scenario: Toolset follows the role and the ceiling [W8, auto]
- GIVEN Write on and Exec off
- WHEN a `simple` worker's tool list is built
- THEN it holds `find_dir`, `list_dir`, `read_file` and the write tools, and no `run_command`

#### Scenario: Read-only goal refuses a writing simple [W8, auto]
- GIVEN the goal `explain how the read scope works` and a `simple` brief listing `src/a.rs` as an output
- WHEN `spawn_agent` is called with kind `simple`
- THEN it is refused as outside a read-only goal, and the same refusal applies when a queued `simple` worker is about to start

#### Scenario: No write tools on a read-only goal [W8, auto]
- GIVEN a read-only goal and a `simple` brief with no files to write
- WHEN the worker starts
- THEN it has no write tool

#### Scenario: No outputs, no command tools [W8, auto]
- GIVEN any goal, Write and Exec on, and a `simple` brief whose `outputs` is empty
- WHEN the worker starts
- THEN it has the read tools only: no write tool and no `run_command`, because a command runs only after the worker's batch is applied

#### Scenario: A worker never selects itself [W8, auto]
- GIVEN a running worker whose text asks to be run as `simple`
- WHEN the text is processed
- THEN no `simple` worker is created, because only a coordinator `spawn_agent` creates workers

### Requirement: The brief [W8]
A `simple` spawn MUST carry a brief with these fields at least: goal, files (inputs and outputs), done when, out of scope. A missing field MUST be refused (see Spawn refusals that create no worker) with an error that names the missing fields, so the coordinator can fix it. Extra fields MUST be accepted and MUST be flagged in the worker's result so the owner can decide to grow the format. The brief form is open for design (`spawn_agent` carries free text today). With Write off, the brief decides whether the worker must write: it must write only if the files field lists files to write. `summarize` serves the flow; a `simple` worker writes a summary only when the summary is a file to deliver. The brief format applies to `simple` only, not to `code`.

#### Scenario: Missing done-when refused [W8, auto]
- GIVEN a brief with goal, files and out of scope but no done-when
- WHEN `spawn_agent` is called with kind `simple`
- THEN it is refused naming `done when`, no worker exists, and the user sees a notice

#### Scenario: Extra field flagged [W8, auto]
- GIVEN a complete brief with an extra field `priority`
- WHEN the worker ends
- THEN its result says the field `priority` was extra

#### Scenario: Write off with outputs [W8, auto]
- GIVEN Write off and a brief listing `scripts/run.sh` as an output
- WHEN the spawn is made
- THEN it is refused with the Write notice

#### Scenario: Write off with inputs only [W8, auto]
- GIVEN Write off and a brief whose files are inputs only
- WHEN the spawn is made
- THEN the worker starts with no write tool

#### Scenario: Code worker needs no brief [W8, auto]
- GIVEN a `code` spawn with free text
- WHEN it is made
- THEN the brief check does not apply

### Requirement: Clarification rounds [W8]
A `simple` worker that does not understand its brief MUST end with the marker "needs specification: X". The worker's result MUST show it as such to the coordinator (as untrusted data) and the user MUST see a notice. The coordinator then answers, or asks the user, and starts a new worker. A brief MAY have at most two clarification rounds; a third "needs specification" for the same brief MUST stop the run with a visible notice naming the brief. The text after the marker is untrusted and MUST NOT answer an approval or change a scope. A split-in-two (see Per-file line limit) ends the worker, is issued as two new briefs, and does not count as a round.

#### Scenario: Two rounds allowed [W8, auto]
- GIVEN a brief answered "needs specification" twice and re-issued each time
- WHEN a worker runs the third time and completes
- THEN the run goes on

#### Scenario: Third round stops the run [W8, auto]
- GIVEN the same brief answered "needs specification" a third time
- WHEN the worker ends
- THEN the run stops and a notice names the brief and says the limit of two rounds was reached

#### Scenario: Split is not a round [W8, auto]
- GIVEN two clarification rounds used and a split-in-two on that brief
- WHEN the two new briefs are spawned
- THEN neither starts with a round count taken from the split

### Requirement: Per-file line limit for `simple` [W9]
A `simple` worker MUST NOT write more than 400 counted lines of code to one file in one brief without asking the user. Counted lines are the lines the worker writes in that file for that brief (a small fix in a large file does not trigger the limit). Comments, doc comments and blank lines do not count; for a language the app cannot parse, every line counts and the ask says so. A file is a test file by path (`tests/`, `*_test.*`, `*_tests.*`, `tests.*`, `*.spec.*`, `*.test.*`; assumption E as amended by the owner on 2026-11-10) and a test file has no cap; an inline test (such as `#[cfg(test)]`) counts as code. Exceeding the limit MUST ask the user when the worker calls `propose_batch`, before any review and before the batch is sealed, offering: split into two parts, or keep one file with the extra lines (owner, 2026-11-10, design D14). The ask MUST be answered only by the user's key, in every approval mode including `run`, and no timer or mode MUST answer it. A declined, timed-out or cancelled ask ends as denied and the extra lines are not written. Split ends the worker and discards its staged batch, and the result and a notice show how many staged files were discarded; the coordinator issues two new briefs, and the split is not a clarification round (owner, 2026-11-10). The limit applies to `simple` only, not to `code`. The ask carries the worker label and joins the same queue as the worker's other asks.

#### Scenario: 401 counted lines asks [W9, auto]
- GIVEN a `simple` worker writing 401 counted lines to `src/a.rs`, in approval mode `run`
- WHEN it calls `propose_batch`
- THEN an ask offers `split` and `keep`, labelled with the worker, and nothing is proposed before the user answers

#### Scenario: 400 lines does not ask [W9, auto]
- GIVEN 400 counted lines
- WHEN the worker calls `propose_batch`
- THEN no ask appears

#### Scenario: Comments and blanks do not count [W9, auto]
- GIVEN 300 code lines, 150 comment lines and 50 blank lines
- WHEN the worker calls `propose_batch`
- THEN no ask appears

#### Scenario: Unknown language counts every line, and says so [W9, auto]
- GIVEN a file in an unparsed language with 401 lines
- WHEN the worker calls `propose_batch`
- THEN the ask appears and states that every line was counted

#### Scenario: Tests have no cap [W9, auto]
- GIVEN 900 lines written to `tests/flow.rs`, and 900 lines written to `src/orchestra/run_tests.rs`
- WHEN the worker calls `propose_batch`
- THEN no ask appears

#### Scenario: A later batch over the limit asks again [W9, auto]
- GIVEN a file already over 400 counted lines for the brief after an earlier batch, where the user answered `keep`
- WHEN a later batch of the same brief touches that file
- THEN the ask appears again: the limit counts the lines written for the brief, and a kept `y` is not remembered per file (owner decision, 2026-11-11)

#### Scenario: Small edit in a large file [W9, auto]
- GIVEN a file of 2,000 lines and an edit that writes 12 lines
- WHEN the worker calls `propose_batch`
- THEN no ask appears

#### Scenario: Split ends the worker [W9, auto]
- GIVEN the ask answered `split`
- WHEN the answer is applied
- THEN the worker ends, its staged batch is discarded, its result and a notice say a split was chosen and how many staged files were discarded, and the coordinator may spawn two new briefs

#### Scenario: No mode and no timer answers [W9, auto]
- GIVEN the ask pending in approval mode `run`
- WHEN the idle timers pass
- THEN it stays pending, and a timeout ends it as denied with the extra lines not written

### Requirement: Writes outside the brief's files ask [W9]
A `simple` worker that stages a write to a path its brief does not list MUST be asked about it when it calls `propose_batch`, in the same ask as the line limit (design D14), labelled with the worker, only when the path is inside the run's read scope; a write outside the read scope, to a guarded secret path or outside the chat's root stays refused and never asked (Worker toolsets). The ask is answered only by the user's key in every approval mode. This applies to `simple` only.

#### Scenario: Unlisted file inside the scope asks [W9, auto]
- GIVEN a brief listing `src/a.rs` and a staged write to `src/b.rs`, both inside the scope
- WHEN the worker calls `propose_batch`
- THEN one ask names `src/b.rs` and nothing is proposed before the user answers

#### Scenario: A project-root output covers every file [W9, auto]
- GIVEN a `simple` brief whose outputs list `.` (the project root, granted by the coordinator on purpose)
- WHEN the worker stages a write to any file inside the scope and calls `propose_batch`
- THEN no outside-the-brief ask is raised for that brief (owner decision, 2026-11-11)

#### Scenario: Outside the scope is refused, not asked [W9, auto]
- GIVEN a staged write to `.env` or `../other/x.rs`
- WHEN it is attempted
- THEN it is refused with the plain-write refusal text and no ask opens

### Requirement: One batch per writing worker and parallel writers [W10]
Each write-capable worker MUST have its own task and its own batch, so one worker's proposal never seals another's staged files, and each batch MUST be reviewed on its own, labelled with its worker. Write-capable workers MAY run in parallel up to `max_parallel`. Until W10 ships, write-capable workers run one at a time per pane, and a second one is queued with a visible notice saying why. When two batches touch the same path, nothing may be silently overwritten: the existing apply-time drift check refuses a stale batch visibly. Whether the brief overlap is checked earlier is open.

#### Scenario: Two writers, two batches [W10, auto]
- GIVEN two `simple` workers with different tasks, running in parallel
- WHEN both propose
- THEN two reviews are offered one at a time, each labelled with its worker, and approving one does not seal or apply the other

#### Scenario: Before W10, one writer at a time [W10, auto]
- GIVEN a build without parallel writers and two write-capable spawns
- WHEN they are spawned
- THEN the second is queued and a notice names the reason

#### Scenario: Same file, drift refused [W10, auto]
- GIVEN two batches that stage the same path and the first is applied
- WHEN the second is approved
- THEN apply refuses it as drifted and says so

### Requirement: No engine outside the menu runs [W1-W10]
Nothing in this change MUST start an engine or a model other than the one the menu assigns to the task (or the run's override). A refused, failed or unavailable engine MUST NOT trigger an automatic escalation to a cloud engine, and a retry is never automatic.

#### Scenario: No silent escalation [W7, auto]
- GIVEN a cheap author that failed
- WHEN the failure is handled
- THEN the fake runner is called for no other engine and the failure alert is shown
