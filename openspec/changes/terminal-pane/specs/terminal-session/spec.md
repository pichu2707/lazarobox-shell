# Terminal Session Specification

## Purpose

Lifecycle of the single PTY child: spawn, environment, I/O, resize, exit, shutdown. Tags: [auto] = `cargo test` (fake PTY or unix PTY test); [manual] = real nvim/htop/Kitty.

## Requirements

### Requirement: Shell spawn and environment
The system MUST spawn `$SHELL` (fallback `/bin/sh`) in the launch cwd with `TERM=xterm-256color` and `COLORTERM=truecolor`, inheriting the rest of the environment.

#### Scenario: SHELL set [auto]
- GIVEN `$SHELL=/bin/zsh`
- WHEN a session starts
- THEN the spawn request names `/bin/zsh`, the launch cwd, and both TERM/COLORTERM values

#### Scenario: SHELL unset or empty [auto]
- GIVEN `$SHELL` is unset or empty
- WHEN a session starts
- THEN the spawn request names `/bin/sh`

### Requirement: PTY I/O
Bytes written by the app MUST reach the child unmodified; child output MUST be delivered to the app as ordered events without blocking the UI loop.

#### Scenario: Echo round trip [auto, unix PTY]
- GIVEN a live session running `cat`
- WHEN `hello\n` is written
- THEN `hello` appears in output events within a timeout

### Requirement: Resize
Pane size MUST be (rows − 1) x cols, minimum 1x1. On terminal resize the emulator AND the PTY MUST both be resized.

#### Scenario: Resize propagates [auto]
- GIVEN a 24x80 terminal
- WHEN it resizes to 40x100
- THEN the pane and PTY are resized to 39x100

#### Scenario: Degenerate size [auto]
- GIVEN any terminal resize to 1 row
- WHEN the pane size is computed
- THEN it is 1x1 at minimum (never zero or underflow)

#### Scenario: Reflow in nvim [manual]
- GIVEN nvim open
- WHEN the terminal is resized
- THEN nvim redraws to the new size

### Requirement: Shell exit and shutdown
When the child exits the app MUST quit with the host terminal restored. On quit no child process MUST remain, and terminal state MUST be restored also on panic.

#### Scenario: Shell exits [auto]
- GIVEN a running session
- WHEN a PTY-exited event arrives
- THEN a Quit effect is emitted

#### Scenario: Quit leaves no orphan [auto, unix PTY]
- GIVEN a running child
- WHEN the session is dropped/shut down
- THEN the child pid no longer exists

#### Scenario: Terminal restored [manual]
- GIVEN `exit` typed, or prefix `q` then `y`
- WHEN the app ends
- THEN the host shell is usable (cooked mode, cursor visible, no alt screen, no mouse capture)

### Requirement: No mouse capture
The system MUST NOT enable mouse capture.

#### Scenario: Native selection [manual]
- GIVEN the app running in Kitty
- WHEN the user drags with the mouse
- THEN Kitty native selection works
