# Delta for modal-input

## MODIFIED Requirements

### Requirement: Prefix key
Ctrl+Space (reported as NUL 0x00) and Ctrl+] (reported as 0x1D) MUST each enter PREFIX without writing to the PTY. Both keys are fixed; neither is configurable. The prefix keys and the action table MUST be data, not hard-coded branches. Pressing a prefix key again in PREFIX MUST write that key's own literal byte to the focused shell pane (Ctrl+Space writes NUL 0x00, Ctrl+] writes 0x1D) and return to TERMINAL. Pressing the other prefix key in PREFIX does the same: it writes its own literal byte. Esc or an unmapped key in PREFIX returns to TERMINAL, swallowed. When the focused pane is not a shell pane, a repeated prefix key MUST write nothing and MUST show a notice; it MUST NOT be ignored silently.
(Previously: only Ctrl+Space entered PREFIX; Ctrl+] was an ordinary key sent to the pane)

#### Scenario: Enter PREFIX
- GIVEN TERMINAL
- WHEN Ctrl+Space is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Enter PREFIX with the alias [auto]
- GIVEN TERMINAL
- WHEN Ctrl+] (0x1D) is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Literal prefix
- GIVEN PREFIX entered with Ctrl+Space
- WHEN Ctrl+Space is pressed again
- THEN WritePty `[0x00]` to the focused pane is emitted and mode is TERMINAL

#### Scenario: Literal alias [auto]
- GIVEN PREFIX entered with Ctrl+]
- WHEN Ctrl+] is pressed again
- THEN WritePty `[0x1D]` to the focused pane is emitted and mode is TERMINAL

#### Scenario: Other prefix key inside PREFIX [auto]
- GIVEN PREFIX entered with Ctrl+]
- WHEN Ctrl+Space is pressed
- THEN WritePty `[0x00]` to the focused pane is emitted and mode is TERMINAL
- AND the same holds with the keys swapped (`[0x1D]`)

#### Scenario: Literal prefix on a chat pane [auto]
- GIVEN PREFIX with a chat pane focused
- WHEN Ctrl+Space or Ctrl+] is pressed
- THEN no WritePty is emitted, mode is TERMINAL and a notice is shown

#### Scenario: Cancel/unknown
- GIVEN PREFIX
- WHEN Esc or an unmapped key (e.g. `z`) is pressed
- THEN mode is TERMINAL and the key is swallowed (no WritePty)

#### Scenario: Alias drives the whole tree [auto]
- GIVEN TERMINAL
- WHEN Ctrl+] then `w` then `v` are pressed
- THEN the pane splits right, exactly as with Ctrl+Space

#### Scenario: Real terminal reports the prefix [manual, terminal records]
- GIVEN a terminal that has a record under `docs/terminals/`
- WHEN Ctrl+] is pressed (and Ctrl+Space, where the terminal delivers it)
- THEN PREFIX is entered and nothing is written to the PTY (the root which-key hint appears in the statusline while PREFIX is pending; see statusline)
- AND the result is written in the terminal's record (see terminal-compatibility)

The row of the binding table for the prefix MUST read: `Ctrl+Space` / `Ctrl+]`: send the prefix key itself: a NUL byte (0x00) / 0x1D to the focused shell pane (see Prefix key).
