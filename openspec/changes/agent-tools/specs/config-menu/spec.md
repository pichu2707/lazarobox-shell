# Delta for Config Menu

Tags as in the main spec. The new `Tools` row is appended after `Notify`, and the `Approval` row (design D18) after `Tools`. Every other requirement of the main spec that enumerates the row order or the popup size (Sections and items are data, Navigation, Tiny terminals, AI task list, Picker viewport) MUST be read with the extension in "Tools row"; sdd-archive MUST update those lists and numbers when merging.

## MODIFIED Requirements

### Requirement: Notify row
The AI section MUST contain a static row `Notify` after `Memory` and before `Tools` (order: Set all tasks, general, code, chat, review, summarize, Profile, Context, Memory, Notify, Tools, Approval). It is found by `ItemKind::Notify`, never by position, and the popup body height (`AI_BODY`) grows with it automatically. It reads `off` or `on` from the AI draft; `h`, `l`, arrows, Space and Enter toggle the draft (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect; the footer is the settings hint. Only a changed value is an edit (`ai.notify`, `true`/`false`), placed after the memory edits.
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
- THEN the only difference against the pre-Notify snapshots is the extra `Notify` row and the `Tools` and `Approval` rows of this change

## ADDED Requirements

### Requirement: Tools row
The AI section MUST hold a static row `Tools` after `Notify`, followed only by `Approval` (see Approval row) (key path `ai.tools.enabled`, values `off`, `on`; it reads `on` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft (an Enter repeat is ignored, like on `Notify`); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to a stream, a pending approval or a running tool, and it applies from the next turn after a successful save. Esc at the top level restores the draft; `s` saves `ai.tools.enabled` through config-persistence only when it differs. The row order becomes `... Memory, Notify, Tools, Approval`; Navigation wraps from `Approval` to `Statusline position`; `AI_BODY` grows by one row (it is derived from the descriptor tables; the resulting size is pinned in Approval row); the descriptor integrity rules extend to the new row (`Tools` has key path `ai.tools.enabled`, values `off`, `on`, no duplicate key path).

#### Scenario: Navigation reaches Tools and wraps [auto]
- GIVEN `Memory` selected
- WHEN `j` is pressed three times
- THEN `Tools`, then `Approval`, then `Statusline position` is selected; and from the first row `k` selects `Approval`

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

### Requirement: Approval row
The AI section MUST end with a static row `Approval` after `Tools` (key path `ai.tools.approval`, values `ask`, `auto`; it reads `ask` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards only. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs. The row is shown whatever the `Tools` draft. With it the task list body is 19 rows (two section titles, six Settings rows, eleven AI rows), the full popup needs 24 columns by `AI_BODY + 3` = 22 rows and is 23 rows tall, so it still fits 80x24; the descriptor integrity rules extend to the new row.

#### Scenario: Cycle [auto]
- GIVEN `Approval` selected with the draft `ask`
- WHEN `l` is pressed, then `l` again
- THEN the draft is `auto`, then `ask`, no save is requested and no file is touched

#### Scenario: No effect on open chats [auto]
- GIVEN an open chat in `ask` with a pending approval and the menu open on `Approval`
- WHEN the draft is cycled to `auto` and saved
- THEN the open chat is still `ask` with its approval pending, no WritePty is emitted, and a chat pane opened afterwards is `auto`

#### Scenario: Esc reverts [auto]
- GIVEN the draft changed to `auto`
- WHEN Esc is pressed on the task list
- THEN the mode default is `ask` and no file is read or written

#### Scenario: Footer [auto, TestBackend + insta]
- GIVEN `Approval` selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and the value reads `ask`

#### Scenario: Popup size and fallback [auto, TestBackend + insta]
- GIVEN terminals of 80x24, of 24x22, and of 23x22 and 24x21
- WHEN the task list is rendered
- THEN the first two show the full popup (the 24x22 one without the blank row) and the last two show the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Approval` exists with key path `ai.tools.approval` and values `ask`, `auto`, and no key path is duplicated

#### Scenario: Approval row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Approval` is set to `auto` and saved, and a new chat is opened
- THEN the new chat shows `AUTO`, the chat opened before does not, and the file holds `approval = "auto"` under `[ai.tools]`
