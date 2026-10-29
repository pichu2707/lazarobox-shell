# Agent Orchestrator Specification

## Purpose

Defines the orchestrator: one coordinator agent that splits a multi-step job across worker agents, each running on the engine and model the AI menu assigns to its task. It covers the run lifecycle, the orchestration tools, the worker contract, limits, the run-start read scope, approvals, failure alerts and relaunch, the provider notice, the ODD flow and the receipt-driven review (RDD). Handoff persistence is in orchestrator-handoffs. The menu is the single source of truth: lazarobox never picks an engine or a model.

Slice tags: [S1] contract + one read-only worker; [S2] failure alerts; [S3] multiple workers; [S4] handoffs and ledger (see orchestrator-handoffs); [S5] ODD flow, read-only until S7; [S6] usage; [S7] write integration (needs coding-tools M2, merged); [S7b] receipt-driven review (RDD); [S8] ODD test-first evidence (needs coding-tools M3 and S7); [S9] run view and persistence. Tags: [auto] = `cargo test` with a fake `Runner` and paused tokio time; [manual] = real machine.

Product goal: the finished feature MUST let the user build a programming project entirely from the lazarobox chat, without opening an IDE: plan, write code, run tests and review. That end state depends on the coding-tools write tools (M2) and `run_command` (M3) and is accepted only by the end-to-end requirement tagged [S10]. [S10] is the final slice: user-facing documentation and end-to-end acceptance, after S1..S9. Slices before S10 MUST NOT change the README; the feature stays off by default and undocumented there until S10. Internal docs (openspec, CHANGELOG Internal entries) may change in any slice.

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
The first time the orchestrator would start a run while `[ai.orchestrator] notice_accepted` is `false`, an informational notice MUST be shown saying that providers recommend API keys for automation and that consumer-account logins carry risk. The notice MUST be accepted with one key, persisted as `notice_accepted = true`, and never shown again once accepted. It MUST NOT block: the run starts without waiting for the answer, no engine is refused because of it, and no API key is required. (The README statement is a [S10] requirement; see Final documentation.)

#### Scenario: Shown once, never blocks [auto]
- GIVEN `notice_accepted = false` and an orchestrating pane
- WHEN the first run starts
- THEN the notice is shown, the first worker is spawned without waiting for the key, and after acceptance the key is saved as `true`

#### Scenario: Not shown again [auto]
- GIVEN `notice_accepted = true`
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

