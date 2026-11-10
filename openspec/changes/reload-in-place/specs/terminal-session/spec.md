# Delta for Terminal Session

## Base of this delta

This delta targets `openspec/specs/terminal-session/spec.md`. It adds two requirements and changes none: "Shell exit and shutdown" (:126-157) still governs a quit, and a reload is not a quit. "Nesting guard" (:37-63) already holds after a reload because `exec` keeps the pid (probed, design D1); the added requirement pins it. Today the PTY masters are close-on-exec (portable-pty 0.9.0 `src/unix.rs:60-65`), so nothing below changes a pane that is not reloaded.

## ADDED Requirements

### Requirement: Adopted panes behave as spawned panes [S4]
A pane adopted after a reload MUST behave as a pane spawned in this process: its output is delivered ordered and by `PaneId`, it is resized, written to, probed for its folder and foreground program, and closed with the same SIGHUP, grace and SIGKILL teardown, inside the same 3-second quit bound. Its exit MUST be reported once, with the exit status for a pane that reports one. New panes opened after the reload are spawned as before.

#### Scenario: Resize reaches the adopted child [auto, unix PTY, real exec]
- GIVEN an adopted shell pane
- WHEN it is resized to 30 rows and 100 columns and `stty size` is typed
- THEN the pane shows `30 100`

#### Scenario: Quit after a reload leaves no orphan [auto, unix PTY, real exec]
- GIVEN two adopted shell panes
- WHEN the app quits
- THEN neither pid exists within 3 seconds

### Requirement: Nesting marker across a reload [S4]
A pane that keeps running across a reload MUST keep a `LAZAROBOX` value equal to the pid of the lazarobox that owns it after the reload, and a pane spawned after the reload MUST get the same value.

#### Scenario: Same marker [auto, unix PTY, real exec]
- GIVEN a shell pane spawned before the reload and one spawned after it
- WHEN `echo $LAZAROBOX` is typed in both
- THEN both print the pid of the process, unchanged by the reload
