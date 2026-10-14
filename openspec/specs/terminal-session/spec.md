# Terminal Session Specification

## Purpose

Lifecycle of the single PTY child: spawn, environment, I/O, resize, exit, shutdown. Tags: [auto] = `cargo test` (fake PTY or unix PTY test); [manual] = real nvim/htop/Kitty.

## Requirements

### Requirement: Shell spawn and environment
The system MUST spawn `$SHELL` (fallback `/bin/sh`) for each shell pane, in the pane's requested cwd (the launch cwd for the first pane), with `TERM=xterm-256color` and `COLORTERM=truecolor`, inheriting the rest of the environment. A chat pane MUST NOT spawn a shell.

#### Scenario: SHELL set [auto]
- GIVEN `$SHELL=/bin/zsh`
- WHEN a shell pane is spawned
- THEN the spawn request names `/bin/zsh`, the requested cwd, and both TERM/COLORTERM values

#### Scenario: SHELL unset or empty [auto]
- GIVEN `$SHELL` is unset or empty
- WHEN a shell pane is spawned
- THEN the spawn request names `/bin/sh`

#### Scenario: Chat pane spawns nothing [auto]
- GIVEN a chat pane is opened
- WHEN the effects are computed
- THEN no SpawnPane effect is emitted

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
When a pane's child exits, that pane MUST close (sibling expands; if it was focused, focus moves per the pane-layout close rule). When the last pane of a tab closes the tab MUST close; when the last tab closes the app MUST quit with the host terminal restored. On quit the host terminal MUST be restored first, including disabling mouse capture when it was enabled (see mouse-input), so the user gets the prompt back immediately; then all child processes of all panes MUST be torn down in parallel, bounded to 3 seconds, after which no child process of any pane MUST remain. Terminal state, including mouse capture, MUST also be restored on panic.

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

#### Scenario: Terminal restored [manual]
- GIVEN `exit` typed in the last pane, or prefix `q` then `y`
- WHEN the app ends
- THEN the host shell is usable (cooked mode, cursor visible, no alt screen, mouse capture off: moving the wheel prints nothing)

#### Scenario: Mouse disabled during restore [auto]
- GIVEN mouse capture enabled
- WHEN the app quits, and in another run panics
- THEN `\x1b[?1006l\x1b[?1000l` is written as part of the restore in both cases

#### Scenario: No orphan shells after quit [manual, Kitty]
- GIVEN several panes and tabs open
- WHEN the app quits
- THEN `pgrep` / `ps` shows no leftover shells from the session

### Requirement: Per-pane PTY registry
Every shell pane MUST own exactly one PTY child; chat panes own none. PTY events and effects MUST be addressed by `PaneId`. Events from a pane that was already closed MUST be dropped silently. Output from shell panes MUST be delivered ordered per pane without blocking the UI loop. A PTY effect (WritePty, ResizePty, ClosePane for a PTY) addressed to a chat pane MUST never be produced.

#### Scenario: Output routed by id [auto]
- GIVEN two live shell panes
- WHEN pane 2's PTY produces output
- THEN only pane 2's screen changes

#### Scenario: Stale events dropped [auto]
- GIVEN pane 2 was closed
- WHEN a late output or exit event for pane 2 arrives
- THEN it is ignored with no panic and no effect

#### Scenario: No PTY effect for a chat pane [auto]
- GIVEN a focused chat pane in TERMINAL
- WHEN keys, a paste and a host resize are handled
- THEN no WritePty or ResizePty effect addresses the chat pane

### Requirement: Inherited working directory
A new pane or tab MUST spawn its shell in the focused pane's effective cwd (the OSC 7 value once seen, otherwise the polled value). If that cwd is unknown, or is not an existing directory at spawn time, it MUST fall back to the launch cwd, then to an absolute `$HOME`, then to `/`, taking the first that exists. This is a precedence preference (the launch cwd over `$HOME`), not a failure fix: the PTY layer already starts the shell in `$HOME` when the cwd is not a directory. The existence check is injected into the app (the core does no IO); the runtime passes `Path::is_dir`.

#### Scenario: Split inherits cwd [auto]
- GIVEN the focused pane's cwd is `/tmp`
- WHEN a split is requested
- THEN the SpawnPane effect names `/tmp`

#### Scenario: Vanished cwd falls back [auto]
- GIVEN the focused pane's cwd was `/tmp/x` and the directory has since been removed
- WHEN a split or a new tab is requested
- THEN the SpawnPane effect names the launch cwd, or an absolute `$HOME` when the launch cwd is gone too, or `/` when nothing exists

#### Scenario: Existing cwd is inherited unchanged [auto]
- GIVEN the focused pane's cwd exists
- WHEN a split is requested
- THEN the SpawnPane effect names that cwd

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
The runtime MUST keep the `/proc` cwd poll (~1 s) per shell pane as a fallback for panes that never reported an OSC 7 cwd. The effective cwd of a shell pane MUST be its last valid OSC 7 value if it has ever reported one; otherwise the polled value. Once a pane has reported OSC 7, a poll result MUST NOT override it (the poll may still run). A failed poll MUST keep the last value. Chat panes MUST NOT be polled; their cwd is the fixed origin cwd.

