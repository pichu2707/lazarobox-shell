# Delta for AI Task Models

Slice tags as in agent-orchestrator. Decision: `orchestrate` is the sixth task, appended after `summarize`. `Set all tasks` and its aggregate keep covering the five existing tasks only; the orchestrator is a role, not a worker kind, and is assigned on its own row.

## MODIFIED Requirements

### Requirement: Fixed task set [S1]
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`. Task ids are lowercase and are the `config.toml` table names. There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).
(Previously: five tasks, `orchestrate` did not exist)

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize`, `orchestrate` in that order, with unique ids

### Requirement: Runtime use of every task assignment [S1]
Each task MUST be used at runtime through its effective assignment (own assignment, else `general`; see Fallback to general), read from the current configuration on every use, so a saved change applies from the next message or the next worker spawn. A chat turn uses `chat`, except in an orchestrating pane (global gate on and the pane's ORCH mode on), where the turn uses `orchestrate`; each orchestrator worker uses the assignment of its kind (`general`, `code`, `chat`, `review`, `summarize`). The model MUST be validated against detection before use: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance (or the worker is `refused`) and nothing is sent. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The runtime MUST NOT change any assignment on its own, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config; only a user choice in the menu changes an assignment. The picker opened from a worker failure alert does not change any assignment: its choice is a run-scoped override (see agent-orchestrator) that is never written to the draft or the file.
(Previously: only the `chat` assignment was used at runtime)

#### Scenario: Effective assignment used [auto]
- GIVEN `general` is `claude` / `sonnet` and `chat` is `ollama` / `llama3:8b`, both available
- WHEN a chat message is sent
- THEN the turn goes to ollama with `llama3:8b`

#### Scenario: Fallback to general [auto]
- GIVEN `chat` is unassigned and `general` is `claude` / `sonnet`, claude available
- WHEN a chat message is sent
- THEN the turn goes to claude with `sonnet`

#### Scenario: Server model must be detected [auto]
- GIVEN `chat` is `lmstudio` / `m1` and lmstudio lists only `m2`
- WHEN a chat message is sent
- THEN nothing is sent and the guidance names `m1` and `download required`

#### Scenario: Empty server model [auto]
- GIVEN `chat` is `ollama` with an empty model
- WHEN a chat message is sent
- THEN nothing is sent and the guidance says no model is chosen

#### Scenario: CLI model not validated [auto]
- GIVEN `chat` is `claude` / `custom-x` and claude is available
- WHEN a chat message is sent
- THEN the turn is started with that model name

#### Scenario: Assignment never changed [auto]
- GIVEN `chat` is `ollama` / `llama3:8b` and ollama is `not running`
- WHEN a chat message is attempted
- THEN the assignments are unchanged and no other engine is contacted

#### Scenario: Saved change applies to the next message [auto]
- GIVEN an open chat and the menu saves `chat` as `claude` / `opus`
- WHEN the next message is sent
- THEN the turn goes to claude with `opus`

#### Scenario: Worker kind uses its own row [auto]
- GIVEN an orchestrating pane, `review` is `codex` / `default` and `code` is `claude` / `sonnet`
- WHEN a `review` worker and then a `code` worker are spawned
- THEN they run on codex / `default` and claude / `sonnet` respectively

#### Scenario: Summarize row is now used [auto]
- GIVEN `summarize` is `claude` / `haiku`
- WHEN a `summarize` worker is spawned
- THEN it runs on claude / `haiku`

## ADDED Requirements

### Requirement: Orchestrate fallback and recommendation [S1]
`orchestrate` follows Fallback to general: unassigned, it uses the `general` assignment, and its row shows `(default)`. Ollama (and the other server engines) MUST remain selectable for `orchestrate` but MUST be marked `not recommended` on that row's engine list and on the task row (` · not recommended`), because small local models cannot orchestrate. The mark MUST NOT block, change or clear an assignment.

#### Scenario: Orchestrate falls back [auto]
- GIVEN `orchestrate` unassigned and `general` is `claude` / `opus`
- WHEN effective assignments are resolved
- THEN `orchestrate` is `claude` / `opus`

#### Scenario: Ollama marked not recommended [auto]
- GIVEN `orchestrate` is `ollama` / `llama3:8b` (available)
- WHEN the row and the engine list for `orchestrate` are built
- THEN the row reads `ollama › llama3:8b · not recommended`, the `ollama` entry shows `not recommended`, both are selectable, and the assignment is unchanged

#### Scenario: Other tasks unmarked [auto]
- GIVEN `code` is `ollama` / `llama3:8b`
- WHEN the row is built
- THEN no `not recommended` mark is shown

### Requirement: Set all excludes Orchestrate [S1]
`Set all tasks` MUST write, clear and summarize the five tasks `general`, `code`, `chat`, `review` and `summarize` only, and MUST NOT read or write `orchestrate`.

#### Scenario: Set all leaves the orchestrator alone [auto]
- GIVEN `orchestrate` is `claude` / `opus`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the five tasks are `codex` / `default` and `orchestrate` is still `claude` / `opus`, and the aggregate ignores `orchestrate`
