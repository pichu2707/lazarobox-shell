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
The root of the prefix tree MUST also contain the leaf binding `m` with description `menu`, entering MENU. The root key hint therefore includes `m menu`; the pinned list of root hint entries changes from `w t g b [ q` to include `m`. All other bindings and rules of the requirement are unchanged.
(Previously: root entries were `w`, `t`, `g`, `b`, `[`, `q`)

#### Scenario: `m` is in the table [auto]
- GIVEN the prefix table
- WHEN it is validated
- THEN `m` exists at the root with a non-empty description, and the table integrity rules still hold

### Requirement: TERMINAL mode
Repeat MUST be handled like Press in TERMINAL, COPY, RESIZE and in MENU for navigation and cycling keys only; it MUST be ignored for Enter and Esc in MENU and in decision states. Release is ignored in every mode.
(Previously: Repeat handled in TERMINAL, COPY and RESIZE; ignored in decision states)

#### Scenario: Repeat in MENU [auto]
- GIVEN MENU
- WHEN Repeat events for `j` and for Esc arrive
- THEN `j` moves the selection and Esc does nothing

### Requirement: Paste and resize in any mode
Paste events MUST be sent to the focused pane's PTY only in TERMINAL (so never in MENU). A host Resize event in MENU recomputes rects with the forced tab bar and previewed positions, emits ResizePty only for panes whose size changed, and keeps mode MENU.
(Previously: same rule without MENU)

#### Scenario: Paste in MENU [auto]
- GIVEN MENU
- WHEN a paste event arrives
- THEN no WritePty is emitted

## REMOVED Requirements

(None.)
