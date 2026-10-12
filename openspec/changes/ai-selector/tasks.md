# Tasks: AI Selector (per-task engine and model)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~2,650 total (~950 deletions), 11 PRs of 190-300 lines each |
| 400-line budget risk | High (total); Medium per PR (re-measure with `git diff --shortstat` before opening each PR) |
| Chained PRs recommended | Yes |
| Suggested split | PR1 → PR2 → ... → PR11 (see work units) |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain (tracker `feat/ai-selector` = main 63544bf) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Rules for every PR: strict TDD (RED with compiling stubs and exact assertions, then GREEN, then refactor); tests are never trimmed; gate = `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`; one work-unit commit per task group (conventional commits, no AI attribution); if a PR measures above 400 (additions + deletions) split it before opening. Only the tracker merges to main.

### Suggested Work Units

| Unit | Goal | PR | Est. | Base |
|------|------|----|------|------|
| AS1 | Task model, `[ai.tasks.*]` reader, `ai_of` ripple | PR1 | ~300 | `feat/ai-selector` |
| AS2 | Nested `config_edit` + collisions | PR2 | ~260 | PR1 |
| AS3a | `catalog.rs` + engine/model row derivation | PR3 | ~200 | PR2 |
| AS3b | Picker state machine, viewport, custom buffer, assignment actions | PR4 | ~220 | PR3 |
| AS4a | Delete cycling tests in `src/app.rs` (deletion only) | PR5 | ~280 | PR4 |
| AS4b | Delete old `ai_tests.rs` + obsolete UI snapshots (deletion only) | PR6 | ~280 | PR5 |
| AS4c | Delete `AiField`/`cycle`/Engine-Model-Status rows; read-only task rows | PR7 | ~290 | PR6 |
| AS5a | Picker wiring in `MenuState`, key model, custom, paste, rekey | PR8 | ~230 | PR7 |
| AS5b | `footer()`, per-task `edits()`, `s`/Enter/`r`, cues | PR9 | ~210 | PR8 |
| AS6a | Render: fixed body, picker rows, fallback, snapshots | PR10 | ~220 | PR9 |
| AS6b | App: paste, `sync_checking`, W2, S1, README | PR11 | ~190 | PR10 |

AS4 (~850, 90% deletions) is split in three because two PRs cannot stay under 400 (2 x 400 < 850). PR5-PR6 are pure deletions (tests only, code still compiles); the interim tracker state is acceptable. Design deviation: AS5 splits as picker wiring first (5a) and footer/edits/save second (5b), because footers need the picker states.

## PR1 / AS1: Schema and reader (`src/core/ai/config.rs`)

- [x] 1.1 RED: add `Task` (`ALL`, `id()`, `table()`), `Assignment`, slot `AiConfig` (`get/set/resolve`, `general(e,m)` helper) as compiling stubs; tests for task set integrity, resolve/inherit, labels `(not set)`/`(default)`/`<e> › <m>`/model-without-engine, aggregate `claude › opus`/`(not set)`/`claude › opus`/`(mixed)`.
- [x] 1.2 RED: `read_text` fixtures: task tables, `""`, `general` and unknown tasks ignored, unknown engine kept, wrong types per key, non-table `ai.tasks` and `ai.tasks.code`, problem count text (exact notices).
- [x] 1.3 GREEN: implement the model and the `[ai.tasks.<id>]` reader with per-key fallback.
- [x] 1.4 GREEN: ripple `ai_of` helper through `src/app.rs` and `src/core/menu.rs` tests so everything compiles on `AiConfig::general`.
- [x] 1.5 Gate: cargo test, clippy, fmt.

## PR2 / AS2: Nested edits (`src/core/config_edit.rs`, `src/runtime/config_write.rs`)

