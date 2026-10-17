# Delta for Config Menu (config-menu-tabs)

Note: this delta is written from the code (`src/core/menu.rs`, `src/ui/components/menu.rs`), not from the main spec, which has drifted. Requirements marked "drift fix" correct the main spec to match shipped behavior. After this change merges, the agent-tools `config-menu` deltas (Tools, Approval) and D18's size paragraph assume the stacked layout and MUST be rebased onto tabs. That work is outside this change.

Terms: "menu tab" is a tab of the config popup (Settings, AI). It is not a pane tab: where this spec says "the tab bar MUST be shown while the menu is open" (Live preview and revert) it means the pane tab bar, which is unchanged. "Menu tab bar" is the one-line strip of menu tabs inside the popup. "Body" is the popup content between the borders, excluding the blank row and footer.

## ADDED Requirements

### Requirement: Menu tabs
The popup MUST group its rows into menu tabs. The tabs MUST come from a static table (label plus the rows it shows); adding a tab MUST NOT require changing navigation or drawing code. The table order is `Settings`, `AI`. The `AI` tab exists only when the AI section is attached. An `MCP` tab MUST NOT be listed until agent-tools 4a adds its table entry. `Tab` MUST activate the next tab; `Tab` with SHIFT and `BackTab` (with or without SHIFT) MUST activate the previous one. Wrap is across tabs (Settings, AI, Settings). Ctrl+Tab and Alt+Tab MUST stay swallowed. With one tab, Tab does nothing. A held key repeats like `j`/`k`: Repeat events switch tabs like Press. The cursor MUST be remembered per tab: leaving a tab and coming back MUST restore the row that was selected there. Each tab with at least one row starts on its first row. Switching tabs MUST NOT change any draft value and MUST NOT emit `ResizePty`, `WritePty` or a detection run.

#### Scenario: Tab cycles forward with wrap [auto]
- GIVEN the AI section attached and the active tab `Settings`
- WHEN Tab is pressed, then Tab again
- THEN the active tab is `AI`, then `Settings`

#### Scenario: Shift+Tab, both encodings [auto]
- GIVEN the active tab `Settings`
- WHEN `BackTab` is pressed, in a separate run `BackTab` with SHIFT, and in another `Tab` with SHIFT
- THEN in all runs the active tab is `AI`; and pressing the same key again returns to `Settings`

#### Scenario: Cursor is remembered per tab [auto]
- GIVEN the `Mouse` row selected on `Settings`, then Tab to `AI`, `j` pressed twice (on `code`)
- WHEN Shift+Tab then Tab are pressed
- THEN `Mouse` was selected while on `Settings`, and `code` is selected again on `AI`

#### Scenario: Modified Tab and repeat [auto]
- GIVEN the active tab `Settings`
- WHEN Ctrl+Tab and Alt+Tab are pressed, then a Repeat event for Tab arrives, then a Repeat for `BackTab`
- THEN the first two change nothing, the Repeat of Tab moves to `AI`, and the Repeat of `BackTab` returns to `Settings`

#### Scenario: Switching tabs changes nothing else [auto]
- GIVEN a changed draft and the menu open
- WHEN Tab and Shift+Tab are pressed
- THEN the draft is unchanged and no ResizePty, WritePty, Refresh or Save is emitted

#### Scenario: MCP is not listed [auto]
- GIVEN the tab table and the AI section attached
- WHEN the tabs are listed
- THEN they are exactly `Settings` and `AI`, in that order

