# Modal Input Specification

## Purpose

TERMINAL / PREFIX / COPY state machine plus quit confirmation, as a pure `update(event) -> effects` function. All scenarios [auto] unless tagged.

## Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be delivered to the focused pane. For a shell pane it is encoded and sent to the pane's PTY. For a chat pane it is handled by the chat (see Chat pane input) and MUST NEVER reach a PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU, for `v`, `V`, `y`, Esc, `q` and `i` in COPY, and in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action, confirms, toggles a selection, yanks or exits. On a chat pane Repeat is handled like Press for editing and scrolling keys, and ignored for Enter (send), Esc and Ctrl+C (cancel), Ctrl+Y and Ctrl+B (yank).

A held key MUST act once wherever only a Press may act. Terminals that report no event types (the default: no keyboard enhancement flags are pushed) deliver OS auto-repeat as repeated Press events, so a Press of the identical key (same code and modifiers) that arrives within 500 ms of the previous key event (inclusive; every event of the hold renews the window) is part of the hold. Any other key, a Release of that key, a paste, or a gap over 500 ms ends the hold. MENU, every confirmation and the chat pane (the conversation view, approval prompts, the batch and commit reviews, the `engram serve` offer, the conversation list and the tag, note and summary editors) MUST handle a Press that is part of a hold like a Repeat: a held key never saves, cancels, discards, sends, yanks, toggles or answers more than once, and a key held from before a prompt opened never answers it once it arms: a prompt arms 800 ms after it opened, later than the usual OS initial repeat delay (X11 660 ms, KDE 600 ms), so the first repeat, which comes past the hold window and reads as a new press, still finds it unarmed. An OS repeat delay over 800 ms can answer it once. Typing and navigation keys act on every Press as before. Shell panes, PREFIX, group pending and COPY keep the Repeat rule above. Tradeoff: a deliberate second press of the same decision key within 500 ms of the first is dropped too (press it again after a pause).

A key whose Press left COPY (`q`, `i`, Esc, or `y` after a successful yank), closed MENU, answered a confirmation or left RESIZE MUST NOT leak into TERMINAL: while it is held (its Repeat events, or its Presses within the hold window) it MUST be dropped, until its Release, any other Press, a paste, or a gap over 500 ms. A yank that leaves COPY open (selection over the cap, or no text) does not start it. Tradeoff: a deliberate retype of that key within 500 ms is dropped too.

Ctrl+H MUST act as Backspace in every text field: the chat composer, the note, summary and tag editors, and the menu's text fields (the `custom…` model field, the Hugging Face search and token fields, the Feedback form). Terminals that send `^H` (0x08) for Backspace report it as Ctrl+H. A shell pane MUST still receive the 0x08 byte as it came.

#### Scenario: Passthrough
- GIVEN TERMINAL
- WHEN `l` is pressed
- THEN a WritePty effect addressed to the focused pane with `l` is emitted and mode stays TERMINAL

#### Scenario: Repeat ignored in decision states
- GIVEN PREFIX, group pending, or any confirmation
- WHEN a Repeat event arrives
- THEN no action is dispatched and the state is unchanged

#### Scenario: A held decision key acts once [auto]
- GIVEN a terminal that sends a held key as Presses, and a tool approval prompt that arms 800 ms after it opened
- WHEN `y`, `n` or Esc arrives as a Press every 50 ms from the moment the prompt opened, for 1 s
- THEN none of them answers the prompt, and a new Press more than 500 ms after the last one answers it

#### Scenario: The first OS repeat does not answer a prompt [auto]
- GIVEN a tool approval prompt that opened at t0, and a key held from t0+100 ms with an OS initial repeat delay of 660 ms
- WHEN `y`, `n` or Esc arrives as a Press at t0+100 ms, then at t0+760 ms and every 25 ms after it until t0+1500 ms
- THEN none of them answers the prompt

#### Scenario: A held Esc backs out of a picker once [auto]
- GIVEN MENU at the model level of a task picker
- WHEN Esc arrives as a Press at 0, 100, 200 and 300 ms
- THEN the picker goes back one level only and the menu stays open, so its draft is not discarded

#### Scenario: Ctrl+H is Backspace in a text field [auto]
- GIVEN the chat composer, a note, summary or tag editor, or a text field of the menu, holding `ab`
- WHEN Ctrl+H is pressed
- THEN the field holds `a`; on a shell pane Ctrl+H writes `[0x08]` to the PTY

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
- GIVEN a focused chat pane with composer text, an idle stream and a collapsed reasoning block
- WHEN Repeat events for `a`, Backspace, Enter, Ctrl+Y, Ctrl+B and Ctrl+R arrive
- THEN `a` and Backspace edit the composer like Press, and Enter, Ctrl+Y, Ctrl+B and Ctrl+R do nothing

