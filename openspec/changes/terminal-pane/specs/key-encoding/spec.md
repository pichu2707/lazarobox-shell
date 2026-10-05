# Key Encoding Specification

## Purpose

Pure translation of key events and paste into PTY bytes.

## Requirements

### Requirement: Key to bytes
The encoder MUST map: chars to UTF-8; Ctrl+letter to 0x01–0x1A; Alt+key to ESC-prefixed bytes; Enter `\r`; Backspace 0x7F; Tab `\t`; Shift+Tab `\e[Z`; Esc 0x1B; Home/End/PgUp/PgDn/Del/Ins and F-keys to xterm sequences. Kitty keyboard flags MUST NOT be used.

#### Scenario: Table-driven basics [auto]
- GIVEN `a`, `é`, Ctrl+c, Alt+x, Enter, Backspace, Shift+Tab
- THEN bytes are `a`, `é`(UTF-8), `03`, `1b 78`, `0d`, `7f`, `1b 5b 5a`

### Requirement: Application cursor mode
Arrows MUST send `ESC O x` when application-cursor is enabled, else `ESC [ x`.

#### Scenario: Normal mode [auto]
- GIVEN application cursor off
- WHEN Up is encoded
- THEN bytes are `\e[A`

#### Scenario: Application mode [auto]
- GIVEN application cursor on
- WHEN Up/Down/Right/Left are encoded
- THEN bytes are `\eOA`/`\eOB`/`\eOC`/`\eOD`

#### Scenario: Arrows in nvim [manual]
- GIVEN nvim running
- THEN arrow keys move the cursor

### Requirement: Bracketed paste
Pasted text MUST be wrapped in `\e[200~`…`\e[201~` only if the inner app enabled bracketed paste; otherwise sent raw.

#### Scenario: Enabled [auto]
- GIVEN bracketed paste enabled and paste `hi`
- THEN bytes are `\e[200~hi\e[201~`

#### Scenario: Disabled [auto]
- GIVEN bracketed paste disabled
- THEN bytes are `hi` with no markers
