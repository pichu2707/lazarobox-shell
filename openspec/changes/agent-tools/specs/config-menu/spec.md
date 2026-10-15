# Delta for Config Menu

Tags as in the main spec. The new `Tools` row is appended after `Notify`. Every other requirement of the main spec that enumerates the row order or the popup size (Sections and items are data, Navigation, Tiny terminals, AI task list, Picker viewport) MUST be read with the extension in "Tools row"; sdd-archive MUST update those lists and numbers when merging.

## MODIFIED Requirements

### Requirement: Notify row
The AI section MUST contain a static row `Notify` after `Memory` and before `Tools` (order: Set all tasks, general, code, chat, review, summarize, Profile, Context, Memory, Notify, Tools). It is found by `ItemKind::Notify`, never by position, and the popup body height (`AI_BODY`) grows with it automatically. It reads `off` or `on` from the AI draft; `h`, `l`, arrows, Space and Enter toggle the draft (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect; the footer is the settings hint. Only a changed value is an edit (`ai.notify`, `true`/`false`), placed after the memory edits.
(Previously: the AI section MUST end with the `Notify` row)

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section
- WHEN navigating down from Memory
- THEN the selection reaches Notify and then Tools; going up from Notify returns to Memory

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
- THEN the only difference against the pre-Notify snapshots is the extra `Notify` row and the `Tools` row of this change

## ADDED Requirements

### Requirement: Tools row
The AI section MUST end with a static row `Tools` after `Notify` (key path `ai.tools.enabled`, values `off`, `on`; it reads `on` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft (an Enter repeat is ignored, like on `Notify`); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to a stream, a pending approval or a running tool, and it applies from the next turn after a successful save. Esc at the top level restores the draft; `s` saves `ai.tools.enabled` through config-persistence only when it differs. The row order becomes `... Memory, Notify, Tools`; Navigation wraps from `Tools` to `Statusline position`; `AI_BODY` grows by one row so the popup is 62x20 and the full-popup minimum is 24 columns by `AI_BODY + 3` rows (19 today); the descriptor integrity rules extend to the new row (`Tools` has key path `ai.tools.enabled`, values `off`, `on`, no duplicate key path).

#### Scenario: Navigation reaches Tools and wraps [auto]
- GIVEN `Notify` selected
- WHEN `j` is pressed twice
- THEN `Tools` is selected, then `Statusline position`; and from the first row `k` selects `Tools`

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
- GIVEN a terminal at exactly the new minimum, and one column or one row less
- WHEN the task list and the picker are rendered
- THEN the first shows the full popup (clipped, no panic) and the second shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Tools` exists with key path `ai.tools.enabled` and values `off`, `on`, and no key path is duplicated

#### Scenario: Tools row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Tools` is turned off and saved, and a chat turn is sent
- THEN no tool row, no run hint and no confirm ever appears, and turning it on again restores them
