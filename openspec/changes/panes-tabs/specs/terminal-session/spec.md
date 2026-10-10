# Delta for Terminal Session

Tags: [auto] = `cargo test` (fake PTY or unix PTY); [manual] = real Kitty.

## ADDED Requirements

### Requirement: Per-pane PTY registry
Every pane MUST own exactly one PTY child. Events and effects MUST be addressed by `PaneId`. Events from a pane that was already closed MUST be dropped silently. Output from panes MUST be delivered ordered per pane without blocking the UI loop.

#### Scenario: Output routed by id [auto]
- GIVEN two live panes
- WHEN pane 2's PTY produces output
- THEN only pane 2's screen changes

#### Scenario: Stale events dropped [auto]
- GIVEN pane 2 was closed
- WHEN a late output or exit event for pane 2 arrives
- THEN it is ignored with no panic and no effect

### Requirement: Inherited working directory
A new pane or tab MUST spawn its shell in the focused pane's effective cwd (the OSC 7 value once seen, otherwise the polled value). If that cwd is unknown, it MUST fall back to the launch cwd. A known cwd that no longer exists is not checked by the app: the spawn proceeds and the PTY layer falls back to `$HOME`; this is not a spawn failure.

#### Scenario: Split inherits cwd [auto]
- GIVEN the focused pane's cwd is `/tmp`
- WHEN a split is requested
- THEN the SpawnPane effect names `/tmp`

#### Scenario: Unknown cwd falls back [auto]
- GIVEN the focused pane has no known cwd
- WHEN a new tab is requested
- THEN the SpawnPane effect names the launch cwd

### Requirement: Spawn failure
The first pane is spawned before the UI starts, and a failure there MUST be fatal (the app exits with the error). If spawning a later pane (split or new tab) fails, the app MUST NOT crash: the pane MUST be removed through the normal close rules (sibling expands, focus per the close rule, an emptied tab closes) and the statusline MUST show a notice `spawn failed: {error}` in place of the cwd until the next key press.

#### Scenario: Failed split removes the pane [auto]
- GIVEN one pane and a split whose spawn fails
- WHEN the spawn-failed event arrives
- THEN the new pane is removed, the original pane fills the area and is focused, no Quit is emitted, and the notice is `spawn failed: {error}`

#### Scenario: Failed new tab [auto]
- GIVEN one tab and a `t n` whose spawn fails
- WHEN the spawn-failed event arrives
- THEN the new tab is removed, the first tab is active, and the notice is shown

#### Scenario: Notice clears on next key [auto]
- GIVEN a spawn-failure notice is shown
- WHEN any key is pressed
- THEN the notice is gone and the statusline shows the cwd again

#### Scenario: Bad cwd is not a failure [auto, unix PTY]
- GIVEN the requested cwd does not exist
- WHEN a pane is spawned
- THEN the shell starts (in `$HOME`) and no notice is shown

### Requirement: Non-blocking close
Closing a pane MUST NOT block the UI loop: the SIGHUP / grace / SIGKILL teardown MUST run off the event loop.

#### Scenario: UI stays responsive [auto, unix PTY]
- GIVEN a pane whose shell ignores SIGHUP
- WHEN the pane is closed
- THEN the ClosePane effect returns immediately, and the child is gone after the grace period

### Requirement: Per-pane cwd poll fallback
The runtime MUST keep the `/proc` cwd poll (~1 s) per pane as a fallback for panes that never reported an OSC 7 cwd. The effective cwd of a pane MUST be its last valid OSC 7 value if it has ever reported one; otherwise the polled value. Once a pane has reported OSC 7, a poll result MUST NOT override it (the poll may still run). A failed poll MUST keep the last value.

#### Scenario: Fallback for plain shell [auto, unix PTY]
- GIVEN a pane whose shell never emits OSC 7 and runs `cd /tmp`
- WHEN the next poll occurs
- THEN that pane's cwd is `/tmp`

#### Scenario: OSC 7 wins [auto]
- GIVEN a pane that reported OSC 7 `/a`
- WHEN a poll reads `/b`
- THEN the pane cwd remains `/a`

## MODIFIED Requirements

### Requirement: Shell spawn and environment
The system MUST spawn `$SHELL` (fallback `/bin/sh`) for each pane, in the pane's requested cwd (the launch cwd for the first pane), with `TERM=xterm-256color` and `COLORTERM=truecolor`, inheriting the rest of the environment.
(Previously: a single session in the launch cwd)