### Requirement: Worker contract [S1]
A worker MUST be started from a `WorkerSpec` holding: kind, prompt, the engine and model resolved for that kind at spawn time (`general`, `code`, `chat`, `review`, `summarize`; the run's override for the kind if one exists, else the menu), the toolset for the kind, and a deadline (default 10 minutes, key `worker_timeout_secs`). It MUST end with a `WorkerResult` holding: status (`done`, `failed`, `cancelled`, `timed_out`, `refused`), the final text (capped and wrapped as untrusted data), elapsed time, and the provider error class and message when the status is `failed`. A worker MUST be one stateless process per turn with a private working folder and the same jail as a chat turn. The engine and model used MUST be those in effect at spawn time (run override, else menu), even if the menu changes later during the run.

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

### Requirement: Refusal on an unavailable engine [S1]
Before starting a worker, the engine resolved for its kind MUST pass the same availability checks as a chat turn for that task (installed, version, model present, jail where required). When it does not, the spawn MUST end with status `refused`, a structured reason and no process started. lazarobox MUST NOT pick another engine or model. A refusal MUST be handled as a worker failure (see Failure stops the run).

#### Scenario: No jail refuses [auto]
- GIVEN `review` is `gemini` and the jail is Unavailable
- WHEN a `review` worker is spawned
- THEN the status is `refused` with the no-jail reason, no process starts and no other engine is contacted

#### Scenario: Model missing refuses [auto]
- GIVEN `code` is `ollama` with a model that is not downloaded
- WHEN a `code` worker is spawned
- THEN the status is `refused` and the reason names the model and `download required`

### Requirement: Worker toolsets [S1, S7]
Until write tools exist, every worker MUST be offered only the read-only tools (`find_dir`, `list_dir`, `read_file`). It MUST NOT be offered `run_in_pane`, any write tool or any orchestration tool. [S7] `code` workers MAY be offered the coding-tools write tools. Their writes MUST be staged and reach the disk only through the user's batch review, labelled with the worker; no scope pre-approves a write.

#### Scenario: Read-only subset [auto]
- GIVEN a `review` worker
- WHEN its tool list is built
- THEN it is exactly `find_dir`, `list_dir`, `read_file`

#### Scenario: Writes wait for the batch review [S7, auto]
- GIVEN a `code` worker with a write tool and a run's read scope
- WHEN it calls a write tool inside the scope
- THEN the change is staged with the worker label and nothing is written before the user approves the batch

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
- THEN both are refused with the one identical refusal message and no approval is offered

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
Each adapter MUST classify a worker failure as one of: `rate_limit` (429), `conflict` (409), `auth`, `quota`, `timeout`, `jail_refusal`, `other`. The class MUST be shown with the raw message. `rate_limit` MUST be its own class: the alert names the limit and offers the same options, and lazarobox MUST NOT retry on its own.

#### Scenario: 429 is its own class [auto]
- GIVEN a worker whose provider answers 429
- WHEN the failure is classified
- THEN the class is `rate_limit`, the alert shows the provider message, and no retry is made without the user

#### Scenario: Class table [auto]
- GIVEN decoder fixtures for 409, auth failure, quota exhausted, a timeout, a jail exit 71 and an unknown error
- WHEN each is classified
- THEN they give `conflict`, `auth`, `quota`, `timeout`, `jail_refusal`, `other`

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

### Requirement: ODD flow [S5]
The run MUST follow the ODD steps in order: `authorize`, `explore`, `resolve`, `classify`, `track` (large goals only), `implement`, `close`. A step MUST NOT be recorded as done without a handoff for it.
- `authorize`: a goal that asks to investigate, explain, review, compare or plan is read-only, and no `code` worker MUST be spawned for it.
- `resolve`: the orchestrator MAY ask the user one question, then MUST wait for the answer; it MUST NOT ask a second question in the same turn.
- `classify`: the goal is `small` when it is understood, touches no high-risk item and could be finished from the goal and the change alone; otherwise `large`. The number of files or tasks MUST NOT decide it.
- `track`: a `large` goal MUST get a feature document (header, `Specs` with numbered `S#` quoting the user's words verbatim, `Tasks` with the linked `S#`, and `Log` whose `L1` is the goal verbatim), saved as a handoff. A `small` goal MUST NOT get one.
- `implement`: each task worker MUST receive the feature document by reference (document, task id, `S#`) and report which `S#` it covered.
- `close`: the run MUST report the verified outcome, every failed, skipped or pending check, and a risk line.
Until S7, `implement` MUST produce only drafts and review findings, and the orchestrator MUST NOT claim code was written or tests passed.

#### Scenario: Read-only goal spawns no code worker [auto]
- GIVEN the goal `explain how the read scope works`
- WHEN the orchestrator calls `spawn_agent` with kind `code`
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

### Requirement: ODD test-first evidence [S8]
Once `run_command` exists, for a task with a runnable deterministic test the run MUST require failing evidence before the implementation is accepted and passing evidence after it. Evidence MUST come from the `run_command` result, never from worker text alone. A task without a runnable test MUST record an explicit exception with its reason, and is then checked by a review worker instead.

#### Scenario: Green without red refused [S8, auto]
- GIVEN an implementation result with passing evidence and no earlier failing evidence
- WHEN the task is closed
- THEN it is refused as missing red evidence

#### Scenario: Exception recorded [S8, auto]
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
Each `code` worker MUST keep a ledger of the files it staged (path, size, diff hash), shown in its result as `files changed`. Its writes MUST be staged and reach the disk only when the user approves the batch in the batch review.

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

### Requirement: Receipt-driven review [S7b]
A `high` change, or any change the user asks to review, MUST ask the user for consent for that change, inline, before any review starts. A declined change MUST go on to the batch review unreviewed, marked `not reviewed`. On consent:
- The orchestrator MUST pick only the lenses the change calls for, from `risk`, `resilience`, `readability` and `reliability`, with a one-line reason.
- Each lens MUST run as a read-only `review` worker over the staged change, wrapped as untrusted data.
- Only a severe finding the change caused MUST block. A finding that already held before the change is a follow-up.
- Inferential blockers MUST go to one refuter worker before they count.
- At most one bounded correction MUST run, by a `code` worker, scoped to the blockers that survived; its result is validated against those blockers only.
- The review MUST end in a receipt handoff (tier, lenses, findings, correction, outcome), shown in the batch review next to the diff.
A receipt MUST NOT approve the batch, and the orchestrator MUST NOT consent or approve anything.

#### Scenario: Consent first [S7b, auto]
- GIVEN a `high` change
- WHEN the review is due
- THEN the user is asked for consent and no review worker runs before the answer

#### Scenario: Declined goes on unreviewed [S7b, auto]
- GIVEN a `high` change and a declined consent
- WHEN the batch review opens
- THEN it shows the change marked `not reviewed` and no review worker ran

#### Scenario: One correction at most [S7b, auto]
- GIVEN a review whose correction still leaves a blocker
- WHEN the correction result is validated
- THEN no second correction runs, and the receipt names the open blocker for the user to decide

#### Scenario: A receipt never approves [S7b, auto]
- GIVEN a receipt with no blockers
- WHEN the batch review opens
- THEN the batch still waits for the user's `y`

### Requirement: Run view and persistence [S9]
A key MUST open a full-pane transcript of a worker. Runs MUST be stored locally and, after a restart, shown as interrupted with a resume choice; a resumed run relaunches unfinished workers from scratch only on the user's choice, and re-asks the read scope.

#### Scenario: Restart shows an interrupted run [S9, auto]
- GIVEN a stored run with one running worker when lazarobox quit
- WHEN lazarobox starts again
- THEN the run shows as interrupted, nothing runs, and resume asks the scope again

### Requirement: Final documentation [S10]
The README MUST be updated once, in the final slice, with: how to enable and use the orchestrator (the `Orchestrator` menu gate and the per-pane Ctrl+O toggle), the Orchestrate menu row, the provider-terms statement (providers recommend API keys for automation, consumer logins carry risk, no key required) and the Ollama "not recommended" note. No earlier slice MUST edit the README for this feature.

#### Scenario: README written once [manual]
- GIVEN the final slice is merged
- WHEN the README is read
- THEN it documents the orchestrator, the menu row and the provider note, and the earlier slices' diffs contain no README change for this feature

### Requirement: Build a project from the chat, end to end [S10]
With coding-tools M2 and M3 built and S1..S9 and S7b merged, a user MUST be able, from one lazarobox chat and without opening an IDE, to take a small programming task through plan, failing test, implementation, passing test and review: the orchestrator drives the ODD steps, a `code` worker stages files (applied only through the user's batch review and listed in `files changed`), tests run only through `run_command` with red evidence before and green evidence after, a high-risk change gets a receipt-driven review, and handoffs record each step. If either dependency is missing the requirement is not accepted and the feature is described as drafts only.

#### Scenario: Small task from the chat [S10, manual]
- GIVEN the gate on, a chat pane with ORCH on, a scratch project, and `orchestrate`, `code` and `review` assigned to available engines
- WHEN the user asks for a function with tests in the chat and approves the scope and the batch
- THEN a failing test run is recorded, the files are written, a passing run is recorded, a review result is returned, handoffs exist for each ODD step, and the user never left the chat

#### Scenario: Missing dependency [S10, auto]
- GIVEN no write tool or `run_command` tool is registered
- WHEN the end-to-end flow is requested
- THEN the orchestrator states it can only produce drafts and does not claim code was written or tests passed

### Requirement: No content in logs [S1]
Worker prompts, worker output, provider error text and paths MUST NOT be written to logs or error values that outlive the screen; logs may carry ids, kinds, statuses and error classes.

#### Scenario: Log hygiene [auto]
- GIVEN a run with a failing worker
- WHEN logs are captured
- THEN they hold the class and status and none of the prompt, output or error message text
