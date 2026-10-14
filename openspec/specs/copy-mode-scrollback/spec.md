# Copy Mode Scrollback Specification

## Purpose

Vim-style navigation of ~10k lines of scrollback with character/line selection and yank (see copy-selection). All scenarios [auto] unless tagged.

## Requirements

### Requirement: Navigation
COPY has a cursor (absolute row, column). On entry it MUST be at the position of the pane's live cursor, clamped into the pane. `h`/`l` MUST move it one column left/right, `0` to the first column of its row and `$` to the last non-blank column of its row (column 0 when the row is blank). `j`/`k` MUST move it one row down/up; the viewport MUST scroll only when the cursor would cross the top or bottom edge of the viewport. Ctrl+d/Ctrl+u MUST move the cursor and the viewport half a page down/up; `gg` MUST move to the top of scrollback; `G` to the bottom (live screen). Movement MUST clamp at all bounds. If output scrolled the cursor off screen, the next motion of any kind (including `h`/`l`/`0`/`$`) MUST bring it back into view.

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

#### Scenario: Horizontal motion brings an off-screen cursor back [auto]
- GIVEN COPY with the cursor scrolled off screen by output
- WHEN `l` is pressed
- THEN the cursor moves one column right and is visible again

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
While in COPY the effective outer cursor MUST be a steady block placed on the COPY cursor's cell, regardless of the shape requested by the child. On returning to TERMINAL the shape last requested by the child MUST be restored. (Chosen behavior: the child's cursor is not shown in COPY because the viewport may be scrolled away from it.)

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

