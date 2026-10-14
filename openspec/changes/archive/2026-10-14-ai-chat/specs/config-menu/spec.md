# Delta for Config Menu

Tags: [auto] = `cargo test`; [manual] = real Kitty. The AI section gains a `Profile` row after the six task rows. Requirements not listed here (open and close, value cycling, live preview, input swallowed, footer, events while open, detection results, refresh key, AI draft, task picker, custom model entry, picker viewport, detection cues) are unchanged, except as stated in the ADDED requirement. Popup size is a design decision (see AI task list).

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections and items MUST come from a static descriptor table; adding a section or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). The sections are "Settings" and "AI", shown in that order. Settings has the rows, in order: `Statusline position` (`statusline.position`: top, bottom), `Tab bar position` (`tabbar.position`: top, bottom) and a disabled row `Mouse: off (coming soon)`. AI has the assignment rows, in order: `Set all tasks` (aggregate over the five tasks, no single key path), `general` (`ai`), `code` (`ai.tasks.code`), `chat` (`ai.tasks.chat`), `review` (`ai.tasks.review`) and `summarize` (`ai.tasks.summarize`), followed by the static row `Profile` (`ai.profile`: analyst, programmer).
(Previously: AI had only the six assignment rows)

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor table
- WHEN it is validated
- THEN every section has a non-empty title, every item a non-empty label, every static item a non-empty key path and at least two allowed values, the five task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile` has the key path `ai.profile` and the values `analyst`, `programmer`, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened
- WHEN rendered on a normal terminal
- THEN the "Settings" title and the three rows are visible, each setting showing its current value, and the Mouse row is drawn muted; the `AI` section with `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize` and `Profile` follows them, and no `Engine`, `Model` or `Status` row exists

### Requirement: Navigation
`j`, `k`, Down and Up MUST move the selection between the selectable rows of all sections, in display order (`Statusline position`, `Tab bar position`, `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `Profile`). Movement MUST wrap around at the ends (from the last selectable row to the first and back), because the list is short and wrapping never leaves the user on a dead end. The disabled `Mouse` row MUST be skipped: it is drawn but never selected, so no key can interact with it (picker disabled rows are different, see Task picker). Repeat events MUST be handled like Press.
(Previously: the last selectable row was `summarize`)

#### Scenario: Move down and up [auto]
- GIVEN MENU with the first row selected
- WHEN `j` is pressed, then `k`
- THEN the second setting row is selected, then the first again

#### Scenario: Wrap [auto]
- GIVEN the last selectable row (`Profile`) selected
- WHEN `j` (or Down) is pressed
- THEN the first selectable row is selected; and from the first row `k` (or Up) selects the last selectable row

#### Scenario: Disabled row is skipped [auto]
- GIVEN the Tab bar position row selected
- WHEN `j` is pressed
- THEN the selection goes to `Set all tasks`, never to the Mouse row; and from `Set all tasks` `k` goes back to Tab bar position

#### Scenario: Task rows in order [auto]
- GIVEN `Set all tasks` selected
- WHEN `j` is pressed five times
- THEN the selection visits `general`, `code`, `chat`, `review`, `summarize` in that order, a sixth `j` selects `Profile`, and a seventh wraps to `Statusline position`

#### Scenario: Repeat moves [auto]
- GIVEN MENU
- WHEN a Repeat event for `j` arrives
- THEN the selection moves like a Press

