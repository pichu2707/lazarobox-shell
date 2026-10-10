# Config Menu Specification

## Purpose

A general in-app menu opened with `Ctrl+Space m`. Its only section for now is "Settings", where each row maps to one `config.toml` key. Changes preview live; Enter saves (see config-persistence), Esc reverts. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## ADDED Requirements

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
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label, its `config.toml` key path and its ordered list of allowed values. The only section is "Settings", with the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and a disabled row `Mouse: off (coming soon)`.

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, and every setting item a non-empty key path and at least two allowed values, with no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each setting showing its current value, and the Mouse row is drawn muted

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of the current section. Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. The disabled row MUST be skipped: it is drawn but never selected, so no key can interact with it. Repeat events MUST be handled like Press.

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
- THEN the selection goes to Statusline position, never to the Mouse row

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
In MENU every key not listed above MUST be swallowed with no effect. The PTY MUST NOT receive any key or paste while the menu is open. Release events MUST be ignored. Ctrl+Space MUST be swallowed (not cancel, not literal NUL). Shift is ignored for listed keys (Shift+Space cycles, Shift+Enter saves); uppercase letters such as `J` are unlisted and swallowed.

#### Scenario: Unknown key [auto]
- GIVEN MENU
- WHEN `x`, Tab or Ctrl+Space is pressed
- THEN no WritePty is emitted and mode stays MENU

#### Scenario: Paste ignored [auto]
- GIVEN MENU
- WHEN a paste event arrives
- THEN no WritePty is emitted and the menu is unchanged

### Requirement: Footer
The menu footer MUST show the hint `j/k move · h/l change · Enter save · Esc cancel` while there is no error. When an error exists it MUST replace the hint with the error message. A pending error MUST be cleared by the next key press (Press, not Repeat or Release), which then acts normally; an unlisted key clears it and has no other effect.

#### Scenario: Footer hint [auto, TestBackend + insta]
- GIVEN MENU just opened
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · Enter save · Esc cancel`

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