### Requirement: COPY on a chat pane
Entering COPY (`[` in PREFIX) while the focused pane is a chat pane MUST succeed when the pane shows the conversation view: the mode becomes COPY and the transcript is scrolled by lines through the keys of modal-input (Chat COPY keys). COPY on a chat pane has no cursor, no selection and no yank: the vt100 state is not involved and no COPY cursor state is created. When the chat pane shows the conversation list, the tag editor, the note editor, the summary draft, a delete confirmation or an engine guidance state with no transcript, entering COPY MUST leave the mode TERMINAL and show the notice `nothing to scroll here` through the existing notice slot until the next key. The previous refusal notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer` MUST NOT exist any more.

#### Scenario: Enter on a chat pane
- GIVEN a focused chat pane in PREFIX showing a conversation
- WHEN `[` is pressed
- THEN the mode is COPY, no notice is shown, the composer text is unchanged and no WritePty is emitted

#### Scenario: Enter on the conversation list
- GIVEN a focused chat pane in PREFIX showing the conversation list
- WHEN `[` is pressed
- THEN the mode is TERMINAL and the notice `nothing to scroll here` is shown until the next key

#### Scenario: Notice clears on the next key
- GIVEN the notice is shown
- WHEN any key is pressed
- THEN the notice is gone and the statusline shows the chat segments again

#### Scenario: COPY on a shell pane beside a chat pane
- GIVEN a shell pane focused next to a chat pane
- WHEN `Ctrl+Space [` is pressed
- THEN the mode is COPY on the shell pane as before and the chat pane keeps its scroll position

### Requirement: Chat streams continue during COPY
While COPY is active on a shell pane, chat panes (in any tab) MUST keep streaming: deltas MUST keep being applied and saved, and nothing in COPY MUST be affected by them. Leaving COPY MUST show the accumulated chat state.

#### Scenario: Stream while in COPY
- GIVEN COPY on the left shell pane scrolled up 10 lines and a chat pane streaming on the right
- WHEN 3 deltas arrive
- THEN the shell viewport and selection are unchanged and the chat transcript holds the 3 deltas

#### Scenario: Focus moves to the chat pane
- GIVEN COPY on the left shell pane and a streaming chat pane on the right
- WHEN focus moves to the chat pane
- THEN COPY exits per the existing rule, the left viewport returns to the bottom and the stream continues

### Requirement: Chat COPY navigation
In COPY on a chat pane the transcript MUST scroll by lines (see chat-pane, Transcript scrolling): `j`/`k` one line down/up, Ctrl+d/Ctrl+u half a page of the transcript viewport down/up, `gg` to the top of the transcript, `G` to the bottom. Movement MUST clamp at both ends with no error. The view MUST stay on the same content when deltas arrive (pinned, the stream keeps running and being saved); `G` or reaching the bottom does not leave COPY. Leaving COPY (Esc or `q`) returns the transcript to the bottom and follow mode, and sets the mode to TERMINAL. Isolation: no key is delivered to the composer or to any PTY while in chat COPY.

#### Scenario: Line motion
- GIVEN a transcript of 100 lines in a 20-line viewport at the bottom, in COPY
- WHEN `k` is pressed
- THEN the view is 1 line above the bottom; `j` returns it

#### Scenario: Half page
- GIVEN a chat pane in COPY with a 20-line viewport
- WHEN Ctrl+u is pressed
- THEN the view moves up 10 lines; Ctrl+d moves it down 10

#### Scenario: Top and bottom
- GIVEN a chat pane in COPY mid transcript
- WHEN `g` `g` then `G` are pressed
- THEN the view is at the top, then at the bottom

#### Scenario: Clamping
- GIVEN the view at the top of the transcript
- WHEN `k` or Ctrl+u is pressed
- THEN the view is unchanged; likewise `j`/Ctrl+d at the bottom

#### Scenario: Short transcript
- GIVEN a transcript shorter than the viewport, in COPY
- WHEN `k`, Ctrl+u and `gg` are pressed
- THEN the view is unchanged and nothing panics

#### Scenario: Pinned while a stream runs
- GIVEN COPY on a chat pane scrolled up 10 lines with a stream running
- WHEN 5 lines of delta arrive
- THEN the same content stays visible

#### Scenario: Exit returns to follow
- GIVEN COPY on a chat pane scrolled up
- WHEN Esc (or `q`) is pressed
- THEN the mode is TERMINAL and the last transcript line is visible and follows new deltas

#### Scenario: Composer untouched
- GIVEN a chat pane with composer text `hi` in COPY
- WHEN `j`, `x`, Enter and Backspace are pressed
- THEN the composer text is `hi`, no ChatSend and no WritePty is emitted

#### Scenario: Lone g
- GIVEN `g` pending in chat COPY
- WHEN any non-`g` key is pressed
- THEN the pending `g` is discarded

#### Scenario: Per-pane scroll state
- GIVEN a shell pane and a chat pane, COPY on the chat pane scrolled up 10 lines
- WHEN the shell pane's view offset is inspected
- THEN it is unchanged (0)

#### Scenario: Focus or tab change after Ctrl+Space keeps the view
- GIVEN COPY on a chat pane scrolled up 3 lines
- WHEN `Ctrl+Space` is pressed (mode PREFIX, COPY is over) and then the focus moves to another pane or the tab changes
- THEN the mode is TERMINAL, the chat transcript stays pinned where it was and its `[+3]` badge keeps showing (consistent with the chat-pane rule that chat scroll survives focus and tab changes)

### Requirement: Scrolled view badge
Any pane whose view is scrolled back MUST show the badge `[+N]` in its top-right corner, over the content rectangle: for a shell pane N is the scrollback offset (rows above the live bottom, whether the pane is in COPY or only scrolled by the wheel); for a chat pane N is the number of transcript lines below the viewport. The badge MUST NOT be shown at offset 0 (live, or following), on the alternate screen, or on a pane too narrow to hold it. It MUST update as the view moves and as anchored output arrives, MUST NOT shift the pane's content or its PTY size, and MUST NOT panic at any pane size. The badge is a drawing overlay: it is not part of the pane's text, selection or yank.

#### Scenario: Shell pane scrolled by wheel
- GIVEN a main-screen pane at offset 12 and not in COPY
- WHEN rendered
- THEN `[+12]` is drawn in the pane's top-right corner

#### Scenario: Not shown when live
- GIVEN a pane at offset 0
- WHEN rendered
- THEN no badge is drawn

#### Scenario: Shown in COPY
- GIVEN COPY on a pane at offset 7
- WHEN rendered
- THEN `[+7]` is drawn in that pane

#### Scenario: Chat pane scrolled
- GIVEN a chat pane whose view is 9 lines above the bottom
- WHEN rendered
- THEN `[+9]` is drawn in its top-right corner, and no badge is drawn when it is at the bottom

#### Scenario: Anchored output updates the badge
- GIVEN a scrolled shell pane at `[+10]`
- WHEN 5 lines of output arrive
- THEN the badge reads `[+15]`

#### Scenario: Unfocused scrolled pane
- GIVEN a scrolled pane that is not focused
- WHEN rendered
- THEN its badge is drawn

#### Scenario: Tiny pane
- GIVEN a scrolled pane 3 columns wide
- WHEN rendered
- THEN no badge is drawn and nothing panics

#### Scenario: Alternate screen
- GIVEN an alternate-screen pane
- WHEN rendered
- THEN no badge is drawn
