# Pane Layout Specification

## Purpose

Pure layout tree for the panes of one tab: split, close, geometric focus, resize, zoom and separators. Tags: [auto] = `cargo test` (pure layout, no I/O); [manual] = real Kitty.

## Requirements

### Requirement: Pane identity and tree
A tab's layout MUST be a tree of panes with 1-cell separators between siblings. Every pane MUST have a `PaneId` that is never reused during the process lifetime, even after the pane is closed. Sibling sizes MUST derive from integer weights, and the computed rects MUST tile the available area exactly (no gaps, no overlap, separators included).

#### Scenario: Ids are never reused [auto]
- GIVEN a layout with panes 1 and 2
- WHEN pane 2 is closed and a new pane is split
- THEN the new pane has an id different from 1 and 2

#### Scenario: Rects tile the area [auto]
- GIVEN any sequence of splits on an area of W x H
- WHEN rects are computed
- THEN pane rects plus separators cover the area exactly, with no overlap

### Requirement: Split
`w v` MUST split the focused pane right (side by side) and `w h` MUST split it below (stacked). The new pane MUST receive focus, and the original pane MUST keep its content. A split MUST be refused (no state change, no spawn) when either resulting pane would be smaller than the minimum pane size (2 rows x 10 columns; see design). Focus MUST move to the new pane only when the split is applied.

#### Scenario: Split right [auto]
- GIVEN one pane
- WHEN split right is requested
- THEN two panes exist side by side, separated by a 1-cell separator, and the new pane is focused

#### Scenario: Split below [auto]
- GIVEN one pane
- WHEN split below is requested
- THEN two panes exist stacked, separated by a 1-cell separator, and the new pane is focused

#### Scenario: Refused below minimum size [auto]
- GIVEN a focused pane already at the minimum size in the split direction
- WHEN a split in that direction is requested
- THEN the layout is unchanged and no pane is spawned

#### Scenario: Tiny terminal never panics [auto]
- GIVEN a terminal smaller than one minimum-size pane
- WHEN any layout operation runs
- THEN no panic or underflow occurs, and every PTY size derived from the layout is at least 1x1

#### Scenario: Minimum split sizes [auto]
- GIVEN a pane 21 columns wide (or 5 rows tall)
- WHEN a split right (or below) is requested
- THEN the split is applied and each resulting pane is at least 10 columns wide (or 2 rows tall); with one cell less the split is refused

