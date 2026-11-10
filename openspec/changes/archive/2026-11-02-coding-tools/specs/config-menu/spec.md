# Delta for Config Menu

## Base of this delta

`openspec/specs/config-menu/spec.md` exists. The `Tools` row is added by the unarchived `agent-tools` change, so `agent-tools` MUST be archived first. Only ADDED requirements are used. Row labels and the exact order are provisional and pinned by design.

## ADDED Requirements

### Requirement: Family toggle rows
The AI tab MUST hold five static rows, one per family toggle (Search, Write, Exec, Git, GitWrite), placed after the `Tools` row and before `Approval`. Each reads `off` by default, with values `off`, `on` and the key path of its config key. Each is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter toggle the draft; an Enter repeat is ignored. Changing a draft MUST have no live effect and applies from the next turn after a successful save. Esc restores the draft; `s` saves only the keys that differ. The Write, Exec and GitWrite rows MUST show a one-line caution that the family can change files or run code. Navigation wraps inside the AI tab, the popup does not change size, and the descriptor integrity rules extend to the new rows (no duplicate key path).

#### Scenario: Navigation reaches every row [auto]
- GIVEN the AI tab with `Tools` selected
- WHEN `j` is pressed five times
- THEN the five family rows are selected in order and the sixth press selects `Approval`

#### Scenario: Toggle [auto]
- GIVEN the Write row with the draft `off`
- WHEN `l` is pressed, then `h`
- THEN the draft is `on`, then `off`, and no file is touched

#### Scenario: Save [auto]
- GIVEN the Exec draft changed to `on`
- WHEN `s` is pressed and the save succeeds
- THEN the file holds the exec key as `true` and the next turn offers `run_command`

#### Scenario: No live effect [auto]
- GIVEN a running stream with a pending approval and the Write draft cycled
- WHEN the draft changes
- THEN no ResizePty or WritePty is emitted and the stream and approval are untouched

#### Scenario: Caution line [auto, TestBackend + insta]
- GIVEN the Exec row selected
- WHEN the tab is rendered
- THEN the caution line is visible and the popup size is unchanged

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN the five rows exist with distinct key paths and values `off`, `on`
