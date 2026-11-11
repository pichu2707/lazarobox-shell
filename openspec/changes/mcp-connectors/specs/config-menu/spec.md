# Config Menu Specification (delta: MCP connectors)

## Base of this delta

This delta targets the owning spec `openspec/specs/config-menu/spec.md` as it stands on f7d6b6f4. The old `AI` tab no longer exists: ai-menu-tabs (archived 2026-11-02) split it into the menu tabs `Settings`, `Models`, `Tools`, `Connectors`, `Chat`, and reserved `Connectors` for MCP ("it has none today, so it MUST NOT be drawn ... and MUST appear, in its table position between `Tools` and `Chat`, as soon as a row is declared for it (MCP, agent-tools 4a)", config-menu spec:760; `sections: &[]`, src/core/menu.rs:772-778). `Hugging Face` is now the last row of the Models tab. So the row this change adds goes to the `Connectors` tab (assumption: the reserved tab; the earlier plan put it after `Hugging Face` in the `AI` tab), and the tab stops being hidden. The MODIFIED blocks are full copies of "Sections and items are data" (config-menu spec:44-65), "Menu tabs" (:759-805) and "Jump keys" (:1625-1661) where only the sentences and scenarios that assumed an empty `Connectors` tab change; a scenario that needs Connectors hidden now runs on a test table, as the code already does (`TEST_TABS`, src/core/menu.rs:789-802). The other requirements stay as they are; their scenarios that say "Connectors hidden" run on such a test table. The rule that the menu never writes `ai.mcp.*` stays (config-persistence, openspec/specs/config-persistence/spec.md:494; C4).

Traceability: C1, C2 (a menu wizard), C4 (the menu writes no `ai.mcp.*`), C6 as amended on 2026-11-11 (the menu writes `secrets.toml` only through a confirmed secret field of the area, see mcp-connect, Secret fields, and mcp-secrets, Menu save; until that date C6 read "the menu writes no `secrets.toml`"), proposal scope items 8 and 11. Slice tags: [S4], [S6a2].

## ADDED Requirements

### Requirement: Connectors row
The `Connectors` menu tab MUST hold one static row, `Connectors` (no key path), as its only row (see Menu tabs). Its value is the number of catalog entries that are declared and enabled in `config.toml` (`<N> declared`), or `none` when there are none (see mcp-connect, Entry list). Enter on the row opens the Connectors area (see mcp-connect); Esc there comes back to the row. The row is not a draft value: it writes no config edit, `s` saves only the other rows' drafts, and the row's value never changes on Esc of the menu. [S6a2] A secret-field save of the area is not part of the menu draft either: it is written when the user confirms it in the field (mcp-secrets, Menu save), `s` neither writes nor repeats it, Esc of the menu does not revert it, and the unsaved-changes hint does not count it. While a secret field is open, every key goes to the field (no tab switch, jump key or `s`), and Esc closes the field first. The area's screens MUST be drawn inside the popup body, whose height is computed (`view_body()`, at least 13 rows today; never hard-code a count), and MUST NOT change the popup height (see Popup height is fixed); a screen longer than the body scrolls (assumption).

#### Scenario: The Connectors tab [auto, TestBackend + insta]
- GIVEN the AI section attached and no declared catalog server
- WHEN `4` is pressed and the tab is rendered
- THEN the bar reads ` Settings │ Models │ Tools │ Connectors │ Chat `, and the tab shows one row, `Connectors`, with the value `none`

#### Scenario: Count of declared entries [auto]
- GIVEN declared `clarity` and `ga4`, an unrelated declared server `fs`, and `gsc` declared with `enabled = false`
- WHEN the row is built
- THEN its value is `2 declared`

#### Scenario: Opens the area [auto]
- GIVEN the `Connectors` row selected
- WHEN Enter is pressed, then Esc
- THEN the Connectors area opens, and Esc returns to the row with the menu draft unchanged

#### Scenario: No config edit [auto]
- GIVEN a file with `[[ai.mcp.servers]]` entries and a changed position draft
- WHEN the Connectors area is visited and `s` is pressed
- THEN only the position changes and every `ai.mcp.*` byte is identical

