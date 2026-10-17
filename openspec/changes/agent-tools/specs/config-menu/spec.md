# Delta for Config Menu

Tags as in the main spec (`openspec/specs/config-menu/spec.md`, tabbed menu: tabs `Settings` and `AI`, `MENU_BODY` 12, popup 16 rows, minimum 24x15). `Tools` and `Approval` are rows of the AI tab, after `Memory`, in that order: `Approval` is the 11th and last AI row. They edit the AI draft, so they are shown only when the AI section is attached. The popup does not change size: the AI tab at 11 rows (plus the bar, 12) equals `MENU_BODY`, and the picker (title plus a 10-row viewport) stays the binding view. The MODIFIED blocks below carry the changed requirements of the main spec under its own headings; sdd-archive MUST merge them over the main spec.

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The tabs are `Settings` and `AI`, in that order (see Menu tabs). The `Settings` tab MUST show exactly six rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom), `Mouse` (`mouse`: on, off; written as the booleans `true` and `false`), `Notify` (four modes: auto, desktop, bell, off), `Sound` (off, on) and `Updates` (the running version, or the newer one when known). `Notify` and `Sound` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached. The `AI` tab MUST show the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static rows `Profile` (`ai.profile`: analyst, programmer), `Context` (`ai.context`: off, on), `Memory` (`ai.memory.enabled`: off, on), `Tools` (`ai.tools.enabled`: off, on) and `Approval` (`ai.tools.approval`: ask, auto), which is the last AI row (the 11th).
(Previously: the static rows ended with `Tools`, the last AI row)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every section has a non-empty title, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context`, `Memory`, `Tools` and `Approval` have their key paths and values above, every tab lists only rows that exist, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound` and `Updates`, each with its current value, and the AI rows are not drawn

#### Scenario: AI rows end with Approval [auto, TestBackend + insta]
- GIVEN the AI tab active
- WHEN rendered
- THEN the rows read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools`, `Approval` in that order, and no `Engine`, `Model`, `Status`, `Notify` or `Sound` row exists there

#### Scenario: No AI hides Notify and Sound [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse` and `Updates`, and there is no AI tab

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab and Shift+Tab are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The `Settings` tab order is `Statusline position`, `Tab bar position`, `Mouse`, `Notify`, `Sound`, `Updates`. The `AI` tab order is `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools`, `Approval`.
(Previously: the AI tab order ended with `Tools`)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap within Settings [auto]
- GIVEN the `Updates` row selected on Settings
- WHEN `j` (or Down) is pressed
- THEN `Statusline position` is selected and the active tab is still `Settings`; and from the first row `k` (or Up) selects `Updates`

#### Scenario: Wrap within AI [auto]
- GIVEN the AI tab active with `Approval` selected
- WHEN `j` is pressed, then `k`
- THEN `Set all tasks` is selected, then `Approval`, and the active tab is still `AI`

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN `Mouse` is selected, `j` again selects `Notify`; from `Notify` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN the AI tab active with `Set all tasks` selected
- WHEN `j` is pressed eleven times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools`, `Approval` in that order and the eleventh press wraps to `Set all tasks`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: AI task list
The AI tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly six task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, and then the static rows `Profile`, `Context`, `Memory`, `Tools` and `Approval`. Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks differ, and the marks `· download required` and `(not detected)` as defined there. The `Profile` row shows the profile id; `Context`, `Memory` and `Tools` show `off` or `on`; `Approval` shows `ask` or `auto`. The number of AI rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size between both tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup; the exact dimensions are a design decision.
(Previously: the static rows ended with `Tools`)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, the profile `analyst`, and the AI tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `Profile` `analyst`, the `Context`, `Memory` and `Tools` rows with their `off` or `on` value and the `Approval` row with `ask` or `auto`

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned and the AI tab active
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)`, the other four read `(default)`, `Profile` reads `programmer` (the default profile), `Context`, `Memory` and `Tools` read their defaults and `Approval` reads `ask`

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the AI tab is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the Settings tab, the AI tab and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in all and every row of the active tab is visible

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup without panic

### Requirement: Popup height is fixed
The popup body height MUST be the maximum over every view: each tab's rows, the picker (title plus its viewport) and the privacy warning. The popup height MUST be that body plus the chrome and MUST NOT change when switching tabs or entering or leaving a picker, the custom field or the warning. The tab bar row is part of the body. The body (`MENU_BODY`) is 12 rows: a 1-row bar plus 11 rows for the tallest view (`VIEW_BODY`: the picker's title plus its 10-row viewport). The chrome stays 4 rows, so the popup is 16 rows. The privacy warning needs 10 rows, the AI tab 11 (with `Tools` and `Approval`), the Settings tab 9 (six rows, one gap, the two-line note) and the custom entry 2; the picker and the AI tab are both at the binding 11, so the two rows of this change fit with no size change and the minimum stays 24 columns by 15 rows. The single-tab (no-AI) layout keeps its previous size.
(Previously: the AI tab needed 10 rows and Approval was foreseen as a fit without a size change)

#### Scenario: Same height across views [auto, TestBackend + insta]
- GIVEN a normal terminal
- WHEN the Settings tab, the AI tab (with `Approval`), a picker level 1 and 2, the custom field and the warning are rendered
- THEN the popup border spans 16 rows in all of them

