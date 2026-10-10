# Delta for Statusline

One row at the configured edge (default bottom), always reserved. Existing statusline snapshots for TERMINAL, PREFIX, COPY and Quit MUST remain valid. All scenarios [auto] unless tagged.

## ADDED Requirements

### Requirement: Group-pending key hint
While a group is pending (after `w`, `t`, `g` or `b`) the mode block MUST show the group label (`WINDOW`, `TAB`, `GO`, `BUFFER`) in the PREFIX color, and the path segment MUST show a one-line key hint generated from the prefix tree: each binding of the group as `{key} {description}`, in table order, joined by ` · `. For group `w` the hint is `v split right · h split below · q close · r resize · z zoom`. When the hint does not fit the available width it MUST be clipped by whole entries and end with `…`; it MUST never show a partial entry. The hint is minimal: the full `?` command viewer is out of scope.

With plain PREFIX (no group yet) the path segment MUST show the same kind of hint built from the root of the prefix tree, using the same function and clipping rule: `w window · t tab · g go · b buffer · [ copy · q quit`. A group entry shows its short description; focus keys, the literal Ctrl+Space and the reserved `?` are not listed.

#### Scenario: Root hint in PREFIX [auto, TestBackend + insta]
- GIVEN PREFIX with no group pending on a wide terminal
- WHEN rendered
- THEN the mode block shows `PREFIX` and the path segment shows `w window · t tab · g go · b buffer · [ copy · q quit`

#### Scenario: Root hint clipped and cleared [auto]
- GIVEN PREFIX and a path segment too narrow for the full root hint
- WHEN rendered, and then Esc is pressed
- THEN the hint shows only whole entries followed by `…`, and after Esc the statusline shows TERMINAL and the cwd

#### Scenario: Group hint [auto, TestBackend + insta]
- GIVEN group `w` pending on a wide terminal
- WHEN rendered
- THEN the mode block shows `WINDOW` and the path segment shows `v split right · h split below · q close · r resize · z zoom`

#### Scenario: Hint is generated from the table [auto]
- GIVEN each group `w`, `t`, `g` and `b`
- WHEN its hint is built
- THEN it contains exactly the key and description of every binding in that group, in table order

#### Scenario: Hint clipped by whole entries [auto]
- GIVEN group `w` pending and a path segment too narrow for the full hint
- WHEN rendered
- THEN the hint shows only the entries that fit followed by `…`, with no partial entry

#### Scenario: Hint gone after the group resolves [auto]
- GIVEN group `w` pending
- WHEN `v` is pressed
- THEN the statusline shows the TERMINAL label and the cwd again

#### Scenario: Narrow width [auto]
- GIVEN group pending on a very narrow terminal
- WHEN rendered
- THEN the mode block takes priority, the hint may be empty, and rendering does not panic

### Requirement: Zoom indicator
While the active tab is zoomed the statusline MUST show `[Z]` right after the mode block. It MUST NOT be shown otherwise.

#### Scenario: Zoomed [auto, TestBackend]
- GIVEN a zoomed pane
- WHEN rendered
- THEN `[Z]` is visible after the mode block

#### Scenario: Unzoomed [auto, TestBackend]
- GIVEN the zoom toggled twice
- WHEN rendered
- THEN `[Z]` is not shown

### Requirement: Spawn-failure notice
After a spawn failure the path segment MUST show `spawn failed: {error}` instead of the cwd until the next key press, and a newer notice MUST replace an older one.

#### Scenario: Notice shown [auto, TestBackend]
- GIVEN a spawn failure with error `no such file`
- WHEN rendered
- THEN the path segment shows `spawn failed: no such file`

#### Scenario: Notice cleared by next key [auto]
- GIVEN the notice is shown
- WHEN any key is pressed
- THEN the path segment shows the cwd again

### Requirement: Statusline position
The statusline MUST occupy one row at the edge set by `[statusline] position` (default `"bottom"`). On the same edge as the tab bar, the tab bar is the outermost row and the statusline sits between it and the body. The body keeps `rows - 1 - (tab bar ? 1 : 0)` rows (minimum 1) wherever the bars are.

