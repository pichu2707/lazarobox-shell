# Delta for Config Menu

Tags: [auto] = `cargo test`; [manual] = real Kitty. The AI section gains a `Memory` row after `Profile`. Requirements not listed here (open and close, value cycling, live preview, input swallowed, footer, events while open, detection results, refresh key, AI draft, task picker, custom model entry, picker viewport, detection cues, AI task list, Profile row) are unchanged, except that the popup MUST also show the `Memory` row without clipping and the fallback threshold follows the popup size (exact dimensions are a design decision).

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The sections are "Settings" and "AI", shown in that order. Settings has the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and a disabled row `Mouse: off (coming soon)`. AI has the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static rows `Profile` (`ai.profile`: analyst, programmer) and `Memory` (`ai.memory.enabled`: off, on).
(Previously: AI ended with the `Profile` row; there was no `Memory` row)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, every static item a non-empty key path and at least two allowed values, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile` has the key path `ai.profile` and the values `analyst`, `programmer`, `Memory` has the key path `ai.memory.enabled` and the values `off`, `on`, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each setting showing its current value, and the Mouse row is drawn muted; the `AI` section with `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile` and `Memory` follows them, and no `Engine`, `Model` or `Status` row exists

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of all sections, in display order (`Statusline position`, `Tab bar position`, `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Memory`). Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. The disabled `Mouse` row MUST be skipped: it is drawn but never selected, so no key can interact with it (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press.
(Previously: the last selectable row was `Profile`)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap [auto]
- GIVEN the last selectable row (`Memory`) selected
- WHEN `j` (or Down) is pressed
- THEN the first selectable row is selected; and from the first row `k` (or Up) selects the last selectable row

#### Scenario: Disabled row is skipped [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN the selection goes to `Set all tasks`, never to the Mouse row; and from `Set all tasks` `k` goes back to Tab bar position

#### Scenario: Task rows in order [auto]
- GIVEN `Set all tasks` selected
- WHEN `j` is pressed five times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize` in that order, a sixth `j` selects `Profile`, a seventh selects `Memory`, and an eighth wraps to `Statusline position`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

## ADDED Requirements

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
