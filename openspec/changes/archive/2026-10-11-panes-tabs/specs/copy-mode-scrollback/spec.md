# Delta for Copy Mode Scrollback

All scenarios [auto] unless tagged. Existing requirements (Navigation, Isolation, Stable viewport, Alternate screen, Cursor in COPY, Exit COPY) are unchanged and apply to the focused pane.

## ADDED Requirements

### Requirement: Per-pane COPY state
COPY MUST apply to the focused pane only. The viewport offset and pending `g` of each pane MUST be independent. Entering COPY MUST NOT affect other panes: they keep running, parsing output and keeping their live view. Navigation keys MUST act on the focused pane's scrollback.

#### Scenario: Offsets are independent
- GIVEN two panes with scrollback, COPY entered on the left and scrolled up 10 lines
- WHEN the right pane's offset is inspected
- THEN it is 0 (live)

#### Scenario: Other panes keep running
- GIVEN COPY on the left pane
- WHEN the right pane's shell prints output
- THEN the right pane's screen is updated

#### Scenario: Keys act on the focused pane
- GIVEN COPY on the left pane
- WHEN `k` is pressed
- THEN only the left pane's viewport moves

### Requirement: Position indicator
While in COPY and the focused pane has scrollback, the statusline mode block MUST show `COPY ↑{offset}/{total}` right after the mode label (same placement as `[Z]`, which follows it): `offset` is the viewport's rows above the live bottom, `total` the rows of history. At the bottom the offset is an explicit `0`. When the pane has no history (alternate screen, nothing scrolled out) the block MUST show the bare `COPY`. The indicator MUST follow motions and output arriving while scrolled, the path budget MUST shrink by its width, and narrow widths MUST NOT panic.

#### Scenario: Offset and total [auto]
- GIVEN COPY on a pane with 340 rows of history
- WHEN scrolled 12 rows up
- THEN the mode block reads `COPY ↑12/340`

#### Scenario: Bottom of the history [auto]
- GIVEN COPY just entered
- THEN the mode block reads `COPY ↑0/340`

#### Scenario: Output while scrolled [auto]
- GIVEN COPY at `↑10/N`
- WHEN 5 more lines scroll out
- THEN the indicator reads `↑15/N+5`, since the viewport stays pinned

#### Scenario: No history [auto]
- GIVEN COPY on the alternate screen
- THEN the mode block reads `COPY`

#### Scenario: Narrow width [auto]
- GIVEN any width from 0 to 40
- WHEN rendered
- THEN rendering does not panic

### Requirement: Focus change exits COPY
Any change of the focused pane or the active tab while in COPY MUST exit COPY, return the pane being left to the bottom (live screen), and set mode TERMINAL. This includes the focused pane closing. Splitting is not available inside COPY.

#### Scenario: Focus move exits COPY
- GIVEN COPY on the left pane scrolled up
- WHEN focus moves to the right pane (by any means)
- THEN mode is TERMINAL and the left pane's viewport is at the bottom

#### Scenario: Focused pane exits while in COPY
- GIVEN COPY on a pane whose shell exits
- WHEN the pane closes
- THEN mode is TERMINAL, focus is on the pane that now covers the closed pane's top-left cell (see pane-layout), and its viewport is live

#### Scenario: Tab switch exits COPY
- GIVEN COPY with two tabs
- WHEN the active tab changes
- THEN mode is TERMINAL

#### Scenario: Cursor shape restored on focus exit
- GIVEN COPY (steady block) and then the focus changes to a pane that requested Bar steady
- WHEN COPY exits
- THEN the effective cursor shape is Bar steady

## MODIFIED Requirements

(None.)

## REMOVED Requirements

(None.)
