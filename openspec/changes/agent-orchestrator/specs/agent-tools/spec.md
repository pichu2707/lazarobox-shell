# Delta for agent-tools

DEPENDS ON: change `agent-tools` being archived first. Its `agent-tools` capability is not yet in `openspec/specs/`, so this delta contains only ADDED requirements (nothing is copied or replaced) and must be applied to the main spec after that archive. Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Sessions are keyed per run and worker [S3]
Tool sessions, tokens, scopes and pending approvals MUST be keyed by a session key that is either a chat pane or a (run, worker) pair, not only by pane. Two sessions MUST NOT see each other's tools, scope, approvals or audit. (This relaxes the "one pending approval per chat" rule for worker sessions only; see the approval queue below.)

#### Scenario: Cross-session call refused [auto]
- GIVEN two worker sessions
- WHEN one calls with the other's token
- THEN the call is refused

### Requirement: Toolsets by session role [S1]
The tool registry MUST offer by role: chat session = chat tools as before; orchestrator session = chat read tools plus `spawn_agent`, `wait`, `collect`, `cancel` ([S4] and `finish`) and never `run_in_pane`; worker session = read tools only (until write tools exist), never `run_in_pane`, never orchestration tools. Orchestration tools MUST NOT be reachable from user MCP servers, from a chat pane whose ORCH mode is off, or while the global orchestrator gate is off. The orchestrator session exists only for a turn of an orchestrating pane.

#### Scenario: Role toolsets [auto]
- GIVEN each role
- WHEN the tool list is built
- THEN it matches the role definition exactly

### Requirement: Pre-approved read scope [S1]
A session MAY carry a run-scoped read scope (its roots default to the chat origin folder). `read_file` for a path inside it MUST run without asking, also on a cloud engine; a worker read of a path outside it MUST follow the chat's approval mode exactly as a chat read does, and when it asks, it asks with the worker label (an empty or expired scope grants nothing, so every read follows the mode). The scope MUST be evaluated with the Path policy on both the requested and the resolved path; an excluded path is refused whatever the scope, and no scope entry may make an excluded path readable. `find_dir` and `list_dir` keep their rules.

#### Scenario: Inside runs, outside asks [auto]
- GIVEN a scope `~/dev/app`, a cloud engine and the `ask` mode
- WHEN `read_file` is called on a file inside and on one outside
- THEN the first runs with no approval and the second asks

#### Scenario: Excluded under scope [auto]
- GIVEN a scope that contains `.env`
- WHEN `read_file` is called on it
- THEN it is refused with the one identical refusal message

### Requirement: Approval queue for workers [S3]
Approvals raised by worker sessions MUST be queued FIFO with a worker label, one visible at a time, answered by user key events only; cancelling a session MUST deny its pending and queued approvals. A timer MUST never approve.

#### Scenario: FIFO order [auto]
- GIVEN asks A then B from different workers
- WHEN the user answers twice
- THEN A is shown and answered first, then B

### Requirement: Orchestration results are untrusted data [S1]
`collect` output and orchestration error text MUST be wrapped with the nonce wrapper of Tool output is untrusted data.

#### Scenario: Wrapped [auto]
- GIVEN a collect result
- WHEN it is returned to the orchestrator
- THEN it is inside the wrapper