#### Scenario: Adding a row grows only the tallest view [auto]
- GIVEN the height is derived from the tables, with the AI tab at 11 rows, which equals `VIEW_BODY` 11
- WHEN the tallest view is measured, and then a 12th AI row is added
- THEN the fixed body equals the tallest view and no view exceeds it, and with the 12th AI row the popup grows by one row (only the tallest view moves the height)

## ADDED Requirements

### Requirement: Tools row
The AI tab MUST hold a static row `Tools` after `Memory` and before `Approval` (key path `ai.tools.enabled`, values `off`, `on`; it reads `on` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft (an Enter repeat is ignored, like on `Context`); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to a stream, a pending approval or a running tool, and it applies from the next turn after a successful save. Esc at the top level restores the draft; `s` saves `ai.tools.enabled` through config-persistence only when it differs. The row order is `... Memory, Tools, Approval` (see Sections and items are data); Navigation wraps inside the AI tab; the popup does not change size (see Popup height is fixed); the descriptor integrity rules extend to the row (`Tools` has key path `ai.tools.enabled`, values `off`, `on`, no duplicate key path).

#### Scenario: Navigation reaches Tools and wraps [auto]
- GIVEN the AI tab active with `Memory` selected
- WHEN `j` is pressed three times
- THEN `Tools`, then `Approval`, then `Set all tasks` is selected, the active tab is still `AI`, and from `Set all tasks` `k` selects `Approval`

#### Scenario: Toggle [auto]
- GIVEN `Tools` selected with the draft `on`
- WHEN `l` is pressed, then `h`
- THEN the draft is `off`, then `on`, no save is requested and no file is touched

#### Scenario: Enter repeat ignored [auto]
- GIVEN `Tools` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: Footer and refresh [auto, TestBackend + insta]
- GIVEN `Tools` selected and detection in flight
- WHEN rendered, and `r` is pressed
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and no detection run is requested

#### Scenario: No side effects on change [auto]
- GIVEN a running stream with a pending approval and the menu open on `Tools`
- WHEN the draft is cycled to `off`
- THEN no ResizePty or WritePty is emitted, the stream and the approval are untouched

#### Scenario: Applies from the next turn [auto]
- GIVEN the draft turned `off` and saved while a turn is running
- WHEN the next turn starts
- THEN the request is built without tools, and the running turn was not interrupted

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with tools `on` and the draft changed to `off`
- WHEN Esc is pressed on the task list
- THEN tools are `on` and no file is read or written

#### Scenario: Save [auto]
- GIVEN the draft changed to `off`
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes, the file holds `enabled = false` under `[ai.tools]` and tools are off

#### Scenario: Popup size and fallback [auto, TestBackend + insta]
- GIVEN a terminal at exactly the minimum (24 columns by 15 rows), and one column or one row less
- WHEN the AI tab and the picker are rendered
- THEN the first shows the full popup (clipped, no panic) and the second shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Tools` exists with key path `ai.tools.enabled` and values `off`, `on`, and no key path is duplicated

#### Scenario: Tools row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Tools` is turned off and saved, and a chat turn is sent
- THEN no tool row, no run hint and no confirm ever appears, and turning it on again restores them

### Requirement: Approval row
The AI tab MUST end with a static row `Approval` after `Tools` (key path `ai.tools.approval`, values `ask`, `auto`; it reads `ask` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards and at the next conversation switch inside an open pane. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs, and the value is written explicitly even when it equals the default. The row is shown whatever the `Tools` draft. With it the AI tab has 11 rows and the popup keeps its 16 rows and its 24x15 minimum (see Popup height is fixed); the descriptor integrity rules extend to the row.

Note: when the file holds a bad value (for example `approval = "AUTO"`), the row opens on `ask` and choosing `ask` writes nothing, so the bad value stays in the file until `auto` is saved. `Tools` behaves the same way. This is accepted (design D18); no force flag exists.

#### Scenario: Cycle [auto]
- GIVEN `Approval` selected with the draft `ask`
- WHEN `l` is pressed, then `l` again
- THEN the draft is `auto`, then `ask`, no save is requested and no file is touched

#### Scenario: Enter repeat ignored [auto]
- GIVEN `Approval` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: No effect on open chats [auto]
- GIVEN an open chat in `ask` and the menu open on `Approval`
- WHEN the draft is cycled to `auto` and saved
- THEN the open chat is still `ask`, no WritePty is emitted, and a chat pane opened afterwards is `auto`

#### Scenario: Esc reverts [auto]
- GIVEN the draft changed to `auto`
- WHEN Esc is pressed on the task list
- THEN the saved mode is still `ask` and no file is read or written

#### Scenario: Footer [auto, TestBackend + insta]
- GIVEN `Approval` selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and the value reads `ask`

#### Scenario: Popup size is unchanged [auto, TestBackend + insta]
- GIVEN the AI tab with `Approval`, a terminal of 80x24 and one of exactly 24x15
- WHEN rendered
- THEN both show the full popup of 16 rows (the 24x15 one clipped, without panic), and one column or one row less shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Approval` exists with key path `ai.tools.approval` and values `ask`, `auto`, and no key path is duplicated

#### Scenario: Approval row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Approval` is set to `auto` and saved, and a new chat is opened
- THEN the new chat shows `AUTO`, the chat opened before does not, and the file holds `approval = "auto"` under `[ai.tools]`
