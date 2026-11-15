# Delta for Configuration

## Base of this delta

This delta targets the owning spec `openspec/specs/configuration/spec.md` and exists only for slice S9 (per-server approval level, C13; owner decision 2026-11-14). Before S9 the proposal said "Not modified: `configuration` (the `[[ai.mcp.servers]]` schema stays, C5)"; the owner lifted that for exactly one key, the optional `approval`. The MODIFIED block below is a full copy of "MCP server lists" (openspec/specs/configuration/spec.md:440-486 on origin/main 3b9548c2) with these edits only: the entry gains `approval`; one problem text and two scenarios are added; the last sentence names the level edit as a writer. In code, the entry is parsed at src/core/ai/config.rs:1226-1233 (`enabled`, the model for `approval`: a wrong value is reported and falls back alone) and the known keys of an entry are the array at :1594. A build without this change reports `config: unknown key: ai.mcp.servers[N].approval` for the key (owner accepted that, 2026-11-14).

Traceability: C13, proposal open question 10 (resolved), Out of Scope amendment of C5.

## MODIFIED Requirements

### Requirement: MCP server lists
The file MAY hold an array of tables `[[ai.mcp.servers]]`, the global list of user MCP servers (see mcp-servers). Each entry has `name` (lowercase letters, digits and `-`, 1 to 24 characters, unique in its scope: the global list, or one project root; it is also the server part of a qualified tool name), `command` (non-empty string, run directly without a shell), `args` (array of strings, default empty), `env` (array of environment variable NAMES to pass from lazarobox's own environment, default empty, each an identifier of at most 64 bytes; values are never in the file), `enabled` (boolean, default `true`) and, [S9] `approval` (string, one of `ask`, `reads` or `all`, case-sensitive, default `ask`: the entry's approval level, see mcp-servers, Approval level per server). Per-project extras MUST be declared in this same user file, keyed by project root, in a shape pinned by design; no other file is ever read for servers. An invalid entry MUST be skipped as a whole and reported, the other entries kept: `ai.mcp.servers[N].name: expected a name of lowercase letters, digits and - (1 to 24 characters)`, `ai.mcp.servers[N].command: expected a non-empty string`, `ai.mcp.servers[N].args: expected an array of strings`, `ai.mcp.servers[N].env: expected an array of variable names`, `ai.mcp.servers[N].enabled: expected a boolean` (only `enabled` falls back alone), [S9] `ai.mcp.servers[N].approval: expected "ask", "reads" or "all"` (the entry is kept and only `approval` falls back, to `ask`; the notice is never omitted), `ai.mcp.servers[N].name: duplicate name` (the later entry is skipped), `ai.mcp.servers: expected an array of tables` and `ai.mcp: expected a table`. Unknown keys inside an entry MUST be ignored, the entry kept, and each one is reported as `unknown key: ai.mcp.servers[N].<key>` (see Unknown keys in owned tables); `approval` is a known key and MUST NOT be reported as unknown. There are no `Authorization`, token or secret keys in the schema. The list, levels included, is read once at startup (no hot reload). It is edited by hand; the only other writers are Connect Google (config-menu, Connect Google), which, after the user's `y` on its plan, appends the catalog entries of `gsc` and `google` that are not declared at the end of the file, keeps the old file as `config.toml.bak`, and never changes or removes an entry, and [S9] the Connectors area's approval-level change (mcp-connect, Approval level of an entry), which, after the user's confirmation, sets the `approval` key of one declared entry and changes nothing else.
(Previously: entries had no `approval` key, so it would have been reported as unknown, and Connect Google was the only writer besides the user.)

#### Scenario: Defaults [auto]
- GIVEN a file with no `[ai.mcp]`
- WHEN it is parsed
- THEN there are no servers and no notice is produced

#### Scenario: Entry parsed [auto]
- GIVEN `[[ai.mcp.servers]]` with `name = "fs"`, `command = "mcp-fs"`, `args = ["--root", "/home/u/dev"]`, `env = ["GITHUB_TOKEN"]`
- WHEN it is parsed
- THEN one enabled server `fs` exists with those values, level `ask`, and no notice is produced

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
- THEN the valid keys apply, the entry is kept and the notice is `config: unknown key: ai.mcp.servers[0].timeout`

#### Scenario: Project extras only from the user file [design-confirm]
- GIVEN a repository file that declares a server and a project entry in the user file for another root
- WHEN servers are resolved for a chat in the repository
- THEN only entries from the user file for that root are active

#### Scenario: approval missing means ask [S9, auto]
- GIVEN an entry with no `approval` key
- WHEN it is parsed
- THEN its level is `ask` and no notice is produced

#### Scenario: Each valid level parses [S9, auto]
- GIVEN three entries with `approval = "ask"`, `"reads"` and `"all"`
- WHEN they are parsed
- THEN the levels are `ask`, `reads` and `all`, no notice is produced and `approval` is not reported as an unknown key

#### Scenario: Invalid approval value gives a notice and asks [S9, auto]
- GIVEN an entry `fs` with `approval = "always"`, in another file `approval = "Reads"`, `approval = true` and `approval = ""`
- WHEN each is parsed
- THEN each entry is kept, its level is `ask`, and the notice is `config: ai.mcp.servers[0].approval: expected "ask", "reads" or "all"`

#### Scenario: A bad approval does not drop the entry or its siblings [S9, auto]
- GIVEN two entries where the first has `approval = 3` and the second is valid at `approval = "all"`
- WHEN the app starts
- THEN both servers are active, the first at `ask` and the second at `all`, and the only problem is the first's `approval`
