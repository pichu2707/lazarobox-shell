# Config Menu Specification

## Purpose

A general in-app menu opened with `Ctrl+Space m`. It has two tabs, "Settings" (rows that map to `config.toml` keys, with static values) and "AI" (a fixed list of tasks, each with an `engine › model` assignment chosen from a picker that shows what is detected and downloaded, see ai-task-models and ai-engine-detection). Changes preview live; `s` saves (see config-persistence), Esc goes back or reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.

Terms: "task list" is the main menu view (Settings rows and AI rows). "Picker" is the view that replaces it inside the same popup: level 1 lists engines, level 2 lists the models of the chosen engine, and the "custom field" is a text entry reached from a CLI engine's `custom…` row. A "disabled row" is drawn muted, the cursor CAN land on it (so long lists scroll it into view), and Enter on it does nothing. The cursor skips no row of the task list. "Menu tab" is a tab of the config popup (Settings, AI). It is not a pane tab: where the Live preview and revert requirement says "the tab bar MUST be shown while the menu is open", it means the pane tab bar, which is unchanged. "Menu tab bar" is the one-line strip of menu tabs inside the popup. "Body" is the popup content between the borders, excluding the blank row and footer.

## Requirements

### Requirement: Open and close
The menu MUST open only from TERMINAL through the prefix: `Ctrl+Space` then `m`, always on the `Settings` tab, on its first row (`Statusline position`), with the per-tab cursor memory cleared and the picker closed. Opening MUST NOT write to any PTY. The menu MUST close on `s` (after a successful save, or at once when nothing changed) and on Esc at the top level of the task list (after reverting). Esc while a picker is open MUST go back exactly one level (custom field to level 2, level 2 to level 1, level 1 to the task list) and MUST NOT close the menu or revert the draft. This Esc rule takes precedence wherever Esc is described. While open the mode is MENU (see modal-input). Opening captures the current values of the menu-owned settings as the "original" values and as the initial draft.
(Previously: scenarios named the `AI` tab; text unchanged)

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
- GIVEN the menu closed while the Models tab was active with `code` selected, and the Settings cursor had been moved to `Mouse`
- WHEN the menu is reopened
- THEN the active tab is `Settings`, the selected row is `Statusline position`, after Tab then Shift+Tab the Settings row is again the first, not `Mouse`, and after `2` the Models row is `Set all tasks`, not `code`

#### Scenario: Esc in the picker goes back one level [auto]
- GIVEN the picker is open at level 2 for `code` after changing the Statusline position in the draft
- WHEN Esc is pressed, then Esc again, then Esc a third time
- THEN level 1 shows, then the task list shows on the Models tab with `code` selected and the draft unchanged, then the menu closes and the Statusline position is reverted

#### Scenario: Reopen after picker is on the task list [auto]
- GIVEN the menu closed with Esc while no picker was open, after a picker was used earlier
- WHEN the menu is reopened
- THEN the task list shows on the Settings tab, the picker is closed and the draft equals the current values

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). Each row MUST belong to exactly one tab, as in the Tab map: `Settings` (`Statusline position` `statusline.position`: top, bottom; `Tab bar position` `tabbar.position`: top, bottom; `Mouse` `mouse`: on, off, written as the booleans `true` and `false`; `Notify`: auto, desktop, bell, off; `Sound`: off, on; `llama-server idle` `ai.servers.llamacpp.idle_minutes`: 5 min, 15 min, 30 min, never, written as a TOML integer, never = 0; `Updates`: the running version, or the newer one when known; `Feedback`: `bug report or idea`, no key path), `Models` (`Set all tasks` aggregate over the five tasks, no single key path; `general` `ai`; `code` `ai.tasks.code`; `chat` `ai.tasks.chat`; `review` `ai.tasks.review`; `summarize` `ai.tasks.summarize`; `orchestrate` its task table; `Hugging Face`, no key path), `Tools` (`Tools` `ai.tools.enabled`: off, on; `Search`, `Write`, `Exec`, `Git`, `Git write`: off, on, each with its own `ai.tools.<key>`; `Approval` `ai.tools.approval`: ask, auto, run; `Orchestrator` `ai.orchestrator.enabled`: off, on), `Connectors` (no rows) and `Chat` (`Profile` `ai.profile`: analyst, programmer; `Context` `ai.context`: off, on; `Memory` `ai.memory.enabled`: off, on). `Notify`, `Sound` and `llama-server idle` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached; then `Settings` holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and no other tab exists. Moving a row between tabs MUST NOT change its key path, its values or the config edit it produces.
(Previously: two tabs, `Settings` with seven rows and a 19-row `AI` tab; the Settings list omitted `Feedback` and the no-AI list omitted `Feedback`)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every tab has a non-empty label, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context`, `Memory`, `Tools`, `Approval` and the five family rows have their key paths and values above, every tab lists only rows that exist, every row appears on exactly one tab, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound`, `llama-server idle` (`15 min`), `Updates` and `Feedback`, each with its current value, and no Models, Tools or Chat row is drawn

#### Scenario: Every row is on its tab [auto, TestBackend + insta]
- GIVEN the AI section attached
- WHEN each of Models, Tools and Chat is rendered
- THEN Models reads `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`; Tools reads `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator`; Chat reads `Profile`, `Context`, `Memory`; and no `Notify`, `Sound`, `Updates` or `Feedback` row exists on them

#### Scenario: No AI keeps one tab [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and there is no other tab

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the ACTIVE tab only, in display order, and MUST wrap around at the ends of that tab (from its last row to its first and back); they MUST NOT cross to another tab, because Tab, Shift+Tab and the jump keys are the only way across tabs. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press. The order of each tab is the one in the Tab map: `Settings` ends with `Updates`, `Feedback`; `Models` ends with `orchestrate`, `Hugging Face`; `Tools` ends with `Approval`, `Orchestrator`; `Chat` is `Profile`, `Context`, `Memory`.
(Previously: two tabs; the `AI` tab order ran from `Set all tasks` to `Hugging Face` in one list; the Settings order omitted `llama-server idle` and `Feedback`)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap within Settings [auto]
- GIVEN the `Feedback` row selected on Settings
- WHEN `j` (or Down) is pressed
- THEN `Statusline position` is selected and the active tab is still `Settings`; and from the first row `k` (or Up) selects `Feedback`

#### Scenario: Wrap within Models, Tools and Chat [auto]
- GIVEN each of Models (`Hugging Face` selected), Tools (`Orchestrator` selected) and Chat (`Memory` selected) in turn
- WHEN `j` is pressed, then `k`
- THEN the first row of that tab (`Set all tasks`, `Tools`, `Profile`) is selected, then the last again, and the active tab does not change

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN `Mouse` is selected, `j` again selects `Notify`; from `Notify` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN the Models tab active with `Set all tasks` selected
- WHEN `j` is pressed eight times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face` in that order and the eighth press wraps to `Set all tasks`

#### Scenario: Tools tab in order [auto]
- GIVEN the Tools tab active with `Tools` selected
- WHEN `j` is pressed eight times
- THEN the selection visits `Search`, `Write`, `Exec`, `Git`, `Git write`, `Approval`, `Orchestrator` in that order and the eighth press wraps to `Tools`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Value cycling
On a Settings row, `h`, `l`, Left, Right, Space and Enter MUST change the draft value by cycling through the allowed values with wrap (with two values, any of these keys toggles). `l`/Right/Space/Enter advance to the next value and `h`/Left to the previous. Repeat events MUST be handled like Press. On an AI task row (including `Set all tasks`) `h`, `l`, Left, Right and Space MUST have no effect, and Enter opens the picker. These keys MUST have no effect when there is no selectable row.

#### Scenario: Cycle forward [auto]
- GIVEN Statusline position is `bottom` and selected
- WHEN `l` is pressed
- THEN the draft value is `top`, and pressing `l` again gives `bottom`

#### Scenario: Cycle backward and Space [auto]
- GIVEN Tab bar position is `top` and selected
- WHEN `h` is pressed, then Space
- THEN the value is `bottom`, then `top`

#### Scenario: Arrow keys [auto]
- GIVEN a setting row selected
- WHEN Left or Right is pressed
- THEN the value cycles like `h` or `l`

#### Scenario: Enter cycles a Settings row [auto]
- GIVEN Statusline position is `bottom` and selected
- WHEN Enter is pressed
- THEN the draft value is `top`, the menu stays open, no save is requested, and Enter again gives `bottom`

#### Scenario: Cycling keys do nothing on task rows [auto]
- GIVEN the `code` row selected and unassigned
- WHEN `h`, `l`, Left, Right and Space are pressed
- THEN the draft is unchanged and no picker opens

### Requirement: Live preview and revert
Every change of a draft value MUST be applied immediately to the running layout through the normal relayout. `ResizePty` MUST be emitted only for panes whose size actually changed, and no preview step may write to any PTY. The tab bar MUST be shown while the menu is open (see tabs). Cycling the `Mouse` row MUST also apply immediately: it enables or disables host mouse capture with the sequences of mouse-input (one write per actual change, no write when the effective value is unchanged), without touching any PTY and without relayout. Esc MUST restore the original values captured at open (including the capture state if `Mouse` was cycled), relayout again (with `ResizePty` only for panes whose size changed back), and close the menu; the file MUST NOT be read or written. External edits to `config.toml` made while the menu is open MUST NOT be reloaded: Esc restores the values the app had at open.

#### Scenario: Statusline moves live [auto]
- GIVEN a 80x25 terminal, statusline `bottom`, tab bar `top`, two panes
- WHEN the statusline value is cycled to `top`
- THEN the statusline is drawn at row 0 or on the top edge per the bar placement rules, the body keeps the same height, and ResizePty is emitted only for panes whose rect size changed (none when sizes are equal)

#### Scenario: Preview emits ResizePty only for changed panes [auto]
- GIVEN two tabs of panes and a preview that changes the body width or height of only some panes
- WHEN the value changes
- THEN ResizePty is emitted exactly for the panes whose size changed

#### Scenario: Mouse cycles and toggles capture live [auto]
- GIVEN the menu open with `Mouse` at `on` and selected
- WHEN `l` is pressed, then `l` again
- THEN the draft value is `off` and the capture-disable sequence is written once, then `on` and the capture-enable sequence is written once, and no PTY write or ResizePty happens

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with statusline `bottom` and tab bar `top`, both changed in the draft
- WHEN Esc is pressed
- THEN statusline and tab bar are back at `bottom` and `top`, the layout is recomputed, and mode is TERMINAL

#### Scenario: Esc reverts mouse capture [auto]
- GIVEN the menu opened with `mouse` on, `Mouse` cycled to `off` (capture disabled)
- WHEN Esc is pressed
- THEN capture is enabled again, the file is not written and mode is TERMINAL

#### Scenario: Esc reverts the one-tab bar [auto]
- GIVEN one tab (no tab bar) and the menu open (bar forced visible)
- WHEN Esc is pressed
- THEN the tab bar is hidden, the body returns to the pre-menu size, and ResizePty is emitted for panes whose size changed back

#### Scenario: External edit is not reloaded [auto]
- GIVEN the menu open and `config.toml` edited externally to a different position
- WHEN Esc is pressed
- THEN the positions are those the app had when the menu opened

### Requirement: Input is swallowed
In MENU every key not listed above MUST be swallowed with no effect. The listed keys are `j`, `k`, Up, Down, `h`, `l`, Left, Right, Space, Enter, `s`, `r`, `t`, `y` (on the Updates command and the warning), `d` (on a Hugging Face row with a saved token), Tab, Shift+Tab (`BackTab` with or without SHIFT, and Tab with SHIFT), the jump keys `1` to `5` (unmodified, at the top level only) and Esc, each with the behavior described in this spec, and inside the custom field typed characters, Backspace and Ctrl+U; in the Hugging Face token field and the Feedback form, Ctrl+O opens their link (see those requirements). Ctrl+Tab and Alt+Tab are unlisted and swallowed; so is any digit with Ctrl or Alt (modified keys stay swallowed as today). At the top level `0` and `6` to `9` are not swallowed silently: they show the footer notice `Tabs are 1-5` (see Jump keys), and Tab or Shift+Tab with a single visible tab shows `No other tabs`. Tab, Shift+Tab and the jump keys are listed at the top level only; in the picker, the custom field and the warning they follow Sub-levels keep the bar inert. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL), also inside the custom field. Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter cycles or selects like Enter); uppercase letters such as `J`, `S` and `R` are unlisted and swallowed outside the custom field. `r` is listed only for the refresh behavior (see Refresh key) and `s` only for saving (see config-persistence).
(Previously: no digit was listed; `d` was not mentioned)

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x` or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

