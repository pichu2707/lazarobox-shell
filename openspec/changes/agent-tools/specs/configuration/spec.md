# Delta for Configuration

All existing requirements stay as they are. The new keys follow the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`, per-key fallback, unknown keys ignored). In the notice order the `ai.tools` problems follow `ai.notify`, and the `ai.mcp` problems follow them. Tags as in the main spec; [design-confirm] = shape pinned by design.

## ADDED Requirements

### Requirement: Tools key
The file MAY hold a table `[ai.tools]` with `enabled` (boolean, default `true`). It is the off switch of all agent tools: the run action of pane-commands, the built-in tools, the lazarobox MCP server and the user MCP servers (see agent-tools). A wrong type falls back to `true` for that key only and reports `ai.tools.enabled: expected a boolean`; a non-table `ai.tools` reports `ai.tools: expected a table` and uses the default. Unknown keys under `ai.tools` MUST be ignored. The key is read at startup and when changed from the config menu; a change takes effect at the next turn, never on a stream or approval already running. No secret is part of the schema.

#### Scenario: Default on [auto]
- GIVEN a file with no `[ai.tools]`
- WHEN it is parsed
- THEN tools are on and no notice is produced

#### Scenario: Off [auto]
- GIVEN `[ai.tools]` with `enabled = false`
- WHEN it is parsed
- THEN tools are off and no notice is produced

#### Scenario: Wrong type [auto]
- GIVEN `[ai.tools]` with `enabled = "no"`
- WHEN the app starts
- THEN tools are on (the default) and the notice is `config: ai.tools.enabled: expected a boolean`

#### Scenario: Non-table [auto]
- GIVEN `[ai]` holding `tools = 3`
- WHEN the app starts
- THEN tools use the default and the notice is `config: ai.tools: expected a table`

#### Scenario: Counted with other problems [auto]
- GIVEN `enabled = 1` under `[ai.tools]` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[ai.tools]` with `mode = "ask"` next to `enabled = true`
- WHEN it is parsed
- THEN `enabled` applies and no notice is produced

### Requirement: MCP server lists
The file MAY hold an array of tables `[[ai.mcp.servers]]`, the global list of user MCP servers (see mcp-servers). Each entry has `name` (non-empty string of letters, digits, `-` and `_`, up to 32 characters, unique in the list), `command` (non-empty string, run directly without a shell), `args` (array of strings, default empty), `env` (array of environment variable NAMES to pass from lazarobox's own environment, default empty; values are never in the file) and `enabled` (boolean, default `true`). Per-project extras MUST be declared in this same user file, keyed by project root, in a shape pinned by design; no other file is ever read for servers. An invalid entry MUST be skipped as a whole and reported, the other entries kept: `ai.mcp.servers[N].name: expected a name of letters, digits, - and _`, `ai.mcp.servers[N].command: expected a non-empty string`, `ai.mcp.servers[N].args: expected an array of strings`, `ai.mcp.servers[N].env: expected an array of variable names`, `ai.mcp.servers[N].enabled: expected a boolean` (only `enabled` falls back alone), `ai.mcp.servers[N].name: duplicate name` (the later entry is skipped), `ai.mcp.servers: expected an array of tables` and `ai.mcp: expected a table`. Unknown keys inside an entry MUST be ignored. There are no `Authorization`, token or secret keys in the schema. The list is read once at startup (no hot reload). The menu has no row for it in this change: it is edited by hand.

#### Scenario: Defaults [auto]
- GIVEN a file with no `[ai.mcp]`
- WHEN it is parsed
- THEN there are no servers and no notice is produced

#### Scenario: Entry parsed [auto]
- GIVEN `[[ai.mcp.servers]]` with `name = "fs"`, `command = "mcp-fs"`, `args = ["--root", "/home/u/dev"]`, `env = ["GITHUB_TOKEN"]`
- WHEN it is parsed
- THEN one enabled server `fs` exists with those values and no notice is produced

#### Scenario: Invalid entry skipped, others kept [auto]
- GIVEN two entries where the first has `command = 5` and the second is valid
- WHEN the app starts
- THEN only the second is kept and the notice is `config: ai.mcp.servers[0].command: expected a non-empty string`

#### Scenario: Duplicate name [auto]
- GIVEN two entries named `fs`
- WHEN the app starts
- THEN the first is kept, the second is skipped and the notice is `config: ai.mcp.servers[1].name: duplicate name`

#### Scenario: Values never in the file [auto]
- GIVEN `env = { GITHUB_TOKEN = "abc" }`
- WHEN the app starts
- THEN the entry is skipped with the `env` problem and `abc` appears in no notice or log

#### Scenario: enabled false [auto]
- GIVEN an entry with `enabled = false`
- WHEN servers are resolved
- THEN the entry is not active and no process is started for it

#### Scenario: Wrong container types [auto]
- GIVEN `ai.mcp = 3`, and in another file `ai.mcp.servers = 3`
- WHEN each is parsed
- THEN the notices are `config: ai.mcp: expected a table` and `config: ai.mcp.servers: expected an array of tables`

#### Scenario: Unknown keys ignored [auto]
- GIVEN an entry with `timeout = 5` next to valid keys
- WHEN it is parsed
- THEN the valid keys apply and no notice is produced

#### Scenario: Project extras only from the user file [design-confirm]
- GIVEN a repository file that declares a server and a project entry in the user file for another root
- WHEN servers are resolved for a chat in the repository
- THEN only entries from the user file for that root are active