#### Scenario: A secret save is not a draft [auto]
- GIVEN the area with `CLARITY_API_TOKEN` saved through its field, and no other change
- WHEN the area and the menu are closed with Esc
- THEN the menu reports no unsaved change, no config edit is emitted, and the saved value stays in `secrets.toml`

#### Scenario: Keys go to the open field [auto]
- GIVEN a secret field open on the `clarity` entry
- WHEN `4`, Tab, `s` and `q` are pressed
- THEN they are typed into the field as text (shown as `•`), no tab changes and no save is requested

#### Scenario: The popup keeps its size [auto, TestBackend + insta]
- GIVEN a normal terminal
- WHEN the Settings tab, the Connectors tab, the area's entry list, the `google` entry screen, its field list with a field open, and its link waiting screen are rendered
- THEN the popup has the same height in all six

## MODIFIED Requirements

### Requirement: Sections and items are data
Sections, items and tabs MUST come from static descriptor tables; adding a section, a tab or a setting MUST NOT require changing the popup drawing or navigation code. Each setting item MUST declare its label and its `config.toml` key path where one exists, and either an ordered list of allowed values (static item) or a task kind (assignment item, whose value is an `engine › model` chosen in the picker). Each row MUST belong to exactly one tab, as in the Tab map: `Settings` (`Statusline position` `statusline.position`: top, bottom; `Tab bar position` `tabbar.position`: top, bottom; `Mouse` `mouse`: on, off, written as the booleans `true` and `false`; `Notify`: auto, desktop, bell, off; `Sound`: off, on; `llama-server idle` `ai.servers.llamacpp.idle_minutes`: 5 min, 15 min, 30 min, never, written as a TOML integer, never = 0; `Updates`: the running version, or the newer one when known; `Feedback`: `bug report or idea`, no key path), `Models` (`Set all tasks` aggregate over the five tasks, no single key path; `general` `ai`; `code` `ai.tasks.code`; `chat` `ai.tasks.chat`; `review` `ai.tasks.review`; `summarize` `ai.tasks.summarize`; `orchestrate` its task table; `Hugging Face`, no key path), `Tools` (`Tools` `ai.tools.enabled`: off, on; `Search`, `Write`, `Exec`, `Git`, `Git write`, `Issue draft` (on by default): off, on, each with its own `ai.tools.<key>`; `Approval` `ai.tools.approval`: ask, auto, run; `Orchestrator` `ai.orchestrator.enabled`: off, on), `Connectors` (`Connectors`, no key path; see Connectors row) and `Chat` (`Profile` `ai.profile`: analyst, programmer; `Context` `ai.context`: off, on; `Memory` `ai.memory.enabled`: off, on). `Notify`, `Sound` and `llama-server idle` edit the AI draft, so they MUST NOT be shown (and MUST NOT be selectable) when the AI section is not attached; then `Settings` holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and no other tab exists. Moving a row between tabs MUST NOT change its key path, its values or the config edit it produces.

#### Scenario: Descriptor integrity [auto]
- GIVEN the descriptor tables
- WHEN they are validated
- THEN every tab has a non-empty label, every item a non-empty label, every static item with a key path has at least two allowed values, `Mouse` has the key path `mouse` and the values `on`, `off`, the task rows have the task ids of ai-task-models and a key path, `Set all tasks` has no single key path, `Profile`, `Context`, `Memory`, `Tools`, `Approval` and the five family rows have their key paths and values above, every tab lists only rows that exist, every row appears on exactly one tab, and there are no duplicate key paths

#### Scenario: Settings rows [auto, TestBackend + insta]
- GIVEN the menu just opened with the AI section attached and `mouse` at its default
- WHEN rendered on a normal terminal
- THEN, with no `Settings` title row (the bar names the tab), the Settings tab shows `Statusline position`, `Tab bar position`, `Mouse` (`on`, not muted), `Notify`, `Sound`, `llama-server idle` (`15 min`), `Usage (alpha)`, `Updates` and `Feedback`, each with its current value, and no Models, Tools, Connectors or Chat row is drawn

