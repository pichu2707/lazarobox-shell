# Delta for Agent Tools

Slice tags as in the agent-orchestrator delta of this change (W1..W10).

## MODIFIED Requirements

### Requirement: Toolsets by session role [S1, W2, W6, W8]
The tool registry MUST offer by role: chat session = chat tools as before; orchestrator session = chat read tools plus `spawn_agent`, `wait`, `collect`, `cancel` ([S4] and `finish`), plus the tools of the user's MCP servers, each call asking `y` exactly as in a chat (user decision, 2026-10-29), and never `run_in_pane` and never write tools or `run_command` (the coordinator keeps Read plus Orchestra whatever the toggles say); worker session = read tools for every kind, plus the coding-tools write tools for a `code` or `simple` worker, plus `run_command` for a `code` or `simple` worker ([W6]), never `run_in_pane`, never user server tools, never orchestration tools. [W2] The Write and Exec toggles are a per-role ceiling: a write tool is offered to a worker only when Write is on, `run_command` only when Exec is on, and a toggle never adds a tool the role does not have (a `review` worker stays read-only with both on). Orchestration tools MUST NOT be reachable from user MCP servers, from a chat pane whose ORCH mode is off, or while the global orchestrator gate is off. The orchestrator session exists only for a turn of an orchestrating pane.
(Previously: "worker session = read tools only (until write tools exist)"; the menu toggles did not limit workers; no worker could be offered `run_command`)

#### Scenario: Role toolsets [auto]
- GIVEN each role
- WHEN the tool list is built
- THEN it matches the role definition exactly

#### Scenario: Orchestrator user server tool asks [auto]
- GIVEN an orchestrator turn and a user MCP server with tool `search`
- WHEN the tool list is built and `search` is called
- THEN `search` is listed beside the orchestration tools, the call asks `y`, and the server is contacted only after `y`; a worker's call to it is refused unasked

#### Scenario: Ceiling limits a writer [W2, auto]
- GIVEN Write off and Exec off
- WHEN the tool lists of a `code` worker, a `simple` worker and a `review` worker are built
- THEN each holds the read tools only

#### Scenario: Ceiling never widens [W2, auto]
- GIVEN Write and Exec on
- WHEN the tool lists of a `review` worker and of the orchestrator are built
- THEN the `review` worker holds the read tools only and the orchestrator holds no write tool and no `run_command`

#### Scenario: A forged call is refused visibly [W2, auto]
- GIVEN a worker without `run_command` because Exec is off
- WHEN it calls `run_command` anyway
- THEN the call returns an error result naming the cause, no ask opens and nothing starts
