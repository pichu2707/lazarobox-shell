# Delta for Statusline (copy-selection)

All scenarios [auto] unless tagged. Existing statusline snapshots MUST remain valid when no selection or copy notice exists.

## ADDED Requirements

### Requirement: Selection marker
While in COPY with a selection, the mode block MUST show `VISUAL` (character selection) or `V-LINE` (line selection), separated by one space, after the position indicator (or directly after `COPY` when the pane has no history) and before `[Z]`. It MUST NOT be shown without a selection. The path budget MUST shrink by its width and narrow widths MUST NOT panic.

#### Scenario: Marker with indicator [TestBackend + insta]
- GIVEN COPY on a pane with 340 rows of history, scrolled 12 up, with a character selection
- WHEN rendered
- THEN the mode block reads `COPY ↑12/340 VISUAL`

#### Scenario: Line marker without history
- GIVEN COPY on the alternate screen with a line selection
- WHEN rendered
- THEN the mode block reads `COPY V-LINE`

#### Scenario: Marker gone
- GIVEN a selection
- WHEN Esc clears it
- THEN the mode block reads `COPY ↑{offset}/{total}` without a marker

#### Scenario: Narrow width
- GIVEN a selection and any width from 0 to 40
- WHEN rendered
- THEN rendering does not panic

### Requirement: Copy notices
After a yank the path segment MUST show `Copied N characters`, where N is the number of Unicode characters of the copied text (newlines included), or `Copied 1 character` when N is 1. After an over-cap yank it MUST show `Selection too large to copy`, and after a yank with empty text `Nothing to copy`. Both use the same notice slot as the spawn-failure notice: shown instead of the cwd until the next key press, replaced by any newer notice.

#### Scenario: Copied notice
- GIVEN the selection `hello`
- WHEN `y` is pressed and the statusline is rendered
- THEN the path segment shows `Copied 5 characters` and the mode block shows `TERMINAL`

#### Scenario: Multi-row count
- GIVEN the selection `foo\nbar`
- WHEN `y` is pressed
- THEN the path segment shows `Copied 7 characters`

#### Scenario: Singular notice
- GIVEN the selection `x`
- WHEN `y` is pressed
- THEN the path segment shows `Copied 1 character`

#### Scenario: Too-large notice
- GIVEN a selection over the size cap
- WHEN `y` is pressed
- THEN the path segment shows `Selection too large to copy`

#### Scenario: Nothing-to-copy notice
- GIVEN a selection whose text is empty (blank rows)
- WHEN `y` is pressed
- THEN the path segment shows `Nothing to copy` and the mode block shows `COPY`

#### Scenario: Notice cleared by next key
- GIVEN the `Copied 5 characters` notice is shown
- WHEN any key is pressed
- THEN the path segment shows the cwd again