#### Scenario: SHELL set [auto]
- GIVEN `$SHELL=/bin/zsh`
- WHEN a pane is spawned
- THEN the spawn request names `/bin/zsh`, the requested cwd, and both TERM/COLORTERM values

#### Scenario: SHELL unset or empty [auto]
- GIVEN `$SHELL` is unset or empty
- WHEN a pane is spawned
- THEN the spawn request names `/bin/sh`

### Requirement: PTY I/O
Bytes written for a pane MUST reach that pane's child unmodified; child output MUST be delivered to the app as ordered per-pane events without blocking the UI loop.
(Previously: one child, one stream)

#### Scenario: Echo round trip [auto, unix PTY]
- GIVEN a live pane running `cat`
- WHEN `hello\n` is written
- THEN `hello` appears in that pane's output events within a timeout

### Requirement: Resize
Each pane's PTY size MUST equal its layout rect (minimum 1x1) after the statusline row and the tab bar row (when visible) are excluded. On any layout or host change, the emulator AND the PTY of a pane MUST be resized, and ResizePty MUST be emitted only for panes whose size changed (no SIGWINCH storm), including panes of inactive tabs (for example when the tab bar appears or disappears, or the host resizes). Panes hidden by zoom keep their last size.
(Previously: one pane sized (rows - 1) x cols)

#### Scenario: Resize propagates [auto]
- GIVEN a single pane on a 24x80 terminal
- WHEN it resizes to 40x100
- THEN the pane and PTY are resized to 39x100

#### Scenario: Degenerate size [auto]
- GIVEN any resize to 1 row
- WHEN pane sizes are computed
- THEN each is 1x1 at minimum (never zero or underflow)

#### Scenario: Only changed panes resized [auto]
- GIVEN two panes and a split-border move that changes both
- WHEN resize effects are computed
- THEN ResizePty is emitted for exactly those two, and a no-op layout change emits none

#### Scenario: Unaffected pane untouched [auto]
- GIVEN three panes and a resize of one column that does not touch pane C
- WHEN effects are computed
- THEN no ResizePty is emitted for pane C

#### Scenario: Hidden tabs stay sized [auto]
- GIVEN two tabs with panes on a 25-row terminal, the first tab active
- WHEN `t n` makes the tab bar appear
- THEN ResizePty is emitted for the panes of the first tab as well, with one row less

#### Scenario: Zoom resizes only the zoomed pane [auto]
- GIVEN two panes
- WHEN the focused pane is zoomed
- THEN ResizePty is emitted for the zoomed pane only, and unzooming emits it again to restore the size

#### Scenario: Reflow in nvim [manual]
- GIVEN nvim open in a pane
- WHEN the pane is resized with `w r` or the terminal is resized
- THEN nvim redraws to the new size

### Requirement: Shell exit and shutdown
When a pane's child exits, that pane MUST close (sibling expands; if it was focused, focus moves per the pane-layout close rule). When the last pane of a tab closes the tab MUST close; when the last tab closes the app MUST quit with the host terminal restored. On quit the host terminal MUST be restored first, so the user gets the prompt back immediately; then all child processes of all panes MUST be torn down in parallel, bounded to 3 seconds, after which no child process of any pane MUST remain. Terminal state MUST also be restored on panic.
(Previously: a single child exit quit the app)

#### Scenario: One of two shells exits [auto]
- GIVEN two panes with pane 2 focused
- WHEN pane 2's PTY-exited event arrives
- THEN pane 2 is closed, no Quit is emitted, and the other pane is focused

#### Scenario: Last shell exits [auto]
- GIVEN one tab with one pane
- WHEN its PTY-exited event arrives
- THEN a Quit effect is emitted

#### Scenario: Quit leaves no orphan [auto, unix PTY]
- GIVEN several running children across panes
- WHEN the app shuts down
- THEN the terminal is restored before the teardown wait, and none of the child pids exists within 3 seconds

Teardown sends SIGHUP to the shell's process group, waits a short grace so the shell can save history and run its traps, then sends SIGKILL to the group. Accepted limitation: jobs that an interactive shell with job control moved into their own process group are not reached and may outlive the app, as with tmux.

#### Scenario: Terminal restored [manual]
- GIVEN `exit` typed in the last pane, or prefix `q` then `y`
- WHEN the app ends
- THEN the host shell is usable (cooked mode, cursor visible, no alt screen, no mouse capture)

#### Scenario: No orphan shells after quit [manual, Kitty]
- GIVEN several panes and tabs open
- WHEN the app quits
- THEN `pgrep` / `ps` shows no leftover shells from the session

## REMOVED Requirements

(None. "No mouse capture" is unchanged.)
