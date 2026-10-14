# Delta for Modal Input

All scenarios [auto] unless tagged. Only COPY entry on chat panes and the chat COPY key table change; shell-pane COPY, the prefix tree, the other modes and the repeat/debounce rules are unchanged. There is NO Shift+PageUp/PageDown binding and no new direct Ctrl+letter key (the terminal emulator consumes Shift+PageUp; new chat keys live behind the prefix).

Archive-order dependency: the requirement "Enter COPY" is modified by the not yet archived ai-chat change (`openspec/changes/ai-chat/specs/modal-input/spec.md`), which makes `[` refuse on chat panes. pane-scroll MUST be archived AFTER ai-chat; this delta then replaces that text.

## MODIFIED Requirements

### Requirement: Enter COPY
`[` in PREFIX MUST enter COPY on the focused pane, whether it is a shell pane or a chat pane. On a chat pane it enters chat COPY (see copy-mode-scrollback) when the pane shows the conversation view; otherwise mode returns to TERMINAL with the notice `nothing to scroll here`. No refusal for chat panes remains.
(Previously: `[` in PREFIX entered COPY on a shell pane and was refused on a chat pane with the notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer`)

#### Scenario: Enter COPY
- GIVEN PREFIX on a shell pane
- WHEN `[` is pressed
- THEN mode is COPY and no WritePty is emitted

#### Scenario: Enter COPY on a chat pane
- GIVEN PREFIX on a chat pane showing a conversation
- WHEN `[` is pressed
- THEN mode is COPY, no notice is shown and no WritePty is emitted

#### Scenario: Chat list view
- GIVEN PREFIX on a chat pane showing the conversation list
- WHEN `[` is pressed
- THEN mode is TERMINAL and the notice `nothing to scroll here` is shown

## ADDED Requirements

### Requirement: Chat COPY keys
In COPY with a chat pane focused, keys MUST act as follows; every other key (including printable characters, Enter, Backspace, Tab, `v`, `V`, `y`, `i`, `h`, `l`, `0`, `$`) MUST be swallowed: no effect on the composer, the transcript or the clipboard, and no WritePty.

| Key | Effect |
|---|---|
| `j` | scroll the transcript one line down |
| `k` | scroll the transcript one line up |
| Ctrl+d | half a page down |
| Ctrl+u | half a page up |
| `g` `g` | top of the transcript |
| `G` | bottom of the transcript |
| Esc, `q` | leave COPY: mode TERMINAL, transcript at the bottom and following |
| Ctrl+Space | enter PREFIX as in any pane |

Repeat MUST be handled like Press for `j`, `k`, Ctrl+d and Ctrl+u and ignored for Esc and `q`. The exit-key debounce of the TERMINAL requirement (Repeat dropped, identical Press within 500 ms dropped) applies to a chat pane left through Esc or `q`, so a held exit key never reaches the composer. Selection keys have no function in this change: a visual selection and yank over the transcript are a follow-up.

#### Scenario: Scroll keys [auto]
- GIVEN chat COPY on a long transcript
- WHEN `k`, `k`, `j` are pressed
- THEN the view is one line above the bottom and the mode is COPY

#### Scenario: Page and ends [auto]
- GIVEN chat COPY at the bottom
- WHEN Ctrl+u, then `g` `g`, then `G` are pressed
- THEN the view moves up half a page, then to the top, then to the bottom

#### Scenario: Exit with Esc and q [auto]
- GIVEN chat COPY scrolled up
- WHEN Esc is pressed, and in another run `q` is pressed
- THEN each time the mode is TERMINAL, the transcript is at the bottom and following, and the composer text is unchanged

#### Scenario: Selection keys do nothing [auto]
- GIVEN chat COPY
- WHEN `v`, `V` and `y` are pressed
- THEN the mode is still COPY, no SetClipboard and no WritePty is emitted

#### Scenario: Printable keys swallowed [auto]
- GIVEN chat COPY with an empty composer
- WHEN `a`, `i`, Enter and Backspace are pressed
- THEN the composer is empty, nothing is sent and the mode is still COPY

#### Scenario: Prefix inside chat COPY [auto]
- GIVEN chat COPY
- WHEN `Ctrl+Space` is pressed
- THEN the mode is PREFIX

#### Scenario: Repeat of scroll keys [auto]
- GIVEN chat COPY
- WHEN a Repeat event for `j` arrives, and one for Esc
- THEN the view moves one line down and the mode is still COPY

#### Scenario: Held exit key does not leak into the composer [auto]
- GIVEN chat COPY, and `q` is pressed, leaving COPY
- WHEN Repeat events of `q` (or identical Presses within 500 ms) arrive in TERMINAL
- THEN the composer text is unchanged and no WritePty is emitted; a `q` Press after 600 ms is typed into the composer

#### Scenario: Same key table on a shell pane is unchanged [auto]
- GIVEN COPY on a shell pane
- WHEN `v`, `y` and `h` are pressed
- THEN they select, yank and move as before

### Requirement: Mouse events by mode
Mouse wheel events MUST be handled only in TERMINAL and COPY (see mouse-input); in every other mode they are ignored and no state changes. A wheel event MUST NEVER change the mode, and in COPY it MUST NEVER exit COPY. Click and drag events are never handled.

#### Scenario: Wheel in TERMINAL keeps the mode [auto]
- GIVEN TERMINAL
- WHEN a wheel event scrolls a pane
- THEN the mode is still TERMINAL

#### Scenario: Wheel in COPY keeps COPY [auto]
- GIVEN COPY on a shell pane
- WHEN wheel events arrive over that pane and over another pane
- THEN the mode is still COPY

#### Scenario: Wheel in PREFIX [auto]
- GIVEN PREFIX
- WHEN a wheel event arrives
- THEN nothing happens and the mode is still PREFIX
