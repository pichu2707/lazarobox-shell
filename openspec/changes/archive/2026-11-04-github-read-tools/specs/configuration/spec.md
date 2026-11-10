# Delta for Configuration

## MODIFIED Requirements

### Requirement: Family toggle keys
The table `[ai.tools]` MAY hold six booleans: `search`, `write`, `exec`, `git`, `git_write` and `github` (provisional names). Each defaults to `false`. They control the families Search, Write, Exec, Git read verbs, Git write verbs and the GitHub read tools (see agent-tools, Tool families). A family is active only when `[ai.tools] enabled` is also `true`. A wrong type falls back to `false` for that key only and reports `ai.tools.<key>: expected a boolean`, after the `ai.tools.approval` problem in the notice order. A non-table `ai.tools` uses the defaults with the existing `ai.tools: expected a table` problem only. Unknown keys MUST be ignored, so a build without `github` reads a file that has it. A change takes effect at the next turn, never on a stream or approval already running. A chat grant (see github-tools) is never written to this table.
(Previously: five booleans, with no `github`.)

#### Scenario: Defaults off [auto]
- GIVEN a file with no family keys, and a file with no `[ai.tools]`
- WHEN each is parsed
- THEN all six toggles are off and no notice is produced

#### Scenario: On [auto]
- GIVEN `[ai.tools]` with `write = true` and `exec = true`
- WHEN it is parsed
- THEN Write and Exec are on and the other four are off

#### Scenario: GitHub on [auto]
- GIVEN `[ai.tools]` with `github = true`
- WHEN it is parsed
- THEN GitHub is on and the other five are off

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `git = "yes"`, and in another run `github = "yes"`
- WHEN the app starts
- THEN that key is off and the notice is `config: ai.tools.git: expected a boolean`, or `config: ai.tools.github: expected a boolean`

#### Scenario: Master switch wins [auto]
- GIVEN `enabled = false` and `write = true`, and in another run `enabled = false` and `github = true`
- WHEN the tool list is built
- THEN no tool is offered

#### Scenario: Git toggles are independent [auto]
- GIVEN `git = false` and `git_write = true`
- WHEN the tool list is built
- THEN only `git_add` and `git_commit` are offered among the git tools

#### Scenario: Old file with no key [auto]
- GIVEN a file written before this change
- WHEN it is parsed and the menu saves other keys
- THEN `github` is off, no notice is produced and the file gains no `github` key unless the user changes the row

#### Scenario: Grant never writes the file [auto]
- GIVEN a chat granted with `/github`
- WHEN the config is saved by the menu
- THEN `github` is unchanged in the file
