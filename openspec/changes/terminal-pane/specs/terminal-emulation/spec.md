# Terminal Emulation Specification

## Purpose

Parsing child output into a screen, answering terminal queries, and rendering the screen.

## Requirements

### Requirement: Screen state
The emulator MUST track text, colors (default/indexed/RGB), attributes, wide chars, alt screen, cursor position/visibility, application-cursor mode and bracketed-paste mode, and keep ~10k lines of scrollback.

#### Scenario: Colors and text [auto]
- GIVEN bytes `\e[31mA\e[0m`
- WHEN fed to the emulator
- THEN cell (0,0) is `A` with indexed red foreground

#### Scenario: Mode tracking [auto]
- GIVEN `\e[?1h` then `\e[?2004h`
- WHEN fed
- THEN application-cursor and bracketed-paste are reported enabled; `\e[?1l` disables the former

### Requirement: Query responder
The emulator MUST reply to queries via bytes returned to be written to the PTY: DA1 (`\e[c`) → `\e[?62;c`; DSR 5n → `\e[0n`; DSR 6n → `\e[<row>;<col>R` (1-based, current cursor).

#### Scenario: DA1 [auto]
- GIVEN `\e[c` is fed
- THEN the reply is `\e[?62;c`

#### Scenario: DSR status [auto]
- GIVEN `\e[5n` is fed
- THEN the reply is `\e[0n`

#### Scenario: DSR cursor position [auto]
- GIVEN the cursor at row 3, col 5 (0-based 2,4)
- WHEN `\e[6n` is fed
- THEN the reply is `\e[3;5R`

#### Scenario: Query split across reads [auto]
- GIVEN `\e[6` and `n` arrive in separate chunks
- THEN exactly one reply is produced

### Requirement: Cursor shape (DECSCUSR)
The emulator MUST capture the cursor style requested by the child via DECSCUSR (`CSI Ps SP q`) and expose it as a `CursorShape` value (`Default`, or a shape of Block/Underline/Bar plus blinking/steady). Mapping: Ps 0 → Default; 1 → Block blinking; 2 → Block steady; 3 → Underline blinking; 4 → Underline steady; 5 → Bar blinking; 6 → Bar steady. Unknown Ps values MUST be ignored (shape unchanged). The runtime MUST apply the effective shape to the outer terminal with crossterm `SetCursorStyle` and MUST reset it to the user's default shape on exit and on panic. The effective shape is computed by a pure function of app state (see the copy-mode-scrollback spec for the COPY behavior).

#### Scenario: Bar steady [auto]
- GIVEN `\e[6 q` is fed
- THEN the cursor shape is Bar, steady

#### Scenario: Bar blinking [auto]
- GIVEN `\e[5 q` is fed
- THEN the cursor shape is Bar, blinking

#### Scenario: Block steady [auto]
- GIVEN `\e[2 q` is fed
- THEN the cursor shape is Block, steady

#### Scenario: Default [auto]
- GIVEN `\e[6 q` and then `\e[0 q` are fed
- THEN the cursor shape is Default

#### Scenario: Unknown parameter [auto]
- GIVEN `\e[6 q` and then `\e[9 q` are fed
- THEN the cursor shape remains Bar, steady (no panic)

#### Scenario: Split across reads [auto]
- GIVEN `\e[6 ` and `q` arrive in separate chunks
- THEN the cursor shape is Bar, steady

#### Scenario: No query reply [auto]
- GIVEN `\e[6 q` is fed
- THEN no reply bytes are produced

#### Scenario: Restore on exit [auto, runtime executor with fake writer]
- GIVEN the app has applied a Bar shape
- WHEN the runtime shuts down (normal exit or panic hook)
- THEN `DefaultUserShape` is emitted as part of the terminal restore (the cursor style is terminal-global, so its order relative to leaving the alternate screen does not matter)

#### Scenario: Cursor shape in nvim [manual]
- GIVEN nvim running in the pane inside Kitty
- WHEN entering insert mode, then pressing Esc
- THEN the cursor is a bar in insert mode and a block in normal mode, and after quitting nvim and exiting the app the Kitty cursor is back to its configured default

### Requirement: Terminal view rendering
The view MUST render the emulator screen into the pane area (colors and attributes mapped; the default background comes from the theme, while the default foreground is the host terminal's default (`Reset`) because the theme defines no foreground; wide-char continuation cells skipped) and place the real cursor unless hidden.

#### Scenario: Snapshot [auto, TestBackend + insta]
- GIVEN styled bytes fed to a 10x40 emulator
- WHEN rendered
- THEN the buffer matches the snapshot

#### Scenario: Hidden cursor [auto]
- GIVEN `\e[?25l`
- WHEN rendered
- THEN no cursor position is set

#### Scenario: Real apps [manual]
- GIVEN nvim/LazyVim and htop
- THEN colors, alt screen and layout render correctly