### Requirement: AI task list
The AI section MUST show, below the Settings rows, the section title `AI`, exactly six task rows in this order: `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, and then the `Profile` row. Each task row shows its label and its value (see ai-task-models): `<engine> › <model>`, `(default)` for an unassigned non-general task, `(not set)` for an unset `general` or an unset aggregate, `(mixed)` for the aggregate when the tasks differ, and the marks `· download required` and `(not detected)` as defined there. The `Profile` row shows the profile id. The number of AI rows MUST never change because of a detection result. The popup MUST show every row of both sections without clipping on a normal terminal, MUST keep the same size between the task list and the picker, and the minimum terminal size below which only the one-line fallback is drawn (see Tiny terminals) MUST be adjusted to the popup; the exact dimensions are a design decision.
(Previously: exactly six rows; the popup was 62x15 with the AI section and 51 columns wide without it)

#### Scenario: Task rows rendered [auto, TestBackend + insta]
- GIVEN general is `claude` / `sonnet`, code is `ollama` / `llama3:8b` (ollama available with that model), the other tasks unassigned, the profile `analyst`
- WHEN the menu is rendered on a normal terminal
- THEN `AI` and the rows are visible: `Set all tasks` `(mixed)`, `general` `claude › sonnet`, `code` `ollama › llama3:8b`, `chat` `(default)`, `review` `(default)`, `summarize` `(default)`, `Profile` `analyst`

#### Scenario: Nothing assigned [auto, TestBackend + insta]
- GIVEN no task is assigned
- WHEN the menu is rendered
- THEN `Set all tasks` reads `(not set)`, `general` reads `(not set)`, the other four read `(default)` and `Profile` reads `programmer` (the default profile)

#### Scenario: Marks on rows [auto]
- GIVEN code is `ollama` / `old:7b` not downloaded, and review is `codex` / `default` with codex `not installed`
- WHEN the menu is rendered
- THEN the rows read `ollama › old:7b · download required` and `codex (not detected) › default`

#### Scenario: Popup size is stable [auto, TestBackend + insta]
- GIVEN the task list and then the picker open on a normal terminal
- WHEN each is rendered
- THEN the popup has the same size in both and every row of both sections is visible in the task list

#### Scenario: Fallback threshold follows the popup [auto]
- GIVEN a terminal one column or one row smaller than the popup minimum, and one exactly at it
- WHEN rendered
- THEN the first shows the one-line fallback and the second shows the full popup without panic

## ADDED Requirements

### Requirement: Profile row
The `Profile` row is a static item in the AI section. On it, `h`, `l`, Left, Right, Space and Enter MUST cycle the draft value through `analyst` and `programmer` with wrap (any of these keys toggles; `l`/Right/Space/Enter advance and `h`/Left go back), and Repeat is handled like Press except on Enter, as for Settings rows. The footer on the `Profile` row MUST be the Settings-row hint `j/k move · h/l change · s save · Esc cancel`; detection cues and the `r` refresh key MUST NOT apply to it (`r` has no effect there, as on a Settings row). Changing the draft MUST have no live effect on layout, MUST NOT emit ResizePty, WritePty or a detection run, and MUST NOT change any open conversation. Esc at the top level MUST restore the profile to the value captured at open; `s` saves it through config-persistence only when it differs.

#### Scenario: Cycle [auto]
- GIVEN `Profile` selected with the draft `analyst`
- WHEN `l` is pressed, then `h`
- THEN the draft is `programmer`, then `analyst`

#### Scenario: Enter cycles and does not save [auto]
- GIVEN `Profile` selected with the draft `analyst`
- WHEN Enter is pressed
- THEN the draft is `programmer`, the menu stays open and no save is requested

#### Scenario: Repeat of Enter ignored [auto]
- GIVEN `Profile` selected
- WHEN a Repeat event for Enter arrives
- THEN the draft is unchanged

#### Scenario: Footer on the Profile row [auto, TestBackend + insta]
- GIVEN `Profile` selected and detection still in flight
- WHEN rendered
- THEN the footer reads `j/k move · h/l change · s save · Esc cancel`

#### Scenario: Refresh is not triggered [auto]
- GIVEN `Profile` selected
- WHEN `r` is pressed
- THEN no detection run is requested

#### Scenario: No side effects on change [auto]
- GIVEN `Profile` selected
- WHEN its value is cycled
- THEN no ResizePty, WritePty or file access happens and no chat state changes

#### Scenario: Esc reverts [auto]
- GIVEN the menu opened with `analyst` and the draft changed to `programmer`
- WHEN Esc is pressed on the task list
- THEN the current profile is `analyst` and no file is read or written

#### Scenario: Save [auto]
- GIVEN the menu opened with `analyst` and the draft changed to `programmer`
- WHEN `s` is pressed and the save succeeds
- THEN the menu closes and the current profile is `programmer`

#### Scenario: Unchanged profile does not touch the file [auto]
- GIVEN the profile draft equals the open value and nothing else changed
- WHEN `s` is pressed
- THEN the menu closes and the file is not read or written
