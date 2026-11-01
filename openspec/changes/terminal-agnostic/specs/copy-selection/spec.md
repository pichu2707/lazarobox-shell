# Delta for copy-selection

## MODIFIED Requirements

### Requirement: Yank to the clipboard via OSC 52
`y` with a selection MUST emit one `SetClipboard` effect carrying the OSC 52 bytes `ESC ] 52 ; c ; <base64 of the UTF-8 text> BEL`, and the runtime MUST always write them to the host terminal. Because a terminal never confirms OSC 52, when a local clipboard tool fits the session the app MUST also emit one `CopyLocal` effect carrying the extracted text, and the runtime MUST run one tool chosen from `wl-copy`, `xclip`, `xsel` and `clip.exe` (first one present, in that order of fit for the session). When `SSH_CONNECTION` is set, no `CopyLocal` is emitted and no local tool runs: OSC 52 only. After `y` the mode MUST be TERMINAL with the viewport at the bottom, and the selection dropped. `y` with no selection MUST do nothing. A yank whose extracted text is empty MUST send nothing (no `SetClipboard`), keep COPY and the selection, and show the notice `Nothing to copy`. No key in COPY, including `y`, MUST be written to the PTY.

Every copy MUST show a notice, and the notice MUST say honestly what happened:
- `Copied (<tool>)` when a local tool ran and succeeded (`<tool>` is its name, e.g. `wl-copy`).
- On a local desktop session with no tool found: `No paste? Install wl-copy or xclip (OSC 52 sent)`.
- Over SSH (tools skipped): `No paste? Enable OSC 52 in your terminal (sent)`.
- `<tool> failed; sent via OSC 52 (<reason>)` when the tool was started and failed. OSC 52 was still sent, and the notice MUST NOT claim a successful copy.
- `<tool> failed; not copied: too large for OSC 52 (<reason>)` when the tool failed and the text was too large for OSC 52, so nothing was sent to the terminal.

A copy MUST NOT end without a notice, whichever caller started it (COPY yank, chat copy, feedback, update). The same fallback chain and notices apply to every caller of the copy path.
(Previously: OSC 52 only, no notice on success)

#### Scenario: Yank emits OSC 52
- GIVEN the extracted text `hello` and no local tool available
- WHEN `y` is pressed
- THEN the written sequence is `\x1b]52;c;aGVsbG8=\x07`, mode is TERMINAL, the viewport is at the bottom and no WritePty is emitted

#### Scenario: Local tool ran [auto]
- GIVEN the extracted text `hello`, `SSH_CONNECTION` unset, and `wl-copy` available and succeeding
- WHEN `y` is pressed
- THEN OSC 52 is written, `wl-copy` receives `hello`, and the notice is `Copied (wl-copy)`

#### Scenario: Tool order [auto]
- GIVEN both `wl-copy` and `xclip` are available
- WHEN `y` is pressed
- THEN only `wl-copy` runs and the notice names `wl-copy`

#### Scenario: Only OSC 52 sent [auto]
- GIVEN a local desktop session and no local tool is available
- WHEN `y` is pressed
- THEN the notice is `No paste? Install wl-copy or xclip (OSC 52 sent)`

#### Scenario: SSH skips local tools [auto]
- GIVEN `SSH_CONNECTION` is set and `wl-copy` is available
- WHEN `y` is pressed
- THEN `wl-copy` is not run, OSC 52 is written and the notice is `No paste? Enable OSC 52 in your terminal (sent)`

#### Scenario: Tool fails [auto]
- GIVEN `wl-copy` is available but exits with an error
- WHEN `y` is pressed
- THEN OSC 52 is still written, the notice is `wl-copy failed; sent via OSC 52 (<reason>)`, and the notice does not start with `Copied`

#### Scenario: Tool fails on a selection too large for OSC 52 [auto]
- GIVEN `wl-copy` is available and the extracted text is larger than one OSC 52 sequence allows
- WHEN `y` is pressed and `wl-copy` exits with an error
- THEN nothing was sent to the terminal, and the notice is `wl-copy failed; not copied: too large for OSC 52 (<reason>)`, never `sent via OSC 52`

#### Scenario: Other callers get the same notice [auto]
- GIVEN a chat copy, a feedback copy or an update copy goes through the copy path
- WHEN the app and runtime handle it
- THEN the same fallback chain runs and the same notice rules apply

#### Scenario: Yank of blank rows
- GIVEN a line selection over rows that are all blank
- WHEN `y` is pressed
- THEN no effect is emitted, mode stays COPY, the selection is kept and the notice is `Nothing to copy`

#### Scenario: Yank without selection
- GIVEN COPY with no selection
- WHEN `y` is pressed
- THEN no effect is emitted and mode stays COPY

#### Scenario: Paste elsewhere [manual, terminal records]
- GIVEN `v`, a motion, then `y`
- WHEN pasting into another application
- THEN the pasted text equals the selection exactly
- AND the result is written in the terminal's record

#### Scenario: Line yank runs nothing [manual, terminal records]
- GIVEN a line yank of a shell command
- WHEN pasting into a shell prompt
- THEN the text appears without a trailing newline and nothing runs

#### Scenario: Yank over ssh [manual, terminal records]
- GIVEN lazarobox running in an ssh session
- WHEN `y` is pressed on a selection
- THEN the text reaches the local clipboard when the terminal supports OSC 52, and the notice is the SSH OSC 52 one (`... enable OSC 52 in your terminal`)