### Requirement: Menu tab bar
The popup MUST draw a one-line menu tab bar as the first body row whenever there are two or more tabs. Each label is padded with one space on each side and labels are joined by `│`: with two tabs the bar ` Settings │ AI ` is 15 cells, and with `Settings*` it is 16 (BAR). The active tab MUST be drawn bold, `bg_base` on `primary_cyan` (like the main tab bar, and not like the selected row); idle tabs, the separator and the hint are `text_muted`. The hint `⇥ Tab` (5 cells) is right-aligned and ends one column before the inner right edge. With W = inner width (popup width minus 2), the bar MUST degrade in this order: bar plus hint when W >= BAR + 7; bar only when BAR <= W; only the active tab as ` ‹ AI › ` (its label between `‹` and `›`), clipped, when W < BAR. Below 24 columns the existing one-line fallback applies. The active-only step is a guard: with three or fewer tabs it cannot occur in the full popup, so it is specified as a rule on the bar line and not by an end-to-end scenario. The bar MUST be clipped to the popup and MUST NOT panic at any size. While an update is waiting (see Updates row) the `Settings` label MUST read `Settings*`. With a single tab (the no-AI path) the bar MUST be hidden and the popup layout MUST be unchanged from before this change. The hint is hidden while a sub-level is open (see Sub-levels keep the bar inert). With tabs shown, section titles are not drawn (the bar names the tab); with the bar hidden, section titles are drawn as before.

#### Scenario: Active tab highlighted [auto, TestBackend + insta]
- GIVEN the menu open on a normal terminal, active tab `Settings`, then Tab pressed
- WHEN each state is rendered
- THEN the bar lists `Settings` and `AI`, with `Settings` in the active style in the first and `AI` in the second, idle tabs and the `⇥ Tab` hint in `text_muted`, and no section title rows are drawn

#### Scenario: Hint dropped at narrow widths [auto]
- GIVEN two tabs and an update waiting (BAR = 16) at a popup width of 24 (W = 22 < BAR + 7 = 23)
- WHEN rendered
- THEN both labels show and `⇥ Tab` does not; at popup width 25 the hint shows

#### Scenario: Bar at 24 columns [auto, TestBackend + insta]
- GIVEN two tabs (BAR = 15), no update, a terminal 24 columns wide and 15 rows high
- WHEN rendered
- THEN the full popup is drawn with the bar and the `⇥ Tab` hint (W = 22 >= 22), clipped inside the popup

#### Scenario: Bar at 24 columns with Settings* [auto, TestBackend + insta]
- GIVEN two tabs and an update waiting (BAR = 16), a terminal 24 columns wide
- WHEN rendered
- THEN the bar reads ` Settings* │ AI ` without the hint

#### Scenario: Fallback at 22 columns [auto]
- GIVEN the menu open with the AI section
- WHEN rendered at a terminal 22 columns wide
- THEN the one-line fallback is drawn and no bar

#### Scenario: Active-only guard [auto]
- GIVEN the bar line function with labels whose bar is wider than W
- WHEN it is built
- THEN it yields only the active tab as ` ‹ label › `, clipped to W, without panic

#### Scenario: Settings marked while an update waits [auto]
- GIVEN a newer version known and not yet installed, active tab `AI`
- WHEN rendered
- THEN the bar reads `Settings*` for the Settings label

#### Scenario: No update, no marker [auto]
- GIVEN no newer version known
- WHEN rendered
- THEN the label reads `Settings` without `*`

#### Scenario: Single tab hides the bar [auto, TestBackend + insta]
- GIVEN the menu without the AI section (one tab)
- WHEN rendered
- THEN no bar is drawn, Tab and Shift+Tab are swallowed with no effect, and the four no-AI snapshots are byte-identical to before this change

### Requirement: Sub-levels keep the bar inert
While a sub-level is open (privacy warning, picker level 1 or 2, custom field) the menu tab bar MUST stay drawn with the same styles, with the active tab unchanged, but WITHOUT the `⇥ Tab` hint (it would advertise a key that is swallowed), and MUST be inert: Tab and Shift+Tab (both encodings) MUST be swallowed with no effect in the picker and the custom field (the custom field MUST NOT type them). The privacy warning MUST keep declining on any key other than an unmodified lowercase `y`, Tab and Shift+Tab included, and closes only the warning view.

#### Scenario: Tab inert in the picker [auto]
- GIVEN the picker open at level 1, then at level 2
- WHEN Tab, `BackTab` and Tab+SHIFT are pressed
- THEN the picker view, its cursor, the active tab and the draft are unchanged, the bar is still drawn and it has no `⇥ Tab` hint

