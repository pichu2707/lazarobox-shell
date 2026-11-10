# Config Menu Specification (delta: local-only mode)

## Base of this delta

This delta targets `openspec/specs/config-menu/spec.md` on e480992e. The new row is the last row of the Models tab, after `Hugging Face` (assumption: the tab that holds the engine assignments; the last place changes the fewest existing scenarios, and `2` still selects `Set all tasks`, see Jump keys). The MODIFIED blocks are full copies of "Sections and items are data", "Navigation" and "Models tab" where only the sentences and scenarios that list the Models rows change, plus one new scenario in "Models tab". Existing drift in those blocks (for example the `GitHub` and `Issue draft` rows of the Tools tab missing from some lists) is kept as it is and is not fixed here. If the mcp-connectors change lands first, its own copies of "Sections and items are data" are merged with this one. Slice tags: [S2] row and marks, [S5] Updates and GitHub rows.

## ADDED Requirements

### Requirement: Local only row [S2]
The Models tab MUST end with a static row `Local only` (key path `ai.local_only`, values `off`, `on`; see configuration, The local-only key and local-only-mode). It edits the AI draft, so it is not shown when the AI section is not attached. On it, `h`, `l`, Left, Right, Space and Enter toggle the draft, Repeat is handled like Press except on Enter, the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`, and detection cues and `r` do not apply. Inside the menu, the marks of the task rows and the picker follow the draft, so the user sees what the mode refuses before saving; outside the menu the draft has no live effect, and the mode changes only when the menu is saved (see local-only-mode, Turning it on stops cloud work in flight). Esc at the top level restores the value captured at open; `s` writes only `ai.local_only` when it changed, as `true` or `false`, and every other byte is kept (see config-persistence). The row never changes a task assignment.

#### Scenario: Toggle and save [auto]
- GIVEN a file with `[ai] engine = "claude"` and no `local_only`, the Models tab and `Local only` selected
- WHEN Space and then `s` are pressed
- THEN the file gains `local_only = true` under `[ai]`, `engine = "claude"` is byte-identical, and the mode is on

#### Scenario: Esc discards [auto]
- GIVEN `Local only` toggled to `on` in the draft
- WHEN Esc is pressed at the top level
- THEN the file is unchanged and the mode stays off

#### Scenario: A wrong value shows on [auto]
- GIVEN `[ai] local_only = "yes"` in the file
- WHEN the Models tab is rendered
- THEN `Local only` reads `on` (fail closed); toggling it to `off` and saving writes `local_only = false`

### Requirement: Updates and GitHub rows under local only [S5]
While local-only mode is on, the value of the `Updates` row MUST end with ` · checks off: local only` and `r` on it MUST start no check and show `update checks are off while local only is on`; the `GitHub` row of the Tools tab MUST read `off: local only` and MUST NOT change its saved `ai.tools.github` value when toggled while the mode is on (assumption: toggling shows the same `GitHub tools are off while local only is on` in the footer and changes nothing). With the mode off both rows are as today.

#### Scenario: Rows under local only [auto, TestBackend + insta]
- GIVEN local-only mode on, `[updates] check` unset and `[ai.tools] github = true`
- WHEN the Settings and Tools tabs are rendered and `r` is pressed on `Updates`
- THEN `Updates` ends with ` · checks off: local only`, `GitHub` reads `off: local only`, no check starts and the footer shows `update checks are off while local only is on`

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). Each row MUST belong to exactly one tab, as in the Tab map: `Settings` (`Statusline position` `statusline.position`: top, bottom; `Tab bar position` `tabbar.position`: top, bottom; `Mouse` `mouse`: on, off, written as the booleans `true` and `false`; `Notify`: auto, desktop, bell, off; `Sound`: off, on; `llama-server idle` `ai.servers.llamacpp.idle_minutes`: 5 min, 15 min, 30 min, never, written as a TOML integer, never = 0; `Updates`: the running version, or the newer one when known; `Feedback`: `bug report or idea`, no key path), `Models` (`Set all tasks` aggregate over the five tasks, no single key path; `general` `ai`; `code` `ai.tasks.code`; `chat` `ai.tasks.chat`; `review` `ai.tasks.review`; `summarize` `ai.tasks.summarize`; `orchestrate` its task table; `Hugging Face`, no key path; `Local only` `ai.local_only`: off, on, written as the booleans `false` and `true`), `Tools` (`Tools` `ai.tools.enabled`: off, on; `Search`, `Write`, `Exec`, `Git`, `Git write`, `Issue draft` (on by default): off, on, each with its own `ai.tools.<key>`; `Approval` `ai.tools.approval`: ask, auto, run; `Orchestrator` `ai.orchestrator.enabled`: off, on), `Connectors` (no rows) and `Chat` (`Profile` `ai.profile`: analyst, programmer; `Context` `ai.context`: off, on; `Memory` `ai.memory.enabled`: off, on). `Notify`, `Sound`, `llama-server idle` and `Local only` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached; then `Settings` holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and no other tab exists. Moving a row between tabs MUST NOT change its key path, its values or the config edit it produces.

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every tab has a non-empty label, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context`, `Memory`, `Tools`, `Approval`, `Local only` and the five family rows have their key paths and values above, every tab lists only rows that exist, every row appears on exactly one tab, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound`, `llama-server idle` (`15 min`), `Usage (alpha)`, `Updates` and `Feedback`, each with its current value, and no Models, Tools or Chat row is drawn

#### Scenario: Every row is on its tab [auto, TestBackend + insta]
- GIVEN the AI section attached
- WHEN each of Models, Tools and Chat is rendered
- THEN Models reads `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`, `Local only`; Tools reads `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `Issue draft`, `Approval`, `Orchestrator`; Chat reads `Profile`, `Context`, `Memory`; and no `Notify`, `Sound`, `Updates` or `Feedback` row exists on them

