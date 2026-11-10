# Delta for Config Menu

## Base of this delta

This delta targets `openspec/specs/config-menu/spec.md`. The MODIFIED block is a full copy of the requirement "Notify, Sound and Updates keys (drift fix)" (openspec/specs/config-menu/spec.md:919-935) as it stands on e480992e. Only the Updates sentences change: Enter, which has no effect once an update was installed in this run (src/core/menu.rs:3033, src/app.rs:1986-2011), now asks for the reload. Scenarios are added at the end. The key and where it appears are Open question 1 of design.md; this delta writes its recommendation (A).

## MODIFIED Requirements

### Requirement: Notify, Sound and Updates keys (drift fix)
On the `Notify` or `Sound` row, `t` (Press only, not Repeat) MUST send a test notification using the draft setting, and the footer is `j/k move · h/l change · t test · s save · Esc cancel`; the outcome is shown in the footer until the next key press. On the `Updates` row, Enter MUST request the update when a newer version is known and none is installed yet; with the install command shown, Enter or `y` MUST copy it; once a version was installed in this run, Enter MUST request the reload (see reload-in-place); otherwise Enter has no effect (it does not cycle). The `Updates` row reads the running version, or `<latest> available` with the label `* Updates` when newer is known, or `<installed> installed: Enter reloads` once a version was installed in this run, with the footer `Updated to <installed> · Enter reloads · Esc cancel`. A refused reload MUST be shown in the footer as its reason until the next key press, and the menu stays open.

#### Scenario: Test notification [auto]
- GIVEN `Notify` selected and the AI section attached
- WHEN `t` is pressed, and a Repeat event for `t` arrives
- THEN exactly one test is requested, with the draft mode

#### Scenario: `t` elsewhere is inert [auto]
- GIVEN `Mouse` selected
- WHEN `t` is pressed
- THEN no test is requested and nothing changes

#### Scenario: Enter on Updates [auto]
- GIVEN `Updates` selected with a newer version known and none installed
- WHEN Enter is pressed
- THEN an update is requested; with no newer version known, Enter has no effect

#### Scenario: Enter reloads after an install [auto]
- GIVEN `Updates` selected and `0.16.0` installed in this run
- WHEN the row and footer are drawn, and Enter is pressed
- THEN the row reads `0.16.0 installed: Enter reloads`, the footer `Updated to 0.16.0 · Enter reloads · Esc cancel`, and a reload is requested

#### Scenario: A refused reload is shown [auto]
- GIVEN `0.16.0` installed in this run and chat pane 3 answering
- WHEN Enter is pressed on `Updates`
- THEN the footer reads `reload refused: chat 3: the model is answering`, the menu stays open, and the next key press clears it
