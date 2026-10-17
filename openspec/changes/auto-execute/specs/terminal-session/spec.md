# Delta for Terminal Session

Applies on top of the unarchived `agent-tools` delta for terminal-session. Everything else stays (PTY I/O still delivers written bytes unmodified; a PTY effect addressed to a chat pane is still never produced). Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approved writes respect bracketed paste
A write requested by an approved pane command (see pane-commands) or by a run decided under pane-execution MUST be built from the target pane's current terminal state at the moment of the write: with bracketed paste enabled by the pane's application, the validated command is framed as one bracketed paste (start marker, command, end marker) in a single WritePty; with it disabled, a single-line command is written as plain text and a command with a newline or tab is refused before any byte is written. The command text inside the frame MUST NOT contain the end marker or any ESC (validation refuses it), so the frame cannot be closed early. The write MUST end with the command or the end marker and never add Enter, EXCEPT a run decided under pane-execution in `run` mode, whose single write appends exactly one `\r` after the command or the end marker and nothing else. The write MUST be addressed only to a live shell pane (never a chat pane) and its bytes reach the child unmodified like any other WritePty. Ordinary user typing and user pastes into a shell pane MUST be unchanged by this requirement.
(Previously: the write never added Enter in any case)

#### Scenario: Framed when enabled [auto]
- GIVEN the target pane's application enabled bracketed paste (mode 2004)
- WHEN an approved command `a\nb` is written
- THEN one WritePty carries `ESC[200~a\nbESC[201~` and nothing after it

#### Scenario: Plain when disabled [auto]
- GIVEN bracketed paste disabled and the command `cd /tmp`
- WHEN it is written after `y`
- THEN one WritePty carries exactly `cd /tmp`

#### Scenario: Run appends one carriage return [auto]
- GIVEN bracketed paste disabled and a run of `cd /tmp`, and in another run bracketed paste enabled
- WHEN each is written
- THEN one WritePty carries `cd /tmp\r`, and in the other run `ESC[200~cd /tmpESC[201~\r`

#### Scenario: Enter only in run [auto]
- GIVEN writes decided by `y`, by `auto` and by `run`
- WHEN the bytes are inspected
- THEN only the `run` write contains `\r`

#### Scenario: Multi-line refused when disabled [auto]
- GIVEN bracketed paste disabled and the command `a\nb`
- WHEN it is written
- THEN no WritePty is emitted

#### Scenario: State read at write [auto]
- GIVEN the pane toggled bracketed paste between the prompt (or the check) and the write
- WHEN the write happens
- THEN the framing follows the state at the write

#### Scenario: User paste unchanged [auto]
- GIVEN the user pastes text into a shell pane
- WHEN the paste is handled
- THEN the bytes are exactly as before this change

#### Scenario: Real paste in a shell [manual, Kitty]
- GIVEN bash and zsh with bracketed paste enabled
- WHEN a two-line approved command is written
- THEN each shows the whole command as one pasted buffer without running any line

## ADDED Requirements

### Requirement: Pane state for a run
For a pane with a run in flight the app MUST be able to answer, without blocking the event loop and without reading the PTY beyond its emulated screen: whether its foreground process is its shell (unknown is a distinct answer), its real current directory (unknown is a distinct answer), whether it entered the alternate screen since the write, the cursor line and the line generation. A pane that enters the alternate screen while a run is captured MUST abort the run (see pane-execution). Events of a closed pane remain dropped silently.

#### Scenario: Foreground and cwd answers [auto]
- GIVEN a pane whose foreground is its shell, and one whose foreground is `python3`, and one whose foreground cannot be read
- WHEN the state is read
- THEN it reports shell, other and unknown respectively; the cwd likewise reports a path or unknown

#### Scenario: Alternate screen during a run [auto]
- GIVEN a run in flight and the pane processes `ESC[?1049h`
- WHEN the state is read
- THEN it reports the alternate screen since the write and the run aborts

#### Scenario: Closed pane [auto]
- GIVEN a closed pane id
- WHEN the state is read
- THEN it reports that the pane does not exist