- [x] 2.1 RED: exact-string tests: new `[ai.tasks.code]` after `[ai]` appended with no empty parent header; existing nested edited in place; key added to existing task table; dotted and inline parents; CRLF kept; comment-only-file prefix.
- [x] 2.2 RED: collision tests per variant (`ai = 3`, `[ai] tasks = 3`, `[[ai.tasks]]`, `[ai.tasks] code = "x"`, `[ai.tasks.code.engine]`) with exact `NotATable`/`KeyIsTable` prefixes and footer messages; general table byte-identical; `chat = 3` untouched when only code changes.
- [x] 2.3 GREEN: `apply_edit` walks dotted `table` segments, implicit parents, static prefix slices.
- [x] 2.4 RED+GREEN: round trip through `Config::parse` (task tables, Set all five in order, `engine = ""`/`model = ""` for unassigned, `old:7b` kept).
- [x] 2.5 Gate.

## PR3 / AS3a: Catalog and row derivation (`src/core/ai/catalog.rs`, `picker.rs`)

- [x] 3.1 Create `catalog.rs` stubs (`cli_models`, `OLLAMA_CURATED` 10 entries, `curated`, `same_model`); register in `mod.rs`; make `parse::is_format` `pub(crate)`.
- [x] 3.2 RED: catalog integrity (8-12 unique non-empty, Ollama only), `:latest` dedupe, claude `default/opus/sonnet/fable`, others `default`.
- [x] 3.3 RED: `engine_rows` (clear label per target, available then assigned-undetected `(not detected)`, never lmstudio-not-running unassigned, `checking…`) and `model_rows` (server order ✓ → assigned `●` download required → curated; CLI catalog → assigned custom → `custom…`; `no models detected`; engine not available; `checking…`; no `custom…` on servers).
- [x] 3.4 GREEN: implement `PickRow`, `PickKey`, `Mark`, derivation functions; doc comment says catalog is informational and possibly stale.
- [x] 3.5 Gate.

## PR4 / AS3b: Picker state machine (`src/core/ai/picker.rs`, `config.rs`)

- [x] 4.1 RED: `Picker` open positions (cursor on current engine/model, else first selectable, else first), `step` wrap over all rows including disabled, `enter` outcomes (`Commit`/`Stay`/`Close`; Enter on disabled is `Stay`), `back` per level.
- [x] 4.2 RED: viewport (VIEW=10, offset rules, wrap follows, `n/total` only when longer).
- [x] 4.3 RED: custom buffer (filter control/format/whitespace, cap 128, paste first line, Ctrl+U, Backspace, blank refused) and `validate_custom` (`  my-model  `, empty, spaces, 129 chars).
- [x] 4.4 RED: assignment actions on `AiConfig` (assign one, Set all five, `none` clears all five, unassign) and "no download path" (every key on a disabled row yields no effect).
- [x] 4.5 GREEN: implement all of the above; `PickKey` re-anchor helper for later detection updates.
- [x] 4.6 Gate.

## PR5 / AS4a: Delete cycling tests in app (deletion only)

- [x] 5.1 Delete AI cycling/Engine/Model/Status tests in `src/app.rs` (list them in the commit body); keep generic menu tests.
- [x] 5.2 Gate (compiles, remaining tests green). Re-measure: if above 400, split by test group.

## PR6 / AS4b: Delete old menu AI tests and snapshots (deletion only)

- [x] 6.1 Delete obsolete tests in `src/core/menu/ai_tests.rs` and the insta snapshots of Engine/Model/Status rows in `src/ui/components/`.
- [x] 6.2 Gate; verify no orphan `.snap` files.

## PR7 / AS4c: Retire cycling, read-only task rows (`src/core/ai/menu.rs`, `src/core/menu.rs`)

- [x] 7.1 RED: descriptor integrity (six AI rows, ids, key paths, Set all has none), navigation visits Statusline, Tabbar, Set all, general, code, chat, review, summarize with wrap; Mouse skipped; repeat moves.
- [x] 7.2 RED: row values `(default)`/`(not set)`/`(mixed)`, marks `· download required` / `(not detected)`, none before first detection, Result-arrives-on-task-list, Engine-disappears.
- [x] 7.3 GREEN: `AiMenu` over `AiConfig`; task rows and selectability; delete `AiField`, `cycle`, choices.
- [x] 7.4 Gate.