#### Scenario: Tab is listed at the top level [auto]
- GIVEN MENU at the top level with the AI section attached
- WHEN Tab is pressed
- THEN the active tab changes, no WritePty is emitted and mode stays MENU

#### Scenario: Out-of-range digits show a notice, modified digits are swallowed [auto]
- GIVEN MENU at the top level on Settings
- WHEN `0`, `6` and `9` are pressed, then Ctrl+`2` and Alt+`3`
- THEN the first three each show the footer notice `Tabs are 1-5`; Ctrl+`2` and Alt+`3` change nothing and show no notice; in all cases the active tab and the draft are unchanged, no WritePty is emitted and mode stays MENU

#### Scenario: Paste ignored [auto]
- GIVEN MENU with the custom field not focused
- WHEN a paste event arrives
- THEN no WritePty is emitted and the menu is unchanged

#### Scenario: Uppercase S is swallowed [auto]
- GIVEN the task list with a changed draft
- WHEN `S` is pressed
- THEN no save is requested and the menu is unchanged

### Requirement: Footer
The menu footer MUST show, while there is no error, the hint of the current view, exactly:
- Settings row: `j/k move · h/l change · s save · Esc cancel`
- AI task row (including `Set all tasks`): `j/k move · Enter pick · r refresh · s save · Esc cancel`
- Picker level 1: `j/k move · Enter choose · r refresh · Esc back · s save`
- Picker level 2: `j/k move · Enter choose · r refresh · Esc back · s save`
- Picker, cursor on a disabled row: `<reason> · not selectable`, where `<reason>` is the row's mark text: `download required`, `no models detected`, `engine not detected` or `checking…`
- Picker, cursor on a CLI model that is not in the account's list: `not in your account's list · kept as typed, the list may be stale` (a warning; the row stays selectable)
- Custom field: `type a model · Enter ok · Ctrl+U clear · Esc back`
- Any other AI or Settings view while the AI draft differs from the saved config: `unsaved changes · s to save · Esc discards` (`Esc back` in the picker), because Enter never saves and Esc at the top level discards the draft

When a detection cue is active (see Detection cues) the hint of an AI view (task row, picker level 1 or 2) is replaced by `<cue> · s save · Esc cancel` on the task list and `<cue> · s save · Esc back` in the picker, so the footer fits the 62-column popup. Precedence, highest first: error, disabled-row text, not-in-list warning, cue, unsaved-changes hint, hint. A pending error MUST be cleared by the next key press (Press, not Repeat or Release), which then acts normally; an unlisted key clears it and has no other effect.

#### Scenario: Footer hint [auto, TestBackend + insta]
- GIVEN MENU just opened and detection finished
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel`

#### Scenario: Footer hint on AI row [auto, TestBackend + insta]
- GIVEN the `code` row selected and detection finished
- WHEN rendered
- THEN the footer reads `j/k move · Enter pick · r refresh · s save · Esc cancel`

#### Scenario: Footer hints in the picker [auto, TestBackend + insta]
- GIVEN the picker at level 1, then at level 2, then in the custom field, detection finished
- WHEN each is rendered
- THEN the footers read `j/k move · Enter choose · r refresh · Esc back · s save` (twice) and `type a model · Enter ok · Ctrl+U clear · Esc back`

#### Scenario: Footer on a disabled row [auto, TestBackend + insta]
- GIVEN level 2 for ollama with the cursor on a curated not-downloaded model, detection finished
- WHEN rendered
- THEN the footer reads `download required · not selectable`

#### Scenario: Unsaved changes hint [auto]
- GIVEN the code task is assigned in the saved config and the user picks another model in the picker
- WHEN the footer is read after returning to the task list
- THEN it reads `unsaved changes · s to save · Esc discards`, and it is the normal hint again once the draft matches the saved config

#### Scenario: Error replaces hint [auto]
- GIVEN a save failure message
- WHEN rendered
- THEN the footer shows the message instead of the hint, and the next key press restores the hint

### Requirement: Tiny terminals
The menu MUST never panic at any terminal size down to 0x0, on every visible tab and in every picker view. The popup width is 62 with the AI section (51 without) and its height is the fixed popup height (see Popup height is fixed: `MENU_BODY` 12 plus `CHROME` 4 rows of chrome: two borders, the blank row and the footer, so 16; 17 once the ai-cost `Usage (alpha)` row lands). The full popup needs at least 24 columns by 15 rows (16 with the Usage row), the smallest full popup (the blank row is the first thing dropped); adding a row to the tallest view grows these numbers. Below that the menu MUST draw only a one-line fallback message instead of the full popup; the key handling MUST be unchanged, so Esc still reverts and closes (or goes back one level when a picker is open), `s` still saves, and Tab, Shift+Tab and the jump keys still switch tabs. Content that does not fit MUST be clipped, including the menu tab bar, which narrows by the steps of Menu tab bar.
(Previously: scenarios named the `AI` tab; the bar degraded only as far as the hint)

#### Scenario: No panic at any size [auto]
- GIVEN the menu open on each visible tab, then in each picker view and the warning
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

#### Scenario: Tabs still switch under the fallback [auto]
- GIVEN a terminal smaller than the menu minimum and the AI section attached
- WHEN Tab, then `3` are pressed
- THEN the active tab is `Models`, then `Tools`

#### Scenario: Picker too small keeps keys [auto]
- GIVEN a terminal one column narrower than 24, and one 24 wide but 14 rows high, and the picker open at level 1
- WHEN rendered, then Esc is pressed
- THEN the one-line fallback is shown, then the task list view is the current view

#### Scenario: Smallest full popup [auto, TestBackend + insta]
- GIVEN a terminal of exactly the minimum size (24 columns by 16 rows)
- WHEN the Settings, Models, Tools and Chat tabs and the picker are rendered
- THEN the full popup is drawn (not the fallback), clipped, without panic, with the bar reduced to the active tab

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup

### Requirement: Events while open
While the menu is open, PTY output MUST keep being parsed and drawn, cwd polls and OSC 7 MUST keep updating, and tabs and panes in the background MUST keep running. The menu MUST stay open (and the picker, if open, MUST stay in the same view) when a pane exits, a spawn fails, or the host terminal resizes; geometry MUST be recomputed with the forced tab bar and the previewed positions. If the exit of a pane would quit the app (last pane of the last tab), Quit MUST still be emitted. A focus or active-tab change caused by a pane exit MUST NOT close the menu. A spawn failure notice MUST NOT replace the menu footer.

#### Scenario: Output keeps flowing [auto]
- GIVEN the menu open
- WHEN a pane emits output
- THEN the pane's screen is updated and the menu stays open

#### Scenario: Pane exit keeps the menu [auto]
- GIVEN the menu open and two panes
- WHEN the other pane's shell exits
- THEN mode is still MENU, the layout is recomputed, and the draft is unchanged

#### Scenario: Pane exit keeps the picker [auto]
- GIVEN the picker open at level 2 and two panes
- WHEN the other pane's shell exits
- THEN mode is still MENU, the picker is still at level 2 with the same entry selected

#### Scenario: Spawn failure keeps the menu [auto]
- GIVEN the menu open
- WHEN a spawn failure is reported
- THEN mode is still MENU and the footer is unchanged

#### Scenario: Host resize [auto]
- GIVEN the menu open with previewed positions
- WHEN a host Resize event arrives
- THEN rects are recomputed with the previewed positions and the forced tab bar, ResizePty is emitted only for panes whose size changed, and mode is still MENU

#### Scenario: Last pane exits [auto]
- GIVEN the menu open with one tab and one pane
- WHEN that shell exits
- THEN a Quit effect is emitted

#### Scenario: Menu in Kitty [manual, Kitty]
- GIVEN the app in Kitty with nvim open and one tab
- WHEN `Ctrl+Space m` is pressed, the positions are changed with `l`, then Esc, then the menu is reopened and the position is changed and `s` is pressed
- THEN the bars move live, Esc restores them, `s` closes the menu, nvim receives no keys, and a restart shows the saved positions

### Requirement: Detection results update the open menu
Detection results MUST update the open menu in place: the task-row marks (`(not detected)`, `· download required`), the aggregate and the picker lists are rebuilt from the new statuses, the selected row MUST NOT change, and the draft values MUST NOT change. A picker that is open MUST stay open at the same level; its cursor stays on the same entry (identified by engine id or model name, not by position), even when that entry became disabled, and when the entry no longer exists the cursor is clamped to the list. Results arriving while the menu is closed MUST be applied to the state used the next time it opens. Opening the menu MUST NOT write to any PTY, and a detection result MUST NOT either.

#### Scenario: Result arrives on the task list [auto]
- GIVEN the menu open with the `code` row selected, code assigned `ollama` / `llama3:8b`, and no result yet
- WHEN a result with ollama `available · 2 models` listing only `qwen2.5:7b` arrives
- THEN the code row reads `ollama › llama3:8b · download required`, `code` stays selected, the draft is unchanged, and no WritePty is emitted

#### Scenario: Result arrives in the picker [auto]
- GIVEN the picker open at level 2 for ollama with the cursor on `qwen2.5:7b`, which a refresh then reports as no longer listed
- WHEN the result arrives
- THEN the picker is still at level 2, `qwen2.5:7b` is muted with `download required` (moved below the detected models), the cursor is still on `qwen2.5:7b`, and the footer reads `download required · not selectable`

#### Scenario: Cursor entry removed [auto]
- GIVEN the picker open at level 1 with the cursor on `claude`, which a refresh then reports as `not installed` and unassigned
- WHEN the result arrives
- THEN the picker is still at level 1 and the cursor is clamped to a valid row

#### Scenario: Engine disappears while open [auto]
- GIVEN code is assigned `ollama` / `llama3:8b` and a refresh returns ollama `not running`
- WHEN the result arrives
- THEN the assignment stays, and the code row reads `ollama (not detected) › llama3:8b`

### Requirement: Refresh key
`r` (Press only, not Repeat) MUST request a new detection run (see ai-engine-detection) and have no other effect when the selection is on an AI task row (including `Set all tasks`) or the picker is at level 1 or level 2 (the user may run `ollama pull` while looking at models). On a Settings row and in the custom field (where it is a typed letter) `r` has no refresh effect. Uppercase `R` is unlisted and swallowed.

#### Scenario: Refresh on a task row [auto]
- GIVEN the `code` row selected
- WHEN `r` is pressed
- THEN exactly one detection run is requested, the selection and draft are unchanged, and no WritePty is emitted

#### Scenario: Refresh at picker level 1 [auto]
- GIVEN the picker at level 1
- WHEN `r` is pressed
- THEN exactly one detection run is requested and the picker stays at level 1

#### Scenario: Refresh at picker level 2 [auto]
- GIVEN the picker at level 2 for ollama
- WHEN `r` is pressed
- THEN exactly one detection run is requested and the picker stays at level 2 with the cursor on the same entry

#### Scenario: Refresh elsewhere or repeated [auto]
- GIVEN the Statusline position row selected, or the custom field focused, or the `code` row with a Repeat event for `r`
- WHEN `r` arrives
- THEN no detection run is requested (in the custom field `r` is typed into the buffer)

### Requirement: AI draft, revert and save
The AI draft (the five task assignments) is captured at open with the other menu-owned values. Esc at the top level MUST restore the AI draft to the open values and read or write no file. `s` MUST save through config-persistence only the keys whose draft differs; when the whole draft equals the open values, the menu closes without touching the file. AI choices have no live effect on layout and emit no `ResizePty`. Choosing in the picker changes only the draft.

#### Scenario: Esc reverts AI [auto]
- GIVEN the menu opened with general `ollama` / `llama3:8b`, and the draft changed to `claude` / `opus`
- WHEN Esc is pressed on the task list and the menu is reopened
- THEN the draft general is `ollama` / `llama3:8b`

#### Scenario: Save closes with AI values [auto]
- GIVEN code was assigned `claude` / `opus` through the picker
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes and the current code assignment is `claude` / `opus`

#### Scenario: Unchanged AI draft [auto]
- GIVEN no change was made
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written

#### Scenario: Choosing changes only the draft [auto]
- GIVEN the picker used to assign code `claude` / `opus`
- WHEN the selection is made
- THEN the file is not read or written and no ResizePty or WritePty is emitted

#### Scenario: AI menu in Kitty [manual, Kitty]
- GIVEN the app in Kitty on a machine with Ollama running and `claude` installed
- WHEN the menu is opened, `code` is assigned `claude` / `opus` through the picker, `r` is pressed, `s` is pressed, and the app is restarted
- THEN the file gains `[ai.tasks.code]` with `engine = "claude"` and `model = "opus"`, the UI never freezes, and the `code` row reads `claude › opus` after restart

