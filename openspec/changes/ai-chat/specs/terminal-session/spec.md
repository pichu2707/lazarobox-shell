# Delta for Terminal Session

Tags: [auto] = `cargo test` (fake PTY or unix PTY test); [manual] = real nvim/htop/Kitty. The PTY registry, spawn, resize, write and the cwd poll now apply to shell (PTY) panes only; chat panes have no PTY (see pane-layout and chat-pane). Requirements not listed here are unchanged, and "pane" in them means a shell pane.

## MODIFIED Requirements

### Requirement: Shell spawn and environment
The system MUST spawn `$SHELL` (fallback `/bin/sh`) for each shell pane, in the pane's requested cwd (the launch cwd for the first pane), with `TERM=xterm-256color` and `COLORTERM=truecolor`, inheriting the rest of the environment. A chat pane MUST NOT spawn a shell.
(Previously: for each pane; a chat pane did not exist)

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

### Requirement: Per-pane PTY registry
Every shell pane MUST own exactly one PTY child; chat panes own none. PTY events and effects MUST be addressed by `PaneId`. Events from a pane that was already closed MUST be dropped silently. Output from shell panes MUST be delivered ordered per pane without blocking the UI loop. A PTY effect (WritePty, ResizePty, ClosePane for a PTY) addressed to a chat pane MUST never be produced.
(Previously: every pane owned a PTY child)

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

### Requirement: Per-pane cwd poll fallback
The runtime MUST keep the `/proc` cwd poll (~1 s) per shell pane as a fallback for panes that never reported an OSC 7 cwd. The effective cwd of a shell pane MUST be its last valid OSC 7 value if it has ever reported one; otherwise the polled value. Once a pane has reported OSC 7, a poll result MUST NOT override it (the poll may still run). A failed poll MUST keep the last value. Chat panes MUST NOT be polled; their cwd is the fixed origin cwd.
(Previously: per pane, with no chat panes)

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

## ADDED Requirements

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
