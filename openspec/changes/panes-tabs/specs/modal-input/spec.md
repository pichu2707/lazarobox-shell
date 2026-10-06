# Delta for Modal Input

All scenarios [auto] unless tagged. Pure `update(event) -> effects`.

## ADDED Requirements

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
| `q` | quit (confirm) |

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

## MODIFIED Requirements

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be encoded and sent to the focused pane's PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY and RESIZE, and MUST be ignored in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action or confirms.
(Previously: Release ignored only generally; Repeat ignored only in PREFIX and CONFIRM_QUIT; bytes went to the single PTY)

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

### Requirement: Paste and resize in any mode
Paste events MUST be sent to the focused pane's PTY only in TERMINAL. A host Resize event MUST recompute pane rects in every mode and MUST emit ResizePty only for panes (in any tab) whose size actually changed.
(Previously: Resize emitted a single ResizePty in every mode)

#### Scenario: Paste in COPY
- GIVEN COPY
- WHEN a paste event arrives
- THEN no WritePty is emitted

#### Scenario: Resize only changed panes
- GIVEN two panes and a host resize that changes only the width of the right pane
- WHEN the Resize event is handled
- THEN ResizePty is emitted only for panes whose rect size changed

## REMOVED Requirements

(None. "Enter COPY" is unchanged: `[` in PREFIX enters COPY on the focused pane.)