#### Scenario: Real Ollama models [manual, Kitty]
- GIVEN Ollama running with 8 models downloaded and other curated models not downloaded
- WHEN the picker is opened at level 2 for ollama and `j` is pressed through the whole list
- THEN the 8 downloaded models show `✓` and can be chosen, the curated ones show `download required`, are muted, the cursor reaches them (the list scrolls), the footer reads `download required · not selectable`, and Enter on them does nothing and never starts a download

### Requirement: Models tab
The Models tab MUST show, as its rows (with no section title row while the tab bar is shown), exactly seven task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, and then the static row `Hugging Face` (see Hugging Face account row). Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks it covers differ, and the marks `· download required`, `(not detected)` and, on `orchestrate` only, `· not recommended` as defined there. The `Hugging Face` row shows its account state. The number of rows MUST never change because of a detection result. The popup MUST show every row of the active tab without clipping on a normal terminal, MUST keep the same size across all tabs and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST follow the popup. The detection cues, the `r` refresh key and the task hint apply on the task rows only; on the `Hugging Face` row, on Tools and on Chat rows `r` requests no detection run.
(Previously: "AI task list", one tab of 19 rows that also held Profile, Context, Memory, Tools, the family switches, Approval and Orchestrator)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, no Hugging Face token, and the Models tab active
- WHEN the menu is rendered on a normal terminal
- THEN the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `orchestrate` `(default)`, `Hugging Face` `not connected`, and no `Profile`, `Context`, `Memory`, `Tools` or `Orchestrator` row

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

### Requirement: Task picker
Enter on an AI task row MUST replace the task list with the picker, inside the same popup, at level 1 with the cursor on the task's current engine when listed and otherwise on the first entry. Level 1 lists the engine entries of ai-task-models: the first entry is `(default · use general)` for non-general tasks and `none` for `general` and `Set all tasks`; each engine row shows its id and its status text; assigned engines that are not detected are listed as `(not detected)` and remain selectable. Enter on an engine row opens level 2 for that engine, with the cursor on the current model when it is selectable and otherwise on the first selectable row (on the first row when none is selectable). Level 2 lists the models of ai-task-models: detected models marked `✓` (selectable), then the assigned model that is no longer listed (`download required`, disabled, marked `●` as current), then the curated not-downloaded models (`download required`, disabled); for CLI engines the catalog, the assigned custom models, then `custom…`. Enter on a selectable model assigns the engine and model to the task (to all five for `Set all tasks`), closes the picker and returns to the task list with that task row selected. Enter on the first entry of level 1 clears the task (all five for `Set all tasks`, general only for `general`) and returns to the task list. `j`, `k`, Down and Up move the cursor over ALL rows, with wrap, disabled rows included, so long lists scroll them into view; on a disabled row Enter does nothing and the footer explains why (see Footer). `h`, `l`, Left, Right and Space have no effect in the picker. The picker title names the view: `Engine for <row label>` at level 1 and `Model for <row label> (<engine>)` at level 2, followed by ` n/total` only when the list is longer than the viewport.

#### Scenario: Open level 1 [auto, TestBackend + insta]
- GIVEN ollama (2 models) and claude are available, lmstudio is `not running`, and `code` is selected
- WHEN Enter is pressed
- THEN the title reads `Engine for code` and the rows are `(default · use general)`, `ollama` with `available · 2 models`, and `claude` with `available · 2.1.0`, with the cursor on the first

#### Scenario: Assign a CLI model [auto]
- GIVEN the picker at level 1 for `code`, claude available
- WHEN the cursor is moved to `claude`, Enter is pressed, the cursor is moved to `opus` and Enter is pressed
- THEN code is `claude` / `opus` in the draft, the task list shows with `code` selected, and the row reads `claude › opus`

#### Scenario: Server model marks [auto, TestBackend + insta]
- GIVEN ollama available with `llama3:8b` and the curated list containing `mistral:7b`, and level 2 open for `code`
- WHEN rendered
- THEN `llama3:8b` shows `✓` and `mistral:7b` is muted and shows `download required`

#### Scenario: Disabled rows are reachable but inert [auto]
- GIVEN level 2 for ollama with `llama3:8b ✓` first and `mistral:7b download required` after it
- WHEN `j` is pressed, then Enter
- THEN the cursor is on `mistral:7b`, the footer reads `download required · not selectable`, and Enter changes nothing (picker still at level 2, draft unchanged)

#### Scenario: Enter with no selectable row [auto]
- GIVEN level 2 for lmstudio showing only the disabled row `no models detected`
- WHEN Enter, `j` and `l` are pressed
- THEN nothing changes and the draft is unchanged, the footer reads `no models detected · not selectable`; Esc returns to level 1

#### Scenario: Unassign entry [auto]
- GIVEN code is `claude` / `opus` and the picker at level 1 for code
- WHEN Enter is pressed on `(default · use general)`
- THEN code is unassigned, the task list shows and the row reads `(default)`

#### Scenario: General and Set all use none [auto]
- GIVEN the picker at level 1 for `general`, then for `Set all tasks`
- WHEN each is opened
- THEN the first entry reads `none` and Enter on it clears general, or clears all five tasks, respectively

#### Scenario: Set all assigns five [auto]
- GIVEN the picker for `Set all tasks`
- WHEN `claude` then `sonnet` is chosen
- THEN all five rows read `claude › sonnet` and `Set all tasks` reads `claude › sonnet`

#### Scenario: Saved model no longer downloaded stays [auto]
- GIVEN code is `ollama` / `old:7b` and ollama no longer lists `old:7b`
- WHEN level 2 is opened for code and Esc is pressed
- THEN `old:7b` is listed muted with `download required` and `●` (current), after the detected models, Enter on it does nothing, and the code assignment is unchanged

#### Scenario: Undetected assigned engine listed [auto]
- GIVEN code is assigned `codex` and codex is `not installed`
- WHEN level 1 is opened for code
- THEN `codex` is listed with `(not detected)`, it is selectable (Enter reopens it), and the draft is unchanged until another choice is made

### Requirement: Custom model entry
The `custom…` row exists only at level 2 of the CLI engines (`claude`, `codex`, `gemini`, `opencode`). Enter on it MUST focus the custom field, a text entry with an empty buffer. While it is focused: printable characters, including `s`, `r`, `j`, `k`, `h` and `l`, are typed into the buffer, but Space is NOT (model names have no spaces; whitespace, control and format characters are dropped); Backspace deletes the last character; `s` MUST NOT save; Enter with a valid name (see ai-task-models) assigns the engine and that model to the task, closes the picker and returns to the task list; Enter with an empty or blank buffer has no effect; Ctrl+U clears the buffer; Esc discards the buffer and returns to level 2 with the cursor on `custom…`, without changing the draft. A paste event while the custom field is focused MUST insert its first line into the buffer (text after the first line break is dropped), with whitespace, control and format characters removed and the buffer capped at 128 characters, and MUST NOT reach any PTY. Outside the custom field a paste is ignored (see Input is swallowed).

#### Scenario: Type and accept [auto]
- GIVEN the custom field focused for `claude` on `code`
- WHEN `m`, `s`, `-`, `1` are typed and Enter is pressed
- THEN no save is requested, the draft code is `claude` / `ms-1`, and the task list shows `claude › ms-1`

#### Scenario: `s` is a letter in the field [auto]
- GIVEN the custom field focused and a changed draft
- WHEN `s` is pressed
- THEN `s` is appended to the buffer and no save is requested

#### Scenario: Paste goes to the field [auto]
- GIVEN the custom field focused with buffer `ab`
- WHEN a paste of `cd\nef` arrives
- THEN the buffer is `abcd` and no WritePty is emitted

#### Scenario: Paste elsewhere ignored [auto]
- GIVEN the picker at level 2 (field not focused)
- WHEN a paste event arrives
- THEN the picker and draft are unchanged and no WritePty is emitted

#### Scenario: Ctrl+U clears [auto]
- GIVEN the custom field focused with buffer `abc`
- WHEN Ctrl+U is pressed
- THEN the buffer is empty and the field stays focused

#### Scenario: Blank entry refused [auto]
- GIVEN the custom field focused with buffer `   `
- WHEN Enter is pressed
- THEN nothing is assigned and the field stays focused

#### Scenario: Esc leaves the field [auto]
- GIVEN the custom field focused with buffer `abc`
- WHEN Esc is pressed
- THEN level 2 shows, the draft is unchanged, and reopening `custom…` shows an empty buffer

#### Scenario: Servers have no custom entry [auto]
- GIVEN level 2 for any of ollama, lmstudio, llamacpp or vllm
- WHEN the rows are listed
- THEN there is no `custom…` row

### Requirement: Picker viewport
The picker body MUST be a title plus a viewport of exactly 10 rows, inside the fixed popup (62 columns wide; its height is the fixed popup height, see Popup height is fixed). The picker is one of the views the fixed height is the maximum over; `VIEW` = 10 plus the title MUST fit the body. When the rows do not fit, the list MUST scroll so that the cursor row is always visible (cursor above the window sets the window start to the cursor; cursor below it sets the window so the cursor is the last row), and the popup height MUST NOT change while scrolling. Disabled rows scroll with the rest, and `j`/`k` wrap with the window following.

#### Scenario: Long list scrolls [auto, TestBackend + insta]
- GIVEN level 2 for ollama with 18 rows (8 detected, 10 not downloaded) and the cursor moved to the last row, a disabled one
- WHEN rendered
- THEN the cursor row is visible and muted, the title shows `18/18`, the popup height equals its height with the cursor on the first row, and nothing is drawn outside the popup

#### Scenario: Wrap moves the window [auto]
- GIVEN level 2 with more rows than the viewport and the cursor on the first row
- WHEN `k` is pressed, then `j`
- THEN the cursor is on the last row with that row visible, then back on the first row with the window at the start

### Requirement: Detection cues
While no detection result has ever arrived, the footer of the AI views MUST show the cue `checking engines…`. While a later detection run is in flight, it MUST show `refreshing…` and the last known engines, statuses and marks MUST stay visible. The cue replaces the hint as defined in Footer (`<cue> · s save · Esc cancel` on the task list, `<cue> · s save · Esc back` in the picker). With no run in flight, no cue shows. Before the first result, server model rows are disabled and marked `checking…`. The menu MUST NOT close silently at any point of detection: pressing `s` before the first result saves the draft as in config-persistence.

#### Scenario: Before the first result [auto, TestBackend + insta]
- GIVEN the menu opened and no detection result has arrived, the `code` row selected
- WHEN rendered
- THEN the footer reads `checking engines… · s save · Esc cancel`

#### Scenario: Refreshing keeps last known engines [auto]
- GIVEN a first result with ollama `available · 2 models` and the picker at level 1, then `r` is pressed
- WHEN rendered while the run is in flight
- THEN the footer reads `refreshing… · s save · Esc back` and ollama is still listed with `available · 2 models`

#### Scenario: Refreshing at level 2 [auto, TestBackend + insta]
- GIVEN the picker at level 2 for ollama and `r` pressed
- WHEN rendered while the run is in flight
- THEN the footer reads `refreshing… · s save · Esc back` and the models stay listed

#### Scenario: Cue clears [auto]
- GIVEN the `refreshing…` cue showing
- WHEN the run's result arrives
- THEN the footer shows only the hint

#### Scenario: Undetected saved engine survives [auto]
- GIVEN the saved file has code `codex`, codex is `not installed`
- WHEN the menu is opened, the draft is changed elsewhere, `s` is pressed, and the app is restarted
- THEN the file still has `engine = "codex"` for code and the row still reads `codex (not detected) › default`

### Requirement: Profile row
The `Profile` row is a static item in the AI section. On it, `h`, `l`, Left, Right, Space and Enter MUST cycle the draft value through `analyst` and `programmer` with wrap (any of these keys toggles; `l`/Right/Space/Enter advance and `h`/Left go back), and Repeat is handled like Press except on Enter, as for Settings rows. The footer on the `Profile` row MUST be the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and the `r` refresh key MUST NOT apply to it (`r` has no effect there, as on a Settings row). Changing the draft MUST have no live effect on layout, MUST NOT emit ResizePty, WritePty or a detection run, and MUST NOT change any open conversation. Esc at the top level MUST restore the profile to the value captured at open; `s` saves it through config-persistence only when it differs.

#### Scenario: Cycle [auto]
- GIVEN `Profile` selected with the draft `analyst`
- WHEN `l` is pressed, then `h`
- THEN the draft is `programmer`, then `analyst`

#### Scenario: Enter cycles and does not save [auto]
- GIVEN `Profile` selected with the draft `analyst`
- WHEN Enter is pressed
- THEN the draft is `programmer`, the menu stays open and no save is requested

