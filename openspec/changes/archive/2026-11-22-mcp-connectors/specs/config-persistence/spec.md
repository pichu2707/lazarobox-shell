# Delta for Config Persistence

## Base of this delta

This delta targets the owning spec `openspec/specs/config-persistence/spec.md` and exists only for slice S9 (per-server approval level, C13; owner decision 2026-11-14: "the menu writes this key"). The MODIFIED block is a full copy of "MCP server lists are never changed" (openspec/specs/config-persistence/spec.md:494-520 on origin/main 3b9548c2). The save (`s`) is NOT changed: it still never touches `ai.mcp.*`. What changes is the list of writers outside the save: besides Connect Google's append, the Connectors area may set the `approval` key of one declared entry, after the user's confirmation, on the same safety model (handles, size bound, bytes compared with the ones shown, `config.toml.bak` first, mode kept, atomic replace, result parsed again; src/core/config_append.rs:1-6, 77-110; src/runtime/config_append.rs:1-8, 17-45). See mcp-connect, Approval level of an entry, for the rules of that write.

Traceability: C4 (the save never writes `ai.mcp.*`), C13.

## MODIFIED Requirements

### Requirement: MCP server lists are never changed
The save MUST NEVER add, change, reorder or remove any `ai.mcp.*` key, the global `[[ai.mcp.servers]]` entries and the per-project extras included: they are edited by hand. Every byte of them, including arrays of tables, comments, unknown keys and invalid entries, MUST be preserved by every save, and a collision or an error inside them MUST NOT block the save of other keys. No secret is ever written, and no environment value is ever written. The only writes of the server list are not a save. First: Connect Google (config-menu, Connect Google; openspec change `google-connect`) MAY append, after the user's `y` on its plan, the catalog entries of `gsc` and `google` that are not declared, at the end of `config.toml`. That append MUST keep the old bytes as a prefix (adding at most a line break and one empty line before the entries), MUST write `config.toml.bak` with the old bytes first, MUST NEVER change, reorder or remove an existing entry, and MUST be refused, writing nothing, unless the result parses with every old server unchanged followed by exactly the new ones. Second [S9]: the Connectors area MAY set the `approval` key of one declared entry (mcp-connect, Approval level of an entry), only after the user's confirmation in the menu; it MUST change at most that one key of that one entry, MUST write `config.toml.bak` with the old bytes first, MUST keep every other byte, and MUST be refused, writing nothing, unless the result parses with every server unchanged except the level of that entry.
(Previously: Connect Google's append was the only write of the server list outside the hand edit.)

#### Scenario: Servers preserved [auto]
- GIVEN a file with two `[[ai.mcp.servers]]` entries, comments between them, and a changed statusline position
- WHEN the save succeeds
- THEN only the position changes and the server entries are byte-identical

#### Scenario: Invalid entries do not block [auto]
- GIVEN `ai.mcp.servers` holding invalid entries and a changed tools draft
- WHEN the save succeeds
- THEN the tools key is saved and the entries are byte-identical

#### Scenario: Nothing created [auto]
- GIVEN a file with no `ai.mcp`
- WHEN any save succeeds
- THEN the file gains no `ai.mcp` table

#### Scenario: Connect Google appends and keeps every old byte [google-connect, auto]
- GIVEN a file with comments and one `[[ai.mcp.servers]]` entry named `docs`
- WHEN Connect Google writes a plan that appends `gsc` and `google`
- THEN the old bytes are a prefix of the new file, `config.toml.bak` holds the old bytes, and the parsed servers are `docs`, `gsc`, `google`

#### Scenario: An inline server list is never appended to [google-connect, auto]
- GIVEN `[ai.mcp]` with `servers = [{ name = "docs", command = "x" }]`
- WHEN Connect Google plans or writes an append
- THEN the file is byte-identical, no `config.toml.bak` is written, and the notice is `config.toml: the entries would not parse after it (an inline ai.mcp.servers list?)`

#### Scenario: A save never writes a level [S9, auto]
- GIVEN a file with `[[ai.mcp.servers]]` entries, one with `approval = "all"`, and a changed statusline position
- WHEN the save succeeds
- THEN only the position changes and every server entry, the `approval` line included, is byte-identical