#### Scenario: Tab inert in the custom field [auto]
- GIVEN the custom field focused with buffer `ab`
- WHEN Tab and BackTab are pressed
- THEN the buffer is still `ab`, the field stays focused and the active tab is unchanged

#### Scenario: Tab declines the warning [auto]
- GIVEN the privacy warning shown, active tab `AI`
- WHEN Tab is pressed
- THEN the warning closes, the draft stays `off`, nothing is acknowledged, and the active tab is still `AI`

### Requirement: Popup height is fixed
The popup body height MUST be the maximum over every view: each tab's rows, the picker (title plus its viewport) and the privacy warning. The popup height MUST be that body plus the chrome and MUST NOT change when switching tabs or entering or leaving a picker, the custom field or the warning. The tab bar row is part of the body. The body (`MENU_BODY`) is 12 rows: a 1-row bar plus 11 rows for the tallest view (`VIEW_BODY`: the picker's title plus its 10-row viewport). The chrome stays 4 rows, so the popup is 16 rows (was 22). The privacy warning needs 10 rows, the AI tab 10, the Settings tab 9 (six rows, one gap, the two-line note) and the custom entry 2; the picker is the binding term. Adding one row to the AI tab (agent-tools 2e-3, Approval) fits with no size change. The single-tab (no-AI) layout keeps its previous size.

#### Scenario: Same height across views [auto, TestBackend + insta]
- GIVEN a normal terminal
- WHEN the Settings tab, the AI tab, a picker level 1 and 2, the custom field and the warning are rendered
- THEN the popup border spans 16 rows in all of them

#### Scenario: Adding a row grows only the tallest view [auto]
- GIVEN the height is derived from the tables
- WHEN the tallest view is measured
- THEN the fixed body equals the tallest view and no view exceeds it

### Requirement: Update note is Settings-only
The two-line update note MUST be drawn only on the Settings tab, inside the fixed body in its last 2 rows (just above the blank row and the footer), and MUST NOT appear on the AI tab or in any sub-level. The note MUST NOT make the popup taller than the fixed height and MUST NOT trigger the fallback. On the single-tab path the note keeps its previous position under the list.

#### Scenario: Note on Settings [auto, TestBackend + insta]
- GIVEN a newer version known, active tab `Settings`
- WHEN rendered
- THEN the update note shows in the last 2 body rows, above the blank row and the footer

#### Scenario: No note on AI [auto]
- GIVEN a newer version known, active tab `AI`
- WHEN rendered
- THEN the note text is absent and the popup height equals its height on Settings

### Requirement: Notify, Sound and Updates keys (drift fix)
On the `Notify` or `Sound` row, `t` (Press only, not Repeat) MUST send a test notification using the draft setting, and the footer is `j/k move · h/l change · t test · s save · Esc cancel`; the outcome is shown in the footer until the next key press. On the `Updates` row, Enter MUST request the update when a newer version is known and none is installed yet; with the install command shown, Enter or `y` MUST copy it; otherwise Enter has no effect (it does not cycle). The `Updates` row reads the running version, or `<latest> available` with the label `* Updates` when newer is known.

#### Scenario: Test notification [auto]
- GIVEN `Notify` selected and the AI section attached
- WHEN `t` is pressed, and a Repeat event for `t` arrives
- THEN exactly one test is requested, with the draft mode

#### Scenario: `t` elsewhere is inert [auto]
- GIVEN `Mouse` selected
- WHEN `t` is pressed
- THEN no test is requested and nothing changes

#### Scenario: Enter on Updates [auto]
- GIVEN `Updates` selected with a newer version known and none installed
- WHEN Enter is pressed
- THEN an update is requested; with no newer version known, Enter has no effect

## MODIFIED Requirements