#### Scenario: Repeat of Enter ignored [auto]
- GIVEN `Profile` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: Footer on the Profile row [auto, TestBackend + insta]
- GIVEN `Profile` selected and detection still in flight
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel`

#### Scenario: Refresh is not triggered [auto]
- GIVEN `Profile` selected
- WHEN `r` is pressed
- THEN no detection run is requested

#### Scenario: No side effects on change [auto]
- GIVEN `Profile` selected
- WHEN its value is cycled
- THEN no ResizePty, WritePty or file access happens and no chat state changes

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with `analyst` and the draft changed to `programmer`
- WHEN Esc is pressed on the task list
- THEN the current profile is `analyst` and no file is read or written

#### Scenario: Save [auto]
- GIVEN the menu opened with `analyst` and the draft changed to `programmer`
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes and the current profile is `programmer`

#### Scenario: Unchanged profile does not touch the file [auto]
- GIVEN the profile draft equals the open value and nothing else changed
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written

### Requirement: Memory row
The `Memory` row is a static item in the AI section showing `on` only when memory is active on the draft (`enabled` AND `acknowledged` both true), else `off`; a hand-edited `enabled = true` without `acknowledged = true` therefore shows `off`. On it, `h`, `l`, Left, Right, Space and Enter MUST toggle the draft, with Repeat handled like Press except on Enter. The footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Turning the draft to `on` when memory is not active and `acknowledged` is not true (including the hand-edited case above) MUST first show the privacy warning view and change the draft only if the user accepts. Changing the draft MUST have no live effect: no request to engram, no worker, no change to any open conversation until the menu is saved (and, for a conversation already open, until its next view opens). Esc at the top level restores the draft to the value captured at open, including the acknowledgement; `s` saves through config-persistence only what changed.

#### Scenario: Toggle when acknowledged [auto]
- GIVEN `Memory` selected, `acknowledged` true and the draft `off`
- WHEN `l` is pressed, then `h`
- THEN the draft is `on`, then `off`, and no warning is shown

#### Scenario: Footer and refresh [auto, TestBackend + insta]
- GIVEN `Memory` selected
- WHEN rendered, and `r` is pressed
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and no detection run is requested

#### Scenario: No side effects on change [auto]
- GIVEN `Memory` selected and a recording fake on the memory port
- WHEN the draft is toggled
- THEN no connection is made and no chat state changes

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with memory `off`, the draft turned `on` after accepting the warning
- WHEN Esc is pressed on the task list
- THEN memory is `off`, the acknowledgement draft is discarded and no file is read or written

#### Scenario: Save [auto]
- GIVEN the draft turned `on` after accepting the warning
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes, the file holds `enabled = true` and `acknowledged = true`, and memory is active

#### Scenario: Repeat of Enter ignored [auto]
- GIVEN `Memory` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

### Requirement: Memory privacy warning view
The warning view is shown over the menu popup (same size) with the text of the chat-memory opt-in requirement (notes leave the private 0600 conversation store; every local process and agent that uses engram can read them, and engram sync if it is on; lazarobox cannot restrict that; notes stay in engram until the user deletes them; secret redaction is best effort; once saved, this note is not shown again) and the prompt `Turn memory on? (y/n)`. The footer is `y accept (then s to save) · any other key declines`, because `y` only sets the drafts and `s` must save them. Only an unmodified lowercase `y` accepts: the draft becomes `on` and the acknowledgement draft true. Any other key (including Esc, Enter and `Y`) declines: the draft stays `off` and the view closes; Esc closes only the view, not the menu. When the popup does not fit and the one-line fallback is shown, the facts are not visible, so `y` MUST NOT accept: any key closes the warning and keeps memory off (informed consent). Repeat events MUST be ignored. Rendering MUST NOT panic at any terminal size.

#### Scenario: Accept [auto]
- GIVEN the warning shown
- WHEN `y` is pressed
- THEN the draft is `on`, the acknowledgement draft is true, the menu is shown again, and nothing is written to the file yet (`s` persists, Esc at the top level reverts, like every other row)

#### Scenario: Decline [auto]
- GIVEN the warning shown
- WHEN `n`, Esc, Enter or `Y` is pressed in separate runs
- THEN the draft stays `off`, nothing is acknowledged and the menu stays open

#### Scenario: Repeat ignored [auto]
- GIVEN the warning shown
- WHEN a Repeat event for `y` arrives
- THEN the view is unchanged

#### Scenario: Warning text and size [auto, TestBackend + insta]
- GIVEN the warning shown on a normal terminal
- WHEN rendered
- THEN the three facts and the prompt are visible; on a terminal too small for the popup the one-line fallback `Enlarge the terminal to read the privacy note` is shown instead, without panic

#### Scenario: No consent without the facts [auto]
- GIVEN the warning open and a terminal too small for the popup (only the one-line fallback is shown)
- WHEN `y` is pressed
- THEN nothing is accepted: like any other key it closes the warning, the draft stays `off` and nothing is acknowledged

#### Scenario: Real terminal [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN the user turns memory on for the first time
- THEN the warning is readable and legible without clipping

### Requirement: Mouse row
The `Mouse` row shows `on` when the draft `mouse` is true and `off` when false, starting from the effective value at open (the config value, default true). Saving with `s` persists the draft (see config-persistence) and leaves the capture in the state already previewed. The footer and the other rows are unchanged.

#### Scenario: Reflects the config [auto]
- GIVEN `mouse = false` in the config
- WHEN the menu opens
- THEN the `Mouse` row shows `off`

#### Scenario: Save keeps the previewed state [auto]
- GIVEN `Mouse` cycled to `off` in the menu
- WHEN `s` is pressed and the save succeeds
- THEN capture stays disabled, no extra mouse sequence is written, and the file holds `mouse = false`

#### Scenario: Failed save keeps the preview [auto]
- GIVEN `Mouse` cycled to `off` and a save that is refused
- WHEN the footer shows the refusal
- THEN the menu stays open with the draft `off`, and capture remains in the previewed state until Esc or a successful save

### Requirement: Notify row
`Notify` is a static row of the `Settings` tab (after `Mouse`, before `Sound`), found by `ItemKind::Notify`, never by position. It edits the AI draft, so it is shown only when the AI section is attached. It reads one of four modes from the AI draft: `auto`, `desktop`, `bell`, `off`. `h`, `l`, arrows, Space and Enter cycle the mode with wrap (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect. `t` tests the notification (see Notify, Sound and Updates keys). The footer is `j/k move · h/l change · t test · s save · Esc cancel`. Only a changed value is an edit (`ai.notify`), placed after the memory edits.
(Previously: the order scenario stopped at `Updates` and the snapshot scenario named the `AI` tab)

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section, Settings tab active
- WHEN navigating down from `Mouse`
- THEN the selection reaches `Notify`, then `Sound`, then `llama-server idle`, then `Updates`, then `Feedback`, then wraps to `Statusline position`

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
- THEN `Notify` and `Sound` appear on the Settings tab and not on Models, Tools or Chat

### Requirement: Menu tabs
The popup MUST group its rows into menu tabs. The tabs MUST come from a static table (label, the rows it shows, whether it needs the AI section, whether it is hidden while it has no rows); adding a tab MUST NOT require changing navigation or drawing code. The table order is `Settings`, `Models`, `Tools`, `Connectors`, `Chat`. Without the AI section only `Settings` exists. With it, `Models`, `Tools` and `Chat` exist and `Connectors` is visible only while it has at least one row; it has none today, so it MUST NOT be drawn, MUST NOT be reached by Tab or Shift+Tab, and MUST appear, in its table position between `Tools` and `Chat`, as soon as a row is declared for it (MCP, agent-tools 4a). `Tab` MUST activate the next visible tab; `Tab` with SHIFT and `BackTab` (with or without SHIFT) MUST activate the previous one. Wrap is across the visible tabs (Settings, Models, Tools, Chat, Settings while Connectors is hidden). Ctrl+Tab and Alt+Tab MUST stay swallowed. With one visible tab, Tab and Shift+Tab change nothing and show the footer notice `No other tabs` (cleared by the next key press, like the jump-key notice). A held key repeats like `j`/`k`: Repeat events switch tabs like Press. The cursor MUST be remembered per tab, keyed by the tab and not by its place among the visible ones: leaving a tab and coming back MUST restore the row that was selected there, and a tab becoming visible MUST NOT move the memory of another. Each tab with at least one row starts on its first row. Switching tabs, by Tab or by a jump key, MUST NOT change any draft value and MUST NOT emit `ResizePty`, `WritePty` or a detection run.
(Previously: tabs `Settings` and `AI`; "An `MCP` tab MUST NOT be listed until agent-tools 4a adds its table entry")

#### Scenario: Tab cycles forward with wrap [auto]
- GIVEN the AI section attached and the active tab `Settings`
- WHEN Tab is pressed four times
- THEN the active tab is `Models`, `Tools`, `Chat`, then `Settings` (Connectors is skipped)

#### Scenario: Shift+Tab, both encodings [auto]
- GIVEN the active tab `Settings`
- WHEN `BackTab` is pressed, in a separate run `BackTab` with SHIFT, and in another `Tab` with SHIFT
- THEN in all runs the active tab is `Chat`; and pressing the same key again gives `Tools`

#### Scenario: Cursor is remembered per tab [auto]
- GIVEN the `Mouse` row selected on `Settings`, then Tab to `Models`, `j` pressed twice (on `code`), then `3` to Tools and `j` once (on `Search`)
- WHEN `1`, `2` and `3` are pressed in turn
- THEN `Mouse` is selected on Settings, `code` on Models and `Search` on Tools

#### Scenario: Modified Tab and repeat [auto]
- GIVEN the active tab `Settings`
- WHEN Ctrl+Tab and Alt+Tab are pressed, then a Repeat event for Tab arrives, then a Repeat for `BackTab`
- THEN the first two change nothing, the Repeat of Tab moves to `Models`, and the Repeat of `BackTab` returns to `Settings`

#### Scenario: Switching tabs changes nothing else [auto]
- GIVEN a changed draft and the menu open
- WHEN Tab, Shift+Tab and `3` are pressed
- THEN the draft is unchanged and no ResizePty, WritePty, Refresh or Save is emitted

#### Scenario: Connectors is hidden without rows [auto]
- GIVEN the tab table and the AI section attached, and no row declared for Connectors
- WHEN the tabs are listed
- THEN they are exactly `Settings`, `Models`, `Tools`, `Chat`, in that order

#### Scenario: Connectors appears with its first row [auto]
- GIVEN a test table whose Connectors tab has one row, and the AI section attached
- WHEN the tabs are listed, and Tab is pressed from `Tools`, then again
- THEN they are `Settings`, `Models`, `Tools`, `Connectors`, `Chat`, and the active tab is `Connectors`, then `Chat`

#### Scenario: A tab appearing keeps the cursors [auto]
- GIVEN `Chat` with `Memory` selected and Connectors hidden
- WHEN a row is declared for Connectors and the menu is rebuilt with the same memory, then `5` is pressed
- THEN `Chat` is active with `Memory` still selected

#### Scenario: No AI, one tab [auto]
- GIVEN the menu without the AI section
- WHEN Tab and Shift+Tab are pressed
- THEN the tab does not change, the footer shows `No other tabs` and nothing else happens

### Requirement: Menu tab bar
The popup MUST draw a one-line menu tab bar as the first body row whenever there are two or more visible tabs. Each label is padded with one space on each side and labels are joined by `│`: BAR = sum of the label cells + 3 x tabs - 1. With the four visible tabs the bar ` Settings │ Models │ Tools │ Chat ` is 34 cells; with Connectors visible, ` Settings │ Models │ Tools │ Connectors │ Chat ` is 47; the `*` of `Settings*` adds one. The active tab MUST be drawn bold, `bg_base` on `primary_cyan` (like the main tab bar, and not like the selected row); idle tabs, the separator and the hint are `text_muted`. The hint `⇥ Tab · 1-5` (11 cells) is right-aligned and ends one column before the inner right edge. With W = inner width (popup width minus 2, where the popup width is the terminal width up to 62), the bar MUST degrade in this order: bar plus hint when W >= BAR + 13; bar only when BAR <= W; only the active tab as ` ‹ Models › ` (its label between `‹` and `›`), clipped, when W < BAR. Below 24 columns the existing one-line fallback applies. The active-only step is reachable in the full popup (a 5-tab bar below 49 columns, or below 50 with `Settings*`; a 4-tab bar below 36 columns, or below 37 with `Settings*`) and MUST be covered by end-to-end scenarios. The bar MUST be clipped to the popup and MUST NOT panic at any size. While an update is waiting (see Updates row) the `Settings` label MUST read `Settings*`. With a single tab (the no-AI path) the bar MUST be hidden and the popup layout MUST be unchanged from before this change. The hint is hidden while a sub-level is open (see Sub-levels keep the bar inert). With tabs shown, section titles are not drawn (the bar names the tab); with the bar hidden, section titles are drawn as before.
(Previously: two tabs, hint `⇥ Tab` of 5 cells shown from W >= BAR + 7, and the active-only step a guard that three or fewer tabs could not reach)

#### Scenario: Active tab highlighted [auto, TestBackend + insta]
- GIVEN the menu open on a normal terminal, active tab `Settings`, then Tab pressed
- WHEN each state is rendered
- THEN the bar lists `Settings`, `Models`, `Tools`, `Chat`, with `Settings` in the active style in the first and `Models` in the second, idle tabs and the `⇥ Tab · 1-5` hint in `text_muted`, and no section title rows are drawn

#### Scenario: Standard width, four tabs [auto, TestBackend + insta]
- GIVEN Connectors hidden, no update, a terminal 80 columns wide
- WHEN rendered
- THEN the bar (34) and the hint `⇥ Tab · 1-5` are drawn, the hint ending one column before the inner right edge

#### Scenario: Standard width, five tabs [auto, TestBackend + insta]
- GIVEN a test table with Connectors visible, no update, a terminal 80 columns wide (W = 60)
- WHEN rendered
- THEN the 47-cell bar and the hint are both drawn (W = 60 = BAR + 13)

#### Scenario: Five tabs and an update drop the hint [auto, TestBackend + insta]
- GIVEN Connectors visible and an update waiting (BAR = 48), a terminal 80 columns wide (W = 60 < 61)
- WHEN rendered
- THEN the bar reads ` Settings* │ Models │ Tools │ Connectors │ Chat ` without the hint

#### Scenario: Thresholds of the five-tab bar [auto]
- GIVEN Connectors visible and no update
- WHEN rendered at 49, 48 and 62 columns
- THEN 49 columns (W = 47) draws the whole bar without hint, 48 columns (W = 46) draws only ` ‹ <active> › `, and 62 columns draws the bar and the hint

#### Scenario: Thresholds of the four-tab bar [auto]
- GIVEN Connectors hidden and no update
- WHEN rendered at 49, 48, 36 and 35 columns
- THEN 49 columns draws the bar and the hint, 48 columns the bar without hint, 36 columns (W = 34) the bar without hint, and 35 columns (W = 33) only the active tab

#### Scenario: Bar at 24 columns [auto, TestBackend + insta]
- GIVEN the AI section attached, a terminal 24 columns wide and 16 rows high
- WHEN rendered
- THEN the full popup is drawn with the bar reduced to ` ‹ Settings › `, clipped inside the popup, and Tab changes the label

#### Scenario: Active-only guard [auto]
- GIVEN the bar line function with labels whose bar is wider than W
- WHEN it is built
- THEN it yields only the active tab as ` ‹ label › `, clipped to W, without panic

#### Scenario: Fallback at 22 columns [auto]
- GIVEN the menu open with the AI section
- WHEN rendered at a terminal 22 columns wide
- THEN the one-line fallback is drawn and no bar

#### Scenario: Settings marked while an update waits [auto]
- GIVEN a newer version known and not yet installed, active tab `Chat`
- WHEN rendered
- THEN the bar reads `Settings*` for the Settings label

#### Scenario: No update, no marker [auto]
- GIVEN no newer version known
- WHEN rendered
- THEN the label reads `Settings` without `*`

#### Scenario: Single tab hides the bar [auto, TestBackend + insta]
- GIVEN the menu without the AI section (one tab)
- WHEN rendered
- THEN no bar is drawn, Tab and Shift+Tab change nothing and show the notice `No other tabs`, and the single-tab snapshots show no bar and no `⇥` hint

### Requirement: Sub-levels keep the bar inert
While a sub-level is open (privacy warning, picker level 1 or 2 and its screens, custom field, Hugging Face token field and its prompts, Feedback form) the menu tab bar MUST stay drawn with the same styles, with the active tab unchanged, but WITHOUT the `⇥ Tab · 1-5` hint (it would advertise keys that are swallowed), and MUST be inert: Tab and Shift+Tab (both encodings) and the digits `1` to `5` MUST NOT switch tabs and MUST NOT show a notice there. In the picker lists they are swallowed with no effect. In every text entry (custom field, Hugging Face token field, Feedback fields) a digit MUST be typed like any other character and Tab keeps the meaning that view gives it (the Feedback form moves between fields). The privacy warning MUST keep declining on any key other than an unmodified lowercase `y`, Tab, Shift+Tab and the digits included, and closes only the warning view.
(Previously: Tab only, active tab named `AI`; the digits did not exist)

#### Scenario: Tab and digits inert in the picker [auto]
- GIVEN the picker open at level 1, then at level 2
- WHEN Tab, `BackTab`, Tab+SHIFT and `2` are pressed
- THEN the picker view, its cursor, the active tab and the draft are unchanged, no notice shows, the bar is still drawn and it has no hint

#### Scenario: Custom field types digits and ignores Tab [auto]
- GIVEN the custom field focused with buffer `ab`
- WHEN `1`, Tab and BackTab are pressed
- THEN the buffer is `ab1`, the field stays focused and the active tab is unchanged

#### Scenario: Token field and Feedback form type digits [auto]
- GIVEN the Hugging Face token field focused, and in another run the Feedback title field
- WHEN `4` is pressed
- THEN the character is typed in the field and the active tab is unchanged

#### Scenario: Tab and digits decline the warning [auto]
- GIVEN the privacy warning shown, active tab `Chat`
- WHEN Tab is pressed, and in another run `5`
- THEN the warning closes, the draft stays `off`, nothing is acknowledged, and the active tab is still `Chat`

### Requirement: Popup height is fixed
The popup body height MUST be the maximum over every view: each visible tab's rows, the picker (title plus its viewport) and the privacy warning. The popup height MUST be that body plus the chrome and MUST NOT change when switching tabs or entering or leaving a picker, the custom field or the warning. The tab bar row is part of the body. The body (`MENU_BODY`) is 12 rows: a 1-row bar plus 11 rows for the tallest view (`VIEW_BODY`). The chrome stays 4 rows, so the popup is 16 rows (it was 25 with the 19-row `AI` tab). The views need: the picker 11 (title plus its 10-row viewport), the Settings tab 11 (eight rows, one gap, the two-line note), the Tools tab 9 (eight rows and the reserved caution line), the privacy warning 10, the Models tab 8, the Chat tab 3 and the custom entry 2; the picker and the Settings tab are the binding 11, so the minimum was 24 columns by 15 rows. Adding one row to the Settings tab (the ai-cost `Usage (alpha)` row) makes the Settings tab 12 and the popup 17 rows, and the minimum 24 columns by 16 rows. The single-tab (no-AI) layout keeps its previous size.
(Previously: `AI` tab of 19 rows plus the caution made the body 21 and the popup 25)

#### Scenario: Same height across views [auto, TestBackend + insta]
- GIVEN a normal terminal
- WHEN the Settings, Models, Tools and Chat tabs, a picker level 1 and 2, the custom field and the warning are rendered
- THEN the popup border spans 17 rows in all of them (16 before the ai-cost `Usage (alpha)` row; that row takes the extra one)

#### Scenario: Adding a row grows only the tallest view [auto]
- GIVEN the height is derived from the tables, with the picker and the Settings tab both at 11
- WHEN the tallest view is measured, and then a 9th Settings row is added
- THEN the fixed body equals the tallest view and no view exceeds it, and with the 9th Settings row the popup grows by one row; adding a row to Chat or Models does not change the height

### Requirement: Update note is Settings-only
The two-line update note MUST be drawn only on the Settings tab, inside the fixed body in its last 2 rows (just above the blank row and the footer), and MUST NOT appear on Models, Tools, Connectors or Chat or in any sub-level. The note MUST NOT make the popup taller than the fixed height and MUST NOT trigger the fallback. On the single-tab path the note keeps its previous position under the list.
(Previously: "MUST NOT appear on the AI tab")

#### Scenario: Note on Settings [auto, TestBackend + insta]
- GIVEN a newer version known, active tab `Settings`
- WHEN rendered
- THEN the update note shows in the last 2 body rows, above the blank row and the footer

#### Scenario: No note elsewhere [auto]
- GIVEN a newer version known, active tab `Models`, then `Tools`, then `Chat`
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

### Requirement: Engines without a chat adapter are not selectable
This amends level 1 of the Task picker. An available engine that chat has no adapter for (today only `opencode`; see chat-engines, ChatEngine contract) MUST still be listed in catalog order with its status text `detected · not in chat yet`, but its row MUST be disabled: the cursor reaches it, Enter does nothing (the picker stays at level 1 and the draft is unchanged), and the footer reads `detected · not in chat yet · not selectable`. Every engine with an adapter, `lmstudio`, `llamacpp` and `vllm` included, is selectable as before. An assigned engine that is not detected keeps its `(not detected)` row, selectable.

#### Scenario: A detected engine without an adapter is listed but disabled [auto]
- GIVEN llamacpp available with one model and opencode available
- WHEN level 1 opens
- THEN `llamacpp available · 1 model` is selectable and `opencode detected · not in chat yet` is listed but disabled

#### Scenario: Enter on it does nothing [auto]
- GIVEN the cursor on the opencode row of level 1
- WHEN Enter is pressed
- THEN the picker stays at level 1 on that row and the draft is unchanged

#### Scenario: The footer gives its status as the reason [auto]
- GIVEN the cursor on the opencode row of level 1
- WHEN the footer is drawn
- THEN it reads `detected · not in chat yet · not selectable`

### Requirement: Stopped servers in the picker
This amends level 1 and level 2 of the Task picker. Level 1 MUST list, after the available engines and before the assigned engines that are not detected, every server engine (`ollama`, `lmstudio`, `llamacpp`, `vllm`) whose binary is on PATH but that did not answer on its port, in catalog order, with the status text `not running`, selectable. Such an engine is listed once, even when a task has it assigned. Level 2 of that engine MUST show, after its assigned models (disabled), a note instead of `engine not detected`: `<id> not running on 127.0.0.1:<port> · start it, then press r: <command>`, where `<port>` is the port detection used (see configuration, Server ports) and `<command>` starts that server on that port on `127.0.0.1`: `ollama serve` (or `OLLAMA_HOST=127.0.0.1:<port> ollama serve` off its default port), `lms server start --port <port>`, `llama-server --port <port> -m <model.gguf>`, `vllm serve <model> --host 127.0.0.1 --port <port>`. The note follows the copyable-note rules (wrapped, not selectable, Enter or `y` copies the whole text). lazarobox never runs that command. With the cursor on one of its saved models (disabled), the footer reads `not running · not selectable`. A server that is not installed keeps `engine not detected`.

#### Scenario: A stopped server is listed [auto, TestBackend + insta]
- GIVEN ollama and llamacpp installed but not answering, lmstudio not installed, claude available
- WHEN level 1 opens
- THEN after claude come `ollama not running` and `llamacpp not running`, both selectable, and lmstudio is not listed

#### Scenario: Choosing a stopped server says how to start it [auto]
- GIVEN `[ai.servers.llamacpp] port = 39080`, llamacpp installed but not answering, and chat assigned to `llamacpp` / `qwen`
- WHEN level 2 of llamacpp opens
- THEN it shows `qwen` disabled and current (footer `not running · not selectable`), then the note `llamacpp not running on 127.0.0.1:39080 · start it, then press r: llama-server --port 39080 -m <model.gguf>`, and Enter on the note copies it

#### Scenario: A server that is not installed is unchanged [auto]
- GIVEN llamacpp not installed
- WHEN level 2 of llamacpp opens
- THEN its last row is `engine not detected`

### Requirement: Ollama download row
The model level of the `ollama` picker MUST end with a row `download from Ollama…` (key `PickKey::OllamaDownload`, mark Plain). It is shown when Ollama is `available` (its server answered detection) and is enabled then. While a pull runs, the row reads `↓ <percent>% <model>:<tag> · <license title>` instead, and Enter on it asks `cancel the download? (y/n)`. While a Hugging Face download runs, the row stays but Enter on it says `a download is already running`. Likewise, while an Ollama pull runs, the llamacpp row `download from Hugging Face…` keeps its label and Enter on it says `a download is already running`.

#### Scenario: Row only with a running server [auto]
- GIVEN ollama `available`, then ollama `not running`
- WHEN its model level is opened
- THEN the last row is `download from Ollama…` in the first case, and absent in the second

### Requirement: Ollama screens
Enter on `download from Ollama…` opens, in the picker, these levels in turn. Esc on each goes back one level, and no level writes a config edit.
1. **Popular.**
   - Title: `Ollama: most popular for this PC`.
   - Footer: `j/k move · Enter choose · Esc back`, and `asking ollama.com…` while asking.
   - Rows, up to 20: `<name>  <pulls> pulls  <sizes that fit>`, for example `llama3.2  86.3M pulls  1b 3b`, followed by a tool mark when the rules of Tool marks on Ollama download screens give one.
   - Nothing that fits reads the info row `no popular model fits this PC`.
   - The fallback adds its info row first.
2. **Sizes.**
   - Title: `Ollama: <name>`.
   - Rows: `<name>:<tag>  <exact size>`, followed by a tool mark when the same rules give one. A tag too big for this PC ends with ` · too big for this PC` and stays choosable.
   - Enter opens the confirmation.
3. **Confirm.** Info rows, then the footer `y download · n back`:
   - `download <name>:<tag> (<size>)`
   - `license: <title>`, one per license layer, or `license: not stated`
   - `read it: https://ollama.com/library/<name>:<tag>`
   - `to: <Ollama models folder>`, or `to: the Ollama server's models folder`
   - `free: <free space>`, or `free: unknown`

   `y` starts the pull and goes back to the ollama model level. `n` or Esc goes back. The confirmation carries no tool mark.

