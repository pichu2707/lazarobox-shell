# Config Menu Specification

## Purpose

A general in-app menu opened with `Ctrl+Space m`. It has two sections, "Settings" (rows that map to `config.toml` keys, with static values) and "AI" (a fixed list of tasks, each with an `engine › model` assignment chosen from a picker that shows what is detected and downloaded, see ai-task-models and ai-engine-detection). Changes preview live; `s` saves (see config-persistence), Esc goes back or reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.

Terms: "task list" is the main menu view (Settings rows and AI rows). "Picker" is the view that replaces it inside the same popup: level 1 lists engines, level 2 lists the models of the chosen engine, and the "custom field" is a text entry reached from a CLI engine's `custom…` row. A "disabled row" is drawn muted, the cursor CAN land on it (so long lists scroll it into view), and Enter on it does nothing. The cursor skips no row of the task list.

## Requirements

### Requirement: Open and close
The menu MUST open only from TERMINAL through the prefix: `Ctrl+Space` then `m`, always on the task list with the picker closed. Opening MUST NOT write to any PTY. The menu MUST close on `s` (after a successful save, or at once when nothing changed) and on Esc at the top level of the task list (after reverting). Esc while a picker is open MUST go back exactly one level (custom field to level 2, level 2 to level 1, level 1 to the task list) and MUST NOT close the menu or revert the draft. This Esc rule takes precedence wherever Esc is described. While open the mode is MENU (see modal-input). Opening captures the current values of the menu-owned settings as the "original" values and as the initial draft.

#### Scenario: Open [auto]
- GIVEN TERMINAL
- WHEN `Ctrl+Space` then `m` are pressed
- THEN mode is MENU, the menu is visible with the Settings section, and no WritePty is emitted

#### Scenario: Esc closes [auto]
- GIVEN MENU with no changes
- WHEN Esc is pressed
- THEN mode is TERMINAL, the menu is gone, no file is touched and no WritePty is emitted

#### Scenario: Reopen starts fresh [auto]
- GIVEN the menu was opened, changed, and cancelled with Esc
- WHEN it is opened again
- THEN the selection is on the first row and the draft equals the current (reverted) values

#### Scenario: Esc in the picker goes back one level [auto]
- GIVEN the picker is open at level 2 for `code` after changing the Statusline position in the draft
- WHEN Esc is pressed, then Esc again, then Esc a third time
- THEN level 1 shows, then the task list shows with `code` selected and the draft unchanged, then the menu closes and the Statusline position is reverted

#### Scenario: Reopen after picker is on the task list [auto]
- GIVEN the menu closed with Esc while no picker was open, after a picker was used earlier
- WHEN the menu is reopened
- THEN the task list shows, the picker is closed and the draft equals the current values