#### Scenario: Statusline on top [auto]
- GIVEN `position = "top"`, one tab, a 80x25 terminal
- WHEN the app is drawn
- THEN the statusline is row 0, the body is 80x24 from row 1, and the cursor is offset by one row

#### Scenario: Tiny terminal [auto]
- GIVEN any position combination and a terminal down to 0x0
- WHEN the screen is computed
- THEN nothing panics and the body is at least 1x1

## RENAMED Requirements

### Requirement: Quit confirmation visible → Confirmation prompts visible

(Reason: more than one confirmation exists now)
(Migration: tests that reference "Quit confirmation visible" move to the new name; the modified text is below)

## MODIFIED Requirements

### Requirement: Mode block
The statusline MUST show the current mode label (TERMINAL, PREFIX, COPY, RESIZE, or the group label while a group is pending) styled with that mode's color from `mode_style`.

Mode colors (reusing the theme palette, via `input_mode_style`): TERMINAL → `success_green`, PREFIX → `warning_orange`, COPY → `primary_cyan`, any confirmation → `error_red`. RESIZE → `ai_purple`, which no other mode uses. A pending group uses the PREFIX style. The separator accent next to the focused pane follows the same mapping (see pane-layout).
(Previously: no RESIZE label; only ConfirmQuit used the confirmation color)

#### Scenario: Label and color [auto, TestBackend + insta]
- GIVEN each mode, including RESIZE
- WHEN rendered
- THEN the label text and mode color are shown

#### Scenario: Mode color mapping [auto]
- GIVEN each input mode
- WHEN `input_mode_style` is evaluated
- THEN TERMINAL uses `success_green`, PREFIX `warning_orange`, COPY `primary_cyan`, every confirmation `error_red`, and RESIZE `ai_purple`

#### Scenario: Existing snapshots [auto]
- GIVEN the pre-existing statusline snapshots
- THEN they still pass unchanged

### Requirement: Confirmation prompts visible
While a confirmation is pending the statusline MUST show its prompt: "Close pane? (y/n)", "Close tab? (y/n)" or "Quit? (y/n)".
(Previously: only "Quit? (y/n)")

#### Scenario: Close pane prompt [auto]
- GIVEN the close-pane confirmation pending
- WHEN rendered
- THEN "Close pane? (y/n)" is visible

#### Scenario: Close tab prompt [auto]
- GIVEN the close-tab confirmation pending
- WHEN rendered
- THEN "Close tab? (y/n)" is visible

#### Scenario: Quit prompt [auto]
- GIVEN the quit confirmation pending (`q`, or last pane of last tab)
- WHEN rendered
- THEN "Quit? (y/n)" is visible

### Requirement: Live cwd
The statusline MUST show the focused pane's current working directory in the path segment (unless a notice or a key hint replaces it). The value comes from OSC 7 when that pane has reported one, otherwise from the periodic (~1 s) `/proc` poll, and the last value MUST be kept if it cannot be read. Focus changes MUST switch the shown cwd immediately.
(Previously: one shell's cwd from the poll only)

#### Scenario: cwd changes [auto, unix PTY]
- GIVEN the focused shell runs `cd /tmp` and emits no OSC 7
- WHEN the next poll occurs
- THEN the statusline cwd is `/tmp`

#### Scenario: OSC 7 cwd shown [auto]
- GIVEN the focused pane reported OSC 7 `/srv`
- WHEN rendered
- THEN the statusline cwd is `/srv`

#### Scenario: Focus switches cwd [auto]
- GIVEN two panes with cwd `/a` (left, focused) and `/b`
- WHEN focus moves right
- THEN the statusline cwd is `/b`

#### Scenario: Read failure [auto]
- GIVEN the cwd lookup fails
- THEN the previous value is retained (no panic)

### Requirement: Shell segment
The right side MUST show the focused pane's shell name (basename of the shell path).
(Previously: the single shell's name)

#### Scenario: Shell name [auto]
- GIVEN focused pane shell `/bin/zsh`
- WHEN rendered
- THEN `zsh` appears on the right

#### Scenario: Narrow width [auto]
- GIVEN a very narrow terminal
- WHEN rendered
- THEN the mode block takes priority and rendering does not panic

## REMOVED Requirements

(None.)