#### Scenario: Pick without typing [auto]
- GIVEN the Popular reply with `llama3.2` (`1b`, `3b`) and 8 GB of RAM
- WHEN Enter, Enter on `llama3.2`, Enter on `llama3.2:3b`, then `y`
- THEN the effects are the library ask, the sizes ask, the license ask and one pull of `llama3.2:3b`, in order, and nothing is pulled before `y`

#### Scenario: Late reply dropped [auto]
- GIVEN the Sizes request in flight
- WHEN Esc is pressed and its reply arrives
- THEN the reply is dropped and the Popular screen stays

### Requirement: Local models next to a running llama-server
With llamacpp managed, the llamacpp model picker MUST list the local model files (marked `lazarobox starts it`, the managed one first) also when a llama-server already answers on the managed port, and choosing one MUST make it the managed model as when the server is stopped: each model gets a server of its own (see chat-engines, Managed llama-server lifecycle). Unmanaged, a running llama-server lists its own models as before.

#### Scenario: Another server answers [auto]
- GIVEN llamacpp managed with `/m/b.gguf`, the files `/m/a.gguf` and `/m/b.gguf`, and a llama-server serving `/m/x.gguf` on the port
- WHEN the llamacpp models of a task are listed
- THEN the rows are `b.gguf` (current) and `a.gguf`, both `lazarobox starts it`, and choosing `b.gguf` makes it the managed model

