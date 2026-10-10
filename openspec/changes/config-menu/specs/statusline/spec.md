# Delta for Statusline (config-menu)

All scenarios [auto] unless tagged. Builds on the panes-tabs statusline spec. Existing snapshots for other modes MUST remain valid.

## MODIFIED Requirements

### Requirement: Mode block
The statusline MUST show the current mode label (TERMINAL, PREFIX, COPY, RESIZE, MENU, or the group label while a group is pending) styled with that mode's color from `mode_style`.

MENU MUST have its own accent color from the theme palette, different from the colors of TERMINAL, PREFIX, COPY, RESIZE and the confirmations (the specific palette entry is a design decision). The separator accent next to the focused pane follows the same mapping. All existing mode colors are unchanged. While MENU is active the path segment shows the usual cwd or notice; the root hint is not shown.
(Previously: no MENU label)

#### Scenario: MENU label and color [auto, TestBackend + insta]
- GIVEN MENU
- WHEN rendered
- THEN the label `MENU` is shown in its accent color

#### Scenario: Mode color mapping includes MENU [auto]
- GIVEN each input mode
- WHEN `input_mode_style` is evaluated
- THEN MENU's style differs from every other mode's style and the other mappings are unchanged

#### Scenario: Existing snapshots [auto]
- GIVEN the pre-existing statusline snapshots
- THEN they still pass unchanged, except the root hint snapshot which now includes `m menu`

### Requirement: Group-pending key hint
The root hint built from the prefix tree MUST include the entry `m menu`, in table order, with the same clipping rule (whole entries, ending with `…`). Group hints are unchanged.
(Previously: the root hint was `w window · t tab · g go · b buffer · [ copy · q quit`)

#### Scenario: Root hint with menu [auto, TestBackend + insta]
- GIVEN PREFIX with no group pending on a wide terminal
- WHEN rendered
- THEN the path segment contains `m menu` among the root entries and still omits focus keys, the literal Ctrl+Space and `?`

#### Scenario: Root hint stays table-generated [auto]
- GIVEN the root of the prefix tree
- WHEN its hint is built
- THEN it contains exactly the root entries that are listed (groups and leaf bindings, including `m menu`), in table order

## REMOVED Requirements

(None.)
