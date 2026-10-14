# Mouse Input Specification

## Purpose

Mouse capture and the vertical mouse wheel for every pane: the wheel scrolls the pane under the pointer (shell or chat) or is forwarded to the child application when the child asked for mouse events. Only the wheel is handled; clicks, drags and motion are not (native selection needs Shift, see Capture lifecycle). Tags: [auto] = `cargo test`; [manual] = real Kitty. Scenarios without a tag are [auto]. All coordinates in this spec are terminal cells; "step" is the fixed wheel step of 3 lines.

## ADDED Requirements

### Requirement: Capture lifecycle
When the effective `mouse` setting is true (default, see configuration) the app MUST enable host mouse capture with exactly the byte sequence `\x1b[?1000h\x1b[?1006h` (button press/release reporting plus SGR encoding) and no other mouse mode: `?1002`, `?1003` (motion) and `?1015` MUST NOT be enabled. Capture MUST be enabled after the host terminal setup (raw mode and alternate screen) and before the first frame is drawn. When `mouse` is false the app MUST NEVER write any mouse mode sequence, and no mouse event is ever delivered.

On shutdown capture MUST be disabled with exactly `\x1b[?1006l\x1b[?1000l`, as part of restoring the host terminal and before the child teardown wait (see terminal-session). The panic hook MUST emit the same disable sequence whenever capture was enabled at the time of the panic, before the panic message is printed. Changing the effective `mouse` value while the app runs (config menu preview, revert or save) MUST enable or disable capture immediately with the same sequences, and MUST NOT write them when the effective value did not change. While capture is on, native terminal selection requires holding Shift (a documented tradeoff).

#### Scenario: Enabled at startup by default
- GIVEN no `mouse` key in the config
- WHEN the app starts
- THEN `\x1b[?1000h\x1b[?1006h` is written to the host exactly once, before the first frame, and nothing else mouse-related

#### Scenario: Disabled by config
- GIVEN `mouse = false`
- WHEN the app starts, runs and quits
- THEN no `?1000`, `?1002`, `?1003`, `?1006` or `?1015` sequence is ever written

#### Scenario: Only minimal modes
- GIVEN capture enabled
- WHEN the written bytes are inspected
- THEN they contain neither `?1002h`, `?1003h` nor `?1015h`

#### Scenario: Restored on quit
- GIVEN capture enabled
- WHEN the app quits
- THEN `\x1b[?1006l\x1b[?1000l` is written during the terminal restore, before the child teardown wait

#### Scenario: Restored on panic
- GIVEN capture enabled
- WHEN the app panics
- THEN the panic hook writes `\x1b[?1006l\x1b[?1000l` and the rest of the restore still happens

#### Scenario: Panic with capture off writes no mouse bytes
- GIVEN `mouse = false`
- WHEN the app panics
- THEN no mouse mode sequence is written

#### Scenario: Toggled at runtime
- GIVEN the app running with capture on
- WHEN the effective `mouse` value becomes false, and later true again
- THEN the disable sequence is written once, then the enable sequence once, and no sequence is written when the value is set to what it already is

#### Scenario: Terminal is clean after quit [manual, Kitty]
- GIVEN the app used with the wheel and then quit (and, in another run, forced to panic)
- WHEN the host shell prompt returns and the wheel or mouse is moved
- THEN no escape sequences are printed and the terminal scrolls its own scrollback again

#### Scenario: Native selection needs Shift [manual, Kitty]
- GIVEN capture on
- WHEN the user drags with Shift held
- THEN Kitty native selection works

### Requirement: Events handled
Only vertical wheel events (wheel up, wheel down) MUST be handled. Every other mouse event (button press, release, drag, motion, horizontal wheel) MUST be ignored: no effect, no state change, nothing forwarded to any PTY. Modifier keys held during a wheel event MUST NOT change the step; they are encoded as modifier bits only when the wheel is forwarded to a child (see Wheel encoding). A wheel event MUST NEVER change the focused pane, the active tab or the input mode. Wheel events MUST be ignored in every input mode except TERMINAL and COPY: in PREFIX, a pending group, RESIZE, MENU and every confirmation nothing happens and the state is unchanged.

#### Scenario: Non-wheel events ignored
- GIVEN TERMINAL with a shell pane under the pointer
- WHEN a left press, a release, a drag and a motion event arrive
- THEN no effect is emitted and no state changes

#### Scenario: Horizontal wheel ignored
- GIVEN TERMINAL
- WHEN a horizontal wheel event arrives
- THEN nothing happens

