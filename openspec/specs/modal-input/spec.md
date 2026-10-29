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
Ctrl+Space (reported as NUL 0x00) MUST enter PREFIX without writing to the PTY. The prefix key and the action table MUST be data, not hard-coded branches. Pressing Ctrl+Space again in PREFIX writes a literal NUL to the focused pane. Esc or an unmapped key in PREFIX returns to TERMINAL, swallowed.
(Previously: flat action table; same cancel and literal behavior, now addressed to the focused pane)

#### Scenario: Enter PREFIX
- GIVEN TERMINAL
- WHEN Ctrl+Space is pressed
- THEN mode is PREFIX and no WritePty is emitted

#### Scenario: Literal prefix
- GIVEN PREFIX
- WHEN Ctrl+Space is pressed again
- THEN WritePty `[0x00]` to the focused pane is emitted and mode is TERMINAL

#### Scenario: Cancel/unknown
- GIVEN PREFIX
- WHEN Esc or an unmapped key (e.g. `z`) is pressed
- THEN mode is TERMINAL and the key is swallowed (no WritePty)

#### Scenario: Real terminal reports NUL [manual, Kitty]
- GIVEN Kitty
- WHEN Ctrl+Space is pressed
- THEN PREFIX is entered and nothing is written to the PTY (the root which-key hint appears in the statusline while PREFIX is pending, and the group hint once a group such as `w` is pending; see statusline)

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