## PR8 / AS5a: Picker wiring (`src/core/menu.rs`)

- [x] 8.1 RED: Enter on task row opens level 1 (title, rows, cursor); assign CLI model; unassign; `none` for general/Set all; Set all five; Esc back one level to task list with task selected; reopen starts on task list.
- [x] 8.2 RED: disabled rows inert, no selectable row, saved model stays (`●`), undetected assigned engine selectable, no `custom…` on servers.
- [x] 8.3 RED: custom flow (type, `s` is a letter, Ctrl+U, blank refused, Esc to `custom…` row, paste, paste elsewhere ignored); `r` at levels 1 and 2 → `Refresh`, not in custom.
- [x] 8.4 RED: `set_engines` re-anchors by `PickKey` (cursor stays on a row that became disabled; clamp when removed); choosing changes only the draft.
- [x] 8.5 GREEN: implement `Option<Picker>` in `AiMenu`, `Row::Title`/`Row::Pick`, `on_paste`, key routing (custom before modifier guard).
- [x] 8.6 Gate.

## PR9 / AS5b: Footer, edits, save (`src/core/menu.rs`)

- [x] 9.1 RED: `footer()` exact strings per view (Settings, task row, levels 1/2, disabled reason, custom), cues replace hints, precedence error > disabled > cue > hint, error cleared by next press.
- [x] 9.2 RED: Enter cycles Settings and never saves, cycling keys inert on task rows, uppercase `S`/`R` swallowed, unknown keys, `r` only on task rows (Repeat ignored).
- [x] 9.3 RED: `s` from every view except custom, unchanged draft closes with no file access, save before first detection, Esc reverts AI, `edits()` order (Settings, then changed tasks in `Task::ALL`, both keys), failure keeps picker level.
- [x] 9.4 GREEN: `Footer`, `set_checking`, `edits()`, key model.
- [x] 9.5 Gate.

## PR10 / AS6a: Render (`src/ui/components/menu.rs`)

- [ ] 10.1 RED: insta snapshots: Settings rows, task list (mixed/default/not set), level 1, Ollama scrolled to a disabled last row (`18/18`), CLI with `custom…`, custom entry, checking/refreshing cues, disabled-row footer; 24x14 full popup vs 23x14 and 24x13 fallback; no panic 0..70 x 0..20.
- [ ] 10.2 GREEN: fixed 62x15 body, `VIEW=10`, marks, `●`, muted-reversed disabled rows, `footer()` rendering, fallback threshold.
- [ ] 10.3 Gate; review each new `.snap` by eye.

## PR11 / AS6b: App wiring (`src/app.rs`, README)

- [ ] 11.1 RED: paste reaches custom buffer and never the PTY, ignored elsewhere; `sync_checking` after open and Refresh, cleared on result; pane exit keeps picker at level 2; spawn failure, resize, last pane regressions; minimal fallback keeps Esc/`s`.
- [ ] 11.2 RED: W2 (Enter never saves/closes), S1 (undetected saved `codex` survives open, refresh, `s`, reload).
- [ ] 11.3 GREEN: `App::on_paste` Menu branch, `sync_checking`.
- [ ] 11.4 README: Ollama catalog informational and possibly stale, nothing is downloaded, `s` saves.
- [ ] 11.5 Gate.

## PR12 (no code): Verify-prep, real machine [manual]

- [ ] 12.1 Ollama with 8 models: `✓` rows selectable, curated `download required` rows reachable, footer `download required · not selectable`, Enter does nothing, no download.
- [ ] 12.2 claude catalog (`default/opus/sonnet/fable`) and `custom…` for a CLI; `s` typed in the field.
- [ ] 12.3 Per-task assignment saved under `[ai.tasks.*]` and kept after restart; `s` saves, Esc goes back one level, Esc at top discards.
- [ ] 12.4 `checking engines…` and `refreshing…` cues; `r` after `ollama pull` at level 2 refreshes.
- [ ] 12.5 Terminal below 24x14 shows fallback, Esc and `s` still work; positions in Kitty (four combinations).