#### Scenario: Wheel does not change focus
- GIVEN two panes with the left focused and the pointer over the right pane
- WHEN a wheel event arrives
- THEN the focused pane is still the left one and the mode is unchanged

#### Scenario: Ignored in other modes
- GIVEN PREFIX (and, in turn, a pending group, RESIZE, MENU and a close confirmation)
- WHEN a wheel event arrives over a pane
- THEN nothing is scrolled, nothing is written to any PTY and the mode is unchanged

#### Scenario: Modifier ignored
- GIVEN a main-screen pane with history
- WHEN a wheel-up event with Ctrl held arrives
- THEN the view moves by exactly one step, as without the modifier

### Requirement: Pointer hit-test
A wheel event MUST be routed to the pane of the active tab whose content rectangle (the rectangle from the pane layout, see pane-layout) contains the pointer cell, whether or not that pane is focused. When a pane is zoomed only the zoomed pane can be hit. A pointer outside every pane content rectangle (the statusline, the tab bar, a separator, a menu or popup area, or beyond the body) MUST be ignored. Panes of other tabs MUST NEVER be hit. The hit-test uses the layout computed for the last drawn frame, including after a resize, split, close or zoom.

#### Scenario: Pane under the pointer
- GIVEN two side-by-side panes
- WHEN the pointer is over a cell of the right pane
- THEN the right pane is the target

#### Scenario: Pointer on a separator
- GIVEN two panes with a separator column between them
- WHEN the pointer is on the separator
- THEN the event is ignored

#### Scenario: Pointer on the statusline or tab bar
- GIVEN the statusline at the bottom and the tab bar at the top
- WHEN the pointer is on either
- THEN the event is ignored

#### Scenario: Zoomed pane
- GIVEN a zoomed pane
- WHEN the pointer is over the zoomed pane, and in another case over a cell where a hidden pane would be
- THEN the zoomed pane is the target in both cases

#### Scenario: After a split or resize
- GIVEN a pane that was split so its rectangle shrank
- WHEN the pointer is over the cell that now belongs to the new pane
- THEN the new pane is the target

### Requirement: Wheel routing for shell panes
For a shell pane under the pointer the first matching rule MUST apply, in this order:
1. The pane is in COPY (see Wheel in COPY).
2. The child enabled a mouse protocol mode (PressRelease, ButtonMotion or AnyMotion; X10 `Press` (`?9`) reports no wheel and counts as no mouse mode, as in xterm), on the main or the alternate screen: the wheel event is forwarded to the child (see Wheel encoding). The app does not scroll its own view.
3. The child is on the alternate screen with no mouse mode: three arrow keys are sent (see Alternate screen without mouse mode).
4. Otherwise (main screen, no mouse mode): the pane's scrollback view moves (see Main screen scrollback).

#### Scenario: Mouse mode wins over scrollback
- GIVEN a main-screen pane with 200 lines of history whose child enabled mode PressRelease with SGR encoding
- WHEN the wheel moves over it
- THEN an SGR wheel sequence is written to the PTY and the scrollback offset is unchanged

#### Scenario: Mouse mode on the alternate screen
- GIVEN an alternate-screen child with AnyMotion mode and SGR encoding
- WHEN the wheel moves over it
- THEN an SGR wheel sequence is written and no arrow key is sent

#### Scenario: Alternate screen without mouse mode
- GIVEN an alternate-screen child (less) with no mouse mode
- WHEN the wheel moves over it
- THEN arrow keys are written, not a mouse sequence

#### Scenario: Plain main screen
- GIVEN a main-screen shell with history and no mouse mode
- WHEN the wheel moves up over it
- THEN no bytes are written to the PTY and the scrollback offset increases

#### Scenario: Unfocused pane is forwarded to
- GIVEN the left pane focused and the pointer over a right pane whose child is in mouse mode
- WHEN the wheel moves
- THEN the sequence is written to the right pane's PTY only

### Requirement: Wheel encoding
A wheel event forwarded to a child MUST be encoded as a press of button 64 (wheel up) or 65 (wheel down), with NO release event, using the encoding the child selected (vt100 `mouse_protocol_encoding`). Modifier bits are added to the button number (Shift +4, Alt +8, Ctrl +16). The coordinates MUST be pane-local and 1-based: `col = pointer_x - pane_rect.x + 1`, `row = pointer_y - pane_rect.y + 1`. One wheel event produces exactly one sequence (the 3-line step does not apply to forwarded events; the child decides its own step).
- SGR encoding: `\x1b[<{64|65 + mods};{col};{row}M`; without modifiers `\x1b[<64;{col};{row}M` (up) and `\x1b[<65;{col};{row}M` (down).
- Default (X10) encoding: `\x1b[M` followed by three bytes `32+button`, `32+col`, `32+row`, with button 64 (up, `\x60`) or 65 (down, `a`) plus modifier bits. When `col` or `row` exceeds 223 the event MUST be dropped (no bytes written).
- UTF-8 (1005) encoding: as Default, but each of the three values is `32+value` written as a UTF-8 character; when `col` or `row` exceeds 2015 the event MUST be dropped.

