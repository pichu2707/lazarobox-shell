# Modal Input Specification

## Purpose

TERMINAL / PREFIX / COPY state machine plus quit confirmation, as a pure `update(event) -> effects` function. All scenarios [auto] unless tagged.

## Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be encoded and sent to the PTY. Initial mode is TERMINAL.

#### Scenario: Passthrough
- GIVEN TERMINAL
- WHEN `l` is pressed
- THEN a WritePty effect with `l` is emitted and mode stays TERMINAL

### Requirement: Prefix key
Ctrl+Space (reported as NUL 0x00) MUST enter PREFIX without writing to the PTY. The prefix key and the action table MUST be data, not hard-coded branches.

#### Scenario: Enter PREFIX
- GIVEN TERMINAL
- WHEN Ctrl+Space is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Literal prefix
- GIVEN PREFIX
- WHEN Ctrl+Space is pressed again
- THEN WritePty `[0x00]` is emitted and mode is TERMINAL

#### Scenario: Cancel/unknown
- GIVEN PREFIX
- WHEN Esc or an unmapped key (e.g. `z`) is pressed
- THEN mode is TERMINAL and the key is swallowed (no WritePty)

#### Scenario: Real terminal reports NUL [manual, Kitty]
- GIVEN Kitty
- WHEN Ctrl+Space is pressed
- THEN PREFIX is entered

### Requirement: Quit confirmation
`q` in PREFIX MUST enter a confirmation state shown as "Quit? (y/n)". `y` MUST emit Quit; any other key (n, Esc, other) MUST return to TERMINAL, swallowed. Only lowercase `y` with no modifiers confirms; `Y` (Shift or Caps Lock) declines.

#### Scenario: Confirm
- GIVEN confirmation pending
- WHEN `y` is pressed
- THEN a Quit effect is emitted

#### Scenario: Decline
- GIVEN confirmation pending
- WHEN `n`, Esc or `x` is pressed
- THEN mode is TERMINAL, no Quit, no WritePty

### Requirement: Enter COPY
`[` in PREFIX MUST enter COPY.

#### Scenario: Enter COPY
- GIVEN PREFIX
- WHEN `[` is pressed
- THEN mode is COPY and no WritePty is emitted

### Requirement: Paste and resize in any mode
Paste events MUST be sent to the PTY only in TERMINAL; Resize events MUST emit ResizePty in every mode.

#### Scenario: Paste in COPY
- GIVEN COPY
- WHEN a paste event arrives
- THEN no WritePty is emitted