### Requirement: Sections and items are data
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The sections are "Settings" and "AI", shown in that order. Settings has the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and `Mouse` (`mouse`: on, off), whose values are written to the file as the booleans `true` and `false`. AI has the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static rows `Profile` (`ai.profile`: analyst, programmer), `Context` (`ai.context`: off, on), `Memory` (`ai.memory.enabled`: off, on) and `Notify` (`ai.notify`: off, on).

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, every static item a non-empty key path and at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile` has the key path `ai.profile` and the values `analyst`, `programmer`, `Context` has the key path `ai.context` and the values `off`, `on`, `Memory` has the key path `ai.memory.enabled` and the values `off`, `on`, `Notify` has the key path `ai.notify` and the values `off`, `on`, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with `mouse` at its default
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each showing its current value, and the Mouse row reads `Mouse` with the value `on` and is drawn like the other setting rows (not muted); the `AI` section with `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory` and `Notify` follows them, and no `Engine`, `Model` or `Status` row exists

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of all sections, in display order (`Statusline position`, `Tab bar position`, `Mouse`, `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Notify`). Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press.

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap [auto]
- GIVEN the last selectable row (`Notify`) selected
- WHEN `j` (or Down) is pressed
- THEN the first selectable row is selected; and from the first row `k` (or Up) selects the last selectable row

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN the selection goes to the `Mouse` row, and `j` again goes to `Set all tasks`; from `Set all tasks` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN `Set all tasks` selected
- WHEN `j` is pressed ten times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize` in that order, a sixth `j` selects `Profile`, a seventh selects `Context`, an eighth selects `Memory`, a ninth selects `Notify`, and a tenth wraps to `Statusline position`

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
In MENU every key not listed above MUST be swallowed with no effect. The listed keys are `j`, `k`, Up, Down, `h`, `l`, Left, Right, Space, Enter, `s`, `r` and Esc, each with the behavior described in this spec, and inside the custom field typed characters, Backspace and Ctrl+U. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL), also inside the custom field. Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter cycles or selects like Enter); uppercase letters such as `J`, `S` and `R` are unlisted and swallowed outside the custom field. `r` is listed only for the refresh behavior (see Refresh key) and `s` only for saving (see config-persistence).

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x`, Tab or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

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
- Custom field: `type a model · Enter ok · Ctrl+U clear · Esc back`

When a detection cue is active (see Detection cues) the hint of an AI view (task row, picker level 1 or 2) is replaced by `<cue> · s save · Esc cancel` on the task list and `<cue> · s save · Esc back` in the picker, so the footer fits the 62-column popup. Precedence, highest first: error, disabled-row text, cue, hint. A pending error MUST be cleared by the next key press (Press, not Repeat or Release), which then acts normally; an unlisted key clears it and has no other effect.

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

#### Scenario: Error replaces hint [auto]
- GIVEN a save failure message
- WHEN rendered
- THEN the footer shows the message instead of the hint, and the next key press restores the hint

### Requirement: Tiny terminals
The menu MUST never panic at any terminal size down to 0x0, in the task list and in every picker view. The popup width is 62 and its height is derived from the descriptor tables (`AI_BODY` body rows plus 4 rows of chrome: two borders, the blank row and the footer); with the 3 Settings rows and the 10 AI rows that is 15 body rows, so 62x19. The full popup (task list and picker alike) needs at least 24 columns by `AI_BODY + 3` rows (18 today: the blank row is the first thing dropped); adding a row grows these numbers. Below that the menu MUST draw only a one-line fallback message instead of the full popup; the key handling MUST be unchanged, so Esc still reverts and closes (or goes back one level when a picker is open), and `s` still saves. Content that does not fit MUST be clipped.

#### Scenario: No panic at any size [auto]
- GIVEN the menu open on the task list, then in each picker view
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
- GIVEN a terminal of 23x14 (and one of 24x13) and the picker open at level 1
- WHEN rendered, then Esc is pressed
- THEN the one-line fallback is shown, then the task list view is the current view

#### Scenario: Smallest full popup [auto, TestBackend + insta]
- GIVEN a terminal of exactly the minimum size (24 columns by `AI_BODY + 3` rows, 24x18 today)
- WHEN the task list and the picker are rendered
- THEN the full popup is drawn (not the fallback), clipped, without panic

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

### Requirement: AI task list
The AI section MUST show, below the Settings rows, the section title `AI`, exactly six task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, and then the static rows `Profile`, `Context`, `Memory` and `Notify`. Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks differ, and the marks `· download required` and `(not detected)` as defined there. The `Profile` row shows the profile id; `Context`, `Memory` and `Notify` show `off` or `on`. The number of AI rows MUST never change because of a detection result. The popup MUST show every row of both sections without clipping on a normal terminal, MUST keep the same size between the task list and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST be adjusted to the popup; the exact dimensions are a design decision.

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, the profile `analyst`
- WHEN the menu is rendered on a normal terminal
- THEN `AI` and the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `Profile` `analyst`, and the `Context`, `Memory` and `Notify` rows with their `off` or `on` value

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)`, the other four read `(default)`, `Profile` reads `programmer` (the default profile) and `Context`, `Memory` and `Notify` read their defaults

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the menu is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the task list and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in both and every row of both sections is visible in the task list

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
The picker body MUST be a title plus a viewport of exactly 10 rows, inside the fixed popup (62 columns wide, `AI_BODY + 4` rows high, 62x19 today; the task list is the tallest view and `VIEW` = 10 is below `AI_BODY`). When the rows do not fit, the list MUST scroll so that the cursor row is always visible (cursor above the window sets the window start to the cursor; cursor below it sets the window so the cursor is the last row), and the popup height MUST NOT change while scrolling. Disabled rows scroll with the rest, and `j`/`k` wrap with the window following.

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
The AI section MUST end with a static row `Notify` after `Memory` (order: Set all tasks, general, code, chat, review, summarize, Profile, Context, Memory, Notify). It is found by `ItemKind::Notify`, never by position, and the popup body height (`AI_BODY`) grows with it automatically. It reads `off` or `on` from the AI draft; `h`, `l`, arrows, Space and Enter toggle the draft (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect; the footer is the settings hint. Only a changed value is an edit (`ai.notify`, `true`/`false`), placed after the memory edits.

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section
- WHEN navigating down from Memory
- THEN the selection reaches Notify and then wraps to the first settings row; going up from there returns to Memory

#### Scenario: Reads on or off [auto]
- GIVEN drafts with notify `false` and `true`
- WHEN the rows are built
- THEN the value is `off` and `on`; without the AI section there is no row

#### Scenario: Toggle and edit [auto]
- GIVEN the selection on Notify
- WHEN any cycle key is pressed
- THEN the draft flips, `MenuCommand::None` is returned and the edit list holds `ai.notify` only while the draft differs from the open value

#### Scenario: Snapshots [auto]
- GIVEN the AI menu snapshots
- WHEN regenerated
- THEN the only difference is the extra `Notify` row
