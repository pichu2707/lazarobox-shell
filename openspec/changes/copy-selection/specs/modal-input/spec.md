# Delta for Modal Input (copy-selection)

All scenarios [auto] unless tagged. Reason: COPY gains decision-like keys (`v`, `V`, `y`, Esc). A held `y`, Esc, `q` or `i` would otherwise yank or exit COPY and then repeat into TERMINAL as a PTY write.

## MODIFIED Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be encoded and sent to the focused pane's PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU, for `v`, `V`, `y`, Esc, `q` and `i` in COPY, and in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action, confirms, toggles a selection, yanks or exits.
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
