# Design: Config Unknown Keys

Evidence: **[R]** read in code for this design; **[I]** inferred. Proposal scope (warning only, no renames, root not walked, save unchanged) is binding.

## Status finding (read first)

The change is **already implemented on `main` and released in v1.11.0** [R `CHANGELOG.md:128-137`, README `:140`]. Every spec scenario has a test in place [R `config.rs:589-595`, `:1023-1048`, `:1069-1290`; `ai/config.rs:1731-1735`, `:2428-2433`, `:2716-2721`, `:2909-2920`, `:3320-3338`; `config_edit.rs:1118-1121`, `:1707-1708`]. The change folder was never closed: there is no design or tasks, and `openspec/specs/configuration/spec.md` does not yet hold the requirement "Unknown keys in owned tables". This design records the as-built architecture so that verify and archive have a reference. **Remaining production work: 0 lines.**

## Technical Approach

The readers stay as they are: they read the keys they know and note problems for wrong values. After every reader has run, `Config::parse_with_home` walks the owned tables [R `config.rs:261-267`]:

1. `OWNED_SECTIONS` [R `config.rs:357-362`]: `statusline`, `tabbar`, `updates` and `workspace`, each with its key slice, passed to `note_unknown_keys`.
2. `ai::config::note_unknown_ai_keys(&table, &mut problems)` [R `ai/config.rs:1509-1620`] walks `[ai]` and every table under it.

`note_unknown_keys(table, path, known, problems)` [R `config.rs:368-385`] pushes `unknown key: {path}.{key}` for each key not in `known`. A key that is not bare (`[A-Za-z0-9_-]+`) is formatted with `{key:?}`, which quotes it and escapes control characters (ESC shows as `\u{1b}`). The output is `Parsed.problems`, which the existing `load_with_workspace` notice already reports [R `config.rs:314-326`].

## Architecture Decisions

| # | Topic | Choice | Rejected, and why |
|---|---|---|---|
| D1 | When to walk | Run after all readers, so these problems come last and a wrong value stays the first problem [R `config.rs:261`] | Checking inside each reader: problems would mix with the value problems and break the ordering rule |
| D2 | Where the walk lives | Two functions: the generic `note_unknown_keys` in `core/config.rs` and `note_unknown_ai_keys` in `core/ai/config.rs`, next to the AI readers | Serde `deny_unknown_fields`: it fails the whole file and cannot preserve the "file still loads" rule. A single walk in `config.rs`: it would have to import AI internals |
| D3 | Shape guard | The walk goes into a child only when it is `Value::Table` (the `child()` helper) or, for `[[ai.mcp.servers]]`, an array entry that is a table. A wrong shape keeps only its reader's problem | Walking every value: it would give a second problem for one mistake |
| D4 | Unknown table | It is reported once as one key and its content is not walked (the walk only goes into known children) | Walking into it: it would give noise for one typo |
| D5 | Source of truth for known keys | Derived from the same constants the readers use where they exist: `Task::ALL[1..]` (this leaves out `general`, so `ai.tasks.general` is reported), `usage::BILLED`, `ToolToggle::ALL.map(key)`, and the `CATALOG` server ids. The remaining keys are literal slices inside the walk. Drift is caught by `every_known_key_loads_without_a_problem` [R `config.rs:1177`], which sets every read key, root `mouse` and llamacpp `model` included, and expects no problem | Const slices next to each reader, with the readers indexing them: about 40 lines of churn in merged code. A reader that calls `section.get("x")` still escapes any const, so the guard test is the real protection either way |
| D6 | Path format | Dotted full path. Array entries use `[N]` (`ai.mcp.servers[0].timeout`), the same format as the existing MCP problems | - |
| D7 | Save | Not touched: `config_write` / `toml_edit` already keep unknown keys byte for byte. Tests confirm that a kept key is reported again after a save [R `config_edit.rs:1118`, `:1707`] | - |
| D8 | Order inside the walk | `toml::Table` iteration order (sorted; `toml = "0.8"` without `preserve_order` [R `Cargo.toml:50`]) | Source order: needs a feature flag, and no scenario depends on it |

## Data Flow

    text -> toml::Table
         -> read_position / mouse / read_update_check / ai::read_with_port_env / read_workspace
              (value problems)
         -> OWNED_SECTIONS + note_unknown_keys        (unknown keys, top-level owned tables)
         -> note_unknown_ai_keys                      (unknown keys under [ai])
         -> Parsed.problems -> load_with_workspace -> "config[ (N problems)]: <first>"

## File Changes

| File | Action | Status |
|---|---|---|
| `src/core/config.rs` | `OWNED_SECTIONS`, `note_unknown_keys`, call site in `parse_with_home` | Done (v1.11.0) |
| `src/core/ai/config.rs` | `note_unknown_ai_keys` | Done |
| `README.md`, `CHANGELOG.md` | User-facing note | Done |
| `openspec/specs/configuration/spec.md` | Merge the delta | Pending (archive) |

## Testing Strategy (strict TDD, `cargo test`)

| Layer | What | Where |
|---|---|---|
| Unit | One unknown key per owned table gives exactly one problem and the same config | `config.rs:1073` |
| Unit | Ordering after value problems | `config.rs:1138` |
| Unit | Quoting and escaping | `config.rs:1157` |
| Unit | Root and foreign tables are silent | `config.rs:1169` |
| Unit | Every read key is silent (drift guard) | `config.rs:1177` |
| Unit | Per-requirement scenarios (servers, tasks, memory, tools, MCP, workspace, save) | refs above |

Verify must map each `[auto]` scenario of the delta spec to one of these tests. If a scenario has no test, that is the only possible remaining work, and it is test-only.

## Delivery

Production estimate: about 150 lines as built (`note_unknown_keys` + `OWNED_SECTIONS` ≈ 35, `note_unknown_ai_keys` ≈ 115). It fits one PR, and it already shipped. Remaining production lines: **0**.

## Migration / Rollout

No migration. The change was released in v1.11.0. What is left is the SDD close-out: verify, then archive (merge the delta into the main spec).

## Open Questions

- [ ] Owner: should the SDD close-out skip `sdd-tasks`/`sdd-apply` and go straight to `sdd-verify` -> `sdd-archive`?
- [ ] Optional hardening (not required by the proposal): move the literal slices of D5 into named consts next to their readers. This is a separate small refactor if the owner wants it.
