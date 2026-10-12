# Proposal: AI Selector (per-task engine and model)

## Intent

The AI section cycles `Engine` and `Model` with `h`/`l`. That is slow, it hides what is available, and it allows only one assignment. The user wants a gentle-ai style screen: a fixed list of tasks, each with an `engine › model`, chosen from a picker that shows what is detected and what is downloaded. This is groundwork for P6 (the orchestrator). It also closes two items left open by ai-engines: W2 (Enter before the first detection closed the menu silently) and S1 (no app-level test that an undetected saved engine survives).

## Decisions (user answers, 2026-10-11)

- D1 **Task rows** replace `Engine`, `Model` and `Status`. The rows are `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, and each one shows `engine › model`. Tasks other than general show `(default)` when they fall back to general. general shows `(not set)` when it has no engine. Set all shows `(mixed)` when the tasks differ. Tasks are fixed.
- D2 **Picker in the same popup**. Enter on a task row swaps the view:
  - Level 1 lists the detected engines with their status text.
  - Level 2 lists the models of the chosen engine.
  - Esc goes back one level. It never cancels the whole menu while a picker is open.
  - The first entry is `(default · use general)` for tasks other than general, and `none` for general and Set all.
- D3 **Set all** writes the chosen value to all five tasks, general included. Choosing `none` clears general and makes every other task fall back to it.
- D4 **Model lists and marks** (revised by the 2026-10-11 ruling, see D11):
  - For a local server (ollama, lmstudio, llamacpp, vllm), level 2 lists first the detected (downloaded) models, selectable and marked `✓`. Then it lists a small curated static catalog of well-known models that are NOT downloaded. They are shown but NOT selectable (muted), marked `download required`.
  - `custom…` (type a model name) exists ONLY for CLI engines (claude, codex, gemini, opencode), whose availability lazarobox cannot check. Custom and catalog entries of a CLI are selectable and have no `✓`.
  - CLI catalogs: claude offers `default`, `opus`, `sonnet` and `fable`, which are the aliases confirmed by `claude --help`. codex, gemini and opencode offer `default`. Every CLI also has `custom…`.
  - A model saved in config that is no longer downloaded shows `download required` too, is not selectable as a new choice, but the existing assignment is kept and stays visible on its task row with the mark. It is never erased silently.
  - Any model already assigned in the draft for that engine is also listed in level 2.
- D5 **Save and discard**: `s` saves from any view. The exception is the custom text field, where `s` is just a typed letter. Esc at the top level discards. Changes only touch the draft.
- D6 **Enter on Settings rows** cycles the value forward, like `l`. `h`/`l` stay as they are. Enter no longer saves anywhere.
- D7 **Detection feedback**: the `Status` row goes away. Status is shown in picker level 1 and in the footer: `checking engines…` before the first result, and `refreshing…` while a refresh runs. Last-known engines stay visible during a refresh. Pressing `s` before the first result still saves the draft (fixes W2). The menu never closes silently.
- D8 **Undetected assigned engine**: an engine assigned in the draft that is not detected is listed in level 1 and on its row as `(not detected)`. It is never erased (S1, with an app-level test).
- D9 **Config schema**: `[ai] engine/model` stays as the general assignment, so no migration is needed. New tables `[ai.tasks.<task>] engine/model` hold code, chat, review and summarize. Unassigning a task writes `""`. A `[ai.tasks.general]` table is ignored and preserved.
- D10 **Nested-table edits**: `config_edit` supports dotted table paths, and it creates the parent tables implicitly. Collisions follow the existing rule: for example `ai.tasks = 3` gives a footer error, and the file is not touched.

- D11 **Ruling on models (user, 2026-10-11, verbatim)**: "los no descargados tienen que ser no seleccionables, que aparezcan pero no se pueden seleccionar y que marque que tiene que descargarse".
  - Curated Ollama catalog: about 8 to 12 well-known model tags, shipped in code, never fetched. It is informational and possibly stale, and the code and the docs say so. The design picks the exact tags.
  - LM Studio, llama.cpp and vLLM have no curated catalog. They list only detected models.
  - No network beyond localhost. lazarobox never downloads anything (no `ollama pull`): that is a non-goal.
  - The earlier idea of a "wishlist via assignment" (typing a custom model for a server) is dropped.

## Scope

### In Scope
- The task-list rows, the two-level picker (engines and models), the custom text entry (CLI engines only) with paste routed to it, and the viewport for long model lists.
- The CLI catalog and the curated static Ollama catalog in `core::ai`, the model marks (`✓`, `download required`), non-selectable rows, the `s` save key, the Enter change, and the footer detection cues.
- Parsing and persistence for `[ai.tasks.*]`, nested TOML writes, W2 and S1.

### Out of Scope
- Using the models (chat is P2), cloud providers, downloading models (`ollama pull` or any download), custom task names, type-to-filter, a saved wishlist of models, custom model names for servers, and any fetching of a model catalog.

## Capabilities

### New Capabilities
- `ai-task-models`: the fixed task set, the fallback-to-general rule, the CLI model catalog, the model marks (`✓`, `download required`) and non-selectable rows, `custom…` for CLI engines only, and how the engine and model lists are built.

### Modified Capabilities
- `config-menu`: the AI section is replaced by task rows and the picker view. Status row removed, `s` saves, Enter cycles on Settings rows, new footer hints and detection cues, picker viewport and minimum size, paste goes to the custom entry.
- `configuration`: adds the `[ai.tasks.<task>]` schema. `[ai]` is now the general task.
- `config-persistence`: saving moves to `s`, the owned keys gain `ai.tasks.<task>.{engine,model}`, nested tables are created implicitly, and the collision rules are extended.

## Approach

`core::ai` stays pure and gains the catalog plus a picker model: a state machine that turns keys and the draft into a view. `MenuState` owns the picker state, so `InputMode` does not change, and `rows()` returns the picker rows while the picker is open. `ConfigEdit.table` becomes a dotted path. The runtime save path stays the same.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/{config,menu}.rs`, new catalog | Modified/New |
| `src/core/menu.rs`, `src/core/menu/ai_tests.rs` | Modified (cycling tests replaced) |
| `src/core/config_edit.rs`, `src/runtime/config_write.rs` | Modified (nested tables) |
| `src/ui/components/menu.rs` | Modified (picker render, viewport, snapshots) |
| `src/app.rs` | Modified (`s`, paste routing, refreshing flag) |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Users used to Enter=save | Med | Footer always shows `s save`; Enter never discards |
| Popup grows taller, long model lists | Med | Fixed-height viewport with scroll; tiny-terminal tests |
| Nested TOML writes corrupt the user file | Med | Byte-preservation tests, collision errors, file untouched on error |
| Paste or `s` leaks to the PTY or saves mid-typing | Low | Paste and `s` routed to the custom buffer while it is focused |
| Every menu snapshot changes | High | Expected; reviewed slice by slice |

