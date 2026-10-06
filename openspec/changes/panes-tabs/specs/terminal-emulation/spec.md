# Delta for Terminal Emulation

Each pane owns its own emulator. "Screen state" and "Query responder" requirements are unchanged and apply per pane.

## ADDED Requirements

### Requirement: OSC 7 working directory
The emulator MUST capture `ESC ] 7 ; <uri> BEL` and `ESC ] 7 ; <uri> ESC \` (ST) and expose the validated path as the pane's cwd. The payload is untrusted. The implementation MUST rejoin parameters split on `;`, percent-decode the path, accept only `file://` URIs with an empty host or a local host (`localhost` or this machine's hostname, compared ASCII case-insensitively), and accept only absolute paths. A payload (`7;<uri>`) of 1024 bytes or more MUST be ignored, because the parser may have truncated it. Paths are bytes: a percent-decoded path that is not valid UTF-8 is accepted. Anything malformed (bad scheme, remote host, relative path, invalid percent-escape, embedded NUL or other control bytes, empty path, oversize payload) MUST be ignored, leaving the previous cwd unchanged, with no panic and no query reply. Sequences split across reads MUST work.

#### Scenario: BEL terminator [auto]
- GIVEN `\e]7;file://myhost/home/u/proj\x07` and a local host `myhost`
- WHEN fed
- THEN the pane cwd is `/home/u/proj`

#### Scenario: ST terminator [auto]
- GIVEN `\e]7;file:///tmp\e\\`
- WHEN fed
- THEN the pane cwd is `/tmp`

#### Scenario: Percent-decoding [auto]
- GIVEN `\e]7;file:///tmp/my%20dir\x07`
- WHEN fed
- THEN the pane cwd is `/tmp/my dir`

#### Scenario: Semicolon in path [auto]
- GIVEN `\e]7;file:///tmp/a;b\x07` (the parser splits params on `;`)
- WHEN fed
- THEN the pane cwd is `/tmp/a;b`

#### Scenario: Remote host rejected [auto]
- GIVEN a cwd previously `/tmp` and `\e]7;file://other.example/etc\x07`
- WHEN fed
- THEN the cwd remains `/tmp`

#### Scenario: Malformed ignored [auto]
- GIVEN a prior cwd `/tmp`
- WHEN each of `file://host`, `http:///x`, `file://relative/path`, `file:///a%zz`, `file:///a%00b`, `file:///a%0ab`, and a payload of 1024 bytes or more is fed
- THEN the cwd remains `/tmp` and no panic occurs

#### Scenario: Local hosts accepted [auto]
- GIVEN local host `myhost`
- WHEN `file:///a`, `file://localhost/a`, `file://LOCALHOST/a` and `file://MyHost/a` are fed in turn
- THEN each sets the cwd to `/a`

#### Scenario: Non-UTF-8 path bytes [auto]
- GIVEN `\e]7;file:///bad%ff\x07`
- WHEN fed
- THEN the pane cwd is the path `/bad` followed by the byte 0xFF, and no panic occurs

#### Scenario: Split across reads [auto]
- GIVEN `\e]7;file:///t` and `mp\x07` arrive in separate chunks
- THEN the pane cwd is `/tmp`

#### Scenario: No query reply [auto]
- GIVEN a valid OSC 7 is fed
- THEN no reply bytes are produced

#### Scenario: cwd follows `cd` [manual, Kitty]
- GIVEN a shell configured to emit OSC 7
- WHEN the user runs `cd /tmp`
- THEN the statusline shows `/tmp` immediately, without waiting for the poll

### Requirement: Focused-only cursor
Only the focused pane MUST place the real cursor, and only the focused pane's DECSCUSR shape MUST be applied to the outer terminal. Unfocused panes MUST NOT draw or move the cursor. Changing focus MUST apply the new focused pane's last requested shape (or the Default shape when it never requested one).

#### Scenario: Unfocused pane has no cursor [auto]
- GIVEN two panes, both with visible cursors, the left focused
- WHEN rendered
- THEN the cursor position is inside the left pane only

