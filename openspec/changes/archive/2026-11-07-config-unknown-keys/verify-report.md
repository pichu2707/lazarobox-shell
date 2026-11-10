# Verify Report: config-unknown-keys

Mode: hybrid, Strict TDD (cargo test). Change shipped in 4cd64b02 (v1.11.0); design/tasks written afterwards.

## Verdict: PASS WITH WARNINGS (0 CRITICAL, 1 WARNING, 2 SUGGESTION)

## Completeness
Tasks 1.1-1.4, 2.1-2.3, 3.1: 9/9 checked, matching code state. No unchecked tasks.

## Execution evidence
`cargo test config` (CARGO_TARGET_DIR=.../scratchpad/target-verify2): lib 409 passed, 0 failed; all other binaries 0 failed.
Clippy not run (optional).

## Spec compliance matrix (all [auto] scenarios of the delta)
| Scenario | Covering test | Result |
|---|---|---|
| A typo in an owned table | config.rs `an_unknown_key_in_an_owned_table_is_reported_by_its_path` (ai.tols, incl. load notice) | COMPLIANT |
| Known keys next to an unknown one | config.rs `the_known_keys_apply_and_the_unknown_key_is_reported_last` | COMPLIANT |
| Every owned table is checked (18 cases) | `an_unknown_key_in_an_owned_table_is_reported_by_its_path` (all listed paths, config equals file without key) | COMPLIANT |
| Keys outside the owned tables | config.rs `keys_outside_the_owned_tables_are_not_reported` | COMPLIANT |
| Every known key | config.rs `every_known_key_loads_without_a_problem` (mouse, llamacpp managed+model asserted) | COMPLIANT |
| A key that needs quotes | config.rs `an_unknown_key_that_needs_quotes_is_shown_quoted` | COMPLIANT |
| Schema: Unknown keys ignored | config.rs `unknown_keys_and_tables_are_ignored` | COMPLIANT |
| Schema: General task table and unknown tasks ignored | ai/config.rs `the_general_task_table_and_unknown_tasks_are_ignored` | COMPLIANT |
| Memory: Unknown keys ignored | ai/config.rs (~2716, full load) | COMPLIANT |
| Server ports: Unknown servers and keys | ai/config.rs `unknown_servers_and_keys_are_ignored` | COMPLIANT |
| Server ports: Saves keep the table | pre-existing config_edit tests | COMPLIANT (unchanged) |
| Tools: Unknown keys ignored | ai/config.rs (~2909) | COMPLIANT |
| MCP: Unknown keys ignored | ai/config.rs `unknown_mcp_entry_keys_are_ignored` (~3320-3338) | COMPLIANT |
| Workspace: Unknown keys ignored (next to valid dir) | config.rs `workspace_ignores_unknown_keys_and_counts_problems` | COMPLIANT |
| Usage: Local engines ignored | usage/tests.rs `a_local_engine_in_billing_is_reported_as_an_unknown_key` | COMPLIANT |
| Save re-reports kept unknown key (config-persistence) | config_edit.rs ~1118, ~1707 | COMPLIANT |
All other scenarios in MODIFIED requirements are pre-existing and pass in the same run. Scenarios without a test: none.

## Design coherence
D1 order (walk after readers, config.rs:261): confirmed by the ordering test. D3/D4 shape guard and unknown table not walked: confirmed in `note_unknown_ai_keys` (`child()`), `ai.tool` case in the table test. D5 constants (Task::ALL[1..], BILLED, ToolToggle::ALL, CATALOG): confirmed. D7 save untouched: confirmed. No deviations.

## Open risk: keys the config menu writes vs the walk
Every (table, key) the menu writes (src/core/menu.rs, ConfigEdit builders in config.rs) is known to the walk:
statusline.position, tabbar.position, root mouse (root not walked), ai.notify, ai.notify_sound, ai.profile, ai.context, ai.tasks.<task>.engine/model (all non-general tasks, orchestrate included), ai.usage.{line,statusline,turn_tokens,day_tokens,day_usd}, ai.usage.billing.{claude,codex,gemini} (BILLED), ai.tools.{enabled,approval,search,write,exec,git,git_write,github} (ToolToggle::ALL), ai.memory.{enabled,acknowledged}, ai.orchestrator.{enabled,acknowledged}, ai.servers.llamacpp.{managed,model,idle_minutes}. [updates] check is not written by the menu but is known. Result: a menu save cannot trigger an unknown-key notice today.

## Issues
CRITICAL: none.

WARNING
1. The menu-writes-only-known-keys invariant is verified here by inspection only; no test enforces it. A new menu row whose key is not added to `note_unknown_ai_keys` would make every save raise a notice. (Not a spec scenario, so not CRITICAL.)

SUGGESTION
1. Add a test that collects all edits from the menu (every row changed), applies them via config_edit to an empty file and asserts `Config::parse(...).problems` is empty.
2. Design open question D5: move the literal known-key slices into consts next to their readers (separate refactor). Also, archive still has to merge the delta into openspec/specs/configuration/spec.md.

Next: sdd-archive.