## Rollback Plan

Revert the merge of the tracker `feat/ai-selector` into main. Leftover `[ai.tasks.*]` tables are ignored and preserved by the previous version, and `[ai]` keeps working as before.

## Delivery Slices

Feature-branch chain on `feat/ai-selector`, with a 400-line budget per PR and strict TDD. About 1,300 to 1,600 lines in total.

| # | Content |
|---|---|
| AS1 | `[ai.tasks]` schema and reader |
| AS2 | Nested-table `config_edit`, persistence of the task keys |
| AS3 | CLI catalog and the pure picker model |
| AS4 | Task-list wiring in `MenuState`, `s` save, Enter cycles |
| AS5 | Picker render, input, viewport, custom entry and paste |
| AS6 | W2 footer cues and refreshing, S1 app-level test |

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] Each of the five tasks can be assigned through the picker, and `Set all` sets all five. The `(default)`, `(not set)` and `(mixed)` labels match the draft.
- [ ] Downloaded Ollama models show `✓` and are selectable. Curated models that are not downloaded show `download required` and cannot be selected. A saved model that is no longer downloaded keeps its assignment, shows `download required`, and survives a save and a restart.
- [ ] `s` saves only the changed keys and the rest of the file is byte-identical. Esc at the top level reverts. Esc in the picker goes back one level.
- [ ] Before the first detection, the menu shows `checking engines…` and never closes silently. An undetected saved engine survives open, save and restart.
- [ ] Manual check in Kitty: assign code to `claude › opus`, save, restart, and the row shows it.

## Assumptions for user review

1. `s` does not save while you are typing a custom model name, because there it is just a typed letter.
2. Set all with `none` clears general, and every task then falls back to it.
3. Custom models (CLI engines only) are not stored in a separate list. They exist only as task assignments.
4. CLI catalog and custom entries have no mark, because lazarobox cannot check them.
5. `r` refreshes from task rows and from picker level 1.