### Requirement: Prefix key
Ctrl+Space (reported as NUL 0x00) and Ctrl+] (reported as 0x1D) MUST each enter PREFIX without writing to the PTY. Both keys are fixed; neither is configurable. The prefix keys and the action table MUST be data, not hard-coded branches. Pressing a prefix key again in PREFIX MUST write that key's own literal byte to the focused shell pane (Ctrl+Space writes NUL 0x00, Ctrl+] writes 0x1D) and return to TERMINAL. Pressing the other prefix key in PREFIX does the same: it writes its own literal byte. Esc or an unmapped key in PREFIX returns to TERMINAL, swallowed. When the focused pane is not a shell pane, a repeated prefix key MUST write nothing and MUST show a notice; it MUST NOT be ignored silently.
(Previously: only Ctrl+Space entered PREFIX; Ctrl+] was an ordinary key sent to the pane)

#### Scenario: Enter PREFIX
- GIVEN TERMINAL
- WHEN Ctrl+Space is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Enter PREFIX with the alias [auto]
- GIVEN TERMINAL
- WHEN Ctrl+] (0x1D) is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Literal prefix
- GIVEN PREFIX entered with Ctrl+Space
- WHEN Ctrl+Space is pressed again
- THEN WritePty `[0x00]` to the focused pane is emitted and mode is TERMINAL

#### Scenario: Literal alias [auto]
- GIVEN PREFIX entered with Ctrl+]
- WHEN Ctrl+] is pressed again
- THEN WritePty `[0x1D]` to the focused pane is emitted and mode is TERMINAL

#### Scenario: Other prefix key inside PREFIX [auto]
- GIVEN PREFIX entered with Ctrl+]
- WHEN Ctrl+Space is pressed
- THEN WritePty `[0x00]` to the focused pane is emitted and mode is TERMINAL
- AND the same holds with the keys swapped (`[0x1D]`)

#### Scenario: Literal prefix on a chat pane [auto]
- GIVEN PREFIX with a chat pane focused
- WHEN Ctrl+Space or Ctrl+] is pressed
- THEN no WritePty is emitted, mode is TERMINAL and a notice is shown

#### Scenario: Cancel/unknown
- GIVEN PREFIX
- WHEN Esc or an unmapped key (e.g. `z`) is pressed
- THEN mode is TERMINAL and the key is swallowed (no WritePty)

#### Scenario: Alias drives the whole tree [auto]
- GIVEN TERMINAL
- WHEN Ctrl+] then `w` then `v` are pressed
- THEN the pane splits right, exactly as with Ctrl+Space

#### Scenario: Real terminal reports the prefix [manual, terminal records]
- GIVEN a terminal that has a record under `docs/terminals/`
- WHEN Ctrl+] is pressed (and Ctrl+Space, where the terminal delivers it)
- THEN PREFIX is entered and nothing is written to the PTY (the root which-key hint appears in the statusline while PREFIX is pending; see statusline)
- AND the result is written in the terminal's record (see terminal-compatibility)

The row of the binding table for the prefix MUST read: `Ctrl+Space` / `Ctrl+]`: send the prefix key itself: a NUL byte (0x00) / 0x1D to the focused shell pane (see Prefix key).

### Requirement: Quit confirmation
`q` in PREFIX MUST enter a confirmation state shown as "Quit? (y/n)" (the same prompt as the quit variants of `w q` and `t c`). `y` MUST emit Quit; any other key (n, Esc, other) MUST return to TERMINAL, swallowed. Only lowercase `y` with no modifiers confirms; `Y` (Shift or Caps Lock) declines. Quit MUST shut down all panes of all tabs.
(Previously: unchanged except that quit now covers every pane)

#### Scenario: Confirm
- GIVEN confirmation pending
- WHEN `y` is pressed
- THEN a Quit effect is emitted

#### Scenario: Decline
- GIVEN confirmation pending
- WHEN `n`, Esc or `x` is pressed
- THEN mode is TERMINAL, no Quit, no WritePty

### Requirement: Enter COPY
`[` in PREFIX MUST enter COPY on the focused pane, whether it is a shell pane or a chat pane. On a chat pane it enters chat COPY (see copy-mode-scrollback) when the pane shows the conversation view; otherwise mode returns to TERMINAL with the notice `nothing to scroll here`. No refusal for chat panes remains.

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

