# Configuration Specification (delta: local-only mode)

## Base of this delta

This delta targets `openspec/specs/configuration/spec.md` on e480992e. It adds one key under `[ai]`, an owned table (see Unknown keys in owned tables), so "Every key that lazarobox reads MUST NOT be reported" already covers it once the key is read; no requirement needs a full MODIFIED copy. Slice tag: [S1]. The `[[ai.mcp.servers]]` key `local` is added by S6 (see local-only-mode, MCP servers stay off unless declared local, and below).

## ADDED Requirements

### Requirement: The local-only key
`[ai] local_only` is a boolean. A missing key is `false`. A value that is not a boolean (a string, an integer, a table) MUST count as `true` (fail closed, proposal Q6) and MUST produce the problem `ai.local_only: expected true or false (local only is on until this is fixed)` through the rules of Fallback and startup notice. It is independent of every other `[ai]` key and changes no assignment (see local-only-mode). The key is read, so it is never reported as an unknown key. The menu writes it through the `Local only` row (see config-menu); a save that does not change it keeps its bytes. A build that predates it and reads owned tables reports `unknown key: ai.local_only` and does not enforce the mode; this is the reason the key is under `[ai]` and not in a new top-level table, which older builds would ignore in silence.

#### Scenario: Default [auto]
- GIVEN no `[ai]` table, or one without `local_only`
- WHEN the config is read
- THEN the mode is off and there is no problem

#### Scenario: Explicit [auto]
- GIVEN `[ai]\nlocal_only = true`, and in another file `local_only = false`
- WHEN each config is read
- THEN the mode is on, then off, and there is no problem

#### Scenario: Wrong type fails closed [auto]
- GIVEN `[ai]\nlocal_only = "yes"` (or `1`, or a table)
- WHEN the config is read
- THEN the mode is on and the problem is `ai.local_only: expected true or false (local only is on until this is fixed)`

#### Scenario: Not an unknown key [auto]
- GIVEN a file that sets every key lazarobox reads, `[ai] local_only = true` included
- WHEN it is loaded
- THEN no `unknown key` problem is reported

### Requirement: The local key of an MCP server [S6]
An entry of `[[ai.mcp.servers]]` MAY hold `local`, a boolean, default `false`. It matters only while local-only mode is on (see local-only-mode). A value that is not a boolean MUST count as `false` (the server is not started in the mode) and MUST produce the problem `ai.mcp.servers[N].local: expected a boolean`; like `enabled`, it falls back and the entry is kept, not skipped. This amends MCP server lists, whose entries now also hold `local`; it is added to the keys an entry may hold, so `local` is no longer reported as an unknown key; a build that predates it reports `unknown key: ai.mcp.servers[N].local`. The menu never writes it (the menu never writes `ai.mcp.*`, see config-persistence).

#### Scenario: Read [auto]
- GIVEN an entry with `local = true` and one without the key
- WHEN the config is read
- THEN the first is local, the second is not, and there is no problem

#### Scenario: Wrong type [auto]
- GIVEN an entry with `local = "yes"`
- WHEN the config is read
- THEN the entry is kept, it is not local and the problem is `ai.mcp.servers[0].local: expected a boolean`