#### Scenario: Every row is on its tab [auto, TestBackend + insta]
- GIVEN the AI section attached
- WHEN each of Models, Tools, Connectors and Chat is rendered
- THEN Models reads `Set all tasks`, `general`, `code`, `chat`, `review`, `summarize`, `orchestrate`, `Hugging Face`; Tools reads `Tools`, `Search`, `Write`, `Exec`, `Git`, `Git write`, `Issue draft`, `Approval`, `Orchestrator`; Connectors reads `Connectors`; Chat reads `Profile`, `Context`, `Memory`; and no `Notify`, `Sound`, `Updates` or `Feedback` row exists on them

#### Scenario: No AI keeps one tab [auto]
- GIVEN the menu without the AI section
- WHEN the rows are built
- THEN the Settings tab holds `Statusline position`, `Tab bar position`, `Mouse`, `Updates` and `Feedback`, and there is no other tab

### Requirement: Menu tabs
The popup MUST group its rows into menu tabs. The tabs MUST come from a static table (label, the rows it shows, whether it needs the AI section, whether it is hidden while it has no rows); adding a tab MUST NOT require changing navigation or drawing code. The table order is `Settings`, `Models`, `Tools`, `Connectors`, `Chat`. Without the AI section only `Settings` exists. With it, `Models`, `Tools` and `Chat` exist and `Connectors` is visible only while it has at least one row. It holds the `Connectors` row (see Connectors row), so with the AI section it MUST be drawn in its table position between `Tools` and `Chat` and MUST be reached by Tab and Shift+Tab; a tab table whose `Connectors` tab has no row (a test table) MUST NOT draw it and MUST NOT reach it by Tab or Shift+Tab. `Tab` MUST activate the next visible tab; `Tab` with SHIFT and `BackTab` (with or without SHIFT) MUST activate the previous one. Wrap is across the visible tabs (Settings, Models, Tools, Connectors, Chat, Settings; Settings, Models, Tools, Chat, Settings while Connectors is hidden). Ctrl+Tab and Alt+Tab MUST stay swallowed. With one visible tab, Tab and Shift+Tab change nothing and show the footer notice `No other tabs` (cleared by the next key press, like the jump-key notice). A held key repeats like `j`/`k`: Repeat events switch tabs like Press. The cursor MUST be remembered per tab, keyed by the tab and not by its place among the visible ones: leaving a tab and coming back MUST restore the row that was selected there, and a tab becoming visible MUST NOT move the memory of another. Each tab with at least one row starts on its first row. Switching tabs, by Tab or by a jump key, MUST NOT change any draft value and MUST NOT emit `ResizePty`, `WritePty` or a detection run.

#### Scenario: Tab cycles forward with wrap [auto]
- GIVEN the AI section attached and the active tab `Settings`
- WHEN Tab is pressed five times
- THEN the active tab is `Models`, `Tools`, `Connectors`, `Chat`, then `Settings`

#### Scenario: Shift+Tab, both encodings [auto]
- GIVEN the active tab `Settings`
- WHEN `BackTab` is pressed, in a separate run `BackTab` with SHIFT, and in another `Tab` with SHIFT
- THEN in all runs the active tab is `Chat`; and pressing the same key again gives `Connectors`

#### Scenario: Cursor is remembered per tab [auto]
- GIVEN the `Mouse` row selected on `Settings`, then Tab to `Models`, `j` pressed twice (on `code`), then `3` to Tools and `j` once (on `Search`)
- WHEN `1`, `2` and `3` are pressed in turn
- THEN `Mouse` is selected on Settings, `code` on Models and `Search` on Tools

#### Scenario: Modified Tab and repeat [auto]
- GIVEN the active tab `Settings`
- WHEN Ctrl+Tab and Alt+Tab are pressed, then a Repeat event for Tab arrives, then a Repeat for `BackTab`
- THEN the first two change nothing, the Repeat of Tab moves to `Models`, and the Repeat of `BackTab` returns to `Settings`

#### Scenario: Switching tabs changes nothing else [auto]
- GIVEN a changed draft and the menu open
- WHEN Tab, Shift+Tab and `3` are pressed
- THEN the draft is unchanged and no ResizePty, WritePty, Refresh or Save is emitted