### Requirement: Paste and resize in any mode
Paste events MUST be delivered to the focused pane only in TERMINAL: sent to the PTY for a shell pane, inserted into the composer (or the focused text field) for a chat pane, and never to a PTY for a chat pane. Paste is never delivered in MENU. A host Resize event MUST recompute pane rects in every mode and MUST emit ResizePty only for shell panes (in any tab) whose size actually changed. In MENU the recomputation uses the forced tab bar and the previewed positions, and the mode stays MENU.

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
| `n` | save a note of the chat (see Save-note prefix key) |
| `o` | expand or collapse the memory block (see Memory keys) |
| `s` | the end-of-conversation summary (see Memory keys) |
| `T` | edit the tags of the conversation (see Chat pane input) |
| `e` | run the last shell code block of the chat in a pane (confirm; see pane-commands) |
| `a` | cycle the chat's approval mode `ask`, `auto`, `run` (see agent-tools) |
| `q` | quit (confirm) |
| Ctrl+Space | send the prefix key itself: a NUL byte (0x00) to the focused shell pane (see Prefix key) |

`n`, `o`, `s`, `T`, `e` and `a` are root leaves left out of the key hint (the root hint is already 79 columns wide); on a pane that is not a chat they only show a notice.

The root of the prefix tree also contains the leaf bindings `m` with description `menu`, entering MENU, and `c` with description `chat`, opening a chat pane; the root key hint therefore includes `m menu` and `c chat`, in that table order, before `q quit`. The pinned list of root hint entries changes from `w t g b [ m q` to `w t g b [ m c q`.

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

### Requirement: Group cancellation
Inside a group, Esc, an unmapped key, or Ctrl+Space MUST cancel back to TERMINAL, and the key MUST be swallowed (no WritePty).

#### Scenario: Esc cancels
- GIVEN group `t` pending
- WHEN Esc is pressed
- THEN mode is TERMINAL and no WritePty is emitted

#### Scenario: Unmapped key cancels
- GIVEN group `w` pending
- WHEN `x` is pressed
- THEN mode is TERMINAL and the key is swallowed

#### Scenario: Ctrl+Space cancels, not literal
- GIVEN group `g` pending
- WHEN Ctrl+Space is pressed
- THEN mode is TERMINAL and no `[0x00]` is written

### Requirement: Close confirmations
`w q` MUST ask "Close pane? (y/n)". If the pane is the last pane of the only tab, it MUST ask "Quit? (y/n)" instead, because closing it quits. `t c` MUST ask "Close tab? (y/n)", or "Quit? (y/n)" when it is the only tab. Only an unmodified lowercase `y` confirms; any other key declines and returns to TERMINAL, swallowed. Confirmation of a pane close MUST emit a pane-close action for the focused pane. Any pane removal while a confirmation is pending MUST cancel it (mode TERMINAL), so `y` can never act on a pane that changed under the prompt.

#### Scenario: Close pane confirmed
- GIVEN two panes and `w q` pressed
- WHEN `y` is pressed
- THEN the focused pane is closed and mode is TERMINAL

#### Scenario: Close pane declined
- GIVEN "Close pane? (y/n)" pending
- WHEN `n`, Esc, `x` or `Y` is pressed
- THEN no pane is closed, no WritePty, mode is TERMINAL

#### Scenario: Last pane prompts quit
- GIVEN one tab with one pane
- WHEN `w q` is pressed
- THEN the prompt is "Quit? (y/n)" and `y` emits Quit

#### Scenario: Last pane of one of two tabs prompts close pane
- GIVEN two tabs, the active one with a single pane
- WHEN `w q` is pressed
- THEN the prompt is "Close pane? (y/n)"

#### Scenario: Only tab prompts quit
- GIVEN one tab
- WHEN `t c` is pressed
- THEN the prompt is "Quit? (y/n)" and `y` emits Quit

#### Scenario: Removal cancels the prompt
- GIVEN "Close pane? (y/n)" pending on the focused pane of two
- WHEN the other pane's shell exits
- THEN mode is TERMINAL and no pane is closed by a later `y`

### Requirement: RESIZE mode
`w r` MUST enter RESIZE, a sticky mode. In RESIZE, `h/j/k/l` MUST move the focused pane's border by 1 cell and the mode MUST persist until Esc. Repeat events MUST be handled like Press. All other keys, including unknown keys and Ctrl+Space, MUST be swallowed (no WritePty) and the mode MUST stay RESIZE. Esc MUST return to TERMINAL. Entering RESIZE with a single pane MUST be allowed; the resize keys are then no-ops and Esc exits as usual.

#### Scenario: Sticky
- GIVEN RESIZE
- WHEN `l` is pressed three times
- THEN three one-cell resize steps are applied and mode is still RESIZE

#### Scenario: Repeat allowed
- GIVEN RESIZE
- WHEN a Repeat event for `l` arrives
- THEN a resize step is applied

#### Scenario: Esc exits
- GIVEN RESIZE
- WHEN Esc is pressed
- THEN mode is TERMINAL and no WritePty is emitted

#### Scenario: Keys swallowed
- GIVEN RESIZE
- WHEN `x` is pressed
- THEN no WritePty is emitted and mode stays RESIZE

