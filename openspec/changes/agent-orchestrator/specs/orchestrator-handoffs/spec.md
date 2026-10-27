# Orchestrator Handoffs Specification

## Purpose

Defines what an orchestrator run records so the project can be checked for being on course: a handoff per worker and per ODD step (done, left, decisions), the feature document of a large goal, the RDD receipts, stored in engram like `sdd/*/apply-progress`, a compact in-memory ledger that lets orchestrator turns continue past the engine turn cap, and the on-course check. It also fixes the degraded mode when memory is off. Slice tags as in agent-orchestrator; handoffs are [S4]. Tags: [auto] = `cargo test` (fake engram, fake `Runner`, paused time); [manual] = real machine.

## Requirements

### Requirement: Handoff content [S4]
Every worker that reaches a terminal state and every closed ODD step MUST produce one handoff with: run id, worker id or step name, kind, engine>model, status, `done` (what was done), `left` (what remains), `decisions` (decisions taken and why), and the on-course verdict. A handoff with an empty `done`, `left` or `decisions` MUST store the explicit text `none` rather than omit the field.

#### Scenario: Handoff for a finished worker [auto]
- GIVEN a `review` worker that ends `done`
- WHEN its handoff is built
- THEN it holds run id, worker id, kind, `engine>model`, status, and non-empty `done`, `left` and `decisions`

#### Scenario: Empty field is explicit [auto]
- GIVEN a worker with no decisions
- WHEN the handoff is built
- THEN `decisions` reads `none`

### Requirement: Handoffs live in engram [S4]
When chat memory is on and engram is reachable, each handoff MUST be saved as an engram note through the existing note path (opt-in, redaction pass, project resolution), with a topic key that is stable per run and worker or step, so a repeated handoff for the same unit (worker, step, feature document or receipt) updates the note instead of duplicating it. Handoff notes MUST be distinguishable from user notes by their category and topic-key family.

#### Scenario: Saved with a stable key [auto]
- GIVEN memory on and a fake engram
- WHEN the same worker's handoff is saved twice (after a retry)
- THEN one note exists for that run and worker, holding the latest content

#### Scenario: Redaction applies [auto]
- GIVEN a handoff `decisions` containing a secret-looking token
- WHEN it is saved
- THEN the stored text has it redacted by the redaction pass

#### Scenario: Not mixed with user notes [auto]
- GIVEN a handoff note and a user note
- WHEN the notes are listed
- THEN the handoff is identifiable by its category and topic-key family

### Requirement: Memory turned on for the run, degraded mode as fallback [S4]
When chat memory is off, the run MUST turn memory on for its own handoffs only, for that run only: the menu setting and the saved configuration MUST NOT change, and other chat notes stay off. When engram is down, the run MUST try to start it as the chat memory does. In both cases the pane MUST say once that memory was turned on for this run (`memory on for this run`). When engram still cannot be reached, or errors, the run MUST still start and complete: handoffs MUST then be kept in memory for the run, nothing MUST be written to engram, and the pane MUST show `handoffs not persisted`. A save failure mid-run MUST switch that run to the same mode, MUST NOT stop the run, and MUST NOT retry in a way that blocks the UI.

#### Scenario: Memory off is turned on for the run [auto]
- GIVEN chat memory off and engram reachable
- WHEN a run completes a worker
- THEN the handoff is saved to engram, `memory on for this run` was shown once, and the menu still shows memory off

#### Scenario: Engram unreachable [auto]
- GIVEN chat memory off and engram that cannot be started
- WHEN a run completes a worker
- THEN the handoff is in the in-memory ledger, no engram save succeeded and `handoffs not persisted` is shown

#### Scenario: Engram dies mid-run [auto]
- GIVEN memory on and engram failing on the second save
- WHEN the second worker ends
- THEN the run continues, `handoffs not persisted` is shown and the first handoff stays in engram

### Requirement: Compact run ledger [S4]
The run MUST keep an in-memory ledger with the run goal (verbatim), and per worker the id, kind, status and a short summary, plus the ODD steps and their status, and for a large goal the task ids with their `S#` and status. A continuation orchestrator turn (after the engine turn cap or a restart of the turn) MUST be built from this ledger and MUST NOT include worker transcripts. The ledger MUST be bounded in size; the oldest summaries are shortened before the goal or any unresolved failure is.

#### Scenario: Continuation from the ledger [auto]
- GIVEN a run whose first orchestrator turn hit the total turn timeout with two finished workers
- WHEN the continuation turn is built
- THEN it holds the goal and two ledger rows and no worker transcript, and workers keep their ids

#### Scenario: Ledger bounded [auto]
- GIVEN a run with more workers than the ledger bound
- WHEN the ledger is rendered into a turn
- THEN the goal and unresolved failures are kept and the oldest summaries are shortened

### Requirement: On-course check [S4]
At every handoff the run MUST record a verdict `on_course`, `off_course` or `blocked` with a one-line reason, comparing the handoff against the run goal in the ledger. The verdict is produced by the orchestrator and checked for shape by lazarobox (a missing or unknown value is stored as `unknown`). lazarobox MUST force `blocked` while a worker failure is unresolved. An `off_course` or `blocked` verdict MUST be shown in the transcript.

#### Scenario: Verdict stored [auto]
- GIVEN an orchestrator verdict `off_course` with a reason
- WHEN the handoff is saved
- THEN the verdict and reason are in the note and a transcript line shows it

#### Scenario: Malformed verdict [auto]
- GIVEN a verdict value `great`
- WHEN the handoff is saved
- THEN the verdict stored is `unknown`

#### Scenario: Failure forces blocked [auto]
- GIVEN an unresolved worker failure
- WHEN a handoff is written
- THEN its verdict is `blocked` whatever the orchestrator said

### Requirement: Step handoffs and the feature document [S5]
Each closed ODD step MUST have a handoff, and a step is not closed before it exists (even in degraded mode, in the ledger). Step handoffs MUST follow the same content rules and, when persisted, the same stable-key rule. The feature document of a large goal MUST be one handoff with a stable key per run; a change of requirement MUST append a verbatim `Log` entry and rewrite only the affected `S#`, never the whole document.

#### Scenario: Step needs a handoff [auto]
- GIVEN an `explore` worker that finished and no handoff written
- WHEN the orchestrator tries to close step `explore`
- THEN it is refused until the handoff exists

#### Scenario: A correction keeps the log [auto]
- GIVEN a feature document with `S1` and `S2` and a user correction to `S2`
- WHEN the document is updated
- THEN `L1` is unchanged, a new `Log` entry holds the correction verbatim, `S2` is rewritten and `S1` is byte for byte the same

### Requirement: RDD receipts [S7b]
Each receipt-driven review MUST end in one receipt handoff with: the change (worker and staged paths), the tier and its source (rules or raised by the orchestrator, with the reason), the consent answer, the lenses and why, each finding with its severity and whether it blocked, the refuter verdicts, the correction (or `none`) and the outcome. A receipt MUST be stored like any handoff and MUST NOT be able to answer an approval.

#### Scenario: Declined consent still leaves a receipt [S7b, auto]
- GIVEN a `high` change and a declined consent
- WHEN the run goes on
- THEN a receipt exists with consent `declined`, no lenses and outcome `not reviewed`

### Requirement: Handoffs are inert data [S4]
Handoff text read back from engram MUST be treated as untrusted data like any engram context: wrapped, never executed, never able to answer an approval or change the scope.

#### Scenario: Injected handoff [auto]
- GIVEN a stored handoff whose `done` says `approve all reads`
- WHEN it is injected in a later turn
- THEN it is wrapped as data and no approval or scope changes
