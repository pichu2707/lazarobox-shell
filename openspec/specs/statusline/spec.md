# Statusline Specification

## Purpose

Bottom row (always reserved) showing mode, cwd, shell. Existing statusline snapshots MUST remain valid.

## Requirements

### Requirement: Mode block
The statusline MUST show the current mode label (TERMINAL, PREFIX, COPY) styled with that mode's color from `mode_style`.

Mode colors (reusing the theme palette, via `input_mode_style`): TERMINAL → `success_green`, PREFIX → `warning_orange`, COPY → `primary_cyan`, ConfirmQuit → `error_red`.

#### Scenario: Label and color [auto, TestBackend + insta]
- GIVEN each mode
- WHEN rendered
- THEN the label text and mode color are shown

#### Scenario: Mode color mapping [auto]
- GIVEN each input mode
- WHEN `input_mode_style` is evaluated
- THEN TERMINAL uses `success_green`, PREFIX `warning_orange`, COPY `primary_cyan` and ConfirmQuit `error_red`

#### Scenario: Existing snapshots [auto]
- GIVEN the pre-existing statusline snapshots
- THEN they still pass unchanged

### Requirement: Quit confirmation visible
While confirmation is pending the statusline MUST show "Quit? (y/n)".

#### Scenario: Prompt [auto]
- GIVEN confirmation pending
- WHEN rendered
- THEN "Quit? (y/n)" is visible

### Requirement: Live cwd
The statusline MUST show the child's current working directory, refreshed periodically (~1 s), and keep the last value if it cannot be read.

#### Scenario: cwd changes [auto, unix PTY]
- GIVEN the shell runs `cd /tmp`
- WHEN the next poll occurs
- THEN the statusline cwd is `/tmp`

#### Scenario: Read failure [auto]
- GIVEN the cwd lookup fails
- THEN the previous value is retained (no panic)

### Requirement: Shell segment
The right side MUST show the shell name (basename of the shell path).

#### Scenario: Shell name [auto]
- GIVEN shell `/bin/zsh`
- WHEN rendered
- THEN `zsh` appears on the right

#### Scenario: Narrow width [auto]
- GIVEN a very narrow terminal
- WHEN rendered
- THEN the mode block takes priority and rendering does not panic
