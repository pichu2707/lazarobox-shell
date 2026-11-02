# Delta for lazarobox-mcp-server

DEPENDS ON: change `agent-tools` being archived first. The `lazarobox-mcp-server` capability is not yet in `openspec/specs/`; this delta has ADDED requirements only and must be applied after that archive. Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Socket and token per session key [S3]
Each session key (pane, or run and worker) MUST get its own socket path and session token, removed when the session ends, with the same rights as a chat session. A token MUST open only its own session. [S1] A thin wrapper may stand in for the key until S3.

#### Scenario: Own socket per worker [auto]
- GIVEN two workers
- WHEN their sockets are created
- THEN paths and tokens differ and both are removed when the workers end

### Requirement: Tools served by role [S1]
The server MUST list only the tools of the session's role (see agent-tools). A `tools/call` for a tool outside the role MUST return an error result and perform nothing.

#### Scenario: Out-of-role call [auto]
- GIVEN a worker session
- WHEN `spawn_agent` is called
- THEN an error result is returned and no worker is created

### Requirement: Async orchestration calls within the MCP timeout [S1]
`spawn_agent`, `collect` and `cancel` MUST return promptly and `wait` MUST return within 120 seconds, so no call approaches the 900 s MCP timeout. A held call (for example `wait` during an unresolved failure) MUST still end within that bound.

#### Scenario: Bound held [auto]
- GIVEN a failure unresolved and `wait` called
- WHEN time is advanced
- THEN `wait` returns `awaiting_user` within 120 s

### Requirement: Approval round trip carries the worker label [S1, S3]
An approval request from a worker session MUST reach the app with the worker label and be answered only from a user key event; a closed session MUST end its approvals as `denied`.

#### Scenario: Label travels [auto]
- GIVEN a worker `read_file` that needs approval
- WHEN the request reaches the app
- THEN it carries kind and `engine>model`

#### Scenario: Closed session denies [auto]
- GIVEN a pending approval and the worker cancelled
- WHEN the session closes
- THEN the approval ends `denied`
