# Delta for Agent Tools

## MODIFIED Requirements

### Requirement: Tool families
Every registry entry MUST carry exactly one `Family`: `Read`, `Search`, `Write`, `Exec`, `Git`, `GitWrite`, `Pane` or `Github`. `Git` holds the read verbs (status, diff, log), and `GitWrite` holds add and commit. `Github` holds the three GitHub read tools (see github-tools). The set of tools a session sees MUST be a filter over families. `Toolset::Code` MUST include `Read`, `Search`, `Write`, `Exec`, `Git` and `GitWrite`, and MUST NOT include `Pane`, `Github` or any external tool. A worker session and an orchestrator session MUST NOT see `Github`. The per-family toggles MUST apply as a second filter: a family whose toggle is off is not registered, offered or executed, even when a toolset includes it. `Github` is the one family that a chat can also switch on for itself, through a grant the user gives with `/github` (see github-tools, The chat grant); that grant is part of the filter and never changes the saved toggle.
(Previously: seven families, with no `Github` and no chat grant.)

#### Scenario: Code toolset [auto]
- GIVEN every toggle on
- WHEN the registry is filtered for `Toolset::Code`
- THEN it holds Read, Search, Write, Exec and the Git read tools, and no `run_in_pane`, no GitHub tool and no external tool

#### Scenario: Toggle is a second filter [auto]
- GIVEN `Toolset::Code` and the Exec toggle off
- WHEN the registry is filtered
- THEN `run_command` is absent

#### Scenario: Github family is separate [auto]
- GIVEN only the GitHub toggle on
- WHEN the registry is filtered for a chat
- THEN only the three GitHub tools are added to the default list, and `git_add` and `git_commit` are absent

## ADDED Requirements

### Requirement: GitHub reads and the approval model
The "every other tool call needs `y` every time" rule of Approval model does not apply to the GitHub tools. A GitHub read MUST run without asking on a local engine, and on a cloud engine MUST ask once per repository per turn (see github-tools, Cloud consent). In this family an Ollama model whose name ends in `-cloud` counts as a cloud engine, unlike for `read_file` and `list_panes`. The ask MUST obey the existing rules: one pending approval per chat, `denied` on a timeout, cancel, closed chat or quit, and no mode answers it.

#### Scenario: Local read is free, cloud read asks once [auto]
- GIVEN a local engine, and in another run a cloud engine, each with two GitHub calls in a turn
- WHEN the calls are dispatched
- THEN the local calls run with no prompt, and the cloud engine asks once before the first and not before the second

### Requirement: Reads in a blocked conversation
Once a conversation is blocked (see pane-execution, Auto-run block after a GitHub read), on a cloud engine every `read_file` and `grep` call of that conversation MUST ask for the user's `y` each time, in every approval mode (`ask`, `auto`, `run`), including a path inside the chat's root that would normally run free. Here an Ollama model whose name ends in `-cloud` counts as a cloud engine, because the data leaves the machine; this extends the `-cloud` rule of the GitHub tools to these two tools, only while blocked, and changes nothing for a conversation that is not blocked. Local engines MUST be unchanged. The prompt MUST show the reason, `this conversation read GitHub: reads of your files need your y`. The rule applies from the call after the block starts, including later rounds of the same turn, and to a resumed blocked conversation. One `y` answers one call. A call the path guard refuses still returns its refusal without a prompt, as today. Tools that return only names (`find_dir`, `list_dir`, `glob`) are not changed by this requirement. Known limit: a worker of the orchestrator is not covered when its read stays inside its run's scope or when it calls `grep`; those never ask on a cloud engine, blocked or not, because the host builds no ask for them and has no view of the parent conversation's block. A worker read outside its scope does ask while the parent is blocked, with the same reason.

#### Scenario: Run mode asks for an in-root read [auto]
- GIVEN a blocked conversation on a `claude` engine in `run` mode
- WHEN the model calls `read_file` on a file inside the root, and then `grep` in the root
- THEN each opens a prompt with the reason and nothing is read until `y`

#### Scenario: Ask and auto mode ask each time [auto]
- GIVEN a blocked conversation on a cloud engine in `ask`, and in another run `auto`
- WHEN the model calls `read_file` twice on the same file
- THEN each call opens its own prompt

#### Scenario: Ollama cloud model asks while blocked [auto]
- GIVEN a blocked conversation on the Ollama engine with `gpt-oss:120b-cloud` in `run` mode
- WHEN the model calls `read_file` inside the root
- THEN a prompt opens, and in a conversation that is not blocked the same call runs free

#### Scenario: Local engine unchanged [auto]
- GIVEN a blocked conversation on a local engine
- WHEN the model calls `read_file` and `grep` inside the root
- THEN they run under the normal rules with no new prompt

#### Scenario: Resume keeps it [auto]
- GIVEN a conversation that read GitHub text, closed, and resumed on a cloud engine in `run` mode
- WHEN the model calls `read_file` inside the root
- THEN the prompt opens with the reason

#### Scenario: Ollama cloud model differs by tool [auto]
- GIVEN the Ollama engine with a model ending in `-cloud`
- WHEN `list_panes` and then a GitHub tool are called
- THEN `list_panes` runs without asking and the GitHub tool asks