### Requirement: Open and close
The menu MUST open only from TERMINAL through the prefix: `Ctrl+Space` then `m`, always on the `Settings` tab, on its first row (`Statusline position`), with the per-tab cursor memory cleared and the picker closed. Opening MUST NOT write to any PTY. The menu MUST close on `s` (after a successful save, or at once when nothing changed) and on Esc at the top level of the task list (after reverting). Esc while a picker is open MUST go back exactly one level (custom field to level 2, level 2 to level 1, level 1 to the task list) and MUST NOT close the menu or revert the draft. This Esc rule takes precedence wherever Esc is described. While open the mode is MENU (see modal-input). Opening captures the current values of the menu-owned settings as the "original" values and as the initial draft.
(Previously: opened on the first row of a single stacked list; no tab or cursor memory.)

#### Scenario: Open [auto]
- GIVEN TERMINAL
- WHEN `Ctrl+Space` then `m` are pressed
- THEN mode is MENU, the menu is visible on the Settings tab with the first row selected, and no WritePty is emitted

#### Scenario: Esc closes [auto]
- GIVEN MENU with no changes
- WHEN Esc is pressed
- THEN mode is TERMINAL, the menu is gone, no file is touched and no WritePty is emitted

#### Scenario: Reopen starts fresh [auto]
- GIVEN the menu was opened, changed, and cancelled with Esc
- WHEN it is opened again
- THEN the active tab is `Settings`, the selection is on its first row and the draft equals the current (reverted) values

#### Scenario: Reopen resets tab and cursor [auto]
- GIVEN the menu closed while the AI tab was active with `code` selected, and the Settings cursor had been moved to `Mouse`
- WHEN the menu is reopened
- THEN the active tab is `Settings`, the selected row is `Statusline position`, and after Tab then Shift+Tab the Settings row is again the first, not `Mouse`

#### Scenario: Esc in the picker goes back one level [auto]
- GIVEN the picker is open at level 2 for `code` after changing the Statusline position in the draft
- WHEN Esc is pressed, then Esc again, then Esc a third time
- THEN level 1 shows, then the task list shows on the AI tab with `code` selected and the draft unchanged, then the menu closes and the Statusline position is reverted

#### Scenario: Reopen after picker is on the task list [auto]
- GIVEN the menu closed with Esc while no picker was open, after a picker was used earlier
- WHEN the menu is reopened
- THEN the task list shows on the Settings tab, the picker is closed and the draft equals the current values

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The tabs are `Settings` and `AI`, in that order (see Menu tabs). The `Settings` tab MUST show exactly six rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom), `Mouse` (`mouse`: on, off; written as the booleans `true` and `false`), `Notify` (four modes: auto, desktop, bell, off), `Sound` (off, on) and `Updates` (the running version, or the newer one when known). `Notify` and `Sound` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached. The `AI` tab MUST show the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static rows `Profile` (`ai.profile`: analyst, programmer), `Context` (`ai.context`: off, on), `Memory` (`ai.memory.enabled`: off, on) and `Tools` (off, on), which is the last AI row.
(Previously: two sections, Settings with 3 rows and AI ending with `Notify`; Notify and Sound were not Settings rows. Drift fixes: six Settings rows, four-mode Notify, Sound, Updates, Tools last.)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every section has a non-empty title, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context` and `Memory` have their key paths and values above, every tab lists only rows that exist, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound` and `Updates`, each with its current value, and the AI rows are not drawn

#### Scenario: AI rows end with Tools [auto, TestBackend + insta]
- GIVEN the AI tab active
- WHEN rendered
- THEN the rows read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools` in that order, and no `Engine`, `Model`, `Status`, `Notify` or `Sound` row exists there

#### Scenario: No AI hides Notify and Sound [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse` and `Updates`, and there is no AI tab

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab and Shift+Tab are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The `Settings` tab order is `Statusline position`, `Tab bar position`, `Mouse`, `Notify`, `Sound`, `Updates`. The `AI` tab order is `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools`.
(Previously: one global list across all sections; `j` from the last row wrapped to the first of the whole menu.)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap within Settings [auto]
- GIVEN the `Updates` row selected on Settings
- WHEN `j` (or Down) is pressed
- THEN `Statusline position` is selected and the active tab is still `Settings`; and from the first row `k` (or Up) selects `Updates`

