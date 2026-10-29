# Delta for Terminal Session

## MODIFIED Requirements

### Requirement: Shell spawn and environment
The system MUST spawn `$SHELL` (fallback `/bin/sh`) for each shell pane, in the pane's requested cwd (the launch cwd for the first pane), with `TERM=xterm-256color` and `COLORTERM=truecolor`, inheriting the rest of the environment. The launch cwd is the start dir (see start-dir): the `--dir` flag, else `[workspace] dir`, else the process working directory. A chat pane MUST NOT spawn a shell.
(Previously: the launch cwd was always the process working directory)

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

#### Scenario: First pane starts in the start dir [auto]
- GIVEN a start dir `/w/proj` resolved from `--dir` and a process cwd `/h`
- WHEN the first shell pane is spawned
- THEN the spawn request names `/w/proj`

#### Scenario: Start dir from the launcher [manual]
- GIVEN a launcher installed with `install-desktop --dir /w/proj`
- WHEN lazarobox is started from it
- THEN the first shell's `pwd` is `/w/proj`

### Requirement: Per-pane cwd poll fallback
The runtime MUST keep the `/proc` cwd poll (~1 s) per shell pane as a fallback for panes that never reported an OSC 7 cwd. The effective cwd of a shell pane MUST be its last valid OSC 7 value if it has ever reported one; otherwise the polled value. Once a pane has reported OSC 7, a poll result MUST NOT override it (the poll may still run). A failed poll MUST keep the last value. Chat panes MUST NOT be polled; their cwd is the chat folder, which changes only through the picker or a confirmed `/cd` (see chat-folder), never by polling.
(Previously: a chat pane's cwd was the fixed origin cwd)

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
- THEN no poll targets the chat pane and its folder is still `/a`

#### Scenario: Chat folder changes only by command [auto]
- GIVEN a chat pane in `/a` whose confirmed `/cd` set `/b`
- WHEN a poll cycle runs
- THEN its folder is `/b`

### Requirement: Inherited working directory from a chat pane
When the focused pane is a chat pane, a new pane or tab MUST spawn its shell in the chat's current folder (the folder at the moment of the request, including after a `/cd`), with the same fallback chain as for a shell pane (launch cwd, then an absolute `$HOME`, then `/`, the first that exists). A chat whose folder is unknown (no choice made) uses that fallback chain.
(Previously: the chat's fixed origin cwd)

#### Scenario: Split from a chat inherits its folder [auto]
- GIVEN a focused chat pane with folder `/tmp`
- WHEN `w v` is pressed
- THEN the SpawnPane effect names `/tmp`

#### Scenario: New tab from a chat [auto]
- GIVEN a focused chat pane with folder `/tmp`
- WHEN `t n` is pressed
- THEN the SpawnPane effect names `/tmp`

#### Scenario: Vanished folder falls back [auto]
- GIVEN a focused chat pane whose folder was removed
- WHEN a split is requested
- THEN the SpawnPane effect names the launch cwd, or an absolute `$HOME`, or `/`

#### Scenario: Follows a `/cd` [auto]
- GIVEN a focused chat pane whose folder was changed from `/a` to `/b`
- WHEN `w v` is pressed
- THEN the SpawnPane effect names `/b`