#### Scenario: Shape follows focus [auto]
- GIVEN the left pane requested Bar steady and the right requested Block steady
- WHEN focus moves from left to right
- THEN the effective shape is Block steady

#### Scenario: Unfocused shape change ignored [auto]
- GIVEN the right pane unfocused
- WHEN it feeds `\e[6 q`
- THEN the effective outer shape does not change

## MODIFIED Requirements

### Requirement: Cursor shape (DECSCUSR)
Each pane's emulator MUST capture the cursor style requested by its child via DECSCUSR (`CSI Ps SP q`) and expose it as a `CursorShape` value (`Default`, or a shape of Block/Underline/Bar plus blinking/steady). Mapping: Ps 0 → Default; 1 → Block blinking; 2 → Block steady; 3 → Underline blinking; 4 → Underline steady; 5 → Bar blinking; 6 → Bar steady. Unknown Ps values MUST be ignored (shape unchanged). The runtime MUST apply the effective shape of the focused pane to the outer terminal with crossterm `SetCursorStyle` and MUST reset it to the user's default shape on exit and on panic. The effective shape is computed by a pure function of app state (see the copy-mode-scrollback spec for the COPY behavior).
(Previously: a single emulator; the effective shape came from the only pane)

#### Scenario: Bar steady [auto]
- GIVEN `\e[6 q` is fed
- THEN the cursor shape is Bar, steady

#### Scenario: Bar blinking [auto]
- GIVEN `\e[5 q` is fed
- THEN the cursor shape is Bar, blinking

#### Scenario: Block steady [auto]
- GIVEN `\e[2 q` is fed
- THEN the cursor shape is Block, steady

#### Scenario: Default [auto]
- GIVEN `\e[6 q` and then `\e[0 q` are fed
- THEN the cursor shape is Default

#### Scenario: Unknown parameter [auto]
- GIVEN `\e[6 q` and then `\e[9 q` are fed
- THEN the cursor shape remains Bar, steady (no panic)

#### Scenario: Split across reads [auto]
- GIVEN `\e[6 ` and `q` arrive in separate chunks
- THEN the cursor shape is Bar, steady

#### Scenario: No query reply [auto]
- GIVEN `\e[6 q` is fed
- THEN no reply bytes are produced

#### Scenario: Restore on exit [auto, runtime executor with fake writer]
- GIVEN the app has applied a Bar shape
- WHEN the runtime shuts down (normal exit or panic hook)
- THEN `DefaultUserShape` is emitted as part of the terminal restore

#### Scenario: Cursor shape in nvim [manual]
- GIVEN nvim running in the focused pane inside Kitty
- WHEN entering insert mode, then pressing Esc
- THEN the cursor is a bar in insert mode and a block in normal mode, and after quitting nvim and exiting the app the Kitty cursor is back to its configured default

### Requirement: Terminal view rendering
The view MUST render each pane's emulator screen into that pane's layout rect, clipped to it (colors and attributes mapped; the default background comes from the theme, while the default foreground is the host terminal's default (`Reset`) because the theme defines no foreground; wide-char continuation cells skipped). The real cursor MUST be placed, offset to the pane rect, only for the focused pane and only unless hidden.
(Previously: rendered one screen into the whole pane area)

#### Scenario: Snapshot [auto, TestBackend + insta]
- GIVEN styled bytes fed to a 10x40 emulator in a single pane
- WHEN rendered
- THEN the buffer matches the snapshot

#### Scenario: Hidden cursor [auto]
- GIVEN `\e[?25l` fed to the focused pane
- WHEN rendered
- THEN no cursor position is set

#### Scenario: Two panes clipped [auto, TestBackend + insta]
- GIVEN two side-by-side panes with different content
- WHEN rendered
- THEN each pane shows its own content inside its rect, and nothing leaks across the separator

#### Scenario: Cursor offset to pane rect [auto]
- GIVEN the right pane focused with its cursor at (0,0)
- WHEN rendered
- THEN the cursor position is the right pane rect's origin

#### Scenario: Real apps [manual]
- GIVEN nvim/LazyVim and htop in separate panes
- THEN colors, alt screen and layout render correctly in both

## REMOVED Requirements

(None.)