The three modes PressRelease, ButtonMotion and AnyMotion MUST receive the wheel events. When the child's mouse mode is None or Press (X10) nothing is encoded and the wheel follows the next routing rule (alternate screen arrows, or scrollback).

#### Scenario: SGR wheel up
- GIVEN a child in PressRelease mode with SGR encoding, pane rectangle origin (10, 5) and the pointer at (14, 8)
- WHEN the wheel moves up
- THEN `\x1b[<64;5;4M` is written to that pane's PTY

#### Scenario: SGR wheel down
- GIVEN the same child and pointer
- WHEN the wheel moves down
- THEN `\x1b[<65;5;4M` is written

#### Scenario: Default encoding wheel up
- GIVEN a child in PressRelease mode with Default encoding and the pointer at pane-local cell (col 1, row 1)
- WHEN the wheel moves up
- THEN the bytes `\x1b[M` `\x60` `\x21` `\x21` are written

#### Scenario: Default encoding wheel down
- GIVEN a child in ButtonMotion mode with Default encoding and the pointer at pane-local (col 3, row 2)
- WHEN the wheel moves down
- THEN the bytes `\x1b[M` `a` `#` `\x22` are written

#### Scenario: Default encoding out of range
- GIVEN a child with Default encoding and a pointer at pane-local column 224
- WHEN the wheel moves
- THEN nothing is written

#### Scenario: Modifier bits in SGR
- GIVEN a child in PressRelease mode with SGR encoding and Ctrl held
- WHEN the wheel moves up at pane-local (1, 1)
- THEN `\x1b[<80;1;1M` is written (64 + 16)

#### Scenario: X10 Press mode gets no wheel
- GIVEN a child in Press (X10, `?9`) mode with SGR encoding, on the alternate screen and, in another case, on the main screen
- WHEN the wheel moves
- THEN no mouse sequence is written: the alternate screen gets arrow keys and the main screen scrolls its own view

#### Scenario: UTF-8 encoding
- GIVEN a child with UTF-8 encoding and the pointer at pane-local (col 300, row 1)
- WHEN the wheel moves up
- THEN `\x1b[M`, `\x60`, the UTF-8 character U+014C (32+300) and `\x21` are written; at column 2016 nothing is written

#### Scenario: Every wheel-reporting mode receives the wheel
- GIVEN a child with SGR encoding in each of PressRelease, ButtonMotion and AnyMotion
- WHEN the wheel moves up once in each
- THEN exactly one `\x1b[<64;...M` sequence is written each time

#### Scenario: Coordinates are pane-local
- GIVEN a right pane whose rectangle starts at column 40
- WHEN the pointer is on that pane's first column
- THEN the encoded column is 1, not 41

#### Scenario: Forwarded events are not multiplied
- GIVEN a child in mouse mode
- WHEN one wheel event arrives
- THEN exactly one sequence is written (not three)

#### Scenario: Vim and htop [manual, Kitty]
- GIVEN vim with `:set mouse=a` and, in another pane, htop
- WHEN the wheel moves over each
- THEN vim scrolls its buffer and htop scrolls its list

### Requirement: Alternate screen without mouse mode
When the child is on the alternate screen and its mouse mode is None, one wheel event MUST write exactly three cursor keys (the step) to the pane's PTY: Up for wheel up and Down for wheel down. The key encoding MUST honor the child's application cursor mode (DECCKM): `\x1b[A` / `\x1b[B` when it is off (CSI) and `\x1bOA` / `\x1bOB` when it is on (SS3). The app's own scrollback view is not touched (the alternate screen has none).

#### Scenario: Normal cursor mode
- GIVEN `less` on the alternate screen, application cursor mode off
- WHEN the wheel moves down once
- THEN `\x1b[B\x1b[B\x1b[B` is written

#### Scenario: Application cursor mode
- GIVEN an alternate-screen child that set application cursor mode (`\x1b[?1h`)
- WHEN the wheel moves up once
- THEN `\x1bOA\x1bOA\x1bOA` is written

