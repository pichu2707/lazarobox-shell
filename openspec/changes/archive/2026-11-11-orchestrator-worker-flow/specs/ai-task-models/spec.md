# Delta for AI Task Models

Slice tag: [W8]. Decision I: `simple` joins the fixed task set outside `Set all tasks`, like `orchestrate`. The position of `simple` in the order is an assumption of this delta (appended after `orchestrate`, as `orchestrate` was appended after `summarize`); design confirms it.

## MODIFIED Requirements

### Requirement: Fixed task set [S1, W8]
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`. Task ids are lowercase and are the `config.toml` table names. There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).
(Previously: six tasks, `simple` did not exist)

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple` in that order, with unique ids

## ADDED Requirements

### Requirement: Simple fallback and use [W8]
`simple` follows Fallback to general: unassigned, it uses the `general` assignment and its row shows `(default)`, which MUST be visible. If the effective assignment is a cloud engine, the worker is not a cheap author, and no notice other than the visible `(default)` row is required, because the menu chose it. An orchestrator worker of kind `simple` MUST run on the effective `simple` assignment read at spawn time. The runtime MUST NOT pick another engine or model, MUST NOT escalate to the cloud, and MUST NOT write the config. The server engines are not marked `not recommended` for `simple`.

#### Scenario: Simple falls back [W8, auto]
- GIVEN `simple` unassigned and `general` is `claude` / `sonnet`
- WHEN a `simple` worker is spawned
- THEN it runs on claude / `sonnet` and the row reads `(default)`

#### Scenario: Simple on a local model [W8, auto]
- GIVEN `simple` is `ollama` / `llama3:8b` (available)
- WHEN a `simple` worker is spawned
- THEN it runs on ollama / `llama3:8b`, and no other engine is contacted

#### Scenario: Unusable simple engine [W8, auto]
- GIVEN `simple` is `ollama` with a model that is not downloaded
- WHEN a `simple` worker is spawned
- THEN it ends `refused` naming the model and `download required`, the assignments are unchanged and no other engine is contacted

### Requirement: Set all excludes Simple [W8]
`Set all tasks` MUST write, clear and summarize the five tasks `general`, `code`, `chat`, `review` and `summarize` only, and MUST NOT read or write `simple` (as for `orchestrate`).

#### Scenario: Set all leaves simple alone [W8, auto]
- GIVEN `simple` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the five tasks are `codex` / `default`, `simple` is still `ollama` / `llama3:8b`, and the aggregate ignores `simple`

## Stale statements to amend at apply or archive (not rewritten here)
- "Set-all summary" and "Set all excludes Orchestrate" stay true; their wording "the five tasks" holds.
- The `Runtime use of every task assignment [S1]` list of worker kinds (`general`, `code`, `chat`, `review`, `summarize`) lacks `simple`; add it to the list and add the scenario `Worker kind uses its own row` for `simple`.