### Requirement: Tools row
The Tools tab MUST hold a static row `Tools` as its first row, before the family rows and `Approval` (key path `ai.tools.enabled`, values `off`, `on`; it reads `on` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft (an Enter repeat is ignored, like on `Context`); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to a stream, a pending approval or a running tool, and it applies from the next turn after a successful save. Esc at the top level restores the draft; `s` saves `ai.tools.enabled` through config-persistence only when it differs. The row order of the tab is `Tools, Search, Write, Exec, Git, Git write, Approval, Orchestrator` (see Sections and items are data); Navigation wraps inside the Tools tab; the popup does not change size (see Popup height is fixed); the descriptor integrity rules extend to the row (`Tools` has key path `ai.tools.enabled`, values `off`, `on`, no duplicate key path).
(Previously: "a static row after `Memory` and before `Approval`" on the AI tab; Navigation wrapped inside the AI tab)

#### Scenario: Navigation reaches Tools and wraps [auto]
- GIVEN the Tools tab active with `Orchestrator` selected
- WHEN `j` is pressed
- THEN `Tools` is selected, the active tab is still `Tools`, and from `Tools` `k` selects `Orchestrator`

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
- GIVEN a terminal at exactly the minimum (24 columns by 16 rows), and one column or one row less
- WHEN the Tools tab and the picker are rendered
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
The Tools tab MUST hold a static row `Approval` after the family rows and before `Orchestrator` (key path `ai.tools.approval`, values `ask`, `auto`, `run`; it reads `ask` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft `ask`, `auto`, `run`, `ask` (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Entering `run` needs no confirmation. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards and at the next conversation switch inside an open pane. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs, and the value is written explicitly even when it equals the default. The row is shown whatever the `Tools` draft. The popup keeps its size (16 rows and a 24x15 minimum then, 17 and 24x16 with the Usage row; see Popup height is fixed); the descriptor integrity rules extend to the row.
(Previously: the last row of the AI tab; "With it the AI tab has 11 rows")

Note: when the file holds a bad value (for example `approval = "AUTO"`), the row opens on `ask` and choosing `ask` writes nothing, so the bad value stays in the file until `auto` or `run` is saved. `Tools` behaves the same way. This is accepted (design D18); no force flag exists.

#### Scenario: Cycle [auto]
- GIVEN `Approval` selected with the draft `ask`
- WHEN `l` is pressed three times
- THEN the draft is `auto`, then `run`, then `ask`, no save is requested and no file is touched

#### Scenario: Enter repeat ignored [auto]
- GIVEN `Approval` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: No effect on open chats [auto]
- GIVEN an open chat in `ask` and the menu open on `Approval`
- WHEN the draft is cycled to `run` and saved
- THEN the open chat is still `ask`, no WritePty is emitted, and a chat pane opened afterwards is `run`

#### Scenario: Esc reverts [auto]
- GIVEN the draft changed to `run`
- WHEN Esc is pressed on the task list
- THEN the saved mode is still `ask` and no file is read or written

#### Scenario: Footer [auto, TestBackend + insta]
- GIVEN `Approval` selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and the value reads `ask`

#### Scenario: Run value rendered [auto, TestBackend + insta]
- GIVEN the draft is `run`
- WHEN rendered
- THEN the row value reads `run` and no other row or the popup height changes

#### Scenario: Popup size is unchanged [auto, TestBackend + insta]
- GIVEN the Tools tab, a terminal of 80x24 and one of exactly 24x16
- WHEN rendered
- THEN both show the full popup of 17 rows (the 24x16 one clipped, without panic; it was 16 rows and 24x15 before the Usage row, which takes the extra one), and one column or one row less shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Approval` exists with key path `ai.tools.approval` and values `ask`, `auto`, `run`, and no key path is duplicated

#### Scenario: Approval row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Approval` is set to `run` and saved, and a new chat is opened
- THEN the new chat shows ` RUN `, the chat opened before does not, and the file holds `approval = "run"` under `[ai.tools]`

### Requirement: Family toggle rows
The Tools tab MUST hold five static rows, one per family toggle (Search, Write, Exec, Git, GitWrite), placed after the `Tools` row and before `Approval`. Each reads `off` by default, with values `off`, `on` and the key path of its config key. Each is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft; an Enter repeat is ignored. Changing a draft MUST have no live effect and applies from the next turn after a successful save. Esc restores the draft; `s` saves only the keys that differ. The Write, Exec and GitWrite rows MUST show a one-line caution that the family can change files or run code, in the line reserved at the foot of the Tools tab. Navigation wraps inside the Tools tab, the popup does not change size, and the descriptor integrity rules extend to the new rows (no duplicate key path).
(Previously: rows of the AI tab)

#### Scenario: Navigation reaches every row [auto]
- GIVEN the Tools tab with `Tools` selected
- WHEN `j` is pressed five times
- THEN the five family rows are selected in order and the sixth press selects `Approval`

#### Scenario: Toggle [auto]
- GIVEN the Write row with the draft `off`
- WHEN `l` is pressed, then `h`
- THEN the draft is `on`, then `off`, and no file is touched

#### Scenario: Save [auto]
- GIVEN the Exec draft changed to `on`
- WHEN `s` is pressed and the save succeeds
- THEN the file holds the exec key as `true` and the next turn offers `run_command`

#### Scenario: No live effect [auto]
- GIVEN a running stream with a pending approval and the Write draft cycled
- WHEN the draft changes
- THEN no ResizePty or WritePty is emitted and the stream and approval are untouched

#### Scenario: Caution line [auto, TestBackend + insta]
- GIVEN the Exec row selected
- WHEN the Tools tab is rendered
- THEN the caution line is visible and the popup size is unchanged

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN the five rows exist with distinct key paths and values `off`, `on`

### Requirement: HF row independent of llama-server [S1]
The `download from Hugging Face…` row on the llamacpp engine MUST be shown whenever the engine is selectable, including when its state is `not installed`, because the download needs only `curl`. When `llama-server` is not installed the row MUST carry a note that `llama-server` is needed to run downloaded models. Nothing else about the download flow changes.
(Previously, in the pending hf-download delta: the row was hidden when llamacpp was `not installed`.)

#### Scenario: Row visible when not installed [auto]
- GIVEN llamacpp `not installed`
- WHEN the llamacpp menu is built
- THEN the HF download row is present with the "llama-server needed to run" note

#### Scenario: No note when installed [auto]
- GIVEN llamacpp `not running` or `running`
- WHEN the menu is built
- THEN the row is present without the note

#### Scenario: Download works without llama-server [auto]
- GIVEN llamacpp `not installed` and `curl` available
- WHEN the user starts the HF download
- THEN the flow proceeds exactly as with llama-server present

#### Scenario: Row still hidden without curl [auto]
- GIVEN `curl` missing
- WHEN the menu is built
- THEN the row behaves as defined by hf-download for the missing-curl case

### Requirement: Install and remove entries [S4]
When llamacpp is `not installed`, the llamacpp row MUST offer `install llama-server…` opening the wizard. When an owned install exists, it MUST offer `remove llama-server`. With a system binary on PATH and no owned install neither entry is shown.

#### Scenario: Install entry when not installed [auto]
- GIVEN llamacpp `not installed`
- WHEN the row menu is built
- THEN `install llama-server…` is listed and opens the wizard

#### Scenario: Remove entry with owned install [auto]
- GIVEN an owned install
- WHEN the row menu is built
- THEN `remove llama-server` is listed

#### Scenario: No entries with system binary only [auto]
- GIVEN a system binary and no owned install
- WHEN the row menu is built
- THEN neither entry appears

### Requirement: Hugging Face row in the llamacpp picker
The model level of the `llamacpp` picker MUST end with a row `download from Hugging Face…` (key `PickKey::HfDownload`, mark Plain, always enabled). It is shown whenever llama-server is installed (status not `not installed`), with or without local models. While a download runs, the row reads `↓ <percent>% <file> · <license>` instead, and Enter on it asks `cancel the download? (y/n)`.

#### Scenario: The row is last [auto]
- GIVEN llamacpp stopped with two local models
- WHEN its model rows are built
- THEN the last row is `download from Hugging Face…`, enabled, and the local models come before it

#### Scenario: The row shows a running download [auto]
- GIVEN a download of `a-Q4_K_M.gguf` (license `apache-2.0`) at 42 %
- WHEN the model rows are built
- THEN the last row reads `↓ 42% a-Q4_K_M.gguf · apache-2.0`

### Requirement: Hugging Face screens
Enter on `download from Hugging Face…` opens, in the picker, these levels in turn. Esc on each goes back one level; no level writes a config edit.
0. **Popular** (hf-popular): Enter on the row asks for the most downloaded text-generation GGUF repos (`Ask::Popular`, up to 100) and opens over Query, with no typing needed. Its title reads `Hugging Face: most downloaded for this PC` and its footer `j/k move · Enter choose · Esc search`; Esc opens Query. It lists, sorted by downloads, up to 20 repos whose size fits this PC: the size is estimated from the Hub's GGUF parameter count at 5 bits per parameter (about Q4_K_M), and a model fits when it takes at most 70% of the total memory. Repos with no parameter count are left out. With the memory unknown nothing is left out. Nothing left reads the info row `no popular model fits this PC; Esc to search`. A failed request shows its message on Query.
1. **Query**: a text field like `custom…`, with the hint `search the Hub or owner/repo`. Enter with text of the form `<owner>/<repo>` (one `/`, both sides non-empty, no space) opens the files of that repo. Any other non-empty text searches. Empty text does nothing. The footer reads `Enter search · Esc back`.
2. **Repos**: up to 20 rows `<owner>/<repo>  <downloads> ↓`, followed by `  ~<estimated size>` when the Hub gives the parameter count, sorted by downloads. A repo whose estimate does not fit this PC ends with ` · too big for this PC`. No result reads the info row `no GGUF models found for "<text>"`. Enter opens the repo's files.
3. **Files**: one row per GGUF choice of model-download, `<quant>  <size>  <file>` (or `(<n> parts)` after the size of a split model), ending with ` · too big for this PC` when that size does not fit this PC's memory (known memory only). The first row is the info row `license: <id>`. No choice reads the info row `no .gguf files in <owner>/<repo>`. Enter opens the confirmation.
4. **Confirm**: info rows, then the footer `y download · n back`:
   - `download <file> (<size>)`
   - `from <owner>/<repo>`
   - `license: <id>`
   - `read it: <link>`
   - `to: <hub folder>`
   - `free: <free space>`

   `y` starts the download and returns to the llamacpp model level, whose last row then shows the progress. `n` or Esc goes back to Files. When the free space is short, the footer shows the disk space error and `y` does nothing.

While a request runs, the footer reads `asking Hugging Face…` and keys other than Esc are ignored. A failed request shows its model-download message as the footer error and stays on the level that asked. A reply that arrives after the user left that level is dropped. Every label from the Hub is sanitized like the other picker labels (control and format characters drawn as `?`).

#### Scenario: Paste a repo id [auto]
- GIVEN the Query level with `Qwen/Qwen2.5-0.5B-Instruct-GGUF` typed
- WHEN Enter is pressed
- THEN an effect asks for that repo's files, no search effect is emitted, and the footer reads `asking Hugging Face…`

#### Scenario: Search then pick [auto]
- GIVEN the Query level with `qwen 0.5b` typed
- WHEN Enter is pressed, a reply with two repos arrives, Enter opens the first, its files reply arrives, and Enter is pressed on `Q4_K_M`
- THEN the Confirm level shows the file, its size, `license: apache-2.0`, its link, the hub folder and the free space, and no download effect has been emitted yet

#### Scenario: Nothing downloads before y [auto]
- GIVEN the Confirm level
- WHEN `n`, Esc, `j`, Enter and `s` are pressed
- THEN no download effect is emitted and no config edit exists; only `y` emits it, once

#### Scenario: Error stays on its level [auto]
- GIVEN the Query level with `gated/model` typed
- WHEN the reply is a 401
- THEN the footer error reads `gated/model needs a Hugging Face token: set HF_TOKEN` and the Query level keeps its text

#### Scenario: Late reply dropped [auto]
- GIVEN a files request in flight
- WHEN Esc is pressed and then its reply arrives
- THEN the picker stays on the level Esc went to

### Requirement: Download in the status bar
While a download runs, the status bar MUST show `↓ <percent>%` after the other right-hand marks. When it ends, the status bar shows `downloaded <file>` (or the failure message) as a notice until the next key, and the engines are detected again. A cancel shows `download cancelled · run it again to resume`.

#### Scenario: Progress then done [auto]
- GIVEN a download started from the picker
- WHEN progress 10 %, 55 % and the end arrive
- THEN the status bar reads `↓ 10%`, then `↓ 55%`, then `downloaded <file>`, and a redetect effect is emitted once

### Requirement: Hugging Face account row
The Models tab MUST end with a static row `Hugging Face`, after the `orchestrate` row. Its value is:
- `not connected` when no token is found;
- `token saved` when a token file exists but no account has been checked yet; Enter on the row then checks it with that file's token (nothing is sent when the menu opens);
- `<name>` once an account is known, from a connect or a check in this session;
- `token set (HF_TOKEN)` while `HF_TOKEN` is set.

Enter on the row opens a field with the hint `paste a Hugging Face token`, followed by `create a read token at:` and, on a line of its own, `https://huggingface.co/settings/tokens/new?tokenType=read`. The field draws one `•` per character. Ctrl+O opens that link in the browser through the host's opener (`xdg-open`; on WSL `wslview`, then `rundll32.exe url.dll,FileProtocolHandler`; on macOS `open`), and the footer says `Opened in your browser`; on a host with no opener Ctrl+O copies the link instead. When the opener fails (it exits with an error within 5 seconds of its start; a later exit is the browser's own; the exit of a Windows program such as `rundll32.exe` is not checked), the link is copied, the footer says `The browser did not open: link copied`, and Ctrl+O copies from then on. With `Mouse` on, a left click on the link line does the same as Ctrl+O; any other click in the menu does nothing. With `Mouse` off the host terminal handles clicks itself. The field's footer is `Enter connect · Ctrl+O open link · Ctrl+U clear · Esc back`, or `… Ctrl+O copy link …` with no opener. Enter checks the token, with the footer `asking Hugging Face…`, and Esc goes back; Esc during the check stops it, and the token is not saved unless the save had already happened. On a known account, Enter offers instead `disconnect <name>? (y/n)`, which deletes the token file on `y`. With a saved token, `d` offers the same prompt, also when its account is unknown (`disconnect the saved token? (y/n)`), and the footer of such a row is `Enter check · d disconnect · j/k move · s save · Esc cancel`. The row writes no config edit.
(Previously: the last row of the AI tab, after `Approval`)

#### Scenario: Account row [auto]
- GIVEN the Models tab
- WHEN rendered with no token
- THEN the last row reads `Hugging Face` with `not connected`, after `orchestrate`

#### Scenario: Ctrl+O opens the token link [auto]
- GIVEN the token field, on a host with an opener
- WHEN Ctrl+O is pressed
- THEN `https://huggingface.co/settings/tokens/new?tokenType=read` is opened in the browser, the field stays open and the footer reads `Opened in your browser`
- AND with no opener the link is copied instead

#### Scenario: A click opens the token link [auto]
- GIVEN the token field with `Mouse` on, on a host with an opener
- WHEN the line that shows `https://huggingface.co/settings/tokens/new?tokenType=read` is clicked with the left button
- THEN the link is opened in the browser, the field stays open and the footer reads `Opened in your browser`
- AND a click on any other row of the field or of the menu does nothing

#### Scenario: The field hides the token [auto]
- GIVEN the token field
- WHEN `hf_abc` is pasted
- THEN the field shows `••••••` and no rendered cell holds `hf_abc`

### Requirement: Feedback row
The Settings tab MUST end with a row `Feedback` whose value is `bug report or idea`, after `Updates`. Its footer reads `Enter open · j/k move · s save · Esc cancel`. Enter opens the Feedback form, which replaces the rows like the other sub-views (the tab keys are inert there). The row writes no config edit.

#### Scenario: The row is last [auto]
- GIVEN the menu without the AI section
- WHEN its rows are built
- THEN the last row is `Feedback  bug report or idea`

### Requirement: Feedback form
The form shows, under the title `Feedback`:
- `Kind: Bug report` or `Kind: Idea or goal`;
- `Title` and one line of the title;
- `Description` and up to six lines of the description;
- `[x] Attach the environment` (or `[ ]`), and while it is checked, the facts.

The focused field is marked `›`, and the focused text field draws its cursor as `▏`.

The facts are two lines:
- `lazarobox <version> · <os>[ (WSL)] · <shell> · <terminal>`;
- `engines: <id> [<version>] <available|not running> · …`, or `engines: not detected`.

They are wrapped between items, so no fact is cut at 48 columns. An engine that is not installed is left out. A shell, terminal or version that is not printable ASCII is left out.

Keys:
- Tab and BackTab move between the fields.
- ←/→ change the kind.
- Space or Enter on the attach line toggles it.
- Enter in the title goes to the description, and Enter in the description adds a line.
- A paste goes to the focused text field; in the title, its newlines become spaces.
- Esc goes back to the row.

The footer reads:
- `←/→ kind · Tab · Ctrl+S send · Ctrl+O open · Esc` on the kind;
- `Tab next · Ctrl+S send · Ctrl+O open · Esc back` on a text field;
- `Space attach · Ctrl+S send · Ctrl+O open · Esc` on the attach line.

Ctrl+S sends the issue with the user's own `gh` (see Requirement: Sending with gh). Ctrl+O builds the prefilled new-issue link and opens it in the browser through the host's opener; the view then reads `Opened in your browser. GitHub shows the issue`, `form filled in, for you to review and send.` until Esc. On a host with no opener Ctrl+O copies the link instead, and the view reads `Link copied. Paste it in your browser:`, `GitHub opens the issue form filled in, for you`, `to review and send.`, with the footer `Copied <n> characters`, then `Esc back`. When the opener fails (it exits with an error within 5 seconds of its start, for example over SSH with no browser; a later exit is the browser's own), the link is copied instead, the view reads the copied text, the footer says `The browser did not open: link copied`, and later links are copied.

Both keys check the form first: the title and the description are required, and both go through the redaction pass. The link of Ctrl+O is capped at 8000 bytes; the body Ctrl+S sends is capped at GitHub's 65536 characters.

#### Scenario: Ctrl+S sends the issue [auto]
- GIVEN a bug report titled `Scroll jumps`, described `It jumps\nback`, with the environment attached
- WHEN Ctrl+S is pressed
- THEN the runtime runs `gh issue create --repo pichu2707/lazarobox-shell --title "Scroll jumps" --body-file - --label bug`, with the body on stdin
- AND the body reads `### What happened`, a blank line, `It jumps\nback`, a blank line, `### Environment`, a blank line, and the facts in a `text` code block
- AND the footer reads `Sending to GitHub…` until the answer

#### Scenario: Ctrl+O opens the prefilled page [auto]
- GIVEN the bug report above, on a host with an opener
- WHEN Ctrl+O is pressed
- THEN `https://github.com/pichu2707/lazarobox-shell/issues/new?template=bug.yml&title=Scroll%20jumps&what-happened=It%20jumps%0Aback&environment=<the facts>`, percent-encoded, is opened in the browser, and the view reads `Opened in your browser. GitHub shows the issue`, `form filled in, for you to review and send.`
- AND Esc goes back to the row

#### Scenario: Ctrl+O without an opener copies the link [auto]
- GIVEN the bug report above, on a host with no opener
- WHEN Ctrl+O is pressed
- THEN the link is copied, and the view reads `Link copied. Paste it in your browser:`, `GitHub opens the issue form filled in, for you`, `to review and send.`, with the footer `Copied <n> characters`, then `Esc back`
- AND Esc goes back to the row

#### Scenario: An idea, without the environment [auto]
- GIVEN an idea with the attach line unchecked
- WHEN Ctrl+S is pressed
- THEN the issue is labelled `idea` and its body is only `### The idea`, a blank line and the description
- AND Ctrl+O uses `template=idea.yml`, puts the description in `idea=` and has no `environment=`

#### Scenario: Required fields [auto]
- WHEN Ctrl+S is pressed with an empty title (or an empty description)
- THEN nothing is sent, the footer error reads `Title is required` (or `Description is required`), and the cursor moves to that field

#### Scenario: Secrets are masked first [auto]
- GIVEN a description holding `token=hf_…`
- WHEN Ctrl+S is pressed
- THEN nothing is sent, the field now reads `[REDACTED]` in its place, and the footer error reads `Secrets were masked; review and press Ctrl+S again`
- AND the next Ctrl+S sends the issue without the secret

#### Scenario: Too long for a link [auto]
- WHEN the link of Ctrl+O would be longer than 8000 bytes
- THEN no link is made, and the footer error reads `Too long for a link: shorten the description`
- AND Ctrl+S still sends the same text

### Requirement: Sending with gh
The app MUST send only on the user's Ctrl+S, and only through the user's own `gh`: lazarobox stores no GitHub token. `gh` is found on the login PATH the engines use and run with no shell, as `gh issue create --repo <owner/repo> --title <title> --body-file - --label <bug|idea>`, the body on stdin, stdout and stderr captured, the environment inherited. It runs off the UI thread and is killed 30 seconds after Ctrl+S, the retry without labels included. One send at a time: while it runs, every key of the form, Ctrl+S and Esc included, does nothing.

The issue keeps the shape of the repository's issue forms: one `### <label>` section per filled field (`What happened` for a bug report, `The idea` for an idea), and `### Environment` with the facts when attached. The label `bug` or `idea` is asked for; GitHub keeps it only for users with write access, so a failure naming a label or a permission is tried once more without the label. Project 17 is not set from the app.

`gh` is taken as missing when it is not on PATH, and as not logged in when it exits with code 4 or its message names `gh auth login`.

#### Scenario: Sent [auto]
- GIVEN Ctrl+S was pressed
- WHEN `gh` prints one line `https://github.com/pichu2707/lazarobox-shell/issues/<n>`
- THEN the view reads `Sent:` and that link on a row of its own, with the footer `Ctrl+O open · Esc back` (`Ctrl+O copy link · Esc back` with no opener)
- AND Ctrl+O, or a left click on the link row, opens it
- AND Esc goes back to the row
- AND any other output counts as a failure

#### Scenario: gh fails [auto]
- WHEN `gh` fails or times out
- THEN the form stays with everything typed, and the footer error reads `gh: <its message>` on one line, control characters removed, cut to 48 columns
- AND Ctrl+S sends again

#### Scenario: No gh [auto]
- WHEN `gh` is missing or not logged in
- THEN the prefilled page opens in the browser, as Ctrl+O would, and the view reads `gh is not installed or not logged in:`, `opened the form in your browser, for you to`, `review and send.`
- AND on a host with no opener the link is copied, and the view reads `gh is not installed or not logged in: link`, `copied. Paste it in your browser to review`, `and send the issue.`
- AND when the link would be too long, the form stays with the footer error `No gh, and too long for a link: shorten it`

### Requirement: Tool marks on Ollama download screens
The Popular and Sizes rows MUST carry the same tool marks as installed models (see ai-task-models, Tool marks on installed Ollama models), taken from the library data: whether the model card declares `tools`, and the parameter size. No request is added for this; the marks use data the screens already read, and a card whose data is missing gets no mark.
- Sizes row: decided by that tag's size. Declared tools and 3B or more reads `🔧`; declared tools and under 3B reads `tools: unreliable`; tools not declared, or size unknown: no mark.
- Popular row: decided by the LARGEST size shown on the row (the largest size that fits this PC). Declared tools and that size is 3B or more reads `🔧`; declared tools and that size is under 3B reads `tools: unreliable`; tools not declared, or no size known: no mark. A row with mixed sizes such as `1b 3b` therefore reads `🔧`.

Exactly 3B counts as 3B or more. The marks use the text fallback `tools` on `TERM=linux` or a non-UTF-8 locale (see ai-task-models, Tool mark text fallback). A mark MUST NOT change the row's selectability, the row order or the pull flow.

#### Scenario: Popular row with tools [auto]
- GIVEN the Popular reply with `llama3.2` (`1b`, `3b`, tools declared) and `smol` (`1b`, tools declared) and `chat-only` (`7b`, no tools)
- WHEN the Popular screen is built
- THEN `llama3.2` carries `🔧`, `smol` carries `tools: unreliable`, and `chat-only` carries no mark

#### Scenario: Popular row with mixed sizes [auto]
- GIVEN the Popular reply with `mix` (sizes `1b 7b`, only `1b` fits this PC, tools declared) and `mix2` (sizes `1b 3b 8b`, all fit, tools declared)
- WHEN the Popular screen is built
- THEN `mix` carries `tools: unreliable` (largest fitting size 1B) and `mix2` carries `🔧` (largest fitting size 8B)

#### Scenario: Sizes rows [auto]
- GIVEN the Sizes screen of `llama3.2` with tools declared, tags `1b` and `3b`
- WHEN it is built
- THEN `llama3.2:1b` carries `tools: unreliable`, `llama3.2:3b` carries `🔧`, and a `too big for this PC` suffix still ends its row where it applies

#### Scenario: Missing card data [auto]
- GIVEN a Popular card with no capabilities data, and the fallback list
- WHEN the screens are built
- THEN the rows carry no tool mark and no row is dropped

#### Scenario: Text fallback on download screens [auto]
- GIVEN `TERM=linux` and the Popular reply above
- WHEN the Popular screen is built
- THEN `llama3.2` carries `tools` in place of `🔧`

#### Scenario: Marks do not change the pull [auto]
- GIVEN marked rows
- WHEN the pick flow of `Pick without typing` is run on a marked row
- THEN the effects and order are the same as for an unmarked row

### Requirement: Orchestrate row and picker [S1]
Enter on the `orchestrate` row of the Models tab MUST open the same two-level task picker as the other tasks, with `(default · use general)` as its first entry. In its engine list the server engines MUST carry the status text plus `not recommended` and stay selectable. Choosing an entry changes only `orchestrate`. Enabling the orchestrator from the `Orchestrator` row of the Tools tab while `orchestrate` resolves to no engine MUST be allowed, and an orchestrating pane shows the engine guidance on the next send.
(Previously: both rows on the AI tab)

#### Scenario: Orchestrator row is the global gate [auto]
- GIVEN the Tools tab with `Orchestrator` `off`
- WHEN the row is cycled to `on` and saved
- THEN `[ai.orchestrator] enabled = true` is written, no pane changes mode by itself, and the next message of a pane with ORCH off is still a chat turn

#### Scenario: Picker first entry [auto]
- GIVEN the Models tab with `orchestrate` selected
- WHEN Enter is pressed
- THEN the title reads `Engine for orchestrate` and the first entry is `(default · use general)`

#### Scenario: Only orchestrate changes [auto]
- GIVEN `general` is `codex` and the picker for `orchestrate`
- WHEN `claude` then `opus` is chosen
- THEN `orchestrate` is `claude` / `opus` and the other tasks are unchanged

### Requirement: Picker opened from a failure notice [S2]
When the user picks `change model` or `change provider` in a worker failure alert, the app MUST open the menu task picker for that worker's kind (level 2 of the current engine for `change model`, level 1 for `change provider`), showing the same rows and marks as the menu. The picker starts from the effective assignment in use by the failed worker. A confirmed choice is a *run-scoped override*: it MUST apply until the run ends, MUST NOT modify the menu draft, MUST NOT be saved to the configuration, and MUST NOT make the menu show a pending change. It returns to the alert flow, which relaunches the worker on the override; later workers of that task in the same run use it too (see agent-orchestrator). The override MUST be shown in the run state and on the worker rows. Esc MUST create no override and return to the alert. While the picker is open the run stays paused.
(Previously: text unchanged; the last scenario said "the AI tab")

#### Scenario: Change model opens level 2 [auto]
- GIVEN a failed `review` worker on `claude`
- WHEN `change model` is chosen
- THEN the picker shows `Model for review (claude)` with the cursor on the current model

#### Scenario: Change provider opens level 1 [auto]
- GIVEN a failed `code` worker
- WHEN `change provider` is chosen
- THEN the picker shows `Engine for code`

#### Scenario: Choice is an override, not a save [auto]
- GIVEN the picker opened from a `review` alert and `claude` / `sonnet` confirmed
- WHEN the worker is relaunched
- THEN it runs on claude / `sonnet`, the menu draft and the saved file keep the earlier `review` assignment and no save is pending

#### Scenario: Menu value unchanged after the run [auto]
- GIVEN an override created in a run and the run ended
- WHEN the menu is opened and the Models tab shown
- THEN the `review` row shows the menu assignment from before the run

#### Scenario: Esc creates no override [auto]
- GIVEN the picker opened from an alert
- WHEN Esc is pressed
- THEN no override exists, the alert is shown and no worker is relaunched

### Requirement: Provider notice view [S1]
The one-time provider notice MUST be an informational view or inline notice with one accept key and a way to dismiss it; it MUST NOT stop the user from using the chat or the menu while it is shown, and dismissing without accepting MUST leave `acknowledged` false.

#### Scenario: Accept persists [auto]
- GIVEN the notice is shown
- WHEN the accept key is pressed
- THEN `acknowledged` is true and written to the config file at once (one config write, whose result never closes or marks a menu opened meanwhile), and the notice is gone; a failed write leaves it false and says why

#### Scenario: Dismiss does not accept [auto]
- GIVEN the notice is shown
- WHEN it is dismissed
- THEN `acknowledged` stays false

### Requirement: Save output is unchanged by the tab split
Moving rows between tabs MUST NOT change what `s` writes. For the same set of changed draft values, the ordered list of config edits (keys, values and order) MUST be identical to the list produced before the split, whatever the order in which the user visited the tabs and changed the rows. A characterization test MUST pin that list before any refactor of how draft rows are declared, and the same test MUST pass after it. The keys, their values and the write order in `config.toml` do not change, and no migration is involved.

#### Scenario: Same edits as before the split [auto]
- GIVEN a golden list of edits captured before the split, from a draft that changes one row of every kind (Settings choices, Mouse, every task, Profile, Context, Memory, Notify, Sound, `llama-server idle`, Tools, the five family rows, Approval, Orchestrator)
- WHEN the same changes are made and the edits are read
- THEN the list equals the golden list in keys, values and order

#### Scenario: Visit order does not matter [auto]
- GIVEN the same set of changes made once Settings first then Chat and Models, and once in the reverse order
- WHEN the edits are read in both runs
- THEN the two lists are identical

#### Scenario: Unchanged tabs write nothing [auto]
- GIVEN every tab visited and nothing changed
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written

### Requirement: Usage row belongs to Settings
When the ai-cost `Usage (alpha)` row is added, it MUST be declared on the Settings tab, MUST NOT need a new tab or a change to navigation or drawing code, and MUST keep every other row on its tab. The popup grows by the one row that Settings then needs (see Popup height is fixed).

#### Scenario: Usage is on Settings [auto]
- GIVEN the ai-cost row declared in the Settings table
- WHEN the tab table is validated and the Settings tab is rendered
- THEN `Usage (alpha)` appears on Settings only, the tab list is unchanged, and the popup is 17 rows high on every tab

### Requirement: Docs follow the tabs
The documents that tell the user how to reach the AI settings MUST name the tabs and the jump keys: the agent-orchestrator `manual-tests.md` (T02 and every step that says to press Tab to reach AI), `docs/assets/tapes/ai-menu.tape`, the README menu section and the CHANGELOG entry (which MUST say that the `AI` tab was split and that `1`-`5` jump).

#### Scenario: Manual tests name the tab [manual]
- GIVEN `manual-tests.md` of agent-orchestrator
- WHEN T02 and the AI-tab steps are read
- THEN each step names the tab (Models, Tools or Chat) or the digit to reach it, and none says "Tab to AI"

#### Scenario: Tape reaches Models [manual]
- GIVEN `docs/assets/tapes/ai-menu.tape`
- WHEN it is run
- THEN it reaches the Models tab (one Tab or `2`) and the recording shows the new bar

#### Scenario: README and CHANGELOG [manual]
- GIVEN the README menu section and the CHANGELOG
- WHEN they are read
- THEN the README lists the five tabs, `Tab`/`Shift+Tab` and `1`-`5`, and the CHANGELOG notes the split

#### Scenario: Narrow terminals in Kitty [manual, Kitty]
- GIVEN the menu in Kitty resized to 48 and 49 columns with the AI section
- WHEN the bar is read and Tab is pressed
- THEN no panic occurs and the bar shows only the active tab, then the whole bar, as specified

### Requirement: Jump keys
At the top level, an unmodified `1` to `5` MUST jump to a tab by its FIXED position in the tab table: `1` Settings, `2` Models, `3` Tools, `4` Connectors, `5` Chat. The digits do not renumber when Connectors is hidden. Press and Repeat act alike. A jump to the visible tab that is not active MUST activate it with the same cursor memory as Tab. A jump to the active tab MUST change nothing and show the footer notice `Already on <Tab>` (no key is swallowed silently). The `Already on` notice and the other notices follow the rules below. A jump to a tab that does not exist MUST show a visible one-line notice that names the reason and MUST change nothing else: `4` while Connectors has no rows (the notice says there are no connectors yet), and `2` to `5` without the AI section (the notice reads `No AI section`). The notice is drawn in the footer with the precedence of an error, is cleared by the next key press (which then acts normally), blocks neither `s` nor Esc, and writes nothing to any PTY. `0` and `6` to `9` (unmodified, top level) show the notice `Tabs are 1-5` and change nothing else. Digits with Ctrl, Alt or Shift stay swallowed and are not jumps; this is a known exception to the notice rule, the same as every other modified key today. No unmodified digit is ever swallowed without an effect or a notice at the top level.

#### Scenario: Jump to each tab [auto]
- GIVEN the AI section attached and a test table with Connectors visible
- WHEN `1`, `2`, `3`, `4`, `5` are pressed in turn
- THEN the active tab is Settings, Models, Tools, Connectors, Chat

#### Scenario: Digits keep their positions [auto]
- GIVEN the AI section attached and Connectors hidden
- WHEN `5` is pressed
- THEN the active tab is `Chat`, not `Tools` or a fourth tab

#### Scenario: Hidden Connectors shows a notice [auto, TestBackend + insta]
- GIVEN Connectors hidden and the active tab `Models` with `code` selected
- WHEN `4` is pressed
- THEN the active tab is still `Models` with `code` selected, the footer shows a notice about the missing connectors instead of the hint, and the next key press clears it

#### Scenario: No AI section shows a notice [auto]
- GIVEN the menu without the AI section
- WHEN `2`, then `3`, then `4`, then `5` are pressed
- THEN each shows the notice `No AI section`, the tab does not change and nothing is written

#### Scenario: Active tab and Settings [auto]
- GIVEN the active tab `Models`
- WHEN `2` is pressed, then `1`
- THEN the first changes nothing and shows `Already on Models`, and the second activates `Settings`

#### Scenario: The notice does not block [auto]
- GIVEN the notice from `4` is showing and a draft is changed
- WHEN `s` is pressed, and in another run Esc
- THEN the first requests a save and the second reverts and closes

#### Scenario: Jump keys in Kitty [manual, Kitty]
- GIVEN the menu in Kitty with the AI section
- WHEN Tab, Shift+Tab and `1` to `5` are pressed
- THEN the tabs switch as specified, `4` shows the notice, and nvim receives no keys