#### Scenario: Mode follows the child
- GIVEN the same child after it resets application cursor mode (`\x1b[?1l`)
- WHEN the wheel moves up
- THEN `\x1b[A` is written three times

#### Scenario: Less and man [manual, Kitty]
- GIVEN `man ls` open in a pane
- WHEN the wheel moves
- THEN the page scrolls three lines per notch

### Requirement: Main screen scrollback
For a main-screen pane with no mouse mode, not in COPY, one wheel event MUST move the pane's scrollback view by the step of 3 rows: wheel up goes back in history (offset +3), wheel down comes forward (offset -3). The offset MUST clamp to `[0, history length]` with no error. No bytes are written to the PTY and COPY is not entered. While the offset is above 0 the view is anchored: new output MUST be parsed but the same content MUST stay visible (the offset grows by the number of rows that scrolled out).

The pane MUST return to the live screen (offset 0) when any key is encoded and written to the pane's PTY, when a paste is written to it, or when wheel down brings the offset to 0 (a step larger than the remaining offset clamps to 0). A key swallowed by the app (prefix and its tree, MENU, COPY motions) does not return the view to live. As today, a focus change or a tab switch resets the scrollback offset of the shell pane that is left (the focused pane losing focus, or every pane of the tab being left) to live. A pane scrolled by the wheel while NOT focused keeps its offset, unaffected by focus moving between other panes, until it is wheeled back to 0, is focused and then left, or its tab is left. Chat panes are the exception: a wheel- or page-scrolled chat transcript keeps its position across focus changes and tab switches (see chat-pane). A pane with a scrolled view MUST show the badge defined in copy-mode-scrollback; the statusline position is defined in statusline.

#### Scenario: Wheel up scrolls back
- GIVEN a main-screen pane with 100 lines of history at offset 0
- WHEN the wheel moves up once
- THEN the offset is 3 and no bytes are written

#### Scenario: Wheel down comes forward
- GIVEN the offset is 5
- WHEN the wheel moves down once
- THEN the offset is 2

#### Scenario: Wheel down reaches live
- GIVEN the offset is 2
- WHEN the wheel moves down once
- THEN the offset is 0 and the pane shows the live screen

#### Scenario: Clamped at the top
- GIVEN 4 lines of history and the offset at 3
- WHEN the wheel moves up twice
- THEN the offset is 4 after the first event and stays 4

#### Scenario: Clamped at the bottom
- GIVEN the offset is 0
- WHEN the wheel moves down
- THEN the offset stays 0

#### Scenario: No history
- GIVEN a main-screen pane with no history
- WHEN the wheel moves up
- THEN the offset stays 0 and nothing is written

#### Scenario: Anchored on new output
- GIVEN the offset is 10
- WHEN 5 new lines of output arrive
- THEN the same content stays visible and the offset is 15

#### Scenario: A key returns to live
- GIVEN the focused pane has offset 12
- WHEN `l` is pressed in TERMINAL (encoded and written to the PTY)
- THEN the offset is 0

#### Scenario: The literal prefix returns to live
- GIVEN the focused pane has offset 12
- WHEN `Ctrl+Space` is pressed twice (the second sends the prefix byte to the PTY)
- THEN the offset is 0

#### Scenario: COPY entered on a wheel-scrolled pane keeps the view
- GIVEN the focused pane has offset 12 from the wheel
- WHEN `Ctrl+Space [` is pressed
- THEN the mode is COPY, the COPY viewport offset is 12, the cursor is on the last visible row, the badge and the statusline position read 12, and the first `k` moves the cursor up one row without moving the view

#### Scenario: A swallowed key does not return to live
- GIVEN the focused pane has offset 12
- WHEN `Ctrl+Space` is pressed and then Esc cancels the PREFIX
- THEN the offset is still 12

#### Scenario: A paste returns to live
- GIVEN the focused pane has offset 12
- WHEN a paste is written to its PTY
- THEN the offset is 0

#### Scenario: Unfocused pane keeps its offset
- GIVEN three panes, the left one focused, and the right one wheel-scrolled to offset 6
- WHEN focus moves to the middle pane and a wheel event arrives over the right pane
- THEN the right pane's offset was 6 after the focus move and continues from 6

#### Scenario: Focus and leave resets
- GIVEN the right pane at offset 6 (wheel-scrolled while unfocused)
- WHEN focus moves to the right pane and then to another pane
- THEN the right pane's offset is 0

