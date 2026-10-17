# Delta for Config Menu

Applies on top of the unarchived `agent-tools` delta for config-menu. Tags as in the main spec.

## MODIFIED Requirements

### Requirement: Approval row
The AI tab MUST end with a static row `Approval` after `Tools` (key path `ai.tools.approval`, values `ask`, `auto`, `run`; it reads `ask` by default). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft `ask`, `auto`, `run`, `ask` (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Entering `run` needs no confirmation. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards and at the next conversation switch inside an open pane. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs, and the value is written explicitly even when it equals the default. The row is shown whatever the `Tools` draft. With it the AI tab has 11 rows and the popup keeps its 16 rows and its 24x15 minimum, because the row only gains a value (see Popup height is fixed); the descriptor integrity rules extend to the row.
(Previously: values `ask`, `auto`)

Note: when the file holds a bad value (for example `approval = "AUTO"`), the row opens on `ask` and choosing `ask` writes nothing, so the bad value stays in the file until `auto` or `run` is saved. `Tools` behaves the same way. This is accepted (design D18); no force flag exists.

#### Scenario: Cycle [auto]
- GIVEN `Approval` selected with the draft `ask`
- WHEN `l` is pressed three times
- THEN the draft is `auto`, then `run`, then `ask`, no save is requested and no file is touched

#### Scenario: Enter repeat ignored [auto]
- GIVEN `Approval` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: No effect on open chats [auto]
- GIVEN an open chat in `ask` and the menu open on `Approval`
- WHEN the draft is cycled to `run` and saved
- THEN the open chat is still `ask`, no WritePty is emitted, and a chat pane opened afterwards is `run`

#### Scenario: Esc reverts [auto]
- GIVEN the draft changed to `run`
- WHEN Esc is pressed on the task list
- THEN the saved mode is still `ask` and no file is read or written

#### Scenario: Footer [auto, TestBackend + insta]
- GIVEN `Approval` selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and the value reads `ask`

#### Scenario: Run value rendered [auto, TestBackend + insta]
- GIVEN the draft is `run`
- WHEN rendered
- THEN the row value reads `run` and no other row or the popup height changes

#### Scenario: Popup size is unchanged [auto, TestBackend + insta]
- GIVEN the AI tab with `Approval`, a terminal of 80x24 and one of exactly 24x15
- WHEN rendered
- THEN both show the full popup of 16 rows (the 24x15 one clipped, without panic), and one column or one row less shows the one-line fallback

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN `Approval` exists with key path `ai.tools.approval` and values `ask`, `auto`, `run`, and no key path is duplicated

#### Scenario: Approval row in Kitty [manual, Kitty]
- GIVEN the menu in Kitty
- WHEN `Approval` is set to `run` and saved, and a new chat is opened
- THEN the new chat shows ` RUN `, the chat opened before does not, and the file holds `approval = "run"` under `[ai.tools]`