#### Scenario: Fallback for plain shell [auto, unix PTY]
- GIVEN a pane whose shell never emits OSC 7 and runs `cd /tmp`
- WHEN the next poll occurs
- THEN that pane's cwd is `/tmp`

#### Scenario: OSC 7 wins [auto]
- GIVEN a pane that reported OSC 7 `/a`
- WHEN a poll reads `/b`
- THEN the pane cwd remains `/a`

#### Scenario: Chat pane is not polled [auto]
- GIVEN a chat pane opened from `/a`
- WHEN a poll cycle runs
- THEN no poll targets the chat pane and its origin cwd is still `/a`

### Requirement: Inherited working directory from a chat pane
When the focused pane is a chat pane, a new pane or tab MUST spawn its shell in the chat's origin cwd, with the same fallback chain as for a shell pane (launch cwd, then an absolute `$HOME`, then `/`, the first that exists).

#### Scenario: Split from a chat inherits its origin cwd [auto]
- GIVEN a focused chat pane with origin cwd `/tmp`
- WHEN `w v` is pressed
- THEN the SpawnPane effect names `/tmp`

#### Scenario: New tab from a chat [auto]
- GIVEN a focused chat pane with origin cwd `/tmp`
- WHEN `t n` is pressed
- THEN the SpawnPane effect names `/tmp`

#### Scenario: Vanished origin cwd falls back [auto]
- GIVEN a focused chat pane whose origin cwd was removed
- WHEN a split is requested
- THEN the SpawnPane effect names the launch cwd, or an absolute `$HOME`, or `/`

### Requirement: Animation tick only while streaming
The animation of the thinking indicator (see statusline) MUST be driven by a single tick source that runs only while at least one chat stream is running in any pane or tab. With no running stream there MUST be no animation timer firing, no tick event and no redraw caused by the indicator, so an idle app costs no extra wakeups or CPU. The tick rate MUST be bounded to at most 12 ticks per second; the proposed value is 10 per second (a 100 ms period), for design to confirm. Several simultaneous streams MUST share one tick source and never raise the rate. The tick MUST start when the first stream starts and MUST stop within one tick period after the last stream ends by final, error, cut, cancel, pane close, tab close or quit. Each tick advances the animation frame and requests at most one redraw; ticks that were missed because the event loop was busy MUST be skipped, not replayed in a burst. Elapsed time MUST NOT depend on the number of ticks.

#### Scenario: No tick when idle [auto, paused clock]
- GIVEN an app with chat panes open and no running stream, and a paused tokio clock
- WHEN 10 seconds of virtual time pass
- THEN no tick event is produced and no redraw is requested

#### Scenario: Tick rate while streaming [auto, paused clock]
- GIVEN one running stream and a paused tokio clock
- WHEN exactly 1 second of virtual time passes
- THEN exactly 10 tick events are produced (at the proposed 100 ms period) and never more than 12

#### Scenario: Several streams share one tick [auto, paused clock]
- GIVEN three running streams in different panes and tabs
- WHEN 1 second of virtual time passes
- THEN the number of ticks equals that of one stream and the redraws requested by the indicator are at most 12

#### Scenario: Tick stops after the last stream [auto, paused clock]
- GIVEN two running streams of which one ends by final and the other by cancel
- WHEN time passes after each end
- THEN ticks continue after the first end, and no tick is produced more than one period after the second end

#### Scenario: Pane close, tab close, error and quit stop the tick [auto, paused clock]
- GIVEN a running stream ended in turn by closing its pane, closing its tab, an error event and a quit
- WHEN time passes after each
- THEN no tick is produced more than one period later

#### Scenario: Tick restarts with a new stream [auto, paused clock]
- GIVEN an idle app that has had no tick for 5 seconds
- WHEN a new stream starts and 200 ms pass
- THEN ticks are produced again, at the bounded rate

#### Scenario: Missed ticks are not replayed [auto, paused clock]
- GIVEN a running stream and an event loop that does not poll for 5 virtual seconds
- WHEN it polls again
- THEN at most one pending tick is delivered, not fifty

#### Scenario: Tick advances the frame and redraws once [auto]
- GIVEN a running stream
- WHEN one tick is handled
- THEN the animation frame advances by one and exactly one redraw is requested

### Requirement: Shutdown with active chats
On quit, running chat streams MUST be cancelled: server connections closed and CLI child processes (with their process groups) killed and reaped. This MUST happen after the host terminal is restored and inside the same 3-second bound as the PTY teardown, so no engine child process remains. Closing a chat pane or a tab MUST do the same off the event loop without blocking it.

#### Scenario: Quit kills CLI children [auto, unix PTY]
- GIVEN two chat panes streaming through fake CLI processes
- WHEN the app shuts down
- THEN the terminal is restored first and none of the fake CLI pids exists within 3 seconds

#### Scenario: Quit closes server sockets [auto]
- GIVEN a chat pane streaming from a fake server
- WHEN the app shuts down
- THEN the fake server observes the connection closing

#### Scenario: Closing a pane does not block [auto]
- GIVEN a chat pane whose CLI ignores SIGTERM
- WHEN the pane is closed
- THEN the ChatCancel effect returns immediately and the child is gone after the grace period
