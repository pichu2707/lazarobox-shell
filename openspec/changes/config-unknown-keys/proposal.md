# Proposal: Config Unknown Keys

## Intent

`config.toml` ignores every key it does not know, without a word. A typo (`[ai] tols`, `[ai.servers.llama]`, `[ai.tasks.cod]`) or a key that never existed leaves the setting at its default, and the user has no way to see why. The config freeze before 1.0 (stabilization-1.0, S9 item 5) approved warning about them: "Avisar de las que estén en tablas propias; ayuda a detectar erratas y renombres".

**User decision (2026-11-06):** only this warning ships. The two renames reviewed with it (`mouse` to `[input] mouse`, `ai.servers.llamacpp.model` to `model_path`) are dropped, because the format is public since v1.0.0: the root `mouse` and llamacpp's `model` stay the documented keys.

## Scope

### In Scope
- **Owned tables:** `[statusline]`, `[tabbar]`, `[updates]`, `[workspace]` and `[ai]` with every table under it, each `[[ai.mcp.servers]]` entry included.
- **The problem:** `unknown key: <full path>` (for example `unknown key: ai.tols`), through the existing startup notice. The file still loads, the key is ignored, and these problems come after every other one.
- **Not reported:** the root of the file (other keys and tables there are the user's own), and every key lazarobox reads, the root `mouse` and `ai.servers.llamacpp.model` included.
- `[ai.tasks.general]` and unknown task names are now reported: they were already ignored, and a user who writes them expects them to work.
- README and CHANGELOG.

### Out of Scope
- Renaming any key, aliases.
- Unknown values (each key keeps its own problem text).
- The save: unknown keys are still preserved byte for byte (config-persistence is unchanged).

## Approach

After the readers run, one walk over the owned tables compares each table's keys with the keys its reader reads (`note_unknown_keys` in `src/core/config.rs`, `note_unknown_ai_keys` in `src/core/ai/config.rs`). Only tables of the expected shape are walked; a wrong shape already has its problem. A key that is not a bare TOML key is shown quoted with control characters escaped.

## Capabilities

### Modified Capabilities
- `configuration`, Schema and defaults, Memory keys, Server ports, Tools key, MCP server lists, Workspace key and Usage keys (unknown keys in owned tables are reported) and the new requirement Unknown keys in owned tables.

## Risks

- **Older builds and newer files:** a file written for a newer version shows a notice in an older one. The file still loads and the notice names the key, which is the point.
- **A key read but missing from the walk** would be reported by mistake. A test loads a file with every key lazarobox reads and expects no problem; a new key must be added to both the reader and the walk.
