# Delta for Config Persistence

All existing requirements stay as they are. The owned-keys list of "Owned keys only" is extended by `ai.tools.enabled` (below), and `ai.mcp.*` is explicitly NOT owned. Tags as in the main spec.

## ADDED Requirements

### Requirement: Tools key is owned
The save MUST also own `ai.tools.enabled`. It is written only when its draft differs from the value captured at open, as a TOML boolean, explicitly even when it equals the default (`true`), in place when the key exists (comments and neighbours untouched), and created under an implicit `[ai.tools]` table otherwise (no empty parent header). A scalar of the wrong type under `enabled` is replaced by the draft boolean. The save MUST be refused (file unchanged, menu open, draft intact) with the footer `config.toml: ai.tools is not a table; fix it before saving` when `ai.tools` holds a non-table value, and `config.toml: ai.tools.enabled is a table; fix it before saving` when `enabled` holds a table or an array of tables. These checks apply only when the tools draft differs from the value at open. An unknown key under `ai.tools` is preserved.

#### Scenario: Table created [auto]
- GIVEN a file `# mine\n[statusline]\nposition = "bottom"\n` and the tools draft changed to off
- WHEN the save succeeds
- THEN the original bytes are unchanged and the file ends with `[ai.tools]\nenabled = false\n`

#### Scenario: Updated in place [auto]
- GIVEN `[ai.tools]\n# c\nenabled = false  # off\n` and the draft on
- WHEN the save succeeds
- THEN only the value changes: `enabled = true  # off`

#### Scenario: Default written explicitly [auto]
- GIVEN `enabled = false` and the draft changed to on (the default)
- WHEN the save succeeds
- THEN the file contains `enabled = true` and the key is not removed

#### Scenario: Untouched when unchanged [auto]
- GIVEN the tools draft equals the open value and only a position changed
- WHEN the save succeeds
- THEN a file without `[ai.tools]` gains none and an existing one is byte-identical

#### Scenario: Collisions [auto]
- GIVEN `[ai]` holding `tools = 3`, and in another file `[ai.tools.enabled]`, and a changed tools draft
- WHEN `s` is pressed
- THEN each file is byte-identical and the matching footer is shown

#### Scenario: Collision ignored when unchanged [auto]
- GIVEN a file with `[ai.tools.enabled]`, an unchanged tools draft and a changed position
- WHEN `s` is pressed
- THEN the position is saved and the table is untouched

#### Scenario: Saved file parses back [auto]
- GIVEN any save result with a tools draft
- WHEN it is parsed by `Config::parse`
- THEN tools equal the draft and no notice is produced

### Requirement: MCP server lists are never written
The save MUST NEVER add, change, reorder or remove any `ai.mcp.*` key, the global `[[ai.mcp.servers]]` entries and the per-project extras included: they are edited by hand. Every byte of them, including arrays of tables, comments, unknown keys and invalid entries, MUST be preserved by every save, and a collision or an error inside them MUST NOT block the save of other keys. No secret is ever written, and no environment value is ever written.

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
