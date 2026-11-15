# Delta for AI Task Models

## Base of this delta

This delta targets `openspec/specs/ai-task-models/spec.md`. MODIFIED blocks are full copies with edits marked "(Previously: ...)". All requirements here are slice [S3]. Settled: J4 (menu rows), J10 (the second judge), J11 (default model) and J17 (the judges are the two rows `judge-a` and `judge-b` of the `ODD + RDD roles` section of #765; `Set all tasks` does not include them, `Set all ODD + RDD roles` does).

Because the judges stay out of `Set all tasks`, the five-task counts of "Set-all summary", "Set all excludes Orchestrate" and "Set all excludes Simple" of the base remain true and this delta does not modify them (an earlier draft counted six). This slice covers the judges as the first roles of the section; the other roles of #765 are a later change and do not change anything stated here.

## MODIFIED Requirements

### Requirement: Fixed task set [S1, W8, S3]
The tasks MUST be fixed and ordered: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge-a`, `judge-b`. Task ids are lowercase and are the `config.toml` table names (`judge-a` and `judge-b` hold a hyphen, a valid bare TOML key). There is no way to add, rename or remove a task. A task assignment is an engine id plus a model name. A task is "assigned" only when its engine is non-empty; a model without an engine is treated as unassigned (the file keeps it).
(Previously: seven tasks, the judges did not exist)

#### Scenario: Task set integrity [auto]
- GIVEN the task table
- WHEN it is validated
- THEN it has exactly `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge-a`, `judge-b` in that order, with unique ids

### Requirement: Runtime use of every task assignment [S1, S3]
Each task MUST be used at runtime through its effective assignment (own assignment, else `general`; see Fallback to general; the judges use the fallbacks of Judge fallback and use), read from the current configuration on every use, so a saved change applies from the next message or the next worker spawn. A chat turn uses `chat`, except in an orchestrating pane (global gate on and the pane's ORCH mode on), where the turn uses `orchestrate`; each orchestrator worker uses the assignment of its kind (`general`, `code`, `chat`, `review`, `summarize`, `simple`), and each acceptance judge uses `judge-a` (the first judge) or `judge-b` (the second judge of a pair), see `acceptance-judges`. The model MUST be validated against detection before use: for a server engine the model MUST be non-empty and among the detected models (exact match, with the Ollama tag-less equals `:latest` rule); otherwise the chat shows the model guidance (or the worker is `refused`) and nothing is sent. For a CLI engine the model is not validated (empty and `default` mean the CLI's own default). The runtime MUST NOT change any assignment on its own, MUST NOT pick another engine when the effective one is unusable, and MUST NOT write the config; only a user choice in the menu changes an assignment. The picker opened from a worker failure alert does not change any assignment: its choice is a run-scoped override (see agent-orchestrator) that is never written to the draft or the file.
(Previously: the list of worker kinds had no judge)

Existing scenarios stay unchanged. Added scenario:

#### Scenario: Judge uses its effective assignment [S3, auto]
- GIVEN `judge-a` is `ollama` / `qwen3:14b` (available) and `general` is `claude` / `sonnet`
- WHEN the first judge worker is spawned
- THEN it runs on ollama with `qwen3:14b`, and an unusable model ends it `refused` with the model named and nothing sent elsewhere

## ADDED Requirements

### Requirement: Judge fallback and use [S3]
The judges have their own fallback, which MUST be visible as `(default)` on an unassigned row. `judge-a` unassigned uses the `review` assignment when `review` is assigned, otherwise the run's default model (the `general` assignment). `judge-b` unassigned uses the `review` assignment when `review` is assigned and its resolved engine and model differ from those of the effective `judge-a`; otherwise it uses the effective `judge-a` and a visible line MUST say `judge: judges are not independent · both on <engine › model>` (J10; the same line also shows when an own `judge-b` resolves to the same engine and model as the effective `judge-a`). An acceptance judge MUST run on its effective assignment read at spawn time. The runtime MUST NOT pick another engine or model, MUST NOT escalate to the cloud and MUST NOT write the config. The server engines are not marked `not recommended` for the judges: the limits of local judges are said by the run-time notices instead.

#### Scenario: judge-a falls back to review, then to the default [S3, auto]
- GIVEN `judge-a` unassigned, `review` is `codex` / `default` and `general` is `claude` / `sonnet`
- WHEN the first judge is spawned
- THEN it runs on codex / `default`, and with `review` also unassigned it runs on claude / `sonnet`; the row reads `(default)` in both cases

#### Scenario: judge-b falls back to review when different [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`, `judge-b` unassigned and `review` is `codex` / `default`
- WHEN two judges are spawned
- THEN judge 2 runs on codex / `default` and no not-independent line is shown

#### Scenario: judge-b falls back to judge-a, visibly [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`, `judge-b` unassigned and `review` unassigned (or equal to `judge-a`)
- WHEN two judges are spawned
- THEN both run on claude / `opus` and the transcript shows `judge: judges are not independent · both on claude › opus`

#### Scenario: Own judge-b wins [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`, `judge-b` is `ollama` / `llama3:8b` (available) and `review` is `codex` / `default`
- WHEN two judges are spawned
- THEN judge 2 runs on ollama / `llama3:8b` and no other engine is contacted

#### Scenario: Unusable judge engine [S3, auto]
- GIVEN `judge-b` is `ollama` with a model that is not downloaded
- WHEN the second judge is spawned
- THEN it ends `refused` naming the model and `download required`, the assignments are unchanged and no other engine is contacted

#### Scenario: Nothing assigned labels [S3, auto]
- GIVEN no task is assigned
- WHEN the labels are built
- THEN `general` reads `(not set)` and every other task, `judge-a` and `judge-b` included, reads `(default)`

### Requirement: Set all ODD + RDD roles and the judges [S3]
`Set all ODD + RDD roles` MUST write, clear and summarize the roles of the section `ODD + RDD roles` only; in this slice they are `judge-a` and `judge-b`. Its aggregate MUST be derived from the EFFECTIVE (resolved) assignments of those roles: when all resolve to the same value it shows that value (`(not set)` when all resolve to nothing, otherwise `<engine> › <model>`); otherwise it shows `(mixed)`. `Set all ODD + RDD roles` MUST NOT read or write `general`, `code`, `chat`, `review`, `summarize`, `orchestrate` or `simple`, and `Set all tasks` MUST NOT read or write `judge-a` or `judge-b` (the same exclusion as for `orchestrate` and `simple`). Choosing `none` clears both judges. Roles added to the section by a later change join this set without changing it for the judges.

#### Scenario: Roles aggregate values [auto]
- GIVEN (a) both judges `claude` / `opus`, (b) no task assigned, `general` unset, (c) `judge-a` `claude` / `opus` and `judge-b` unassigned with `review` unassigned, (d) `judge-a` `claude` / `opus` and `judge-b` `ollama` / `llama3:8b`
- WHEN the roles aggregate is computed
- THEN it reads `claude › opus`, `(not set)`, `claude › opus` (`judge-b` resolves to `judge-a`) and `(mixed)` respectively

#### Scenario: Set all roles leaves the tasks alone [auto]
- GIVEN `orchestrate` is `claude` / `opus` and `review` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for `Set all ODD + RDD roles`
- THEN `judge-a` and `judge-b` are `codex` / `default`, and every task keeps its value

#### Scenario: Set all tasks leaves the judges alone [auto]
- GIVEN `judge-b` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN the five Set-all tasks are `codex` / `default`, `judge-b` is still `ollama` / `llama3:8b`, and the tasks aggregate ignores the judges
