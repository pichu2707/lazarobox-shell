# desktop-launcher Specification

## Purpose
Launching lazarobox is one step. A TUI always needs a host terminal, so the launcher provides it.

## ADDED Requirements

### Requirement: install-desktop
`install-desktop` MUST create a desktop entry that launches lazarobox in a host terminal in one step. Its result MUST always be shown to the user: success with the path written, or a failure with the reason. It MUST NOT fail or ignore a problem silently. Running it again MUST leave one valid entry, not duplicates.

#### Scenario: Install on Linux [auto]
- GIVEN a Linux session
- WHEN `install-desktop` runs
- THEN a desktop entry is written and the message names its path

#### Scenario: Install twice [auto]
- GIVEN an entry already installed
- WHEN `install-desktop` runs again
- THEN one valid entry remains and the message says so

#### Scenario: Install fails [auto]
- GIVEN the target folder cannot be written
- WHEN `install-desktop` runs
- THEN the message states the failure and the reason, and the exit is non-zero

### Requirement: WSL launch in one step
In WSL, `install-desktop` MUST use the Linux launcher path, so the user launches lazarobox in one step with no manual "open WSL, then type the command". The result message MUST say where the entry was written. If the host does not list the entry in the Windows Start menu (WSLg), the message and the README MUST say so instead of promising it. A Windows `.exe` launcher is not part of this requirement.

#### Scenario: Install in WSL [auto]
- GIVEN a WSL session
- WHEN `install-desktop` runs
- THEN the same entry as on Linux is written and the message names its path

#### Scenario: Start menu not available [auto]
- GIVEN a WSL session where the entry is not shown in the Start menu
- WHEN `install-desktop` runs
- THEN the message states that the entry may not appear in the Start menu and how to launch it otherwise

#### Scenario: One-step launch in WSL [manual, Windows Terminal (WSL)]
- GIVEN a Windows machine with WSL and the entry installed
- WHEN the user starts lazarobox from the entry
- THEN lazarobox opens in a terminal with no further typing, and the result is written in the Windows Terminal (WSL) record

### Requirement: Interactive terminal picker
`install-desktop` MUST ask the user which terminal to use only when stdin and stdout are both a TTY and `--terminal` was not passed. The list MUST contain only installed candidates (the known terminals found on PATH, plus `$TERMINAL` and the resolved `x-terminal-emulator` target), without duplicates. Each entry that has a record under `docs/terminals/` MUST be marked as tested. The default MUST be the terminal that the automatic choice picks today, so Enter keeps the current behaviour. Invalid input MUST ask again or show a notice; it MUST NOT fail silently. Ctrl+D or EOF MUST cancel, write nothing and say so. Without a TTY, or with `--terminal`, there MUST be no prompt and the behaviour MUST be unchanged. Finding candidates MUST NOT run the terminals (no `--version` probes), because some terminals open a window.

#### Scenario: Enter keeps the automatic choice [auto]
- GIVEN a TTY, no `--terminal`, and two installed candidates
- WHEN the user presses Enter
- THEN the entry is written for the terminal the automatic choice picks

#### Scenario: Choose another terminal [auto]
- GIVEN the picker shows three installed candidates
- WHEN the user types the number of the second one
- THEN the entry is written for that terminal

#### Scenario: Only installed and unique candidates [auto]
- GIVEN `$TERMINAL` names a terminal that is also on PATH, and another known terminal is not installed
- WHEN the picker list is built
- THEN each installed terminal appears once and the missing one does not appear

#### Scenario: Tested terminals are marked [auto]
- GIVEN `kitty` has a record and `foot` has none
- WHEN the list is shown
- THEN `kitty` is marked tested and `foot` is not

#### Scenario: Invalid input [auto]
- GIVEN the picker is waiting
- WHEN the user types `abc` or a number outside the list
- THEN a notice says the input is not valid and the picker asks again

#### Scenario: Cancel [auto]
- GIVEN the picker is waiting
- WHEN the user presses Ctrl+D (EOF)
- THEN nothing is written, the message says it was cancelled, and the exit is non-zero

#### Scenario: No TTY or explicit terminal [auto]
- GIVEN stdin is not a TTY, or `--terminal kitty` was passed
- WHEN `install-desktop` runs
- THEN no prompt is shown and the entry is written as before

#### Scenario: No version probes [auto]
- GIVEN the candidate list is being built
- WHEN a candidate is found on PATH
- THEN the terminal binary is not executed

### Requirement: Record-aware notice
Any launcher or startup notice about the host terminal MUST say whether the terminal has a record under `docs/terminals/`. It MUST NOT say "tested in Kitty". A terminal without a record MUST be called untested, and MUST NOT be refused.

#### Scenario: Terminal with a record [auto]
- GIVEN the host terminal is one with a record
- WHEN the notice is built
- THEN it says the terminal has a record

#### Scenario: Terminal without a record [auto]
- GIVEN the host terminal has no record
- WHEN the notice is built
- THEN it says the terminal is untested and launch continues

#### Scenario: Old wording is gone [auto]
- GIVEN any launcher notice
- WHEN its text is checked
- THEN it does not contain "tested in Kitty"