#### Scenario: Wrap within AI [auto]
- GIVEN the AI tab active with `Tools` selected
- WHEN `j` is pressed, then `k`
- THEN `Set all tasks` is selected, then `Tools`, and the active tab is still `AI`

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN `Mouse` is selected, `j` again selects `Notify`; from `Notify` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN the AI tab active with `Set all tasks` selected
- WHEN `j` is pressed ten times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools` in that order and the tenth press wraps to `Set all tasks`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Input is swallowed
In MENU every key not listed above MUST be swallowed with no effect. The listed keys are `j`, `k`, Up, Down, `h`, `l`, Left, Right, Space, Enter, `s`, `r`, `t`, `y` (on the Updates command and the warning), Tab, Shift+Tab (`BackTab` with or without SHIFT, and Tab with SHIFT) and Esc, each with the behavior described in this spec, and inside the custom field typed characters, Backspace and Ctrl+U. Ctrl+Tab and Alt+Tab are unlisted and swallowed. Tab and Shift+Tab are listed at the top level only; in the picker, the custom field and the warning they follow Sub-levels keep the bar inert. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL), also inside the custom field. Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter cycles or selects like Enter); uppercase letters such as `J`, `S` and `R` are unlisted and swallowed outside the custom field. `r` is listed only for the refresh behavior (see Refresh key) and `s` only for saving (see config-persistence).
(Previously: Tab was an unlisted key at the top level and swallowed.)

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x` or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

#### Scenario: Tab is listed at the top level [auto]
- GIVEN MENU at the top level with two tabs
- WHEN Tab is pressed
- THEN the active tab changes, no WritePty is emitted and mode stays MENU

#### Scenario: Paste ignored [auto]
- GIVEN MENU with the custom field not focused
- WHEN a paste event arrives
- THEN no WritePty is emitted and the menu is unchanged

#### Scenario: Uppercase S is swallowed [auto]
- GIVEN the task list with a changed draft
- WHEN `S` is pressed
- THEN no save is requested and the menu is unchanged

### Requirement: Tiny terminals
The menu MUST never panic at any terminal size down to 0x0, on every tab and in every picker view. The popup width is 62 with the AI section (51 without) and its height is the fixed popup height (see Popup height is fixed: `MENU_BODY` 12 plus `CHROME` 4 rows of chrome: two borders, the blank row and the footer, so 16). The full popup needs at least 24 columns by 15 rows, the smallest full popup (the blank row is the first thing dropped); adding a row to the tallest view grows these numbers. Below that the menu MUST draw only a one-line fallback message instead of the full popup; the key handling MUST be unchanged, so Esc still reverts and closes (or goes back one level when a picker is open), `s` still saves, and Tab and Shift+Tab still switch tabs. Content that does not fit MUST be clipped, including the menu tab bar.
(Previously: popup 62x19 in the main spec, 62x22 in the code; minimum 24 columns by `AI_BODY + 3`.)

#### Scenario: No panic at any size [auto]
- GIVEN the menu open on each tab, then in each picker view and the warning
- WHEN rendered at sizes from 0x0 up through small widths and heights
- THEN nothing panics

#### Scenario: Minimal message and Esc works [auto]
- GIVEN a terminal smaller than the menu minimum and the menu open
- WHEN rendered, then Esc is pressed
- THEN a minimal message is shown instead of the popup and Esc reverts and closes

#### Scenario: Minimal message and s saves [auto]
- GIVEN a terminal smaller than the menu minimum, the menu open and a changed draft
- WHEN `s` is pressed
- THEN a save is requested

#### Scenario: Picker too small keeps keys [auto]
- GIVEN a terminal one column narrower than 24, and one 24 wide but 14 rows high, and the picker open at level 1
- WHEN rendered, then Esc is pressed
- THEN the one-line fallback is shown, then the task list view is the current view

