# Copy Mode Scrollback Specification

## Purpose

Vim-style read-only navigation of ~10k lines of scrollback. Selection/yank is out of scope. All scenarios [auto] unless tagged.

## Requirements

### Requirement: Navigation
In COPY, `j`/`k` MUST move the viewport one line down/up; Ctrl+d/Ctrl+u half a page down/up; `gg` to the top of scrollback; `G` to the bottom (live screen). Movement MUST clamp at both bounds.

#### Scenario: Line motion
- GIVEN 100 lines of scrollback, viewport at bottom
- WHEN `k` is pressed
- THEN the viewport is 1 line above the bottom; `j` returns it

#### Scenario: Half page
- GIVEN a 24-row pane
- WHEN Ctrl+u is pressed
- THEN the viewport moves up 12 lines (half pane height); Ctrl+d moves down 12

#### Scenario: Top and bottom
- GIVEN mid scrollback
- WHEN `g` `g` then `G` are pressed
- THEN the viewport is at the top, then at the bottom

#### Scenario: Clamping
- GIVEN viewport at the top
- WHEN `k` or Ctrl+u is pressed
- THEN the offset is unchanged (never negative or beyond history); likewise `j`/Ctrl+d at the bottom

#### Scenario: Lone g
- GIVEN `g` pending
- WHEN any non-`g` key is pressed
- THEN the pending `g` is discarded

### Requirement: Isolation from the PTY
While in COPY no key MUST be written to the PTY.

#### Scenario: Keys swallowed
- GIVEN COPY
- WHEN `j`, `x`, Enter are pressed
- THEN no WritePty effect is emitted

### Requirement: Stable viewport
New PTY output received while in COPY MUST still be parsed, but the viewport MUST stay on the same content (offset adjusted so visible lines do not shift); it MUST NOT jump to the bottom.

#### Scenario: Output during COPY
- GIVEN COPY scrolled up 10 lines
- WHEN 5 new lines of output arrive
- THEN the same content remains visible

### Requirement: Alternate screen has no scrollback
When the child is on the alternate screen (e.g. nvim, htop) there is no scrollback. Entering COPY MUST show the current screen only, and all motions MUST be clamped (offset stays 0) without panic or error. This is an accepted limitation, the same as tmux.

#### Scenario: Enter COPY on alt screen
- GIVEN the child switched to the alternate screen (`\e[?1049h`)
- WHEN COPY is entered
- THEN the current screen is shown and the mode is COPY

#### Scenario: Motions clamped on alt screen
- GIVEN COPY entered on the alternate screen
- WHEN `k`, Ctrl+u, `gg`, `G`, `j`, Ctrl+d are pressed
- THEN the offset remains 0, no panic occurs and no error is shown

### Requirement: Cursor in COPY
While in COPY the effective outer cursor MUST be a steady block, regardless of the shape requested by the child. On returning to TERMINAL the shape last requested by the child MUST be restored. (Chosen behavior: the child's cursor is not shown in COPY because the viewport may be scrolled away from it.)

#### Scenario: Block in COPY [auto]
- GIVEN the child requested Bar steady (`\e[6 q`)
- WHEN the mode becomes COPY
- THEN the effective cursor shape is Block steady

#### Scenario: Restore on exit [auto]
- GIVEN COPY entered with the child's last request being Bar steady
- WHEN COPY exits
- THEN the effective cursor shape is Bar steady again

### Requirement: Exit COPY
`q`, Esc or `i` MUST return to TERMINAL with the viewport at the bottom.

#### Scenario: Exit
- GIVEN COPY scrolled up
- WHEN `q` (or Esc, or `i`) is pressed
- THEN mode is TERMINAL and the live screen is shown

#### Scenario: Real session [manual]
- GIVEN a long `ls -R` output
- THEN j/k/Ctrl+u/Ctrl+d/gg/G navigate it and exit lands at the bottom
