# Delta for Lazarobox MCP Server

## MODIFIED Requirements

### Requirement: Tools served
The shim MUST serve only the tools of the registry that apply to the session: the three read tools, `run_in_pane`, `list_panes`, `read_pane`, the user MCP tools when configured (see mcp-servers), and the tools of each coding-tools family whose toggle is on (search, write, run, git). With tools off no shim is started, and a family with its toggle off MUST NOT be listed or served. The engine-side permission MUST allow every lazarobox tool, because lazarobox is the policy point: `find_dir`, `list_dir` and `glob` run freely, and `read_file`, `grep`, `run_command`, `read_pane` and the git and write tools wait for the user's `y` or review inside lazarobox as their specs say (every CLI is a cloud engine, see agent-tools), except `run_in_pane` in a chat in `auto` mode, which is typed without a prompt (see agent-tools, Chat approval mode). `read_pane` MUST ask in every approval mode. Write tools MUST be described as having side effects. A `tools/call` MUST be executed by the app under the registry rules (path policy, caps, validation) and the result MUST be returned as MCP content labeled as untrusted data (see agent-tools). Tool events MUST be emitted to the chat for every call. On the `codex` engine the injection MUST pass our calls on with `default_tools_approval_mode = "approve"` for the `lazarobox` server only (the key needs codex 0.125 or later), so that codex does not refuse them itself and every call reaches lazarobox's own approval, which is unchanged.
(Previously: `read_pane` was not served.)

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

#### Scenario: read_pane served and asked [auto]
- GIVEN a chat session of any CLI engine in `run` mode
- WHEN `tools/list` is read and `read_pane` is called on another pane
- THEN `read_pane` is listed, the approval shows the redacted text, and the result is returned only after `y`, wrapped as untrusted

#### Scenario: read_pane not served to a worker [auto]
- GIVEN a worker or orchestrator session
- WHEN `tools/list` is read
- THEN `read_pane` is absent
