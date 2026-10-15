# Delta for Terminal Session

All existing requirements stay as they are (PTY I/O still delivers written bytes unmodified; a PTY effect addressed to a chat pane is still never produced). Tags as in the main spec.

## ADDED Requirements

### Requirement: Approved writes respect bracketed paste
A write requested by an approved pane command (see pane-commands) MUST be built from the target pane's current terminal state at the moment of accept: with bracketed paste enabled by the pane's application, the validated command is framed as one bracketed paste (start marker, command, end marker) in a single WritePty; with it disabled, a single-line command is written as plain text and a command with a newline or tab is refused before any byte is written. The command text inside the frame MUST NOT contain the end marker or any ESC (validation refuses it), so the frame cannot be closed early. The write MUST end with the command or the end marker and never add Enter. The write MUST be addressed only to a live shell pane (never a chat pane) and its bytes reach the child unmodified like any other WritePty. Ordinary user typing and user pastes into a shell pane MUST be unchanged by this requirement.

#### Scenario: Framed when enabled [auto]
- GIVEN the target pane's application enabled bracketed paste (mode 2004)
- WHEN an approved command `a\nb` is written
- THEN one WritePty carries `ESC[200~a\nbESC[201~` and nothing after it

#### Scenario: Plain when disabled [auto]
- GIVEN bracketed paste disabled and the command `cd /tmp`
- WHEN it is written
- THEN one WritePty carries exactly `cd /tmp`

#### Scenario: Multi-line refused when disabled [auto]
- GIVEN bracketed paste disabled and the command `a\nb`
- WHEN it is written
- THEN no WritePty is emitted

#### Scenario: State read at accept [auto]
- GIVEN the pane toggled bracketed paste between the prompt and the accept
- WHEN the accept happens
- THEN the framing follows the state at the accept

#### Scenario: User paste unchanged [auto]
- GIVEN the user pastes text into a shell pane
- WHEN the paste is handled
- THEN the bytes are exactly as before this change

#### Scenario: Real paste in a shell [manual, Kitty]
- GIVEN bash and zsh with bracketed paste enabled
- WHEN a two-line approved command is written
- THEN each shows the whole command as one pasted buffer without running any line

### Requirement: Pane state for accept-time rechecks
The app MUST be able to answer, for any shell pane, whether it exists, whether it is on the alternate screen, whether bracketed paste is enabled, and its effective cwd, from state it already tracks, without reading the PTY and without blocking the event loop. Events of a closed pane remain dropped silently.

#### Scenario: Alternate screen reported [auto]
- GIVEN a pane that processed `ESC[?1049h`
- WHEN the state is read
- THEN it reports the alternate screen, and after `ESC[?1049l` it does not

#### Scenario: Bracketed paste reported [auto]
- GIVEN a pane that processed `ESC[?2004h`, then `ESC[?2004l`
- WHEN the state is read after each
- THEN it reports enabled, then disabled

#### Scenario: Closed pane [auto]
- GIVEN a closed pane id
- WHEN the state is read
- THEN it reports that the pane does not exist

### Requirement: Shutdown with MCP processes
On quit, the MCP shims of all chats and the user MCP server processes (see mcp-servers) MUST be stopped inside the same 3-second bound as the PTY and engine teardown: process groups killed and reaped, session sockets removed, with the terminal restored first. Closing a chat pane or a tab MUST stop its shim and any user server no other chat uses, off the event loop, without blocking it.

#### Scenario: Quit leaves no MCP process [auto, unix PTY]
- GIVEN two chats with running fake shims and one user server
- WHEN the app shuts down
- THEN the terminal is restored first, none of those pids exists within 3 seconds and the session sockets are gone

#### Scenario: Closing a chat does not block [auto]
- GIVEN a chat whose user server ignores SIGTERM
- WHEN the chat pane is closed
- THEN the effect returns immediately and the server is gone after the grace period