### Requirement: Geometric focus
`h/j/k/l` after the prefix MUST move focus to the neighbouring pane in that direction, chosen by geometry (adjacent rect overlapping the focused pane's edge), not by tree order. When there is no neighbour in that direction the key MUST be a no-op.

#### Scenario: Move focus to neighbour [auto]
- GIVEN two panes side by side with the left one focused
- WHEN focus right is requested
- THEN the right pane is focused

#### Scenario: Edge is a no-op [auto]
- GIVEN the left pane of two side-by-side panes is focused
- WHEN focus left is requested
- THEN focus is unchanged

#### Scenario: Geometry beats tree order [auto]
- GIVEN a left pane and a right column split into top and bottom, with the left pane focused
- WHEN focus right is requested
- THEN the right pane whose edge overlaps the focused pane is chosen, deterministically

### Requirement: Close and collapse
Closing a pane MUST remove it and let its sibling expand to the freed space. When the closed pane was focused, focus MUST move to the pane that now covers the closed pane's top-left cell. Closing a pane that is not focused MUST keep focus unchanged. Closing the last pane of a tab MUST close the tab.

#### Scenario: Sibling expands [auto]
- GIVEN two panes side by side with the right one focused
- WHEN the right pane closes
- THEN the left pane fills the whole area and is focused

#### Scenario: Focus goes to the pane covering the old top-left [auto]
- GIVEN a left pane A and a right column split into B (top) and C (bottom), with B focused
- WHEN B closes
- THEN C expands over the right column and is focused

#### Scenario: Closing an unfocused pane keeps focus [auto]
- GIVEN two panes with the left focused
- WHEN the right pane closes (for example its shell exits)
- THEN the left pane fills the area and remains focused

#### Scenario: Last pane closes the tab [auto]
- GIVEN a tab with one pane
- WHEN that pane closes
- THEN the tab is closed

### Requirement: Resize
In RESIZE, `h/j/k/l` MUST move, by 1 cell per key press, the border between the focused pane and its neighbour along that axis (the separator of the nearest enclosing split on that axis) in the key's direction, so the focused pane grows or shrinks accordingly. The move MUST be refused (sizes unchanged) when it would take any pane below the minimum size, and MUST be a no-op when no enclosing split exists on that axis (for example a single pane, or `h` when all panes are stacked). With a single pane every resize key is a no-op.

#### Scenario: Border moves one cell [auto]
- GIVEN two side-by-side panes, left focused, 40 and 39 cells wide
- WHEN resize right is pressed once
- THEN the left pane is 41 cells wide and the right pane 38

#### Scenario: Clamped at minimum [auto]
- GIVEN the right sibling already at the minimum size
- WHEN resize right is pressed
- THEN sizes are unchanged

#### Scenario: Focused pane shrinks [auto]
- GIVEN two side-by-side panes, left focused, 40 and 39 cells wide
- WHEN resize left is pressed once
- THEN the left pane is 39 cells wide and the right pane 40

#### Scenario: No border on that axis [auto]
- GIVEN two stacked panes
- WHEN resize left is pressed
- THEN sizes are unchanged

#### Scenario: Single pane [auto]
- GIVEN one pane
- WHEN any resize key is pressed
- THEN sizes are unchanged and no ResizePty is emitted

### Requirement: Zoom
`w z` MUST toggle zoom of the focused pane: while zoomed, that pane MUST fill the whole pane area (no separators drawn) and other panes keep running. A split, a focus change, entering RESIZE or any pane removal while zoomed MUST unzoom first, then apply. While the active tab is zoomed the statusline MUST show `[Z]` (see statusline). Toggling again MUST restore the previous layout exactly.

#### Scenario: Zoom and restore [auto]
- GIVEN two panes with the left focused
- WHEN zoom is toggled twice
- THEN the layout after the second toggle equals the layout before the first

#### Scenario: Split while zoomed [auto]
- GIVEN a zoomed pane
- WHEN a split is requested
- THEN the layout is unzoomed first and the split is then applied

#### Scenario: Focus change while zoomed [auto]
- GIVEN a zoomed pane in a two-pane tab
- WHEN focus right is requested
- THEN the layout is unzoomed and focus moves to the neighbour

#### Scenario: RESIZE while zoomed [auto]
- GIVEN a zoomed pane in a two-pane tab
- WHEN `w r` is pressed
- THEN the layout is unzoomed and mode is RESIZE

### Requirement: Separators
Separators MUST be 1 cell wide. The separator cells adjacent to the focused pane MUST be drawn with the accent of the current input mode (the same color as the mode block: TERMINAL, PREFIX, COPY, RESIZE, confirmation); all other separator cells MUST use the muted text color.

#### Scenario: Focused separator uses the mode accent [auto, TestBackend + insta]
- GIVEN two panes with the left focused in TERMINAL
- WHEN rendered
- THEN the separator between them uses the TERMINAL mode color

#### Scenario: Accent follows the input mode [auto, TestBackend]
- GIVEN two panes in RESIZE
- WHEN rendered
- THEN the separator next to the focused pane uses the RESIZE color

#### Scenario: Focus change recolors [auto, TestBackend]
- GIVEN three panes in a row with the left focused
- WHEN focus moves to the right pane
- THEN only the separator next to the right pane uses the mode accent color

### Requirement: Visible multiplexer milestone
The layout MUST behave correctly with real programs.

#### Scenario: nvim in split panes [manual, Kitty]
- GIVEN the app in Kitty
- WHEN `w v` is pressed and nvim runs in both panes
- THEN each nvim renders within its pane and typing goes only to the focused one

#### Scenario: Focus, close, resize, zoom [manual, Kitty]
- GIVEN three panes
- WHEN the user moves focus with prefix `h/j/k/l`, resizes with `w r`, zooms with `w z` and closes with `w q` then `y`
- THEN each action takes effect visibly, and the accent separator follows focus

#### Scenario: `exit` collapses its pane [manual, Kitty]
- GIVEN two panes
- WHEN `exit` is typed in one shell
- THEN that pane disappears, the sibling expands and, if the exiting pane was focused, focus moves to it
