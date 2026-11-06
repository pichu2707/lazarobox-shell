# Delta for Agent Orchestrator

Slice tags (this change): [W1] disclosure; [W2] worker prompt and menu ceiling; [W3] step-state gates; [W4] app-checked handoffs (see orchestrator-handoffs); [W5] worker command ask (see command-runner); [W6] worker `run_command`; [W7] mandatory review for cheap authors; [W8] `simple` task, brief, clarification; [W9] per-file limit and outside-brief ask; [W10] one batch per writer, parallel writers. Older tags (S1..S10) keep their meaning. Tags: [auto] = `cargo test` with a fake `Runner` and paused tokio time; [manual] = real machine.

Terms added: a *cheap author* is a worker whose resolved engine is a local engine (a server engine such as Ollama or llama.cpp; decision 9, "cheap" means local engines for now). A *brief* is the assignment of a `simple` worker. A *read-only goal* is one classified by `authorize`. The *Review row* is the effective assignment of the `review` task.

Rule for every requirement below: every refusal, gate, limit and notice MUST be visible to the user in the transcript, and the coordinator MUST receive the same fact in the tool result. Nothing fails or is ignored silently. lazarobox never picks an engine or a model and never escalates to the cloud.

## ADDED Requirements

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

### Requirement: Waiting on the user stops the worker clock [W6]
Time a worker spends waiting for the user's answer (a batch review it proposed, a command ask, a read ask, a limit ask) MUST NOT count against its deadline (`worker_timeout_secs`). The clock MUST resume when the answer arrives. Time spent on the worker's own work still counts. Whether the same pause applies to the run wall clock (`run_timeout_mins`) is open.

#### Scenario: A long review does not time out the worker [W6, auto]
- GIVEN a worker with a 600 s deadline whose batch waits 20 minutes for the user
- WHEN the user approves and the worker then runs its tests
- THEN the worker is not `timed_out` and the time it waited was not counted

#### Scenario: Work time still counts [W6, auto]
- GIVEN a worker that runs for the whole deadline without waiting on the user
- WHEN the deadline passes
- THEN it ends `timed_out`

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
A `simple` worker MUST NOT write more than 400 counted lines of code to one file in one brief without asking the user. Counted lines are the lines the worker writes in that file for that brief (a small fix in a large file does not trigger the limit). Comments, doc comments and blank lines do not count; for a language the app cannot parse, every line counts and the ask says so. A file is a test file by path (`tests/`, `*_test.*`, `*.spec.*`, `*.test.*`) and a test file has no cap; an inline test (such as `#[cfg(test)]`) counts as code. Exceeding the limit MUST ask the user before the batch is proposed, offering: split into two parts, or keep one file with the extra lines. The ask MUST be answered only by the user's key, in every approval mode including `run`, and no timer or mode MUST answer it. A declined, timed-out or cancelled ask ends as denied and the extra lines are not written. Split ends the worker, the coordinator issues two new briefs, and the split is not a clarification round. The limit applies to `simple` only, not to `code`. The ask carries the worker label and joins the same queue as the worker's other asks.

#### Scenario: 401 counted lines asks [W9, auto]
- GIVEN a `simple` worker writing 401 counted lines to `src/a.rs`, in approval mode `run`
- WHEN it stages the file
- THEN an ask offers `split` and `keep`, labelled with the worker, and nothing is proposed before the user answers

#### Scenario: 400 lines does not ask [W9, auto]
- GIVEN 400 counted lines
- WHEN the file is staged
- THEN no ask appears

#### Scenario: Comments and blanks do not count [W9, auto]
- GIVEN 300 code lines, 150 comment lines and 50 blank lines
- WHEN the file is staged
- THEN no ask appears

#### Scenario: Unknown language counts every line, and says so [W9, auto]
- GIVEN a file in an unparsed language with 401 lines
- WHEN the file is staged
- THEN the ask appears and states that every line was counted