#### Scenario: Tab switch resets
- GIVEN a wheel-scrolled shell pane at offset 6
- WHEN its tab is left
- THEN the pane's offset is 0 on return

#### Scenario: COPY not entered
- GIVEN TERMINAL and a wheel scroll on the focused pane
- WHEN the mode is inspected
- THEN it is TERMINAL

### Requirement: Wheel in COPY
When the pane under the pointer is the pane in COPY, one wheel event MUST scroll its COPY viewport by the step of 3 rows (wheel up goes back in history), clamped to the history bounds exactly as the COPY motions (see copy-mode-scrollback). The COPY cursor MUST be kept inside the viewport (moved to the nearest edge row if the scroll would take it out), the selection MUST be preserved and extended by the cursor movement as `j`/`k` would, and COPY MUST stay active. A wheel event over a pane that is NOT in COPY while COPY is active on another pane MUST follow the rules of that pane (main-screen scrollback view, forwarding, arrow keys or chat transcript) and MUST leave the COPY state, the COPY viewport, the selection and the mode untouched. The alternate screen pane in COPY has no scrollback and the wheel is clamped (no panic).

#### Scenario: Wheel scrolls COPY
- GIVEN COPY on a pane with 100 lines of history at offset 0
- WHEN the wheel moves up over that pane
- THEN the COPY viewport offset is 3, the mode is still COPY and no bytes are written to any PTY

#### Scenario: Cursor kept inside the viewport
- GIVEN COPY with the cursor on the bottom visible row and the viewport at offset 0
- WHEN the wheel moves up once
- THEN the viewport offset is 3 and the cursor is on a visible row

#### Scenario: Selection kept
- GIVEN COPY with a character selection
- WHEN the wheel moves down over the pane
- THEN the selection is still active

#### Scenario: Clamped in COPY
- GIVEN COPY at the top of the history
- WHEN the wheel moves up
- THEN the offset is unchanged and nothing panics

#### Scenario: Wheel over another pane while COPY is elsewhere
- GIVEN COPY on the left pane scrolled up 10 rows and a right main-screen pane with history
- WHEN the wheel moves up over the right pane
- THEN the right pane's view offset is 3, the left pane is still in COPY at offset 10, and the mode is COPY

#### Scenario: Mouse-mode child beside a COPY pane
- GIVEN COPY on the left pane and the pointer over a right pane whose child is in mouse mode
- WHEN the wheel moves
- THEN the encoded wheel is written to the right pane's PTY and the COPY state is untouched

#### Scenario: Alternate screen in COPY
- GIVEN COPY on an alternate-screen pane
- WHEN the wheel moves up and down over it
- THEN the offset stays 0 and nothing is written

### Requirement: Wheel over a chat pane
A wheel event over a chat pane showing the conversation view MUST scroll its transcript by the step of 3 lines (wheel up goes back, wheel down comes forward), through the same line-step scrolling as chat COPY (see chat-pane), clamped at both ends. It MUST NOT enter COPY, change focus or write to any PTY. When the pane is in COPY the event acts as a COPY motion of the same size. When wheel down brings the view to the bottom it follows new deltas again. While the chat pane shows an overlay or non-conversation view (the conversation list, the tag editor, the note editor, the summary draft, or engine guidance with no transcript) the wheel event MUST be ignored.

#### Scenario: Wheel up scrolls the transcript
- GIVEN a chat pane whose transcript is longer than the pane, at the bottom
- WHEN the wheel moves up over it
- THEN the view is 3 lines above the bottom and no PTY write happens

#### Scenario: Wheel down to the bottom follows again
- GIVEN the view 2 lines above the bottom
- WHEN the wheel moves down
- THEN the view is at the bottom and new deltas keep the last line visible

#### Scenario: Pinned while scrolled
- GIVEN a transcript scrolled up by wheel and a stream running
- WHEN deltas arrive
- THEN the same content stays visible

#### Scenario: Clamped on a short transcript
- GIVEN a transcript shorter than the pane
- WHEN the wheel moves up
- THEN nothing changes

#### Scenario: Unfocused chat pane
- GIVEN a shell pane focused and the pointer over a chat pane
- WHEN the wheel moves up
- THEN the chat transcript scrolls and the shell pane stays focused

#### Scenario: Conversation list ignores the wheel
- GIVEN a chat pane showing the conversation list
- WHEN the wheel moves
- THEN the list selection is unchanged

#### Scenario: Chat in COPY
- GIVEN a chat pane in COPY
- WHEN the wheel moves up over it
- THEN the transcript moves up 3 lines and the mode is still COPY
