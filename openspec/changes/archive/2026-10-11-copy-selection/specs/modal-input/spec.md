# Delta for Modal Input (copy-selection)

All scenarios [auto] unless tagged. Reason: COPY gains decision-like keys (`v`, `V`, `y`, Esc). A held `y`, Esc, `q` or `i` would otherwise yank or exit COPY and then repeat into TERMINAL as a PTY write.

## MODIFIED Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be encoded and sent to the focused pane's PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU, for `v`, `V`, `y`, Esc, `q` and `i` in COPY, and in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action, confirms, toggles a selection, yanks or exits.

A key whose Press left COPY (`q`, `i`, Esc, or `y` after a successful yank) MUST NOT leak into TERMINAL: its Repeat events MUST be dropped until its Release or any other Press arrives. Because terminals that report no event types (the default: no keyboard enhancement flags are pushed) deliver OS auto-repeat as repeated Press events, a Press of the identical key (same code and modifiers) MUST also be dropped when it arrives within 500 ms of the exit or of the previous dropped one, so a long hold keeps being dropped. Any other Press, a Release of that key, a paste, or a gap over 500 ms ends the debounce. A yank that leaves COPY open (selection over the cap, or no text) does not start it. Tradeoff: a deliberate retype of the exit key within 500 ms of leaving COPY is dropped too.
(Previously: Repeat was handled like Press in all of COPY, including `v`, `V`, `y`, Esc, `q` and `i`)

#### Scenario: Passthrough
- GIVEN TERMINAL
- WHEN `l` is pressed
- THEN a WritePty effect addressed to the focused pane with `l` is emitted and mode stays TERMINAL

#### Scenario: Repeat ignored in decision states
- GIVEN PREFIX, group pending, or any confirmation
- WHEN a Repeat event arrives
- THEN no action is dispatched and the state is unchanged

#### Scenario: Release ignored
- GIVEN any mode
- WHEN a Release event arrives
- THEN nothing happens

#### Scenario: Input goes to the focused pane only
- GIVEN two panes with the right focused
- WHEN `a` is pressed
- THEN WritePty is addressed to the right pane only

#### Scenario: Repeat in MENU [auto]
- GIVEN MENU
- WHEN Repeat events for `j` and for Esc arrive
- THEN `j` moves the selection and Esc does nothing

#### Scenario: Repeat of selection keys in COPY [auto]
- GIVEN COPY with a selection
- WHEN Repeat events for `v`, `V`, `y`, Esc, `q` and `i` arrive
- THEN the selection, the mode (still COPY) and the effects are unchanged (no SetClipboard, no WritePty)

#### Scenario: Repeat of motions in COPY [auto]
- GIVEN COPY
- WHEN a Repeat event for `j` arrives
- THEN the cursor moves one row down, as for Press

#### Scenario: Held exit key does not leak into TERMINAL [auto]
- GIVEN COPY, and `q` (or `i`, Esc, or `y` with a selection) is pressed, leaving COPY
- WHEN Repeat events of that same key arrive in TERMINAL
- THEN no WritePty is emitted
- AND after its Release, or after any other Press, the key is passed through again
- AND a Repeat of a different key is passed through normally

#### Scenario: Held exit key sent as Presses is dropped within the window [auto]
- GIVEN COPY, and `q` (or `i`, Esc, or `y` with a selection) is pressed at t0, leaving COPY
- WHEN Press events of that same key arrive at t0+400 ms, t0+433 ms and t0+466 ms (no Repeat kind)
- THEN no WritePty is emitted for any of them
- AND a `y` exit still shows its "Copied N characters" notice
- AND a hold continues to be dropped while each Press is within 500 ms of the previous one

#### Scenario: Deliberate press after the window passes through [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `q` is pressed again at t0+600 ms
- THEN WritePty with `q` is emitted

#### Scenario: Another key ends the debounce [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `x` is pressed at t0+100 ms and `q` at t0+150 ms
- THEN both are passed through as WritePty
- AND a Press of the same code with different modifiers (Shift+Q, Ctrl+q) is never dropped by a plain-`q` exit

#### Scenario: A paste ends the debounce [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN a paste arrives at t0+100 ms and `q` is pressed at t0+200 ms
- THEN WritePty with `q` is emitted

#### Scenario: Window boundary and refresh [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `q` is pressed at t0+500 ms
- THEN it is dropped, and at t0+501 ms (instead) it is passed through
- AND Presses dropped at t0+499 ms and t0+998 ms each refresh the window, so one at t0+1499 ms is passed through

#### Scenario: A yank that stays in COPY does not arm the debounce [auto]
- GIVEN COPY with a selection over the cap, or with no text, and `y` is pressed (COPY stays open)
- WHEN the user later leaves COPY with `q` and presses `y` within 100 ms
- THEN WritePty with `y` is emitted