#### Scenario: Tests have no cap [W9, auto]
- GIVEN 900 lines written to `tests/flow.rs`
- WHEN the file is staged
- THEN no ask appears

#### Scenario: Small edit in a large file [W9, auto]
- GIVEN a file of 2,000 lines and an edit that writes 12 lines
- WHEN the edit is staged
- THEN no ask appears

#### Scenario: Split ends the worker [W9, auto]
- GIVEN the ask answered `split`
- WHEN the answer is applied
- THEN the worker ends, its result says a split was chosen, and the coordinator may spawn two new briefs

#### Scenario: No mode and no timer answers [W9, auto]
- GIVEN the ask pending in approval mode `run`
- WHEN the idle timers pass
- THEN it stays pending, and a timeout ends it as denied with the extra lines not written

### Requirement: Writes outside the brief's files ask [W9]
A `simple` worker that stages a write to a path its brief does not list MUST ask the user, labelled with the worker, only when the path is inside the run's read scope; a write outside the read scope, to a guarded secret path or outside the chat's root stays refused and never asked (Worker toolsets). The ask is answered only by the user's key in every approval mode. This applies to `simple` only.

#### Scenario: Unlisted file inside the scope asks [W9, auto]
- GIVEN a brief listing `src/a.rs` and a staged write to `src/b.rs`, both inside the scope
- WHEN the write is staged
- THEN an ask names `src/b.rs` and nothing is staged before `y`

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

## MODIFIED Requirements

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

### Requirement: Final documentation [S10]
The README MUST be updated once, in the final slice, with: how to enable and use the orchestrator (the `Orchestrator` menu gate and the per-pane Ctrl+O toggle), the Orchestrate menu row, the provider-terms statement (providers recommend API keys for automation, consumer logins carry risk, no key required) and the Ollama "not recommended" note. Delta (2026-10-31): it also states the keys, the config keys, the memory and privacy gate, the persistence of runs and the known limits, among them that workers cannot run commands or tests in 1.1.0 (S8 deferred). No earlier slice MUST edit the README for this feature. (This change, W6) From the release that ships W6 the README MUST instead state that a worker can run a command only with Exec on, only after its batch is approved, and only after the user approves each command; S8 enforcement stays deferred.
(Previously: the known-limits statement said workers cannot run commands or tests)

#### Scenario: README written once [manual]
- GIVEN the final slice is merged
- WHEN the README is read
- THEN it documents the orchestrator, the menu row and the provider note, and the earlier slices' diffs contain no README change for this feature

## Open points (not decided; no behaviour specified)

- Which step-state transitions the W3 spawn and finish gates cover, the turn-cap interaction with clarification rounds, and the override key.
- The brief's form (structured arguments of `spawn_agent` or marked free text), how "needs specification" is recognised, the line counter, and the delegate key.
- The mechanism by which the user turns Write on from the chat (per-chat grant like `/github`, or another form), and whether Exec gets the same path.
- "When the machine can take it": assumed `max_parallel` stays the ceiling with no hardware measuring.
- Order and presentation of several batch reviews in one run; whether the brief overlap of two writers is checked before apply.
- Whether one command at a time per pane stays when workers run in parallel, and whether a worker's command refusal `propose them first` is scoped to its own batch (W10).
- Whether waiting on the user also pauses the run wall clock (`run_timeout_mins`).
- Outcome of a split for content already staged (kept, or discarded).
- Resume of a stored run whose worker kind is `simple`, brief or round counters, and a downgrade build: it MUST stay visible and never be dropped; the form is for design after reading `persist`.
- Stale statements outside the changed requirements, to be reworded at apply/archive: the S10 scenario "Small task from the chat" (tests not run in 1.1.0) and "Missing dependency" (workers never have `run_command`, and the test `the_orchestrate_prompts_say_no_worker_can_run_commands_or_tests`) are 1.1.0 history; W6 changes them.
