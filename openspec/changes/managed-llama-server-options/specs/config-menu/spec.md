# Config Menu Specification (delta: managed llama-server options)

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The tabs are `Settings` and `AI`, in that order (see Menu tabs). The `Settings` tab MUST show exactly seven rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom), `Mouse` (`mouse`: on, off; written as the booleans `true` and `false`), `Notify` (four modes: auto, desktop, bell, off), `Sound` (off, on), `llama-server idle` (`ai.servers.llamacpp.idle_minutes`: 5 min, 15 min, 30 min, never; written as a TOML integer, never = 0) and `Updates` (the running version, or the newer one when known). `Notify`, `Sound` and `llama-server idle` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached. The `AI` tab MUST show the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static rows `Profile` (`ai.profile`: analyst, programmer), `Context` (`ai.context`: off, on), `Memory` (`ai.memory.enabled`: off, on) and `Tools` (off, on), which is the last AI row.

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every section has a non-empty title, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context` and `Memory` have their key paths and values above, every tab lists only rows that exist, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound`, `llama-server idle` (`15 min`) and `Updates`, each with its current value, and the AI rows are not drawn

#### Scenario: AI rows end with Tools [auto, TestBackend + insta]
- GIVEN the AI tab active
- WHEN rendered
- THEN the rows read `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`, `Context`, `Memory`, `Tools` in that order, and no `Engine`, `Model`, `Status`, `Notify` or `Sound` row exists there

#### Scenario: No AI hides Notify and Sound [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse` and `Updates`, and there is no AI tab

### Requirement: Notify row
`Notify` is a static row of the `Settings` tab (after `Mouse`, before `Sound`), found by `ItemKind::Notify`, never by position. It edits the AI draft, so it is shown only when the AI section is attached. It reads one of four modes from the AI draft: `auto`, `desktop`, `bell`, `off`. `h`, `l`, arrows, Space and Enter cycle the mode with wrap (an Enter key repeat is ignored like on `Context`), with no relayout and no other effect. `t` tests the notification (see Notify, Sound and Updates keys). The footer is `j/k move · h/l change · t test · s save · Esc cancel`. Only a changed value is an edit (`ai.notify`), placed after the memory edits.

#### Scenario: Row order and navigation [auto]
- GIVEN the menu with the AI section, Settings tab active
- WHEN navigating down from `Mouse`
- THEN the selection reaches `Notify`, then `Sound`, then `llama-server idle`, then `Updates`, then wraps to `Statusline position`

#### Scenario: Reads the mode [auto]
- GIVEN drafts with notify `auto`, `desktop`, `bell` and `off`
- WHEN the rows are built
- THEN the value is `auto`, `desktop`, `bell` and `off` respectively; without the AI section there is no row

#### Scenario: Cycle and edit [auto]
- GIVEN the selection on `Notify`
- WHEN a cycle key is pressed through all four modes
- THEN the draft returns to the starting mode after four presses, the command is `None` each time and the edit list holds `ai.notify` only while the draft differs from the open value

#### Scenario: Snapshots [auto]
- GIVEN the Settings tab snapshots
- WHEN regenerated
- THEN `Notify` and `Sound` appear on the Settings tab and not on the AI tab
