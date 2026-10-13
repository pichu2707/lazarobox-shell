# Delta for Config Menu

Tags: [auto] = `cargo test`.

## ADDED Requirements

### Requirement: Notify row
The AI section MUST end with a static row `Notify` after `Memory` (order: Set all tasks, general, code, chat, review, summarize, Profile, Context, Memory, Notify). It is found by `ItemKind::Notify`, never by position, and the popup body height (`AI_BODY`) grows with it automatically. It reads `off` or `on` from the AI draft; `h`, `l`, arrows, Space and Enter toggle the draft (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect; the footer is the settings hint. Only a changed value is an edit (`ai.notify`, `true`/`false`), placed after the memory edits.

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section
- WHEN navigating down from Memory
- THEN the selection reaches Notify and then wraps to the first settings row; going up from there returns to Memory

#### Scenario: Reads on or off [auto]
- GIVEN drafts with notify `false` and `true`
- WHEN the rows are built
- THEN the value is `off` and `on`; without the AI section there is no row

#### Scenario: Toggle and edit [auto]
- GIVEN the selection on Notify
- WHEN any cycle key is pressed
- THEN the draft flips, `MenuCommand::None` is returned and the edit list holds `ai.notify` only while the draft differs from the open value

#### Scenario: Snapshots [auto]
- GIVEN the AI menu snapshots
- WHEN regenerated
- THEN the only difference is the extra `Notify` row
