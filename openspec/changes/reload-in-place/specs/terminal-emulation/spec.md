# Delta for Terminal Emulation

## Base of this delta

This delta targets `openspec/specs/terminal-emulation/spec.md` and adds one requirement. It rests on the public API of `vt100` 0.16.2 (Cargo.toml:43): `Screen` is `Clone` and offers `state_formatted`, `rows_formatted`, `set_scrollback` and `input_mode_formatted`, but not the scroll region, origin mode, saved cursor or the inactive grid (design D5). The screen method is Open question 2 of design.md; this delta writes its recommendation (A, serialize at reload).

## ADDED Requirements

### Requirement: Screen snapshot [S3]
A PTY pane MUST be able to produce a snapshot of its screen that, fed to a new pane of the same size and scrollback, reproduces: every visible cell's text and attributes (colours, bold, italic, underline, inverse), wide and combining characters, the wrap of each row, the history rows and their count, the cursor position and visibility, the application keypad and cursor modes, bracketed paste, the mouse mode and encoding, focus reports, the cursor shape and the OSC 7 folder. When the alternate screen is active, the snapshot MUST hold the primary screen as it was when the alternate one was entered, then the alternate screen, and the restored pane MUST be on the alternate screen. Feeding the snapshot MUST send nothing to the child. A snapshot MUST be at most 8 MiB: the oldest history rows are dropped first and the first restored row then reads `[older scrollback not kept by the reload]`. The scroll region, origin mode, the saved cursor and the window title are not kept.

#### Scenario: Round trip of a shell screen [auto]
- GIVEN a pane fed 300 lines of coloured output, a wrapped long line, a wide character, then a prompt with the cursor at column 12
- WHEN its snapshot is fed to a new pane
- THEN every visible cell, the history, the wrap flags and the cursor are equal

#### Scenario: Alternate screen with the primary underneath [auto]
- GIVEN a pane that printed `$ nvim notes.txt`, then received `CSI ? 1049 h` and a full-screen draw in the same chunk
- WHEN its snapshot is restored, then `CSI ? 1049 l` is fed
- THEN the restored pane first shows the full-screen draw, and after the exit shows `$ nvim notes.txt` as before

#### Scenario: Modes survive [auto]
- GIVEN a pane with bracketed paste, SGR mouse, application cursor, focus reports and a steady bar cursor on
- WHEN its snapshot is restored
- THEN each of those reads the same on the new pane

#### Scenario: Restore sends no reply [auto]
- GIVEN a snapshot whose screen holds the text of a device-attributes query
- WHEN it is fed to a new pane
- THEN the bytes to answer the child are empty

#### Scenario: Bounded history [auto]
- GIVEN a pane with 10 000 history rows whose snapshot would exceed 8 MiB
- WHEN the snapshot is taken and restored
- THEN it is at most 8 MiB, the newest rows are kept, and the first restored row reads `[older scrollback not kept by the reload]`