#### Scenario: Smallest full popup [auto, TestBackend + insta]
- GIVEN a terminal of exactly the minimum size (24 columns by 15 rows)
- WHEN the Settings tab, the AI tab and the picker are rendered
- THEN the full popup is drawn (not the fallback), clipped, without panic

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup

### Requirement: AI task list
The AI tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly six task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, and then the static rows `Profile`, `Context`, `Memory` and `Tools`. Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks differ, and the marks `· download required` and `(not detected)` as defined there. The `Profile` row shows the profile id; `Context`, `Memory` and `Tools` show `off` or `on`. The number of AI rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size between both tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup; the exact dimensions are a design decision.
(Previously: AI rows stacked under the Settings rows, ending with `Notify`; the popup showed both sections at once.)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, the profile `analyst`, and the AI tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `Profile` `analyst`, and the `Context`, `Memory` and `Tools` rows with their `off` or `on` value

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned and the AI tab active
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)`, the other four read `(default)`, `Profile` reads `programmer` (the default profile) and `Context`, `Memory` and `Tools` read their defaults

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

### Requirement: Picker viewport
The picker body MUST be a title plus a viewport of exactly 10 rows, inside the fixed popup (62 columns wide; its height is the fixed popup height, see Popup height is fixed). The picker is one of the views the fixed height is the maximum over; `VIEW` = 10 plus the title MUST fit the body. When the rows do not fit, the list MUST scroll so that the cursor row is always visible (cursor above the window sets the window start to the cursor; cursor below it sets the window so the cursor is the last row), and the popup height MUST NOT change while scrolling. Disabled rows scroll with the rest, and `j`/`k` wrap with the window following.
(Previously: `AI_BODY + 4` rows high, 62x19 in the main spec; the task list was the tallest view.)

#### Scenario: Long list scrolls [auto, TestBackend + insta]
- GIVEN level 2 for ollama with 18 rows (8 detected, 10 not downloaded) and the cursor moved to the last row, a disabled one
- WHEN rendered
- THEN the cursor row is visible and muted, the title shows `18/18`, the popup height equals its height with the cursor on the first row, and nothing is drawn outside the popup

#### Scenario: Wrap moves the window [auto]
- GIVEN level 2 with more rows than the viewport and the cursor on the first row
- WHEN `k` is pressed, then `j`
- THEN the cursor is on the last row with that row visible, then back on the first row with the window at the start

### Requirement: Notify row
`Notify` is a static row of the `Settings` tab (after `Mouse`, before `Sound`), found by `ItemKind::Notify`, never by position. It edits the AI draft, so it is shown only when the AI section is attached. It reads one of four modes from the AI draft: `auto`, `desktop`, `bell`, `off`. `h`, `l`, arrows, Space and Enter cycle the mode with wrap (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect. `t` tests the notification (see Notify, Sound and Updates keys). The footer is `j/k move · h/l change · t test · s save · Esc cancel`. Only a changed value is an edit (`ai.notify`), placed after the memory edits.
(Previously: an on/off row at the end of the AI section. Drift fix: four modes, on the Settings tab, with `t`.)

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section, Settings tab active
- WHEN navigating down from `Mouse`
- THEN the selection reaches `Notify`, then `Sound`, then `Updates`, then wraps to `Statusline position`

#### Scenario: Reads the mode [auto]
- GIVEN drafts with notify `auto`, `desktop`, `bell` and `off`
- WHEN the rows are built
- THEN the value is `auto`, `desktop`, `bell` and `off` respectively; without the AI section there is no row

#### Scenario: Cycle and edit [auto]
- GIVEN the selection on `Notify`
- WHEN a cycle key is pressed through all four modes
- THEN the draft returns to the starting mode after four presses, the command is `None` each time and the edit list holds `ai.notify` only while the draft differs from the open value

#### Scenario: Snapshots [auto]
- GIVEN the Settings tab snapshots
- WHEN regenerated
- THEN `Notify` and `Sound` appear on the Settings tab and not on the AI tab
