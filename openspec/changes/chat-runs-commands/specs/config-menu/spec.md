# Delta for Config Menu (chat-runs-commands)

## MODIFIED Requirements

### Requirement: Approval row
The Tools tab MUST hold a static row `Approval` after the family rows and before `Orchestrator` (key path `ai.tools.approval`, values `ask`, `run`; it reads `ask` by default, and when the file holds `auto` it reads `ask`). It is found by its own `ItemKind`, never by position. `h`, `l`, Left, Right, Space and Enter cycle the draft `ask`, `run`, `ask` (an Enter repeat is ignored); the footer is the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and `r` do not apply. Entering `run` needs no confirmation. Changing the draft MUST have no live effect: no relayout, no ResizePty or WritePty, no change to the mode of any open chat pane, a pending approval or a running tool; after a successful save it applies to chat panes opened afterwards and at the next conversation switch inside an open pane. Esc at the top level restores the draft; `s` saves `ai.tools.approval` through config-persistence only when it differs, and the value is written explicitly even when it equals the default. The row is shown whatever the `Tools` draft. With the row selected, the tab's caution line shows its own ASCII hint, `ask: y runs; run: safe reads alone, rest y; per chat` (52 characters, the maximum), within the popup width. The popup keeps its size (16 rows and a 24x15 minimum then, 17 and 24x16 with the Usage row; see Popup height is fixed); the descriptor integrity rules extend to the row.
(Previously: values `ask`, `auto`, `run`, and the hint `auto types; run runs safe reads only; per chat`.)

Note: when the file holds a bad value (for example `approval = "AUTO"`), the row opens on `ask` and choosing `ask` writes nothing, so the old value stays in the file, and its notice stays at each start, until `run` is saved or the user edits the file. This is accepted (design D18); no force flag exists. The removed value `auto` is different: load rewrites it to `"ask"` once (see configuration, Approval mode key), so the file holds `ask` before the menu opens; only if that rewrite failed does the old value stay.

#### Scenario: Cycle [auto]
- GIVEN `Approval` selected with the draft `ask`
- WHEN `l` is pressed twice
- THEN the draft is `run`, then `ask`, no save is requested and no file is touched, and `auto` is never shown

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

#### Scenario: Config holding auto opens on ask [auto]
- GIVEN a file with `approval = "auto"`
- WHEN the Tools tab opens
- THEN the row reads `ask`, the file holds `approval = "ask"`, and the startup notice was `config: ai.tools.approval: "auto" was removed; the file now says "ask"` (or, when the rewrite failed, the failure notice of configuration)

#### Scenario: Footer [auto, TestBackend + insta]
- GIVEN `Approval` selected
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel` and the value reads `ask`

#### Scenario: The Approval row has its own hint [auto, TestBackend]
- GIVEN the Tools tab with `Approval` selected, at 62 and at 80 columns
- WHEN it is rendered
- THEN the caution line reads `ask: y runs; run: safe reads alone, rest y; per chat` once, the text "Can change files or run code" is not shown, and the popup keeps its size

#### Scenario: Run value rendered [auto, TestBackend + insta]
- GIVEN the draft is `run`
- WHEN rendered
- THEN the row value reads `run` and no other row or the popup height changes