#### Scenario: Single pane RESIZE
- GIVEN one pane
- WHEN `w r` is pressed, then `l`
- THEN mode is RESIZE after `w r`, `l` changes nothing and emits no effect, and mode stays RESIZE until Esc

### Requirement: Focus change exits COPY and RESIZE
Any change of focused pane or active tab MUST exit COPY (the pane being left returns its viewport to the bottom) and RESIZE, leaving mode TERMINAL.

#### Scenario: Pane close in COPY
- GIVEN COPY on pane A
- WHEN focus moves to pane B for any reason (including pane A's shell exiting)
- THEN mode is TERMINAL and pane A's viewport is at the bottom

#### Scenario: Tab switch in RESIZE
- GIVEN RESIZE and two tabs
- WHEN the active tab changes
- THEN mode is TERMINAL

#### Scenario: Focus change in RESIZE
- GIVEN RESIZE on pane A of two
- WHEN focus moves to pane B for any reason (for example A's shell exits)
- THEN mode is TERMINAL

#### Scenario: Removal that keeps focus leaves RESIZE
- GIVEN RESIZE on pane A of three
- WHEN another pane's shell exits
- THEN mode is still RESIZE

### Requirement: MENU mode
`m` in PREFIX MUST enter MENU. MENU is a sticky mode that swallows input: the keys defined by the config-menu spec act, every other key (including Ctrl+Space) is swallowed with no WritePty, and the mode stays MENU until Enter saves successfully or Esc cancels. MENU MUST be entered only from TERMINAL through the prefix; `m` inside a group, in COPY, RESIZE or a confirmation MUST NOT open it. Press and Repeat are handled for navigation and cycling keys (Repeat on Enter and Esc MUST be ignored so a held key cannot save or cancel repeatedly, and so MUST a Press that is part of a hold, see TERMINAL mode); Release is ignored. Paste events MUST NOT reach any PTY in MENU. A focus or active-tab change MUST NOT leave MENU.

#### Scenario: Enter MENU [auto]
- GIVEN PREFIX
- WHEN `m` is pressed
- THEN mode is MENU and no WritePty is emitted

#### Scenario: Not from other modes [auto]
- GIVEN COPY, RESIZE, or a group pending
- WHEN `m` is pressed
- THEN MENU is not opened

#### Scenario: Repeat on Enter and Esc ignored [auto]
- GIVEN MENU with a changed draft
- WHEN a Repeat event for Enter or for Esc arrives
- THEN no save is requested, the menu is not closed and the state is unchanged

#### Scenario: Release ignored [auto]
- GIVEN MENU
- WHEN a Release event arrives
- THEN nothing happens

#### Scenario: Focus change keeps MENU [auto]
- GIVEN MENU on pane A of two
- WHEN pane A's shell exits and focus moves to pane B
- THEN mode is still MENU

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
| Ctrl+R | expand or collapse the most recent reasoning block (see chat-pane) |
| Ctrl+G | open the review of an orphan write batch (the badge `orphan N · Ctrl+G` names it) |
| Ctrl+H | Backspace (see TERMINAL mode) |
| Ctrl+Space | enter PREFIX as in any pane |

New chat keys live behind the prefix (user rule 2026-10-13), not on direct Ctrl+letter chords. The chords in the table above predate the rule or are its allowed exceptions; a new direct chord MUST be added to this list of allowed exceptions, with its reason, or live behind the prefix:

- Ctrl+G: a direct shortcut on purpose (user decision 2026-10-29): the orphan badge names it, it only acts when an orphan batch exists, and it opens a review that still needs full coverage and the arm delay before anything is applied.

The tag editor (see chat-history) is opened by the prefix key `T` (`Ctrl+Space T`, capital), not by a direct Ctrl+letter chord, because of that rule, and `t` is already the TAB group. `T` MUST be a root leaf of the prefix table, left out of the key hint like `o` and `s`, and on a shell pane it MUST only show a notice.

Ctrl+C on a chat pane MUST NEVER emit Quit or a WritePty, whether a stream is running or not. Pressing Ctrl+Space twice MUST swallow the second press (no NUL is inserted in the composer). While the conversation list, the tag editor or the delete confirmation is shown, keys act as defined in chat-history, with the same swallow rule. The key bindings are defaults for design to confirm; Ctrl+R (R for reasoning) is a proposed default chosen in the style of Ctrl+Y and Ctrl+B, and is an open question until confirmed on a real terminal (manual check M8).

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

#### Scenario: Ctrl+R toggles reasoning [auto]
- GIVEN a chat pane whose last turn has a collapsed reasoning block
- WHEN Ctrl+R is pressed twice
- THEN the block expands and then collapses, no WritePty is emitted and the composer text is unchanged

#### Scenario: Ctrl+R in the list view [auto]
- GIVEN the conversation list shown in a chat pane
- WHEN Ctrl+R is pressed
- THEN nothing changes and nothing is typed into a composer

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

### Requirement: Memory keys
In TERMINAL, with a chat pane focused and the conversation view shown, no key is added to the Chat pane input table, and the memory block and the summary are driven by prefix keys:

| Key | Effect |
|---|---|
| Ctrl+Space then `s` | request the end-of-conversation summary (see chat-memory). A root leaf of the prefix table, left out of the hint (the root hint is already 79 columns and a longer one would clip `q quit` at 80); on a pane that is not a chat it shows `The summary is for chat panes; open one with Ctrl+Space c` |
| Ctrl+Space then `o` | expand or collapse the memory block (see chat-memory); it never touches the reasoning block, which stays on Ctrl+R. A root leaf of the prefix table, left out of the hint; on a pane that is not a chat it shows `The memory line is for chat panes; open one with Ctrl+Space c` |

Ctrl+N, Ctrl+O and Ctrl+E are NOT bound: in a chat pane they are swallowed as any other unbound key. With memory off or not ready, `Prefix s` only shows its notices and `Prefix o` does nothing when no memory line exists. These keys MUST NEVER reach a PTY and MUST NOT type anything into the composer. In the conversation list, the tag editor and the delete confirmation they do nothing.

#### Scenario: Prefix n opens the editor [auto]
- GIVEN a conversation with an answer and a ready memory
- WHEN Ctrl+Space then `n` are pressed
- THEN the note editor opens, no WritePty is emitted and the composer text is unchanged

#### Scenario: Ctrl+N is not bound [auto]
- GIVEN a focused chat pane
- WHEN Ctrl+N is pressed, and as a Repeat event
- THEN nothing happens, no notice shows and no effect is emitted

#### Scenario: Prefix s requests a summary [auto]
- GIVEN an idle conversation with an answer and a ready memory
- WHEN Ctrl+Space then `s` are pressed
- THEN one summary request effect is emitted and no WritePty is emitted

#### Scenario: Prefix o and Ctrl+R are independent [auto]
- GIVEN a last turn with a collapsed reasoning block and a collapsed memory line
- WHEN Ctrl+Space then `o` are pressed, then Ctrl+R
- THEN the memory line expands first and the reasoning block expands second, each only on its own key

#### Scenario: Unbound Ctrl keys ignored [auto]
- GIVEN a focused chat pane
- WHEN Ctrl+E and Ctrl+O are pressed, and as Repeat events
- THEN nothing happens and no effect is emitted

#### Scenario: Memory off [auto]
- GIVEN memory off
- WHEN Ctrl+Space then `n`, and Ctrl+Space then `s`, are pressed
- THEN only the notice `Memory is off; enable it in the menu (Ctrl+Space m)` appears and no effect is emitted

#### Scenario: Other views ignore them [auto]
- GIVEN the conversation list shown
- WHEN Ctrl+Space then `s` and Ctrl+Space then `o` are pressed
- THEN nothing changes and nothing is typed

### Requirement: Save-note prefix key
The root of the prefix table MUST bind `n` ("note", hinted, after `c` and before `q`) to the save-note action; the `n` of the `t` group (new tab) is unchanged. On a focused chat pane it MUST behave as the save-note action of chat-memory (memory active, a git project, the project resolved first). On a pane that is not a chat it MUST only show the notice `Save as note is for chat panes; open one with Ctrl+Space c` and emit nothing. The root hint reads `... m menu · c chat · n note · q quit`.

#### Scenario: Prefix n on a shell pane [auto]
- GIVEN a focused shell pane
- WHEN Ctrl+Space then `n` are pressed
- THEN only the notice `Save as note is for chat panes; open one with Ctrl+Space c` appears and no effect is emitted

#### Scenario: Prefix n is in the root hint [auto]
- WHEN the prefix is pending
- THEN the root hint lists `n note` between `c chat` and `q quit`

### Requirement: Memory view keys
While the note editor or the summary editor is open in a chat pane, keys act as follows and every other key that is not a printable character MUST be swallowed (no effect, no WritePty); Ctrl+C MUST NEVER emit Quit or a WritePty:

| Key | Effect |
|---|---|
| printable characters, Backspace, Delete, Left, Right, Home, End | edit the focused text field (on the category field, Left and Right cycle the category and printable characters do nothing) |
| Up, Down | move the cursor between lines of the content field |
| Enter | newline in the content field; in the title field, save (note editor); in the summary editor, newline |
| Tab, Shift+Tab | next or previous field (the summary editor has one field) |
| Ctrl+S | save (note) or approve (summary), including the second redaction pass |
| Esc | discard the view |
| Ctrl+Space | enter PREFIX as in any pane |

A paste is inserted into the focused text field with control and format characters dropped and CRLF turned into newline, never saved, and clipped at the field cap. Repeat is handled like Press for editing keys and ignored for Ctrl+S and Esc. In the start-server confirmation, with an empty composer, only an unmodified lowercase `y` press confirms and `n` or Esc declines; every other key (and any key while the composer holds text) is typed or handled as in any chat and leaves the confirmation up, and a repeat or a modifier key on its own answers nothing. While MENU is open no key or paste reaches a memory view.

#### Scenario: Typing and field cap [auto]
- GIVEN the title field holding 120 characters
- WHEN another character is typed
- THEN the title is unchanged

#### Scenario: Category cycles [auto]
- GIVEN the category field on `decision`
- WHEN Right is pressed, and then Left twice
- THEN the category is `architecture`, then `decision`, then `preference` (wrap)

#### Scenario: Enter in the title saves, in the content adds a line [auto]
- GIVEN a valid note editor with the title field focused, and in another run the content field focused
- WHEN Enter is pressed
- THEN the first emits one save effect and the second inserts a newline and emits none

#### Scenario: Ctrl+S saves, Esc discards [auto]
- GIVEN a valid note editor
- WHEN Ctrl+S is pressed, and in another run Esc
- THEN the first emits one save effect and the second emits none

#### Scenario: Paste into the focused field [auto]
- GIVEN the content field focused
- WHEN a paste of `x\r\ny\u{1b}[31m` arrives
- THEN the field holds `x`, newline, `y[31m` and nothing is saved

#### Scenario: Unlisted keys swallowed, Ctrl+C never quits [auto]
- GIVEN the note editor open
- WHEN F5, Ctrl+X and Ctrl+C are pressed
- THEN nothing changes and no Quit or WritePty is emitted

#### Scenario: Server offer accepts only y [auto]
- GIVEN the start-server confirmation
- WHEN `Y`, `n`, Enter and then (in another run) `y` are pressed
- THEN the first three decline and only `y` confirms

#### Scenario: Server offer yields to the stream and to the first message [auto]
- GIVEN the `engram serve` offer pending
- WHEN a stream runs and Esc is pressed
- THEN the stream is cancelled and the offer is not answered
- AND WHEN a message was sent since the offer was shown THEN `y` and `n` at an empty composer are typed as text

#### Scenario: Prefix still works [auto]
- GIVEN the note editor open
- WHEN Ctrl+Space then `w` then `v` are pressed
- THEN a split is made and the editor is still open in its pane

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN a chat with an answer and memory ready, in Kitty
- WHEN the user presses Ctrl+Space then `n`, edits the fields, saves with Ctrl+S, and presses Ctrl+Space then `o`, then Ctrl+Space then `s` and Ctrl+S in the summary view
- THEN each key is received as listed and none is intercepted by the terminal

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

### Requirement: Esc per view
Esc means a different thing in each view; this table documents the meanings 1.0 keeps (user decision 2026-10-29: no behavior change). A held Esc acts once in every view below except a shell pane (see TERMINAL mode).

| View | Esc |
|---|---|
| TERMINAL, shell pane | sent to the program (0x1b) |
| PREFIX, group pending | cancels; the key is swallowed |
| quit, close-pane and close-tab confirmations | declines, like any key but `y` |
| RESIZE | leaves to TERMINAL |
| COPY, chat COPY | clears the selection; with none, leaves COPY |
| MENU, top level | cancels: the unsaved draft is discarded and the menu closes |
| MENU, task picker, install wizard, Hugging Face screens | goes back one level (from the Hugging Face popular list, to its search field) |
| MENU, Hugging Face token field | closes the field; a token check in flight is cancelled and nothing is saved |
| MENU, Feedback form | closes the form, back to its row (also after a send) |
| chat, conversation view | cancels the running stream; with none, does nothing |
| chat, tool approval and `Prefix e` confirmation | declines, once the prompt is armed |
| chat, batch and commit review | rejects, once the review delay is over (an orphan batch is discarded) |
| chat, `engram serve` offer | later, like `n` |
| chat, conversation list | starts a new conversation, like `n` |
| chat, tag editor and summary draft | discards the edit |
| chat, note editor | closes it without saving |

#### Scenario: Esc backs out one step in the menu [auto]
- GIVEN MENU in a task picker at the model level
- WHEN Esc is pressed
- THEN the picker goes back to the engine level and the menu stays open

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

### Requirement: The quit prompt names what still runs
Opening "Quit? (y/n)" (from `q`, or `w q` / `t c` when they quit) MUST ask the runtime what still runs in the panes. A job is a process group of a pane's session (closed panes included) other than its shell's own group. While the prompt is open and jobs run, the statusline MUST read `Quit? still running: <names> (y/n)`: at most three names, each cut to 24 columns, then `+N more`; when that does not fit, fewer names, then `Quit? N still running (y/n)`, then the plain prompt. An answer that arrives after the prompt closed MUST be ignored. Lazarobox MUST NOT signal those jobs. After lazarobox quits and restores the terminal, the jobs still running MUST be written to stderr, one per line with their process group, after a short settle so jobs ending on their hangup are not named.

#### Scenario: The prompt names the jobs
- GIVEN a pane that started `sleep 77 &` and `npm run dev &`
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? still running: sleep 77, npm run dev (y/n)" and `y` emits Quit

#### Scenario: Nothing runs
- GIVEN no job besides the pane shells
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? (y/n)"

#### Scenario: Narrow statusline
- GIVEN two jobs and a 40-column terminal
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? 2 still running (y/n)"

#### Scenario: exit in the last pane
- GIVEN the only pane started `sleep 77 &`
- WHEN its shell exits and lazarobox quits
- THEN stderr reads "lazarobox-shell: still running from your panes:" and "  sleep 77 (process group <pgid>)", and `sleep 77` keeps running

### Requirement: The quit prompt names a shared llama-server that is answering
When "Quit? (y/n)" opens, lazarobox MUST also ask, in the background, whether the managed llama-server it started is answering a request that is none of its own turns (another lazarobox that uses it: `GET /slots` shows more slots processing than its turns in progress). When it is, the prompt MUST name it after the jobs, once, as `llama-server (shared)`, in the same list and with the same limits as the jobs (see The quit prompt names what still runs), whatever arrives first. A server it only uses (started by another lazarobox or by hand), one that is not running, or one whose `/slots` does not answer, is never named. An answer that arrives after the prompt closed MUST be dropped. Tabs and panes of one lazarobox share its server, so they never count.

#### Scenario: Another lazarobox is being answered [auto]
- GIVEN a pane that started `sleep 77 &`, and the managed server answering another lazarobox
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? still running: sleep 77, llama-server (shared) (y/n)" and `y` emits Quit

#### Scenario: Only our own turn [auto]
- GIVEN the managed server answering one request, which is a turn of this lazarobox
- WHEN the runtime is asked
- THEN the server is not named

#### Scenario: Two lazarobox [manual]
- GIVEN two lazarobox, the second using the first one's llama-server, with a reply streaming in the second
- WHEN `q` is pressed in the first
- THEN its prompt names `llama-server (shared)`; with no reply streaming, it does not

### Requirement: Run-in-pane binding
The root of the prefix table MUST gain a leaf for the run-in-pane action of pane-commands, with a non-empty description, on the letter `e`, which collides with no existing root key or group. It MUST be left out of the root key hint if the hint would exceed 80 columns, as `o`, `s` and `T` are. On a focused chat pane showing the conversation view it MUST open the pending confirm of pane-commands; on a shell pane, on a chat list or editor view, and with tools off it MUST only show a notice and emit nothing. It MUST NEVER write to a PTY by itself.

#### Scenario: Leaf in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN the leaf exists at the root with a non-empty description, no duplicate key exists and the root hint is at most 80 columns

#### Scenario: Opens the confirm [auto]
- GIVEN PREFIX on a chat pane whose last answer has a bash block
- WHEN the binding is pressed
- THEN a pending confirm exists, mode is TERMINAL and no WritePty is emitted

#### Scenario: Shell pane and other views [auto]
- GIVEN PREFIX on a shell pane, and on a chat pane showing the conversation list
- WHEN the binding is pressed
- THEN only a notice appears and no effect is emitted

#### Scenario: Inside a group does nothing special [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN the binding letter is pressed
- THEN no confirm opens

### Requirement: Approval mode binding
The root of the prefix table MUST gain a leaf on the letter `a` that cycles the approval mode of the focused chat pane, `ask`, `auto`, `run`, back to `ask` (see agent-tools, Chat approval mode), with a non-empty description, left out of the root key hint like `e`. No direct Ctrl+letter key is added and `run` has no key of its own. Entering `run` needs no extra confirmation. On a focused chat pane in any chat view it MUST advance the mode and add a system line naming the new mode and what it does (see chat-pane), also while an approval is pending (which stays pending). On a shell pane, and with tools off, it MUST only show a notice and change nothing. It MUST NEVER write to a PTY.
(Previously: the leaf toggled between `ask` and `auto`)

#### Scenario: Leaf in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `a` exists only at the root with a non-empty description, no duplicate key exists and the root hint is unchanged and at most 80 columns

#### Scenario: Cycles the focused chat [auto]
- GIVEN PREFIX on a chat pane in `ask`
- WHEN `a` is pressed, then PREFIX and `a` again, then PREFIX and `a` a third time
- THEN the mode is `auto`, then `run`, then `ask`, mode is TERMINAL after each and no effect writes to a PTY

#### Scenario: No confirmation to enter run [auto]
- GIVEN a chat in `auto`
- WHEN PREFIX and `a` are pressed
- THEN the mode is `run` at once, with a system line, and no prompt or modal opens

#### Scenario: Pending approval survives the cycle [auto]
- GIVEN a chat with a pending `run_in_pane` confirm
- WHEN `a` is pressed until the mode is `run`
- THEN the confirm is still pending and nothing is written

#### Scenario: Shell pane and tools off [auto]
- GIVEN PREFIX on a shell pane, and in another run tools off on a chat pane
- WHEN `a` is pressed
- THEN only a notice appears and no mode changes

#### Scenario: Inside a group [auto]
- GIVEN group `w` pending, or COPY, or RESIZE
- WHEN `a` is pressed
- THEN no mode changes

### Requirement: Approval keys
While a chat pane has a pending approval (pane command or tool call) and is focused in TERMINAL, keys MUST act as follows and every other key MUST be swallowed (no composer edit, no WritePty, no Quit):

| Key | Effect |
|---|---|
| `y` (unmodified, lowercase, Press) | approve |
| `n`, Esc | decline |
| `p` (unmodified, pane command only) | move the target pane to the next shell pane |
| Ctrl+C | cancel the stream and deny the approval; never Quit |
| Ctrl+Space | enter PREFIX as in any pane |

Any other key, `Y`, and `y` with a modifier MUST NOT approve; they are swallowed (the prompt stays). Only `n` and Esc decline. The approval is per-chat state routed before the chat key table; it is not an `InputMode` variant, so it survives focus and tab changes. Repeat and Release MUST NOT answer. A paste MUST NOT answer and MUST NOT be inserted into the composer while the approval is pending. While MENU is open no key reaches the approval. In the other modes (PREFIX, COPY, RESIZE) the approval is not answered and stays pending.

#### Scenario: y approves, n and Esc decline [auto]
- GIVEN a pending pane-command approval
- WHEN `y`, and in other runs `n` and Esc, are pressed
- THEN `y` emits one WritePty to the target and the others emit none; the approval is closed each time

#### Scenario: Not approving keys [auto]
- GIVEN a pending approval
- WHEN `Y`, Enter, Space, Ctrl+y and Alt+y are pressed
- THEN no WritePty is emitted and the approval is still pending

#### Scenario: Repeat and Release [auto]
- GIVEN a pending approval
- WHEN Repeat and Release events for `y` arrive
- THEN nothing is answered

#### Scenario: Paste does not answer [auto]
- GIVEN a pending approval and the composer holding `draft`
- WHEN a paste of `y` arrives
- THEN the approval is pending and the composer still holds `draft`

#### Scenario: Cycle changes only the target [auto]
- GIVEN a pending approval with shell panes 1 and 2
- WHEN the cycle key is pressed
- THEN the target changes and no WritePty is emitted

#### Scenario: Ctrl+C denies, never quits [auto]
- GIVEN a streaming chat with a pending approval
- WHEN Ctrl+C is pressed
- THEN a ChatCancel effect is emitted, the approval is denied, and no Quit or WritePty is emitted

#### Scenario: Keys swallowed [auto]
- GIVEN a pending approval
- WHEN `a`, Tab and F5 are pressed
- THEN nothing changes, the composer is untouched and no WritePty is emitted

#### Scenario: Prefix still works [auto]
- GIVEN a pending approval
- WHEN `Ctrl+Space` then `w` then `v` are pressed
- THEN a split is made and the approval is still pending

#### Scenario: MENU shields the approval [auto]
- GIVEN a pending approval and MENU opened with `Ctrl+Space m`
- WHEN `y` is pressed
- THEN the menu handles `y` as an unlisted key and the approval is unanswered

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN a chat with a pending confirm in Kitty
- WHEN the user presses `p`, `y`, and then (in another run) `n`
- THEN each key behaves as listed and none is intercepted by the terminal

### Requirement: Pane removal and the pending approval
A pending approval is not cancelled by the removal of a pane other than its chat pane and its target pane. If the target pane is removed, `y` MUST be refused with a notice and the cycle key stays usable (see pane-commands); if the chat pane is removed the approval ends denied. This is the exception to "Any pane removal while a confirmation is pending MUST cancel it" of Close confirmations, which keeps applying to the close-pane, close-tab and quit confirmations.

#### Scenario: Other pane removed [auto]
- GIVEN a pending approval on pane 2 and a third pane is closed
- WHEN `y` is pressed
- THEN the write goes to pane 2

#### Scenario: Target removed [auto]
- GIVEN a pending approval on pane 2 and pane 2 exits
- WHEN `y` is pressed
- THEN no WritePty is emitted and a notice says the pane is gone

#### Scenario: Close prompts still cancel on removal [auto]
- GIVEN "Close pane? (y/n)" pending and a pane removal
- WHEN `y` is pressed afterwards
- THEN no pane is closed, as before
