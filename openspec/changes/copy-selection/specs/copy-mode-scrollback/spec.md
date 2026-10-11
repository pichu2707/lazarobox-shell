# Delta for Copy Mode Scrollback (copy-selection)

All scenarios [auto] unless tagged. Note for archive: the Purpose line of the main spec ("Selection/yank is out of scope.") MUST be edited by hand at archive to read "Vim-style navigation of ~10k lines of scrollback with character/line selection and yank (see copy-selection)."; Purpose is not a requirement and cannot be carried by a MODIFIED block.

## MODIFIED Requirements

### Requirement: Navigation
COPY has a cursor (absolute row, column). On entry it MUST be at the position of the pane's live cursor, clamped into the pane. `h`/`l` MUST move it one column left/right, `0` to the first column of its row and `$` to the last non-blank column of its row (column 0 when the row is blank). `j`/`k` MUST move it one row down/up; the viewport MUST scroll only when the cursor would cross the top or bottom edge of the viewport. Ctrl+d/Ctrl+u MUST move the cursor and the viewport half a page down/up; `gg` MUST move to the top of scrollback; `G` to the bottom (live screen). Movement MUST clamp at all bounds.
(Previously: `j`/`k` moved the viewport one line; there was no cursor)

#### Scenario: Line motion
- GIVEN 100 lines of scrollback, viewport at bottom, cursor on the top visible row
- WHEN `k` is pressed
- THEN the viewport is 1 line above the bottom and the cursor stays on the top visible row; `j` returns it

#### Scenario: Cursor moves inside the viewport
- GIVEN the cursor in the middle of the viewport
- WHEN `k` is pressed
- THEN the cursor is one row higher and the viewport offset is unchanged

#### Scenario: Half page
- GIVEN a 24-row pane
- WHEN Ctrl+u is pressed
- THEN the viewport moves up 12 lines (half pane height) and the cursor moves up 12 rows; Ctrl+d moves both down 12

#### Scenario: Top and bottom
- GIVEN mid scrollback
- WHEN `g` `g` then `G` are pressed
- THEN the viewport and cursor are at the top, then at the bottom

#### Scenario: Clamping
- GIVEN the cursor on the top row of the scrollback
- WHEN `k` or Ctrl+u is pressed
- THEN the cursor and offset are unchanged (never beyond history); likewise `j`/Ctrl+d at the bottom

#### Scenario: Lone g
- GIVEN `g` pending
- WHEN any non-`g` key is pressed
- THEN the pending `g` is discarded

#### Scenario: Cursor starts at the live cursor
- GIVEN the child's cursor at row 5, col 3
- WHEN COPY is entered
- THEN the COPY cursor is at row 5, col 3

#### Scenario: Horizontal motion
- GIVEN the cursor at col 3 of an 80-column pane whose row reads `echo hi` (last non-blank at col 6)
- WHEN `h`, `0`, `h`, `$`, `l` are pressed
- THEN the cursor is at col 2, 0, 0, 6, 7

#### Scenario: End of a blank row
- GIVEN the cursor at col 4 on a blank row
- WHEN `$` is pressed
- THEN the cursor is at col 0

### Requirement: Cursor in COPY
While in COPY the effective outer cursor MUST be a steady block placed on the COPY cursor's cell, regardless of the shape requested by the child. On returning to TERMINAL the shape last requested by the child MUST be restored. (Chosen behavior: the child's cursor is not shown in COPY because the viewport may be scrolled away from it.)
(Previously: a steady block with no stated position)

#### Scenario: Block in COPY [auto]
- GIVEN the child requested Bar steady (`\e[6 q`)
- WHEN the mode becomes COPY
- THEN the effective cursor shape is Block steady

#### Scenario: Restore on exit [auto]
- GIVEN COPY entered with the child's last request being Bar steady
- WHEN COPY exits
- THEN the effective cursor shape is Bar steady again

#### Scenario: Block follows the COPY cursor [auto]
- GIVEN COPY with the cursor at pane cell (row 2, col 4)
- WHEN rendered
- THEN the host cursor is at that cell of the pane

### Requirement: Exit COPY
`q` or `i` MUST return to TERMINAL with the viewport at the bottom, dropping any selection. Esc with a selection MUST clear it and stay in COPY; Esc with no selection MUST return to TERMINAL with the viewport at the bottom. `y` with a selection also exits (see copy-selection).
(Previously: `q`, Esc or `i` always returned to TERMINAL)

#### Scenario: Exit
- GIVEN COPY scrolled up with no selection
- WHEN `q` (or Esc, or `i`) is pressed
- THEN mode is TERMINAL and the live screen is shown

#### Scenario: Esc with selection
- GIVEN COPY with a selection
- WHEN Esc is pressed
- THEN the selection is cleared and mode is still COPY

#### Scenario: `q` with selection
- GIVEN COPY with a selection
- WHEN `q` is pressed
- THEN mode is TERMINAL and the selection is dropped

#### Scenario: Real session [manual]
- GIVEN a long `ls -R` output
- THEN j/k/Ctrl+u/Ctrl+d/gg/G navigate it and exit lands at the bottom
