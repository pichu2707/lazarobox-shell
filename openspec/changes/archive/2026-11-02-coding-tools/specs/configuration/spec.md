# Delta for Configuration

## Base of this delta

`openspec/specs/configuration/spec.md` exists. The `[ai.tools]` table is itself added by the unarchived `openspec/changes/agent-tools/specs/configuration/spec.md`; the keys below extend it, so `agent-tools` MUST be archived first. Only ADDED requirements are used, so no existing requirement is replaced. Key names are provisional and pinned by design.

## ADDED Requirements

### Requirement: Family toggle keys
The table `[ai.tools]` MAY hold five booleans: `search`, `write`, `exec`, `git` and `git_write` (provisional names). Each defaults to `false`. They control the families Search, Write, Exec, Git read verbs and Git write verbs (see agent-tools, Tool families). A family is active only when `[ai.tools] enabled` is also `true`. A wrong type falls back to `false` for that key only and reports `ai.tools.<key>: expected a boolean`, after the `ai.tools.approval` problem in the notice order. A non-table `ai.tools` uses the defaults with the existing `ai.tools: expected a table` problem only. Unknown keys MUST be ignored. A change takes effect at the next turn, never on a stream or approval already running.

#### Scenario: Defaults off [auto]
- GIVEN a file with no family keys, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN all five toggles are off and no notice is produced

#### Scenario: On [auto]
- GIVEN `[ai.tools]` with `write = true` and `exec = true`
- WHEN it is parsed
- THEN Write and Exec are on and the other three are off

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `git = "yes"`
- WHEN the app starts
- THEN `git` is off and the notice is `config: ai.tools.git: expected a boolean`

#### Scenario: Master switch wins [auto]
- GIVEN `enabled = false` and `write = true`
- WHEN the tool list is built
- THEN no tool is offered

#### Scenario: Git toggles are independent [auto]
- GIVEN `git = false` and `git_write = true`
- WHEN the tool list is built
- THEN only `git_add` and `git_commit` are offered among the git tools
