# Delta for Config Menu

## Base of this delta

This delta targets `openspec/specs/config-menu/spec.md`. MODIFIED blocks copy the requirement text with edits marked "(Previously: ...)"; for long requirements only the scenarios that change are listed and every other scenario stays. All slice [S3]. Decisions J4 (a `judge` row, model from the menu) and J11 (unassigned shows `(default)`; `Set all tasks` includes `judge`).

Row order after this change on the Models tab: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge`, `Hugging Face`. Any other scenario of the base that lists the Models rows (the "Every row is on its tab" scenario of "Sections and items are data", the "Task rows rendered" scenario of "Models tab", and the "Row is present and ordered" scenario of "Simple row and picker") MUST read with `judge` between `simple` and `Hugging Face`. The base text "exactly seven task rows" of "Models tab" predates `simple`; this delta states the count again.

## MODIFIED Requirements

### Requirement: Models tab [S3]
The Models tab MUST show, as its rows (with no section title row while the tab bar is shown), `Set all tasks` followed by exactly eight task rows in this order: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge`, and then the static row `Hugging Face` (see Hugging Face account row). Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there. The `Hugging Face` row shows its account state. The number of rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size across all tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup. The detection cues, the `r` refresh key and the task hint apply on the task rows only; on the `Hugging Face` row, on Tools and on Chat rows `r` requests no detection run.
(Previously: "exactly seven task rows ... `summarize`, `orchestrate`" with no `simple` and no `judge`)

Existing scenarios stay unchanged except "Task rows rendered", whose THEN reads `... `orchestrate` `(default)`, `simple` `(default)`, `judge` `(default)`, `Hugging Face` `not connected` ...`. Added scenario:

#### Scenario: The popup keeps its size with the extra row [S3, auto, TestBackend + insta]
- GIVEN the Models tab on a normal terminal
- WHEN it is rendered with the `judge` row
- THEN every row is drawn whole, the popup has the same size as the other tabs and the picker, and the one-line fallback threshold still follows the popup

### Requirement: Navigation [S3]
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab, Shift+Tab and the jump keys are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The order of each tab is the one in the Tab map: `Settings` ends with `Updates`, `Feedback`; `Models` ends with `orchestrate`, `simple`, `judge`, `Hugging Face`; `Tools` ends with `Approval`, `Orchestrator`; `Chat` is `Profile`, `Context`, `Memory`.
(Previously: `Models` ended with `orchestrate`, `simple`, `Hugging Face`)

Existing scenarios stay unchanged except "Task rows in order", replaced by:

#### Scenario: Task rows in order [S3, auto]
- GIVEN the Models tab active with `Set all tasks` selected
- WHEN `j` is pressed nine times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge`, `Hugging Face` in that order and the tenth press wraps to `Set all tasks`

### Requirement: Hugging Face account row [S3]
The Models tab MUST end with a static row `Hugging Face`, after the `judge` row. Its value, its field, its keys and its behaviour are unchanged.
(Previously: "after the `simple` row". The full text of this requirement stays as it is, with only that position changed.)

Existing scenarios stay unchanged except "Account row", whose THEN reads `the last row reads `Hugging Face` with `not connected`, after `judge``.

## ADDED Requirements

### Requirement: Judge row and picker [S3]
The Models tab MUST show a `judge` row after `simple` and before `Hugging Face`. Enter on it MUST open the same two-level task picker as the other tasks, with `(default · use general)` as its first entry. Choosing an entry changes only `judge`. Its row reads `(default)` when unassigned and `<engine> › <model>` when assigned. The row MUST NOT carry `not recommended` on any engine. `Set all tasks` DOES write, clear and summarize `judge` (unlike `orchestrate` and `simple`), so after Set all the `judge` row shows the chosen value. The row MUST be reachable with the same navigation keys as every other row.

#### Scenario: Row is present and ordered [S3, auto]
- GIVEN the Models tab
- WHEN the rows are built
- THEN they read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `judge`, `Hugging Face`

#### Scenario: Picker changes only judge [S3, auto]
- GIVEN `general` is `codex` and the picker for `judge`
- WHEN `claude` / `opus` is chosen
- THEN `judge` is `claude` / `opus` and the other tasks are unchanged

#### Scenario: Unassigned row says default [S3, auto]
- GIVEN `judge` unassigned and `general` `claude` / `sonnet`
- WHEN the Models tab is rendered
- THEN the `judge` row reads `(default)`

#### Scenario: Navigation visits the row [S3, auto]
- GIVEN the Models tab with `simple` selected
- WHEN `j` is pressed
- THEN `judge` is selected, and one more press selects `Hugging Face`

#### Scenario: Set all includes judge [S3, auto]
- GIVEN `judge` is `claude` / `opus`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN `judge` is `codex` / `default`, and `orchestrate` and `simple` keep their values

#### Scenario: No not-recommended mark [S3, auto]
- GIVEN `judge` assigned to a server engine
- WHEN the row is rendered
- THEN it carries no `not recommended` mark
