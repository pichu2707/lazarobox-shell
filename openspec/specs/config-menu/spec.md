# Config Menu Specification

## Purpose

A general in-app menu opened with `Ctrl+Space m`. It has two sections, "Settings" (rows that map to `config.toml` keys, with static values) and "AI" (the default engine and model, with choices built from detection, plus a read-only status row for the selected engine, see ai-engine-detection). Changes preview live; Enter saves (see config-persistence), Esc reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## Requirements

### Requirement: Open and close
The menu MUST open only from TERMINAL through the prefix: `Ctrl+Space` then `m`. Opening MUST NOT write to any PTY. The menu MUST close on Enter (after a successful save) and on Esc (after reverting). While open the mode is MENU (see modal-input). Opening captures the current values of the menu-owned settings as the "original" values and as the initial draft.

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

### Requirement: Sections and items are data
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path, and either an ordered list of allowed values (static item) or a dynamic choice source (dynamic item, whose choices are built at runtime, see AI section). The sections are "Settings" and "AI", shown in that order. Settings has the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and a disabled row `Mouse: off (coming soon)`. AI has the dynamic rows `Engine` (`ai.engine`) and `Model` (`ai.model`) and the display-only row `Status` (no key path, shows the draft engine's status).

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, every setting item a non-empty key path, every static item at least two allowed values, every dynamic item a choice source, every display-only item a value source and no key path, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each setting showing its current value, and the Mouse row is drawn muted; the `AI` section with `Engine`, `Model` and `Status` follows them

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of all sections, in display order (`Statusline position`, `Tab bar position`, `Engine`, `Model`). Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. The disabled `Mouse` row and the display-only AI `Status` row MUST be skipped: they are drawn but never selected, so no key can interact with them. Repeat events MUST be handled like Press.

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap [auto]
- GIVEN the last selectable row selected
- WHEN `j` (or Down) is pressed
- THEN the first selectable row is selected; and from the first row `k` (or Up) selects the last selectable row

#### Scenario: Disabled row is skipped [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN the selection goes to Engine, never to the Mouse row; and from Engine `k` goes back to Tab bar position

#### Scenario: Status row is skipped and navigation wraps [auto]
- GIVEN the Engine row selected
- WHEN `j` is pressed twice, then `k` twice
- THEN the Model row is selected, then Statusline position (never the Status row); and `k` from Statusline position selects Model, then Engine

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Value cycling
On a setting row, `h`, `l`, Left, Right and Space MUST change the draft value by cycling through the allowed values with wrap (with two values, any of these keys toggles). `l`/Right/Space advance to the next value and `h`/Left to the previous. Repeat events MUST be handled like Press. These keys MUST have no effect when there is no selectable row.

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

### Requirement: Live preview and revert
Every change of a draft value MUST be applied immediately to the running layout through the normal relayout. `ResizePty` MUST be emitted only for panes whose size actually changed, and no preview step may write to any PTY. The tab bar MUST be shown while the menu is open (see tabs). Esc MUST restore the original values captured at open, relayout again (with `ResizePty` only for panes whose size changed back), and close the menu; the file MUST NOT be read or written. External edits to `config.toml` made while the menu is open MUST NOT be reloaded: Esc restores the values the app had at open.

#### Scenario: Statusline moves live [auto]
- GIVEN a 80x25 terminal, statusline `bottom`, tab bar `top`, two panes
- WHEN the statusline value is cycled to `top`
- THEN the statusline is drawn at row 0 or on the top edge per the bar placement rules, the body keeps the same height, and ResizePty is emitted only for panes whose rect size changed (none when sizes are equal)

#### Scenario: Preview emits ResizePty only for changed panes [auto]
- GIVEN two tabs of panes and a preview that changes the body width or height of only some panes
- WHEN the value changes
- THEN ResizePty is emitted exactly for the panes whose size changed

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with statusline `bottom` and tab bar `top`, both changed in the draft
- WHEN Esc is pressed
- THEN statusline and tab bar are back at `bottom` and `top`, the layout is recomputed, and mode is TERMINAL

#### Scenario: Esc reverts the one-tab bar [auto]
- GIVEN one tab (no tab bar) and the menu open (bar forced visible)
- WHEN Esc is pressed
- THEN the tab bar is hidden, the body returns to the pre-menu size, and ResizePty is emitted for panes whose size changed back

#### Scenario: External edit is not reloaded [auto]
- GIVEN the menu open and `config.toml` edited externally to a different position
- WHEN Esc is pressed
- THEN the positions are those the app had when the menu opened

### Requirement: Input is swallowed
In MENU every key not listed above MUST be swallowed with no effect. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL). Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter saves); uppercase letters such as `J` are unlisted and swallowed. `r` is listed only for the refresh behavior of the AI rows (see Refresh key).

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x`, Tab or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

#### Scenario: Paste ignored [auto]
- GIVEN MENU
- WHEN a paste event arrives
- THEN no WritePty is emitted and the menu is unchanged

### Requirement: Footer
The menu footer MUST show the hint `j/k move · h/l change · Enter save · Esc cancel` while there is no error and the selection is on a Settings row, and the hint `j/k move · h/l change · r refresh · Enter save · Esc cancel` while the selection is on an AI row. When an error exists it MUST replace the hint with the error message. A pending error MUST be cleared by the next key press (Press, not Repeat or Release), which then acts normally; an unlisted key clears it and has no other effect.

#### Scenario: Footer hint [auto, TestBackend + insta]
- GIVEN MENU just opened
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · Enter save · Esc cancel`

