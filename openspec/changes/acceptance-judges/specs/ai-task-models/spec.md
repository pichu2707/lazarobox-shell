# Delta for AI Task Models

## Base of this delta

This delta targets `openspec/specs/ai-task-models/spec.md`. MODIFIED blocks are full copies with edits marked "(Previously: ...)". All requirements here are slice [S3]. Settled: J4 (menu row), J11 (an unassigned `judge` falls back to the run's default model like `general` does, and `Set all tasks` includes `judge`).

Note on drift in the base: "Set-all summary" and "Engine choices" still say "five" tasks and "Fallback to general" scenarios say "the other four", although `orchestrate` and `simple` exist. This delta edits only what `judge` changes: "Fixed task set", "Set-all summary", "Runtime use of every task assignment" and the two "Set all excludes" requirements. Anywhere else the base counts the Set-all tasks as five (for example "Engine choices" listing engines assigned in "any of the five tasks"), it MUST count six, including `judge`.

## MODIFIED Requirements

### Requirement: Fixed task set [S1, W8, S3]
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge`. Task ids are lowercase and are the `config.toml` table names. There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).
(Previously: seven tasks, `judge` did not exist)

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge` in that order, with unique ids

### Requirement: Set-all summary [S3]
The aggregate `Set all tasks` value MUST be derived from the six EFFECTIVE (resolved) assignments of the draft (`general`, `code`, `chat`, `review`, `summarize`, `judge`): when all six resolve to the same value it shows that value (`(not set)` when all resolve to nothing, otherwise `<engine> › <model>`); otherwise it shows `(mixed)`. An unassigned non-general task resolves to general, so choosing `none` for Set all (everything unassigned) reads `(not set)`.
(Previously: five tasks, `judge` not among them)

#### Scenario: Aggregate values [auto]
- GIVEN (a) all six are `claude` / `opus`, (b) none assigned, (c) general `claude` / `opus` and the other five unassigned, (d) general `claude` / `opus` and code `ollama` / `llama3:8b`, (e) the five others `claude` / `opus` and `judge` `codex` / `default`
- WHEN the aggregate is computed
- THEN it reads `claude › opus`, `(not set)`, `claude › opus`, `(mixed)` and `(mixed)` respectively

### Requirement: Set all excludes Orchestrate [S1, S3]
`Set all tasks` MUST write, clear and summarize the six tasks `general`, `code`, `chat`, `review`, `summarize` and `judge` only, and MUST NOT read or write `orchestrate`.
(Previously: five tasks, without `judge`)

#### Scenario: Set all leaves the orchestrator alone [auto]
- GIVEN `orchestrate` is `claude` / `opus`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the six tasks are `codex` / `default` and `orchestrate` is still `claude` / `opus`, and the aggregate ignores `orchestrate`

### Requirement: Set all excludes Simple [W8, S3]
`Set all tasks` MUST write, clear and summarize the six tasks `general`, `code`, `chat`, `review`, `summarize` and `judge` only, and MUST NOT read or write `simple` (as for `orchestrate`).
(Previously: five tasks, without `judge`)

#### Scenario: Set all leaves simple alone [W8, auto]
- GIVEN `simple` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the six tasks are `codex` / `default`, `simple` is still `ollama` / `llama3:8b`, and the aggregate ignores `simple`

### Requirement: Runtime use of every task assignment [S1, S3]
Each task MUST be used at runtime through its effective assignment (own assignment, else `general`; see Fallback to general), read from the current configuration on every use, so a saved change applies from the next message or the next worker spawn. A chat turn uses `chat`, except in an orchestrating pane (global gate on and the pane's ORCH mode on), where the turn uses `orchestrate`; each orchestrator worker uses the assignment of its kind (`general`, `code`, `chat`, `review`, `summarize`, `simple`), and each acceptance judge uses `judge` (the second judge of a pair uses `review`, see `acceptance-judges`). The model MUST be validated against detection before use: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance (or the worker is `refused`) and nothing is sent. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The runtime MUST NOT change any assignment on its own, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config; only a user choice in the menu changes an assignment. The picker opened from a worker failure alert does not change any assignment: its choice is a run-scoped override (see agent-orchestrator) that is never written to the draft or the file.
(Previously: the list of worker kinds had no judge)

Existing scenarios stay unchanged. Added scenario:

#### Scenario: Judge uses its effective assignment [S3, auto]
- GIVEN `judge` is `ollama` / `qwen3:14b` (available) and `general` is `claude` / `sonnet`
- WHEN a judge worker is spawned
- THEN it runs on ollama with `qwen3:14b`, and an unusable model ends it `refused` with the model named and nothing sent elsewhere

## ADDED Requirements

### Requirement: Judge fallback and use [S3]
`judge` follows Fallback to general: unassigned, it uses the `general` assignment (the run's default model, as `general`), and its row shows `(default)`, which MUST be visible. An acceptance judge MUST run on the effective `judge` assignment read at spawn time, and the second judge of a pair on `review` when it differs (see `acceptance-judges`). The runtime MUST NOT pick another engine or model, MUST NOT escalate to the cloud and MUST NOT write the config. The server engines are not marked `not recommended` for `judge`: the limits of local judges are said by the run-time notices instead.

#### Scenario: Judge falls back [S3, auto]
- GIVEN `judge` unassigned and `general` is `claude` / `sonnet`
- WHEN a judge is spawned
- THEN it runs on claude / `sonnet` and the row reads `(default)`

#### Scenario: Judge on a local model [S3, auto]
- GIVEN `judge` is `ollama` / `llama3:8b` (available)
- WHEN a judge is spawned
- THEN it runs on ollama / `llama3:8b` and no other engine is contacted

#### Scenario: Unusable judge engine [S3, auto]
- GIVEN `judge` is `ollama` with a model that is not downloaded
- WHEN a judge is spawned
- THEN it ends `refused` naming the model and `download required`, the assignments are unchanged and no other engine is contacted

#### Scenario: Nothing assigned labels [S3, auto]
- GIVEN no task is assigned
- WHEN the labels are built
- THEN `general` reads `(not set)` and every other task, `judge` included, reads `(default)`
