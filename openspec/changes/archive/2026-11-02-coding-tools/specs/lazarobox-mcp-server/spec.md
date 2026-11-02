# Delta for lazarobox MCP Server

## Base of this delta

`openspec/specs/` does not hold `lazarobox-mcp-server` (checked). It exists only in the unarchived change `openspec/changes/agent-tools/specs/lazarobox-mcp-server/spec.md`. This delta targets that file, as it will stand when `agent-tools` is archived, and the MODIFIED block is a full copy of its requirement, edited. `agent-tools` MUST be archived before `coding-tools`. The `agent-tools` change is not modified. Other requirements (Shim process, Socket and session token, Approval round trip, Jail and process rights, Per-engine injection) stay as they are. The shim still starts no process and the CLI jail is not widened: the tools below run in the app.

## ADDED Requirements

### Requirement: Coding tools run in the app
Every coding tool (search, write, run, git) MUST be executed by the app, never by the shim. A `run_command` or git child MUST be started by the app inside its own jail profile (see command-runner), not inside the CLI jail. The CLI jail spec MUST be identical with and without coding tools. A call that blocks on a review or a prompt MUST end as `denied` when the CLI's own call timeout expires, and a staged batch MUST then stay as an orphan (see code-write).

#### Scenario: Shim starts nothing [auto]
- GIVEN a shim under a syscall-recording harness and `run_command` served
- WHEN the call is approved and runs
- THEN the shim started no process and the app started the child

#### Scenario: CLI jail unchanged [auto]
- GIVEN the CLI jail spec with coding tools on, and with them off
- WHEN both are built
- THEN they are identical

#### Scenario: Long review outlives the CLI timeout [auto]
- GIVEN a fake CLI whose call timeout is shorter than the user's review
- WHEN the timeout fires
- THEN the propose call returns `denied`, the review is withdrawn and the batch stays staged

## MODIFIED Requirements

### Requirement: Tools served
The shim MUST serve only the tools of the registry that apply to the session: the three read tools, `run_in_pane`, the user MCP tools when configured (see mcp-servers), and the tools of each coding-tools family whose toggle is on (search, write, run, git). With tools off no shim is started, and a family with its toggle off MUST NOT be listed or served. The engine-side permission MUST allow every lazarobox tool, because lazarobox is the policy point: `find_dir`, `list_dir` and `glob` run freely, and `read_file`, `grep`, `run_command` and the git and write tools wait for the user's `y` or review inside lazarobox as their specs say (every CLI is a cloud engine, see agent-tools), except `run_in_pane` in a chat in `auto` mode, which is typed without a prompt (see agent-tools, Chat approval mode). Write tools MUST be described as having side effects. A `tools/call` MUST be executed by the app under the registry rules (path policy, caps, validation) and the result MUST be returned as MCP content labeled as untrusted data (see agent-tools). Tool events MUST be emitted to the chat for every call.
(Previously: only the three read tools, `run_in_pane` and user MCP tools were served; `write_file` was an unknown tool.)

#### Scenario: Only registry tools [auto]
- GIVEN a session with every toggle off
- WHEN `tools/call` names `bash` or `write_file`
- THEN an error result is returned and nothing runs

#### Scenario: Family served when on [auto]
- GIVEN the Write toggle on
- WHEN `tools/list` is read and `write_file` is called
- THEN `write_file` is listed and the call stages a change without touching the disk

#### Scenario: Unknown tool still refused [auto]
- GIVEN every toggle on
- WHEN `tools/call` names `bash` or `delete_file`
- THEN an error result is returned and nothing runs

#### Scenario: read_file asks through MCP [auto]
- GIVEN a `read_file` call through the shim of any CLI
- WHEN it is served
- THEN an approval naming the path is pending and the file is not opened until `y`; `find_dir` and `list_dir` through the shim need none

#### Scenario: Policy applies through MCP [auto]
- GIVEN a call to `read_file` on `~/.ssh/id_ed25519` through the shim
- WHEN it is served
- THEN the same refusal as in the registry is returned

#### Scenario: Events emitted [auto]
- GIVEN a `find_dir` call through the shim
- WHEN it completes
- THEN the chat receives a `ToolCall` and a `ToolResult`

#### Scenario: Tools off [auto]
- GIVEN tools off
- WHEN a turn starts
- THEN no shim, socket or token is created and the CLI is started exactly as before
