# Delta for Config Menu

Tags: [auto] = `cargo test`; [manual] = real Kitty. The Settings section gains a real, selectable `Mouse` row (it was a disabled "coming soon" row). The Terms sentence "The Settings Mouse row is the only row the cursor skips" no longer holds: the cursor skips no row. Requirements not listed here (open and close, value cycling, input swallowed, footer, events while open, detection results, refresh key, AI draft, task picker, custom model entry, picker viewport, detection cues) are unchanged; the Mouse row cycles like any other Settings row.

Archive-order dependency: the base text of these requirements is the ai-chat delta (`openspec/changes/ai-chat/specs/config-menu/spec.md`, which adds the `Profile` row), and `openspec/changes/engram-memory/specs/config-menu/spec.md` adds further AI rows. This delta only changes the Settings rows, the navigation order around the Mouse row and the preview; the AI rows listed below are those of the latest base and MUST be kept in sync with whatever is archived first (ai-chat, then engram-memory, then pane-scroll).

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The sections are "Settings" and "AI", shown in that order. Settings has the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and `Mouse` (`mouse`: on, off), whose values are written to the file as the booleans `true` and `false`. AI has the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static row `Profile` (`ai.profile`: analyst, programmer).
(Previously: the third Settings row was the disabled row `Mouse: off (coming soon)`)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, every static item a non-empty key path and at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile` has the key path `ai.profile` and the values `analyst`, `programmer`, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with `mouse` at its default
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each showing its current value, and the Mouse row reads `Mouse` with the value `on` and is drawn like the other setting rows (not muted); the `AI` section follows them, and no `Engine`, `Model` or `Status` row exists

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of all sections, in display order (`Statusline position`, `Tab bar position`, `Mouse`, `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`). Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. No row is skipped (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press.
(Previously: the disabled `Mouse` row was drawn but skipped by the cursor)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap [auto]
- GIVEN the last selectable row (`Profile`) selected
- WHEN `j` (or Down) is pressed
- THEN the first selectable row is selected; and from the first row `k` (or Up) selects the last selectable row

#### Scenario: Mouse row is selectable [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN the selection goes to the `Mouse` row, and `j` again goes to `Set all tasks`; from `Set all tasks` `k` goes back to `Mouse`

#### Scenario: Task rows in order [auto]
- GIVEN `Set all tasks` selected
- WHEN `j` is pressed five times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize` in that order, a sixth `j` selects `Profile`, and a seventh wraps to `Statusline position`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: Live preview and revert
Every change of a draft value MUST be applied immediately to the running layout through the normal relayout. `ResizePty` MUST be emitted only for panes whose size actually changed, and no preview step may write to any PTY. The tab bar MUST be shown while the menu is open (see tabs). Cycling the `Mouse` row MUST also apply immediately: it enables or disables host mouse capture with the sequences of mouse-input (one write per actual change, no write when the effective value is unchanged), without touching any PTY and without relayout. Esc MUST restore the original values captured at open (including the capture state if `Mouse` was cycled), relayout again (with `ResizePty` only for panes whose size changed back), and close the menu; the file MUST NOT be read or written. External edits to `config.toml` made while the menu is open MUST NOT be reloaded: Esc restores the values the app had at open.
(Previously: the preview covered only the layout)

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

## ADDED Requirements

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
