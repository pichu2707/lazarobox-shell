# Delta for Modal Input (config-menu)

All scenarios [auto] unless tagged. Pure `update(event) -> effects`. Builds on the panes-tabs modal-input delta.

## ADDED Requirements

### Requirement: MENU mode
`m` in PREFIX MUST enter MENU. MENU is a sticky mode that swallows input: the keys defined by the config-menu spec act, every other key (including Ctrl+Space) is swallowed with no WritePty, and the mode stays MENU until Enter saves successfully or Esc cancels. MENU MUST be entered only from TERMINAL through the prefix; `m` inside a group, in COPY, RESIZE or a confirmation MUST NOT open it. Press and Repeat are handled for navigation and cycling keys (Repeat on Enter and Esc MUST be ignored so a held key cannot save or cancel repeatedly); Release is ignored. Paste events MUST NOT reach any PTY in MENU. A focus or active-tab change MUST NOT leave MENU.

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

## MODIFIED Requirements

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
| `q` | quit (confirm) |

The root of the prefix tree also contains the leaf binding `m` with description `menu`, entering MENU; the root key hint therefore includes `m menu`. The pinned list of root hint entries changes from `w t g b [ q` to include `m`.
(Previously: root entries were `w`, `t`, `g`, `b`, `[`, `q`)

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

### Requirement: TERMINAL mode
In TERMINAL every key except the prefix MUST be encoded and sent to the focused pane's PTY. Initial mode is TERMINAL.

Key events of kind Release MUST be ignored in every mode. Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU and in decision states (PREFIX, group pending, and all confirmations) so a held key never selects an action or confirms.
(Previously: Release ignored only generally; Repeat ignored only in PREFIX and CONFIRM_QUIT; bytes went to the single PTY; then Repeat handled in TERMINAL, COPY and RESIZE and ignored in decision states)

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

### Requirement: Paste and resize in any mode
Paste events MUST be sent to the focused pane's PTY only in TERMINAL (so never in MENU). A host Resize event MUST recompute pane rects in every mode and MUST emit ResizePty only for panes (in any tab) whose size actually changed. In MENU the recomputation uses the forced tab bar and the previewed positions, and the mode stays MENU.
(Previously: Resize emitted a single ResizePty in every mode; then the same rule without MENU)

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

## REMOVED Requirements

(None.)