#### Scenario: Footer hint on AI row [auto, TestBackend + insta]
- GIVEN the Engine row selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · r refresh · Enter save · Esc cancel`

#### Scenario: Error replaces hint [auto]
- GIVEN a save failure message
- WHEN rendered
- THEN the footer shows the message instead of the hint, and the next key press restores the hint

### Requirement: Tiny terminals
The menu MUST never panic at any terminal size down to 0x0. Below a minimum size (decided in design) the menu MUST draw only a minimal message instead of the full popup; the key handling MUST be unchanged, so Esc still reverts and closes, and Enter still saves. Content that does not fit MUST be clipped.

#### Scenario: No panic at any size [auto]
- GIVEN the menu open
- WHEN rendered at sizes from 0x0 up through small widths and heights
- THEN nothing panics

#### Scenario: Minimal message and Esc works [auto]
- GIVEN a terminal smaller than the menu minimum and the menu open
- WHEN rendered, then Esc is pressed
- THEN a minimal message is shown instead of the popup and Esc reverts and closes

### Requirement: Events while open
While the menu is open, PTY output MUST keep being parsed and drawn, cwd polls and OSC 7 MUST keep updating, and tabs and panes in the background MUST keep running. The menu MUST stay open when a pane exits, a spawn fails, or the host terminal resizes; geometry MUST be recomputed with the forced tab bar and the previewed positions. If the exit of a pane would quit the app (last pane of the last tab), Quit MUST still be emitted. A focus or active-tab change caused by a pane exit MUST NOT close the menu. A spawn failure notice MUST NOT replace the menu footer.

#### Scenario: Output keeps flowing [auto]
- GIVEN the menu open
- WHEN a pane emits output
- THEN the pane's screen is updated and the menu stays open

#### Scenario: Pane exit keeps the menu [auto]
- GIVEN the menu open and two panes
- WHEN the other pane's shell exits
- THEN mode is still MENU, the layout is recomputed, and the draft is unchanged

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
- WHEN `Ctrl+Space m` is pressed, the positions are changed with `l`, then Esc, then the menu is reopened and Enter is pressed on a changed value
- THEN the bars move live, Esc restores them, Enter closes the menu, nvim receives no keys, and a restart shows the saved positions

### Requirement: AI section layout
Below the Settings rows the popup MUST show the section title `AI`, then exactly three rows, in this order: `Engine` (`ai.engine`, selectable), `Model` (`ai.model`, selectable) and `Status` (display only). The `Status` row shows the status text of the draft engine (see ai-engine-detection): for a catalog engine its status text (`detecting…`, `available · 2 models`, `available · 0.5.0`, `available`, `not running`, `not installed`, `unknown`); for a saved id that is not in the catalog, `not detected`; with no engine, `-`. The `Status` row is drawn muted, like the disabled `Mouse` row, and is never selected. The AI section always has these three rows, so a detection result never changes the number of rows or moves the selection. The popup is 62 columns wide when it includes the AI section and 51 columns wide without it, so the Settings-only popup is unchanged.

#### Scenario: AI rows rendered [auto, TestBackend + insta]
- GIVEN ollama is available with 2 models and the draft is `ollama` / `llama3:8b`
- WHEN the menu is rendered on a normal terminal
- THEN `AI` and exactly three rows are visible: `Engine` showing `ollama`, `Model` showing `llama3:8b` and `Status` showing `available · 2 models`; the `Status` row is muted and no per-engine status list is drawn

#### Scenario: Status follows the draft engine [auto]
- GIVEN lmstudio is `not running` and is the saved engine, and claude is `available · 2.1.0`
- WHEN the menu is rendered, then the Engine is cycled to `none` and then to `claude`
- THEN the Status row reads `not running`, then `-`, then `available · 2.1.0`

### Requirement: Engine and Model choices
The `Engine` choices MUST be, in this order: `none` (no engine); then the ids of the engines whose status is `available`, in catalog order; then, if not already offered, the engine id saved at open and the current draft engine id (an id that is not available, or not in the catalog). Catalog engines that are not available MUST NOT be offered as choices otherwise. The `Engine` row shows `none` or the id; an id that is not available is shown with the suffix `(checking)` while its status is `detecting…` (no result yet) and `(not detected)` otherwise.

The `Model` choices MUST be: for an available server, its models in response order; for an available CLI, the single choice `default`; otherwise none. A non-empty draft model that is not among the choices (for example a saved model no longer listed, or a model of an engine that is not available) MUST stay as the draft, be shown as `<name> · not found`, and be offered as the last choice, so it is never erased silently. A row with no choices and an empty draft shows `none` and the keys have no effect on it.

The same keys as value cycling (`h`, `l`, Left, Right, Space) apply, with wrap, over the current choices. Changing the engine MUST reset the draft model to the first model choice of the new engine, or to empty when there is none. The draft MUST NOT change because of detection alone. An empty engine or model is saved as `""` (see config-persistence).

#### Scenario: Cycle engines [auto]
- GIVEN ollama, claude and codex are available, lmstudio is not running, the draft engine is `ollama`, and Engine is selected
- WHEN `l` is pressed three times, then once more
- THEN the draft engine is `claude`, then `codex`, then `none`, then `ollama` (`lmstudio` is never offered)

#### Scenario: Engine change resets model [auto]
- GIVEN engine `ollama` with model `qwen2.5:7b` (second of two), and claude is available
- WHEN the engine is cycled to `claude`
- THEN the model is `default`; cycling back to `ollama` gives the first model `llama3:8b`

#### Scenario: Engine none clears model [auto]
- GIVEN engine `ollama` with model `llama3:8b`
- WHEN the engine is cycled to `none`
- THEN the draft engine and model are empty and the Model row shows `none`

#### Scenario: Server without models [auto]
- GIVEN ollama is available with `{"models":[]}`
- WHEN ollama is the draft engine
- THEN the Model row shows `none`, the draft model is empty, and `l` on it changes nothing

#### Scenario: Saved model no longer listed [auto]
- GIVEN the saved engine is `ollama` (models `llama3:8b`, `qwen2.5:7b`) and the saved model is `old:7b`
- WHEN the menu is rendered, then `l` is pressed on Model
- THEN the Model row shows `old:7b · not found` with the draft still `old:7b`, and then shows `llama3:8b`
- AND `old:7b` stays the last Model choice while the menu is open (like the saved engine), so `h` on `llama3:8b` shows `old:7b · not found` again; it is offered only while the saved engine is the draft engine
- AND repeated model names from a server are offered once, in first-seen order

#### Scenario: Saved engine not available [auto]
- GIVEN the saved engine is `codex`, its status is `not installed`, and only ollama is available
- WHEN the menu is rendered
- THEN the Engine row shows `codex (not detected)`, the choices are `none`, `ollama`, `codex`, and `l` selects `none`

#### Scenario: Pending detection [auto]
- GIVEN the saved engine is `claude` and no detection result has arrived yet
- WHEN the menu is rendered
- THEN the Engine row shows `claude (checking)`, the Status row shows `detecting…`, and the draft is still `claude`

#### Scenario: No engines at all [auto]
- GIVEN no engine is available and no engine is saved
- WHEN the menu is rendered
- THEN Engine and Model show `none`, and `l` on Engine changes nothing

### Requirement: Detection results update the open menu
Detection results MUST update the open menu in place: the `Status` row and the choices are refreshed, the selected row MUST NOT change, and the draft values MUST NOT change. Results arriving while the menu is closed MUST be applied to the state used the next time it opens. Opening the menu MUST NOT write to any PTY, and a detection result MUST NOT either.

#### Scenario: Result arrives while open [auto]
- GIVEN the menu open with the Model row selected, the draft engine `ollama`, and ollama still `detecting…`
- WHEN ollama's result `available · 2 models` arrives
- THEN the Status row changes from `detecting…` to `available · 2 models`, the Model row stays selected, the draft is unchanged, and no WritePty is emitted

#### Scenario: Engine disappears while open [auto]
- GIVEN the draft engine is `ollama` and a refresh returns ollama `not running`
- WHEN the result arrives
- THEN the draft stays `ollama`, the Engine row shows `ollama (not detected)`, and the Status row shows `not running`

### Requirement: Refresh key
Only when the selection is on the `Engine` or `Model` row, `r` (Press only, not Repeat) MUST request a new detection run (see ai-engine-detection) and have no other effect. On any other row `r` MUST be swallowed with no effect. Uppercase `R` is unlisted and swallowed.

#### Scenario: Refresh on AI row [auto]
- GIVEN the Engine row selected
- WHEN `r` is pressed
- THEN exactly one detection run is requested, the selection and draft are unchanged, and no WritePty is emitted

#### Scenario: Refresh elsewhere or repeated [auto]
- GIVEN the Statusline position row selected, or the Engine row with a Repeat event for `r`
- WHEN `r` arrives
- THEN no detection run is requested

### Requirement: AI draft, revert and save
The AI draft (engine and model) is captured at open with the other menu-owned values. Esc MUST restore the AI draft to the open values and read or write no file. Enter MUST save through config-persistence only the keys whose draft differs; when the whole draft equals the open values, the menu closes without touching the file. AI choices have no live effect on layout and emit no `ResizePty`.

#### Scenario: Esc reverts AI [auto]
- GIVEN the menu opened with engine `ollama`, and the draft changed to `claude`
- WHEN Esc is pressed and the menu is reopened
- THEN the draft engine is `ollama`

#### Scenario: Save closes with AI values [auto]
- GIVEN the draft engine changed to `claude` with model `default`
- WHEN Enter is pressed and the save succeeds
- THEN the menu closes and the current values are `claude` / `default`

#### Scenario: Unchanged AI draft [auto]
- GIVEN no change was made
- WHEN Enter is pressed
- THEN the menu closes and the file is not read or written

#### Scenario: AI menu in Kitty [manual, Kitty]
- GIVEN the app in Kitty on a machine with Ollama running and `claude` installed
- WHEN the menu is opened, Engine is changed to `claude`, `r` is pressed, Enter is pressed, and the app is restarted
- THEN statuses match the machine, the UI never freezes, the file gains an `[ai]` table with `engine = "claude"` and `model = "default"`, and the menu shows `claude` after restart