#### Scenario: Connectors is hidden without rows [auto]
- GIVEN a test table whose Connectors tab has no row, and the AI section attached
- WHEN the tabs are listed, and Tab is pressed four times from `Settings`
- THEN they are exactly `Settings`, `Models`, `Tools`, `Chat`, in that order, and the active tab is `Models`, `Tools`, `Chat`, then `Settings` (Connectors is skipped)

#### Scenario: Connectors appears with its first row [auto]
- GIVEN the tab table (its Connectors tab holds the `Connectors` row), and the AI section attached
- WHEN the tabs are listed, and Tab is pressed from `Tools`, then again
- THEN they are `Settings`, `Models`, `Tools`, `Connectors`, `Chat`, and the active tab is `Connectors`, then `Chat`

#### Scenario: A tab appearing keeps the cursors [auto]
- GIVEN `Chat` with `Memory` selected and Connectors hidden (a test table whose Connectors tab has no row)
- WHEN a row is declared for Connectors and the menu is rebuilt with the same memory, then `5` is pressed
- THEN `Chat` is active with `Memory` still selected

#### Scenario: No AI, one tab [auto]
- GIVEN the menu without the AI section
- WHEN Tab and Shift+Tab are pressed
- THEN the tab does not change, the footer shows `No other tabs` and nothing else happens

### Requirement: Jump keys
At the top level, an unmodified `1` to `5` MUST jump to a tab by its FIXED position in the tab table: `1` Settings, `2` Models, `3` Tools, `4` Connectors, `5` Chat. The digits do not renumber when Connectors is hidden. Press and Repeat act alike. A jump to the visible tab that is not active MUST activate it with the same cursor memory as Tab. A jump to the active tab MUST change nothing and show the footer notice `Already on <Tab>` (no key is swallowed silently). The `Already on` notice and the other notices follow the rules below. A jump to a tab that does not exist MUST show a visible one-line notice that names the reason and MUST change nothing else: `4` while Connectors has no rows (the notice says there are no connectors yet), and `2` to `5` without the AI section (the notice reads `No AI section`). The notice is drawn in the footer with the precedence of an error, is cleared by the next key press (which then acts normally), blocks neither `s` nor Esc, and writes nothing to any PTY. `0` and `6` to `9` (unmodified, top level) show the notice `Tabs are 1-5` and change nothing else. Digits with Ctrl, Alt or Shift stay swallowed and are not jumps; this is a known exception to the notice rule, the same as every other modified key today. No unmodified digit is ever swallowed without an effect or a notice at the top level.

#### Scenario: Jump to each tab [auto]
- GIVEN the AI section attached and a test table with Connectors visible
- WHEN `1`, `2`, `3`, `4`, `5` are pressed in turn
- THEN the active tab is Settings, Models, Tools, Connectors, Chat

#### Scenario: Digits keep their positions [auto]
- GIVEN the AI section attached and Connectors hidden (a test table whose Connectors tab has no row)
- WHEN `5` is pressed
- THEN the active tab is `Chat`, not `Tools` or a fourth tab

#### Scenario: Hidden Connectors shows a notice [auto, TestBackend + insta]
- GIVEN Connectors hidden (a test table whose Connectors tab has no row) and the active tab `Models` with `code` selected
- WHEN `4` is pressed
- THEN the active tab is still `Models` with `code` selected, the footer shows a notice about the missing connectors instead of the hint, and the next key press clears it

#### Scenario: No AI section shows a notice [auto]
- GIVEN the menu without the AI section
- WHEN `2`, then `3`, then `4`, then `5` are pressed
- THEN each shows the notice `No AI section`, the tab does not change and nothing is written

#### Scenario: Active tab and Settings [auto]
- GIVEN the active tab `Models`
- WHEN `2` is pressed, then `1`
- THEN the first changes nothing and shows `Already on Models`, and the second activates `Settings`

#### Scenario: The notice does not block [auto]
- GIVEN Connectors hidden (a test table whose Connectors tab has no row), the notice from `4` showing and a draft changed
- WHEN `s` is pressed, and in another run Esc
- THEN the first requests a save and the second reverts and closes

#### Scenario: Jump keys in Kitty [manual, Kitty]
- GIVEN the menu in Kitty with the AI section
- WHEN Tab, Shift+Tab and `1` to `5` are pressed
- THEN the tabs switch as specified, `4` activates `Connectors`, and nvim receives no keys
