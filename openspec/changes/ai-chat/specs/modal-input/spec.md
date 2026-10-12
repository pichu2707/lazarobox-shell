# Delta for Modal Input

All scenarios [auto] unless tagged. In TERMINAL, keys of a focused chat pane go to the chat (composer, list or editor) and never to a PTY. The prefix, the other modes, the quit and close confirmations, RESIZE and MENU are unchanged except where a block below says otherwise.

## MODIFIED Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be delivered to the focused pane. For a shell pane it is encoded and sent to the pane's PTY. For a chat pane it is handled by the chat (see Chat pane input) and MUST NEVER reach a PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU, for `v`, `V`, `y`, Esc, `q` and `i` in COPY, and in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action, confirms, toggles a selection, yanks or exits. On a chat pane Repeat is handled like Press for editing and scrolling keys, and ignored for Enter (send), Esc and Ctrl+C (cancel), Ctrl+Y and Ctrl+B (yank).
(Previously: every key except the prefix was sent to the focused pane's PTY; no chat pane)

A key whose Press left COPY (`q`, `i`, Esc, or `y` after a successful yank) MUST NOT leak into TERMINAL: its Repeat events MUST be dropped until its Release or any other Press arrives. Because terminals that report no event types (the default: no keyboard enhancement flags are pushed) deliver OS auto-repeat as repeated Press events, a Press of the identical key (same code and modifiers) MUST also be dropped when it arrives within 500 ms of the exit or of the previous dropped one, so a long hold keeps being dropped. Any other Press, a Release of that key, a paste, or a gap over 500 ms ends the debounce. A yank that leaves COPY open (selection over the cap, or no text) does not start it. Tradeoff: a deliberate retype of the exit key within 500 ms of leaving COPY is dropped too.

#### Scenario: Passthrough
- GIVEN TERMINAL
- WHEN `l` is pressed
- THEN a WritePty effect addressed to the focused pane with `l` is emitted and mode stays TERMINAL

#### Scenario: Repeat ignored in decision states
- GIVEN PREFIX, group pending, or any confirmation
- WHEN a Repeat event arrives
- THEN no action is dispatched and the state is unchanged

#### Scenario: Release ignored
- GIVEN any mode
- WHEN a Release event arrives
- THEN nothing happens

#### Scenario: Input goes to the focused pane only
- GIVEN two panes with the right focused
- WHEN `a` is pressed
- THEN WritePty is addressed to the right pane only

#### Scenario: Repeat in MENU [auto]
- GIVEN MENU
- WHEN Repeat events for `j` and for Esc arrive
- THEN `j` moves the selection and Esc does nothing

#### Scenario: Repeat of selection keys in COPY [auto]
- GIVEN COPY with a selection
- WHEN Repeat events for `v`, `V`, `y`, Esc, `q` and `i` arrive
- THEN the selection, the mode (still COPY) and the effects are unchanged (no SetClipboard, no WritePty)

#### Scenario: Repeat of motions in COPY [auto]
- GIVEN COPY
- WHEN a Repeat event for `j` arrives
- THEN the cursor moves one row down, as for Press

#### Scenario: Held exit key does not leak into TERMINAL [auto]
- GIVEN COPY, and `q` (or `i`, Esc, or `y` with a selection) is pressed, leaving COPY
- WHEN Repeat events of that same key arrive in TERMINAL
- THEN no WritePty is emitted
- AND after its Release, or after any other Press, the key is passed through again
- AND a Repeat of a different key is passed through normally

#### Scenario: Held exit key sent as Presses is dropped within the window [auto]
- GIVEN COPY, and `q` (or `i`, Esc, or `y` with a selection) is pressed at t0, leaving COPY
- WHEN Press events of that same key arrive at t0+400 ms, t0+433 ms and t0+466 ms (no Repeat kind)
- THEN no WritePty is emitted for any of them
- AND a `y` exit still shows its "Copied N characters" notice
- AND a hold continues to be dropped while each Press is within 500 ms of the previous one

#### Scenario: Deliberate press after the window passes through [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `q` is pressed again at t0+600 ms
- THEN WritePty with `q` is emitted

#### Scenario: Another key ends the debounce [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `x` is pressed at t0+100 ms and `q` at t0+150 ms
- THEN both are passed through as WritePty
- AND a Press of the same code with different modifiers (Shift+Q, Ctrl+q) is never dropped by a plain-`q` exit

#### Scenario: A paste ends the debounce [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN a paste arrives at t0+100 ms and `q` is pressed at t0+200 ms
- THEN WritePty with `q` is emitted

#### Scenario: Window boundary and refresh [auto]
- GIVEN COPY, and `q` is pressed at t0, leaving COPY
- WHEN `q` is pressed at t0+500 ms
- THEN it is dropped, and at t0+501 ms (instead) it is passed through
- AND Presses dropped at t0+499 ms and t0+998 ms each refresh the window, so one at t0+1499 ms is passed through

#### Scenario: A yank that stays in COPY does not arm the debounce [auto]
- GIVEN COPY with a selection over the cap, or with no text, and `y` is pressed (COPY stays open)
- WHEN the user later leaves COPY with `q` and presses `y` within 100 ms
- THEN WritePty with `y` is emitted

#### Scenario: Chat pane keys never reach a PTY [auto]
- GIVEN TERMINAL with a chat pane focused next to a shell pane
- WHEN `a`, Enter, Esc, Ctrl+C, Tab and Backspace are pressed
- THEN no WritePty is emitted for any pane

#### Scenario: Repeat on a chat pane [auto]
- GIVEN a focused chat pane with composer text and an idle stream
- WHEN Repeat events for `a`, Backspace, Enter, Ctrl+Y and Ctrl+B arrive
- THEN `a` and Backspace edit the composer like Press, and Enter, Ctrl+Y and Ctrl+B do nothing

### Requirement: Prefix tree with described key groups
After the prefix, the key table MUST be a tree: leaf bindings and groups `w` (labelled `WINDOW`), `t` (`TAB`), `g` (`GO`), `b` (`BUFFER`). Every binding AND every group MUST have a non-empty description, every group a non-empty label, and a group MUST NOT contain duplicate keys. The table is the single source of truth for dispatch, for the key hint (see statusline) and for the future command viewer. Letter keys are matched by character case: the SHIFT modifier MUST be ignored for character keys (terminals report `B` with or without SHIFT), while Ctrl chords match exactly. Bindings:

| Keys | Action |
|---|---|
| `h` `j` `k` `l` | focus left/down/up/right |
| `w v` / `w h` | split right / below |
| `w q` | close pane (confirm) |
| `w r` | enter RESIZE |
| `w z` | zoom toggle |
| `t n` / `t c` | new tab / close tab (confirm) |
| `g b` / `g B` | next / previous tab |
| `b 1`..`b 9` | go to tab N |
| `?` | reserved, no effect |
| `[` | enter COPY |
| `m` | open the config menu (enter MENU) |
| `c` | open a chat pane (split right) |
| `q` | quit (confirm) |

The root of the prefix tree also contains the leaf bindings `m` with description `menu`, entering MENU, and `c` with description `chat`, opening a chat pane; the root key hint therefore includes `m menu` and `c chat`, in that table order, before `q quit`. The pinned list of root hint entries changes from `w t g b [ m q` to `w t g b [ m c q`.
(Previously: root entries were `w`, `t`, `g`, `b`, `[`, `m`, `q`; no `c`)

#### Scenario: Table integrity
- GIVEN the prefix table
- WHEN it is validated
- THEN every binding and group has a non-empty description and no group has duplicate keys

#### Scenario: Group opens
- GIVEN PREFIX
- WHEN `w` is pressed
- THEN a group-pending state for `w` is entered, no WritePty is emitted, and its child keys with descriptions are available to render

#### Scenario: Binding in a group
- GIVEN group `w` pending
- WHEN `v` is pressed
- THEN a split-right action is dispatched and mode is TERMINAL

#### Scenario: Shift on `g B`
- GIVEN group `g` pending
- WHEN `B` is pressed, once with the SHIFT modifier and once without
- THEN both select the previous tab

#### Scenario: Focus follows the new pane
- GIVEN one pane
- WHEN `w v` is pressed
- THEN the new pane is focused and mode is TERMINAL

#### Scenario: Focus keys
- GIVEN PREFIX
- WHEN `h` is pressed
- THEN focus moves left and mode is TERMINAL

#### Scenario: Reserved `?`
- GIVEN PREFIX
- WHEN `?` is pressed
- THEN mode is TERMINAL, no effect is emitted and nothing is written to the PTY

#### Scenario: `m` is in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `m` exists at the root with a non-empty description, and the table integrity rules still hold

#### Scenario: `c` is in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `c` exists at the root with the description `chat`, appears after `m` and before `q`, and the table integrity rules still hold

#### Scenario: `c` opens a chat [auto]
- GIVEN PREFIX
- WHEN `c` is pressed
- THEN a chat pane is opened as a right split (see chat-pane), mode is TERMINAL and no WritePty is emitted

#### Scenario: `c` inside a group or in another mode does nothing special [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN `c` is pressed
- THEN no chat is opened (the group cancels as for an unmapped key)

### Requirement: Enter COPY
`[` in PREFIX MUST enter COPY when the focused pane is a shell pane. When the focused pane is a chat pane it MUST be refused: mode returns to TERMINAL and the notice of copy-mode-scrollback is shown.
(Previously: `[` in PREFIX always entered COPY)

#### Scenario: Enter COPY
- GIVEN PREFIX on a shell pane
- WHEN `[` is pressed
- THEN mode is COPY and no WritePty is emitted

#### Scenario: Refused on a chat pane
- GIVEN PREFIX on a chat pane
- WHEN `[` is pressed
- THEN mode is TERMINAL and the notice `copy mode is not available in chat panes; Ctrl+Y copies the last answer` is shown

### Requirement: Paste and resize in any mode
Paste events MUST be delivered to the focused pane only in TERMINAL: sent to the PTY for a shell pane, inserted into the composer (or the focused text field) for a chat pane, and never to a PTY for a chat pane. Paste is never delivered in MENU. A host Resize event MUST recompute pane rects in every mode and MUST emit ResizePty only for shell panes (in any tab) whose size actually changed. In MENU the recomputation uses the forced tab bar and the previewed positions, and the mode stays MENU.
(Previously: paste went to the focused pane's PTY; ResizePty for panes whose size changed, with no chat pane)

#### Scenario: Paste in COPY
- GIVEN COPY
- WHEN a paste event arrives
- THEN no WritePty is emitted

#### Scenario: Resize only changed panes
- GIVEN two panes and a host resize that changes only the width of the right pane
- WHEN the Resize event is handled
- THEN ResizePty is emitted only for panes whose rect size changed

#### Scenario: Paste in MENU [auto]
- GIVEN MENU
- WHEN a paste event arrives
- THEN no WritePty is emitted

#### Scenario: Paste into a chat composer [auto]
- GIVEN TERMINAL with a chat pane focused
- WHEN a paste of `hello` arrives
- THEN the composer holds `hello`, nothing is sent and no WritePty is emitted

#### Scenario: Resize with a chat pane [auto]
- GIVEN a shell pane and a chat pane and a host resize that changes both
- WHEN the Resize event is handled
- THEN ResizePty is emitted for the shell pane only

## ADDED Requirements

### Requirement: Chat pane input
In TERMINAL, with a chat pane focused and the conversation view shown, keys MUST act as follows; every key not listed below that is not a printable character MUST be swallowed (no effect, no WritePty):

| Key | Effect |
|---|---|
| printable characters, Backspace, Delete, Left, Right, Home, End | edit the composer |
| Enter | send the message |
| Alt+Enter, Ctrl+J | insert a newline in the composer |
| Ctrl+U | clear the composer |
| Esc, Ctrl+C | cancel the running stream; with no stream running, do nothing |
| Ctrl+Y | yank the last assistant answer (see chat-pane) |
| Ctrl+B | yank the last code block of the last assistant answer (see chat-pane) |
| PageUp, PageDown | scroll the transcript |
| Ctrl+T | open the tag editor (see chat-history) |
| Ctrl+Space | enter PREFIX as in any pane |

Ctrl+C on a chat pane MUST NEVER emit Quit or a WritePty, whether a stream is running or not. Pressing Ctrl+Space twice MUST swallow the second press (no NUL is inserted in the composer). While the conversation list, the tag editor or the delete confirmation is shown, keys act as defined in chat-history, with the same swallow rule. The key bindings are defaults for design to confirm.

#### Scenario: Typing edits the composer [auto]
- GIVEN a focused idle chat pane
- WHEN `h`, `i` and Backspace are pressed
- THEN the composer holds `h` and no WritePty is emitted

#### Scenario: Enter sends, newline bindings do not [auto]
- GIVEN the composer holding `hi`
- WHEN Alt+Enter, then Ctrl+J, then Enter are pressed
- THEN two newlines are inserted and only the Enter emits a ChatSend

#### Scenario: Ctrl+B yanks a code block [auto]
- GIVEN a chat pane whose last answer holds a fenced block
- WHEN Ctrl+B is pressed
- THEN a SetClipboard effect carries the block and no WritePty is emitted

#### Scenario: Esc and Ctrl+C cancel a stream [auto]
- GIVEN a streaming chat pane
- WHEN Esc is pressed, and in another run Ctrl+C is pressed
- THEN each time a ChatCancel effect is emitted and the partial answer is kept

#### Scenario: Esc and Ctrl+C idle do nothing [auto]
- GIVEN an idle chat pane with composer text
- WHEN Esc and Ctrl+C are pressed
- THEN the composer text is unchanged, no Quit and no WritePty is emitted

#### Scenario: Unlisted key swallowed [auto]
- GIVEN a focused chat pane
- WHEN Tab, F5 and Ctrl+X are pressed
- THEN nothing changes and no WritePty is emitted

#### Scenario: Prefix still works [auto]
- GIVEN a focused chat pane
- WHEN `Ctrl+Space` then `w` then `v` are pressed
- THEN a split is made and mode is TERMINAL
- AND when `Ctrl+Space` is pressed twice instead, nothing is inserted into the composer

#### Scenario: Keys in the list view [auto]
- GIVEN the conversation list shown in a chat pane
- WHEN `j`, `d` and `y` are pressed
- THEN the cursor moves, the delete confirmation shows and `y` confirms it, and nothing is typed into a composer

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN the app in Kitty with a chat pane focused
- WHEN the user types, sends with Enter, adds a line with Alt+Enter, cancels with Esc and presses Ctrl+C when idle
- THEN each key behaves as listed and the app never quits on Ctrl+C
