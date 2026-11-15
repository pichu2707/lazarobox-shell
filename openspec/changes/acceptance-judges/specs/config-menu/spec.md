# Delta for Config Menu

## Base of this delta

This delta targets `openspec/specs/config-menu/spec.md`. MODIFIED blocks copy the requirement text with edits marked "(Previously: ...)"; for long requirements only the scenarios that change are listed and every other scenario stays. All slice [S3]. Decisions J4 (judge rows, model from the menu), J11 (unassigned rows show `(default)`) and J17 (the judges are two rows, `judge-a` and `judge-b`, in the section `ODD + RDD roles` of issue #765; `Set all tasks` does not include them, `Set all ODD + RDD roles` does).

Row order after this change on the Models tab: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, then the section title `ODD + RDD roles` (not selectable), `Set all ODD + RDD roles`, `judge-a`, `judge-b`, and `Hugging Face`. Any other scenario of the base that lists the Models rows (the "Every row is on its tab" scenario of "Sections and items are data", the "Task rows rendered" scenario of "Models tab", and the "Row is present and ordered" scenario of "Simple row and picker") MUST read with the section (title, `Set all ODD + RDD roles`, `judge-a`, `judge-b`) between `simple` and `Hugging Face`. The base text "exactly seven task rows" of "Models tab" predates `simple`; this delta states the count again. This slice builds the section header and the two judge rows only; the other roles of #765 (coordinator, code, simple, the four lenses, refuter, corrector) and the sub-headers of the section belong to a later change, which widens the section without changing these rows.

## MODIFIED Requirements

### Requirement: Models tab [S3]
The Models tab MUST show, as its rows (with no section title row for the tab itself while the tab bar is shown), `Set all tasks` followed by exactly seven task rows in this order: `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`; then the section `ODD + RDD roles`, made of a non-selectable title row `ODD + RDD roles`, the row `Set all ODD + RDD roles` and the two rows `judge-a` and `judge-b`; and then the static row `Hugging Face` (see Hugging Face account row). Each task row and each role row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task or role, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for an aggregate when the tasks or roles it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there. The `Hugging Face` row shows its account state. The number of rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size across all tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup. The detection cues, the `r` refresh key and the task hint apply on the task rows and the judge rows only; on the `Set all` rows, the `Hugging Face` row, on Tools and on Chat rows `r` requests no detection run.
(Previously: "exactly seven task rows ... `summarize`, `orchestrate`" with no `simple` and no section)

Existing scenarios stay unchanged except "Task rows rendered", whose THEN reads `... `orchestrate` `(default)`, `simple` `(default)`, the title `ODD + RDD roles`, `Set all ODD + RDD roles` `(not set)`, `judge-a` `(default)`, `judge-b` `(default)`, `Hugging Face` `not connected` ...`. Added scenarios:

#### Scenario: The popup keeps its size with the section [S3, auto, TestBackend + insta]
- GIVEN the Models tab on a normal terminal
- WHEN it is rendered with the `ODD + RDD roles` section
- THEN every row is drawn whole, the popup has the same size as the other tabs and the picker, and the one-line fallback threshold still follows the popup

#### Scenario: The section title is not a row to select [S3, auto]
- GIVEN the Models tab
- WHEN the rows are built
- THEN the title `ODD + RDD roles` is drawn as a title and no selection ever rests on it

### Requirement: Navigation [S3]
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab, Shift+Tab and the jump keys are the only way across tabs. Title rows are not selectable and are passed over; no other row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The order of each tab is the one in the Tab map: `Settings` ends with `Updates`, `Feedback`; `Models` ends with `orchestrate`, `simple`, `Set all ODD + RDD roles`, `judge-a`, `judge-b`, `Hugging Face`; `Tools` ends with `Approval`, `Orchestrator`; `Chat` is `Profile`, `Context`, `Memory`.
(Previously: `Models` ended with `orchestrate`, `simple`, `Hugging Face`)

Existing scenarios stay unchanged except "Task rows in order", replaced by:

#### Scenario: Task rows in order [S3, auto]
- GIVEN the Models tab active with `Set all tasks` selected
- WHEN `j` is pressed eleven times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, `Set all ODD + RDD roles`, `judge-a`, `judge-b`, `Hugging Face` in that order and the twelfth press wraps to `Set all tasks`

### Requirement: Hugging Face account row [S3]
The Models tab MUST end with a static row `Hugging Face`, after the `judge-b` row. Its value, its field, its keys and its behaviour are unchanged.
(Previously: "after the `simple` row". The full text of this requirement stays as it is, with only that position changed.)

Existing scenarios stay unchanged except "Account row", whose THEN reads `the last row reads `Hugging Face` with `not connected`, after `judge-b``.

## ADDED Requirements

### Requirement: ODD + RDD roles section, judge rows and picker [S3]
The Models tab MUST show the section `ODD + RDD roles` after `simple` and before `Hugging Face`, with, in this order, a non-selectable title row, the row `Set all ODD + RDD roles`, `judge-a` and `judge-b`. Enter on `judge-a` or `judge-b` MUST open the same two-level task picker as the other tasks, with `(default)` as its first entry (what an unassigned judge uses is stated in ai-task-models). Choosing an entry changes only that judge. A judge row reads `(default)` when unassigned and `<engine> › <model>` when assigned. The rows MUST NOT carry `not recommended` on any engine. `Set all ODD + RDD roles` writes, clears and summarizes the roles of the section (in this slice `judge-a` and `judge-b`) and does NOT touch `general`, `code`, `chat`, `review`, `summarize`, `orchestrate` or `simple`; `Set all tasks` does NOT touch the judges. The rows MUST be reachable with the same navigation keys as every other row.

#### Scenario: Rows are present and ordered [S3, auto]
- GIVEN the Models tab
- WHEN the rows are built
- THEN they read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `simple`, the title `ODD + RDD roles`, `Set all ODD + RDD roles`, `judge-a`, `judge-b`, `Hugging Face`

#### Scenario: Picker changes only the chosen judge [S3, auto]
- GIVEN `general` is `codex` and the picker for `judge-a`
- WHEN `claude` / `opus` is chosen
- THEN `judge-a` is `claude` / `opus` and `judge-b` and every other task are unchanged

#### Scenario: Unassigned rows say default [S3, auto]
- GIVEN `judge-a` and `judge-b` unassigned and `general` `claude` / `sonnet`
- WHEN the Models tab is rendered
- THEN both rows read `(default)`

#### Scenario: Navigation visits the rows [S3, auto]
- GIVEN the Models tab with `simple` selected
- WHEN `j` is pressed
- THEN `Set all ODD + RDD roles` is selected (the title is passed over), then `judge-a`, then `judge-b`, then `Hugging Face`

#### Scenario: Set all ODD + RDD roles sets the judges only [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`, `judge-b` is unassigned and `review` is `ollama` / `llama3:8b`
- WHEN `codex` / `default` is chosen for `Set all ODD + RDD roles`
- THEN `judge-a` and `judge-b` are `codex` / `default` and `review`, `orchestrate`, `simple` and the other tasks keep their values

#### Scenario: Set all tasks leaves the judges alone [S3, auto]
- GIVEN `judge-a` is `claude` / `opus`
- WHEN `codex` / `default` is chosen for Set all tasks
- THEN `judge-a` is still `claude` / `opus` and `judge-b` is unchanged

#### Scenario: No not-recommended mark [S3, auto]
- GIVEN `judge-a` assigned to a server engine
- WHEN the row is rendered
- THEN it carries no `not recommended` mark