## Coverage: spec scenario → PR

ai-task-models: Task set integrity P1; Effective assignment, Nothing assigned, Model without engine P1; Aggregate values P1; Engine list, Unassign entry label P3; CLI lists, Assigned custom listed P3; Detected and catalog, Tag-less name, Assigned model no longer downloaded, Server without models, Server not available P3; Catalog integrity P3; No download path P4; Docs say stale P11+12.5; Row marks, Before first detection P7; Validation P4; Assign one, Set all, Set all none, Unassign P4.

configuration (all existing + new, P1): Defaults, Each value, Unknown keys, AI keys parsed, Unknown engine kept, Empty string unset, Task tables parsed, Empty task string, General table and unknown ignored, Unknown task engine kept, Missing file, Invalid value per key, Several invalid keys, Unparseable, Mouse reserved, Invalid AI types, Non-table ai, Non-table ai.tasks, Non-table task, Invalid task key type, Several task problems; Positions in Kitty 12.5.

config-persistence: Implicit parents, Existing parent reused, Key added to existing task table, Round trip P2; Task table created, Only changed tasks, Unassigned as empty strings, Set all writes five, General preserved, Download-required kept P2 (edit layer) + P9 (`edits()`); Comments preserved, Missing keys, Mouse untouched, Saved file parses back, AI table created, Existing AI keys in place, No engine empty string, Special characters, AI untouched, Undetected saved engine survives P2 + P11 (S1); collisions (ai scalar, engine table, scalar replaced, ignored when unchanged, ai.tasks scalar, task scalar, task engine table, other task unchanged) P2, one end to end P9; Save closes, Unchanged draft, Enter never saves, Save before first detection P9; Save from the picker P9; Missing file and dirs, No config path, refuse-on-errors (5), Line endings P2/P9 regression; Write fails, Retry and cancel P9; Failure keeps the picker open P9; Persistence in Kitty 12.3.

config-menu: Open, Esc closes, Reopen starts fresh P9; Esc in picker back one level, Reopen after picker P8; Descriptor integrity P7; Settings rows P10; Move, Wrap, Disabled row skipped, Task rows in order, Repeat moves P7; Cycle forward, Cycle backward and Space, Arrow keys (regression) P7; Enter cycles Settings row, Cycling keys do nothing on task rows P9; Unknown key, Uppercase S P9; Paste ignored P9+P11; Footer hint, Footer hint on AI row, Error replaces hint P9 (strings) + P10 (snapshots); Footer hints in the picker, Footer on a disabled row P9+P10; No panic, Smallest full popup P10; Minimal message and Esc / and s, Picker too small keeps keys P10+P11; Output keeps flowing, Pane exit keeps the menu, Spawn failure, Host resize, Last pane exits P11; Pane exit keeps the picker P11; Menu in Kitty 12.5; Result arrives on the task list, Engine disappears P7; Result arrives in the picker, Cursor entry removed P8; Refresh on a task row P9; Refresh at level 1 and level 2 P8; Refresh elsewhere or repeated P9 (+P8 custom); Esc reverts AI, Save closes with AI values, Unchanged AI draft P9; Choosing changes only the draft P8; AI menu in Kitty 12.3; Real Ollama models 12.1; Task rows rendered, Nothing assigned P7 (values) + P10 (snapshot); Marks on rows P7; Open level 1 P8 (+P10); Assign a CLI model, Unassign entry, General and Set all use none, Set all assigns five P8; Server model marks P3 (+P10); Disabled rows reachable but inert, Enter with no selectable row, Saved model stays, Undetected assigned engine listed P8; Type and accept, `s` is a letter, Paste goes to the field, Paste elsewhere ignored, Ctrl+U clears, Blank entry refused, Esc leaves the field P8 (app routing P11); Servers have no custom entry P3; Long list scrolls P4 (+P10); Wrap moves the window P4; Before the first result P9+P10; Refreshing keeps last known engines P9; Refreshing at level 2 P10; Cue clears P9; Undetected saved engine survives P11.