#### Scenario: No AI keeps one tab [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and there is no other tab

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab, Shift+Tab and the jump keys are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The order of each tab is the one in the Tab map: `Settings` ends with `Updates`, `Feedback`; `Models` ends with `orchestrate`, `Hugging Face`, `Local only`; `Tools` ends with `Approval`, `Orchestrator`; `Chat` is `Profile`, `Context`, `Memory`.

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap within Settings [auto]
- GIVEN the `Feedback` row selected on Settings
- WHEN `j` (or Down) is pressed
- THEN `Statusline position` is selected and the active tab is still `Settings`; and from the first row `k` (or Up) selects `Feedback`

#### Scenario: Wrap within Models, Tools and Chat [auto]
- GIVEN each of Models (`Local only` selected), Tools (`Orchestrator` selected) and Chat (`Memory` selected) in turn
- WHEN `j` is pressed, then `k`
- THEN the first row of that tab (`Set all tasks`, `Tools`, `Profile`) is selected, then the last again, and the active tab does not change

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN `Mouse` is selected, `j` again selects `Notify`; from `Notify` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN the Models tab active with `Set all tasks` selected
- WHEN `j` is pressed nine times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`, `Local only` in that order and the ninth press wraps to `Set all tasks`

#### Scenario: Tools tab in order [auto]
- GIVEN the Tools tab active with `Tools` selected
- WHEN `j` is pressed eight times
- THEN the selection visits `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator` in that order and the eighth press wraps to `Tools`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Models tab
The Models tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly seven task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, and then the static rows `Hugging Face` (see Hugging Face account row) and `Local only` (see Local only row). Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there; while local-only mode is on, a row whose effective engine is refused by local only ends with ` · off: local only` (see local-only-mode). The `Hugging Face` row shows its account state. The number of rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size across all tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup. The detection cues, the `r` refresh key and the task hint apply on the task rows only; on the `Hugging Face` and `Local only` rows, on Tools and on Chat rows `r` requests no detection run.

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, no Hugging Face token, and the Models tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `orchestrate` `(default)`, `Hugging Face` `not connected`, `Local only` `off`, and no `Profile`, `Context`, `Memory`, `Tools` or `Orchestrator` row

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned and the Models tab active
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)` and the other five read `(default)`

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the Models tab is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Refresh only on task rows [auto]
- GIVEN the Models tab with `Hugging Face` selected, then the Tools tab with `Approval` selected
- WHEN `r` is pressed in each
- THEN no detection run is requested; and with `code` selected exactly one is

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the Settings, Models, Tools and Chat tabs and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in all and every row of the active tab is visible

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup without panic

#### Scenario: Marks under local only [auto]
- GIVEN local-only mode on, general `claude` / `sonnet`, code `ollama` / `llama3:8b` available, the other tasks unassigned
- WHEN the Models tab is rendered
- THEN `general` reads `claude › sonnet · off: local only`, `code` reads `ollama › llama3:8b` with no such mark, `chat`, `review`, `summarize` and `orchestrate` read `(default) · off: local only`, and `Local only` reads `on`
