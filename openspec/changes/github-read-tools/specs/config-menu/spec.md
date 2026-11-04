# Delta for Config Menu

## ADDED Requirements

### Requirement: GitHub row on the Tools tab
The Tools tab MUST hold a `GitHub` row, an on/off toggle that reads and writes `[ai.tools] github` (see configuration), placed after `Git write` and before `Approval`. It cycles like the other family rows (Value cycling) and is saved with the rest of the draft. The row shows the saved value only: a chat grant never changes it. Turning the row on while `Tools` is off follows the same rule as the other family rows. This row changes the Tools tab order to `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `GitHub`, `Approval`, `Orchestrator`; the scenarios `Every row is on its tab`, `Tools tab in order` (now nine presses to wrap) and `Wrap within Models, Tools and Chat` MUST be read with that order. The tab MUST fit 80 columns and `TERM=linux`.

#### Scenario: Tools tab rows [auto, TestBackend + insta]
- GIVEN the AI section attached
- WHEN the Tools tab is rendered
- THEN it reads `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `GitHub`, `Approval`, `Orchestrator`

#### Scenario: Tools tab in order [auto]
- GIVEN the Tools tab active with `Tools` selected
- WHEN `j` is pressed nine times
- THEN the selection visits `Search`, `Write`, `Exec`, `Git`, `Git write`, `GitHub`, `Approval`, `Orchestrator` in that order and the ninth press wraps to `Tools`

#### Scenario: Toggle is saved [auto]
- GIVEN the `GitHub` row off
- WHEN it is switched on and the menu is saved
- THEN the file holds `github = true` under `[ai.tools]` and the next turn offers the three tools

#### Scenario: Grant does not show in the menu [auto]
- GIVEN a chat granted with `/github` and the saved toggle off
- WHEN the menu opens
- THEN the `GitHub` row reads off

#### Scenario: Row at 80 columns and TERM=linux [auto, TestBackend + insta]
- GIVEN a width of 80, and in another run a `linux` console
- WHEN the Tools tab is rendered
- THEN the `GitHub` row is drawn whole, in ASCII on the console
